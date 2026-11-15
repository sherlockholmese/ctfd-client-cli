//! Find the most likely set of solves that clears a score gap.

use super::Target;

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
