//! Standings retrieval and ranking for a selected scoring window.

use super::account_key;
use super::series::parse_time;
use crate::client::{CtfdClient, CtfdError};
use crate::models::{ScoreboardEntry, TopEntry};
use std::collections::HashMap;

/// Fetch the standings, counting only points scored at or after `since`
/// (Unix seconds) when it is set.
///
/// With `since`, the full score history comes from
/// `/api/v1/scoreboard/top/{N}` (N = every ranked account) and the board is
/// re-ranked; see [`rescore`].  The history of every ranked account is
/// returned alongside, already filtered and in the new rank order.
pub async fn fetch_standings(
    client: &CtfdClient,
    since: Option<i64>,
) -> Result<(Vec<ScoreboardEntry>, Option<Vec<TopEntry>>), CtfdError> {
    let standings = client.get_leaderboard().await?;
    let Some(since) = since else {
        return Ok((standings, None));
    };
    if standings.is_empty() {
        return Ok((standings, Some(Vec::new())));
    }
    let count = u32::try_from(standings.len()).unwrap_or(u32::MAX);
    let history = client.get_scoreboard_top(count).await?;
    let (standings, history) = rescore(standings, history, since);
    Ok((standings, Some(history)))
}

/// Re-rank `standings` counting only the points in `history` scored at or
/// after `since`.
///
/// Accounts are matched to their history by ID (by name when CTFd omits
/// IDs); an account without history scores 0.  Ties are broken like CTFd
/// does: whoever reached the score first ranks higher, then by the original
/// order.  Events with unparseable dates are dropped.  Returns the new
/// standings and the filtered history in the same order (accounts without
/// history are left out of it).
pub fn rescore(
    standings: Vec<ScoreboardEntry>,
    history: Vec<TopEntry>,
    since: i64,
) -> (Vec<ScoreboardEntry>, Vec<TopEntry>) {
    let mut by_key: HashMap<String, TopEntry> = history
        .into_iter()
        .map(|mut entry| {
            entry
                .solves
                .retain(|e| parse_time(&e.date).is_some_and(|t| t >= since));
            (account_key(entry.id, &entry.name), entry)
        })
        .collect();

    let mut rows: Vec<(ScoreboardEntry, Option<TopEntry>, i64)> = standings
        .into_iter()
        .map(|mut entry| {
            let hist = by_key.remove(&account_key(entry.account_id, &entry.name));
            let events = hist.as_ref().map(|h| h.solves.as_slice()).unwrap_or(&[]);
            entry.score = events.iter().map(|e| e.value).sum();
            let last = events
                .iter()
                .filter_map(|e| parse_time(&e.date))
                .max()
                .unwrap_or(i64::MAX);
            (entry, hist, last)
        })
        .collect();
    // Stable sort keeps CTFd's order among exact ties.
    rows.sort_by(|a, b| b.0.score.cmp(&a.0.score).then(a.2.cmp(&b.2)));

    let mut standings = Vec::with_capacity(rows.len());
    let mut history = Vec::new();
    for (i, (mut entry, hist, _)) in rows.into_iter().enumerate() {
        entry.pos = i as i64 + 1;
        standings.push(entry);
        history.extend(hist);
    }
    (standings, history)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ScoreEvent;

    fn entry(pos: i64, id: u64, name: &str, score: i64) -> ScoreboardEntry {
        ScoreboardEntry {
            pos,
            name: name.into(),
            score,
            account_id: Some(id),
            account_type: Some("team".into()),
        }
    }

    fn hist(id: u64, name: &str, events: &[(i64, &str)]) -> TopEntry {
        TopEntry {
            id: Some(id),
            name: name.into(),
            solves: events
                .iter()
                .map(|(value, date)| ScoreEvent {
                    challenge_id: None,
                    value: *value,
                    date: (*date).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn ignores_points_before_since_and_reranks() {
        let since = parse_time("2026-11-07T10:00:00Z").unwrap();
        let standings = vec![
            entry(1, 1, "A", 600),
            entry(2, 2, "B", 300),
            entry(3, 3, "C", 100),
        ];
        let history = vec![
            hist(
                1,
                "A",
                &[(500, "2026-11-07T09:00:00Z"), (100, "2026-11-07T11:00:00Z")],
            ),
            hist(2, "B", &[(300, "2026-11-07T10:30:00+00:00")]),
            hist(3, "C", &[(100, "2026-11-07T10:00:00Z")]),
        ];
        let (board, history) = rescore(standings, history, since);
        let got: Vec<(i64, &str, i64)> = board
            .iter()
            .map(|e| (e.pos, e.name.as_str(), e.score))
            .collect();
        // A and C tie on 100; C got there first.
        assert_eq!(got, vec![(1, "B", 300), (2, "C", 100), (3, "A", 100)]);
        let names: Vec<&str> = history.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, vec!["B", "C", "A"]);
        assert_eq!(history[2].solves.len(), 1);
    }

    #[test]
    fn ties_go_to_whoever_got_there_first() {
        let since = 0;
        let standings = vec![entry(1, 1, "A", 100), entry(2, 2, "B", 100)];
        let history = vec![
            hist(1, "A", &[(100, "2026-11-07T12:00:00Z")]),
            hist(2, "B", &[(100, "2026-11-07T11:00:00Z")]),
        ];
        let (board, _) = rescore(standings, history, since);
        assert_eq!(board[0].name, "B");
        assert_eq!(board[1].name, "A");
    }

    #[test]
    fn accounts_without_history_score_zero() {
        let standings = vec![entry(1, 1, "A", 100), entry(2, 2, "B", 50)];
        let history = vec![hist(2, "B", &[(50, "2026-11-07T11:00:00Z")])];
        let (board, _) = rescore(standings, history, 0);
        assert_eq!((board[0].name.as_str(), board[0].score), ("B", 50));
        assert_eq!((board[1].name.as_str(), board[1].score), ("A", 0));
    }
}
