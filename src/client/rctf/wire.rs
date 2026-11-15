//! rCTF JSON shapes. Optional fields accommodate both API versions.

use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

/// rCTF IDs are strings, but accept numbers too.
fn de_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::String(s) => s,
        other => other.to_string(),
    })
}

#[derive(Debug, Deserialize)]
pub(super) struct Envelope {
    pub(super) kind: String,
    #[serde(default)]
    pub(super) message: Option<String>,
    #[serde(default)]
    pub(super) data: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RctfChallenge {
    #[serde(deserialize_with = "de_id")]
    pub(super) id: String,
    pub(super) name: String,
    #[serde(default)]
    pub(super) description: String,
    #[serde(default)]
    pub(super) category: String,
    #[serde(default)]
    pub(super) author: String,
    #[serde(default)]
    pub(super) files: Vec<Attachment>,
    #[serde(default)]
    pub(super) points: Option<i64>,
    #[serde(default)]
    pub(super) solves: Option<u64>,
    #[serde(default)]
    pub(super) tags: Option<Vec<String>>,
    #[serde(default, rename = "yourScore")]
    pub(super) your_score: Option<i64>,
    #[serde(default, rename = "scoringKind")]
    pub(super) scoring_kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct Attachment {
    #[serde(default)]
    pub(super) name: String,
    pub(super) url: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct Profile {
    #[serde(default, deserialize_with = "de_opt_id")]
    pub(super) id: Option<String>,
    pub(super) name: String,
    #[serde(default)]
    pub(super) solves: Vec<ProfileSolve>,
}

fn de_opt_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(match Option::<Value>::deserialize(d)? {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s),
        Some(other) => Some(other.to_string()),
    })
}

#[derive(Debug, Deserialize)]
pub(super) struct ProfileSolve {
    #[serde(deserialize_with = "de_id")]
    pub(super) id: String,
    #[serde(default)]
    pub(super) name: String,
    #[serde(default)]
    pub(super) category: String,
    #[serde(default)]
    pub(super) points: Option<i64>,
    #[serde(default, rename = "awardedPoints")]
    pub(super) awarded_points: Option<i64>,
    #[serde(rename = "createdAt")]
    pub(super) created_at: i64,
}

#[derive(Debug, Deserialize)]
pub(super) struct LeaderboardPage {
    #[serde(default)]
    pub(super) total: Option<usize>,
    pub(super) leaderboard: Vec<LeaderboardEntry>,
}

#[derive(Debug, Deserialize)]
pub(super) struct LeaderboardEntry {
    #[serde(deserialize_with = "de_id")]
    pub(super) id: String,
    pub(super) name: String,
    #[serde(default)]
    pub(super) score: i64,
    #[serde(default, rename = "globalPlace")]
    pub(super) global_place: Option<i64>,
    /// v2 only.
    #[serde(default)]
    pub(super) solves: Option<Vec<LeaderboardSolve>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct LeaderboardSolve {
    #[serde(deserialize_with = "de_id")]
    pub(super) id: String,
    #[serde(rename = "solveTime")]
    pub(super) solve_time: i64,
}

#[derive(Debug, Deserialize)]
pub(super) struct Graph {
    pub(super) graph: Vec<GraphEntry>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GraphEntry {
    #[serde(deserialize_with = "de_id")]
    pub(super) id: String,
    pub(super) name: String,
    #[serde(default)]
    pub(super) points: Vec<GraphPoint>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GraphPoint {
    pub(super) time: i64,
    pub(super) score: i64,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChallengePoints {
    pub(super) challenges: HashMap<String, ChallengePointValue>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChallengePointValue {
    #[serde(default)]
    pub(super) points: Option<i64>,
}
