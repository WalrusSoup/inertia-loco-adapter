use super::{
    context::{RequestContext, VisitKind},
    InertiaState,
};
use crate::{config::InertiaConfig, html, ssr, Page};
use axum::{
    body::Body,
    response::{IntoResponse, Response},
};
use http::{header, HeaderValue, StatusCode};
use std::sync::atomic::Ordering;
pub(super) fn finalize_passthrough(mut response: Response, context: &RequestContext) -> Response {
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

pub(super) fn version_conflict(
    config: &InertiaConfig,
    context: &RequestContext,
) -> Option<Response> {
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

pub(super) fn inertia_response(mut response: Response, page: &Page) -> Response {
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

pub(super) async fn browser_response(
    mut response: Response,
    page: &Page,
    state: &InertiaState,
) -> Response {
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

pub(super) fn add_vary(response: &mut Response) {
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
