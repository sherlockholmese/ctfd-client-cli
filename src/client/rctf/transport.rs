//! Bearer-token login, API-version fallback, and rCTF response handling.

use super::wire::Envelope;
use crate::client::response::ResponseDump;
use crate::client::{CtfdClient, CtfdError, DenyKind};
use serde_json::Value;

/// A parsed rCTF response, good or bad.
pub(super) struct Reply {
    status: reqwest::StatusCode,
    pub(super) envelope: Option<Envelope>,
    method: &'static str,
    url: String,
    final_url: String,
    content_type: Option<String>,
    body: String,
}

impl Reply {
    async fn read(method: &'static str, url: &str, response: reqwest::Response) -> Self {
        let status = response.status();
        let final_url = response.url().to_string();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let body = response.text().await.unwrap_or_default();
        Self {
            status,
            envelope: serde_json::from_str(&body).ok(),
            method,
            url: url.to_string(),
            final_url,
            content_type,
            body,
        }
    }

    pub(super) fn kind(&self) -> &str {
        self.envelope.as_ref().map_or("", |e| e.kind.as_str())
    }

    /// An unknown route: try the older API instead.
    fn missing_route(&self) -> bool {
        self.status == reqwest::StatusCode::NOT_FOUND
            && (self.envelope.is_none() || self.kind() == "badEndpoint")
    }

    fn dump(&self) -> ResponseDump<'_> {
        ResponseDump {
            method: self.method,
            url: &self.url,
            final_url: &self.final_url,
            status: self.status,
            content_type: self.content_type.as_deref(),
            body: &self.body,
        }
    }

    /// The `data` of a `goodXxx` reply, or an error explaining the rest.
    pub(super) fn good(self) -> Result<Value, CtfdError> {
        match self.envelope {
            Some(ref e) if e.kind.starts_with("good") => {
                Ok(self.envelope.and_then(|e| e.data).unwrap_or(Value::Null))
            }
            _ => Err(self.error()),
        }
    }

    pub(super) fn error(&self) -> CtfdError {
        let dump = self.dump();
        let Some(ref e) = self.envelope else {
            let headline = if self.status.is_success() {
                "Invalid or unexpected response format (not rCTF API JSON).".to_string()
            } else {
                format!("rCTF request failed with HTTP {}", self.status)
            };
            return CtfdError::Api(dump.render(&headline));
        };
        let message = e.message.clone().unwrap_or_else(|| e.kind.clone());
        match e.kind.as_str() {
            "badToken" | "badUnknownUser" | "badTokenVerification" => CtfdError::Denied {
                kind: DenyKind::BadToken,
                message: format!(
                    "rCTF did not accept the team token ({message}). Copy the login link \
                     from your rCTF profile page and run `ctfdc init` again."
                ),
            },
            "badNotStarted" | "badEnded" => dump.denied(Some(&message), false),
            "badPerms" => CtfdError::Denied {
                kind: DenyKind::Forbidden,
                message: dump.render(&format!("rCTF refused access: {message}")),
            },
            kind => CtfdError::Api(dump.render(&format!("rCTF returned {kind}: {message}"))),
        }
    }
}

impl CtfdClient {
    /// The Bearer header, logging in with the team token on first use.
    async fn rctf_bearer(&self) -> Result<String, CtfdError> {
        let token = self
            .rctf
            .auth
            .get_or_try_init(|| async { login(self).await })
            .await?;
        Ok(format!("Bearer {token}"))
    }

    async fn rctf_get_once(&self, path: &str, auth: bool) -> Result<Reply, CtfdError> {
        let url = self.api_url(path);
        let bearer = if auth {
            Some(self.rctf_bearer().await?)
        } else {
            None
        };
        let response = self.send_get_with(&url, bearer.as_deref()).await?;
        Ok(Reply::read("GET", &url, response).await)
    }

    /// GET the first of `paths` the server knows (v2 before v1).
    pub(super) async fn rctf_get(&self, paths: &[&str], auth: bool) -> Result<Value, CtfdError> {
        let mut last = None;
        for path in paths {
            let reply = self.rctf_get_once(path, auth).await?;
            if reply.missing_route() {
                last = Some(reply);
                continue;
            }
            return reply.good();
        }
        Err(last.map_or_else(
            || CtfdError::Api("no rCTF route to try".into()),
            |r| r.error(),
        ))
    }

    pub(super) async fn rctf_post(
        &self,
        path: &str,
        body: &Value,
        auth: bool,
    ) -> Result<Reply, CtfdError> {
        let url = self.api_url(path);
        let mut request = self.http.post(&url).json(body);
        if auth {
            request = request.header("Authorization", self.rctf_bearer().await?);
        }
        let response = request.send().await?;
        Ok(Reply::read("POST", &url, response).await)
    }
}

/// Exchange the configured token for an auth token.  The token is normally
/// the team token from the login link; an auth token works as-is.
async fn login(client: &CtfdClient) -> Result<String, CtfdError> {
    let token = client.api_token.clone();
    let url = client.api_url("/api/v1/auth/login");
    let response = client
        .http
        .post(&url)
        .json(&serde_json::json!({ "teamToken": token }))
        .send()
        .await?;
    let reply = Reply::read("POST", &url, response).await;
    if reply.kind() == "goodLogin" {
        let data = reply.good()?;
        return data
            .get("authToken")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| CtfdError::Parse("rCTF login returned no authToken".into()));
    }
    if matches!(
        reply.kind(),
        "badTokenVerification" | "badUnknownUser" | "badBody"
    ) {
        // Maybe it already is an auth token.
        let url = client.api_url("/api/v1/auth/test");
        let response = client
            .send_get_with(&url, Some(&format!("Bearer {token}")))
            .await?;
        if Reply::read("GET", &url, response).await.kind() == "goodToken" {
            return Ok(token);
        }
    }
    Err(reply.error())
}
