use crate::{config::InertiaConfig, response::InertiaResponse};
use axum::{
    extract::Request,
    middleware::{from_fn_with_state, Next},
    response::Response,
    Router,
};
use std::sync::{atomic::AtomicBool, Arc};

mod context;
mod merge;
mod page;
mod props;
mod response;

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
        let ssr_client = crate::ssr::client().map_err(|err| err.to_string());
        let config = Arc::new(config);
        let ssr_status = config.ssr.as_ref().and_then(|ssr_config| {
            ssr_config.status_url.as_ref()?;

            match &ssr_client {
                Ok(client) => crate::ssr::monitor(ssr_config, client),
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
    let context = context::RequestContext::from_request(&request);
    let mut response = next.run(request).await;
    match response.extensions_mut().remove::<InertiaResponse>() {
        Some(marker) => page::finalize_page(response, marker, &state, context).await,
        None => response::finalize_passthrough(response, &context),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use super::{
        context::{RequestContext, VisitKind},
        page::finalize_page,
        response::add_vary,
    };
    use crate::Inertia;
    use axum::body::to_bytes;
    use axum::response::IntoResponse;
    use http::{header, HeaderValue, StatusCode};
    use serde_json::{json, Value};
    use std::{
        future::Future,
        sync::atomic::{AtomicUsize, Ordering},
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
