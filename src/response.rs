use axum::{
    body::Body,
    response::{IntoResponse, Response},
};
use http::StatusCode;
use serde::Serialize;
use serde_json::{Map, Value};
use std::{future::Future, pin::Pin, sync::Arc};

/// Errors raised while building an Inertia page or serializing its props.
#[derive(Debug, thiserror::Error)]
pub enum PropsError {
    /// The props could not be converted to JSON.
    #[error("could not serialize Inertia props: {0}")]
    Serialize(#[from] serde_json::Error),
    /// Props must serialize to a JSON object at the top level.
    #[error("Inertia props must serialize to a JSON object")]
    NotObject,
    /// The component name was empty or whitespace-only.
    #[error("Inertia component name must not be empty")]
    EmptyComponent,
    /// A deferred prop could not be produced.
    #[error("deferred Inertia prop failed")]
    Deferred(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Namespace for constructing an Inertia page response.
#[derive(Debug)]
pub struct Inertia;

impl Inertia {
    /// Start building a page for the named frontend component.
    pub fn render(component: impl Into<String>) -> PageBuilder {
        PageBuilder {
            component: component.into(),
        }
    }
}

/// Builder for an Inertia page's component and eager props.
#[derive(Debug)]
pub struct PageBuilder {
    component: String,
}

impl PageBuilder {
    /// Serialize and attach props to the page. Props must serialize to an object.
    ///
    /// Page-specific values are merged over shared props configured on
    /// [`InertiaConfig`](crate::InertiaConfig).
    ///
    /// # Errors
    ///
    /// Returns [`PropsError::EmptyComponent`] for a blank component name,
    /// [`PropsError::NotObject`] for non-object props, or
    /// [`PropsError::Serialize`] if serialization fails.
    pub fn props<T: Serialize>(self, props: T) -> Result<InertiaResponse, PropsError> {
        if self.component.trim().is_empty() {
            return Err(PropsError::EmptyComponent);
        }
        let value = serde_json::to_value(props)?;
        let Value::Object(props) = value else {
            return Err(PropsError::NotObject);
        };
        Ok(InertiaResponse {
            component: self.component,
            props,
            always_props: Vec::new(),
            deferred: Vec::new(),
            scroll_resolvers: Vec::new(),
            merge_props: Vec::new(),
            prepend_props: Vec::new(),
            deep_merge_props: Vec::new(),
            match_props_on: Vec::new(),
            once_props: Vec::new(),
            scroll_props: Map::new(),
            flash: Map::new(),
            clear_history: false,
            encrypt_history: false,
        })
    }
}

type DeferredFuture = Pin<Box<dyn Future<Output = Result<Value, PropsError>> + Send>>;
type DeferredResolver = Arc<dyn Fn() -> DeferredFuture + Send + Sync>;
pub(crate) type ScrollFuture =
    Pin<Box<dyn Future<Output = Result<(Value, Value), PropsError>> + Send>>;
pub(crate) type ScrollResolver = Arc<dyn Fn() -> ScrollFuture + Send + Sync>;

#[derive(Clone)]
pub(crate) struct DeferredProp {
    pub(crate) key: String,
    pub(crate) group: Option<String>,
    pub(crate) optional: bool,
    pub(crate) resolve: DeferredResolver,
}

#[derive(Clone)]
pub(crate) struct ScrollProp {
    pub(crate) key: String,
    pub(crate) resolve: ScrollResolver,
}

#[derive(Clone, Debug)]
pub(crate) struct OnceProp {
    pub(crate) prop: String,
    pub(crate) key: String,
    pub(crate) expires_at: Option<u64>,
    pub(crate) fresh: bool,
}

/// Request-independent response marker consumed by [`crate::InertiaLayer`].
///
/// Applications get this from [`PageBuilder::props`] and may add lazy or merge
/// metadata through its builder methods. Request-specific data is added later
/// by the middleware.
#[derive(Clone)]
#[must_use = "return the configured response from the handler"]
pub struct InertiaResponse {
    pub(crate) component: String,
    pub(crate) props: Map<String, Value>,
    pub(crate) always_props: Vec<String>,
    pub(crate) deferred: Vec<DeferredProp>,
    pub(crate) scroll_resolvers: Vec<ScrollProp>,
    pub(crate) merge_props: Vec<String>,
    pub(crate) prepend_props: Vec<String>,
    pub(crate) deep_merge_props: Vec<String>,
    pub(crate) match_props_on: Vec<String>,
    pub(crate) once_props: Vec<OnceProp>,
    pub(crate) scroll_props: Map<String, Value>,
    pub(crate) flash: Map<String, Value>,
    pub(crate) clear_history: bool,
    pub(crate) encrypt_history: bool,
}

impl std::fmt::Debug for InertiaResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InertiaResponse")
            .field("component", &self.component)
            .field("prop_keys", &self.props.keys().collect::<Vec<_>>())
            .field("always_prop_count", &self.always_props.len())
            .field(
                "deferred_keys",
                &self
                    .deferred
                    .iter()
                    .map(|prop| &prop.key)
                    .collect::<Vec<_>>(),
            )
            .field("merge_props", &self.merge_props)
            .field("prepend_props", &self.prepend_props)
            .field("deep_merge_props", &self.deep_merge_props)
            .field("match_props_on", &self.match_props_on)
            .field("once_props", &self.once_props)
            .field("scroll_props", &self.scroll_props)
            .field("flash_keys", &self.flash.keys().collect::<Vec<_>>())
            .field("clear_history", &self.clear_history)
            .field("encrypt_history", &self.encrypt_history)
            .field("scroll_resolver_count", &self.scroll_resolvers.len())
            .finish_non_exhaustive()
    }
}

impl InertiaResponse {
    /// Add a prop that is included even when a partial reload omits its key.
    ///
    /// # Errors
    ///
    /// Returns an error if the value cannot be serialized.
    pub fn always<T: Serialize>(
        mut self,
        key: impl Into<String>,
        value: T,
    ) -> Result<Self, PropsError> {
        let key = key.into();
        self.props.insert(key.clone(), serde_json::to_value(value)?);
        self.always_props.push(key);
        Ok(self)
    }

    /// Register a prop evaluated only when requested by a partial reload.
    ///
    /// The resolver must be repeatable because response values implement
    /// `Clone` for compatibility with Axum response extensions.
    pub fn optional<F, Fut, T, E>(mut self, key: impl Into<String>, resolve: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
        E: std::error::Error + Send + Sync + 'static,
        T: Serialize + 'static,
    {
        self.deferred
            .push(deferred_prop(key.into(), None, true, resolve));
        self
    }

    /// Register a lazily evaluated prop included on full visits and when selected in partial reloads.
    pub fn lazy<F, Fut, T, E>(mut self, key: impl Into<String>, resolve: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
        E: std::error::Error + Send + Sync + 'static,
        T: Serialize + 'static,
    {
        self.deferred
            .push(deferred_prop(key.into(), None, false, resolve));
        self
    }

    /// Register a prop to load after the initial page render.
    ///
    /// Deferred props in the same group are requested together by Inertia.
    pub fn deferred<F, Fut, T, E>(
        mut self,
        key: impl Into<String>,
        group: impl Into<String>,
        resolve: F,
    ) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
        E: std::error::Error + Send + Sync + 'static,
        T: Serialize + 'static,
    {
        self.deferred.push(deferred_prop(
            key.into(),
            Some(group.into()),
            false,
            resolve,
        ));
        self
    }

    /// Mark a prop path to append during partial reloads.
    pub fn merge(mut self, path: impl Into<String>) -> Self {
        self.merge_props.push(path.into());
        self
    }

    /// Mark a prop path to append during partial reloads.
    ///
    /// This is an explicit alias for [`Self::merge`].
    pub fn append(self, path: impl Into<String>) -> Self {
        self.merge(path)
    }

    /// Mark a prop path to prepend during partial reloads.
    pub fn prepend(mut self, path: impl Into<String>) -> Self {
        let path = path.into();
        self.merge_props.push(path.clone());
        self.prepend_props.push(path);
        self
    }

    /// Mark a prop path for a recursive merge on partial reloads.
    pub fn deep_merge(mut self, path: impl Into<String>) -> Self {
        self.deep_merge_props.push(path.into());
        self
    }

    /// Match incoming merged items using the given full prop path, such as
    /// `users.data.id`.
    pub fn match_on(mut self, path: impl Into<String>) -> Self {
        self.match_props_on.push(path.into());
        self
    }

    /// Mark a prop as client-cached across visits. The prop path is its cache
    /// key by default.
    pub fn once(mut self, prop: impl Into<String>) -> Self {
        let prop = prop.into();
        self.once_props.push(OnceProp {
            key: prop.clone(),
            prop,
            expires_at: None,
            fresh: false,
        });
        self
    }

    /// Mark a prop as client-cached under a stable key shared by other props.
    pub fn once_as(mut self, prop: impl Into<String>, key: impl Into<String>) -> Self {
        self.once_props.push(OnceProp {
            prop: prop.into(),
            key: key.into(),
            expires_at: None,
            fresh: false,
        });
        self
    }

    /// Mark a prop as client-cached until the given Unix timestamp in
    /// milliseconds.
    pub fn once_until(mut self, prop: impl Into<String>, expires_at: u64) -> Self {
        let prop = prop.into();
        self.once_props.push(OnceProp {
            key: prop.clone(),
            prop,
            expires_at: Some(expires_at),
            fresh: false,
        });
        self
    }

    /// Mark a prop as client-cached but always resolve it on this response.
    pub fn once_fresh(mut self, prop: impl Into<String>) -> Self {
        let prop = prop.into();
        self.once_props.push(OnceProp {
            key: prop.clone(),
            prop,
            expires_at: None,
            fresh: true,
        });
        self
    }

    /// Add one-time response data exposed as `page.flash`.
    ///
    /// This adds data to this response only. Persisting flash data across a
    /// redirect requires application session integration.
    ///
    /// # Errors
    ///
    /// Returns an error if the value cannot be serialized.
    pub fn flash<T: Serialize>(
        mut self,
        key: impl Into<String>,
        value: T,
    ) -> Result<Self, PropsError> {
        self.flash.insert(key.into(), serde_json::to_value(value)?);
        Ok(self)
    }

    /// Add an object of one-time response data exposed as `page.flash`.
    ///
    /// # Errors
    ///
    /// Returns an error if the value cannot be serialized or is not an object.
    pub fn flash_data<T: Serialize>(mut self, data: T) -> Result<Self, PropsError> {
        let value = serde_json::to_value(data)?;
        let Value::Object(data) = value else {
            return Err(PropsError::NotObject);
        };
        self.flash.extend(data);
        Ok(self)
    }

    /// Ask the Inertia client to clear its browser history when processing this page.
    pub fn clear_history(mut self) -> Self {
        self.clear_history = true;
        self
    }

    /// Ask the Inertia client to encrypt this page in browser history.
    /// Encryption is performed by the client and requires a secure context.
    pub fn encrypt_history(mut self) -> Self {
        self.encrypt_history = true;
        self
    }
}

fn deferred_prop<F, Fut, T, E>(
    key: String,
    group: Option<String>,
    optional: bool,
    resolve: F,
) -> DeferredProp
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<T, E>> + Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
    T: Serialize + 'static,
{
    DeferredProp {
        key,
        group,
        optional,
        resolve: Arc::new(move || {
            let future = resolve();
            Box::pin(async move {
                let value = future
                    .await
                    .map_err(|err| PropsError::Deferred(Box::new(err)))?;
                Ok(serde_json::to_value(value)?)
            })
        }),
    }
}

impl IntoResponse for InertiaResponse {
    fn into_response(self) -> Response {
        let mut response = (StatusCode::OK, Body::empty()).into_response();
        response.extensions_mut().insert(self);
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[test]
    fn page_builder_rejects_blank_component_and_non_object_props() {
        assert!(matches!(
            Inertia::render("  ").props(serde_json::json!({})),
            Err(PropsError::EmptyComponent)
        ));
        assert!(matches!(
            Inertia::render("Users").props(vec![1, 2]),
            Err(PropsError::NotObject)
        ));
    }

    #[test]
    fn page_builder_serializes_typed_props() {
        #[derive(Serialize)]
        struct Props {
            name: &'static str,
        }

        let response = Inertia::render("Users")
            .props(Props { name: "Ada" })
            .unwrap();
        assert_eq!(response.props["name"], "Ada");
    }

    #[test]
    fn merge_and_prepend_register_protocol_paths() {
        let response = Inertia::render("Users")
            .props(serde_json::json!({}))
            .unwrap()
            .merge("items")
            .prepend("older");

        assert_eq!(response.merge_props, ["items", "older"]);
        assert_eq!(response.prepend_props, ["older"]);
    }
}
