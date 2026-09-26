use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{get, put},
    Router,
};
use loco_inertia::{Inertia, InertiaConfig, InertiaLayer};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tower::ServiceExt;

async fn get_json(response: Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn page(props: Value) -> loco_inertia::InertiaResponse {
    Inertia::render("Users").props(props).unwrap()
}

async fn request(router: Router, request: Request<Body>) -> Response {
    router.oneshot(request).await.unwrap()
}

fn with_layer(router: Router, config: InertiaConfig) -> Router {
    InertiaLayer::new(config).layer(router)
}

fn get_request(path: &str) -> Request<Body> {
    Request::builder().uri(path).body(Body::empty()).unwrap()
}

#[tokio::test]
async fn browser_visit_renders_escaped_bootstrap_html() {
    let app = with_layer(
        Router::new().route("/users", get(|| async { page(json!({ "name": "<Ada>" })) })),
        InertiaConfig::default(),
    );
    let response = request(app, get_request("/users")).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers()[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("text/html"));
    let html = String::from_utf8(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("\\u003cAda\\u003e"));
    assert!(html.contains("<script data-page=\"app\" type=\"application/json\">"));
    assert!(html.contains("<div id=\"app\">"));
}

#[tokio::test]
async fn inertia_visit_returns_json_protocol_headers() {
    let app = with_layer(
        Router::new().route("/users", get(|| async { page(json!({ "name": "Ada" })) })),
        InertiaConfig::default(),
    );
    let response = request(
        app,
        Request::builder()
            .uri("/users?active=true")
            .header("x-inertia", "true")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-inertia"], "true");
    assert!(response.headers()[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("application/json"));
    let vary = response.headers()[header::VARY].to_str().unwrap();
    assert!(vary.eq_ignore_ascii_case("x-inertia"));
    let page = get_json(response).await;
    assert_eq!(page["url"], "/users?active=true");
    assert_eq!(page["props"]["name"], "Ada");
}

#[tokio::test]
async fn matching_partial_only_resolves_requested_lazy_props() {
    let calls = Arc::new(AtomicUsize::new(0));
    let route_calls = calls.clone();
    let app = with_layer(
        Router::new().route(
            "/users",
            get(move || {
                let calls = route_calls.clone();
                async move {
                    page(json!({ "eager": 1, "other": 2 })).optional("lazy", move || {
                        let calls = calls.clone();
                        async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            Ok::<_, std::io::Error>("loaded")
                        }
                    })
                }
            }),
        ),
        InertiaConfig::default(),
    );
    let response = request(
        app,
        Request::builder()
            .uri("/users")
            .header("x-inertia", "true")
            .header("x-inertia-partial-component", "Users")
            .header("x-inertia-partial-data", "lazy")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let page = get_json(response).await;

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(page["props"], json!({ "lazy": "loaded" }));
}

#[tokio::test]
async fn only_and_except_headers_are_combined_with_except_taking_precedence() {
    let app = with_layer(
        Router::new().route(
            "/users",
            get(|| async { page(json!({ "kept": 1, "excluded": 2, "not_requested": 3 })) }),
        ),
        InertiaConfig::default(),
    );
    let response = request(
        app,
        Request::builder()
            .uri("/users")
            .header("x-inertia", "true")
            .header("x-inertia-partial-component", "Users")
            .header("x-inertia-partial-data", "kept, excluded")
            .header("x-inertia-partial-except", "excluded")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(get_json(response).await["props"], json!({ "kept": 1 }));
}

#[tokio::test]
async fn partial_reload_for_another_component_returns_all_props() {
    let app = with_layer(
        Router::new().route(
            "/users",
            get(|| async { page(json!({ "first": 1, "second": 2 })) }),
        ),
        InertiaConfig::default(),
    );
    let response = request(
        app,
        Request::builder()
            .uri("/users")
            .header("x-inertia", "true")
            .header("x-inertia-partial-component", "Other")
            .header("x-inertia-partial-data", "first")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let page = get_json(response).await;

    assert_eq!(page["props"], json!({ "first": 1, "second": 2 }));
}

#[tokio::test]
async fn asset_version_mismatch_returns_location_conflict() {
    let mut config = InertiaConfig::default();
    config.version = Some("current".into());
    let app = with_layer(
        Router::new().route("/users", get(|| async { page(json!({ "name": "Ada" })) })),
        config,
    );
    let response = request(
        app,
        Request::builder()
            .uri("/users?sort=name")
            .header("x-inertia", "true")
            .header("x-inertia-version", "old")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(response.headers()["x-inertia-location"], "/users?sort=name");
}

#[tokio::test]
async fn asset_version_mismatch_does_not_interrupt_mutations() {
    let mut config = InertiaConfig::default();
    config.version = Some("current".into());
    let app = with_layer(
        Router::new().route("/users", put(|| async { page(json!({ "updated": true })) })),
        config,
    );
    let response = request(
        app,
        Request::builder()
            .method("PUT")
            .uri("/users")
            .header("x-inertia", "true")
            .header("x-inertia-version", "old")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn middleware_preserves_multiple_vary_values() {
    let app = with_layer(
        Router::new().route(
            "/users",
            get(|| async {
                let mut response = page(json!({})).into_response();
                response
                    .headers_mut()
                    .append(header::VARY, "Accept-Encoding".parse().unwrap());
                response
                    .headers_mut()
                    .append(header::VARY, "Accept-Language".parse().unwrap());
                response
            }),
        ),
        InertiaConfig::default(),
    );
    let response = request(
        app,
        Request::builder()
            .uri("/users")
            .header("x-inertia", "true")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    let vary = response.headers()[header::VARY]
        .to_str()
        .unwrap()
        .to_ascii_lowercase();
    assert!(vary.contains("accept-encoding"));
    assert!(vary.contains("accept-language"));
    assert!(vary.contains("x-inertia"));
}

#[tokio::test]
async fn inertia_mutation_redirect_is_converted_to_see_other() {
    let app = with_layer(
        Router::new().route("/users", put(|| async { Redirect::to("/done") })),
        InertiaConfig::default(),
    );
    let response = request(
        app,
        Request::builder()
            .method("PUT")
            .uri("/users")
            .header("x-inertia", "true")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/done");
}

#[tokio::test]
async fn custom_root_view_receives_prepared_mount_and_head_data() {
    let config = InertiaConfig::default().root_view(|data| {
        Ok(format!(
            "<html><head>{}</head><body>{}</body></html>",
            data.inertia_head, data.inertia_root
        ))
    });
    let app = with_layer(
        Router::new().route("/users", get(|| async { page(json!({ "name": "Ada" })) })),
        config,
    );
    let response = request(app, get_request("/users")).await;
    let html = String::from_utf8(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();

    assert!(html.starts_with(
        "<html><head></head><body><script data-page=\"app\" type=\"application/json\">"
    ));
    assert!(html.ends_with("</body></html>"));
}

#[tokio::test]
async fn json_visit_skips_strict_ssr_transport() {
    let mut config = InertiaConfig::default();
    config.ssr = Some(loco_inertia::SsrConfig {
        url: "http://127.0.0.1:1/render".into(),
        strict: true,
        ..loco_inertia::SsrConfig::default()
    });
    let app = with_layer(
        Router::new().route("/users", get(|| async { page(json!({})) })),
        config,
    );
    let response = request(
        app,
        Request::builder()
            .uri("/users")
            .header("x-inertia", "true")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-inertia"], "true");
}

#[tokio::test]
async fn strict_ssr_failure_returns_bad_gateway() {
    let mut config = InertiaConfig::default();
    config.ssr = Some(loco_inertia::SsrConfig {
        url: "http://127.0.0.1:1/render".into(),
        strict: true,
        ..loco_inertia::SsrConfig::default()
    });
    let app = with_layer(
        Router::new().route("/users", get(|| async { page(json!({})) })),
        config,
    );
    let response = request(app, get_request("/users")).await;

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn non_strict_ssr_failure_falls_back_to_html() {
    let mut config = InertiaConfig::default();
    config.ssr = Some(loco_inertia::SsrConfig {
        url: "http://127.0.0.1:1/render".into(),
        ..loco_inertia::SsrConfig::default()
    });
    let app = with_layer(
        Router::new().route("/users", get(|| async { page(json!({})) })),
        config,
    );
    let response = request(app, get_request("/users")).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers()[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("text/html"));
}

#[tokio::test]
async fn replaced_json_body_drops_stale_entity_headers() {
    let app = with_layer(
        Router::new().route(
            "/users",
            get(|| async {
                let mut response = page(json!({ "name": "Ada" })).into_response();
                response
                    .headers_mut()
                    .insert(header::CONTENT_LENGTH, "999".parse().unwrap());
                response
                    .headers_mut()
                    .insert(header::CONTENT_ENCODING, "gzip".parse().unwrap());
                response
                    .headers_mut()
                    .insert(header::ETAG, "old".parse().unwrap());
                response
            }),
        ),
        InertiaConfig::default(),
    );
    let response = request(
        app,
        Request::builder()
            .uri("/users")
            .header("x-inertia", "true")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    let content_length = response.headers()[header::CONTENT_LENGTH]
        .to_str()
        .unwrap()
        .parse::<usize>()
        .unwrap();
    assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
    assert!(!response.headers().contains_key(header::ETAG));
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(content_length, body.len());
}
