use crate::Page;
use serde::Deserialize;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

/// HTTP settings for the official Inertia `/render` service.
///
/// SSR is disabled unless a value is assigned to [`crate::InertiaConfig::ssr`]. The
/// default endpoint is `http://127.0.0.1:13714/render`; failures fall back to
/// client rendering unless `strict` is enabled. The default timeout is two
/// seconds and the response body limit is 2 MiB.
#[derive(Clone)]
pub struct SsrConfig {
    /// Full HTTP URL of the SSR `/render` endpoint.
    pub url: String,
    /// Optional readiness URL polled in the background. When configured,
    /// browser requests skip SSR while this endpoint is unavailable.
    pub status_url: Option<String>,
    /// Delay between background readiness checks.
    pub status_poll_interval: Duration,
    /// Maximum time allowed for each readiness check.
    pub status_timeout: Duration,
    /// Maximum time allowed for the request and response body.
    pub timeout: Duration,
    /// Maximum accepted response body size in bytes.
    pub max_response_bytes: usize,
    /// Return a 502 response instead of falling back to client rendering.
    pub strict: bool,
}

impl std::fmt::Debug for SsrConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SsrConfig")
            .field("url_configured", &!self.url.is_empty())
            .field("status_url_configured", &self.status_url.is_some())
            .field("status_poll_interval", &self.status_poll_interval)
            .field("status_timeout", &self.status_timeout)
            .field("timeout", &self.timeout)
            .field("max_response_bytes", &self.max_response_bytes)
            .field("strict", &self.strict)
            .finish()
    }
}
impl Default for SsrConfig {
    fn default() -> Self {
        Self {
            url: "http://127.0.0.1:13714/render".into(),
            status_url: None,
            status_poll_interval: Duration::from_secs(1),
            status_timeout: Duration::from_millis(250),
            timeout: Duration::from_secs(2),
            max_response_bytes: 2 * 1024 * 1024,
            strict: false,
        }
    }
}

/// Rendered fragments returned by the Inertia SSR service.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct SsrResult {
    /// HTML fragments to insert into the root document's `<head>`.
    #[serde(default)]
    pub head: Vec<String>,
    /// Rendered page markup to place inside the Inertia mount element.
    #[serde(default)]
    pub body: String,
}

/// Errors returned while requesting or decoding an SSR response.
#[derive(Debug, thiserror::Error)]
pub enum SsrError {
    /// The shared HTTP client could not be initialized.
    #[error("SSR HTTP client initialization failed: {0}")]
    Client(String),
    /// The request failed before a successful response was read.
    #[error("SSR request failed: {0}")]
    Request(#[from] reqwest::Error),
    /// The SSR service returned a non-success HTTP status.
    #[error("SSR returned HTTP {0}")]
    Status(reqwest::StatusCode),
    /// The response exceeded [`SsrConfig::max_response_bytes`].
    #[error("SSR response exceeded the configured byte limit")]
    TooLarge,
    /// The response body was not a valid SSR JSON payload.
    #[error("invalid SSR response: {0}")]
    Json(#[from] serde_json::Error),
}

pub(crate) fn client() -> Result<reqwest::Client, SsrError> {
    reqwest::Client::builder()
        .build()
        .map_err(|err| SsrError::Client(err.to_string()))
}

pub(crate) fn monitor(config: &SsrConfig, client: &reqwest::Client) -> Option<Arc<AtomicBool>> {
    let url = config.status_url.as_ref()?.clone();
    let available = Arc::new(AtomicBool::new(false));
    let state = Arc::clone(&available);
    let client = client.clone();
    let interval = config.status_poll_interval.max(Duration::from_millis(50));
    let timeout = config.status_timeout;

    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        tracing::warn!(
            status_url = %url,
            "Inertia SSR status monitoring requires an active Tokio runtime; monitoring is disabled"
        );
        return None;
    };

    runtime.spawn(async move {
        let mut previous = None;
        loop {
            let healthy = client
                .get(&url)
                .timeout(timeout)
                .send()
                .await
                .is_ok_and(|response| response.status().is_success());
            state.store(healthy, Ordering::Release);

            if previous != Some(healthy) {
                if healthy {
                    tracing::info!(status_url = %url, "Inertia SSR service is ready");
                } else {
                    tracing::warn!(status_url = %url, "Inertia SSR service is unavailable");
                }
                previous = Some(healthy);
            }

            tokio::time::sleep(interval).await;
        }
    });

    Some(available)
}

pub(crate) async fn render(
    client: &reqwest::Client,
    config: &SsrConfig,
    page: &Page,
) -> Result<SsrResult, SsrError> {
    let mut response = client
        .post(&config.url)
        .timeout(config.timeout)
        .json(page)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(SsrError::Status(response.status()));
    }
    if response
        .content_length()
        .is_some_and(|n| usize::try_from(n).map_or(true, |n| n > config.max_response_bytes))
    {
        return Err(SsrError::TooLarge);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > config.max_response_bytes {
            return Err(SsrError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Router};
    use tokio::sync::oneshot;

    async fn mock_server(
        status: reqwest::StatusCode,
        body: &'static str,
    ) -> (String, oneshot::Sender<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let body = Arc::new(body);
        let app = Router::new().route(
            "/render",
            post(move || {
                let body = *body;
                async move { (status, body) }
            }),
        );
        let (shutdown, signal) = oneshot::channel();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = signal.await;
                })
                .await
                .unwrap();
        });
        (format!("http://{address}/render"), shutdown)
    }

    fn page() -> Page {
        Page {
            component: "Users".into(),
            props: serde_json::Map::new(),
            url: "/users".into(),
            version: None,
            deferred_props: serde_json::Map::new(),
            merge_props: Vec::new(),
            prepend_props: Vec::new(),
            deep_merge_props: Vec::new(),
            match_props_on: Vec::new(),
            scroll_props: serde_json::Map::new(),
            once_props: serde_json::Map::new(),
            flash: serde_json::Map::new(),
            clear_history: false,
            encrypt_history: false,
        }
    }

    #[tokio::test]
    async fn successful_response_deserializes_rendered_fragments() {
        let (url, shutdown) = mock_server(
            reqwest::StatusCode::OK,
            r#"{"head":["<title>Users</title>"],"body":"<main>Users</main>"}"#,
        )
        .await;
        let config = SsrConfig {
            url,
            ..SsrConfig::default()
        };
        let result = render(&client().unwrap(), &config, &page()).await.unwrap();
        let _ = shutdown.send(());

        assert_eq!(result.head, ["<title>Users</title>"]);
        assert_eq!(result.body, "<main>Users</main>");
    }

    #[tokio::test]
    async fn non_success_status_is_reported() {
        let (url, shutdown) = mock_server(reqwest::StatusCode::BAD_GATEWAY, "failed").await;
        let config = SsrConfig {
            url,
            ..SsrConfig::default()
        };
        let result = render(&client().unwrap(), &config, &page()).await;
        let _ = shutdown.send(());

        assert!(matches!(
            result,
            Err(SsrError::Status(reqwest::StatusCode::BAD_GATEWAY))
        ));
    }

    #[tokio::test]
    async fn response_body_limit_is_enforced() {
        let (url, shutdown) = mock_server(reqwest::StatusCode::OK, "123456").await;
        let config = SsrConfig {
            url,
            max_response_bytes: 5,
            ..SsrConfig::default()
        };
        let result = render(&client().unwrap(), &config, &page()).await;
        let _ = shutdown.send(());

        assert!(matches!(result, Err(SsrError::TooLarge)));
    }

    #[tokio::test]
    async fn invalid_json_response_is_reported() {
        let (url, shutdown) = mock_server(reqwest::StatusCode::OK, "not json").await;
        let config = SsrConfig {
            url,
            ..SsrConfig::default()
        };
        let result = render(&client().unwrap(), &config, &page()).await;
        let _ = shutdown.send(());

        assert!(matches!(result, Err(SsrError::Json(_))));
    }
}
