//! GET retries and streaming attachment downloads. POST requests are never retried here.

use super::{CtfdClient, CtfdError};
use std::path::Path;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

/// Number of attempts for idempotent GET requests (1 initial + retries).
const GET_ATTEMPTS: u32 = 3;
/// Base delay for exponential backoff between GET retries.
const RETRY_BASE_DELAY: Duration = Duration::from_millis(300);
/// Upper bound on a server-requested `Retry-After` delay.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(10);

impl CtfdClient {
    /// Send an authenticated GET request, retrying transient failures.
    ///
    /// Connection errors, timeouts, HTTP 429 and HTTP 502/503/504 are
    /// retried with exponential backoff (honouring a short `Retry-After`).
    /// CTFd servers are often overloaded at the start of a CTF, so this
    /// keeps bulk downloads from failing on a single hiccup.  Only GETs are
    /// retried; flag submissions are never resent automatically.
    pub(super) async fn send_get(&self, url: &str) -> Result<reqwest::Response, CtfdError> {
        self.send_get_with(url, Some(&format!("Token {}", self.api_token)))
            .await
    }

    /// [`Self::send_get`] with an explicit `Authorization` value (or none).
    pub(super) async fn send_get_with(
        &self,
        url: &str,
        authorization: Option<&str>,
    ) -> Result<reqwest::Response, CtfdError> {
        let mut attempt = 1;
        loop {
            let mut request = self
                .http
                .get(url)
                // CTFd only honours `Authorization: Token` on requests with
                // a JSON content type; without it, it redirects to /login.
                .header(reqwest::header::CONTENT_TYPE, "application/json");
            if let Some(value) = authorization {
                request = request.header("Authorization", value);
            }
            let result = request.send().await;

            let retry_delay = match &result {
                Ok(resp) if is_retryable_status(resp.status()) => {
                    Some(retry_after(resp).unwrap_or_else(|| backoff(attempt)))
                }
                Err(e) if e.is_connect() || e.is_timeout() || e.is_request() => {
                    Some(backoff(attempt))
                }
                _ => None,
            };

            match retry_delay {
                Some(delay) if attempt < GET_ATTEMPTS => {
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
                _ => return result.map_err(CtfdError::Network),
            }
        }
    }

    /// Resolve an attachment URL: relative URLs (starting with `/`) are
    /// joined to the configured base URL, absolute URLs are used as-is.
    fn resolve_attachment_url(&self, url: &str) -> String {
        if url.starts_with('/') {
            format!("{}{url}", self.base_url)
        } else {
            url.to_string()
        }
    }

    /// Stream an attachment to `dest`, returning the number of bytes written.
    ///
    /// Data is written to a temporary `.part` file next to `dest` and
    /// renamed into place only after the whole body has been received, so
    /// an interrupted download never leaves a truncated file behind.
    pub async fn download_attachment_to(&self, url: &str, dest: &Path) -> anyhow::Result<u64> {
        let resolved_url = self.resolve_attachment_url(url);
        // rCTF serves files from public (often third-party) storage: never
        // send credentials there.
        let mut response = if self.is_rctf() {
            self.send_get_with(&resolved_url, None).await?
        } else {
            self.send_get(&resolved_url).await?
        };

        let status = response.status();
        if status.is_client_error() || status.is_server_error() {
            return Err(
                CtfdError::Api(format!("Attachment download failed with HTTP {status}")).into(),
            );
        }

        let file_name = dest
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "attachment".to_string());
        let part_path = dest.with_file_name(format!(".{file_name}.part"));

        let result: anyhow::Result<u64> = async {
            let mut file = tokio::fs::File::create(&part_path).await?;
            let mut written = 0u64;
            while let Some(chunk) = response.chunk().await.map_err(CtfdError::Network)? {
                file.write_all(&chunk).await?;
                written += chunk.len() as u64;
            }
            file.flush().await?;
            drop(file);
            tokio::fs::rename(&part_path, dest).await?;
            Ok(written)
        }
        .await;

        if result.is_err() {
            let _ = tokio::fs::remove_file(&part_path).await;
        }
        result
    }
}

/// Whether an HTTP status is worth retrying for an idempotent request.
fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 429 | 502 | 503 | 504)
}

/// Exponential backoff delay for the given (1-based) attempt.
fn backoff(attempt: u32) -> Duration {
    RETRY_BASE_DELAY * 2u32.pow(attempt.saturating_sub(1))
}

/// Parse a `Retry-After` header given in seconds, capped to a short delay.
fn retry_after(resp: &reqwest::Response) -> Option<Duration> {
    let secs: u64 = resp
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(Duration::from_secs(secs).min(MAX_RETRY_AFTER))
}
