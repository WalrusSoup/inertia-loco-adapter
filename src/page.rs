use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Inertia page payload sent to the client or SSR service.
///
/// The adapter fills `url` and `version` from request-scoped middleware data;
/// callers normally create pages with [`crate::Inertia::render`].
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    /// Name of the frontend page component.
    pub component: String,
    /// JSON object passed to the component as props.
    pub props: Map<String, Value>,
    /// Effective request path and query string.
    pub url: String,
    /// Frontend asset version, when configured.
    pub version: Option<String>,
    /// Props that the client should request after the initial render, grouped by name.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub deferred_props: Map<String, Value>,
    /// Prop paths that should be appended during partial reloads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merge_props: Vec<String>,
    /// Prop paths that should be prepended during partial reloads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prepend_props: Vec<String>,
    /// Prop paths to recursively merge during partial reloads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deep_merge_props: Vec<String>,
    /// Prop paths used to match and replace items during merges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub match_props_on: Vec<String>,
    /// Pagination metadata used by Inertia's infinite-scroll component.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub scroll_props: Map<String, Value>,
    /// Client-cached props keyed by cache key, with their prop path and expiry.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub once_props: Map<String, Value>,
    /// One-time values exposed outside regular props and browser history.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub flash: Map<String, Value>,
    /// Whether the client should clear its browser history.
    #[serde(default, skip_serializing_if = "is_false")]
    pub clear_history: bool,
    /// Whether the client should encrypt this page in browser history.
    #[serde(default, skip_serializing_if = "is_false")]
    pub encrypt_history: bool,
}

impl std::fmt::Debug for Page {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Page")
            .field("component", &self.component)
            .field("prop_keys", &self.props.keys().collect::<Vec<_>>())
            .field("url", &self.url)
            .field("version", &self.version)
            .field("deferred_props", &self.deferred_props)
            .field("merge_props", &self.merge_props)
            .field("prepend_props", &self.prepend_props)
            .field("deep_merge_props", &self.deep_merge_props)
            .field("match_props_on", &self.match_props_on)
            .field(
                "scroll_prop_keys",
                &self.scroll_props.keys().collect::<Vec<_>>(),
            )
            .field("once_props", &self.once_props)
            .field("flash_keys", &self.flash.keys().collect::<Vec<_>>())
            .field("clear_history", &self.clear_history)
            .field("encrypt_history", &self.encrypt_history)
            .finish()
    }
}

// Serde's `skip_serializing_if` callback receives a reference by contract.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !value
}
