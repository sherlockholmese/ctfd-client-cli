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
    /// Team ID (teams mode) or user ID (users mode).
    #[serde(default)]
    pub account_id: Option<u64>,
    /// `"team"` or `"user"`.
    #[serde(default)]
    pub account_type: Option<String>,
}

/// One account from `GET /api/v1/scoreboard/top/{count}`, with the solve
/// (and award) history used to draw the score graph.
#[derive(Debug, Deserialize, Clone)]
pub struct TopEntry {
    /// Team or user ID.
    #[serde(default)]
    pub id: Option<u64>,
    /// Display name.
    pub name: String,
    /// Point-gaining events, in any order.
    #[serde(default)]
    pub solves: Vec<ScoreEvent>,
}

/// A dated score change (solve or award).
#[derive(Debug, Deserialize, Clone)]
pub struct ScoreEvent {
    /// The solved challenge (absent for awards).
    #[serde(default)]
    pub challenge_id: Option<u64>,
    /// Points gained (awards may be negative).
    #[serde(default)]
    pub value: i64,
    /// ISO 8601 timestamp.
    pub date: String,
}

/// A solve from `GET /api/v1/{teams|users}/{id}/solves`.
#[derive(Debug, Deserialize, Clone)]
pub struct AccountSolve {
    /// The solved challenge's ID.
    #[serde(default)]
    pub challenge_id: Option<u64>,
    /// ISO 8601 timestamp.
    pub date: String,
    /// The solved challenge (for its point value).
    #[serde(default)]
    pub challenge: Option<SolvedChallenge>,
}

/// Minimal challenge info embedded in an [`AccountSolve`].
#[derive(Debug, Deserialize, Clone)]
pub struct SolvedChallenge {
    /// Challenge ID.
    #[serde(default)]
    pub id: Option<u64>,
    /// Point value.
    #[serde(default)]
    pub value: i64,
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
    /// Number of accounts that solved it (CTFd 3.x reports this).
    #[serde(default)]
    pub solves: Option<u64>,
}

/// Detailed information about a single challenge.
///
/// Returned by `GET /api/v1/challenges/{id}`. Contains the full
/// description, connection info, attachments, tags, and other metadata
/// needed for the download command.
#[derive(Debug, Deserialize, Clone)]
pub struct ChallengeDetail {
    /// Numeric challenge ID.
    pub id: u64,
    /// User-facing challenge name.
    pub name: String,
    /// Category (e.g. "Web", "Crypto", "Forensics").
    pub category: String,
    /// Point value.
    pub value: i64,
    /// Challenge description (markdown).
    pub description: String,
    /// Optional connection info string (e.g. `nc host port`).
    #[serde(default)]
    pub connection_info: Option<String>,
    /// Challenge type (e.g. "standard", "dynamic").
    #[serde(rename = "type", default)]
    pub challenge_type: Option<String>,
    /// Visibility state (e.g. "visible", "hidden").
    #[serde(default)]
    pub state: Option<String>,
    /// Whether the authenticated user has solved this challenge.
    #[serde(rename = "solved_by_me", default)]
    pub solved_by_me: bool,
    /// Attachment file URLs (may be relative or absolute).
    #[serde(default)]
    pub files: Option<Vec<String>>,
    /// Challenge tags.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Maximum number of flag attempts.
    #[serde(default)]
    pub max_attempts: Option<i64>,
}

/// The authenticated user, from `GET /api/v1/users/me`.
#[derive(Debug, Deserialize, Clone)]
pub struct UserMe {
    /// Display name.
    pub name: String,
    /// User ID.
    #[serde(default)]
    pub id: Option<u64>,
    /// Team ID (teams mode).
    #[serde(default)]
    pub team_id: Option<u64>,
}

/// Response data from a flag submission attempt.
///
/// Returned by `POST /api/v1/challenges/attempt`. The `status` field
/// indicates the submission outcome.
#[derive(Debug, Deserialize, Clone)]
pub struct SubmitResponse {
    /// Submission outcome: "correct", "incorrect", or "already_solved".
    pub status: String,
    /// Human-readable message from the server.
    #[serde(default)]
    pub message: Option<String>,
}

/// Classification of a flag submission result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitOutcome {
    /// The flag was accepted (correct).
    Accepted,
    /// The flag was rejected (incorrect).
    Rejected,
    /// The challenge was already solved by this user.
    AlreadySolved,
    /// CTFd refused the attempt because flags are submitted too fast.
    /// The attempt was not counted, so it is safe to resend.
    RateLimited,
    /// The CTF is paused; no submissions are accepted.
    Paused,
}

impl SubmitResponse {
    /// Classify the submission status into a high-level outcome.
    pub fn outcome(&self) -> SubmitOutcome {
        match self.status.as_str() {
            "correct" => SubmitOutcome::Accepted,
            "already_solved" => SubmitOutcome::AlreadySolved,
            "ratelimited" => SubmitOutcome::RateLimited,
            "paused" => SubmitOutcome::Paused,
            _ => SubmitOutcome::Rejected,
        }
    }

    /// A user-facing message describing the outcome.
    pub fn user_message(&self) -> String {
        match self.outcome() {
            SubmitOutcome::Accepted => "Flag accepted! Challenge solved.".to_string(),
            SubmitOutcome::Rejected => {
                if let Some(ref msg) = self.message {
                    format!("Flag rejected: {msg}")
                } else {
                    "Flag rejected (incorrect).".to_string()
                }
            }
            SubmitOutcome::AlreadySolved => "Challenge already solved.".to_string(),
            SubmitOutcome::RateLimited => match self.message {
                Some(ref msg) => format!("Rate limited: {msg}"),
                None => "Rate limited: submitting flags too fast.".to_string(),
            },
            SubmitOutcome::Paused => match self.message {
                Some(ref msg) => format!("Submissions are closed: {msg}"),
                None => "Submissions are closed: the CTF is paused.".to_string(),
            },
        }
    }
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

    // --- ChallengeDetail parsing ---

    #[test]
    fn parse_challenge_detail_full() {
        let json = r#"{
            "id": 1,
            "name": "Baby Login",
            "category": "Web",
            "value": 100,
            "description": "Login as admin.\n\nFind the password.",
            "connection_info": "nc example.com 1337",
            "type": "standard",
            "state": "visible",
            "solved_by_me": false,
            "files": ["/files/abc123/handout.zip"],
            "tags": ["web", "easy"],
            "max_attempts": 10
        }"#;
        let detail: ChallengeDetail = serde_json::from_str(json).expect("should parse");
        assert_eq!(detail.id, 1);
        assert_eq!(detail.name, "Baby Login");
        assert_eq!(detail.category, "Web");
        assert_eq!(detail.value, 100);
        assert_eq!(detail.description, "Login as admin.\n\nFind the password.");
        assert_eq!(
            detail.connection_info.as_deref(),
            Some("nc example.com 1337")
        );
        assert_eq!(detail.challenge_type.as_deref(), Some("standard"));
        assert_eq!(detail.state.as_deref(), Some("visible"));
        assert!(!detail.solved_by_me);
        assert_eq!(
            detail.files.as_deref(),
            Some(&["/files/abc123/handout.zip".to_string()][..])
        );
        assert_eq!(
            detail.tags.as_deref(),
            Some(&["web".to_string(), "easy".to_string()][..])
        );
        assert_eq!(detail.max_attempts, Some(10));
    }

    #[test]
    fn parse_challenge_detail_minimal() {
        // Only required fields.
        let json = r#"{
            "id": 2,
            "name": "RSA 101",
            "category": "Crypto",
            "value": 200,
            "description": "Break RSA"
        }"#;
        let detail: ChallengeDetail = serde_json::from_str(json).expect("should parse");
        assert_eq!(detail.id, 2);
        assert_eq!(detail.name, "RSA 101");
        assert_eq!(detail.category, "Crypto");
        assert_eq!(detail.value, 200);
        assert_eq!(detail.description, "Break RSA");
        assert!(detail.connection_info.is_none());
        assert!(detail.challenge_type.is_none());
        assert!(detail.state.is_none());
        assert!(!detail.solved_by_me);
        assert!(detail.files.is_none());
        assert!(detail.tags.is_none());
        assert!(detail.max_attempts.is_none());
    }

    #[test]
    fn parse_challenge_detail_with_multiple_files() {
        let json = r#"{
            "id": 3,
            "name": "Multi File",
            "category": "Forensics",
            "value": 300,
            "description": "Analyze",
            "files": ["/files/dump.pcap", "https://cdn.example.com/hint.txt"]
        }"#;
        let detail: ChallengeDetail = serde_json::from_str(json).expect("should parse");
        let files = detail.files.unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0], "/files/dump.pcap");
        assert_eq!(files[1], "https://cdn.example.com/hint.txt");
    }

    #[test]
    fn parse_challenge_detail_empty_files_array() {
        let json = r#"{
            "id": 4,
            "name": "No Files",
            "category": "Misc",
            "value": 50,
            "description": "No attachments",
            "files": []
        }"#;
        let detail: ChallengeDetail = serde_json::from_str(json).expect("should parse");
        let files = detail.files.unwrap();
        assert!(files.is_empty());
    }

    // --- SubmitResponse parsing ---

    #[test]
    fn parse_submit_correct() {
        let json = r#"{"status":"correct","message":"Correct!"}"#;
        let resp: SubmitResponse = serde_json::from_str(json).expect("should parse");
        assert_eq!(resp.status, "correct");
        assert_eq!(resp.message.as_deref(), Some("Correct!"));
        assert_eq!(resp.outcome(), SubmitOutcome::Accepted);
        assert!(resp.user_message().contains("accepted"));
    }

    #[test]
    fn parse_submit_incorrect() {
        let json = r#"{"status":"incorrect","message":"Incorrect!"}"#;
        let resp: SubmitResponse = serde_json::from_str(json).expect("should parse");
        assert_eq!(resp.status, "incorrect");
        assert_eq!(resp.outcome(), SubmitOutcome::Rejected);
        assert!(resp.user_message().contains("rejected"));
    }

    #[test]
    fn parse_submit_already_solved() {
        let json = r#"{"status":"already_solved","message":"You already solved this."}"#;
        let resp: SubmitResponse = serde_json::from_str(json).expect("should parse");
        assert_eq!(resp.status, "already_solved");
        assert_eq!(resp.outcome(), SubmitOutcome::AlreadySolved);
        assert!(resp.user_message().contains("already solved"));
    }

    #[test]
    fn parse_submit_missing_message() {
        let json = r#"{"status":"incorrect"}"#;
        let resp: SubmitResponse = serde_json::from_str(json).expect("should parse");
        assert_eq!(resp.outcome(), SubmitOutcome::Rejected);
        assert!(resp.message.is_none());
    }
}
