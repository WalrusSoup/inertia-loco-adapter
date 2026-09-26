use reqwest::{Client, Url};
use std::{path::PathBuf, time::Duration};

/// Configuration for Vite development assets and optional React Fast Refresh.
///
/// The Vite dev server writes its resolved URL to a hot file. The adapter
/// checks that URL at request time and emits the Vite client and configured
/// entry when the server is available. React users can opt into the React
/// Fast Refresh preamble with [`Self::react_refresh`]. Otherwise the adapter
/// falls back to [`crate::InertiaConfig::asset_tags`].
#[derive(Clone, Debug)]
pub struct ViteDevConfig {
    hot_file: PathBuf,
    entry: String,
    server_url: Option<String>,
    react_refresh: bool,
}

impl ViteDevConfig {
    /// Configure Vite using the file where its dev-server plugin writes its
    /// actual URL and the entry module served by Vite (for example, `app.jsx`).
    #[must_use]
    pub fn new(hot_file: impl Into<PathBuf>, entry: impl Into<String>) -> Self {
        Self {
            hot_file: hot_file.into(),
            entry: entry.into(),
            server_url: None,
            react_refresh: false,
        }
    }

    /// Override the URL read from the hot file.
    #[must_use]
    pub fn server_url(mut self, url: impl Into<String>) -> Self {
        self.server_url = Some(url.into());
        self
    }

    /// Include the React Fast Refresh preamble required by `@vitejs/plugin-react`
    /// when HTML is rendered by a backend instead of Vite.
    #[must_use]
    pub fn react_refresh(mut self) -> Self {
        self.react_refresh = true;
        self
    }

    pub(crate) async fn asset_tags(&self) -> Option<String> {
        let server_url = match &self.server_url {
            Some(url) => url.clone(),
            None => tokio::fs::read_to_string(&self.hot_file)
                .await
                .ok()?
                .trim()
                .to_owned(),
        };
        let server_url = Url::parse(server_url.trim()).ok()?;
        if !matches!(server_url.scheme(), "http" | "https") {
            return None;
        }

        let client_url = server_url.join("@vite/client").ok()?;
        let entry_url = server_url.join(self.entry.trim_start_matches('/')).ok()?;
        let health_url = server_url.join("@vite/client").ok()?;
        let client = Client::builder()
            .timeout(Duration::from_millis(500))
            .build()
            .ok()?;
        let response = client.get(health_url).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }

        let react_preamble = if self.react_refresh {
            let refresh_url = server_url.join("@react-refresh").ok()?;
            format!(
                r#"<script type="module">
import {{ injectIntoGlobalHook }} from "{refresh_url}";
injectIntoGlobalHook(window);
window.$RefreshReg$ = () => {{}};
window.$RefreshSig$ = () => (type) => type;
window.__vite_plugin_react_preamble_installed__ = true;
</script>"#
            )
        } else {
            String::new()
        };

        Some(format!(
            "{react_preamble}<script type=\"module\" src=\"{client_url}\"></script><script type=\"module\" src=\"{entry_url}\"></script>"
        ))
    }
}
