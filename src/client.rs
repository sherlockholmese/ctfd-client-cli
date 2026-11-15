//! Platform-neutral API methods. CTFd transport and response handling live
//! alongside the rCTF adapter in `client/`.

mod rctf;
mod response;
mod transport;

use response::parse_response;

use crate::config::{Platform, ResolvedConfig};
use crate::models::{
    AccountSolve, Challenge, ChallengeDetail, ScoreboardEntry, SubmitResponse, TopEntry, UserMe,
};
use reqwest::Client as HttpClient;
use std::time::Duration;

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

    /// CTFd refused the request (HTTP 401/403 or a login redirect); `kind`
    /// says why, `message` is the rendered, user-facing explanation.
    #[error("{message}")]
    Denied { kind: DenyKind, message: String },
}

/// Why CTFd refused a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyKind {
    /// The API token is missing, invalid, or expired.
    BadToken,
    /// The token is fine, but the CTF has not started yet.
    NotStarted,
    /// The token is fine, but the CTF is over.
    Ended,
    /// The token is fine, but the CTF is paused.
    Paused,
    /// Any other refusal (no team yet, unconfirmed email, banned, hidden).
    Forbidden,
}

impl DenyKind {
    /// Whether the refusal proves the token itself was accepted.
    pub fn token_accepted(self) -> bool {
        matches!(self, Self::NotStarted | Self::Ended | Self::Paused)
    }
}

/// Authenticated CTF API client.
///
/// Talks to CTFd directly; for rCTF ([`Platform::Rctf`]) every method maps
/// the rCTF API onto the same CTFd-shaped models, so commands don't care
/// which platform they run against.  Never includes a token in error
/// output.
#[derive(Clone)]
pub struct CtfdClient {
    base_url: String,
    api_token: String,
    http: HttpClient,
    platform: Platform,
    rctf: std::sync::Arc<rctf::State>,
}

impl CtfdClient {
    /// Create a new client from the resolved configuration.
    pub fn new(config: &ResolvedConfig) -> Self {
        Self {
            base_url: config.base_url.clone(),
            api_token: config.api_token.clone(),
            http: HttpClient::builder()
                .user_agent(concat!("ctfdc/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(Duration::from_secs(15))
                // Per-read timeout: large attachments can take a long time
                // overall, but a stalled connection should not hang forever.
                .read_timeout(Duration::from_secs(60))
                .build()
                .unwrap_or_else(|_| HttpClient::new()),
            platform: config.platform,
            rctf: Default::default(),
        }
    }

    fn is_rctf(&self) -> bool {
        self.platform == Platform::Rctf
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
        parse_response("GET", &url, response, false).await
    }

    /// Fetch the scoreboard / leaderboard from the CTFd API.
    pub async fn get_leaderboard(&self) -> Result<Vec<ScoreboardEntry>, CtfdError> {
        if self.is_rctf() {
            return rctf::leaderboard(self).await;
        }
        self.get::<Vec<ScoreboardEntry>>("/api/v1/scoreboard").await
    }

    /// Fetch the top `count` accounts with their score history.
    ///
    /// CTFd keys the result by rank (`"1"`, `"2"`, ...); entries are
    /// returned in rank order.
    pub async fn get_scoreboard_top(&self, count: u32) -> Result<Vec<TopEntry>, CtfdError> {
        if self.is_rctf() {
            return rctf::top(self, count).await;
        }
        let map = self
            .get::<std::collections::HashMap<String, TopEntry>>(&format!(
                "/api/v1/scoreboard/top/{count}"
            ))
            .await?;
        let mut ranked: Vec<(u64, TopEntry)> = map
            .into_iter()
            .map(|(rank, entry)| (rank.parse().unwrap_or(u64::MAX), entry))
            .collect();
        ranked.sort_by_key(|(rank, _)| *rank);
        Ok(ranked.into_iter().map(|(_, e)| e).collect())
    }

    /// Fetch the solves of one team (`teams`) or user (`users`).
    pub async fn get_account_solves(
        &self,
        kind: &str,
        account_id: u64,
    ) -> Result<Vec<AccountSolve>, CtfdError> {
        if self.is_rctf() {
            return rctf::account_solves(self, account_id).await;
        }
        self.get::<Vec<AccountSolve>>(&format!("/api/v1/{kind}/{account_id}/solves"))
            .await
    }

    /// Fetch the authenticated user (used to verify a token).
    pub async fn get_me(&self) -> Result<UserMe, CtfdError> {
        if self.is_rctf() {
            return rctf::me(self).await;
        }
        self.get::<UserMe>("/api/v1/users/me").await
    }

    /// Fetch the list of visible challenges from the CTFd API.
    pub async fn get_challenges(&self) -> Result<Vec<Challenge>, CtfdError> {
        if self.is_rctf() {
            return rctf::challenges(self).await;
        }
        self.get::<Vec<Challenge>>("/api/v1/challenges").await
    }

    /// Fetch detailed information for a single challenge.
    pub async fn get_challenge_detail(
        &self,
        challenge_id: u64,
    ) -> Result<ChallengeDetail, CtfdError> {
        if self.is_rctf() {
            return rctf::challenge_detail(self, challenge_id).await;
        }
        self.get::<ChallengeDetail>(&format!("/api/v1/challenges/{challenge_id}"))
            .await
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

        parse_response("POST", &url, response, true).await
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
        if self.is_rctf() {
            return rctf::submit(self, challenge_id, flag).await;
        }
        let body = serde_json::json!({
            "challenge_id": challenge_id,
            "submission": flag,
        });

        self.post_json("/api/v1/challenges/attempt", &body).await
    }
}

/// Whether `base_url` serves rCTF: its public client config answers with
/// an rCTF `goodClientConfig` envelope.  Any failure means "not rCTF".
pub async fn detect_rctf(base_url: &str) -> bool {
    rctf::detect(base_url).await
}
