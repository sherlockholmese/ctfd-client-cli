//! A deterministic, fictional CTF for `ctfdc lb --demo`.
//!
//! Lets the scoreboard UI be checked before a CTF starts (or offline):
//! 36 fictional teams play 30 fictional challenges with CTFd-style
//! dynamic scoring, and new solves keep arriving on every update so the
//! live behaviour can be seen.  Nothing here touches the network.

use super::model::Data;
use crate::models::{
    AccountSolve, Challenge, ScoreEvent, ScoreboardEntry, SolvedChallenge, TopEntry,
};
use chrono::{DateTime, Utc};

/// The demo team that plays "you".
pub const ME_ID: u64 = 107;
pub const ME_NAME: &str = "Null Pointers";

const TEAMS: [&str; 36] = [
    "Segfault Syndicate",
    "Heap Hoppers",
    "Kernel Panic Club",
    "Bit Flippers",
    "0xDEADC0DE",
    "Gadget Chain Gang",
    ME_NAME,
    "東京ヒープ研究会",
    "🦀 Rustaceans",
    "The Extremely Long Team Name That Keeps Going And Going Forever",
    "Race Conditioners",
    "Format String Theory",
    "Off By One",
    "Stack Canaries",
    "Return Oriented",
    "Side Channel Surfers",
    "Packet Sniffers United",
    "x",
    "Use After Free",
    "Byte Bandits",
    "Shellcode Society",
    "Null Byte Ninjas",
    "Padding Oracles",
    "Integer Overflowers",
    "Cache Missers",
    "Dangling Pointers",
    "The Fuzzers",
    "Glitch Mob",
    "Ret2Libc",
    "Hash Collision Course",
    "Blind SQL",
    "Taint Trackers",
    "Undefined Behaviour",
    "Little Endians",
    "Big Endians",
    "Zero Day Dreamers",
];

/// (name, category, initial value, difficulty 0..1, dynamic scoring)
const CHALLENGES: [(&str, &str, i64, f64, bool); 30] = [
    ("Sanity Check", "Misc", 50, 0.02, false),
    ("Discord Flag", "Misc", 50, 0.05, false),
    ("Baby Login", "Web", 500, 0.18, true),
    ("Cookie Jar", "Web", 500, 0.30, true),
    ("Template Trouble", "Web", 500, 0.52, true),
    ("Prototype Poison", "Web", 500, 0.70, true),
    ("Cache Me Outside", "Web", 500, 0.86, true),
    ("ret2win", "Pwn", 500, 0.25, true),
    ("Format Fiesta", "Pwn", 500, 0.45, true),
    ("Heap Haven", "Pwn", 500, 0.68, true),
    ("Kernel Rope", "Pwn", 500, 0.88, true),
    ("Tiny VM", "Pwn", 500, 1.00, true),
    ("Caesar Salad", "Crypto", 500, 0.12, true),
    ("RSA Warmup", "Crypto", 500, 0.32, true),
    ("Padding Party", "Crypto", 500, 0.55, true),
    ("Lattice Lane", "Crypto", 500, 0.80, true),
    ("Curve Ball", "Crypto", 500, 0.93, true),
    ("Strings Attached", "Rev", 500, 0.15, true),
    ("Crackme 2026", "Rev", 500, 0.38, true),
    ("Obfuscated Orchard", "Rev", 500, 0.62, true),
    ("Wasm Maze", "Rev", 500, 0.78, true),
    ("Hypervisor Hunt", "Rev", 500, 1.05, true),
    ("Lost in Packets", "Forensics", 500, 0.22, true),
    ("Memory Lane", "Forensics", 500, 0.48, true),
    ("Disk Detective", "Forensics", 500, 0.66, true),
    ("Stego Sunrise", "Forensics", 500, 0.84, true),
    ("Jail Break", "Misc", 500, 0.42, true),
    ("Blockchain Bingo", "Misc", 500, 0.60, true),
    ("Quantum Quiz", "Misc", 500, 1.30, true),
    ("Satellite Signals", "Misc", 500, 1.45, true),
];

const MIN_VALUE: i64 = 100;
const DECAY: f64 = 18.0;
/// How long the fictional CTF has been running.
const ELAPSED_SECS: i64 = 16 * 3600;

pub struct DemoWorld {
    rng: u64,
    start: i64,
    skills: Vec<f64>,
    /// (team index, challenge index, unix time)
    solves: Vec<(usize, usize, i64)>,
}

impl DemoWorld {
    pub fn new(now: i64) -> Self {
        let mut world = Self {
            rng: 0x5eed_c7fd,
            start: now - ELAPSED_SECS,
            skills: Vec::new(),
            solves: Vec::new(),
        };
        world.skills = (0..TEAMS.len())
            .map(|i| {
                let base = 0.92 - i as f64 * 0.024;
                let noise = (world.next_f64() - 0.5) * 0.08;
                if TEAMS[i] == ME_NAME {
                    0.70
                } else {
                    base + noise
                }
            })
            .collect();

        for team in 0..TEAMS.len() {
            for (c, &(_, _, _, difficulty, _)) in CHALLENGES.iter().enumerate() {
                let p = solve_probability(world.skills[team], difficulty);
                if world.next_f64() < p {
                    // Harder challenges tend to fall later.
                    let spread = (world.next_f64() * 0.7 + difficulty * 0.3).min(0.98);
                    let t = world.start + (spread * ELAPSED_SECS as f64) as i64;
                    world.solves.push((team, c, t));
                }
            }
        }
        world
    }

    fn next_f64(&mut self) -> f64 {
        // xorshift64*: deterministic, good enough for fiction.
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let v = self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d);
        (v >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Simulate the time since the last update: a few new solves.
    pub fn tick(&mut self, now: i64) {
        let attempts = 1 + (self.next_f64() * 3.0) as usize;
        for _ in 0..attempts {
            let team = (self.next_f64() * self.next_f64() * TEAMS.len() as f64) as usize;
            let c = (self.next_f64() * CHALLENGES.len() as f64) as usize;
            let solved = self.solves.iter().any(|&(t, ch, _)| t == team && ch == c);
            let p = solve_probability(self.skills[team], CHALLENGES[c].3) * 0.8;
            if !solved && self.next_f64() < p {
                self.solves.push((team, c, now));
            }
        }
    }

    fn solve_count(&self, c: usize) -> u64 {
        self.solves.iter().filter(|&&(_, ch, _)| ch == c).count() as u64
    }

    fn value(&self, c: usize) -> i64 {
        let (_, _, initial, _, dynamic) = CHALLENGES[c];
        if !dynamic {
            return initial;
        }
        // CTFd's dynamic formula (solver count excludes the first blood).
        let solves = self.solve_count(c).saturating_sub(1) as f64;
        let v = ((MIN_VALUE - initial) as f64 / (DECAY * DECAY)) * solves * solves + initial as f64;
        (v.ceil() as i64).max(MIN_VALUE)
    }

    fn team_score(&self, team: usize) -> i64 {
        self.solves
            .iter()
            .filter(|&&(t, _, _)| t == team)
            .map(|&(_, c, _)| self.value(c))
            .sum()
    }

    fn account_id(team: usize) -> u64 {
        101 + team as u64
    }

    fn iso(t: i64) -> String {
        DateTime::<Utc>::from_timestamp(t, 0)
            .unwrap_or_default()
            .to_rfc3339()
    }

    /// A CTFd-shaped snapshot.  `accounts` lists teams whose solves were
    /// requested individually (outside the top list).
    pub fn snapshot(&self, top_n: u32, accounts: &[u64]) -> Data {
        let mut ranked: Vec<(usize, i64, i64)> = (0..TEAMS.len())
            .map(|team| {
                let last = self
                    .solves
                    .iter()
                    .filter(|&&(t, _, _)| t == team)
                    .map(|&(_, _, at)| at)
                    .max()
                    .unwrap_or(i64::MAX);
                (team, self.team_score(team), last)
            })
            .filter(|&(_, score, _)| score > 0)
            .collect();
        // Higher score first; ties go to whoever got there first.
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));

        let standings: Vec<ScoreboardEntry> = ranked
            .iter()
            .enumerate()
            .map(|(i, &(team, score, _))| ScoreboardEntry {
                pos: i as i64 + 1,
                name: TEAMS[team].to_string(),
                score,
                account_id: Some(Self::account_id(team)),
                account_type: Some("team".to_string()),
            })
            .collect();

        let events = |team: usize| -> Vec<(usize, i64)> {
            self.solves
                .iter()
                .filter(|&&(t, _, _)| t == team)
                .map(|&(_, c, at)| (c, at))
                .collect()
        };

        let top = ranked
            .iter()
            .take(top_n as usize)
            .map(|&(team, _, _)| TopEntry {
                id: Some(Self::account_id(team)),
                name: TEAMS[team].to_string(),
                solves: events(team)
                    .into_iter()
                    .map(|(c, at)| ScoreEvent {
                        challenge_id: Some(c as u64 + 1),
                        value: self.value(c),
                        date: Self::iso(at),
                    })
                    .collect(),
            })
            .collect();

        let me = TEAMS.iter().position(|n| *n == ME_NAME).unwrap_or(0);
        let challenges = CHALLENGES
            .iter()
            .enumerate()
            .map(|(c, &(name, category, _, _, _))| Challenge {
                id: c as u64 + 1,
                name: name.to_string(),
                category: category.to_string(),
                value: self.value(c),
                solved_by_me: self.solves.iter().any(|&(t, ch, _)| t == me && ch == c),
                solves: Some(self.solve_count(c)),
            })
            .collect();

        let accounts = accounts
            .iter()
            .filter_map(|&id| {
                let team = id.checked_sub(101)? as usize;
                (team < TEAMS.len()).then(|| (id, self.account_solves(team)))
            })
            .collect();

        Data {
            standings,
            top,
            challenges,
            accounts,
            names: Default::default(),
        }
    }

    fn account_solves(&self, team: usize) -> Vec<AccountSolve> {
        self.solves
            .iter()
            .filter(|&&(t, _, _)| t == team)
            .map(|&(_, c, at)| AccountSolve {
                challenge_id: Some(c as u64 + 1),
                date: Self::iso(at),
                challenge: Some(SolvedChallenge {
                    id: Some(c as u64 + 1),
                    value: self.value(c),
                    name: None,
                    category: None,
                }),
            })
            .collect()
    }

    /// Solves of one team by account ID.
    pub fn account(&self, id: u64) -> Vec<AccountSolve> {
        match id.checked_sub(101) {
            Some(team) if (team as usize) < TEAMS.len() => self.account_solves(team as usize),
            _ => Vec::new(),
        }
    }
}

fn solve_probability(skill: f64, difficulty: f64) -> f64 {
    1.0 / (1.0 + (-(skill - difficulty) * 9.0).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_plausible() {
        let a = DemoWorld::new(1_800_000_000).snapshot(10, &[]);
        let b = DemoWorld::new(1_800_000_000).snapshot(10, &[]);
        assert_eq!(a.standings.len(), b.standings.len());
        assert_eq!(a.standings[0].score, b.standings[0].score);
        assert!(a.standings.len() > 20);
        assert_eq!(a.top.len(), 10);
        assert!(a.standings.windows(2).all(|w| w[0].score >= w[1].score));
        let me = a.standings.iter().position(|e| e.name == ME_NAME).unwrap();
        assert!(
            (3..15).contains(&me),
            "you should be mid-table, got #{}",
            me + 1
        );
        assert!(
            a.challenges.iter().any(|c| c.solves == Some(0)),
            "some unsolved"
        );
    }

    #[test]
    fn ticks_add_solves() {
        let mut w = DemoWorld::new(1_800_000_000);
        let before = w.solves.len();
        for i in 0..50 {
            w.tick(1_800_000_000 + i);
        }
        assert!(w.solves.len() > before);
    }
}
