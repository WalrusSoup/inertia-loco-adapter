use crate::{config::InertiaConfig, html, response::InertiaResponse, ssr, Page};
use axum::{
    body::Body,
    extract::Request,
    middleware::{from_fn_with_state, Next},
    response::{IntoResponse, Response},
    Router,
};
use http::{header, HeaderValue, StatusCode};
use serde_json::{Map, Value};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Clone)]
struct InertiaState {
    config: Arc<InertiaConfig>,
    ssr_client: Result<reqwest::Client, String>,
    ssr_status: Option<Arc<AtomicBool>>,
}

/// Axum layer that finalizes [`InertiaResponse`] values after handlers run.
#[derive(Clone)]
pub struct InertiaLayer(Arc<InertiaState>);

impl std::fmt::Debug for InertiaLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("InertiaLayer").field(&self.0.config).finish()
    }
}

impl InertiaLayer {
    /// Create the layer with request-independent adapter configuration.
    #[must_use]
    pub fn new(config: InertiaConfig) -> Self {
        let ssr_client = ssr::client().map_err(|err| err.to_string());
        let config = Arc::new(config);
        let ssr_status = config.ssr.as_ref().and_then(|ssr_config| {
            ssr_config.status_url.as_ref()?;

            match &ssr_client {
                Ok(client) => ssr::monitor(ssr_config, client),
                Err(_) => Some(Arc::new(AtomicBool::new(false))),
            }
        });
        Self(Arc::new(InertiaState {
            config,
            ssr_client,
            ssr_status,
        }))
    }
    /// Add Inertia finalization middleware to a router.
    ///
    /// The layer reads protocol headers, converts marker responses to JSON or
    /// HTML, applies version and redirect behavior, and optionally performs
    /// SSR for initial HTML visits.
    pub fn layer<S>(self, router: Router<S>) -> Router<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        router.layer(from_fn_with_state(self.0, finalize))
    }
}

async fn finalize(
    state: axum::extract::State<Arc<InertiaState>>,
    request: Request,
    next: Next,
) -> Response {
    let context = RequestContext::from_request(&request);
    let mut response = next.run(request).await;
    match response.extensions_mut().remove::<InertiaResponse>() {
        Some(marker) => finalize_page(response, marker, &state, context).await,
        None => finalize_passthrough(response, &context),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisitKind {
    Browser,
    Inertia,
}

struct RequestContext {
    visit: VisitKind,
    method: http::Method,
    url: String,
    incoming_version: Option<String>,
    partial_component: Option<String>,
    partial_data: Option<String>,
    partial_except: Option<String>,
    except_once_props: Option<String>,
    reset_props: Option<String>,
    scroll_prepend: bool,
}

impl RequestContext {
    fn from_request(request: &Request) -> Self {
        let headers = request.headers();
        let text_header = |name| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        Self {
            visit: if headers
                .get("x-inertia")
                .is_some_and(|value| value == "true")
            {
                VisitKind::Inertia
            } else {
                VisitKind::Browser
            },
            method: request.method().clone(),
            url: request.uri().to_string(),
            incoming_version: text_header("x-inertia-version"),
            partial_component: text_header("x-inertia-partial-component"),
            partial_data: text_header("x-inertia-partial-data"),
            partial_except: text_header("x-inertia-partial-except"),
            except_once_props: text_header("x-inertia-except-once-props"),
            reset_props: text_header("x-inertia-reset"),
            scroll_prepend: text_header("x-inertia-infinite-scroll-merge-intent")
                .is_some_and(|intent| intent == "prepend"),
        }
    }
}

fn finalize_passthrough(mut response: Response, context: &RequestContext) -> Response {
    if context.visit == VisitKind::Inertia {
        if matches!(
            context.method,
            http::Method::PUT | http::Method::PATCH | http::Method::DELETE
        ) && response.status() == StatusCode::FOUND
        {
            *response.status_mut() = StatusCode::SEE_OTHER;
        }
        add_vary(&mut response);
    }
    response
}

async fn finalize_page(
    response: Response,
    marker: InertiaResponse,
    state: &InertiaState,
    context: RequestContext,
) -> Response {
    if let Some(response) = version_conflict(&state.config, &context) {
        return response;
    }

    let page = match build_page(marker, state, &context).await {
        Ok(page) => page,
        Err(err) => {
            tracing::error!(error = %err, "failed to resolve Inertia page");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    match context.visit {
        VisitKind::Inertia => inertia_response(response, &page),
        VisitKind::Browser => browser_response(response, &page, state).await,
    }
}

async fn build_page(
    marker: InertiaResponse,
    state: &InertiaState,
    context: &RequestContext,
) -> Result<Page, crate::PropsError> {
    let partial_keys = if context.visit == VisitKind::Inertia
        && context.partial_component.as_deref() == Some(marker.component.as_str())
    {
        context
            .partial_data
            .as_deref()
            .map(|keys| keys.split(',').map(str::trim).map(str::to_owned).collect())
    } else {
        None
    };
    let except_keys = if context.visit == VisitKind::Inertia
        && context.partial_component.as_deref() == Some(marker.component.as_str())
    {
        context
            .partial_except
            .as_deref()
            .map(|keys| keys.split(',').map(str::trim).map(str::to_owned).collect())
    } else {
        None
    };
    let except_once_props = if context.visit == VisitKind::Inertia {
        context
            .except_once_props
            .as_deref()
            .map(|keys| keys.split(',').map(str::trim).map(str::to_owned).collect())
            .unwrap_or_default()
    } else {
        HashSet::new()
    };
    let prop_request = PropRequest {
        partial_keys,
        except_keys,
        except_once_props,
    };
    let mut resolved_props = resolve_props(
        state.config.shared_props.clone(),
        marker.props,
        marker.always_props,
        marker.deferred,
        marker.once_props,
        prop_request.clone(),
    )
    .await?;
    let (scroll_merge_props, resolved_scroll_props) = resolve_scroll_props(
        &mut resolved_props.props,
        marker.scroll_resolvers,
        prop_request.partial_keys.as_ref(),
        prop_request.except_keys.as_ref(),
    )
    .await?;
    let mut merge_paths = marker.merge_props;
    merge_paths.extend(scroll_merge_props);
    let mut scroll_props = marker.scroll_props;
    scroll_props.extend(resolved_scroll_props);
    let merge_metadata = finalize_merge_metadata(MergeMetadataRequest {
        merge_props: merge_paths,
        prepend_props: marker.prepend_props,
        deep_merge_props: marker.deep_merge_props,
        match_props_on: marker.match_props_on,
        scroll_props,
        reset_header: context
            .reset_props
            .as_deref()
            .unwrap_or_default()
            .to_owned(),
        scroll_prepend: context.scroll_prepend,
        partial_keys: prop_request.partial_keys,
        except_keys: prop_request.except_keys,
    });
    Ok(Page {
        component: marker.component,
        props: resolved_props.props,
        url: context.url.clone(),
        version: state.config.version.clone(),
        deferred_props: resolved_props.deferred_props,
        merge_props: merge_metadata.merge_props,
        prepend_props: merge_metadata.prepend_props,
        deep_merge_props: merge_metadata.deep_merge_props,
        match_props_on: merge_metadata.match_props_on,
        scroll_props: merge_metadata.scroll_props,
        once_props: resolved_props.once_props,
        flash: marker.flash,
        clear_history: marker.clear_history,
        encrypt_history: marker.encrypt_history,
    })
}

async fn resolve_scroll_props(
    props: &mut Map<String, Value>,
    scroll_props: Vec<crate::response::ScrollProp>,
    partial_keys: Option<&HashSet<String>>,
    except_keys: Option<&HashSet<String>>,
) -> Result<(Vec<String>, Map<String, Value>), crate::PropsError> {
    let is_partial = partial_keys.is_some() || except_keys.is_some();
    let mut merge_paths = Vec::new();
    let mut metadata = Map::new();
    for scroll in scroll_props {
        let selected = !is_partial
            || (partial_keys.is_none_or(|keys| keys.contains(&scroll.key))
                && !except_keys.is_some_and(|keys| keys.contains(&scroll.key)));
        if selected {
            let (value, scroll_metadata) = (scroll.resolve)().await?;
            props.insert(scroll.key.clone(), value);
            merge_paths.push(format!("{}.data", scroll.key));
            metadata.insert(scroll.key, scroll_metadata);
        }
    }
    Ok((merge_paths, metadata))
}

struct MergeMetadataRequest {
    merge_props: Vec<String>,
    prepend_props: Vec<String>,
    deep_merge_props: Vec<String>,
    match_props_on: Vec<String>,
    scroll_props: Map<String, Value>,
    reset_header: String,
    scroll_prepend: bool,
    partial_keys: Option<HashSet<String>>,
    except_keys: Option<HashSet<String>>,
}

struct MergeMetadata {
    merge_props: Vec<String>,
    prepend_props: Vec<String>,
    deep_merge_props: Vec<String>,
    match_props_on: Vec<String>,
    scroll_props: Map<String, Value>,
}

fn finalize_merge_metadata(request: MergeMetadataRequest) -> MergeMetadata {
    let reset_paths: Vec<_> = request
        .reset_header
        .split(',')
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .collect();
    let is_reset = |path: &str| reset_paths.iter().any(|reset| paths_overlap(path, reset));
    let selected = |path: &str| {
        request
            .partial_keys
            .as_ref()
            .is_none_or(|only| only.iter().any(|key| paths_overlap(path, key)))
            && !request.except_keys.as_ref().is_some_and(|except| {
                except
                    .iter()
                    .any(|key| path == key || path.starts_with(&format!("{key}.")))
            })
    };
    let merge_props: Vec<_> = request
        .merge_props
        .iter()
        .filter(|path| !is_reset(path) && selected(path))
        .cloned()
        .collect();
    let mut prepend_props: Vec<_> = request
        .prepend_props
        .into_iter()
        .filter(|path| !is_reset(path) && selected(path))
        .collect();
    let deep_merge_props = request
        .deep_merge_props
        .into_iter()
        .filter(|path| !is_reset(path) && selected(path))
        .collect();
    let match_props_on = request
        .match_props_on
        .into_iter()
        .filter(|path| !is_reset(path) && selected(path))
        .collect();
    if request.scroll_prepend {
        for key in request.scroll_props.keys() {
            if let Some(path) = request
                .merge_props
                .iter()
                .find(|path| path.starts_with(&format!("{key}.")))
            {
                if !is_reset(path) && !prepend_props.contains(path) {
                    prepend_props.push(path.clone());
                }
            }
        }
    }
    let mut scroll_props = request.scroll_props;
    for (key, metadata) in &mut scroll_props {
        if is_reset(key)
            || request
                .merge_props
                .iter()
                .any(|path| path.starts_with(&format!("{key}.")) && is_reset(path))
        {
            if let Value::Object(metadata) = metadata {
                metadata.insert("reset".into(), Value::Bool(true));
            }
        }
    }
    MergeMetadata {
        merge_props,
        prepend_props,
        deep_merge_props,
        match_props_on,
        scroll_props,
    }
}

fn paths_overlap(left: &str, right: &str) -> bool {
    left == right
        || left.starts_with(&format!("{right}."))
        || right.starts_with(&format!("{left}."))
}

#[derive(Clone)]
struct PropRequest {
    partial_keys: Option<HashSet<String>>,
    except_keys: Option<HashSet<String>>,
    except_once_props: HashSet<String>,
}

struct ResolvedProps {
    props: Map<String, Value>,
    deferred_props: Map<String, Value>,
    once_props: Map<String, Value>,
}

async fn resolve_props(
    mut props: Map<String, Value>,
    page_props: Map<String, Value>,
    always_props: Vec<String>,
    deferred: Vec<crate::response::DeferredProp>,
    once_props: Vec<crate::response::OnceProp>,
    request: PropRequest,
) -> Result<ResolvedProps, crate::PropsError> {
    let PropRequest {
        partial_keys,
        except_keys,
        except_once_props,
    } = request;
    props.extend(page_props);
    let is_partial = partial_keys.is_some() || except_keys.is_some();
    let always_props: HashSet<_> = always_props.iter().map(String::as_str).collect();
    let mut once_metadata = Map::new();
    let mut loaded_once_paths = HashSet::new();
    for once in once_props {
        let included_in_partial = partial_keys
            .as_ref()
            .is_none_or(|keys| keys.iter().any(|key| paths_overlap(&once.prop, key)))
            && !except_keys.as_ref().is_some_and(|keys| {
                keys.iter()
                    .any(|key| once.prop == *key || once.prop.starts_with(&format!("{key}.")))
            });
        if !included_in_partial {
            continue;
        }
        let already_loaded =
            except_once_props.contains(&once.key) || except_once_props.contains(&once.prop);
        let explicitly_requested = partial_keys
            .as_ref()
            .is_some_and(|keys| keys.iter().any(|key| paths_overlap(&once.prop, key)));
        once_metadata.insert(
            once.key.clone(),
            serde_json::json!({ "prop": once.prop, "expiresAt": once.expires_at }),
        );
        if already_loaded && !once.fresh && !explicitly_requested {
            loaded_once_paths.insert(once.prop);
        }
    }
    for path in &loaded_once_paths {
        remove_prop_path(&mut props, path);
    }
    if is_partial {
        props.retain(|key, _| {
            always_props.contains(key.as_str())
                || (partial_keys.as_ref().is_none_or(|keys| keys.contains(key))
                    && !except_keys.as_ref().is_some_and(|keys| keys.contains(key)))
        });
    }
    let mut deferred_props = Map::<String, Value>::new();
    for deferred in deferred {
        let selected = partial_keys
            .as_ref()
            .is_none_or(|keys| keys.contains(&deferred.key))
            && !except_keys
                .as_ref()
                .is_some_and(|keys| keys.contains(&deferred.key));
        let lazy_on_full_visit = !is_partial && !deferred.optional && deferred.group.is_none();
        if loaded_once_paths.contains(&deferred.key) {
            continue;
        }
        let requested = (is_partial && selected) || lazy_on_full_visit;
        if requested {
            let value = (deferred.resolve)().await?;
            props.insert(deferred.key, value);
        } else if let Some(group) = deferred.group.filter(|_| !deferred.optional && !is_partial) {
            let values = deferred_props
                .entry(group)
                .or_insert_with(|| Value::Array(Vec::new()));
            if let Value::Array(keys) = values {
                keys.push(Value::String(deferred.key));
            }
        }
    }
    Ok(ResolvedProps {
        props,
        deferred_props,
        once_props: once_metadata,
    })
}

fn remove_prop_path(props: &mut Map<String, Value>, path: &str) {
    let Some((key, tail)) = path.split_once('.') else {
        props.remove(path);
        return;
    };
    let Some(Value::Object(nested)) = props.get_mut(key) else {
        return;
    };
    remove_prop_path(nested, tail);
}

fn version_conflict(config: &InertiaConfig, context: &RequestContext) -> Option<Response> {
    if context.visit != VisitKind::Inertia
        || context.method != http::Method::GET
        || config.version.is_none()
        || config.version == context.incoming_version
    {
        return None;
    }
    let mut response = StatusCode::CONFLICT.into_response();
    if let Ok(value) = HeaderValue::from_str(&context.url) {
        response.headers_mut().insert("x-inertia-location", value);
    }
    add_vary(&mut response);
    Some(response)
}

fn inertia_response(mut response: Response, page: &Page) -> Response {
    match serde_json::to_vec(page) {
        Ok(bytes) => {
            *response.body_mut() = Body::from(bytes);
            clear_entity_headers(&mut response);
            *response.status_mut() = StatusCode::OK;
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json; charset=utf-8"),
            );
            response
                .headers_mut()
                .insert("x-inertia", HeaderValue::from_static("true"));
            add_vary(&mut response);
            response
        }
        Err(err) => {
            tracing::error!(error = %err, "failed to serialize Inertia page");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn browser_response(mut response: Response, page: &Page, state: &InertiaState) -> Response {
    let monitored_down = state
        .ssr_status
        .as_ref()
        .is_some_and(|status| !status.load(Ordering::Acquire));
    let ssr_result = if monitored_down {
        if state
            .config
            .ssr
            .as_ref()
            .is_some_and(|config| config.strict)
        {
            return StatusCode::BAD_GATEWAY.into_response();
        }
        None
    } else {
        match &state.config.ssr {
            Some(ssr_config) => match state.ssr_client.as_ref() {
                Ok(client) => match ssr::render(client, ssr_config, page).await {
                    Ok(result) => Some(result),
                    Err(err) => {
                        if let Some(status) = &state.ssr_status {
                            status.store(false, Ordering::Release);
                        }
                        if ssr_config.strict {
                            tracing::warn!(component = %page.component, error = %err, "Inertia SSR failed");
                            return StatusCode::BAD_GATEWAY.into_response();
                        }
                        tracing::warn!(component = %page.component, error = %err, "Inertia SSR failed; using client rendering");
                        None
                    }
                },
                Err(err) if ssr_config.strict => {
                    tracing::warn!(component = %page.component, error = %err, "Inertia SSR client unavailable");
                    return StatusCode::BAD_GATEWAY.into_response();
                }
                Err(err) => {
                    tracing::warn!(component = %page.component, error = %err, "Inertia SSR client unavailable; using client rendering");
                    None
                }
            },
            None => None,
        }
    };
    let root_data = match html::root_data(
        page,
        &state.config.mount_id,
        &browser_asset_tags(state).await,
        ssr_result
            .as_ref()
            .map_or(&[], |result| result.head.as_slice()),
        ssr_result
            .as_ref()
            .map_or("", |result| result.body.as_str()),
    ) {
        Ok(data) => data,
        Err(err) => {
            tracing::error!(error = %err, "failed to encode Inertia root view data");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let body = match &state.config.root_view {
        Some(render) => match render(&root_data) {
            Ok(body) => body,
            Err(err) => {
                tracing::error!(error = %err, "failed to render configured Inertia root view");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        },
        None => html::fallback_shell(&root_data),
    };
    *response.body_mut() = Body::from(body);
    clear_entity_headers(&mut response);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    add_vary(&mut response);
    response
}

async fn browser_asset_tags(state: &InertiaState) -> String {
    if let Some(vite) = &state.config.vite_dev_server {
        if let Some(asset_tags) = vite.asset_tags().await {
            tracing::debug!("using Vite development assets");
            return asset_tags;
        }

        tracing::debug!("Vite dev server not available; using configured asset tags");
    }

    state.config.asset_tags.clone()
}

fn clear_entity_headers(response: &mut Response) {
    response.headers_mut().remove(header::CONTENT_LENGTH);
    response.headers_mut().remove(header::CONTENT_ENCODING);
    response.headers_mut().remove(header::CONTENT_RANGE);
    response.headers_mut().remove(header::ETAG);
}

fn add_vary(response: &mut Response) {
    let mut values = Vec::new();
    let existing = response.headers().get_all(header::VARY);
    for value in &existing {
        let Ok(value) = value.to_str() else {
            continue;
        };
        if value
            .split(',')
            .any(|token| token.trim() == "*" || token.trim().eq_ignore_ascii_case("x-inertia"))
        {
            return;
        }
        values.push(value);
    }
    let value = if values.is_empty() {
        "X-Inertia".to_owned()
    } else {
        format!("{}, X-Inertia", values.join(", "))
    };
    if let Ok(value) = HeaderValue::from_str(&value) {
        response.headers_mut().remove(header::VARY);
        response.headers_mut().insert(header::VARY, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Inertia;
    use axum::body::to_bytes;
    use serde_json::json;
    use std::{
        future::Future,
        sync::atomic::AtomicUsize,
        task::{Context, Poll, Waker},
    };

    fn block_on<F: Future>(future: F) -> F::Output {
        let mut context = Context::from_waker(Waker::noop());
        let mut future = Box::pin(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(output) => return output,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    fn context(partial_data: Option<&str>, partial_except: Option<&str>) -> RequestContext {
        RequestContext {
            visit: VisitKind::Inertia,
            method: http::Method::GET,
            url: "/users".into(),
            incoming_version: None,
            partial_component: Some("Users".into()),
            partial_data: partial_data.map(str::to_owned),
            partial_except: partial_except.map(str::to_owned),
            except_once_props: None,
            reset_props: None,
            scroll_prepend: false,
        }
    }

    fn state() -> InertiaState {
        InertiaState {
            config: Arc::new(InertiaConfig::default()),
            ssr_client: Err("unused in these tests".into()),
            ssr_status: None,
        }
    }

    fn page() -> crate::InertiaResponse {
        Inertia::render("Users").props(json!({})).unwrap()
    }

    fn scroll_page() -> crate::InertiaResponse {
        let mut marker = page();
        marker
            .props
            .insert("posts".into(), json!({ "data": [1, 2] }));
        marker.merge_props.push("posts.data".into());
        marker.scroll_props.insert(
            "posts".into(),
            json!({
                "pageName": "page",
                "previousPage": 1,
                "nextPage": 3,
                "currentPage": 2,
            }),
        );
        marker
    }

    async fn response_json(response: Response) -> Value {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn deferred_props_are_advertised_but_not_evaluated_initially() {
        let calls = Arc::new(AtomicUsize::new(0));
        let resolver_calls = calls.clone();
        let marker = page().deferred("users", "content", move || {
            let calls = resolver_calls.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::convert::Infallible>(vec!["A".to_owned()])
            }
        });
        let response = block_on(finalize_page(
            StatusCode::OK.into_response(),
            marker,
            &state(),
            context(None, None),
        ));
        let payload = block_on(response_json(response));

        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(payload["props"], json!({}));
        assert_eq!(payload["deferredProps"]["content"], json!(["users"]));
    }

    #[test]
    fn requested_deferred_props_are_resolved_in_partial_responses() {
        let marker = page().deferred("users", "content", || async {
            Ok::<_, std::convert::Infallible>(vec!["A".to_owned()])
        });
        let response = block_on(finalize_page(
            StatusCode::OK.into_response(),
            marker,
            &state(),
            context(Some("users"), None),
        ));
        let payload = block_on(response_json(response));

        assert_eq!(payload["props"]["users"], json!(["A"]));
        assert!(payload.get("deferredProps").is_none());
    }

    #[test]
    fn except_selection_preserves_always_props() {
        let marker = Inertia::render("Users")
            .props(json!({ "kept": 1, "omitted": false }))
            .unwrap()
            .always("required", true)
            .unwrap();
        let response = block_on(finalize_page(
            StatusCode::OK.into_response(),
            marker,
            &state(),
            context(None, Some("omitted")),
        ));
        let payload = block_on(response_json(response));

        assert_eq!(payload["props"], json!({ "kept": 1, "required": true }));
    }

    #[test]
    fn lazy_props_run_on_full_visits_and_only_when_selected_on_partial_visits() {
        let calls = Arc::new(AtomicUsize::new(0));
        let resolver_calls = calls.clone();
        let marker = page().lazy("expensive", move || {
            let calls = resolver_calls.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::convert::Infallible>("computed")
            }
        });

        let full = block_on(finalize_page(
            StatusCode::OK.into_response(),
            marker.clone(),
            &state(),
            context(None, None),
        ));
        assert_eq!(
            block_on(response_json(full))["props"]["expensive"],
            "computed"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let unrelated = block_on(finalize_page(
            StatusCode::OK.into_response(),
            marker.clone(),
            &state(),
            context(Some("other"), None),
        ));
        assert!(block_on(response_json(unrelated))["props"]
            .get("expensive")
            .is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let selected = block_on(finalize_page(
            StatusCode::OK.into_response(),
            marker,
            &state(),
            context(Some("expensive"), None),
        ));
        assert_eq!(
            block_on(response_json(selected))["props"]["expensive"],
            "computed"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[cfg(feature = "loco")]
    #[test]
    fn loco_scroll_query_is_skipped_when_partial_reload_omits_it() {
        use loco_rs::{controller::views::pagination::PagerMeta, model::query::PageResponse};

        let calls = Arc::new(AtomicUsize::new(0));
        let resolver_calls = calls.clone();
        let marker = page().infinite_scroll("posts", move || {
            let calls = resolver_calls.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::convert::Infallible>(PageResponse {
                    page: vec!["page item"],
                    meta: PagerMeta {
                        page: 1,
                        page_size: 20,
                        total_pages: 2,
                        total_items: 21,
                    },
                })
            }
        });

        let unrelated = block_on(finalize_page(
            StatusCode::OK.into_response(),
            marker.clone(),
            &state(),
            context(Some("other"), None),
        ));
        let payload = block_on(response_json(unrelated));
        assert!(payload["props"].get("posts").is_none());
        assert!(payload.get("scrollProps").is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let selected = block_on(finalize_page(
            StatusCode::OK.into_response(),
            marker,
            &state(),
            context(Some("posts"), None),
        ));
        let payload = block_on(response_json(selected));
        assert_eq!(payload["props"]["posts"]["data"], json!(["page item"]));
        assert_eq!(payload["scrollProps"]["posts"]["nextPage"], 2);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn scroll_props_emit_merge_and_pagination_metadata() {
        let marker = scroll_page();
        let response = block_on(finalize_page(
            StatusCode::OK.into_response(),
            marker,
            &state(),
            context(None, None),
        ));
        let payload = block_on(response_json(response));

        assert_eq!(payload["mergeProps"], json!(["posts.data"]));
        assert_eq!(payload["scrollProps"]["posts"]["currentPage"], 2);
        assert_eq!(payload["scrollProps"]["posts"]["previousPage"], 1);
        assert_eq!(payload["scrollProps"]["posts"]["nextPage"], 3);
    }

    #[test]
    fn scroll_prepend_intent_emits_prepend_metadata() {
        let marker = scroll_page();
        let mut request_context = context(None, None);
        request_context.scroll_prepend = true;
        let response = block_on(finalize_page(
            StatusCode::OK.into_response(),
            marker,
            &state(),
            request_context,
        ));
        let payload = block_on(response_json(response));

        assert_eq!(payload["mergeProps"], json!(["posts.data"]));
        assert_eq!(payload["prependProps"], json!(["posts.data"]));
    }

    #[test]
    fn reset_scroll_prop_replaces_existing_data_and_marks_scroll_state() {
        let mut marker = scroll_page();
        marker.props.insert("posts".into(), json!({ "data": [2] }));
        let mut request_context = context(Some("posts"), None);
        request_context.reset_props = Some("posts".into());
        request_context.scroll_prepend = true;
        let response = block_on(finalize_page(
            StatusCode::OK.into_response(),
            marker,
            &state(),
            request_context,
        ));
        let payload = block_on(response_json(response));

        assert_eq!(payload["props"]["posts"]["data"], json!([2]));
        assert_eq!(payload["scrollProps"]["posts"]["reset"], true);
        assert!(payload.get("mergeProps").is_none());
        assert!(payload.get("prependProps").is_none());
    }

    #[test]
    fn deferred_prop_errors_return_internal_server_error() {
        let marker = page().deferred("users", "content", || async {
            Err::<Vec<String>, _>(std::io::Error::other("database unavailable"))
        });
        let response = block_on(finalize_page(
            StatusCode::OK.into_response(),
            marker,
            &state(),
            context(Some("users"), None),
        ));

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn vary_wildcard_is_preserved_without_adding_another_value() {
        let mut response = StatusCode::OK.into_response();
        response
            .headers_mut()
            .insert(header::VARY, HeaderValue::from_static("*"));

        add_vary(&mut response);

        assert_eq!(response.headers()[header::VARY], "*");
    }
}
