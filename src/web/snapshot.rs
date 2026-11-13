//! The JSON the web page renders: one self-contained snapshot of the
//! [`BoardModel`], sent on load and after every change.

use crate::board::analysis::{self, CategoryRow, ChallengeStat};
use crate::board::model::{BoardModel, Momentum, Party, account_key};
use serde::Serialize;

#[derive(Serialize)]
pub struct Snapshot {
    pub host: String,
    /// Points scored before this Unix time are ignored.
    pub since: Option<i64>,
    pub demo: bool,
    /// `team` or `player`.
    pub noun: &'static str,
    pub interval: u64,
    pub top_n: u32,
    pub cutoff: Option<u32>,
    /// Server time (Unix seconds) the snapshot was built.
    pub now: i64,
    /// When standings last loaded (Unix seconds).
    pub updated_at: i64,
    pub refreshing: bool,
    pub error: Option<String>,
    pub standings: Vec<Row>,
    pub lines: Vec<Line>,
    pub feed: Vec<Solve>,
    pub h2h: HeadToHead,
    pub plan: Plan,
    /// What just happened, oldest first; the page shows each `id` once.
    pub notices: Vec<Notice>,
}

#[derive(Serialize)]
pub struct Notice {
    pub id: u64,
    /// `your_solve`, `solve`, `rank_up`, `rank_down`, `failed` or
    /// `recovered`.
    pub kind: &'static str,
    pub title: String,
    pub detail: String,
}

#[derive(Serialize)]
pub struct Row {
    pub key: String,
    pub id: Option<u64>,
    pub pos: i64,
    pub name: String,
    pub score: i64,
    pub is_me: bool,
    pub is_rival: bool,
    pub pinned: bool,
    pub can_pin: bool,
    /// Graph line slot: `-1` for you, `null` when not graphed.
    pub slot: Option<i64>,
    /// Movement over the last 15 minutes.
    pub change: Option<Change>,
}

#[derive(Serialize)]
pub struct Change {
    pub rank: i64,
    pub score: i64,
    /// Seconds since the account last moved.
    pub age: u64,
}

#[derive(Serialize)]
pub struct Line {
    pub key: String,
    pub name: String,
    pub slot: i64,
    pub is_me: bool,
    /// `[unix_seconds, score]` steps.
    pub points: Vec<(i64, i64)>,
}

#[derive(Serialize)]
pub struct Solve {
    pub t: i64,
    pub key: String,
    pub team: String,
    pub challenge: String,
    pub category: String,
    pub value: i64,
    pub is_me: bool,
    pub fresh: bool,
}

#[derive(Serialize)]
pub struct Side {
    pub key: String,
    pub pos: i64,
    pub name: String,
    pub score: i64,
    pub solves: Option<usize>,
    pub momentum: Option<Pace>,
}

#[derive(Serialize)]
pub struct Pace {
    pub last_hour: i64,
    pub last_3h: i64,
    pub last_solve: Option<i64>,
}

#[derive(Serialize)]
pub struct Margin {
    pub label: String,
    pub points: i64,
    /// Whether you are currently inside the qualifying places.
    pub inside: bool,
}

#[derive(Serialize)]
pub struct Challenge {
    pub id: u64,
    pub name: String,
    pub category: String,
    pub value: i64,
    pub solves: Option<u64>,
    pub top_solves: usize,
    /// How far into the CTF it was solved, in words, by whoever the list
    /// is about (the rival for catch-up, you for your edge).
    pub solved_in: Option<String>,
}

#[derive(Serialize)]
pub struct Category {
    pub category: String,
    pub total: usize,
    pub total_points: i64,
    pub mine: usize,
    pub mine_points: i64,
    pub theirs: usize,
    pub theirs_points: i64,
}

#[derive(Serialize)]
pub struct HeadToHead {
    pub me: Option<Side>,
    pub rival: Option<Side>,
    pub rival_loading: bool,
    pub cutoff: Option<Margin>,
    pub categories: Vec<Category>,
    pub catch_up: Vec<Challenge>,
    pub catch_up_points: i64,
    /// Index into `catch_up` where solving the list in order would close
    /// the gap to the rival.
    pub closes_gap_at: Option<usize>,
    pub edge: Vec<Challenge>,
    pub edge_points: i64,
    pub field: Vec<Challenge>,
}

/// The climb to the top: see [`crate::board::model::Plan`].
#[derive(Serialize)]
pub struct Plan {
    pub ladder: Vec<Rung>,
    pub targets: Vec<Target>,
    pub focus: Vec<Focus>,
    /// Rank of the team to aim for first.
    pub goal: Option<i64>,
    /// How targets off the route are ordered: `likely`, `per_hour`,
    /// `odds`, `points` or `time`.
    pub sort: &'static str,
    /// Your points per hour over the last three hours.
    pub my_pace: Option<f64>,
    /// How often you solve against the field, and whether you are
    /// closing on the goal, in words.
    pub pace_note: String,
    /// The orders the targets can be put in: key and name.
    pub sorts: Vec<(&'static str, &'static str)>,
}

#[derive(Serialize)]
pub struct Rung {
    pub key: String,
    pub pos: i64,
    pub name: String,
    pub score: i64,
    /// Points ahead of you (negative: behind you).
    pub ahead: i64,
    pub momentum: Option<Pace>,
    /// Solves on the likeliest route past them, and the points it brings.
    pub solves: Option<usize>,
    pub points: Option<i64>,
    /// Challenge IDs on that route, likeliest first.
    pub route: Vec<u64>,
    /// Chance of landing the whole route (0..=1).
    pub odds: Option<f64>,
    /// Your time for the whole route, in words.
    pub time: Option<String>,
    /// Points per hour over the last three hours.
    pub pace: Option<f64>,
    /// Time until you draw level at both current paces, in words.
    pub catch_in: Option<String>,
}

#[derive(Serialize)]
pub struct Target {
    #[serde(flatten)]
    pub challenge: Challenge,
    pub share: f64,
    /// `easy`, `medium`, `hard` or `extreme`, and the word to show.
    pub difficulty: &'static str,
    pub difficulty_label: &'static str,
    /// Whether the difficulty is the organisers' tag, not an estimate.
    pub difficulty_tagged: bool,
    /// Your estimated chance of solving it (0..=1).
    pub odds: f64,
    pub expected: f64,
    /// Solves in the last hour by accounts whose solves are known.
    pub recent: usize,
    /// Why it stands out: `quick_win`, `trending`, `strength`, `rival`
    /// or `unsolved`.
    pub reason: Option<&'static str>,
    /// The same, as the words to show.
    pub why: Option<String>,
    /// How long it should take you, in words, and whether that is a
    /// guess from its difficulty.
    pub time: Option<String>,
    pub time_guessed: bool,
    /// Likely points per hour of that time.
    pub per_hour: Option<f64>,
    /// Everything known about it, as labelled lines (the text behind the
    /// drawn details).
    pub facts: Vec<Fact>,
    /// The same as numbers, for drawing.
    pub stats: Stats,
    pub running: i64,
    pub reach: i64,
    /// Name of the best-ranked account this solve takes you past.
    pub passes: Option<String>,
}

/// One target's signals as numbers.  Times are seconds; `*_text` are the
/// same in words.
#[derive(Serialize)]
pub struct Stats {
    /// Seconds from the start of scoring to now.
    pub elapsed: i64,
    /// Each tracked solve: seconds into the CTF, who, and that in words.
    pub solves: Vec<(i64, String, String)>,
    pub latest_text: Option<String>,
    pub last_hour: usize,
    pub last_3h: usize,
    /// `[solved, out of]` over the field, the graphed top, and the
    /// tracked teams ahead of you.
    pub field: Option<(u64, usize)>,
    pub top: (usize, usize),
    pub ahead: (usize, usize),
    pub odds_low: f64,
    pub odds_high: f64,
    /// What it took the tracked teams: shortest, lower quartile, median,
    /// upper quartile, longest.
    pub spans: Option<[i64; 5]>,
    pub samples: usize,
    pub typical_text: Option<String>,
    /// Your time.
    pub time: Option<i64>,
    /// `[solved by you, challenges]` in its category, and the field's
    /// average solve share there.
    pub category: (usize, usize),
    pub category_share: f64,
    /// 0 (easy) to 3 (extreme).
    pub level: u8,
}

#[derive(Serialize)]
pub struct Fact {
    pub label: String,
    pub value: String,
    /// A long value that wants a line of its own.
    pub wide: bool,
}

#[derive(Serialize)]
pub struct Focus {
    pub category: String,
    pub open: usize,
    pub points: i64,
    pub expected: f64,
    /// Challenges of the category you have solved, and how many it has.
    pub mine: usize,
    pub total: usize,
}

fn pace(m: Option<Momentum>) -> Option<Pace> {
    m.map(|m| Pace {
        last_hour: m.last_hour,
        last_3h: m.last_3h,
        last_solve: m.last_solve,
    })
}

fn side(p: &Party, momentum: Option<Momentum>) -> Side {
    Side {
        key: account_key(p.account, &p.name),
        pos: p.pos,
        name: p.name.clone(),
        score: p.score,
        solves: p.solves,
        momentum: pace(momentum),
    }
}

fn challenge(c: &ChallengeStat, start: i64) -> Challenge {
    Challenge {
        solved_in: c.at.map(|t| analysis::span_text(t - start)),
        id: c.id,
        name: c.name.clone(),
        category: c.category.clone(),
        value: c.value,
        solves: c.solves,
        top_solves: c.top_solves,
    }
}

fn category(c: &CategoryRow) -> Category {
    Category {
        category: c.category.clone(),
        total: c.total,
        total_points: c.total_points,
        mine: c.mine,
        mine_points: c.mine_points,
        theirs: c.theirs,
        theirs_points: c.theirs_points,
    }
}

fn stats(t: &analysis::Target, plan: &crate::board::model::Plan, now: i64) -> Stats {
    let (field, start, ahead_tracked) = plan.context().unwrap_or((
        analysis::Field {
            ranked: 0,
            top_n: 0,
        },
        now,
        0,
    ));
    let timing = &t.timing;
    let spans = timing
        .extent()
        .zip(timing.typical())
        .map(|((min, max), median)| {
            let (q1, q3) = timing.spread().unwrap_or((min, max));
            [min, q1, median, q3, max]
        });
    Stats {
        elapsed: (now - start).max(1),
        solves: timing
            .times
            .iter()
            .zip(&timing.solvers)
            .map(|(&at, who)| (at - start, who.clone(), analysis::span_text(at - start)))
            .collect(),
        latest_text: timing.last().map(|at| analysis::span_text(now - at)),
        last_hour: timing.recent(now, 3600),
        last_3h: timing.recent(now, 3 * 3600),
        field: t
            .stat
            .solves
            .filter(|_| field.ranked > 0)
            .map(|n| (n, field.ranked)),
        top: (t.stat.top_solves, field.top_n),
        ahead: (t.ahead, ahead_tracked),
        odds_low: t.odds_range.0,
        odds_high: t.odds_range.1,
        spans,
        samples: timing.spans.len(),
        typical_text: timing.typical().map(analysis::span_text),
        time: t.time,
        category: t.category_record,
        category_share: t.category_share,
        level: t.difficulty.level as u8,
    }
}

fn slot(s: Option<usize>) -> i64 {
    s.map_or(-1, |s| s as i64)
}

impl Snapshot {
    pub fn of(model: &BoardModel, host: &str) -> Self {
        let rival_index = model.rival_index();
        let top = model.top_ids();
        let standings = model
            .data
            .standings
            .iter()
            .enumerate()
            .map(|(i, e)| Row {
                key: account_key(e.account_id, &e.name),
                id: e.account_id,
                pos: e.pos,
                name: e.name.clone(),
                score: e.score,
                is_me: model.is_me(e),
                is_rival: Some(i) == rival_index && !model.is_me(e),
                pinned: e.account_id.is_some_and(|id| model.pins.contains(&id)),
                can_pin: e
                    .account_id
                    .is_some_and(|id| !top.contains(&id) && model.can_pin(id)),
                slot: model.line_slot(e.account_id).map(slot),
                change: model.change_for(e).map(|c| Change {
                    rank: c.rank_delta,
                    score: c.score_delta,
                    age: c.at.elapsed().as_secs(),
                }),
            })
            .collect();

        let lines = model
            .lines
            .iter()
            .map(|l| Line {
                key: account_key(l.account, &l.name),
                name: l.name.clone(),
                slot: slot(l.slot),
                is_me: l.is_me,
                points: l
                    .points
                    .iter()
                    .map(|&(t, s)| (t as i64, s as i64))
                    .collect(),
            })
            .collect();

        let feed = model
            .feed
            .iter()
            .map(|s| Solve {
                t: s.t,
                key: account_key(Some(s.account), &s.team),
                team: s.team.clone(),
                challenge: s.challenge.clone(),
                category: s.category.clone(),
                value: s.value,
                is_me: s.is_me,
                fresh: s.fresh,
            })
            .collect();

        let start = model.plan.start;
        let h = &model.h2h;
        let c = &h.comparison;
        let gap = match (&h.me, &h.rival) {
            (Some(me), Some(r)) => r.score - me.score,
            _ => 0,
        };
        let h2h = HeadToHead {
            me: h.me.as_ref().map(|p| side(p, h.me_momentum)),
            rival: h.rival.as_ref().map(|p| side(p, h.rival_momentum)),
            rival_loading: h.rival_loading,
            cutoff: h.cutoff.as_ref().map(|m| Margin {
                label: m.label.clone(),
                points: m.points,
                inside: h
                    .me
                    .as_ref()
                    .zip(model.cutoff)
                    .is_some_and(|(me, n)| me.pos <= i64::from(n)),
            }),
            categories: c.categories.iter().map(category).collect(),
            catch_up: c.catch_up.iter().map(|c| challenge(c, start)).collect(),
            catch_up_points: c.catch_up_points(),
            closes_gap_at: c.gap_closing_index(gap),
            edge: c.edge.iter().map(|c| challenge(c, start)).collect(),
            edge_points: c.edge_points(),
            field: c.field.iter().map(|c| challenge(c, start)).collect(),
        };

        let p = &model.plan;
        let plan = Plan {
            ladder: p
                .ladder
                .iter()
                .map(|r| Rung {
                    key: account_key(r.account, &r.name),
                    pos: r.pos,
                    name: r.name.clone(),
                    score: r.score,
                    ahead: r.ahead,
                    momentum: pace(r.momentum),
                    solves: r.to_pass.map(|(n, _)| n),
                    points: r.to_pass.map(|(_, pts)| pts),
                    route: r.route.clone(),
                    odds: r.route_odds,
                    time: r.route_time.map(analysis::span_text),
                    pace: r.pace,
                    catch_in: r.catch_in.map(analysis::span_text),
                })
                .collect(),
            targets: p
                .listed()
                .into_iter()
                .map(|t| Target {
                    challenge: challenge(&t.stat, start),
                    time: t.time.map(analysis::span_text),
                    time_guessed: t.time_guessed,
                    per_hour: t.per_hour,
                    stats: stats(t, p, model.now),
                    facts: p
                        .facts(t, model.now)
                        .into_iter()
                        .map(|f| Fact {
                            label: f.label,
                            value: f.value,
                            wide: f.wide,
                        })
                        .collect(),
                    share: t.share,
                    difficulty: t.difficulty.level.key(),
                    difficulty_label: t.difficulty.level.label(),
                    difficulty_tagged: t.difficulty.tagged,
                    odds: t.odds,
                    expected: t.expected,
                    recent: t.recent,
                    reason: t.reason.map(|r| r.key()),
                    why: t.why(),
                    running: t.running,
                    reach: t.reach,
                    passes: t.passes.as_ref().map(|(_, name)| name.clone()),
                })
                .collect(),
            focus: p
                .focus
                .iter()
                .map(|f| Focus {
                    category: f.category.clone(),
                    open: f.open,
                    points: f.points,
                    expected: f.expected,
                    mine: f.mine,
                    total: f.total,
                })
                .collect(),
            goal: p.goal,
            sort: p.sort.key(),
            my_pace: p.my_pace,
            pace_note: p.pace_note(),
            sorts: analysis::Sort::ALL
                .iter()
                .map(|s| (s.key(), s.label()))
                .collect(),
        };

        Snapshot {
            host: host.to_string(),
            since: model.since,
            demo: model.demo,
            noun: model.kind.noun(),
            interval: model.interval.as_secs(),
            top_n: model.top_n,
            cutoff: model.cutoff,
            now: model.now,
            updated_at: model.updated_at.timestamp(),
            refreshing: model.refreshing,
            error: model.error.clone(),
            standings,
            lines,
            feed,
            h2h,
            plan,
            notices: model
                .live_notices()
                .map(|n| Notice {
                    id: n.id,
                    kind: n.kind.key(),
                    title: n.title.clone(),
                    detail: n.detail.clone(),
                })
                .collect(),
        }
    }
}
