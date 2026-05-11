use serde::Deserialize;

/// Generic CTFd API response envelope.
///
/// CTFd API responses wrap their data in a top-level JSON object with
/// `success` (bool), `data` (payload when success is true), and optional
/// `errors` (string array when success is false).
#[derive(Debug, Deserialize)]
pub struct CtfdResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    #[serde(default)]
    pub errors: Option<Vec<String>>,
}

impl<T> CtfdResponse<T> {
    /// Extract a user-facing error message from an unsuccessful envelope.
    pub fn error_message(&self) -> String {
        match &self.errors {
            Some(errs) if !errs.is_empty() => errs.join("; "),
            _ => "unknown API error".to_string(),
        }
    }
}

/// A single ranked entry from the CTFd scoreboard.
#[derive(Debug, Deserialize, Clone)]
pub struct ScoreboardEntry {
    /// Position / rank (1-based).
    pub pos: i64,
    /// Team or user display name.
    pub name: String,
    /// Numeric score.
    pub score: i64,
}

/// A visible challenge from the CTFd challenge list.
///
/// Returned by `GET /api/v1/challenges`.  Uses `#[serde(rename)]` to map
/// the snake_case CTFd field name `solved_by_me` to the struct field.
#[derive(Debug, Deserialize, Clone)]
pub struct Challenge {
    /// Numeric challenge ID.
    #[allow(dead_code)]
    pub id: u64,
    /// User-facing challenge name.
    pub name: String,
    /// Category (e.g. "Web", "Crypto", "Forensics").
    pub category: String,
    /// Point value.
    pub value: i64,
    /// Whether the authenticated user has solved this challenge.
    #[serde(rename = "solved_by_me")]
    pub solved_by_me: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- CtfdResponse parsing ---

    #[test]
    fn parse_success_envelope_with_data() {
        let json = r#"{"success":true,"data":[{"pos":1,"name":"Alpha","score":5000}]}"#;
        let resp: CtfdResponse<Vec<ScoreboardEntry>> =
            serde_json::from_str(json).expect("should parse");
        assert!(resp.success);
        let entries = resp.data.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].pos, 1);
        assert_eq!(entries[0].name, "Alpha");
        assert_eq!(entries[0].score, 5000);
    }

    #[test]
    fn parse_error_envelope_with_errors() {
        let json = r#"{"success":false,"errors":["Invalid token"]}"#;
        let resp: CtfdResponse<Vec<ScoreboardEntry>> =
            serde_json::from_str(json).expect("should parse");
        assert!(!resp.success);
        assert_eq!(resp.error_message(), "Invalid token");
    }

    #[test]
    fn parse_error_envelope_without_errors_field() {
        let json = r#"{"success":false}"#;
        let resp: CtfdResponse<Vec<ScoreboardEntry>> =
            serde_json::from_str(json).expect("should parse");
        assert!(!resp.success);
        assert_eq!(resp.error_message(), "unknown API error");
    }

    #[test]
    fn parse_error_envelope_with_empty_errors() {
        let json = r#"{"success":false,"errors":[]}"#;
        let resp: CtfdResponse<Vec<ScoreboardEntry>> =
            serde_json::from_str(json).expect("should parse");
        assert!(!resp.success);
        assert_eq!(resp.error_message(), "unknown API error");
    }

    // --- Challenge parsing ---

    #[test]
    fn parse_challenge_with_solved_by_me() {
        let json = r#"{"success":true,"data":[
            {"id":1,"name":"Baby Login","value":100,"category":"Web","solved_by_me":false},
            {"id":2,"name":"RSA 101","value":200,"category":"Crypto","solved_by_me":true}
        ]}"#;
        let resp: CtfdResponse<Vec<Challenge>> = serde_json::from_str(json).expect("should parse");
        assert!(resp.success);
        let challenges = resp.data.unwrap();
        assert_eq!(challenges.len(), 2);

        assert_eq!(challenges[0].id, 1);
        assert_eq!(challenges[0].name, "Baby Login");
        assert_eq!(challenges[0].category, "Web");
        assert_eq!(challenges[0].value, 100);
        assert!(!challenges[0].solved_by_me);

        assert_eq!(challenges[1].id, 2);
        assert_eq!(challenges[1].name, "RSA 101");
        assert_eq!(challenges[1].category, "Crypto");
        assert_eq!(challenges[1].value, 200);
        assert!(challenges[1].solved_by_me);
    }

    #[test]
    fn parse_empty_challenge_list() {
        let json = r#"{"success":true,"data":[]}"#;
        let resp: CtfdResponse<Vec<Challenge>> = serde_json::from_str(json).expect("should parse");
        assert!(resp.success);
        let challenges = resp.data.unwrap();
        assert!(challenges.is_empty());
    }

    #[test]
    fn parse_challenge_missing_optional_fields() {
        // CTFd may omit some fields; verify serde handles missing fields gracefully.
        let json = r#"{"success":true,"data":[{"id":1,"name":"Minimal","category":"Misc","value":0,"solved_by_me":false}]}"#;
        let resp: CtfdResponse<Vec<Challenge>> = serde_json::from_str(json).expect("should parse");
        assert!(resp.success);
        let challenges = resp.data.unwrap();
        assert_eq!(challenges[0].id, 1);
        assert_eq!(challenges[0].name, "Minimal");
    }
}
