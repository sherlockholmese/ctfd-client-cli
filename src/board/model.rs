//! Scoreboard state shared by the terminal and web views: fetched data,
//! live-change tracking, and the derived analytics both render.
//!
//! Nothing here knows how it is drawn.  Each front end keeps its own view
//! state (selection, layout, scroll) and maps series slots to its colors.

mod history;
mod notices;
mod planning;

pub use history::{Change, FeedItem, ScoreChange, SeriesLine};
pub use notices::{Notice, NoticeKind};
use planning::Rung;
pub use planning::{HeadToHead, Momentum, Party, Plan};

use super::analysis;
use super::series;
use crate::models::{AccountSolve, Challenge, ScoreboardEntry, TopEntry};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Window for the standings' rank/score change column.
pub const CHANGE_WINDOW: Duration = Duration::from_secs(15 * 60);

/// Rank and score per account key, at one point in time.
type RankScores = HashMap<String, (i64, i64)>;

/// Everything fetched from CTFd in one refresh.
#[derive(Debug, Clone, Default)]
pub struct Data {
    pub standings: Vec<ScoreboardEntry>,
    /// Top accounts with score history, in rank order.
    pub top: Vec<TopEntry>,
    pub challenges: Vec<Challenge>,
    /// Solves of accounts outside the top list (pinned, you, rival).
    pub accounts: HashMap<u64, Vec<AccountSolve>>,
    /// Name and category of challenges missing from `challenges` (hidden
    /// once the CTF ends), learned from account solves.
    pub names: HashMap<u64, (String, String)>,
}

/// Messages from the background fetcher.
#[derive(Debug)]
pub enum Update {
    Data(Box<Data>),
    Account(u64, Vec<AccountSolve>),
    Refreshing,
    Failed(String),
    /// A download of challenge files began: what is being fetched.
    Downloading(String),
    /// It ended: whether it worked, and what to say about it.
    Downloaded {
        ok: bool,
        title: String,
        detail: String,
    },
    /// Which challenges have a folder in the workspace.
    OnDisk(HashSet<u64>),
    /// A challenge's description and files arrived (or failed to).
    Detail(u64, Result<Box<ChallengeInfo>, String>),
    /// A flag is on its way to the server.
    Submitting,
    /// The server answered a flag.
    Submitted {
        id: u64,
        outcome: crate::models::SubmitOutcome,
        message: String,
    },
}

/// What a challenge says beyond the list: fetched when it is opened.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChallengeInfo {
    pub description: String,
    pub connection_info: Option<String>,
    /// Attachment file names.
    pub files: Vec<String>,
    pub tags: Vec<String>,
    pub max_attempts: Option<i64>,
}

/// The last flag sent for a challenge and what came back.
#[derive(Debug, Clone, PartialEq)]
pub struct Attempt {
    pub id: u64,
    /// Accepted, or solved already.
    pub solved: bool,
    pub message: String,
}

/// Which challenges to download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Download {
    /// Every visible challenge (files already on disk are skipped).
    All,
    /// The challenge with this ID.
    One(u64),
}

/// Requests to the background fetcher.
#[derive(Debug, PartialEq)]
pub enum Cmd {
    Refresh,
    /// Keep these accounts' solves fetched (outside the top list).
    Accounts(Vec<u64>),
    /// Download challenge files; handled by the view, not the fetcher.
    Download(Download),
    /// Load a challenge's description and files; likewise.
    Detail(u64),
    /// Submit a flag for a challenge; likewise.
    Submit(u64, String),
}

/// Teams or users mode, from the scoreboard's `account_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountKind {
    Teams,
    Users,
}

impl AccountKind {
    pub fn from_standings(standings: &[ScoreboardEntry]) -> Self {
        match standings.first().and_then(|e| e.account_type.as_deref()) {
            Some("user") => Self::Users,
            _ => Self::Teams,
        }
    }

    /// API path segment for solves.
    pub fn path(self) -> &'static str {
        match self {
            Self::Teams => "teams",
            Self::Users => "users",
        }
    }

    /// What an account is called in copy.
    pub fn noun(self) -> &'static str {
        match self {
            Self::Teams => "team",
            Self::Users => "player",
        }
    }
}

pub use super::account_key;

pub struct BoardModel {
    /// Header label: server host, plus the scoring start when set.
    pub host: String,
    /// Points scored before this Unix time are ignored.
    pub since: Option<i64>,
    pub demo: bool,
    pub interval: Duration,
    pub top_n: u32,
    pub cutoff: Option<u32>,
    pub kind: AccountKind,
    pub me_account: Option<u64>,
    pub me_name: Option<String>,
    pub data: Data,

    pub rival_key: Option<String>,
    /// Account picked as the Plan goal; `None` follows the default.
    pub goal_key: Option<String>,
    /// How the Plan orders the targets off the route.
    pub plan_sort: analysis::Sort,
    pub pins: Vec<u64>,
    /// Recent notices, oldest first.
    pub notices: std::collections::VecDeque<Notice>,
    next_notice: u64,

    pub last_update: Instant,
    pub updated_at: chrono::DateTime<chrono::Local>,
    pub refreshing: bool,
    pub error: Option<String>,
    /// Whether challenge files are being downloaded right now.
    pub downloading: bool,
    /// Challenges with a folder in the workspace.
    pub on_disk: HashSet<u64>,
    /// Descriptions and files of the challenges opened so far.
    pub details: HashMap<u64, ChallengeInfo>,
    /// Challenges whose details are being fetched.
    pub details_pending: HashSet<u64>,
    /// Whether a flag is with the server right now.
    pub submitting: bool,
    /// The last flag sent and its answer.
    pub attempt: Option<Attempt>,

    pub changes: HashMap<String, Change>,
    /// Standings snapshots (rank, score) covering the change window.
    snapshots: std::collections::VecDeque<(Instant, RankScores)>,
    /// When each account's rank or score last moved.
    last_moved: HashMap<String, Instant>,
    seen_solves: HashSet<(String, u64)>,
    fresh_solves: HashMap<(String, u64), Instant>,
    pub feed: Vec<FeedItem>,
    pub lines: Vec<SeriesLine>,
    pub h2h: HeadToHead,
    pub plan: Plan,
    pub now: i64,
}

impl BoardModel {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host: String,
        demo: bool,
        interval: Duration,
        top_n: u32,
        cutoff: Option<u32>,
        me_account: Option<u64>,
        me_name: Option<String>,
        data: Data,
    ) -> Self {
        let kind = AccountKind::from_standings(&data.standings);
        let mut model = Self {
            host,
            since: None,
            demo,
            interval,
            top_n,
            cutoff,
            kind,
            me_account,
            me_name,
            data,
            rival_key: None,
            goal_key: None,
            plan_sort: analysis::Sort::default(),
            pins: Vec::new(),
            notices: Default::default(),
            next_notice: 1,
            last_update: Instant::now(),
            updated_at: chrono::Local::now(),
            refreshing: false,
            error: None,
            downloading: false,
            on_disk: HashSet::new(),
            details: HashMap::new(),
            details_pending: HashSet::new(),
            submitting: false,
            attempt: None,
            changes: HashMap::new(),
            snapshots: Default::default(),
            last_moved: HashMap::new(),
            seen_solves: HashSet::new(),
            fresh_solves: HashMap::new(),
            feed: Vec::new(),
            lines: Vec::new(),
            h2h: HeadToHead::default(),
            plan: Plan::default(),
            now: series::now(),
        };
        model.rival_key = model.default_rival_key().or_else(|| {
            model
                .data
                .standings
                .first()
                .map(|e| account_key(e.account_id, &e.name))
        });
        let initial = model.data.standings.clone();
        model.track_changes(&initial);
        model.rebuild(false);
        model
    }

    pub fn me_index(&self) -> Option<usize> {
        let standings = &self.data.standings;
        if let Some(id) = self.me_account
            && let Some(i) = standings.iter().position(|e| e.account_id == Some(id))
        {
            return Some(i);
        }
        let name = self.me_name.as_deref()?;
        standings.iter().position(|e| e.name == name)
    }

    pub fn is_me(&self, entry: &ScoreboardEntry) -> bool {
        match (self.me_account, entry.account_id) {
            (Some(me), Some(id)) => me == id,
            _ => self.me_name.as_deref() == Some(entry.name.as_str()),
        }
    }

    /// Position in the standings of the account with this key.
    pub fn index_of_key(&self, key: &str) -> Option<usize> {
        self.data
            .standings
            .iter()
            .position(|e| account_key(e.account_id, &e.name) == key)
    }

    pub fn rival_index(&self) -> Option<usize> {
        self.index_of_key(self.rival_key.as_deref()?)
    }

    /// The team just above you, or just below when you lead.
    pub fn default_rival_key(&self) -> Option<String> {
        let me = self.me_index()?;
        let idx = if me > 0 { me - 1 } else { me + 1 };
        let e = self.data.standings.get(idx)?;
        Some(account_key(e.account_id, &e.name))
    }

    pub fn top_ids(&self) -> HashSet<u64> {
        self.data.top.iter().filter_map(|t| t.id).collect()
    }

    /// Score history of an account, from the top list or fetched solves.
    pub fn changes_for(&self, account: Option<u64>) -> Option<Vec<ScoreChange>> {
        let id = account?;
        if let Some(top) = self.data.top.iter().find(|t| t.id == Some(id)) {
            return Some(
                top.solves
                    .iter()
                    .filter_map(|s| {
                        Some(ScoreChange {
                            t: series::parse_time(&s.date)?,
                            value: s.value,
                            challenge_id: s.challenge_id,
                        })
                    })
                    .collect(),
            );
        }
        let solves = self.data.accounts.get(&id)?;
        Some(
            solves
                .iter()
                .filter_map(|s| {
                    Some(ScoreChange {
                        t: series::parse_time(&s.date)?,
                        value: s.challenge.as_ref().map(|c| c.value).unwrap_or(0),
                        challenge_id: s
                            .challenge_id
                            .or_else(|| s.challenge.as_ref().and_then(|c| c.id)),
                    })
                })
                .collect(),
        )
    }

    /// Accounts outside the top list whose solves are needed: pins, you,
    /// the rival, and `also` (e.g. a highlighted row, to prefetch).
    pub fn wanted_accounts(&self, also: Option<u64>) -> Vec<u64> {
        let top = self.top_ids();
        let mut ids: Vec<u64> = self.pins.clone();
        ids.extend(self.me_account);
        if let Some(i) = self.rival_index() {
            ids.extend(self.data.standings[i].account_id);
        }
        ids.extend(also);
        ids.retain(|id| !top.contains(id));
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Rank/score change over the last [`CHANGE_WINDOW`], if any.
    pub fn change_for(&self, entry: &ScoreboardEntry) -> Option<Change> {
        self.changes
            .get(&account_key(entry.account_id, &entry.name))
            .copied()
    }

    /// Series slot of an account's graph line: `None` when not graphed,
    /// `Some(None)` for your own line.
    pub fn line_slot(&self, account: Option<u64>) -> Option<Option<usize>> {
        let id = account?;
        self.lines
            .iter()
            .find(|l| l.account == Some(id))
            .map(|l| l.slot)
    }

    /// Seconds until the next automatic refresh.
    pub fn next_refresh_in(&self) -> u64 {
        self.interval
            .saturating_sub(self.last_update.elapsed())
            .as_secs()
    }

    pub fn apply(&mut self, update: Update) {
        match update {
            Update::Refreshing => self.refreshing = true,
            Update::Failed(msg) => {
                self.refreshing = false;
                if self.error.is_none() {
                    self.notify(
                        NoticeKind::Failed,
                        "Couldn't update the scoreboard".into(),
                        msg.clone(),
                    );
                }
                self.error = Some(msg);
                self.last_update = Instant::now();
            }
            Update::Downloading(what) => {
                self.downloading = true;
                self.notify(NoticeKind::Downloading, what, String::new());
            }
            Update::Downloaded { ok, title, detail } => {
                self.downloading = false;
                let kind = if ok {
                    NoticeKind::Downloaded
                } else {
                    NoticeKind::Failed
                };
                self.notify(kind, title, detail);
            }
            Update::OnDisk(ids) => self.on_disk = ids,
            Update::Detail(id, result) => {
                self.details_pending.remove(&id);
                match result {
                    Ok(info) => {
                        self.details.insert(id, *info);
                    }
                    Err(e) => {
                        let name = self.challenge_name(id);
                        self.notify(NoticeKind::Failed, format!("Couldn't open {name}"), e);
                    }
                }
            }
            Update::Submitting => self.submitting = true,
            Update::Submitted {
                id,
                outcome,
                message,
            } => {
                use crate::models::SubmitOutcome;
                self.submitting = false;
                let solved = matches!(
                    outcome,
                    SubmitOutcome::Accepted | SubmitOutcome::AlreadySolved
                );
                let name = self.challenge_name(id);
                if solved {
                    // Count it now; the next update confirms it.
                    if let Some(c) = self.data.challenges.iter_mut().find(|c| c.id == id) {
                        c.solved_by_me = true;
                    }
                    self.rebuild(false);
                }
                self.notify(
                    if solved {
                        NoticeKind::FlagAccepted
                    } else {
                        NoticeKind::FlagRejected
                    },
                    name,
                    message.clone(),
                );
                self.attempt = Some(Attempt {
                    id,
                    solved,
                    message,
                });
            }
            Update::Account(id, solves) => {
                self.data.accounts.insert(id, solves);
                self.rebuild(true);
            }
            Update::Data(new) => {
                let was = self.me_index().map(|i| self.data.standings[i].pos);
                let failed = self.error.is_some();
                self.track_changes(&new.standings);
                let mut new = *new;
                for (id, solves) in std::mem::take(&mut self.data.accounts) {
                    new.accounts.entry(id).or_insert(solves);
                }
                self.data = new;
                self.kind = AccountKind::from_standings(&self.data.standings);
                self.refreshing = false;
                self.error = None;
                self.last_update = Instant::now();
                self.updated_at = chrono::Local::now();
                self.rebuild(true);
                if failed {
                    self.notify(
                        NoticeKind::Recovered,
                        "Scoreboard is updating again".into(),
                        String::new(),
                    );
                }
                self.notify_rank(was);
            }
        }
    }

    fn challenge_name(&self, id: u64) -> String {
        self.data
            .challenges
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| format!("challenge {id}"))
    }

    /// Every visible challenge, as the Challenges tab lists them: by
    /// category, then cheapest first, then by name.
    pub fn challenge_list(&self) -> Vec<&Challenge> {
        let mut list: Vec<&Challenge> = self.data.challenges.iter().collect();
        list.sort_by(|a, b| {
            a.category
                .to_lowercase()
                .cmp(&b.category.to_lowercase())
                .then(a.value.cmp(&b.value))
                .then_with(|| a.name.cmp(&b.name))
        });
        list
    }

    /// Note that a challenge's details are wanted; `false` when they are
    /// already here or on their way.
    pub fn want_detail(&mut self, id: u64) -> bool {
        !self.details.contains_key(&id) && self.details_pending.insert(id)
    }

    /// Aim the Plan route at the account with this key.
    pub fn set_goal(&mut self, key: String) {
        self.goal_key = Some(key);
        self.rebuild_h2h();
    }

    /// Order the Plan's targets by `sort`.
    pub fn set_sort(&mut self, sort: analysis::Sort) {
        self.plan_sort = sort;
        self.plan.sort = sort;
    }

    /// Go back to the default goal.
    pub fn reset_goal(&mut self) {
        self.goal_key = None;
        self.rebuild_h2h();
    }

    /// Move the goal one team up the board (`-1`) or down (`1`), among
    /// the teams ahead of you on the ladder.
    pub fn step_goal(&mut self, delta: i64) {
        let ahead: Vec<&Rung> = self.plan.ladder.iter().filter(|r| r.ahead >= 0).collect();
        let Some(current) = ahead.iter().position(|r| Some(r.pos) == self.plan.goal) else {
            return;
        };
        let next = (current as i64 + delta).clamp(0, ahead.len() as i64 - 1) as usize;
        if next != current {
            let key = account_key(ahead[next].account, &ahead[next].name);
            self.set_goal(key);
        }
    }

    /// Make the account with this key the rival.
    pub fn set_rival(&mut self, key: String) {
        self.rival_key = Some(key);
        self.rebuild_h2h();
    }

    /// Go back to the default rival (the team just above you).
    pub fn reset_rival(&mut self) {
        self.rival_key = self.default_rival_key();
        self.rebuild_h2h();
    }

    /// Move the rival one rank up (`-1`) or down (`1`), skipping you.
    /// Returns the new rival's position.
    pub fn step_rival(&mut self, delta: i64) -> Option<usize> {
        let len = self.data.standings.len() as i64;
        if len == 0 {
            return None;
        }
        let me = self.me_index();
        let mut i = self
            .rival_index()
            .map(|i| i as i64)
            .unwrap_or(me.unwrap_or(0) as i64);
        loop {
            i += delta;
            if i < 0 || i >= len {
                return None;
            }
            if Some(i as usize) != me {
                break;
            }
        }
        let e = &self.data.standings[i as usize];
        self.set_rival(account_key(e.account_id, &e.name));
        Some(i as usize)
    }

    /// Whether this account can be pinned to the graph (it is not already
    /// graphed as a top account or as you).
    pub fn can_pin(&self, id: u64) -> bool {
        !self.top_ids().contains(&id) && Some(id) != self.me_account
    }

    /// Pin or unpin an account on the graph.  Returns whether it is now
    /// pinned.
    pub fn set_pin(&mut self, id: u64, pinned: bool) -> bool {
        if !self.can_pin(id) {
            return false;
        }
        let pos = self.pins.iter().position(|p| *p == id);
        match (pos, pinned) {
            (Some(pos), false) => {
                self.pins.remove(pos);
            }
            (None, true) => self.pins.push(id),
            _ => {}
        }
        self.rebuild_lines();
        pinned
    }

    /// Recompute graph lines, feed, and analytics.
    pub fn rebuild(&mut self, mark_fresh: bool) {
        self.now = series::now();
        self.rebuild_lines();
        self.rebuild_feed(mark_fresh);
        self.rebuild_h2h();
    }
}
