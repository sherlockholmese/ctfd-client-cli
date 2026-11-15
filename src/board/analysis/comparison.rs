//! Compare your solves with a rival, grouped by challenge and category.

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
