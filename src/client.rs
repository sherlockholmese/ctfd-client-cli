use crate::config::ResolvedConfig;
use crate::models::{
    Challenge, ChallengeDetail, CtfdResponse, ScoreboardEntry, SubmitResponse, UserMe,
};
use reqwest::Client as HttpClient;
use std::path::Path;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

/// Number of attempts for idempotent GET requests (1 initial + retries).
const GET_ATTEMPTS: u32 = 3;
/// Base delay for exponential backoff between GET retries.
const RETRY_BASE_DELAY: Duration = Duration::from_millis(300);
/// Upper bound on a server-requested `Retry-After` delay.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(10);

/// Errors that can occur when interacting with the CTFd API.
///
/// Token values are never included in error messages — only user-facing
/// descriptions and status codes are emitted.
#[derive(Debug, thiserror::Error)]
pub enum CtfdError {
    /// Transport / network failure (connection refused, DNS, timeout, etc.).
    #[error("Network request failed: {0}")]
    Network(#[from] reqwest::Error),

    /// The API returned a success=false envelope or a client/server HTTP error.
    #[error("API error: {0}")]
    Api(String),

    /// JSON parsing failed or the response shape is unexpected.
    #[error("Failed to parse API response: {0}")]
    Parse(String),
}

/// Authenticated CTFd API client.
///
/// Wraps a `reqwest` HTTP client and adds the `Authorization: Token <token>`
/// header to every request. Never includes the token value in error output.
#[derive(Clone)]
pub struct CtfdClient {
    base_url: String,
    api_token: String,
    http: HttpClient,
}

impl CtfdClient {
    /// Create a new client from the resolved configuration.
    pub fn new(config: &ResolvedConfig) -> Self {
        Self {
            base_url: config.base_url.clone(),
            api_token: config.api_token.clone(),
            http: HttpClient::builder()
                .user_agent(concat!("ctfd-client/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(Duration::from_secs(15))
                // Per-read timeout: large attachments can take a long time
                // overall, but a stalled connection should not hang forever.
                .read_timeout(Duration::from_secs(60))
                .build()
                .unwrap_or_else(|_| HttpClient::new()),
        }
    }

    /// Build the full URL for an API path (base URL is already normalized).
    fn api_url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    /// Perform an authenticated GET request and parse the CTFd envelope.
    async fn get<T>(&self, path: &str) -> Result<T, CtfdError>
    where
        T: serde::de::DeserializeOwned,
    {
        let url = self.api_url(path);

        let response = self.send_get(&url).await?;

        let status = response.status();

        // Unauthorized / Forbidden → clear auth guidance.
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(CtfdError::Api(
                "Unauthorized — check that your API token is valid.".to_string(),
            ));
        }

        // Other client errors.
        if status.is_client_error() {
            return Err(CtfdError::Api(format!(
                "API request failed with status {}",
                status.as_u16()
            )));
        }

        // Server errors.
        if status.is_server_error() {
            return Err(CtfdError::Api(format!(
                "CTFd server error (HTTP {})",
                status.as_u16()
            )));
        }

        // Read body first so we can give a useful parse error.
        let body_text = response.text().await?;

        let envelope: CtfdResponse<T> = serde_json::from_str(&body_text)
            .map_err(|e| CtfdError::Parse(format!("Invalid or unexpected response format: {e}")))?;

        if !envelope.success {
            return Err(CtfdError::Api(format!(
                "API returned an error: {}",
                envelope.error_message()
            )));
        }

        envelope
            .data
            .ok_or_else(|| CtfdError::Parse("Response is missing the 'data' field.".to_string()))
    }

    /// Fetch the scoreboard / leaderboard from the CTFd API.
    pub async fn get_leaderboard(&self) -> Result<Vec<ScoreboardEntry>, CtfdError> {
        self.get::<Vec<ScoreboardEntry>>("/api/v1/scoreboard").await
    }

    /// Fetch the authenticated user (used to verify a token).
    pub async fn get_me(&self) -> Result<UserMe, CtfdError> {
        self.get::<UserMe>("/api/v1/users/me").await
    }

    /// Fetch the list of visible challenges from the CTFd API.
    pub async fn get_challenges(&self) -> Result<Vec<Challenge>, CtfdError> {
        self.get::<Vec<Challenge>>("/api/v1/challenges").await
    }

    /// Fetch detailed information for a single challenge.
    pub async fn get_challenge_detail(
        &self,
        challenge_id: u64,
    ) -> Result<ChallengeDetail, CtfdError> {
        self.get::<ChallengeDetail>(&format!("/api/v1/challenges/{challenge_id}"))
            .await
    }

    /// Send an authenticated GET request, retrying transient failures.
    ///
    /// Connection errors, timeouts, HTTP 429 and HTTP 502/503/504 are
    /// retried with exponential backoff (honouring a short `Retry-After`).
    /// CTFd servers are often overloaded at the start of a CTF, so this
    /// keeps bulk downloads from failing on a single hiccup.  Only GETs are
    /// retried; flag submissions are never resent automatically.
    async fn send_get(&self, url: &str) -> Result<reqwest::Response, CtfdError> {
        let mut attempt = 1;
        loop {
            let result = self
                .http
                .get(url)
                .header("Authorization", format!("Token {}", self.api_token))
                .send()
                .await;

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
        let mut response = self.send_get(&resolved_url).await?;

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

    /// Perform an authenticated POST request with a JSON body and parse the
    /// CTFd envelope from the response.
    async fn post_json<T, B>(&self, path: &str, body: &B) -> Result<T, CtfdError>
    where
        T: serde::de::DeserializeOwned,
        B: serde::Serialize,
    {
        let url = self.api_url(path);

        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Token {}", self.api_token))
            .json(body)
            .send()
            .await?;

        let status = response.status();

        // CTFd reports some submission outcomes with a 403/429 status but a
        // regular envelope (e.g. `ratelimited`, `paused`, or "0 tries
        // remaining").  Prefer that envelope over a generic HTTP error.
        if status.as_u16() == 403 || status.as_u16() == 429 {
            let body_text = response.text().await.unwrap_or_default();
            if let Ok(CtfdResponse {
                success: true,
                data: Some(data),
                ..
            }) = serde_json::from_str::<CtfdResponse<T>>(&body_text)
            {
                return Ok(data);
            }
            return Err(CtfdError::Api(if status.as_u16() == 429 {
                "Rate limited by the CTFd server (HTTP 429).".to_string()
            } else {
                "Unauthorized — check that your API token is valid.".to_string()
            }));
        }

        // Unauthorized / Forbidden → clear auth guidance.
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(CtfdError::Api(
                "Unauthorized — check that your API token is valid.".to_string(),
            ));
        }

        // Other client errors.
        if status.is_client_error() {
            return Err(CtfdError::Api(format!(
                "API request failed with status {}",
                status.as_u16()
            )));
        }

        // Server errors.
        if status.is_server_error() {
            return Err(CtfdError::Api(format!(
                "CTFd server error (HTTP {})",
                status.as_u16()
            )));
        }

        // Read body first so we can give a useful parse error.
        let body_text = response.text().await?;

        let envelope: CtfdResponse<T> = serde_json::from_str(&body_text)
            .map_err(|e| CtfdError::Parse(format!("Invalid or unexpected response format: {e}")))?;

        if !envelope.success {
            return Err(CtfdError::Api(format!(
                "API returned an error: {}",
                envelope.error_message()
            )));
        }

        envelope
            .data
            .ok_or_else(|| CtfdError::Parse("Response is missing the 'data' field.".to_string()))
    }

    /// Submit a flag for a challenge via `POST /api/v1/challenges/attempt`.
    ///
    /// The request body contains the numeric `challenge_id` and the exact
    /// `flag` string provided by the user.
    pub async fn submit_flag(
        &self,
        challenge_id: u64,
        flag: &str,
    ) -> Result<SubmitResponse, CtfdError> {
        let body = serde_json::json!({
            "challenge_id": challenge_id,
            "submission": flag,
        });

        self.post_json("/api/v1/challenges/attempt", &body).await
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
