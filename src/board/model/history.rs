//! Rank and score movement, graph series, and the recent-solve feed.

use super::{BoardModel, CHANGE_WINDOW, account_key};
use crate::board::series;
use crate::models::{Challenge, ScoreboardEntry};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// How long a new solve stays highlighted in the live feed.
const FRESH_TTL: Duration = Duration::from_secs(45);
/// Live feed length.
const FEED_LEN: usize = 60;
/// A solve older than this (seconds) is history, not news.
const NEWS_SECS: i64 = 300;

/// A dated score change of one account.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoreChange {
    pub t: i64,
    pub value: i64,
    pub challenge_id: Option<u64>,
}

/// One line on the score graph.
#[derive(Debug, Clone)]
pub struct SeriesLine {
    pub account: Option<u64>,
    pub name: String,
    /// Series color slot, by rank among graphed accounts (`None`: you).
    pub slot: Option<usize>,
    pub is_me: bool,
    pub points: Vec<(f64, f64)>,
}

/// One solve in the live feed.
#[derive(Debug, Clone)]
pub struct FeedItem {
    pub t: i64,
    pub account: u64,
    pub team: String,
    pub challenge: String,
    pub category: String,
    pub value: i64,
    pub is_me: bool,
    pub fresh: bool,
}

/// A rank/score movement seen between refreshes.
#[derive(Debug, Clone, Copy)]
pub struct Change {
    pub rank_delta: i64,
    pub score_delta: i64,
    pub at: Instant,
}

impl BoardModel {
    /// Record a standings snapshot and recompute each account's rank and
    /// score change over the last [`CHANGE_WINDOW`].
    pub(super) fn track_changes(&mut self, new: &[ScoreboardEntry]) {
        let now = Instant::now();
        let current: HashMap<String, (i64, i64)> = new
            .iter()
            .map(|e| (account_key(e.account_id, &e.name), (e.pos, e.score)))
            .collect();
        if let Some((_, previous)) = self.snapshots.back() {
            for (key, value) in &current {
                if previous.get(key).is_some_and(|old| old != value) {
                    self.last_moved.insert(key.clone(), now);
                }
            }
        }
        self.snapshots.push_back((now, current));
        // Keep one snapshot at or before the window start as the baseline.
        while self.snapshots.len() > 1 && self.snapshots[1].0 + CHANGE_WINDOW <= now {
            self.snapshots.pop_front();
        }

        self.changes.clear();
        let (Some((_, base)), Some((_, latest))) = (self.snapshots.front(), self.snapshots.back())
        else {
            return;
        };
        for (key, &(pos, score)) in latest {
            if let Some(&(old_pos, old_score)) = base.get(key)
                && (old_pos != pos || old_score != score)
            {
                self.changes.insert(
                    key.clone(),
                    Change {
                        rank_delta: old_pos - pos,
                        score_delta: score - old_score,
                        at: self.last_moved.get(key).copied().unwrap_or(now),
                    },
                );
            }
        }
    }

    pub(super) fn name_of(&self, id: u64) -> String {
        self.data
            .standings
            .iter()
            .find(|e| e.account_id == Some(id))
            .map(|e| e.name.clone())
            .or_else(|| {
                self.data
                    .top
                    .iter()
                    .find(|t| t.id == Some(id))
                    .map(|t| t.name.clone())
            })
            .unwrap_or_else(|| format!("#{id}"))
    }

    pub(super) fn rebuild_lines(&mut self) {
        let mut accounts: Vec<(Option<u64>, String)> = self
            .data
            .top
            .iter()
            .take(self.top_n as usize)
            .map(|t| (t.id, t.name.clone()))
            .collect();
        let shown: HashSet<u64> = accounts.iter().filter_map(|(id, _)| *id).collect();
        let mut extra: Vec<u64> = self.pins.clone();
        extra.extend(self.me_account);
        for id in extra {
            if !shown.contains(&id) && !accounts.iter().any(|(a, _)| *a == Some(id)) {
                accounts.push((Some(id), self.name_of(id)));
            }
        }

        let histories: Vec<(Option<u64>, String, Vec<ScoreChange>)> = accounts
            .into_iter()
            .filter_map(|(id, name)| Some((id, name, self.changes_for(id)?)))
            .collect();
        let start = histories
            .iter()
            .flat_map(|(_, _, h)| h.iter().map(|c| c.t))
            .min()
            .unwrap_or(self.now - 3600);

        self.lines = histories
            .into_iter()
            .enumerate()
            .map(|(i, (id, name, history))| {
                let is_me = id.is_some() && id == self.me_account;
                SeriesLine {
                    account: id,
                    slot: (!is_me).then_some(i),
                    is_me,
                    points: series::step_series(
                        history.iter().map(|c| (c.t, c.value)).collect(),
                        start,
                        self.now,
                    ),
                    name,
                }
            })
            .collect();
    }

    pub(super) fn rebuild_feed(&mut self, mark_fresh: bool) {
        let challenges: HashMap<u64, &Challenge> =
            self.data.challenges.iter().map(|c| (c.id, c)).collect();
        let mut ids: Vec<u64> = self.top_ids().into_iter().collect();
        ids.extend(self.data.accounts.keys().copied());
        ids.sort_unstable();
        ids.dedup();

        let mut items = Vec::new();
        // (is you, team, challenge, points) of solves first seen just now.
        let mut news: Vec<(bool, String, String, i64)> = Vec::new();
        let now = Instant::now();
        for id in ids {
            let name = self.name_of(id);
            let key = account_key(Some(id), &name);
            for c in self.changes_for(Some(id)).unwrap_or_default() {
                let Some(cid) = c.challenge_id else { continue };
                let solve_key = (key.clone(), cid);
                let first_seen = self.seen_solves.insert(solve_key.clone()) && mark_fresh;
                if first_seen {
                    self.fresh_solves.insert(solve_key.clone(), now);
                }
                let fresh = self
                    .fresh_solves
                    .get(&solve_key)
                    .is_some_and(|at| at.elapsed() < FRESH_TTL);
                let (challenge, category) = challenges
                    .get(&cid)
                    .map(|ch| (ch.name.clone(), ch.category.clone()))
                    .or_else(|| self.data.names.get(&cid).cloned())
                    .unwrap_or_else(|| (format!("challenge {cid}"), String::new()));
                // A newly fetched account brings its whole history; only
                // what happened just now is news.
                if first_seen && c.t >= self.now - NEWS_SECS {
                    news.push((
                        Some(id) == self.me_account,
                        name.clone(),
                        challenge.clone(),
                        c.value,
                    ));
                }
                items.push(FeedItem {
                    t: c.t,
                    account: id,
                    team: name.clone(),
                    challenge,
                    category,
                    value: c.value,
                    is_me: Some(id) == self.me_account,
                    fresh,
                });
            }
        }
        self.fresh_solves.retain(|_, at| at.elapsed() < FRESH_TTL);
        items.sort_by(|a, b| b.t.cmp(&a.t).then(a.team.cmp(&b.team)));
        items.truncate(FEED_LEN);
        self.feed = items;
        self.notify_solves(news);
    }
}
