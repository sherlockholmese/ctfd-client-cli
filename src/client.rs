mod rctf;

use crate::config::{Platform, ResolvedConfig};
use crate::models::{
    AccountSolve, Challenge, ChallengeDetail, CtfdResponse, ScoreboardEntry, SubmitResponse,
    TopEntry, UserMe,
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

    /// Send an authenticated GET request, retrying transient failures.
    ///
    /// Connection errors, timeouts, HTTP 429 and HTTP 502/503/504 are
    /// retried with exponential backoff (honouring a short `Retry-After`).
    /// CTFd servers are often overloaded at the start of a CTF, so this
    /// keeps bulk downloads from failing on a single hiccup.  Only GETs are
    /// retried; flag submissions are never resent automatically.
    async fn send_get(&self, url: &str) -> Result<reqwest::Response, CtfdError> {
        self.send_get_with(url, Some(&format!("Token {}", self.api_token)))
            .await
    }

    /// [`Self::send_get`] with an explicit `Authorization` value (or none).
    async fn send_get_with(
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

/// Classify a CTFd API response and parse its envelope.
///
/// Every failure carries a [`ResponseDump`] — the request, status,
/// content type, any redirect, and the (pretty-printed, truncated) body —
/// so unexpected responses can be diagnosed from the error alone.
///
/// With `outcome_envelopes`, a 403/429 carrying a successful envelope is
/// returned as data: CTFd reports submission outcomes such as
/// `ratelimited`, `paused`, or "0 tries remaining" that way.
async fn parse_response<T>(
    method: &str,
    url: &str,
    response: reqwest::Response,
    outcome_envelopes: bool,
) -> Result<T, CtfdError>
where
    T: serde::de::DeserializeOwned,
{
    let status = response.status();
    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let redirected_to_login = response.url().path().starts_with("/login");
    let body = response.text().await?;

    let dump = ResponseDump {
        method,
        url,
        final_url: &final_url,
        status,
        content_type: content_type.as_deref(),
        body: &body,
    };
    let parsed = serde_json::from_str::<CtfdResponse<T>>(&body);

    if redirected_to_login {
        return Err(dump.denied(None, true));
    }

    let code = status.as_u16();
    if outcome_envelopes
        && (code == 403 || code == 429)
        && let Ok(CtfdResponse {
            success: true,
            data: Some(data),
            ..
        }) = parsed
    {
        return Ok(data);
    }

    if code == 401 || code == 403 {
        return Err(dump.denied(server_message(&body).as_deref(), false));
    }

    let headline = match code {
        429 => Some("Rate limited by the CTFd server (HTTP 429).".to_string()),
        _ if status.is_client_error() => Some(format!("API request failed with status {code}")),
        _ if status.is_server_error() => Some(format!("CTFd server error (HTTP {code})")),
        _ => None,
    };
    if let Some(headline) = headline {
        return Err(CtfdError::Api(dump.render(&headline)));
    }

    let envelope = parsed.map_err(|e| {
        CtfdError::Parse(dump.render(&format!("Invalid or unexpected response format: {e}")))
    })?;

    if !envelope.success {
        return Err(CtfdError::Api(dump.render(&format!(
            "API returned an error: {}",
            envelope.error_message()
        ))));
    }

    envelope
        .data
        .ok_or_else(|| CtfdError::Parse(dump.render("Response is missing the 'data' field.")))
}

/// Extract CTFd's human-readable message from an error body
/// (`{"message": ...}` or `{"errors": [...]}`), if any.
fn server_message(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let msg = match value.get("message") {
        Some(serde_json::Value::String(m)) => Some(m.clone()),
        _ => match value.get("errors") {
            Some(serde_json::Value::Array(errors)) => errors
                .iter()
                .filter_map(|e| e.as_str())
                .next()
                .map(str::to_string),
            Some(serde_json::Value::Object(errors)) => errors
                .values()
                .flat_map(|v| match v {
                    serde_json::Value::Array(a) => a.clone(),
                    other => vec![other.clone()],
                })
                .find_map(|v| v.as_str().map(str::to_string)),
            _ => None,
        },
    }?;
    let msg = msg.trim().to_string();
    (!msg.is_empty()).then_some(msg)
}

/// Decide why a 401/403 happened and phrase a headline for it.
fn classify_denial(
    code: u16,
    server_msg: Option<&str>,
    login_redirect: bool,
) -> (DenyKind, String) {
    let lower = server_msg.unwrap_or_default().to_lowercase();
    let with_period = |m: &str| {
        if m.ends_with(['.', '!', '?']) {
            m.to_string()
        } else {
            format!("{m}.")
        }
    };

    if login_redirect {
        return (
            DenyKind::BadToken,
            "Unauthorized — CTFd redirected to its login page, so the API token was not accepted."
                .to_string(),
        );
    }
    if lower.contains("not started")
        || lower.contains("not begun")
        || lower.contains("not yet started")
    {
        return (
            DenyKind::NotStarted,
            with_period(server_msg.unwrap_or_default()),
        );
    }
    if lower.contains("has ended") || lower.contains("is over") {
        return (DenyKind::Ended, with_period(server_msg.unwrap_or_default()));
    }
    if lower.contains("paused") {
        return (
            DenyKind::Paused,
            with_period(server_msg.unwrap_or_default()),
        );
    }
    if code == 401 || lower.contains("token") {
        let detail = server_msg.map(|m| format!(": {}", with_period(m)));
        return (
            DenyKind::BadToken,
            format!(
                "Unauthorized{}",
                detail.unwrap_or_else(|| " — check that your API token is valid.".to_string())
            ),
        );
    }
    let detail = server_msg
        .map(|m| format!(": {}", with_period(m)))
        .unwrap_or_else(|| ".".to_string());
    (
        DenyKind::Forbidden,
        format!("Unauthorized — CTFd refused access (HTTP {code}){detail}"),
    )
}

/// Maximum number of body lines shown in an error.
const DUMP_MAX_LINES: usize = 20;
/// Maximum characters per body line shown in an error.
const DUMP_MAX_LINE_CHARS: usize = 160;

/// Everything needed to show an unexpected response to the user.
///
/// Never contains the API token: it is sent only in a request header,
/// which is not included.
struct ResponseDump<'a> {
    method: &'a str,
    url: &'a str,
    final_url: &'a str,
    status: reqwest::StatusCode,
    content_type: Option<&'a str>,
    body: &'a str,
}

impl ResponseDump<'_> {
    /// Render `headline` followed by request/response details and the body
    /// in a framed block.
    fn render(&self, headline: &str) -> String {
        let mut out = format!("{headline}\n\n  Request:  {} {}", self.method, self.url);
        if self.final_url != self.url {
            out.push_str(&format!("\n  Redirect: -> {}", self.final_url));
        }
        out.push_str(&format!("\n  Response: HTTP {}", self.status));
        if let Some(ct) = self.content_type {
            out.push_str(&format!(", {ct}"));
        }
        out.push_str(&format!(", {} bytes", self.body.len()));

        let pretty = serde_json::from_str::<serde_json::Value>(self.body)
            .ok()
            .and_then(|v| serde_json::to_string_pretty(&v).ok())
            .unwrap_or_else(|| self.body.to_string());
        let lines: Vec<&str> = pretty.trim().lines().collect();

        if lines.is_empty() {
            out.push_str("\n  Body:     (empty)");
        } else {
            let note = if lines.len() > DUMP_MAX_LINES {
                format!(" (first {DUMP_MAX_LINES} of {} lines)", lines.len())
            } else {
                String::new()
            };
            out.push_str(&format!("\n  ╭─ response body{note}"));
            for line in lines.iter().take(DUMP_MAX_LINES) {
                out.push_str("\n  │ ");
                out.push_str(&sanitize_line(line));
            }
            if lines.len() > DUMP_MAX_LINES {
                out.push_str("\n  │ …");
            }
            out.push_str("\n  ╰─");
        }

        if let Some(hint) = self.hint() {
            out.push_str(&format!("\n  Hint: {hint}"));
        }
        out
    }

    /// Build a [`CtfdError::Denied`] with an explanation tailored to the
    /// reason CTFd gave.  Recognised CTF-state refusals (not started,
    /// ended, paused) get a short, clean message; anything else keeps the
    /// full response dump for diagnosis.
    fn denied(&self, server_msg: Option<&str>, login_redirect: bool) -> CtfdError {
        let (kind, headline) = classify_denial(self.status.as_u16(), server_msg, login_redirect);
        let base = self.url.split("/api/").next().unwrap_or(self.url);
        let explain = match kind {
            DenyKind::NotStarted => Some(
                "Your API token works. Challenges and the scoreboard unlock when the CTF \
                 starts; then `ctfdc dl` downloads everything."
                    .to_string(),
            ),
            DenyKind::Ended => Some(
                "The CTF is over. Challenges may stay visible, but solves no longer count."
                    .to_string(),
            ),
            DenyKind::Paused => {
                Some("The organisers paused the CTF; try again once it resumes.".to_string())
            }
            DenyKind::BadToken | DenyKind::Forbidden => None,
        };

        let message = match explain {
            Some(explain) => format!(
                "{headline}\n\n  {explain}\n\n  Request:  {} {} -> HTTP {}",
                self.method, self.url, self.status
            ),
            None => {
                let mut out = self.render(&headline);
                if kind == DenyKind::Forbidden {
                    out.push_str(&format!(
                        "\n  Common causes: you haven't joined a team yet (teams mode: \
                         {base}/team), your email isn't confirmed, your account is banned, \
                         or this is hidden from you."
                    ));
                }
                out
            }
        };
        CtfdError::Denied { kind, message }
    }

    fn hint(&self) -> Option<&'static str> {
        let looks_html = self.content_type.is_some_and(|ct| ct.contains("html"))
            || self.body.trim_start().starts_with('<');
        if self.final_url.contains("/login") {
            Some(
                "create a new token in CTFd under Settings -> Access Tokens and run \
                 `ctfdc init <url>` again.",
            )
        } else if matches!(self.status.as_u16(), 401 | 403) {
            Some("check the token (CTFd -> Settings -> Access Tokens); `ctfdc init` updates it.")
        } else if self.status.is_server_error() {
            Some("the CTFd server is struggling; try again in a moment.")
        } else if looks_html {
            Some(
                "the server sent an HTML page, not CTFd API JSON. Make sure the URL is the \
                 CTFd root (e.g. https://ctf.example.com) and that no proxy or browser \
                 check (e.g. Cloudflare) sits in front of the API.",
            )
        } else {
            None
        }
    }
}

/// Make a body line safe to print: replace control characters (which could
/// carry terminal escape sequences), expand tabs, and truncate.
fn sanitize_line(line: &str) -> String {
    let mut out = String::new();
    for (count, c) in line.chars().enumerate() {
        if count >= DUMP_MAX_LINE_CHARS {
            out.push('…');
            break;
        }
        match c {
            '\t' => out.push_str("    "),
            c if c.is_control() => out.push('?'),
            c => out.push(c),
        }
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    fn dump<'a>(
        status: u16,
        ct: Option<&'a str>,
        final_url: &'a str,
        body: &'a str,
    ) -> ResponseDump<'a> {
        ResponseDump {
            method: "GET",
            url: "https://ctf.example.com/api/v1/users/me",
            final_url,
            status: reqwest::StatusCode::from_u16(status).unwrap(),
            content_type: ct,
            body,
        }
    }

    #[test]
    fn renders_html_body_in_frame_with_redirect_and_hint() {
        let html = "<!DOCTYPE html>\n<html>\n<title>Login</title>\n</html>";
        let out = dump(
            200,
            Some("text/html"),
            "https://ctf.example.com/login?next=x",
            html,
        )
        .render("Headline");
        assert!(out.starts_with("Headline\n"));
        assert!(out.contains("Request:  GET https://ctf.example.com/api/v1/users/me"));
        assert!(out.contains("Redirect: -> https://ctf.example.com/login?next=x"));
        assert!(out.contains("HTTP 200 OK, text/html"));
        assert!(out.contains("╭─ response body"));
        assert!(out.contains("│ <!DOCTYPE html>"));
        assert!(out.contains("Hint: create a new token"));
    }

    #[test]
    fn pretty_prints_json_and_truncates() {
        let body = r#"{"message":"Your access token is invalid"}"#;
        let url = "https://ctf.example.com/api/v1/users/me";
        let out = dump(401, Some("application/json"), url, body).render("H");
        assert!(out.contains("│   \"message\": \"Your access token is invalid\""));
        assert!(!out.contains("Redirect:"));

        let long: String = (0..50).map(|i| format!("line {i}\n")).collect();
        let out = dump(200, None, url, &long).render("H");
        assert!(out.contains("(first 20 of 50 lines)"));
        assert!(out.contains("│ line 19"));
        assert!(!out.contains("│ line 20"));
    }

    #[test]
    fn classifies_ctf_state_refusals() {
        let (k, h) = classify_denial(403, Some("Hack N' Flag 2026 has not started yet"), false);
        assert_eq!(k, DenyKind::NotStarted);
        assert_eq!(h, "Hack N' Flag 2026 has not started yet.");
        assert_eq!(
            classify_denial(403, Some("MyCTF has ended"), false).0,
            DenyKind::Ended
        );
        assert_eq!(
            classify_denial(403, Some("MyCTF is paused"), false).0,
            DenyKind::Paused
        );
        assert_eq!(
            classify_denial(401, Some("Your access token is invalid"), false).0,
            DenyKind::BadToken
        );
        assert_eq!(
            classify_denial(403, Some("Your access token has expired"), false).0,
            DenyKind::BadToken
        );
        assert_eq!(classify_denial(403, None, false).0, DenyKind::Forbidden);
        assert_eq!(classify_denial(200, None, true).0, DenyKind::BadToken);
    }

    #[test]
    fn extracts_server_messages() {
        assert_eq!(server_message(r#"{"message": "x"}"#).as_deref(), Some("x"));
        assert_eq!(
            server_message(r#"{"success": false, "errors": ["y"]}"#).as_deref(),
            Some("y")
        );
        assert_eq!(
            server_message(r#"{"errors": {"field": ["z"]}}"#).as_deref(),
            Some("z")
        );
        assert_eq!(server_message("<html>"), None);
    }

    #[test]
    fn not_started_message_is_clean() {
        let body = r#"{"message": "Hack N' Flag 2026 has not started yet"}"#;
        let d = dump(
            403,
            Some("application/json"),
            "https://ctf.example.com/api/v1/users/me",
            body,
        );
        let CtfdError::Denied { kind, message } = d.denied(server_message(body).as_deref(), false)
        else {
            panic!("expected Denied");
        };
        assert_eq!(kind, DenyKind::NotStarted);
        assert!(message.starts_with("Hack N' Flag 2026 has not started yet.\n"));
        assert!(message.contains("Your API token works"));
        assert!(message.contains("-> HTTP 403 Forbidden"));
        assert!(!message.contains("response body"));
        assert!(!message.contains("check the token"));
    }

    #[test]
    fn strips_terminal_escapes_and_handles_empty_body() {
        assert_eq!(sanitize_line("a\x1b[31mred"), "a?[31mred");
        let url = "https://ctf.example.com/api/v1/users/me";
        assert!(
            dump(500, None, url, "")
                .render("H")
                .contains("Body:     (empty)")
        );
    }
}
