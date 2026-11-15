//! CTFd envelopes, refusal classification, and bounded response diagnostics.

use super::{CtfdError, DenyKind};
use crate::models::CtfdResponse;

/// Classify a CTFd API response and parse its envelope.
///
/// Every failure carries a [`ResponseDump`] — the request, status,
/// content type, any redirect, and the (pretty-printed, truncated) body —
/// so unexpected responses can be diagnosed from the error alone.
///
/// With `outcome_envelopes`, a 403/429 carrying a successful envelope is
/// returned as data: CTFd reports submission outcomes such as
/// `ratelimited`, `paused`, or "0 tries remaining" that way.
pub(super) async fn parse_response<T>(
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
pub(super) struct ResponseDump<'a> {
    pub(super) method: &'a str,
    pub(super) url: &'a str,
    pub(super) final_url: &'a str,
    pub(super) status: reqwest::StatusCode,
    pub(super) content_type: Option<&'a str>,
    pub(super) body: &'a str,
}

impl ResponseDump<'_> {
    /// Render `headline` followed by request/response details and the body
    /// in a framed block.
    pub(super) fn render(&self, headline: &str) -> String {
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
    pub(super) fn denied(&self, server_msg: Option<&str>, login_redirect: bool) -> CtfdError {
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

#[cfg(test)]
mod tests;
