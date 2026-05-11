use crate::config::ResolvedConfig;
use crate::models::{Challenge, CtfdResponse, ScoreboardEntry};
use reqwest::Client as HttpClient;

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
            http: HttpClient::new(),
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

        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("Token {}", self.api_token))
            .send()
            .await?;

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

    /// Fetch the list of visible challenges from the CTFd API.
    pub async fn get_challenges(&self) -> Result<Vec<Challenge>, CtfdError> {
        self.get::<Vec<Challenge>>("/api/v1/challenges").await
    }
}
