use serde_json::{Map, Value};
use std::collections::HashSet;

pub(super) struct MergeMetadataRequest {
    pub(super) merge_props: Vec<String>,
    pub(super) prepend_props: Vec<String>,
    pub(super) deep_merge_props: Vec<String>,
    pub(super) match_props_on: Vec<String>,
    pub(super) scroll_props: Map<String, Value>,
    pub(super) reset_header: String,
    pub(super) scroll_prepend: bool,
    pub(super) partial_keys: Option<HashSet<String>>,
    pub(super) except_keys: Option<HashSet<String>>,
}

pub(super) struct MergeMetadata {
    pub(super) merge_props: Vec<String>,
    pub(super) prepend_props: Vec<String>,
    pub(super) deep_merge_props: Vec<String>,
    pub(super) match_props_on: Vec<String>,
    pub(super) scroll_props: Map<String, Value>,
}

pub(super) fn finalize_merge_metadata(request: MergeMetadataRequest) -> MergeMetadata {
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

pub(super) fn paths_overlap(left: &str, right: &str) -> bool {
    left == right
        || left.starts_with(&format!("{right}."))
        || right.starts_with(&format!("{left}."))
}
