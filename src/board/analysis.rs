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

/// An unsolved challenge, ranked for the climb to the top.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub stat: ChallengeStat,
    /// Share of accounts that solved it (0..=1): a stand-in for difficulty.
    pub share: f64,
    /// Points weighted by `share`: what an attempt is likely worth.
    pub expected: f64,
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
}

/// What the board knows about solve rates.
#[derive(Debug, Clone, Copy)]
pub struct Field {
    /// Accounts on the scoreboard (for overall solve counts).
    pub ranked: usize,
    /// Accounts in `top_counts`.
    pub top_n: usize,
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
    my_score: i64,
    others: &[i64],
) -> (Vec<Target>, Vec<Focus>) {
    let share = |c: &Challenge, top: usize| match c.solves {
        Some(n) if field.ranked > 0 => (n as f64 / field.ranked as f64).min(1.0),
        _ if field.top_n > 0 => top as f64 / field.top_n as f64,
        _ => 0.0,
    };
    let mut list: Vec<Target> = challenges
        .iter()
        .filter(|c| !mine.contains(&c.id))
        .map(|c| {
            let top_solves = top_counts.get(&c.id).copied().unwrap_or(0);
            let share = share(c, top_solves);
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
                expected: c.value as f64 * share,
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
        let f = focus
            .entry(t.stat.category.as_str())
            .or_insert_with(|| Focus {
                category: t.stat.category.clone(),
                open: 0,
                points: 0,
                expected: 0.0,
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

/// How many of `targets` it takes to gain more than `behind` points, and
/// the points that brings.
pub fn solves_to_pass(targets: &[Target], behind: i64) -> Option<(usize, i64)> {
    if behind < 0 {
        return None;
    }
    targets
        .iter()
        .position(|t| t.running > behind)
        .map(|i| (i + 1, targets[i].running))
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

    #[test]
    fn ranks_targets_by_likely_points() {
        let top: HashMap<u64, usize> = HashMap::new();
        let field = Field {
            ranked: 100,
            top_n: 10,
        };
        // Scores of the other accounts: 1000, 700, 400.
        let (list, focus) = targets(&sample(), &set(&[1]), &top, field, 500, &[1000, 700, 400]);

        let names: Vec<&str> = list.iter().map(|t| t.stat.name.as_str()).collect();
        // xss 200*0.4=80, rop 300*0.2=60, rsa 250*0.15=37.5, heap 500*0.03=15
        assert_eq!(names, ["xss", "rop", "rsa", "heap"]);
        let running: Vec<i64> = list.iter().map(|t| t.running).collect();
        assert_eq!(running, [200, 500, 750, 1250]);
        // 700 ties #2 and 1000 ties #1, but they got there first.
        let reach: Vec<i64> = list.iter().map(|t| t.reach).collect();
        assert_eq!(reach, [3, 2, 1, 1]);

        assert_eq!(focus[0].category, "Web");
        assert_eq!(focus[1].category, "Pwn");
        assert_eq!((focus[1].open, focus[1].points), (2, 800));

        assert_eq!(solves_to_pass(&list, 200), Some((2, 500)));
        assert_eq!(solves_to_pass(&list, 499), Some((2, 500)));
        assert_eq!(solves_to_pass(&list, 500), Some((3, 750)));
        assert_eq!(solves_to_pass(&list, 5000), None);
        assert_eq!(solves_to_pass(&list, -1), None);
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
        let (list, _) = targets(&challenges, &set(&[]), &top, field, 0, &[]);
        assert_eq!(list[0].stat.name, "heap");
        assert!((list[0].share - 0.5).abs() < 1e-9);
        assert_eq!(list[0].reach, 1);
    }
}
