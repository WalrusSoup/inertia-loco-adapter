use loco_rs::{app::AppContext, prelude::Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_inertia_ssr_url")]
    pub inertia_ssr_url: String,
    #[serde(default)]
    pub inertia_vite_dev_server: Option<String>,
}

impl AppSettings {
    /// Load typed application settings from Loco's configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the configured settings cannot be deserialized.
    pub fn from_context(ctx: &AppContext) -> Result<Self> {
        let settings = ctx
            .config
            .settings
            .clone()
            .unwrap_or_else(|| serde_json::json!({}));
        Ok(serde_json::from_value(settings)?)
    }
}

fn default_inertia_ssr_url() -> String {
    "http://127.0.0.1:13714/render".into()
}
