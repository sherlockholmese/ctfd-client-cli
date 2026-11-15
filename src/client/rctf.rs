//! rCTF (v1 and v2 API) behind the CTFd-shaped [`CtfdClient`] methods.
//!
//! rCTF answers `{ "kind": "goodXxx" | "badXxx", "message", "data" }`,
//! authenticates with `Authorization: Bearer <auth token>` (exchanged from
//! the team token in the login link), uses string IDs, and reports times in
//! milliseconds.  Each function here fetches the rCTF route (the v2 one
//! where it exists, else v1) and converts it to the models the commands
//! already use:
//!
//! - IDs become stable numbers ([`numeric_id`]); the client remembers the
//!   original string for routes that need it.
//! - Times become RFC 3339 strings, as CTFd sends them.
//! - Submission outcomes map to CTFd's `correct` / `incorrect` / ... .

mod transport;
mod wire;

use super::{CtfdClient, CtfdError};
use crate::models::{
    AccountSolve, Challenge, ChallengeDetail, ScoreEvent, ScoreboardEntry, SolvedChallenge,
    SubmitResponse, TopEntry, UserMe,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use wire::{
    Attachment, ChallengePoints, Envelope, Graph, LeaderboardEntry, LeaderboardPage, Profile,
    RctfChallenge,
};

/// Leaderboard page size; rCTF caps it per deployment, so pages may come
/// back shorter.
const LEADERBOARD_PAGE_SIZE: usize = 100;
/// Safety stop for leaderboard paging.
const MAX_LEADERBOARD_PAGES: usize = 200;

/// Per-client rCTF state: the auth token and string IDs seen so far.
#[derive(Default)]
pub(super) struct State {
    auth: tokio::sync::OnceCell<String>,
    challenge_ids: Mutex<HashMap<u64, String>>,
    team_ids: Mutex<HashMap<u64, String>>,
    challenges: Mutex<Option<Vec<RctfChallenge>>>,
}

/// A stable numeric ID for an rCTF string ID: the number itself when the
/// ID is a small number, else a 9-digit hash (FNV-1a), so `ctfdc s 123456789`
/// and metadata.json keep working across runs.
pub fn numeric_id(id: &str) -> u64 {
    if let Ok(n) = id.parse::<u64>()
        && n < 100_000_000
    {
        return n;
    }
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    100_000_000 + hash % 900_000_000
}

fn remember(map: &Mutex<HashMap<u64, String>>, id: &str) -> u64 {
    let n = numeric_id(id);
    if let Ok(mut map) = map.lock() {
        map.entry(n).or_insert_with(|| id.to_string());
    }
    n
}

fn lookup(map: &Mutex<HashMap<u64, String>>, n: u64) -> Option<String> {
    map.lock().ok()?.get(&n).cloned()
}

/// Milliseconds since the epoch as RFC 3339.
fn timestamp_from_millis(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .unwrap_or_default()
        .to_rfc3339()
}

fn parse<T: serde::de::DeserializeOwned>(what: &str, value: Value) -> Result<T, CtfdError> {
    serde_json::from_value(value)
        .map_err(|e| CtfdError::Parse(format!("Unexpected rCTF {what} response: {e}")))
}

/// Probe the public client config, without retrying.
pub(super) async fn detect(base_url: &str) -> bool {
    let Ok(http) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .user_agent(concat!("ctfdc/", env!("CARGO_PKG_VERSION")))
        .build()
    else {
        return false;
    };
    let url = format!("{base_url}/api/v1/integrations/client/config");
    let Ok(response) = http.get(&url).send().await else {
        return false;
    };
    let Ok(envelope) = response.json::<Envelope>().await else {
        return false;
    };
    envelope.kind.starts_with("goodClientConfig")
}

async fn profile_me(client: &CtfdClient) -> Result<Profile, CtfdError> {
    let data = client
        .rctf_get(&["/api/v2/users/me", "/api/v1/users/me"], true)
        .await?;
    parse("profile", data)
}

pub(super) async fn me(client: &CtfdClient) -> Result<UserMe, CtfdError> {
    let me = profile_me(client).await?;
    Ok(UserMe {
        name: me.name,
        id: None,
        team_id: me
            .id
            .as_deref()
            .map(|id| remember(&client.rctf.team_ids, id)),
    })
}

async fn challenge_list(client: &CtfdClient) -> Result<Vec<RctfChallenge>, CtfdError> {
    let data = client
        .rctf_get(&["/api/v2/challs", "/api/v1/challs"], true)
        .await?;
    let list: Vec<RctfChallenge> = parse("challenge list", data)?;
    for c in &list {
        remember(&client.rctf.challenge_ids, &c.id);
    }
    if let Ok(mut cache) = client.rctf.challenges.lock() {
        *cache = Some(list.clone());
    }
    Ok(list)
}

async fn cached_challenges(client: &CtfdClient) -> Result<Vec<RctfChallenge>, CtfdError> {
    let cached = client.rctf.challenges.lock().ok().and_then(|c| c.clone());
    match cached {
        Some(list) => Ok(list),
        None => challenge_list(client).await,
    }
}

pub(super) async fn challenges(client: &CtfdClient) -> Result<Vec<Challenge>, CtfdError> {
    let (list, me) = tokio::join!(challenge_list(client), profile_me(client));
    let list = list?;
    let solved: HashSet<String> = me
        .map(|me| me.solves.into_iter().map(|s| s.id).collect())
        .unwrap_or_default();
    Ok(list
        .into_iter()
        .map(|c| Challenge {
            id: numeric_id(&c.id),
            solved_by_me: c.your_score.is_some() || solved.contains(&c.id),
            name: c.name,
            category: c.category,
            value: c.points.unwrap_or(0),
            solves: c.solves,
            tags: c.tags.unwrap_or_default(),
        })
        .collect())
}

pub(super) async fn challenge_detail(
    client: &CtfdClient,
    challenge_id: u64,
) -> Result<ChallengeDetail, CtfdError> {
    let list = cached_challenges(client).await?;
    let c = list
        .into_iter()
        .find(|c| numeric_id(&c.id) == challenge_id)
        .ok_or_else(|| CtfdError::Api(format!("rCTF has no challenge #{challenge_id}")))?;
    let mut description = c.description;
    if !c.author.is_empty() {
        description = format!("{description}\n\nAuthor: {}", c.author);
    }
    Ok(ChallengeDetail {
        id: challenge_id,
        name: c.name,
        category: c.category,
        value: c.points.unwrap_or(0),
        description,
        connection_info: None,
        challenge_type: c.scoring_kind,
        state: None,
        solved_by_me: c.your_score.is_some(),
        files: Some(c.files.into_iter().map(|f| file_url(&f)).collect()),
        tags: c.tags,
        max_attempts: None,
    })
}

/// rCTF names files separately from their (often hashed) URLs; keep the
/// name as the URL's last segment so downloads are saved under it.
fn file_url(f: &Attachment) -> String {
    let last = f
        .url
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('/')
        .next()
        .unwrap_or_default();
    if f.name.is_empty() || last == f.name || f.url.contains('?') {
        return f.url.clone();
    }
    format!("{}#{}", f.url, f.name)
}

pub(super) async fn submit(
    client: &CtfdClient,
    challenge_id: u64,
    flag: &str,
) -> Result<SubmitResponse, CtfdError> {
    let id = match lookup(&client.rctf.challenge_ids, challenge_id) {
        Some(id) => id,
        None => {
            challenge_list(client).await?;
            lookup(&client.rctf.challenge_ids, challenge_id)
                .ok_or_else(|| CtfdError::Api(format!("rCTF has no challenge #{challenge_id}")))?
        }
    };
    let path = format!("/api/v1/challs/{}/submit", urlencode(&id));
    let reply = client
        .rctf_post(&path, &serde_json::json!({ "flag": flag }), true)
        .await?;
    let message = reply.envelope.as_ref().and_then(|e| e.message.clone());
    let status = match reply.kind() {
        "goodFlag" => "correct",
        "badFlag" => "incorrect",
        "badAlreadySolvedChallenge" => "already_solved",
        "badRateLimit" => {
            let wait = reply
                .envelope
                .as_ref()
                .and_then(|e| e.data.as_ref())
                .and_then(|d| d.get("timeLeft"))
                .and_then(Value::as_i64)
                .map(|ms| format!("try again in {}s", (ms + 999) / 1000));
            return Ok(SubmitResponse {
                status: "ratelimited".into(),
                message: wait.or(message),
            });
        }
        "badEnded" | "badNotStarted" => "paused",
        _ => return Err(reply.error()),
    };
    Ok(SubmitResponse {
        status: status.into(),
        message,
    })
}

fn urlencode(id: &str) -> String {
    id.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Leaderboard entries from the top, up to `want` (all when `None`).
async fn leaderboard_entries(
    client: &CtfdClient,
    want: Option<usize>,
) -> Result<Vec<LeaderboardEntry>, CtfdError> {
    let mut out: Vec<LeaderboardEntry> = Vec::new();
    for _ in 0..MAX_LEADERBOARD_PAGES {
        let limit = want.map_or(LEADERBOARD_PAGE_SIZE, |w| {
            (w - out.len()).min(LEADERBOARD_PAGE_SIZE)
        });
        let query = format!("limit={limit}&offset={}", out.len());
        let data = client
            .rctf_get(
                &[
                    &format!("/api/v2/leaderboard/now?{query}"),
                    &format!("/api/v1/leaderboard/now?{query}"),
                ],
                false,
            )
            .await?;
        let page: LeaderboardPage = parse("leaderboard", data)?;
        let offset = out.len();
        let got = page.leaderboard.len();
        out.extend(page.leaderboard.into_iter().enumerate().map(|(i, mut e)| {
            e.global_place = e.global_place.or(Some((offset + i + 1) as i64));
            e
        }));
        let total = page.total.unwrap_or(out.len());
        if got == 0 || out.len() >= total || want.is_some_and(|w| out.len() >= w) {
            break;
        }
    }
    for e in &out {
        remember(&client.rctf.team_ids, &e.id);
    }
    Ok(out)
}

pub(super) async fn leaderboard(client: &CtfdClient) -> Result<Vec<ScoreboardEntry>, CtfdError> {
    Ok(leaderboard_entries(client, None)
        .await?
        .into_iter()
        .enumerate()
        .map(|(i, e)| ScoreboardEntry {
            pos: e.global_place.unwrap_or(i as i64 + 1),
            account_id: Some(numeric_id(&e.id)),
            account_type: Some("team".into()),
            name: e.name,
            score: e.score,
        })
        .collect())
}

/// The top `count` with their solves: from the v2 leaderboard (which lists
/// each team's solves) priced at current challenge points, else from the
/// v1 score graph (no challenge per step).
pub(super) async fn top(client: &CtfdClient, count: u32) -> Result<Vec<TopEntry>, CtfdError> {
    let entries = leaderboard_entries(client, Some(count as usize)).await?;
    if entries.iter().any(|e| e.solves.is_some()) {
        let points = challenge_points(client).await;
        return Ok(entries
            .into_iter()
            .map(|e| TopEntry {
                id: Some(numeric_id(&e.id)),
                name: e.name,
                solves: e
                    .solves
                    .unwrap_or_default()
                    .into_iter()
                    .map(|s| ScoreEvent {
                        challenge_id: Some(remember(&client.rctf.challenge_ids, &s.id)),
                        value: points.get(&s.id).copied().unwrap_or(0),
                        date: timestamp_from_millis(s.solve_time),
                    })
                    .collect(),
            })
            .collect());
    }

    let data = client
        .rctf_get(
            &[
                &format!("/api/v2/leaderboard/graph?limit={count}&offset=0"),
                &format!("/api/v1/leaderboard/graph?limit={count}"),
            ],
            false,
        )
        .await?;
    let graph: Graph = parse("graph", data)?;
    Ok(graph
        .graph
        .into_iter()
        .map(|g| {
            let mut points = g.points;
            points.sort_by_key(|p| p.time);
            let mut prev = 0;
            let solves = points
                .into_iter()
                .filter_map(|p| {
                    let delta = p.score - prev;
                    prev = p.score;
                    (delta != 0).then(|| ScoreEvent {
                        challenge_id: None,
                        value: delta,
                        date: timestamp_from_millis(p.time),
                    })
                })
                .collect();
            TopEntry {
                id: Some(remember(&client.rctf.team_ids, &g.id)),
                name: g.name,
                solves,
            }
        })
        .collect())
}

/// Current points per challenge (string ID): from the leaderboard's
/// challenge list (visible whenever the leaderboard is), else the
/// challenge list.
async fn challenge_points(client: &CtfdClient) -> HashMap<String, i64> {
    if let Ok(data) = client
        .rctf_get(&["/api/v2/leaderboard/challs"], false)
        .await
        && let Ok(list) = parse::<ChallengePoints>("leaderboard challenges", data)
    {
        return list
            .challenges
            .into_iter()
            .map(|(id, c)| (id, c.points.unwrap_or(0)))
            .collect();
    }
    cached_challenges(client)
        .await
        .map(|list| {
            list.into_iter()
                .map(|c| (c.id, c.points.unwrap_or(0)))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) async fn account_solves(
    client: &CtfdClient,
    account_id: u64,
) -> Result<Vec<AccountSolve>, CtfdError> {
    let id = lookup(&client.rctf.team_ids, account_id)
        .ok_or_else(|| CtfdError::Api(format!("unknown rCTF team #{account_id}")))?;
    let id = urlencode(&id);
    let data = client
        .rctf_get(
            &[
                &format!("/api/v2/users/{id}"),
                &format!("/api/v1/users/{id}"),
            ],
            false,
        )
        .await?;
    let profile: Profile = parse("profile", data)?;
    Ok(profile
        .solves
        .into_iter()
        .map(|s| {
            let cid = remember(&client.rctf.challenge_ids, &s.id);
            AccountSolve {
                challenge_id: Some(cid),
                date: timestamp_from_millis(s.created_at),
                challenge: Some(SolvedChallenge {
                    id: Some(cid),
                    value: s.awarded_points.or(s.points).unwrap_or(0),
                    name: (!s.name.is_empty()).then_some(s.name),
                    category: Some(s.category),
                }),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_ids_are_stable() {
        assert_eq!(numeric_id("42"), 42);
        let a = numeric_id("baby-rev");
        assert_eq!(a, numeric_id("baby-rev"));
        assert_ne!(a, numeric_id("baby-pwn"));
        assert!((100_000_000..1_000_000_000).contains(&a));
        assert!(numeric_id("123456789012") >= 100_000_000);
    }

    #[test]
    fn converts_milliseconds() {
        assert_eq!(timestamp_from_millis(60_000), "1970-01-01T00:01:00+00:00");
    }

    #[test]
    fn keeps_file_names() {
        let f = |name: &str, url: &str| Attachment {
            name: name.into(),
            url: url.into(),
        };
        assert_eq!(
            file_url(&f("handout.zip", "https://cdn/x/handout.zip")),
            "https://cdn/x/handout.zip"
        );
        assert_eq!(
            file_url(&f("handout.zip", "https://cdn/uploads/abc123")),
            "https://cdn/uploads/abc123#handout.zip"
        );
        assert_eq!(urlencode("a b/c"), "a%20b%2Fc");
    }
}
