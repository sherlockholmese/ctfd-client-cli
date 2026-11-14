//! Terminal view state on top of the shared [`BoardModel`]: selection,
//! layout, search, and key handling.

use super::theme;
use crate::board::model::{BoardModel, Cmd, Download, Update, account_key};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use ratatui::widgets::TableState;

pub use crate::board::model::Momentum;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Board,
    Analysis,
    /// Best targets and the ladder to the top.
    Plan,
    /// Every challenge: read it, download it, submit its flag.
    Challenges,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardLayout {
    Split,
    Graph,
    Table,
}

/// How much of the score graph is drawn, counted back from now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    All,
    Last(i64, &'static str),
}

/// The ranges `t` steps through, as on the web page.
pub const RANGES: [Range; 7] = [
    Range::All,
    Range::Last(86_400, "24h"),
    Range::Last(43_200, "12h"),
    Range::Last(21_600, "6h"),
    Range::Last(10_800, "3h"),
    Range::Last(3_600, "1h"),
    Range::Last(1_800, "30m"),
];

impl Range {
    pub fn secs(self) -> Option<i64> {
        match self {
            Range::All => None,
            Range::Last(secs, _) => Some(secs),
        }
    }

    pub fn label(self) -> Option<&'static str> {
        match self {
            Range::All => None,
            Range::Last(_, label) => Some(label),
        }
    }
}

pub struct App {
    pub board: BoardModel,

    pub view: View,
    pub layout: BoardLayout,
    pub range: Range,
    pub show_help: bool,
    pub selected: usize,
    pub board_state: TableState,
    pub catch_state: TableState,
    pub plan_state: TableState,
    pub chal_state: TableState,
    /// Hide the challenges you have solved.
    pub unsolved_only: bool,
    /// The flag being typed for the highlighted challenge.
    pub flag_input: Option<String>,
    /// Lines the challenge description is scrolled down by.
    pub detail_scroll: u16,
    pub search: Option<String>,
    last_search: Option<String>,
    sent_wanted: Vec<u64>,
    pub quit: bool,
}

impl App {
    pub fn new(board: BoardModel) -> Self {
        let mut app = Self {
            selected: board.me_index().unwrap_or(0),
            board,
            view: View::Board,
            layout: BoardLayout::Split,
            range: Range::All,
            show_help: false,
            board_state: TableState::default(),
            catch_state: TableState::default().with_selected(Some(0)),
            plan_state: TableState::default().with_selected(Some(0)),
            chal_state: TableState::default().with_selected(Some(0)),
            unsolved_only: false,
            flag_input: None,
            detail_scroll: 0,
            search: None,
            last_search: None,
            sent_wanted: Vec::new(),
            quit: false,
        };
        app.sent_wanted = app.wanted_accounts();
        app
    }

    /// Accounts outside the top list whose solves the UI needs.
    pub fn wanted_accounts(&self) -> Vec<u64> {
        // Prefetch the highlighted team so switching to Analysis is instant.
        let highlighted = match self.view {
            View::Board => self
                .board
                .data
                .standings
                .get(self.selected)
                .and_then(|e| e.account_id),
            View::Analysis | View::Plan | View::Challenges => None,
        };
        self.board.wanted_accounts(highlighted)
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

    /// Color of an account's graph line, if it is graphed.
    pub fn line_color(&self, account: Option<u64>) -> Option<Color> {
        self.board
            .line_slot(account)
            .map(|slot| slot.map_or(Color::Reset, theme::series))
    }

    pub fn apply(&mut self, update: Update) {
        let selected_key = self
            .board
            .data
            .standings
            .get(self.selected)
            .map(|e| account_key(e.account_id, &e.name));
        let is_data = matches!(update, Update::Data(_));
        self.board.apply(update);
        if is_data {
            if let Some(i) = selected_key.and_then(|key| self.board.index_of_key(&key)) {
                self.selected = i;
            }
            self.clamp_selection();
        }
        self.clamp_catch_up();
    }

    /// Recompute graph lines, feed, and analytics.
    #[cfg(test)]
    pub fn rebuild(&mut self, mark_fresh: bool) {
        self.board.rebuild(mark_fresh);
        self.clamp_catch_up();
    }

    fn clamp_selection(&mut self) {
        let len = self.board.data.standings.len();
        self.selected = if len == 0 {
            0
        } else {
            self.selected.min(len - 1)
        };
    }

    fn clamp_catch_up(&mut self) {
        let len = self.board.h2h.comparison.catch_up.len();
        let sel = self.catch_state.selected().unwrap_or(0);
        self.catch_state
            .select(Some(if len == 0 { 0 } else { sel.min(len - 1) }));
        let len = self.board.plan.targets.len();
        let sel = self.plan_state.selected().unwrap_or(0);
        self.plan_state
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
        if let Some(flag) = self.flag_input.as_mut() {
            match key.code {
                KeyCode::Esc => self.flag_input = None,
                KeyCode::Backspace => {
                    flag.pop();
                }
                KeyCode::Char(c) => flag.push(c),
                KeyCode::Enter => {
                    let flag = self.flag_input.take().unwrap_or_default();
                    let flag = flag.trim();
                    if let Some(id) = self.challenge_id().filter(|_| !flag.is_empty()) {
                        return Some(Cmd::Submit(id, flag.to_string()));
                    }
                }
                _ => {}
            }
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
            return self.open_challenge().or_else(|| self.sync_wanted());
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
            // D pulls every challenge; d the highlighted target (Plan).
            KeyCode::Char('D') if !self.board.downloading => {
                return Some(Cmd::Download(Download::All));
            }
            KeyCode::Char('d') if self.view == View::Plan && !self.board.downloading => {
                let listed = self.board.plan.listed();
                let picked = listed.get(self.plan_state.selected().unwrap_or(0))?;
                return Some(Cmd::Download(Download::One(picked.stat.id)));
            }
            // Open the highlighted target among the challenges.
            KeyCode::Char('o') if self.view == View::Plan => {
                let listed = self.board.plan.listed();
                if let Some(id) = listed
                    .get(self.plan_state.selected().unwrap_or(0))
                    .map(|t| t.stat.id)
                {
                    self.unsolved_only = false;
                    let at = self.challenges().iter().position(|c| c.id == id);
                    self.chal_state.select(Some(at.unwrap_or(0)));
                    self.detail_scroll = 0;
                    self.switch_view(View::Challenges);
                }
            }
            KeyCode::Char('d') if self.view == View::Challenges && !self.board.downloading => {
                return Some(Cmd::Download(Download::One(self.challenge_id()?)));
            }
            KeyCode::Tab => self.switch_view(match self.view {
                View::Board => View::Analysis,
                View::Analysis => View::Plan,
                View::Plan => View::Challenges,
                View::Challenges => View::Board,
            }),
            KeyCode::BackTab => self.switch_view(match self.view {
                View::Board => View::Challenges,
                View::Analysis => View::Board,
                View::Plan => View::Analysis,
                View::Challenges => View::Plan,
            }),
            KeyCode::Char('1') => self.switch_view(View::Board),
            KeyCode::Char('2') | KeyCode::Char('a') => self.switch_view(View::Analysis),
            KeyCode::Char('3') | KeyCode::Char('p') => self.switch_view(View::Plan),
            KeyCode::Char('4') | KeyCode::Char('c') => self.switch_view(View::Challenges),
            _ => match self.view {
                View::Board => self.board_key(key.code),
                View::Analysis => self.analysis_key(key.code),
                View::Plan => self.plan_key(key.code),
                View::Challenges => self.challenges_key(key.code),
            },
        }
        self.open_challenge().or_else(|| self.sync_wanted())
    }

    fn switch_view(&mut self, view: View) {
        if view == View::Analysis && self.view == View::Board {
            // Compare against the highlighted team unless it is you.
            if let Some(e) = self.board.data.standings.get(self.selected)
                && !self.board.is_me(e)
            {
                let key = account_key(e.account_id, &e.name);
                self.board.set_rival(key);
            } else if self.board.rival_index().is_none() {
                self.board.reset_rival();
            } else {
                self.board.rebuild_h2h();
            }
            self.catch_state.select(Some(0));
            self.clamp_catch_up();
        }
        self.view = view;
    }

    fn move_selection(&mut self, delta: i64) {
        let len = self.board.data.standings.len() as i64;
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
                if let Some(i) = self.board.me_index() {
                    self.selected = i;
                }
            }
            KeyCode::Char('/') => self.search = Some(String::new()),
            KeyCode::Char('n') => {
                if let Some(q) = self.last_search.clone() {
                    self.jump_to_match(&q, self.selected + 1);
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.toggle_pin(),
            KeyCode::Char('t') => {
                let i = RANGES.iter().position(|r| *r == self.range).unwrap_or(0);
                self.range = RANGES[(i + 1) % RANGES.len()];
            }
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
                self.board.reset_rival();
                self.catch_state.select(Some(0));
                self.clamp_catch_up();
            }
            _ => {}
        }
    }

    fn plan_key(&mut self, code: KeyCode) {
        let goal = self.board.plan.goal;
        match code {
            KeyCode::Left | KeyCode::Char('h') => self.board.step_goal(-1),
            KeyCode::Right | KeyCode::Char('l') => self.board.step_goal(1),
            KeyCode::Char('m') => self.board.reset_goal(),
            KeyCode::Char('s') => {
                use crate::board::analysis::Sort;
                let i = Sort::ALL
                    .iter()
                    .position(|s| *s == self.board.plan.sort)
                    .unwrap_or(0);
                self.board.set_sort(Sort::ALL[(i + 1) % Sort::ALL.len()]);
                self.plan_state.select(Some(0));
                return;
            }
            _ => {}
        }
        if self.board.plan.goal != goal {
            // A new goal means a new route: back to its first step.
            self.plan_state.select(Some(0));
            return;
        }
        let delta = match code {
            KeyCode::Up | KeyCode::Char('k') => -1,
            KeyCode::Down | KeyCode::Char('j') => 1,
            KeyCode::PageUp => -10,
            KeyCode::PageDown => 10,
            KeyCode::Home | KeyCode::Char('g') => i64::MIN / 2,
            KeyCode::End | KeyCode::Char('G') => i64::MAX / 2,
            _ => return,
        };
        let len = self.board.plan.targets.len() as i64;
        if len == 0 {
            return;
        }
        let sel = self.plan_state.selected().unwrap_or(0) as i64;
        self.plan_state
            .select(Some((sel + delta).clamp(0, len - 1) as usize));
    }

    /// The challenges the tab lists: all of them, or those left to solve.
    pub fn challenges(&self) -> Vec<&crate::models::Challenge> {
        self.board
            .challenge_list()
            .into_iter()
            .filter(|c| !(self.unsolved_only && c.solved_by_me))
            .collect()
    }

    /// The highlighted challenge on the Challenges tab.
    pub fn challenge_id(&self) -> Option<u64> {
        let list = self.challenges();
        let i = self.chal_state.selected().unwrap_or(0);
        list.get(i.min(list.len().saturating_sub(1))).map(|c| c.id)
    }

    /// Ask for the highlighted challenge's details the first time it is
    /// shown.
    fn open_challenge(&mut self) -> Option<Cmd> {
        if self.view != View::Challenges {
            return None;
        }
        let id = self.challenge_id()?;
        self.board.want_detail(id).then_some(Cmd::Detail(id))
    }

    fn challenges_key(&mut self, code: KeyCode) {
        let len = self.challenges().len() as i64;
        let step = |app: &mut Self, delta: i64| {
            if len > 0 {
                let sel = app.chal_state.selected().unwrap_or(0) as i64;
                app.chal_state
                    .select(Some((sel + delta).clamp(0, len - 1) as usize));
                app.detail_scroll = 0;
            }
        };
        match code {
            KeyCode::Up | KeyCode::Char('k') => step(self, -1),
            KeyCode::Down | KeyCode::Char('j') => step(self, 1),
            KeyCode::Home | KeyCode::Char('g') => step(self, i64::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => step(self, i64::MAX / 2),
            // The description scrolls a few lines at a time.
            KeyCode::PageDown => self.detail_scroll = self.detail_scroll.saturating_add(5),
            KeyCode::PageUp => self.detail_scroll = self.detail_scroll.saturating_sub(5),
            KeyCode::Char('u') => {
                let keep = self.challenge_id();
                self.unsolved_only = !self.unsolved_only;
                let at = keep.and_then(|id| self.challenges().iter().position(|c| c.id == id));
                self.chal_state.select(Some(at.unwrap_or(0)));
                self.detail_scroll = 0;
            }
            KeyCode::Char('/') => self.search = Some(String::new()),
            KeyCode::Char('n') => {
                if let Some(q) = self.last_search.clone() {
                    let from = self.chal_state.selected().unwrap_or(0) + 1;
                    self.jump_to_match(&q, from);
                }
            }
            KeyCode::Char('f') | KeyCode::Enter
                if !self.board.submitting && self.challenge_id().is_some() =>
            {
                self.flag_input = Some(String::new());
            }
            _ => {}
        }
    }

    fn scroll_catch_up(&mut self, delta: i64) {
        let len = self.board.h2h.comparison.catch_up.len() as i64;
        if len == 0 {
            return;
        }
        let sel = self.catch_state.selected().unwrap_or(0) as i64;
        self.catch_state
            .select(Some((sel + delta).clamp(0, len - 1) as usize));
    }

    fn step_rival(&mut self, delta: i64) {
        if let Some(i) = self.board.step_rival(delta) {
            self.selected = i;
            self.catch_state.select(Some(0));
            self.clamp_catch_up();
        }
    }

    fn toggle_pin(&mut self) {
        let Some(id) = self
            .board
            .data
            .standings
            .get(self.selected)
            .and_then(|e| e.account_id)
        else {
            return;
        };
        let pinned = self.board.pins.contains(&id);
        self.board.set_pin(id, !pinned);
    }

    fn jump_to_match(&mut self, query: &str, from: usize) {
        let q = query.to_lowercase();
        if q.is_empty() {
            return;
        }
        if self.view == View::Challenges {
            let list = self.challenges();
            let found = (from..list.len())
                .chain(0..from.min(list.len()))
                .find(|&i| list[i].name.to_lowercase().contains(&q));
            if let Some(i) = found {
                self.chal_state.select(Some(i));
                self.detail_scroll = 0;
            }
            return;
        }
        let standings = &self.board.data.standings;
        let found = (from..standings.len())
            .chain(0..from.min(standings.len()))
            .find(|&i| standings[i].name.to_lowercase().contains(&q));
        if let Some(i) = found {
            self.selected = i;
        }
    }
}
