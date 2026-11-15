//! Notices derived from refresh failures, rank changes, solves, and actions.

use super::BoardModel;
use std::time::{Duration, Instant};

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
impl BoardModel {
    /// Add a notice for the views to show.
    pub(super) fn notify(&mut self, kind: NoticeKind, title: String, detail: String) {
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
    pub(super) fn notify_rank(&mut self, was: Option<i64>) {
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

    /// One notice per new solve, yours first; a busy update is summed up.
    pub(super) fn notify_solves(&mut self, mut news: Vec<(bool, String, String, i64)>) {
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
}
