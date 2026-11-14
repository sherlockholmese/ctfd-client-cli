//! Scoreboard state shared by the terminal and web views: fetched data,
//! live-change tracking, and the derived analytics both render.
//!
//! Nothing here knows how it is drawn.  Each front end keeps its own view
//! state (selection, layout, scroll) and maps series slots to its colors.

use super::analysis::{self, Comparison};
use super::series;
use crate::models::{AccountSolve, Challenge, ScoreboardEntry, TopEntry};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Window for the standings' rank/score change column.
pub const CHANGE_WINDOW: Duration = Duration::from_secs(15 * 60);
/// How long a new solve stays highlighted in the live feed.
pub const FRESH_TTL: Duration = Duration::from_secs(45);
/// Live feed length.
const FEED_LEN: usize = 60;

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

/// A ranked account in the head-to-head view.
#[derive(Debug, Clone, PartialEq)]
pub struct Party {
    pub pos: i64,
    pub account: Option<u64>,
    pub name: String,
    pub score: i64,
    pub solves: Option<usize>,
}

/// Recent scoring pace.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Momentum {
    pub last_hour: i64,
    pub last_3h: i64,
    pub last_solve: Option<i64>,
}

/// Where you stand relative to a neighbour or a cut-off.
#[derive(Debug, Clone, PartialEq)]
pub struct Margin {
    pub label: String,
    /// Positive: you are ahead by this much; negative: behind.
    pub points: i64,
}

/// Head-to-head analytics between you and the rival.
#[derive(Debug, Clone, Default)]
pub struct HeadToHead {
    pub me: Option<Party>,
    pub rival: Option<Party>,
    /// Rival solves are still being fetched.
    pub rival_loading: bool,
    pub comparison: Comparison,
    pub me_momentum: Option<Momentum>,
    pub rival_momentum: Option<Momentum>,
    pub cutoff: Option<Margin>,
}

/// A team on your way to the top (or just behind you).
#[derive(Debug, Clone, PartialEq)]
pub struct Rung {
    pub pos: i64,
    pub account: Option<u64>,
    pub name: String,
    pub score: i64,
    /// Points they are ahead of you (negative: behind you).
    pub ahead: i64,
    /// Recent pace, when their solves are known.
    pub momentum: Option<Momentum>,
    /// Solves on the likeliest route past them, and the points that brings.
    pub to_pass: Option<(usize, i64)>,
    /// That route: challenge IDs, likeliest solve first.
    pub route: Vec<u64>,
    /// Chance of landing the whole route.
    pub route_odds: Option<f64>,
    /// Your time for the whole route, in seconds, where every step has one.
    pub route_time: Option<i64>,
    /// Points per hour over the last three hours.
    pub pace: Option<f64>,
    /// Seconds until you draw level at both current paces; `None` when
    /// you are not closing in (or their solves are unknown).
    pub catch_in: Option<i64>,
}

/// Solve times over the accounts whose solves are known.
struct SolveTimes {
    timing: HashMap<u64, analysis::Timing>,
    /// Seconds between your solves, typically; `None` until you have a few.
    my_typical: Option<i64>,
    /// The same over everyone else tracked.
    field_typical: Option<i64>,
}

/// What a notice is about; the views pick its tone from this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    /// You solved a challenge.
    YourSolve,
    /// Another account solved one (or several did).
    Solve,
    /// You moved up the board.
    RankUp,
    /// You moved down the board.
    RankDown,
    /// An update failed.
    Failed,
    /// Updates work again.
    Recovered,
    /// A download of challenge files began.
    Downloading,
    /// It finished.
    Downloaded,
    /// A flag was accepted.
    FlagAccepted,
    /// A flag was turned down.
    FlagRejected,
}

impl NoticeKind {
    /// Stable key for the web page.
    pub fn key(self) -> &'static str {
        match self {
            NoticeKind::YourSolve => "your_solve",
            NoticeKind::Solve => "solve",
            NoticeKind::RankUp => "rank_up",
            NoticeKind::RankDown => "rank_down",
            NoticeKind::Failed => "failed",
            NoticeKind::Recovered => "recovered",
            NoticeKind::Downloading => "downloading",
            NoticeKind::Downloaded => "downloaded",
            NoticeKind::FlagAccepted => "flag_accepted",
            NoticeKind::FlagRejected => "flag_rejected",
        }
    }
}

/// Something that just happened, shown briefly in a corner of the view.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    /// Increases with every notice of this run.
    pub id: u64,
    pub kind: NoticeKind,
    pub title: String,
    pub detail: String,
    pub at: Instant,
}

/// How long a notice stays on screen.
pub const NOTICE_TTL: Duration = Duration::from_secs(8);
/// Notices kept for views that connect late or redraw.
const NOTICE_KEEP: usize = 12;
/// Solves named one by one in a single update; more are summed up.
const NOTICE_SOLVES: usize = 3;
/// A solve older than this (seconds) is history, not news.
const NEWS_SECS: i64 = 300;

/// Where to spend time to climb: ranked against the whole board.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    /// Leaders, the teams just above you, and the two just behind.
    pub ladder: Vec<Rung>,
    pub targets: Vec<analysis::Target>,
    pub focus: Vec<analysis::Focus>,
    /// Rank of the team the route aims to pass: the one picked with
    /// [`BoardModel::set_goal`] while it is still ahead of you, else the
    /// last qualifying place when you are outside it, else the team above
    /// you.
    pub goal: Option<i64>,
    /// How the targets off the route are ordered.
    pub sort: analysis::Sort,
    /// Your points per hour over the last three hours.
    pub my_pace: Option<f64>,
    /// Seconds between your solves, typically, and the tracked field's.
    pub my_typical: Option<i64>,
    pub field_typical: Option<i64>,
    /// When scoring began (Unix time).
    pub start: i64,
    /// Accounts above you whose solves are known.
    pub ahead_tracked: usize,
    pub field: Option<analysis::Field>,
}

impl Plan {
    /// What the details of one target draw from, beyond the target itself.
    pub fn context(&self) -> Option<(analysis::Field, i64, usize)> {
        Some((self.field?, self.start, self.ahead_tracked))
    }

    /// The details of one target, as labelled lines.
    pub fn facts(&self, target: &analysis::Target, now: i64) -> Vec<analysis::Fact> {
        match self.field {
            Some(field) => target.facts(field, now, self.start, self.ahead_tracked),
            None => Vec::new(),
        }
    }

    /// The rung the route aims to pass.
    pub fn goal_rung(&self) -> Option<&Rung> {
        let pos = self.goal?;
        self.ladder.iter().find(|r| r.pos == pos)
    }

    /// Targets on the route to the goal, in the order to try them.
    pub fn route(&self) -> Vec<&analysis::Target> {
        self.goal_rung()
            .map(|r| {
                r.route
                    .iter()
                    .filter_map(|id| self.targets.iter().find(|t| t.stat.id == *id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Targets off the route, in the picked order.
    pub fn off_route(&self) -> Vec<&analysis::Target> {
        let on: &[u64] = self.goal_rung().map(|r| r.route.as_slice()).unwrap_or(&[]);
        let mut rest: Vec<&analysis::Target> = self
            .targets
            .iter()
            .filter(|t| !on.contains(&t.stat.id))
            .collect();
        rest.sort_by(|a, b| self.sort.compare(a, b));
        rest
    }

    /// How often you solve against the tracked field, and whether you
    /// are closing on the goal, in a sentence or two.
    pub fn pace_note(&self) -> String {
        let mut note = Vec::new();
        match (self.my_typical, self.field_typical) {
            (Some(mine), Some(field)) => note.push(format!(
                "You solve about every {}, the teams tracked every {}.",
                analysis::span_text(mine),
                analysis::span_text(field)
            )),
            (None, Some(field)) => note.push(format!(
                "The teams tracked solve about every {}.",
                analysis::span_text(field)
            )),
            _ => {}
        }
        if let (Some(goal), Some(_)) = (self.goal_rung(), self.my_pace) {
            match (goal.catch_in, goal.pace) {
                (Some(secs), _) => note.push(format!(
                    "Level with #{} in about {} at the last 3 hours' pace.",
                    goal.pos,
                    analysis::span_text(secs)
                )),
                (None, Some(_)) => note.push(format!(
                    "#{} has scored at least as fast as you over the last 3 hours.",
                    goal.pos
                )),
                _ => {}
            }
        }
        note.join(" ")
    }

    /// The route, then the rest: the order the views list targets in.
    pub fn listed(&self) -> Vec<&analysis::Target> {
        let mut all = self.route();
        all.extend(self.off_route());
        all
    }
}

/// Teams always on the ladder, from the top.
const LADDER_TOP: i64 = 3;
/// Teams just above you on the ladder.
const LADDER_ABOVE: usize = 4;
/// Teams just behind you on the ladder.
const LADDER_BELOW: usize = 2;

/// Stable identity of a scoreboard account.
pub fn account_key(id: Option<u64>, name: &str) -> String {
    match id {
        Some(id) => format!("id:{id}"),
        None => format!("name:{name}"),
    }
}

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

    // ------------------------------------------------------------------
    // Lookups
    // ------------------------------------------------------------------

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

    // ------------------------------------------------------------------
    // Updates
    // ------------------------------------------------------------------

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

    /// Add a notice for the views to show.
    fn notify(&mut self, kind: NoticeKind, title: String, detail: String) {
        self.notices.push_back(Notice {
            id: self.next_notice,
            kind,
            title,
            detail,
            at: Instant::now(),
        });
        self.next_notice += 1;
        while self.notices.len() > NOTICE_KEEP {
            self.notices.pop_front();
        }
    }

    /// Notices still fresh enough to show, oldest first.
    pub fn live_notices(&self) -> impl Iterator<Item = &Notice> {
        self.notices.iter().filter(|n| n.at.elapsed() < NOTICE_TTL)
    }

    /// Say so when your rank moved from `was`, naming who is next to you.
    fn notify_rank(&mut self, was: Option<i64>) {
        let Some(i) = self.me_index() else { return };
        let now = self.data.standings[i].pos;
        let Some(was) = was.filter(|&w| w != now) else {
            return;
        };
        let neighbour = |j: Option<usize>| {
            j.and_then(|j| self.data.standings.get(j))
                .map(|e| e.name.clone())
        };
        if now < was {
            let detail = neighbour(i.checked_add(1))
                .map(|n| format!("Passed {n}"))
                .unwrap_or_default();
            self.notify(NoticeKind::RankUp, format!("You're up to #{now}"), detail);
        } else {
            let detail = neighbour(i.checked_sub(1))
                .map(|n| format!("Passed by {n}"))
                .unwrap_or_default();
            self.notify(
                NoticeKind::RankDown,
                format!("You're down to #{now}"),
                detail,
            );
        }
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

    /// Record a standings snapshot and recompute each account's rank and
    /// score change over the last [`CHANGE_WINDOW`].
    fn track_changes(&mut self, new: &[ScoreboardEntry]) {
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

    /// Recompute graph lines, feed, and analytics.
    pub fn rebuild(&mut self, mark_fresh: bool) {
        self.now = series::now();
        self.rebuild_lines();
        self.rebuild_feed(mark_fresh);
        self.rebuild_h2h();
    }

    fn name_of(&self, id: u64) -> String {
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

    fn rebuild_lines(&mut self) {
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

    fn rebuild_feed(&mut self, mark_fresh: bool) {
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

    /// One notice per new solve, yours first; a busy update is summed up.
    fn notify_solves(&mut self, mut news: Vec<(bool, String, String, i64)>) {
        news.sort_by_key(|(mine, ..)| !mine);
        let (mine, others): (Vec<_>, Vec<_>) = news.into_iter().partition(|(mine, ..)| *mine);
        for (_, _, challenge, value) in mine {
            self.notify(
                NoticeKind::YourSolve,
                format!("You solved {challenge}"),
                format!("+{value} pts"),
            );
        }
        if others.len() > NOTICE_SOLVES {
            let points: i64 = others.iter().map(|(.., value)| value).sum();
            let mut teams: Vec<&str> = others.iter().map(|(_, team, ..)| team.as_str()).collect();
            teams.dedup();
            let detail = match teams.as_slice() {
                [one] => format!("{one}, +{points} pts"),
                [first, rest @ ..] => format!("{first} and {} more, +{points} pts", rest.len()),
                [] => String::new(),
            };
            self.notify(
                NoticeKind::Solve,
                format!("{} new solves", others.len()),
                detail,
            );
        } else {
            for (_, team, challenge, value) in others {
                self.notify(
                    NoticeKind::Solve,
                    format!("{team} solved {challenge}"),
                    format!("+{value} pts"),
                );
            }
        }
    }

    fn momentum(&self, account: Option<u64>) -> Option<Momentum> {
        let history = self.changes_for(account)?;
        let sum_since = |secs: i64| {
            history
                .iter()
                .filter(|c| c.t >= self.now - secs)
                .map(|c| c.value)
                .sum()
        };
        Some(Momentum {
            last_hour: sum_since(3600),
            last_3h: sum_since(3 * 3600),
            last_solve: history.iter().map(|c| c.t).max(),
        })
    }

    /// The challenge list, plus challenges it leaves out that are known
    /// by name from solves, valued at the points they were solved for.
    fn known_challenges(&self) -> Vec<Challenge> {
        let mut challenges = self.data.challenges.clone();
        if self.data.names.is_empty() {
            return challenges;
        }
        let mut values: HashMap<u64, i64> = HashMap::new();
        let top = self.data.top.iter().flat_map(|t| &t.solves);
        for (id, value) in top.filter_map(|s| Some((s.challenge_id?, s.value))) {
            values.entry(id).or_insert(value);
        }
        for s in self.data.accounts.values().flatten() {
            if let Some(c) = &s.challenge
                && let Some(id) = c.id.or(s.challenge_id)
            {
                values.entry(id).or_insert(c.value);
            }
        }
        let mut extra: Vec<Challenge> = self
            .data
            .names
            .iter()
            .map(|(&id, (name, category))| Challenge {
                id,
                name: name.clone(),
                category: category.clone(),
                value: values.get(&id).copied().unwrap_or(0),
                solved_by_me: false,
                solves: None,
                tags: Vec::new(),
            })
            .collect();
        extra.sort_by_key(|c| c.id);
        challenges.extend(extra);
        challenges
    }

    /// When scoring began: the configured start, else the earliest solve
    /// known (close to the start once a few teams have scored).
    fn start_time(&self) -> i64 {
        self.since.unwrap_or_else(|| {
            self.tracked_ids()
                .into_iter()
                .filter_map(|id| self.changes_for(Some(id)))
                .flatten()
                .map(|c| c.t)
                .min()
                .unwrap_or(self.now)
        })
    }

    /// Accounts whose solves are known: the graphed top, pins, you and
    /// the rival.
    fn tracked_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self.data.top.iter().filter_map(|t| t.id).collect();
        ids.extend(self.data.accounts.keys().copied());
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Solve times over every tracked account: per challenge who solved
    /// it when and how long after their previous solve, plus how long a
    /// solve typically takes you and the field.
    fn solve_times(&self, start: i64) -> SolveTimes {
        let mut by_challenge: HashMap<u64, Vec<(i64, i64, String)>> = HashMap::new();
        let mut all_spans = Vec::new();
        let mut my_spans = Vec::new();
        for id in self.tracked_ids() {
            let mut history = self.changes_for(Some(id)).unwrap_or_default();
            history.sort_by_key(|c| c.t);
            let name = self.name_of(id);
            let mut previous = start;
            let mut seen = HashSet::new();
            for c in history {
                let span = (c.t - previous).max(0);
                previous = previous.max(c.t);
                let Some(challenge) = c.challenge_id else {
                    continue;
                };
                if !seen.insert(challenge) {
                    continue;
                }
                by_challenge
                    .entry(challenge)
                    .or_default()
                    .push((c.t, span, name.clone()));
                if Some(id) == self.me_account {
                    my_spans.push(span);
                } else {
                    all_spans.push(span);
                }
            }
        }
        let timing = by_challenge
            .into_iter()
            .map(|(challenge, mut solves)| {
                solves.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.cmp(&b.2)));
                let timing = analysis::Timing {
                    times: solves.iter().map(|s| s.0).collect(),
                    spans: solves.iter().map(|s| s.1).collect(),
                    solvers: solves.into_iter().map(|s| s.2).collect(),
                };
                (challenge, timing)
            })
            .collect();
        SolveTimes {
            timing,
            // A pace needs a few solves behind it.
            my_typical: (my_spans.len() >= 3)
                .then(|| analysis::quantile(&my_spans, 0.5))
                .flatten(),
            field_typical: analysis::quantile(&all_spans, 0.5),
        }
    }

    fn rebuild_plan(
        &mut self,
        challenges: &[Challenge],
        mine: &HashSet<u64>,
        theirs: &HashSet<u64>,
        top_counts: &HashMap<u64, usize>,
    ) {
        let standings = &self.data.standings;
        let Some(me) = self.me_index() else {
            self.plan = Plan::default();
            return;
        };
        let my_score = standings[me].score;
        let others: Vec<i64> = standings
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != me)
            .map(|(_, e)| e.score)
            .collect();
        let field = analysis::Field {
            ranked: standings.len(),
            top_n: self.data.top.len().min(self.top_n as usize),
        };
        let start = self.start_time();
        let times = self.solve_times(start);
        // How many tracked accounts above you have each challenge.
        let tracked = self.tracked_ids();
        let above: Vec<u64> = standings[..me]
            .iter()
            .filter_map(|e| e.account_id)
            .filter(|id| tracked.contains(id))
            .collect();
        let mut ahead: HashMap<u64, usize> = HashMap::new();
        for id in &above {
            let solved: HashSet<u64> = self
                .changes_for(Some(*id))
                .iter()
                .flatten()
                .filter_map(|c| c.challenge_id)
                .collect();
            for challenge in solved {
                *ahead.entry(challenge).or_default() += 1;
            }
        }
        let signals = analysis::Signals {
            timing: &times.timing,
            rival: theirs,
            ahead: &ahead,
            my_typical: times.my_typical,
            field_typical: times.field_typical,
            now: self.now,
        };
        let (mut targets, focus) = analysis::targets(
            challenges, mine, top_counts, field, signals, my_score, &others,
        );
        // Name who each improving solve overtakes: with the others in rank
        // order, reaching rank r puts you just above the r-th of them.
        let others_ranked: Vec<&crate::models::ScoreboardEntry> = standings
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != me)
            .map(|(_, e)| e)
            .collect();
        let mut best = standings[me].pos;
        for t in &mut targets {
            if t.reach < best {
                best = t.reach;
                t.passes = others_ranked
                    .get((t.reach - 1) as usize)
                    .map(|e| (t.reach, e.name.clone()));
            }
        }

        let my_pace = self
            .momentum(standings[me].account_id)
            .map(|m| m.last_3h as f64 / 3.0);
        // The last qualifying place, while you are outside it.
        let line = self
            .cutoff
            .map(|n| n as usize)
            .filter(|&n| n >= 1 && me >= n);
        let ladder: Vec<Rung> = standings
            .iter()
            .enumerate()
            .filter(|&(i, e)| {
                i != me
                    && (e.pos <= LADDER_TOP
                        || (i < me && me - i <= LADDER_ABOVE)
                        || (i > me && i - me <= LADDER_BELOW)
                        || line == Some(i + 1))
            })
            .map(|(_, e)| {
                let ahead = e.score - my_score;
                let route = analysis::route(&targets, ahead);
                let momentum = self.momentum(e.account_id);
                let pace = momentum.map(|m| m.last_3h as f64 / 3.0);
                let catch_in = match (my_pace, pace) {
                    (Some(mine), Some(theirs)) if ahead >= 0 && mine > theirs => {
                        Some(((ahead + 1) as f64 / (mine - theirs) * 3600.0).round() as i64)
                    }
                    _ => None,
                };
                Rung {
                    pos: e.pos,
                    account: e.account_id,
                    name: e.name.clone(),
                    score: e.score,
                    ahead,
                    momentum,
                    pace,
                    catch_in,
                    to_pass: route.as_ref().map(|r| (r.picks.len(), r.points)),
                    route_odds: route.as_ref().map(|r| r.odds),
                    route_time: route.as_ref().and_then(|r| {
                        r.picks
                            .iter()
                            .map(|&i| targets[i].time)
                            .sum::<Option<i64>>()
                    }),
                    route: route
                        .map(|r| r.picks.iter().map(|&i| targets[i].stat.id).collect())
                        .unwrap_or_default(),
                }
            })
            .collect();
        let picked = self.goal_key.as_ref().and_then(|key| {
            ladder
                .iter()
                .find(|r| r.ahead >= 0 && account_key(r.account, &r.name) == *key)
                .map(|r| r.pos)
        });
        let goal = picked
            .or_else(|| line.map(|n| standings[n - 1].pos))
            .or_else(|| me.checked_sub(1).map(|i| standings[i].pos));
        self.plan = Plan {
            ladder,
            targets,
            focus,
            goal,
            sort: self.plan_sort,
            my_pace,
            my_typical: times.my_typical,
            field_typical: times.field_typical,
            start,
            ahead_tracked: above.len(),
            field: Some(field),
        };
    }

    pub fn rebuild_h2h(&mut self) {
        let standings = &self.data.standings;
        let me_idx = self.me_index();
        let rival_idx = self.rival_index().filter(|i| Some(*i) != me_idx);

        // Your solves, from the challenge list and your own history (the
        // list is unavailable once a CTF ends).
        let mut mine: HashSet<u64> = self
            .data
            .challenges
            .iter()
            .filter(|c| c.solved_by_me)
            .map(|c| c.id)
            .collect();
        let my_history = self.changes_for(me_idx.and_then(|i| standings[i].account_id));
        mine.extend(my_history.iter().flatten().filter_map(|c| c.challenge_id));

        let rival_account = rival_idx.and_then(|i| standings[i].account_id);
        let rival_history = self.changes_for(rival_account);
        let theirs: HashSet<u64> = rival_history
            .iter()
            .flatten()
            .filter_map(|c| c.challenge_id)
            .collect();

        let mut top_counts: HashMap<u64, usize> = HashMap::new();
        for t in self.data.top.iter().take(self.top_n as usize) {
            let solved: HashSet<u64> = t.solves.iter().filter_map(|s| s.challenge_id).collect();
            for id in solved {
                *top_counts.entry(id).or_default() += 1;
            }
        }

        let challenges = self.known_challenges();
        let when = |history: &Option<Vec<ScoreChange>>| -> HashMap<u64, i64> {
            let mut at = HashMap::new();
            for c in history.iter().flatten() {
                if let Some(id) = c.challenge_id {
                    at.entry(id).or_insert(c.t);
                }
            }
            at
        };
        let (mine_at, theirs_at) = (when(&my_history), when(&rival_history));
        let comparison = analysis::compare(
            &challenges,
            &mine,
            &theirs,
            &top_counts,
            analysis::SolvedAt {
                mine: &mine_at,
                theirs: &theirs_at,
            },
        );
        self.rebuild_plan(&challenges, &mine, &theirs, &top_counts);
        let standings = &self.data.standings;

        let party = |i: usize, solves: Option<usize>| Party {
            pos: standings[i].pos,
            account: standings[i].account_id,
            name: standings[i].name.clone(),
            score: standings[i].score,
            solves,
        };
        let me = me_idx.map(|i| party(i, Some(mine.len())));
        let rival = rival_idx.map(|i| party(i, rival_history.as_ref().map(|_| theirs.len())));

        let cutoff = self.cutoff.zip(me.as_ref()).and_then(|(n, me)| {
            let n = n as usize;
            if me.pos as usize <= n {
                // Buffer over the first team outside the line.
                standings.get(n).map(|first_out| Margin {
                    label: format!("#{}", first_out.pos),
                    points: me.score - first_out.score,
                })
            } else {
                standings.get(n - 1).map(|last_in| Margin {
                    label: format!("#{}", last_in.pos),
                    points: me.score - last_in.score,
                })
            }
        });

        self.h2h = HeadToHead {
            rival_loading: rival_idx.is_some() && rival_history.is_none(),
            me_momentum: me_idx.and_then(|i| self.momentum(standings[i].account_id)),
            rival_momentum: self.momentum(rival_account),
            me,
            rival,
            comparison,
            cutoff,
        };
    }
}
