//! Scoreboard UI state: data, selection, live-change tracking, and the
//! derived analytics the views render.

use super::analysis::{self, Comparison};
use super::{series, theme};
use crate::models::{AccountSolve, Challenge, ScoreboardEntry, TopEntry};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use ratatui::widgets::TableState;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Window for the standings' rank/score change column.
pub const CHANGE_WINDOW: Duration = Duration::from_secs(15 * 60);
/// How long a new solve stays highlighted in the live feed.
const FRESH_TTL: Duration = Duration::from_secs(45);
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
}

/// Messages from the background fetcher.
#[derive(Debug)]
pub enum Update {
    Data(Box<Data>),
    Account(u64, Vec<AccountSolve>),
    Refreshing,
    Failed(String),
}

/// Requests to the background fetcher.
#[derive(Debug, PartialEq)]
pub enum Cmd {
    Refresh,
    /// Keep these accounts' solves fetched (outside the top list).
    Accounts(Vec<u64>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Board,
    Analysis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardLayout {
    Split,
    Graph,
    Table,
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
    pub color: Color,
    pub is_me: bool,
    pub points: Vec<(f64, f64)>,
}

/// One solve in the live feed.
#[derive(Debug, Clone)]
pub struct FeedItem {
    pub t: i64,
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

pub struct App {
    pub host: String,
    pub demo: bool,
    pub interval: Duration,
    pub top_n: u32,
    pub cutoff: Option<u32>,
    pub kind: AccountKind,
    pub me_account: Option<u64>,
    pub me_name: Option<String>,
    pub data: Data,

    pub view: View,
    pub layout: BoardLayout,
    pub show_help: bool,
    pub selected: usize,
    pub board_state: TableState,
    pub catch_state: TableState,
    pub rival_key: Option<String>,
    pub pins: Vec<u64>,
    pub search: Option<String>,
    last_search: Option<String>,
    sent_wanted: Vec<u64>,

    pub last_update: Instant,
    pub updated_at: chrono::DateTime<chrono::Local>,
    pub refreshing: bool,
    pub error: Option<String>,
    pub quit: bool,

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
    pub now: i64,
}

/// Stable identity of a scoreboard account.
pub fn account_key(id: Option<u64>, name: &str) -> String {
    match id {
        Some(id) => format!("id:{id}"),
        None => format!("name:{name}"),
    }
}

impl App {
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
        let mut app = Self {
            host,
            demo,
            interval,
            top_n,
            cutoff,
            kind,
            me_account,
            me_name,
            data,
            view: View::Board,
            layout: BoardLayout::Split,
            show_help: false,
            selected: 0,
            board_state: TableState::default(),
            catch_state: TableState::default().with_selected(Some(0)),
            rival_key: None,
            pins: Vec::new(),
            search: None,
            last_search: None,
            sent_wanted: Vec::new(),
            last_update: Instant::now(),
            updated_at: chrono::Local::now(),
            refreshing: false,
            error: None,
            quit: false,
            changes: HashMap::new(),
            snapshots: Default::default(),
            last_moved: HashMap::new(),
            seen_solves: HashSet::new(),
            fresh_solves: HashMap::new(),
            feed: Vec::new(),
            lines: Vec::new(),
            h2h: HeadToHead::default(),
            now: series::now(),
        };
        if let Some(me) = app.me_index() {
            app.selected = me;
            app.rival_key = app.default_rival_key();
        } else if let Some(first) = app.data.standings.first() {
            app.rival_key = Some(account_key(first.account_id, &first.name));
        }
        let initial = app.data.standings.clone();
        app.track_changes(&initial);
        app.rebuild(false);
        app.sent_wanted = app.wanted_accounts();
        app
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

    pub fn rival_index(&self) -> Option<usize> {
        let key = self.rival_key.as_deref()?;
        self.data
            .standings
            .iter()
            .position(|e| account_key(e.account_id, &e.name) == key)
    }

    /// The team just above you, or just below when you lead.
    fn default_rival_key(&self) -> Option<String> {
        let me = self.me_index()?;
        let idx = if me > 0 { me - 1 } else { me + 1 };
        let e = self.data.standings.get(idx)?;
        Some(account_key(e.account_id, &e.name))
    }

    fn top_ids(&self) -> HashSet<u64> {
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

    /// Accounts outside the top list whose solves the UI needs.
    pub fn wanted_accounts(&self) -> Vec<u64> {
        let top = self.top_ids();
        let mut ids: Vec<u64> = self.pins.clone();
        ids.extend(self.me_account);
        if let Some(i) = self.rival_index() {
            ids.extend(self.data.standings[i].account_id);
        }
        if self.view == View::Board
            && let Some(e) = self.data.standings.get(self.selected)
        {
            // Prefetch so switching to Analysis is instant.
            ids.extend(e.account_id);
        }
        ids.retain(|id| !top.contains(id));
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Return a fetch request if the wanted set changed.
    fn sync_wanted(&mut self) -> Option<Cmd> {
        let wanted = self.wanted_accounts();
        if wanted == self.sent_wanted {
            return None;
        }
        self.sent_wanted = wanted.clone();
        Some(Cmd::Accounts(wanted))
    }

    // ------------------------------------------------------------------
    // Updates
    // ------------------------------------------------------------------

    pub fn apply(&mut self, update: Update) {
        match update {
            Update::Refreshing => self.refreshing = true,
            Update::Failed(msg) => {
                self.refreshing = false;
                self.error = Some(msg);
                self.last_update = Instant::now();
            }
            Update::Account(id, solves) => {
                self.data.accounts.insert(id, solves);
                self.rebuild(true);
            }
            Update::Data(new) => {
                let selected_key = self
                    .data
                    .standings
                    .get(self.selected)
                    .map(|e| account_key(e.account_id, &e.name));
                self.track_changes(&new.standings);

                let mut new = *new;
                for (id, solves) in std::mem::take(&mut self.data.accounts) {
                    new.accounts.entry(id).or_insert(solves);
                }
                self.data = new;
                self.kind = AccountKind::from_standings(&self.data.standings);

                if let Some(key) = selected_key
                    && let Some(i) = self
                        .data
                        .standings
                        .iter()
                        .position(|e| account_key(e.account_id, &e.name) == key)
                {
                    self.selected = i;
                }
                self.clamp_selection();
                self.refreshing = false;
                self.error = None;
                self.last_update = Instant::now();
                self.updated_at = chrono::Local::now();
                self.rebuild(true);
            }
        }
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

    /// Rank/score change over the last [`CHANGE_WINDOW`], if any.
    pub fn change_for(&self, entry: &ScoreboardEntry) -> Option<Change> {
        self.changes
            .get(&account_key(entry.account_id, &entry.name))
            .copied()
    }

    fn clamp_selection(&mut self) {
        let len = self.data.standings.len();
        self.selected = if len == 0 {
            0
        } else {
            self.selected.min(len - 1)
        };
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
                    color: if is_me {
                        Color::Reset
                    } else {
                        theme::series(i)
                    },
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

    /// Color of an account's graph line, if it is graphed.
    pub fn line_color(&self, account: Option<u64>) -> Option<Color> {
        let id = account?;
        self.lines
            .iter()
            .find(|l| l.account == Some(id))
            .map(|l| l.color)
    }

    fn rebuild_feed(&mut self, mark_fresh: bool) {
        let challenges: HashMap<u64, &Challenge> =
            self.data.challenges.iter().map(|c| (c.id, c)).collect();
        let mut ids: Vec<u64> = self.top_ids().into_iter().collect();
        ids.extend(self.data.accounts.keys().copied());
        ids.sort_unstable();
        ids.dedup();

        let mut items = Vec::new();
        let now = Instant::now();
        for id in ids {
            let name = self.name_of(id);
            let key = account_key(Some(id), &name);
            for c in self.changes_for(Some(id)).unwrap_or_default() {
                let Some(cid) = c.challenge_id else { continue };
                let solve_key = (key.clone(), cid);
                if self.seen_solves.insert(solve_key.clone()) && mark_fresh {
                    self.fresh_solves.insert(solve_key.clone(), now);
                }
                let fresh = self
                    .fresh_solves
                    .get(&solve_key)
                    .is_some_and(|at| at.elapsed() < FRESH_TTL);
                let (challenge, category) = challenges
                    .get(&cid)
                    .map(|ch| (ch.name.clone(), ch.category.clone()))
                    .unwrap_or_else(|| (format!("challenge {cid}"), String::new()));
                items.push(FeedItem {
                    t: c.t,
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

    fn rebuild_h2h(&mut self) {
        let standings = &self.data.standings;
        let me_idx = self.me_index();
        let rival_idx = self.rival_index().filter(|i| Some(*i) != me_idx);

        let mine: HashSet<u64> = self
            .data
            .challenges
            .iter()
            .filter(|c| c.solved_by_me)
            .map(|c| c.id)
            .collect();

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

        let comparison = analysis::compare(&self.data.challenges, &mine, &theirs, &top_counts);

        let party = |i: usize, solves: Option<usize>| Party {
            pos: standings[i].pos,
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
        let len = self.h2h.comparison.catch_up.len();
        let sel = self.catch_state.selected().unwrap_or(0);
        self.catch_state
            .select(Some(if len == 0 { 0 } else { sel.min(len - 1) }));
    }

    // ------------------------------------------------------------------
    // Input
    // ------------------------------------------------------------------

    /// Handle a key press; returns a request for the fetcher, if any.
    pub fn on_key(&mut self, key: KeyEvent) -> Option<Cmd> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return None;
        }
        if let Some(query) = self.search.as_mut() {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.last_search = self.search.take().filter(|q| !q.is_empty());
                }
                KeyCode::Backspace => {
                    query.pop();
                }
                KeyCode::Char(c) => {
                    query.push(c);
                    let q = query.clone();
                    self.jump_to_match(&q, 0);
                }
                _ => {}
            }
            return self.sync_wanted();
        }
        if self.show_help {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
            ) {
                self.show_help = false;
            }
            return None;
        }

        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc => {}
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char('r') => return Some(Cmd::Refresh),
            KeyCode::Tab | KeyCode::BackTab => self.switch_view(match self.view {
                View::Board => View::Analysis,
                View::Analysis => View::Board,
            }),
            KeyCode::Char('1') => self.switch_view(View::Board),
            KeyCode::Char('2') | KeyCode::Char('a') => self.switch_view(View::Analysis),
            _ => match self.view {
                View::Board => self.board_key(key.code),
                View::Analysis => self.analysis_key(key.code),
            },
        }
        self.sync_wanted()
    }

    fn switch_view(&mut self, view: View) {
        if view == View::Analysis && self.view == View::Board {
            // Compare against the highlighted team unless it is you.
            if let Some(e) = self.data.standings.get(self.selected)
                && !self.is_me(e)
            {
                self.rival_key = Some(account_key(e.account_id, &e.name));
            } else if self.rival_index().is_none() {
                self.rival_key = self.default_rival_key();
            }
            self.catch_state.select(Some(0));
            self.rebuild_h2h();
        }
        self.view = view;
    }

    fn move_selection(&mut self, delta: i64) {
        let len = self.data.standings.len() as i64;
        if len == 0 {
            return;
        }
        self.selected = (self.selected as i64 + delta).clamp(0, len - 1) as usize;
    }

    fn board_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::Home | KeyCode::Char('g') => self.selected = 0,
            KeyCode::End | KeyCode::Char('G') => self.move_selection(i64::MAX / 2),
            KeyCode::Char('m') => {
                if let Some(i) = self.me_index() {
                    self.selected = i;
                }
            }
            KeyCode::Char('/') => self.search = Some(String::new()),
            KeyCode::Char('n') => {
                if let Some(q) = self.last_query() {
                    self.jump_to_match(&q, self.selected + 1);
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.toggle_pin(),
            KeyCode::Char('l') => {
                self.layout = match self.layout {
                    BoardLayout::Split => BoardLayout::Graph,
                    BoardLayout::Graph => BoardLayout::Table,
                    BoardLayout::Table => BoardLayout::Split,
                }
            }
            _ => {}
        }
    }

    fn analysis_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.scroll_catch_up(-1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_catch_up(1),
            KeyCode::PageUp => self.scroll_catch_up(-10),
            KeyCode::PageDown => self.scroll_catch_up(10),
            KeyCode::Left | KeyCode::Char('h') => self.step_rival(-1),
            KeyCode::Right | KeyCode::Char('l') => self.step_rival(1),
            KeyCode::Char('m') => {
                self.rival_key = self.default_rival_key();
                self.catch_state.select(Some(0));
                self.rebuild_h2h();
            }
            _ => {}
        }
    }

    fn scroll_catch_up(&mut self, delta: i64) {
        let len = self.h2h.comparison.catch_up.len() as i64;
        if len == 0 {
            return;
        }
        let sel = self.catch_state.selected().unwrap_or(0) as i64;
        self.catch_state
            .select(Some((sel + delta).clamp(0, len - 1) as usize));
    }

    /// Move the rival one rank up (`-1`) or down (`1`), skipping you.
    fn step_rival(&mut self, delta: i64) {
        let len = self.data.standings.len() as i64;
        if len == 0 {
            return;
        }
        let me = self.me_index();
        let mut i = self
            .rival_index()
            .map(|i| i as i64)
            .unwrap_or(me.unwrap_or(0) as i64);
        loop {
            i += delta;
            if i < 0 || i >= len {
                return;
            }
            if Some(i as usize) != me {
                break;
            }
        }
        let e = &self.data.standings[i as usize];
        self.rival_key = Some(account_key(e.account_id, &e.name));
        self.selected = i as usize;
        self.catch_state.select(Some(0));
        self.rebuild_h2h();
    }

    fn toggle_pin(&mut self) {
        let Some(id) = self
            .data
            .standings
            .get(self.selected)
            .and_then(|e| e.account_id)
        else {
            return;
        };
        if self.top_ids().contains(&id) || Some(id) == self.me_account {
            return;
        }
        if let Some(pos) = self.pins.iter().position(|p| *p == id) {
            self.pins.remove(pos);
        } else {
            self.pins.push(id);
        }
        self.rebuild_lines();
    }

    fn last_query(&self) -> Option<String> {
        self.last_search.clone()
    }

    fn jump_to_match(&mut self, query: &str, from: usize) {
        let q = query.to_lowercase();
        if q.is_empty() {
            return;
        }
        let standings = &self.data.standings;
        let found = (from..standings.len())
            .chain(0..from.min(standings.len()))
            .find(|&i| standings[i].name.to_lowercase().contains(&q));
        if let Some(i) = found {
            self.selected = i;
        }
    }

    /// Seconds until the next automatic refresh.
    pub fn next_refresh_in(&self) -> u64 {
        self.interval
            .saturating_sub(self.last_update.elapsed())
            .as_secs()
    }
}
