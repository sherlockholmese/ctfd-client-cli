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

/// Compare `mine` with `theirs` over the visible `challenges`.
///
/// `top_counts` maps challenge ID to how many top accounts solved it; it
/// drives the "field" list and breaks ties when sorting.
pub fn compare(
    challenges: &[Challenge],
    mine: &HashSet<u64>,
    theirs: &HashSet<u64>,
    top_counts: &HashMap<u64, usize>,
) -> Comparison {
    let stat = |c: &Challenge| ChallengeStat {
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
            (false, true) => catch_up.push(stat(c)),
            (true, false) => edge.push(stat(c)),
            _ => {}
        }
        if !i_solved && !they_solved {
            field.push(stat(c));
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
    /// Your estimated chance of solving it (0..=1): `share`, shifted by
    /// how you do in its category compared with your average.
    pub odds: f64,
    /// Points weighted by `odds`: what an attempt is likely worth.
    pub expected: f64,
    /// Solves in the last hour by accounts whose solves are known.
    pub recent: usize,
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
    /// Challenge ID to solves in the last hour by known accounts.
    pub recent: &'a HashMap<u64, usize>,
    /// Challenges the compared rival has solved.
    pub rival: &'a HashSet<u64>,
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
    let mut list: Vec<Target> = challenges
        .iter()
        .filter(|c| !mine.contains(&c.id))
        .map(|c| {
            let top_solves = top_counts.get(&c.id).copied().unwrap_or(0);
            let known = solved_by(c, top_solves);
            let share = known.map_or(0.0, |(n, of)| n as f64 / of as f64);
            let (cat_mine, cat_total) = record[c.category.as_str()];
            let fit = strength(cat_mine, cat_total, all_mine, challenges.len());
            // Smoothed so an unsolved challenge is unlikely, not impossible.
            let base = known.map_or(0.0, |(n, of)| (n as f64 + 0.5) / (of as f64 + 1.0));
            let scaled = base / (1.0 - base) * fit;
            let odds = scaled / (1.0 + scaled);
            let recent = signals.recent.get(&c.id).copied().unwrap_or(0);
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
                },
                share,
                odds,
                expected: c.value as f64 * odds,
                recent,
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
        let cmp = compare(&sample(), &set(&[1, 5]), &set(&[1, 2, 3, 4]), &top);

        let names: Vec<&str> = cmp.catch_up.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["xss", "rop", "heap"], "most-solved first");
        assert_eq!(cmp.catch_up_points(), 1000);
        assert_eq!(cmp.edge.len(), 1);
        assert_eq!(cmp.edge[0].name, "rsa");

        assert!(
            cmp.field.is_empty(),
            "everything open for you is in catch-up"
        );

        let cmp = compare(&sample(), &set(&[1]), &set(&[2]), &top);
        let field: Vec<&str> = cmp.field.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            field,
            ["xss", "rop", "rsa"],
            "solved by neither, popular first"
        );
    }

    #[test]
    fn categories_sorted_by_deficit() {
        let cmp = compare(&sample(), &set(&[5]), &set(&[2, 3, 4]), &HashMap::new());
        assert_eq!(cmp.categories[0].category, "Pwn");
        assert_eq!(cmp.categories[0].delta(), -800);
        assert_eq!(cmp.categories[0].theirs, 2);
        assert_eq!(cmp.categories.last().unwrap().category, "Crypto");
    }

    #[test]
    fn gap_closing_index() {
        let cmp = compare(&sample(), &set(&[]), &set(&[2, 3, 4]), &HashMap::new());
        // Order: xss 200, rop 300, heap 500.
        assert_eq!(cmp.gap_closing_index(450), Some(1));
        assert_eq!(cmp.gap_closing_index(1001), None);
        assert_eq!(cmp.gap_closing_index(0), None);
    }

    fn none() -> (HashMap<u64, usize>, HashSet<u64>) {
        (HashMap::new(), HashSet::new())
    }

    #[test]
    fn ranks_targets_by_likely_points() {
        let top: HashMap<u64, usize> = HashMap::new();
        let field = Field {
            ranked: 100,
            top_n: 10,
        };
        let (recent, rival) = none();
        let signals = Signals {
            recent: &recent,
            rival: &rival,
        };
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
        let (recent, rival) = none();
        let signals = Signals {
            recent: &recent,
            rival: &rival,
        };
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
        let (recent, rival) = none();
        let signals = Signals {
            recent: &recent,
            rival: &rival,
        };
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
        let recent: HashMap<u64, usize> = [(2, 3), (5, 1)].into();
        let rival = set(&[3]);
        let signals = Signals {
            recent: &recent,
            rival: &rival,
        };
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
            },
            share: odds,
            odds,
            expected: value as f64 * odds,
            recent: 0,
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
