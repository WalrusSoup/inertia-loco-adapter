use crate::{Page, PropsError, SsrConfig, ViteDevConfig};
use serde::Serialize;
use serde_json::{Map, Value};
use std::sync::Arc;

/// Data passed to the configured application root view renderer.
///
/// `inertia_root` contains the mount element and escaped page payload.
/// `inertia_head` contains trusted fragments from the configured SSR service.
#[derive(Clone, serde::Serialize)]
pub struct RootViewData {
    /// Complete page payload for templates that need page metadata.
    pub page: Page,
    /// DOM id used by the frontend Inertia client.
    pub mount_id: String,
    /// Application-provided asset markup for the document head.
    pub asset_tags: String,
    /// Ready-to-insert mount element, including escaped `data-page` JSON.
    pub inertia_root: String,
    /// Trusted head fragments returned by the configured SSR service.
    pub inertia_head: String,
}

impl std::fmt::Debug for RootViewData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RootViewData")
            .field("page", &self.page)
            .field("mount_id", &self.mount_id)
            .field("asset_tags_configured", &!self.asset_tags.is_empty())
            .field("inertia_root_configured", &!self.inertia_root.is_empty())
            .field("inertia_head_configured", &!self.inertia_head.is_empty())
            .finish()
    }
}

pub(crate) type RootViewRenderer = dyn Fn(&RootViewData) -> Result<String, String> + Send + Sync;

/// Configuration for request-aware Inertia response finalization.
///
/// Page props override values in [`shared_props`](Self::shared_props). If
/// `root_view` is not registered, the adapter uses a minimal HTML shell. The
/// mount element id defaults to `app`.
#[derive(Clone)]
pub struct InertiaConfig {
    /// Current frontend asset version. A mismatch on an Inertia GET triggers a
    /// full page reload response.
    pub version: Option<String>,
    /// DOM id used for the Inertia mount element. Defaults to `app`.
    pub mount_id: String,
    /// Trusted asset tags inserted into the root view context.
    pub asset_tags: String,
    /// Optional Vite development server integration. When its hot file points
    /// to a live Vite server, Vite client and entry tags are used instead of
    /// `asset_tags`; React Fast Refresh is opt-in on the Vite configuration.
    pub vite_dev_server: Option<ViteDevConfig>,
    /// Optional server-side rendering service configuration.
    pub ssr: Option<SsrConfig>,
    /// Application-wide props included in every page unless partially omitted.
    pub shared_props: Map<String, Value>,
    pub(crate) root_view: Option<Arc<RootViewRenderer>>,
}

impl std::fmt::Debug for InertiaConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InertiaConfig")
            .field("version", &self.version)
            .field("mount_id", &self.mount_id)
            .field("asset_tags_configured", &!self.asset_tags.is_empty())
            .field("vite_dev_server", &self.vite_dev_server)
            .field("ssr", &self.ssr)
            .field(
                "shared_prop_keys",
                &self.shared_props.keys().collect::<Vec<_>>(),
            )
            .field("root_view_configured", &self.root_view.is_some())
            .finish()
    }
}

impl Default for InertiaConfig {
    fn default() -> Self {
        Self {
            version: None,
            mount_id: "app".into(),
            asset_tags: String::new(),
            vite_dev_server: None,
            ssr: None,
            shared_props: Map::new(),
            root_view: None,
        }
    }
}

impl InertiaConfig {
    /// Add application-wide props.
    ///
    /// Page props with the same key take precedence over shared values.
    ///
    /// # Errors
    ///
    /// Returns an error if `props` cannot be serialized or does not serialize
    /// to a JSON object.
    pub fn shared<T: Serialize>(mut self, props: T) -> Result<Self, PropsError> {
        let value = serde_json::to_value(props)?;
        let Value::Object(map) = value else {
            return Err(PropsError::NotObject);
        };
        self.shared_props.extend(map);
        Ok(self)
    }

    /// Render browser visits with the application's root view. The renderer
    /// receives [`RootViewData`]; errors produce a 500 response.
    #[must_use]
    pub fn root_view<F>(mut self, render: F) -> Self
    where
        F: Fn(&RootViewData) -> Result<String, String> + Send + Sync + 'static,
    {
        self.root_view = Some(Arc::new(render));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_props_must_be_an_object() {
        assert!(matches!(
            InertiaConfig::default().shared(vec![1, 2]),
            Err(PropsError::NotObject)
        ));
    }

    #[test]
    fn page_shared_values_are_stored_by_key() {
        let config = InertiaConfig::default()
            .shared(serde_json::json!({ "app": "demo" }))
            .unwrap();
        assert_eq!(config.shared_props["app"], "demo");
    }
}
