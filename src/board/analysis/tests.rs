use super::difficulty::difficulty;
use super::route::Route;
use super::targets::Reason;
use super::*;
use crate::models::Challenge;
use std::collections::{HashMap, HashSet};

fn ch(id: u64, name: &str, category: &str, value: i64, solves: u64) -> Challenge {
    serde_json::from_value(serde_json::json!({
        "id": id, "name": name, "category": category, "value": value,
        "solved_by_me": false, "solves": solves
    }))
    .unwrap()
}

fn no_times() -> SolvedAt<'static> {
    static EMPTY: std::sync::LazyLock<HashMap<u64, i64>> = std::sync::LazyLock::new(HashMap::new);
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
