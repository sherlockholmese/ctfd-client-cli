//! Head-to-head analytics: where a rival out-scores you, and which of
//! their solves you could realistically catch up on.

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
}
