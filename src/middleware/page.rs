use super::{
    context::{RequestContext, VisitKind},
    merge::{finalize_merge_metadata, MergeMetadataRequest},
    props::{resolve_props, PropRequest},
    response::{browser_response, inertia_response, version_conflict},
    InertiaState,
};
use crate::{response::InertiaResponse, Page};
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use serde_json::{Map, Value};
use std::collections::HashSet;
pub(super) async fn finalize_page(
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
