use axum::extract::Request;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum VisitKind {
    Browser,
    Inertia,
}

pub(super) struct RequestContext {
    pub(super) visit: VisitKind,
    pub(super) method: http::Method,
    pub(super) url: String,
    pub(super) incoming_version: Option<String>,
    pub(super) partial_component: Option<String>,
    pub(super) partial_data: Option<String>,
    pub(super) partial_except: Option<String>,
    pub(super) except_once_props: Option<String>,
    pub(super) reset_props: Option<String>,
    pub(super) scroll_prepend: bool,
}

impl RequestContext {
    pub(super) fn from_request(request: &Request) -> Self {
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
