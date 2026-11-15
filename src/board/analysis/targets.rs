//! Rank unsolved challenges by expected points, time, and category strength.

use super::difficulty::difficulty;
use super::{ChallengeStat, Difficulty, Level, Timing, share_range, span_text};
use crate::models::Challenge;
use std::collections::{BTreeMap, HashMap, HashSet};

/// How the targets off the route are ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    /// Points weighted by your odds.
    #[default]
    Likely,
    /// The same, per hour of the time it should take you.
    PerHour,
    Odds,
    Points,
    /// Quickest first.
    Time,
}

impl Sort {
    pub const ALL: [Sort; 5] = [
        Sort::Likely,
        Sort::PerHour,
        Sort::Odds,
        Sort::Points,
        Sort::Time,
    ];

    /// Stable key for the web page.
    pub fn key(self) -> &'static str {
        match self {
            Sort::Likely => "likely",
            Sort::PerHour => "per_hour",
            Sort::Odds => "odds",
            Sort::Points => "points",
            Sort::Time => "time",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            Sort::Likely => "likely points",
            Sort::PerHour => "likely points per hour",
            Sort::Odds => "odds",
            Sort::Points => "points",
            Sort::Time => "time",
        }
    }

    /// Order two targets, best first; ties fall back to likely points.
    pub fn compare(self, a: &Target, b: &Target) -> std::cmp::Ordering {
        let by_likely = || b.expected.total_cmp(&a.expected);
        match self {
            Sort::Likely => by_likely(),
            Sort::PerHour => b
                .per_hour
                .unwrap_or(0.0)
                .total_cmp(&a.per_hour.unwrap_or(0.0))
                .then_with(by_likely),
            Sort::Odds => b.odds.total_cmp(&a.odds).then_with(by_likely),
            Sort::Points => b.stat.value.cmp(&a.stat.value).then_with(by_likely),
            Sort::Time => a
                .time
                .unwrap_or(i64::MAX)
                .cmp(&b.time.unwrap_or(i64::MAX))
                .then_with(by_likely),
        }
        .then_with(|| a.stat.name.cmp(&b.stat.name))
    }
}

/// One line of the details shown for a target: a label and what it says.
#[derive(Debug, Clone, PartialEq)]
pub struct Fact {
    pub label: String,
    pub value: String,
    /// A long value that wants a line of its own.
    pub wide: bool,
}

/// Why a target stands out, when something does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Most of the field has it: cheap points.
    QuickWin,
    /// Being solved right now by the accounts whose solves are known.
    Trending,
    /// In a category you solve more of than your average.
    Strength,
    /// The rival you compare with already has it.
    Rival,
    /// Nobody has solved it.
    Unsolved,
}

impl Target {
    /// Everything known about this target, as labelled lines for the
    /// details panel.  `start` is when scoring began, `ahead_tracked` how
    /// many accounts above you have known solves.
    pub fn facts(&self, field: Field, now: i64, start: i64, ahead_tracked: usize) -> Vec<Fact> {
        let pct = |p: f64| {
            if p > 0.0 && p < 0.005 {
                "<1%".to_string()
            } else {
                format!("{:.0}%", p * 100.0)
            }
        };
        let mut facts = Vec::new();
        let mut add = |label: &str, value: String| {
            facts.push(Fact {
                label: label.into(),
                value,
                wide: false,
            })
        };

        match self.stat.solves {
            Some(n) if field.ranked > 0 => add(
                "Solved by",
                format!("{n} of {} ({})", field.ranked, pct(self.share)),
            ),
            _ => {}
        }
        if field.top_n > 0 {
            add(
                &format!("Top {}", field.top_n),
                format!("{} of {}", self.stat.top_solves, field.top_n),
            );
        }
        if ahead_tracked > 0 {
            add(
                "Teams ahead of you",
                format!("{} of {ahead_tracked} have it", self.ahead),
            );
        }
        add(
            "Odds",
            format!(
                "{} (likely {} to {})",
                pct(self.odds),
                pct(self.odds_range.0),
                pct(self.odds_range.1)
            ),
        );
        add(
            "Difficulty",
            format!(
                "{}, {}",
                self.difficulty.level.label(),
                if self.difficulty.tagged {
                    "tagged"
                } else {
                    "estimated"
                }
            ),
        );
        add(
            "Likely points",
            match self.per_hour {
                Some(rate) => format!("{:.0}, {:.0} per hour", self.expected, rate),
                None => format!("{:.0}", self.expected),
            },
        );
        match self.timing.first() {
            Some(t) => add(
                "First solve",
                match self.timing.solvers.first() {
                    Some(who) => format!("{} in, by {who}", span_text(t - start)),
                    None => format!("{} in", span_text(t - start)),
                },
            ),
            None => add("First solve", "None yet".into()),
        }
        if let Some(t) = self.timing.last() {
            add("Latest solve", format!("{} ago", span_text(now - t)));
        }

        add(
            "Last hour, last 3 h",
            format!(
                "{} and {} solves",
                self.timing.recent(now, HOUR),
                self.timing.recent(now, 3 * HOUR)
            ),
        );
        if let Some(typical) = self.timing.typical() {
            let n = self.timing.spans.len();
            add(
                "Typical time",
                match self.timing.spread() {
                    Some((lo, hi)) => format!(
                        "{} ({} to {}, {n} solves)",
                        span_text(typical),
                        span_text(lo),
                        span_text(hi)
                    ),
                    None => format!(
                        "{} ({n} {})",
                        span_text(typical),
                        if n == 1 { "solve" } else { "solves" }
                    ),
                },
            );
        }
        if let Some(time) = self.time {
            add(
                "Your time",
                if self.time_guessed {
                    format!("about {}, from its difficulty", span_text(time))
                } else {
                    format!("about {}, at your pace", span_text(time))
                },
            );
        }
        let (mine, total) = self.category_record;
        add(
            &format!(
                "Your {}",
                if self.stat.category.is_empty() {
                    "category"
                } else {
                    &self.stat.category
                }
            ),
            format!(
                "{mine} of {total} solved, field averages {}",
                pct(self.category_share)
            ),
        );
        // How far into the CTF each tracked team solved it.
        if !self.timing.times.is_empty() {
            const SHOWN: usize = 6;
            let mut when: Vec<String> = self
                .timing
                .solvers
                .iter()
                .zip(&self.timing.times)
                .take(SHOWN)
                .map(|(who, &t)| format!("{who} {} in", span_text(t - start)))
                .collect();
            let more = self.timing.times.len().saturating_sub(SHOWN);
            if more > 0 {
                when.push(format!("{more} more"));
            }
            facts.push(Fact {
                label: "Solved at".into(),
                value: when.join(", "),
                wide: true,
            });
        }
        facts
    }

    /// Why this target stands out, in a few words.
    pub fn why(&self) -> Option<String> {
        Some(match self.reason? {
            Reason::QuickWin => "Quick win".into(),
            Reason::Trending => format!("{} solves this hour", self.recent),
            Reason::Strength => "Your category".into(),
            Reason::Rival => "Rival has it".into(),
            Reason::Unsolved => "No solves yet".into(),
        })
    }
}

impl Reason {
    /// Stable key for the web page.
    pub fn key(self) -> &'static str {
        match self {
            Reason::QuickWin => "quick_win",
            Reason::Trending => "trending",
            Reason::Strength => "strength",
            Reason::Rival => "rival",
            Reason::Unsolved => "unsolved",
        }
    }
}

/// An unsolved challenge, ranked for the climb to the top.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub stat: ChallengeStat,
    /// Share of accounts that solved it (0..=1): a stand-in for difficulty.
    pub share: f64,
    pub difficulty: Difficulty,
    /// Your estimated chance of solving it (0..=1): `share` weighed
    /// against what its difficulty suggests, then shifted by how you do
    /// in its category compared with your average.
    pub odds: f64,
    /// Points weighted by `odds`: what an attempt is likely worth.
    pub expected: f64,
    /// The range the odds plausibly lie in, given how many teams the
    /// solve share rests on.
    pub odds_range: (f64, f64),
    /// Solves in the last hour by accounts whose solves are known.
    pub recent: usize,
    /// When those accounts solved it.
    pub timing: Timing,
    /// How long it should take you, in seconds: what it typically took
    /// the accounts that solved it, scaled by your pace against theirs.
    pub time: Option<i64>,
    /// Whether `time` is guessed from the difficulty because no tracked
    /// account has solved it.
    pub time_guessed: bool,
    /// Likely points per hour of `time`.
    pub per_hour: Option<f64>,
    /// Tracked accounts ranked above you that have solved it.
    pub ahead: usize,
    /// The rest of the field's average solve share in its category.
    pub category_share: f64,
    /// Challenges of its category you have solved, and how many it has.
    pub category_record: (usize, usize),
    pub reason: Option<Reason>,
    /// Your points gain after solving the list down to here.
    pub running: i64,
    /// The rank you would hold then, if nobody else scores.
    pub reach: i64,
    /// The best-ranked account this solve takes you past, when the rank
    /// improves here: (its rank, its name).
    pub passes: Option<(i64, String)>,
}

/// Open points in one category, for deciding where to spend time.
#[derive(Debug, Clone, PartialEq)]
pub struct Focus {
    pub category: String,
    pub open: usize,
    pub points: i64,
    pub expected: f64,
    /// Challenges of this category you have solved, and how many it has.
    pub mine: usize,
    pub total: usize,
}

/// What the board knows about solve rates.
#[derive(Debug, Clone, Copy)]
pub struct Field {
    /// Accounts on the scoreboard (for overall solve counts).
    pub ranked: usize,
    /// Accounts in `top_counts`.
    pub top_n: usize,
}

/// Signals beyond solve counts that shape the ranking.
#[derive(Debug, Clone, Copy)]
pub struct Signals<'a> {
    /// Challenge ID to when the accounts with known solves solved it.
    pub timing: &'a HashMap<u64, Timing>,
    /// Challenges the compared rival has solved.
    pub rival: &'a HashSet<u64>,
    /// Challenge ID to how many tracked accounts above you solved it.
    pub ahead: &'a HashMap<u64, usize>,
    /// Seconds a solve typically takes you, and the tracked field.
    pub my_typical: Option<i64>,
    pub field_typical: Option<i64>,
    /// Unix time now.
    pub now: i64,
}

/// Seconds that count as "the last hour".
const HOUR: i64 = 3600;

/// How long a challenge of this level takes against the typical solve,
/// when nobody tracked has solved it yet.
fn level_time(level: Level) -> f64 {
    match level {
        Level::Easy => 0.5,
        Level::Medium => 1.0,
        Level::Hard => 2.0,
        Level::Extreme => 4.0,
    }
}

/// Solves in the last hour that mark a challenge as trending.
const TRENDING: usize = 2;
/// Solve share from which a challenge counts as a quick win.
const QUICK_WIN: f64 = 0.5;
/// Category strength from which it counts as yours, with this many solves.
const STRENGTH: f64 = 1.25;
const STRENGTH_SOLVES: usize = 2;

/// How you do in a category against your own average: above 1 when you
/// solve more of it than of the CTF as a whole.  Pulled toward 1 while
/// the category is small, and kept within 0.5..=2.
fn strength(mine: usize, total: usize, all_mine: usize, all_total: usize) -> f64 {
    if all_mine == 0 || all_total == 0 {
        return 1.0;
    }
    let overall = all_mine as f64 / all_total as f64;
    let rate = (mine as f64 + 2.0 * overall) / (total as f64 + 2.0);
    (rate / overall).clamp(0.5, 2.0)
}

/// Rank every challenge you haven't solved by likely points.
///
/// `others` are the other accounts' scores; reaching a rank means
/// overtaking everyone with a higher or equal score (on a tie, whoever got
/// there first stays ahead, and that is not you).
pub fn targets(
    challenges: &[Challenge],
    mine: &HashSet<u64>,
    top_counts: &HashMap<u64, usize>,
    field: Field,
    signals: Signals,
    my_score: i64,
    others: &[i64],
) -> (Vec<Target>, Vec<Focus>) {
    // (solved by you, challenges) per category.
    let mut record: HashMap<&str, (usize, usize)> = HashMap::new();
    for c in challenges {
        let r = record.entry(c.category.as_str()).or_default();
        r.0 += usize::from(mine.contains(&c.id));
        r.1 += 1;
    }
    let all_mine = challenges.iter().filter(|c| mine.contains(&c.id)).count();

    // (solvers, out of) for a challenge, when the board knows either.
    let solved_by = |c: &Challenge, top: usize| match c.solves {
        Some(n) if field.ranked > 0 => Some(((n as usize).min(field.ranked), field.ranked)),
        _ if field.top_n > 0 => Some((top.min(field.top_n), field.top_n)),
        _ => None,
    };
    // Solve counts say nothing about difficulty until someone has solved
    // something; point values only when they differ.
    let top_of = |c: &Challenge| top_counts.get(&c.id).copied().unwrap_or(0);
    let board_has_solves = challenges
        .iter()
        .any(|c| solved_by(c, top_of(c)).is_some_and(|(n, _)| n > 0));
    let cheapest = challenges.iter().map(|c| c.value).min().unwrap_or(0);
    let dearest = challenges.iter().map(|c| c.value).max().unwrap_or(0);
    let mut list: Vec<Target> = challenges
        .iter()
        .filter(|c| !mine.contains(&c.id))
        .map(|c| {
            let top_solves = top_of(c);
            let known = solved_by(c, top_solves);
            let share = known.map_or(0.0, |(n, of)| n as f64 / of as f64);
            let (cat_mine, cat_total) = record[c.category.as_str()];
            let fit = strength(cat_mine, cat_total, all_mine, challenges.len());
            let difficulty = difficulty(
                &c.tags,
                known.filter(|_| board_has_solves).map(|_| share),
                (dearest > cheapest)
                    .then(|| (c.value - cheapest) as f64 / (dearest - cheapest) as f64),
            );
            // The solve share, pulled toward what the difficulty suggests
            // by a prior worth a twentieth of the field (at least four
            // teams): early on the difficulty leads, later the solves do,
            // and an unsolved challenge is unlikely, not impossible.
            let prior = difficulty.level.prior();
            let base = known.map_or(prior, |(n, of)| {
                let weight = (of as f64 / 20.0).max(4.0);
                (n as f64 + weight * prior) / (of as f64 + weight)
            });
            let personal = |base: f64| {
                let base = base.clamp(0.0, 0.999);
                let scaled = base / (1.0 - base) * fit;
                scaled / (1.0 + scaled)
            };
            let odds = personal(base);
            let odds_range = match known {
                Some((n, of)) => {
                    let weight = (of as f64 / 20.0).max(4.0);
                    let blend =
                        |share: f64| (share * of as f64 + weight * prior) / (of as f64 + weight);
                    let (lo, hi) = share_range(n, of);
                    (personal(blend(lo)), personal(blend(hi)))
                }
                None => (odds, odds),
            };
            let timing = signals.timing.get(&c.id).cloned().unwrap_or_default();
            let recent = timing.recent(signals.now, HOUR);
            // Your time: what it took the others, at your pace against
            // theirs; with no solves to go on, the typical solve scaled by
            // the difficulty.
            let pace = match (signals.my_typical, signals.field_typical) {
                (Some(mine), Some(field)) if field > 0 => {
                    (mine as f64 / field as f64).clamp(0.5, 2.0)
                }
                _ => 1.0,
            };
            let (time, time_guessed) = match (timing.typical(), signals.field_typical) {
                (Some(typical), _) => (Some((typical as f64 * pace).round() as i64), false),
                (None, Some(field)) => (
                    Some((field as f64 * level_time(difficulty.level) * pace).round() as i64),
                    true,
                ),
                (None, None) => (None, false),
            };
            let expected = c.value as f64 * odds;
            let per_hour = time
                .filter(|&t| t > 0)
                .map(|t| expected * HOUR as f64 / t as f64);
            let reason = if recent >= TRENDING {
                Some(Reason::Trending)
            } else if share >= QUICK_WIN {
                Some(Reason::QuickWin)
            } else if fit >= STRENGTH && cat_mine >= STRENGTH_SOLVES {
                Some(Reason::Strength)
            } else if signals.rival.contains(&c.id) {
                Some(Reason::Rival)
            } else if known.is_some_and(|(n, _)| n == 0) {
                Some(Reason::Unsolved)
            } else {
                None
            };
            Target {
                stat: ChallengeStat {
                    id: c.id,
                    name: c.name.clone(),
                    category: c.category.clone(),
                    value: c.value,
                    solves: c.solves,
                    top_solves,
                    at: None,
                },
                share,
                difficulty,
                odds,
                odds_range,
                expected,
                recent,
                timing,
                time,
                time_guessed,
                per_hour,
                ahead: signals.ahead.get(&c.id).copied().unwrap_or(0),
                category_share: 0.0,
                category_record: (cat_mine, cat_total),
                reason,
                running: 0,
                reach: 0,
                passes: None,
            }
        })
        .collect();
    list.sort_by(|a, b| {
        b.expected
            .total_cmp(&a.expected)
            .then(b.stat.value.cmp(&a.stat.value))
            .then(a.stat.name.cmp(&b.stat.name))
    });

    // The field's average solve share per category, over every challenge.
    let mut field_rate: HashMap<&str, (f64, usize)> = HashMap::new();
    for c in challenges {
        if let Some((n, of)) = solved_by(c, top_of(c)) {
            let e = field_rate.entry(c.category.as_str()).or_default();
            e.0 += n as f64 / of as f64;
            e.1 += 1;
        }
    }
    for t in &mut list {
        if let Some(&(sum, count)) = field_rate.get(t.stat.category.as_str()) {
            t.category_share = sum / count as f64;
        }
    }

    let mut running = 0;
    for t in &mut list {
        running += t.stat.value;
        t.running = running;
        let score = my_score + running;
        t.reach = 1 + others.iter().filter(|&&o| o >= score).count() as i64;
    }

    let mut focus: BTreeMap<&str, Focus> = BTreeMap::new();
    for t in &list {
        let f = focus.entry(t.stat.category.as_str()).or_insert_with(|| {
            let (mine, total) = record[t.stat.category.as_str()];
            Focus {
                category: t.stat.category.clone(),
                open: 0,
                points: 0,
                expected: 0.0,
                mine,
                total,
            }
        });
        f.open += 1;
        f.points += t.stat.value;
        f.expected += t.expected;
    }
    let mut focus: Vec<Focus> = focus.into_values().collect();
    focus.sort_by(|a, b| {
        b.expected
            .total_cmp(&a.expected)
            .then(a.category.cmp(&b.category))
    });
    (list, focus)
}
