//! Derive rival comparisons, solve timing, and the route toward a goal team.

use super::{BoardModel, ScoreChange, account_key};
use crate::board::analysis::{self, Comparison};
use crate::models::Challenge;
use std::collections::{HashMap, HashSet};

/// A ranked account in the head-to-head view.
#[derive(Debug, Clone, PartialEq)]
pub struct Party {
    pub pos: i64,
    pub account: Option<u64>,
    pub name: String,
    pub score: i64,
    pub solves: Option<usize>,
}

/// Recent scoring pace.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Momentum {
    pub last_hour: i64,
    pub last_3h: i64,
    pub last_solve: Option<i64>,
}

/// Where you stand relative to a neighbour or a cut-off.
#[derive(Debug, Clone, PartialEq)]
pub struct Margin {
    pub label: String,
    /// Positive: you are ahead by this much; negative: behind.
    pub points: i64,
}

/// Head-to-head analytics between you and the rival.
#[derive(Debug, Clone, Default)]
pub struct HeadToHead {
    pub me: Option<Party>,
    pub rival: Option<Party>,
    /// Rival solves are still being fetched.
    pub rival_loading: bool,
    pub comparison: Comparison,
    pub me_momentum: Option<Momentum>,
    pub rival_momentum: Option<Momentum>,
    pub cutoff: Option<Margin>,
}

/// A team on your way to the top (or just behind you).
#[derive(Debug, Clone, PartialEq)]
pub struct Rung {
    pub pos: i64,
    pub account: Option<u64>,
    pub name: String,
    pub score: i64,
    /// Points they are ahead of you (negative: behind you).
    pub ahead: i64,
    /// Recent pace, when their solves are known.
    pub momentum: Option<Momentum>,
    /// Solves on the likeliest route past them, and the points that brings.
    pub to_pass: Option<(usize, i64)>,
    /// That route: challenge IDs, likeliest solve first.
    pub route: Vec<u64>,
    /// Chance of landing the whole route.
    pub route_odds: Option<f64>,
    /// Your time for the whole route, in seconds, where every step has one.
    pub route_time: Option<i64>,
    /// Points per hour over the last three hours.
    pub pace: Option<f64>,
    /// Seconds until you draw level at both current paces; `None` when
    /// you are not closing in (or their solves are unknown).
    pub catch_in: Option<i64>,
}

/// Solve times over the accounts whose solves are known.
struct SolveTimes {
    timing: HashMap<u64, analysis::Timing>,
    /// Seconds between your solves, typically; `None` until you have a few.
    my_typical: Option<i64>,
    /// The same over everyone else tracked.
    field_typical: Option<i64>,
}

/// Where to spend time to climb: ranked against the whole board.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    /// Leaders, the teams just above you, and the two just behind.
    pub ladder: Vec<Rung>,
    pub targets: Vec<analysis::Target>,
    pub focus: Vec<analysis::Focus>,
    /// Rank of the team the route aims to pass: the one picked with
    /// [`BoardModel::set_goal`] while it is still ahead of you, else the
    /// last qualifying place when you are outside it, else the team above
    /// you.
    pub goal: Option<i64>,
    /// How the targets off the route are ordered.
    pub sort: analysis::Sort,
    /// Your points per hour over the last three hours.
    pub my_pace: Option<f64>,
    /// When scoring began (Unix time).
    pub start: i64,
    /// Accounts above you whose solves are known.
    pub ahead_tracked: usize,
    pub field: Option<analysis::Field>,
}

impl Plan {
    /// What the details of one target draw from, beyond the target itself.
    pub fn context(&self) -> Option<(analysis::Field, i64, usize)> {
        Some((self.field?, self.start, self.ahead_tracked))
    }

    /// The details of one target, as labelled lines.
    pub fn facts(&self, target: &analysis::Target, now: i64) -> Vec<analysis::Fact> {
        match self.field {
            Some(field) => target.facts(field, now, self.start, self.ahead_tracked),
            None => Vec::new(),
        }
    }

    /// The rung the route aims to pass.
    pub fn goal_rung(&self) -> Option<&Rung> {
        let pos = self.goal?;
        self.ladder.iter().find(|r| r.pos == pos)
    }

    /// Targets on the route to the goal, in the order to try them.
    pub fn route(&self) -> Vec<&analysis::Target> {
        self.goal_rung()
            .map(|r| {
                r.route
                    .iter()
                    .filter_map(|id| self.targets.iter().find(|t| t.stat.id == *id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Targets off the route, in the picked order.
    pub fn off_route(&self) -> Vec<&analysis::Target> {
        let on: &[u64] = self.goal_rung().map(|r| r.route.as_slice()).unwrap_or(&[]);
        let mut rest: Vec<&analysis::Target> = self
            .targets
            .iter()
            .filter(|t| !on.contains(&t.stat.id))
            .collect();
        rest.sort_by(|a, b| self.sort.compare(a, b));
        rest
    }

    /// The route, then the rest: the order the views list targets in.
    pub fn listed(&self) -> Vec<&analysis::Target> {
        let mut all = self.route();
        all.extend(self.off_route());
        all
    }
}

/// Teams always on the ladder, from the top.
const LADDER_TOP: i64 = 3;
/// Teams just above you on the ladder.
const LADDER_ABOVE: usize = 4;
/// Teams just behind you on the ladder.
const LADDER_BELOW: usize = 2;

impl BoardModel {
    fn momentum(&self, account: Option<u64>) -> Option<Momentum> {
        let history = self.changes_for(account)?;
        let sum_since = |secs: i64| {
            history
                .iter()
                .filter(|c| c.t >= self.now - secs)
                .map(|c| c.value)
                .sum()
        };
        Some(Momentum {
            last_hour: sum_since(3600),
            last_3h: sum_since(3 * 3600),
            last_solve: history.iter().map(|c| c.t).max(),
        })
    }

    /// The challenge list, plus challenges it leaves out that are known
    /// by name from solves, valued at the points they were solved for.
    fn known_challenges(&self) -> Vec<Challenge> {
        let mut challenges = self.data.challenges.clone();
        if self.data.names.is_empty() {
            return challenges;
        }
        let mut values: HashMap<u64, i64> = HashMap::new();
        let top = self.data.top.iter().flat_map(|t| &t.solves);
        for (id, value) in top.filter_map(|s| Some((s.challenge_id?, s.value))) {
            values.entry(id).or_insert(value);
        }
        for s in self.data.accounts.values().flatten() {
            if let Some(c) = &s.challenge
                && let Some(id) = c.id.or(s.challenge_id)
            {
                values.entry(id).or_insert(c.value);
            }
        }
        let mut extra: Vec<Challenge> = self
            .data
            .names
            .iter()
            .map(|(&id, (name, category))| Challenge {
                id,
                name: name.clone(),
                category: category.clone(),
                value: values.get(&id).copied().unwrap_or(0),
                solved_by_me: false,
                solves: None,
                tags: Vec::new(),
            })
            .collect();
        extra.sort_by_key(|c| c.id);
        challenges.extend(extra);
        challenges
    }

    /// When scoring began: the configured start, else the earliest solve
    /// known (close to the start once a few teams have scored).
    fn start_time(&self) -> i64 {
        self.since.unwrap_or_else(|| {
            self.tracked_ids()
                .into_iter()
                .filter_map(|id| self.changes_for(Some(id)))
                .flatten()
                .map(|c| c.t)
                .min()
                .unwrap_or(self.now)
        })
    }

    /// Accounts whose solves are known: the graphed top, pins, you and
    /// the rival.
    fn tracked_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self.data.top.iter().filter_map(|t| t.id).collect();
        ids.extend(self.data.accounts.keys().copied());
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Solve times over every tracked account: per challenge who solved
    /// it when and how long after their previous solve, plus how long a
    /// solve typically takes you and the field.
    fn solve_times(&self, start: i64) -> SolveTimes {
        let mut by_challenge: HashMap<u64, Vec<(i64, i64, String)>> = HashMap::new();
        let mut all_spans = Vec::new();
        let mut my_spans = Vec::new();
        for id in self.tracked_ids() {
            let mut history = self.changes_for(Some(id)).unwrap_or_default();
            history.sort_by_key(|c| c.t);
            let name = self.name_of(id);
            let mut previous = start;
            let mut seen = HashSet::new();
            for c in history {
                let span = (c.t - previous).max(0);
                previous = previous.max(c.t);
                let Some(challenge) = c.challenge_id else {
                    continue;
                };
                if !seen.insert(challenge) {
                    continue;
                }
                by_challenge
                    .entry(challenge)
                    .or_default()
                    .push((c.t, span, name.clone()));
                if Some(id) == self.me_account {
                    my_spans.push(span);
                } else {
                    all_spans.push(span);
                }
            }
        }
        let timing = by_challenge
            .into_iter()
            .map(|(challenge, mut solves)| {
                solves.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.cmp(&b.2)));
                let timing = analysis::Timing {
                    times: solves.iter().map(|s| s.0).collect(),
                    spans: solves.iter().map(|s| s.1).collect(),
                    solvers: solves.into_iter().map(|s| s.2).collect(),
                };
                (challenge, timing)
            })
            .collect();
        SolveTimes {
            timing,
            // A pace needs a few solves behind it.
            my_typical: (my_spans.len() >= 3)
                .then(|| analysis::quantile(&my_spans, 0.5))
                .flatten(),
            field_typical: analysis::quantile(&all_spans, 0.5),
        }
    }

    fn rebuild_plan(
        &mut self,
        challenges: &[Challenge],
        mine: &HashSet<u64>,
        theirs: &HashSet<u64>,
        top_counts: &HashMap<u64, usize>,
    ) {
        let standings = &self.data.standings;
        let Some(me) = self.me_index() else {
            self.plan = Plan::default();
            return;
        };
        let my_score = standings[me].score;
        let others: Vec<i64> = standings
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != me)
            .map(|(_, e)| e.score)
            .collect();
        let field = analysis::Field {
            ranked: standings.len(),
            top_n: self.data.top.len().min(self.top_n as usize),
        };
        let start = self.start_time();
        let times = self.solve_times(start);
        // How many tracked accounts above you have each challenge.
        let tracked = self.tracked_ids();
        let above: Vec<u64> = standings[..me]
            .iter()
            .filter_map(|e| e.account_id)
            .filter(|id| tracked.contains(id))
            .collect();
        let mut ahead: HashMap<u64, usize> = HashMap::new();
        for id in &above {
            let solved: HashSet<u64> = self
                .changes_for(Some(*id))
                .iter()
                .flatten()
                .filter_map(|c| c.challenge_id)
                .collect();
            for challenge in solved {
                *ahead.entry(challenge).or_default() += 1;
            }
        }
        let signals = analysis::Signals {
            timing: &times.timing,
            rival: theirs,
            ahead: &ahead,
            my_typical: times.my_typical,
            field_typical: times.field_typical,
            now: self.now,
        };
        let (mut targets, focus) = analysis::targets(
            challenges, mine, top_counts, field, signals, my_score, &others,
        );
        // Name who each improving solve overtakes: with the others in rank
        // order, reaching rank r puts you just above the r-th of them.
        let others_ranked: Vec<&crate::models::ScoreboardEntry> = standings
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != me)
            .map(|(_, e)| e)
            .collect();
        let mut best = standings[me].pos;
        for t in &mut targets {
            if t.reach < best {
                best = t.reach;
                t.passes = others_ranked
                    .get((t.reach - 1) as usize)
                    .map(|e| (t.reach, e.name.clone()));
            }
        }

        let my_pace = self
            .momentum(standings[me].account_id)
            .map(|m| m.last_3h as f64 / 3.0);
        // The last qualifying place, while you are outside it.
        let line = self
            .cutoff
            .map(|n| n as usize)
            .filter(|&n| n >= 1 && me >= n);
        let ladder: Vec<Rung> = standings
            .iter()
            .enumerate()
            .filter(|&(i, e)| {
                i != me
                    && (e.pos <= LADDER_TOP
                        || (i < me && me - i <= LADDER_ABOVE)
                        || (i > me && i - me <= LADDER_BELOW)
                        || line == Some(i + 1))
            })
            .map(|(_, e)| {
                let ahead = e.score - my_score;
                let route = analysis::route(&targets, ahead);
                let momentum = self.momentum(e.account_id);
                let pace = momentum.map(|m| m.last_3h as f64 / 3.0);
                let catch_in = match (my_pace, pace) {
                    (Some(mine), Some(theirs)) if ahead >= 0 && mine > theirs => {
                        Some(((ahead + 1) as f64 / (mine - theirs) * 3600.0).round() as i64)
                    }
                    _ => None,
                };
                Rung {
                    pos: e.pos,
                    account: e.account_id,
                    name: e.name.clone(),
                    score: e.score,
                    ahead,
                    momentum,
                    pace,
                    catch_in,
                    to_pass: route.as_ref().map(|r| (r.picks.len(), r.points)),
                    route_odds: route.as_ref().map(|r| r.odds),
                    route_time: route.as_ref().and_then(|r| {
                        r.picks
                            .iter()
                            .map(|&i| targets[i].time)
                            .sum::<Option<i64>>()
                    }),
                    route: route
                        .map(|r| r.picks.iter().map(|&i| targets[i].stat.id).collect())
                        .unwrap_or_default(),
                }
            })
            .collect();
        let picked = self.goal_key.as_ref().and_then(|key| {
            ladder
                .iter()
                .find(|r| r.ahead >= 0 && account_key(r.account, &r.name) == *key)
                .map(|r| r.pos)
        });
        let goal = picked
            .or_else(|| line.map(|n| standings[n - 1].pos))
            .or_else(|| me.checked_sub(1).map(|i| standings[i].pos));
        self.plan = Plan {
            ladder,
            targets,
            focus,
            goal,
            sort: self.plan_sort,
            my_pace,
            start,
            ahead_tracked: above.len(),
            field: Some(field),
        };
    }

    pub fn rebuild_h2h(&mut self) {
        let standings = &self.data.standings;
        let me_idx = self.me_index();
        let rival_idx = self.rival_index().filter(|i| Some(*i) != me_idx);

        // Your solves, from the challenge list and your own history (the
        // list is unavailable once a CTF ends).
        let mut mine: HashSet<u64> = self
            .data
            .challenges
            .iter()
            .filter(|c| c.solved_by_me)
            .map(|c| c.id)
            .collect();
        let my_history = self.changes_for(me_idx.and_then(|i| standings[i].account_id));
        mine.extend(my_history.iter().flatten().filter_map(|c| c.challenge_id));

        let rival_account = rival_idx.and_then(|i| standings[i].account_id);
        let rival_history = self.changes_for(rival_account);
        let theirs: HashSet<u64> = rival_history
            .iter()
            .flatten()
            .filter_map(|c| c.challenge_id)
            .collect();

        let mut top_counts: HashMap<u64, usize> = HashMap::new();
        for t in self.data.top.iter().take(self.top_n as usize) {
            let solved: HashSet<u64> = t.solves.iter().filter_map(|s| s.challenge_id).collect();
            for id in solved {
                *top_counts.entry(id).or_default() += 1;
            }
        }

        let challenges = self.known_challenges();
        let when = |history: &Option<Vec<ScoreChange>>| -> HashMap<u64, i64> {
            let mut at = HashMap::new();
            for c in history.iter().flatten() {
                if let Some(id) = c.challenge_id {
                    at.entry(id).or_insert(c.t);
                }
            }
            at
        };
        let (mine_at, theirs_at) = (when(&my_history), when(&rival_history));
        let comparison = analysis::compare(
            &challenges,
            &mine,
            &theirs,
            &top_counts,
            analysis::SolvedAt {
                mine: &mine_at,
                theirs: &theirs_at,
            },
        );
        self.rebuild_plan(&challenges, &mine, &theirs, &top_counts);
        let standings = &self.data.standings;

        let party = |i: usize, solves: Option<usize>| Party {
            pos: standings[i].pos,
            account: standings[i].account_id,
            name: standings[i].name.clone(),
            score: standings[i].score,
            solves,
        };
        let me = me_idx.map(|i| party(i, Some(mine.len())));
        let rival = rival_idx.map(|i| party(i, rival_history.as_ref().map(|_| theirs.len())));

        let cutoff = self.cutoff.zip(me.as_ref()).and_then(|(n, me)| {
            let n = n as usize;
            if me.pos as usize <= n {
                // Buffer over the first team outside the line.
                standings.get(n).map(|first_out| Margin {
                    label: format!("#{}", first_out.pos),
                    points: me.score - first_out.score,
                })
            } else {
                standings.get(n - 1).map(|last_in| Margin {
                    label: format!("#{}", last_in.pos),
                    points: me.score - last_in.score,
                })
            }
        });

        self.h2h = HeadToHead {
            rival_loading: rival_idx.is_some() && rival_history.is_none(),
            me_momentum: me_idx.and_then(|i| self.momentum(standings[i].account_id)),
            rival_momentum: self.momentum(rival_account),
            me,
            rival,
            comparison,
            cutoff,
        };
    }
}
