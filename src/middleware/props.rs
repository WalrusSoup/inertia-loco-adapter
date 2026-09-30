use super::merge::paths_overlap;
use serde_json::{Map, Value};
use std::collections::HashSet;
#[derive(Clone)]
pub(super) struct PropRequest {
    pub(super) partial_keys: Option<HashSet<String>>,
    pub(super) except_keys: Option<HashSet<String>>,
    pub(super) except_once_props: HashSet<String>,
}

pub(super) struct ResolvedProps {
    pub(super) props: Map<String, Value>,
    pub(super) deferred_props: Map<String, Value>,
    pub(super) once_props: Map<String, Value>,
}

pub(super) async fn resolve_props(
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
