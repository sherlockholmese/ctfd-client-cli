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
}
