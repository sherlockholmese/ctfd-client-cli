//! Head-to-head analytics: where a rival out-scores you, and which of
//! their solves you could realistically catch up on.  [`targets`] ranks
//! everything you haven't solved against the whole board.

use crate::models::Challenge;
use std::collections::{BTreeMap, HashMap, HashSet};

/// A challenge annotated with how popular it is.
#[derive(Debug, Clone, PartialEq)]
pub struct ChallengeStat {
    pub id: u64,
    pub name: String,
    pub category: String,
    pub value: i64,
    /// Total solves across the CTF, when CTFd reports it.
    pub solves: Option<u64>,
    /// How many of the graphed top accounts solved it.
    pub top_solves: usize,
    /// When it was solved (Unix time) by whoever the list is about: the
    /// rival for catch-up targets, you for your edge.
    pub at: Option<i64>,
}

/// Per-category totals for you and a rival.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CategoryRow {
    pub category: String,
    pub total: usize,
    pub total_points: i64,
    pub mine: usize,
    pub mine_points: i64,
    pub theirs: usize,
    pub theirs_points: i64,
}

impl CategoryRow {
    /// Your points minus theirs (negative: they lead in this category).
    pub fn delta(&self) -> i64 {
        self.mine_points - self.theirs_points
    }
}

/// Result of comparing your solves with a rival's.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Comparison {
    /// Categories, biggest deficit first.
    pub categories: Vec<CategoryRow>,
    /// Solved by the rival but not by you, most-solved (easiest) first.
    pub catch_up: Vec<ChallengeStat>,
    /// Solved by you but not by the rival, highest value first.
    pub edge: Vec<ChallengeStat>,
    /// Solved by neither of you, most popular among top accounts first
    /// (challenges nobody has solved come last).
    pub field: Vec<ChallengeStat>,
}

impl Comparison {
    /// Total points available in [`Comparison::catch_up`].
    pub fn catch_up_points(&self) -> i64 {
        self.catch_up.iter().map(|c| c.value).sum()
    }

    /// Total points in [`Comparison::edge`].
    pub fn edge_points(&self) -> i64 {
        self.edge.iter().map(|c| c.value).sum()
    }

    /// Index of the first catch-up target at which the running total
    /// reaches `gap`, if solving the list (in order) would close it.
    pub fn gap_closing_index(&self, gap: i64) -> Option<usize> {
        if gap <= 0 {
            return None;
        }
        let mut sum = 0;
        self.catch_up.iter().position(|c| {
            sum += c.value;
            sum >= gap
        })
    }
}

/// When you and the rival solved each challenge (challenge ID to Unix
/// time), where known.
#[derive(Debug, Clone, Copy)]
pub struct SolvedAt<'a> {
    pub mine: &'a HashMap<u64, i64>,
    pub theirs: &'a HashMap<u64, i64>,
}

/// Compare `mine` with `theirs` over the visible `challenges`.
///
/// `top_counts` maps challenge ID to how many top accounts solved it; it
/// drives the "field" list and breaks ties when sorting.
pub fn compare(
    challenges: &[Challenge],
    mine: &HashSet<u64>,
    theirs: &HashSet<u64>,
    top_counts: &HashMap<u64, usize>,
    solved_at: SolvedAt,
) -> Comparison {
    let stat = |c: &Challenge, at: Option<i64>| ChallengeStat {
        at,
        id: c.id,
        name: c.name.clone(),
        category: c.category.clone(),
        value: c.value,
        solves: c.solves,
        top_solves: top_counts.get(&c.id).copied().unwrap_or(0),
    };

    let mut categories: BTreeMap<&str, CategoryRow> = BTreeMap::new();
    let mut catch_up = Vec::new();
    let mut edge = Vec::new();
    let mut field = Vec::new();

    for c in challenges {
        let row = categories
            .entry(c.category.as_str())
            .or_insert_with(|| CategoryRow {
                category: c.category.clone(),
                ..Default::default()
            });
        row.total += 1;
        row.total_points += c.value;

        let i_solved = mine.contains(&c.id);
        let they_solved = theirs.contains(&c.id);
        if i_solved {
            row.mine += 1;
            row.mine_points += c.value;
        }
        if they_solved {
            row.theirs += 1;
            row.theirs_points += c.value;
        }

        match (i_solved, they_solved) {
            (false, true) => catch_up.push(stat(c, solved_at.theirs.get(&c.id).copied())),
            (true, false) => edge.push(stat(c, solved_at.mine.get(&c.id).copied())),
            _ => {}
        }
        if !i_solved && !they_solved {
            field.push(stat(c, None));
        }
    }

    // Easiest first: most solves overall, then most top-team solves, then
    // the most points for the effort.
    let popularity = |c: &ChallengeStat| (c.solves.unwrap_or(0), c.top_solves);
    catch_up.sort_by(|a, b| {
        popularity(b)
            .cmp(&popularity(a))
            .then(b.value.cmp(&a.value))
            .then(a.name.cmp(&b.name))
    });
    edge.sort_by(|a, b| b.value.cmp(&a.value).then(a.name.cmp(&b.name)));
    field.sort_by(|a, b| {
        b.top_solves
            .cmp(&a.top_solves)
            .then(b.solves.unwrap_or(0).cmp(&a.solves.unwrap_or(0)))
            .then(b.value.cmp(&a.value))
            .then(a.name.cmp(&b.name))
    });

    let mut categories: Vec<CategoryRow> = categories.into_values().collect();
    categories.sort_by(|a, b| a.delta().cmp(&b.delta()).then(a.category.cmp(&b.category)));

    Comparison {
        categories,
        catch_up,
        edge,
        field,
    }
}

/// How hard a challenge is, in four steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Easy,
    Medium,
    Hard,
    Extreme,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::Easy => "Easy",
            Level::Medium => "Medium",
            Level::Hard => "Hard",
            Level::Extreme => "Extreme",
        }
    }

    /// Stable key for the web page.
    pub fn key(self) -> &'static str {
        match self {
            Level::Easy => "easy",
            Level::Medium => "medium",
            Level::Hard => "hard",
            Level::Extreme => "extreme",
        }
    }

    /// Chance that a team solves a challenge of this level, before any
    /// solve counts are in.
    fn prior(self) -> f64 {
        match self {
            Level::Easy => 0.6,
            Level::Medium => 0.3,
            Level::Hard => 0.12,
            Level::Extreme => 0.04,
        }
    }

    /// The level an organiser's tag names, if it names one.
    fn from_tag(tag: &str) -> Option<Self> {
        let tag = tag.trim().to_lowercase();
        let tag = tag.strip_prefix("difficulty:").unwrap_or(&tag).trim();
        Some(match tag {
            "baby" | "warmup" | "warm-up" | "beginner" | "intro" | "trivial" | "easy" => {
                Level::Easy
            }
            "medium" | "normal" | "intermediate" | "moderate" => Level::Medium,
            "hard" | "difficult" | "advanced" => Level::Hard,
            "insane" | "expert" | "extreme" | "nightmare" | "very hard" | "brutal" => {
                Level::Extreme
            }
            _ => return None,
        })
    }
}

/// A challenge's difficulty and where the reading comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Difficulty {
    pub level: Level,
    /// From an organiser's tag, rather than estimated from solves and
    /// points.
    pub tagged: bool,
}

/// Solve share at or above which a challenge reads as easy on solves
/// alone, and at or below which as hard as it gets.
const SHARE_EASY: f64 = 0.5;
const SHARE_HARDEST: f64 = 0.02;

/// Read a challenge's difficulty: its tag when it has one, else an
/// estimate from how few solved it (`share`, when the board has solves)
/// and where its points sit between the cheapest and dearest challenge
/// (`points`, 0..=1, when values differ).  Solves count double.
fn difficulty(tags: &[String], share: Option<f64>, points: Option<f64>) -> Difficulty {
    if let Some(level) = tags.iter().find_map(|t| Level::from_tag(t)) {
        return Difficulty {
            level,
            tagged: true,
        };
    }
    let by_share = share.map(|s| {
        let s = s.clamp(SHARE_HARDEST, SHARE_EASY);
        (SHARE_EASY / s).ln() / (SHARE_EASY / SHARE_HARDEST).ln()
    });
    let score = match (by_share, points) {
        (Some(s), Some(p)) => (2.0 * s + p) / 3.0,
        (Some(s), None) => s,
        (None, Some(p)) => p,
        (None, None) => 0.4,
    };
    let level = match score {
        s if s < 0.25 => Level::Easy,
        s if s < 0.5 => Level::Medium,
        s if s < 0.8 => Level::Hard,
        _ => Level::Extreme,
    };
    Difficulty {
        level,
        tagged: false,
    }
}

/// When the accounts whose solves are known solved one challenge.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Timing {
    /// Unix time of each known solve, oldest first.
    pub times: Vec<i64>,
    /// For each of those solves, the seconds since that account's
    /// previous solve (or the start): the most it can have spent on it.
    pub spans: Vec<i64>,
    /// Who made each of those solves.
    pub solvers: Vec<String>,
}

impl Timing {
    pub fn first(&self) -> Option<i64> {
        self.times.first().copied()
    }

    pub fn last(&self) -> Option<i64> {
        self.times.last().copied()
    }

    /// Known solves in the `secs` before `now`.
    pub fn recent(&self, now: i64, secs: i64) -> usize {
        self.times.iter().filter(|&&t| t >= now - secs).count()
    }

    /// The median span: what the challenge typically took.
    pub fn typical(&self) -> Option<i64> {
        quantile(&self.spans, 0.5)
    }

    /// The shortest and longest span.
    pub fn extent(&self) -> Option<(i64, i64)> {
        Some((*self.spans.iter().min()?, *self.spans.iter().max()?))
    }

    /// The middle half of the spans: (lower quartile, upper quartile).
    pub fn spread(&self) -> Option<(i64, i64)> {
        if self.spans.len() < 3 {
            return None;
        }
        Some((quantile(&self.spans, 0.25)?, quantile(&self.spans, 0.75)?))
    }
}

/// The `q` quantile (0..=1) of `values`, interpolated; `None` when empty.
pub fn quantile(values: &[i64], q: f64) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let at = q.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let (lo, hi) = (at.floor() as usize, at.ceil() as usize);
    let part = at - lo as f64;
    Some((sorted[lo] as f64 + (sorted[hi] - sorted[lo]) as f64 * part).round() as i64)
}

/// The range a share of `n` out of `of` plausibly lies in (Wilson score
/// interval, 80%).
pub fn share_range(n: usize, of: usize) -> (f64, f64) {
    if of == 0 {
        return (0.0, 1.0);
    }
    const Z: f64 = 1.2816;
    let (n, of) = (n.min(of) as f64, of as f64);
    let p = n / of;
    let denom = 1.0 + Z * Z / of;
    let centre = p + Z * Z / (2.0 * of);
    let margin = Z * (p * (1.0 - p) / of + Z * Z / (4.0 * of * of)).sqrt();
    (
        ((centre - margin) / denom).clamp(0.0, 1.0),
        ((centre + margin) / denom).clamp(0.0, 1.0),
    )
}

/// A length of time in words: `45m`, `1h 05m`, `2d 3h`.
pub fn span_text(secs: i64) -> String {
    let secs = secs.max(0);
    if secs < 60 {
        "under 1m".into()
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h {:02}m", secs / 3600, secs % 3600 / 60)
    } else {
        format!("{}d {}h", secs / 86_400, secs % 86_400 / 3600)
    }
}

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

/// The likeliest set of solves that gains a number of points.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    /// Indices into the target list, likeliest solve first.
    pub picks: Vec<usize>,
    /// Points the picks bring.
    pub points: i64,
    /// Chance of landing every pick, taking them as independent.
    pub odds: f64,
}

/// Odds below this cost the same: a guess, not a measurement.
const ODDS_FLOOR: f64 = 0.01;
/// Cost added per solve, so a shorter route wins a near tie.
const PER_SOLVE: f64 = 0.05;
/// Point steps the search works in at most; larger gaps are coarsened.
const ROUTE_STEPS: i64 = 4096;

/// Pick the targets most likely to all be solved that together gain more
/// than `behind` points (enough to pass an account that far ahead).
///
/// `None` when you are already ahead, or when everything open is not
/// enough.  The search is exact while `behind` is under [`ROUTE_STEPS`]
/// points; past that, values are rounded down to a coarser unit, so the
/// route still clears the gap but may not be the single best one.
pub fn route(targets: &[Target], behind: i64) -> Option<Route> {
    if behind < 0 {
        return None;
    }
    let need = behind + 1;
    let unit = (need + ROUTE_STEPS - 1) / ROUTE_STEPS;
    let cap = ((need + unit - 1) / unit) as usize;
    let cost = |t: &Target| -t.odds.max(ODDS_FLOOR).ln() + PER_SOLVE;

    // best[j]: least cost of gaining at least j units from the targets
    // seen so far; took[i][j]: whether target i is part of that.
    let mut best = vec![f64::INFINITY; cap + 1];
    best[0] = 0.0;
    let mut took = vec![vec![false; cap + 1]; targets.len()];
    for (i, t) in targets.iter().enumerate() {
        let weight = (t.stat.value / unit).max(0) as usize;
        if weight == 0 {
            continue;
        }
        let c = cost(t);
        for j in (1..=cap).rev() {
            let with = best[j.saturating_sub(weight)] + c;
            if with < best[j] {
                best[j] = with;
                took[i][j] = true;
            }
        }
    }
    if !best[cap].is_finite() {
        return None;
    }

    let mut picks = Vec::new();
    let mut j = cap;
    for i in (0..targets.len()).rev() {
        if j > 0 && took[i][j] {
            picks.push(i);
            j = j.saturating_sub((targets[i].stat.value / unit) as usize);
        }
    }
    picks.sort_by(|&a, &b| targets[b].odds.total_cmp(&targets[a].odds).then(a.cmp(&b)));
    Some(Route {
        points: picks.iter().map(|&i| targets[i].stat.value).sum(),
        odds: picks.iter().map(|&i| targets[i].odds).product(),
        picks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(id: u64, name: &str, category: &str, value: i64, solves: u64) -> Challenge {
        serde_json::from_value(serde_json::json!({
            "id": id, "name": name, "category": category, "value": value,
            "solved_by_me": false, "solves": solves
        }))
        .unwrap()
    }

    fn no_times() -> SolvedAt<'static> {
        static EMPTY: std::sync::LazyLock<HashMap<u64, i64>> =
            std::sync::LazyLock::new(HashMap::new);
        SolvedAt {
            mine: &EMPTY,
            theirs: &EMPTY,
        }
    }

    fn set(ids: &[u64]) -> HashSet<u64> {
        ids.iter().copied().collect()
    }

    fn sample() -> Vec<Challenge> {
        vec![
            ch(1, "warmup", "Misc", 50, 90),
            ch(2, "heap", "Pwn", 500, 3),
            ch(3, "rop", "Pwn", 300, 20),
            ch(4, "xss", "Web", 200, 40),
            ch(5, "rsa", "Crypto", 250, 15),
        ]
    }

    #[test]
    fn splits_catch_up_edge_and_field() {
        let top: HashMap<u64, usize> = [(1, 10), (3, 6), (4, 7), (2, 1)].into();
        let cmp = compare(
            &sample(),
            &set(&[1, 5]),
            &set(&[1, 2, 3, 4]),
            &top,
            no_times(),
        );

        let names: Vec<&str> = cmp.catch_up.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["xss", "rop", "heap"], "most-solved first");
        assert_eq!(cmp.catch_up_points(), 1000);
        assert_eq!(cmp.edge.len(), 1);
        assert_eq!(cmp.edge[0].name, "rsa");

        assert!(
            cmp.field.is_empty(),
            "everything open for you is in catch-up"
        );

        let cmp = compare(&sample(), &set(&[1]), &set(&[2]), &top, no_times());
        let field: Vec<&str> = cmp.field.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            field,
            ["xss", "rop", "rsa"],
            "solved by neither, popular first"
        );
    }

    #[test]
    fn categories_sorted_by_deficit() {
        let cmp = compare(
            &sample(),
            &set(&[5]),
            &set(&[2, 3, 4]),
            &HashMap::new(),
            no_times(),
        );
        assert_eq!(cmp.categories[0].category, "Pwn");
        assert_eq!(cmp.categories[0].delta(), -800);
        assert_eq!(cmp.categories[0].theirs, 2);
        assert_eq!(cmp.categories.last().unwrap().category, "Crypto");
    }

    #[test]
    fn gap_closing_index() {
        let cmp = compare(
            &sample(),
            &set(&[]),
            &set(&[2, 3, 4]),
            &HashMap::new(),
            no_times(),
        );
        // Order: xss 200, rop 300, heap 500.
        assert_eq!(cmp.gap_closing_index(450), Some(1));
        assert_eq!(cmp.gap_closing_index(1001), None);
        assert_eq!(cmp.gap_closing_index(0), None);
    }

    /// What the tests feed `targets` beyond solve counts.
    #[derive(Default)]
    struct Extra {
        timing: HashMap<u64, Timing>,
        rival: HashSet<u64>,
        ahead: HashMap<u64, usize>,
        my_typical: Option<i64>,
        field_typical: Option<i64>,
    }

    const NOW: i64 = 100_000;

    impl Extra {
        fn signals(&self) -> Signals<'_> {
            Signals {
                timing: &self.timing,
                rival: &self.rival,
                ahead: &self.ahead,
                my_typical: self.my_typical,
                field_typical: self.field_typical,
                now: NOW,
            }
        }
    }

    /// Solves `ago` seconds before now, each having taken `span` seconds.
    fn solved(ago: &[i64], span: i64) -> Timing {
        let mut times: Vec<i64> = ago.iter().map(|a| NOW - a).collect();
        times.sort_unstable();
        Timing {
            solvers: (0..times.len()).map(|i| format!("team{i}")).collect(),
            spans: vec![span; times.len()],
            times,
        }
    }

    #[test]
    fn ranks_targets_by_likely_points() {
        let top: HashMap<u64, usize> = HashMap::new();
        let field = Field {
            ranked: 100,
            top_n: 10,
        };
        let extra = Extra::default();
        let signals = extra.signals();
        // Scores of the other accounts: 1000, 700, 400.
        let (list, focus) = targets(
            &sample(),
            &set(&[1]),
            &top,
            field,
            signals,
            500,
            &[1000, 700, 400],
        );

        let names: Vec<&str> = list.iter().map(|t| t.stat.name.as_str()).collect();
        // Shares: xss 0.40, rop 0.20, rsa 0.15, heap 0.03.
        assert_eq!(names, ["xss", "rop", "rsa", "heap"]);
        assert!((list[0].share - 0.4).abs() < 1e-9);
        assert!(list.iter().all(|t| t.odds > 0.0 && t.odds < 1.0));
        let running: Vec<i64> = list.iter().map(|t| t.running).collect();
        assert_eq!(running, [200, 500, 750, 1250]);
        // 700 ties #2 and 1000 ties #1, but they got there first.
        let reach: Vec<i64> = list.iter().map(|t| t.reach).collect();
        assert_eq!(reach, [3, 2, 1, 1]);

        assert_eq!(focus[0].category, "Web");
        assert_eq!(focus[1].category, "Pwn");
        assert_eq!((focus[1].open, focus[1].points), (2, 800));
        assert_eq!((focus[1].mine, focus[1].total), (0, 2));
    }

    #[test]
    fn falls_back_to_top_solves_without_counts() {
        let mut challenges = sample();
        for c in &mut challenges {
            c.solves = None;
        }
        let top: HashMap<u64, usize> = [(2, 5), (3, 1)].into();
        let field = Field {
            ranked: 100,
            top_n: 10,
        };
        let extra = Extra::default();
        let signals = extra.signals();
        let (list, _) = targets(&challenges, &set(&[]), &top, field, signals, 0, &[]);
        assert_eq!(list[0].stat.name, "heap");
        assert!((list[0].share - 0.5).abs() < 1e-9);
        assert_eq!(list[0].reach, 1);
    }

    #[test]
    fn odds_follow_your_record_in_the_category() {
        // Two categories with identical challenges; you have cleared most
        // of Web and none of Pwn.
        let challenges = vec![
            ch(1, "w1", "Web", 100, 50),
            ch(2, "w2", "Web", 100, 50),
            ch(3, "w3", "Web", 100, 50),
            ch(4, "w4", "Web", 300, 20),
            ch(5, "p1", "Pwn", 100, 50),
            ch(6, "p2", "Pwn", 100, 50),
            ch(7, "p3", "Pwn", 100, 50),
            ch(8, "p4", "Pwn", 300, 20),
        ];
        let field = Field {
            ranked: 100,
            top_n: 10,
        };
        let extra = Extra::default();
        let signals = extra.signals();
        let (list, _) = targets(
            &challenges,
            &set(&[1, 2, 3]),
            &HashMap::new(),
            field,
            signals,
            0,
            &[],
        );
        let of = |name: &str| list.iter().find(|t| t.stat.name == name).unwrap();
        assert_eq!(of("w4").share, of("p4").share);
        assert!(of("w4").odds > of("w4").share, "your category: likelier");
        assert!(of("p4").odds < of("p4").share, "not your category");
        assert_eq!(of("w4").reason, Some(Reason::Strength));
        assert_eq!(list[0].stat.name, "w4");
    }

    #[test]
    fn names_why_a_target_stands_out() {
        let challenges = vec![
            ch(1, "easy", "Misc", 50, 80),
            ch(2, "hot", "Web", 200, 10),
            ch(3, "theirs", "Pwn", 300, 5),
            ch(4, "nobody", "Rev", 500, 0),
            ch(5, "plain", "Crypto", 250, 12),
        ];
        let field = Field {
            ranked: 100,
            top_n: 10,
        };
        let extra = Extra {
            timing: [
                (2, solved(&[60, 600, 1200], 900)),
                (5, solved(&[60, 20_000], 900)),
            ]
            .into(),
            rival: set(&[3]),
            ..Default::default()
        };
        let signals = extra.signals();
        let (list, _) = targets(
            &challenges,
            &set(&[]),
            &HashMap::new(),
            field,
            signals,
            0,
            &[],
        );
        let reason = |name: &str| list.iter().find(|t| t.stat.name == name).unwrap().reason;
        assert_eq!(reason("easy"), Some(Reason::QuickWin));
        assert_eq!(reason("hot"), Some(Reason::Trending));
        assert_eq!(reason("theirs"), Some(Reason::Rival));
        assert_eq!(reason("nobody"), Some(Reason::Unsolved));
        assert_eq!(reason("plain"), None);
        let nobody = list.iter().find(|t| t.stat.name == "nobody").unwrap();
        assert!(nobody.odds > 0.0, "unsolved is unlikely, not impossible");
    }

    fn target(name: &str, value: i64, odds: f64) -> Target {
        Target {
            stat: ChallengeStat {
                id: 0,
                name: name.into(),
                category: String::new(),
                value,
                solves: None,
                top_solves: 0,
                at: None,
            },
            share: odds,
            difficulty: Difficulty {
                level: Level::Medium,
                tagged: false,
            },
            odds,
            odds_range: (odds, odds),
            expected: value as f64 * odds,
            recent: 0,
            timing: Timing::default(),
            time: None,
            time_guessed: false,
            per_hour: None,
            ahead: 0,
            category_share: 0.0,
            category_record: (0, 0),
            reason: None,
            running: 0,
            reach: 0,
            passes: None,
        }
    }

    fn names(list: &[Target], route: &Route) -> Vec<String> {
        route
            .picks
            .iter()
            .map(|&i| list[i].stat.name.clone())
            .collect()
    }

    #[test]
    fn difficulty_comes_from_tags_then_from_solves_and_points() {
        let tag = |t: &str| vec![t.to_string()];
        let d = difficulty(&tag("Easy"), Some(0.01), Some(1.0));
        assert_eq!((d.level, d.tagged), (Level::Easy, true), "the tag wins");
        let d = difficulty(&tag("difficulty: insane"), None, None);
        assert_eq!((d.level, d.tagged), (Level::Extreme, true));
        let d = difficulty(&tag("web"), Some(0.6), Some(0.0));
        assert_eq!((d.level, d.tagged), (Level::Easy, false), "not a level");

        let level = |share, points| difficulty(&[], share, points).level;
        assert_eq!(level(Some(0.6), Some(0.1)), Level::Easy);
        assert_eq!(level(Some(0.2), Some(0.5)), Level::Medium);
        assert_eq!(level(Some(0.05), Some(0.6)), Level::Hard);
        assert_eq!(level(Some(0.0), Some(1.0)), Level::Extreme);
        // Before anyone has solved anything, points are all there is.
        assert_eq!(level(None, Some(0.1)), Level::Easy);
        assert_eq!(level(None, Some(0.9)), Level::Extreme);
        assert_eq!(level(None, None), Level::Medium);
    }

    #[test]
    fn difficulty_orders_targets_nobody_has_solved() {
        // An unsolved board: same points, same (zero) solves, so only the
        // tags tell the targets apart.
        let mut challenges = vec![
            ch(1, "brutal", "Pwn", 300, 0),
            ch(2, "gentle", "Pwn", 300, 0),
            ch(3, "plain", "Pwn", 300, 0),
        ];
        challenges[0].tags = vec!["hard".into()];
        challenges[1].tags = vec!["easy".into()];
        let field = Field {
            ranked: 100,
            top_n: 10,
        };
        let extra = Extra::default();
        let signals = extra.signals();
        let (list, _) = targets(
            &challenges,
            &set(&[]),
            &HashMap::new(),
            field,
            signals,
            0,
            &[],
        );
        let names: Vec<&str> = list.iter().map(|t| t.stat.name.as_str()).collect();
        assert_eq!(names, ["gentle", "plain", "brutal"]);
        assert!(list[0].odds > list[1].odds && list[1].odds > list[2].odds);
        assert_eq!(list[1].difficulty.level, Level::Medium);
        assert!(!list[1].difficulty.tagged);
    }

    #[test]
    fn solve_times_give_typical_time_your_time_and_points_per_hour() {
        let challenges = vec![
            ch(1, "quick", "Web", 100, 40),
            ch(2, "slow", "Web", 400, 40),
            ch(3, "untouched", "Web", 400, 0),
        ];
        let field = Field {
            ranked: 100,
            top_n: 10,
        };
        let extra = Extra {
            // Three teams took 20, 30 and 60 minutes over "quick".
            timing: [
                (
                    1,
                    Timing {
                        times: vec![NOW - 9000, NOW - 5000, NOW - 600],
                        spans: vec![1200, 1800, 3600],
                        solvers: vec!["A".into(), "B".into(), "C".into()],
                    },
                ),
                (2, solved(&[7200], 4 * 3600)),
            ]
            .into(),
            ahead: [(1, 2)].into(),
            // You take twice as long per solve as the field.
            my_typical: Some(3600),
            field_typical: Some(1800),
            ..Default::default()
        };
        let (list, _) = targets(
            &challenges,
            &set(&[]),
            &HashMap::new(),
            field,
            extra.signals(),
            0,
            &[],
        );
        let of = |name: &str| list.iter().find(|t| t.stat.name == name).unwrap();

        let quick = of("quick");
        assert_eq!(quick.timing.typical(), Some(1800));
        assert_eq!(quick.timing.spread(), Some((1500, 2700)));
        assert_eq!((quick.time, quick.time_guessed), (Some(3600), false));
        assert_eq!(quick.recent, 1);
        assert_eq!(quick.ahead, 2);
        assert!((quick.per_hour.unwrap() - quick.expected).abs() < 1e-9);
        assert!(quick.odds_range.0 < quick.odds && quick.odds < quick.odds_range.1);

        // Nobody tracked solved it: time comes from its difficulty.
        let untouched = of("untouched");
        assert!(untouched.time_guessed);
        assert!(untouched.time.unwrap() > quick.time.unwrap());

        // Same odds, four times the points, but eight times the hours.
        let slow = of("slow");
        assert!(slow.expected > quick.expected);
        assert!(slow.per_hour.unwrap() < quick.per_hour.unwrap());
        assert_eq!(Sort::Likely.compare(slow, quick), std::cmp::Ordering::Less);
        assert_eq!(Sort::PerHour.compare(quick, slow), std::cmp::Ordering::Less);
        assert_eq!(Sort::Time.compare(quick, slow), std::cmp::Ordering::Less);

        // The details say how far into the CTF each team solved it.
        let facts = quick.facts(field, NOW, NOW - 10_000, 3);
        let fact = |label: &str| {
            facts
                .iter()
                .find(|f| f.label == label)
                .map(|f| f.value.as_str())
        };
        assert_eq!(fact("First solve"), Some("16m in, by A"));
        assert_eq!(
            fact("Solved at"),
            Some("A 16m in, B 1h 23m in, C 2h 36m in")
        );
        assert!(facts.last().unwrap().wide);
        assert_eq!(fact("Latest solve"), Some("10m ago"));
        assert_eq!(fact("Typical time"), Some("30m (25m to 45m, 3 solves)"));
        assert_eq!(fact("Your time"), Some("about 1h 00m, at your pace"));
        assert_eq!(fact("Teams ahead of you"), Some("2 of 3 have it"));
        assert_eq!(fact("Last hour, last 3 h"), Some("1 and 3 solves"));
    }

    #[test]
    fn quantiles_ranges_and_spans() {
        assert_eq!(quantile(&[], 0.5), None);
        assert_eq!(quantile(&[10], 0.5), Some(10));
        assert_eq!(quantile(&[10, 20], 0.5), Some(15));
        assert_eq!(quantile(&[30, 10, 20], 0.5), Some(20));
        let (lo, hi) = share_range(20, 100);
        assert!(lo > 0.14 && lo < 0.2 && hi > 0.2 && hi < 0.27, "{lo} {hi}");
        let (lo, hi) = share_range(0, 10);
        assert!(lo < 1e-9 && hi > 0.05);
        assert_eq!(span_text(30), "under 1m");
        assert_eq!(span_text(45 * 60), "45m");
        assert_eq!(span_text(3900), "1h 05m");
        assert_eq!(span_text(2 * 86_400 + 3 * 3600), "2d 3h");
    }

    #[test]
    fn route_takes_the_likeliest_solves_that_clear_the_gap() {
        let list = vec![
            target("big", 500, 0.10),
            target("mid", 300, 0.40),
            target("small", 200, 0.60),
            target("tiny", 50, 0.90),
        ];
        // 400 behind: mid + small (24%) beats big alone (10%), and tiny
        // is not needed.
        let r = route(&list, 400).unwrap();
        assert_eq!(names(&list, &r), ["small", "mid"]);
        assert_eq!(r.points, 500);
        assert!((r.odds - 0.24).abs() < 1e-9);

        // A tie on points is not a pass: 500 behind needs more than 500.
        let r = route(&list, 500).unwrap();
        assert_eq!(names(&list, &r), ["tiny", "small", "mid"]);
        assert_eq!(r.points, 550);

        // Level on points: one solve settles it.
        let r = route(&list, 0).unwrap();
        assert_eq!(names(&list, &r), ["tiny"]);

        assert_eq!(route(&list, 1050), None, "everything open is not enough");
        assert_eq!(route(&list, -1), None, "already ahead");
    }

    #[test]
    fn route_prefers_one_solve_over_two_equally_likely() {
        let list = vec![
            target("a", 100, 0.5),
            target("b", 100, 0.5),
            target("c", 200, 0.25),
        ];
        let r = route(&list, 150).unwrap();
        assert_eq!(names(&list, &r), ["c"]);
    }

    #[test]
    fn route_clears_large_gaps() {
        let list: Vec<Target> = (0..40)
            .map(|i| target(&format!("c{i}"), 9_000 + i * 37, 0.3))
            .collect();
        let behind = 123_456;
        let r = route(&list, behind).unwrap();
        assert!(r.points > behind);
        assert_eq!(r.picks.len(), 13, "no more solves than the gap needs");
    }
}
