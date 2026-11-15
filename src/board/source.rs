//! Refresh snapshots and account history from a live server or demo world.
//! Cache names learned from solve history when the challenge list is hidden.

use super::model::{AccountKind, Data};
use super::{demo, series, standings};
use crate::client::{CtfdClient, CtfdError};
use crate::models::AccountSolve;
use std::collections::{HashMap, HashSet};

/// Where scoreboard data comes from.
pub enum Source {
    /// A CTFd server; with `since`, only points scored at or after that
    /// Unix time count.
    Live(CtfdClient, Option<i64>, Box<Names>),
    Demo(Box<demo::DemoWorld>),
}

/// Most account-solve lookups one refresh spends on unknown challenge names.
const NAME_LOOKUPS: usize = 5;

/// Challenge names learned so far.
///
/// CTFd hides `/api/v1/challenges` once a CTF ends (and never lists hidden
/// challenges), but an account's solves still name what it solved, so the
/// feed asks a few of the solving accounts instead of showing bare IDs.
#[derive(Default)]
pub struct Names {
    known: HashMap<u64, (String, String)>,
    /// Challenges already looked up, found or not, so a missing name costs
    /// one request rather than one per refresh.
    tried: HashSet<u64>,
}

impl Names {
    fn learn(&mut self, solves: &[AccountSolve]) {
        for c in solves.iter().filter_map(|s| s.challenge.as_ref()) {
            if let (Some(id), Some(name)) = (c.id, &c.name) {
                self.known
                    .entry(id)
                    .or_insert_with(|| (name.clone(), c.category.clone().unwrap_or_default()));
            }
        }
    }

    /// Fill `data.names` with every challenge the top list solved that the
    /// challenge list does not name, looking up a few accounts if needed.
    async fn resolve(&mut self, client: &CtfdClient, kind: AccountKind, data: &mut Data) {
        for c in &data.challenges {
            self.known
                .entry(c.id)
                .or_insert_with(|| (c.name.clone(), c.category.clone()));
        }
        for solves in data.accounts.values() {
            self.learn(solves);
        }
        let mut lookups = 0;
        for entry in &data.top {
            let Some(account) = entry.id else { continue };
            let missing: Vec<u64> = entry
                .solves
                .iter()
                .filter_map(|s| s.challenge_id)
                .filter(|id| !self.known.contains_key(id) && !self.tried.contains(id))
                .collect();
            if missing.is_empty() {
                continue;
            }
            if lookups == NAME_LOOKUPS {
                break;
            }
            lookups += 1;
            self.tried.extend(&missing);
            if let Ok(solves) = client.get_account_solves(kind.path(), account).await {
                self.learn(&solves);
            }
        }
        let listed: HashSet<u64> = data.challenges.iter().map(|c| c.id).collect();
        data.names = self
            .known
            .iter()
            .filter(|(id, _)| !listed.contains(id))
            .map(|(&id, name)| (id, name.clone()))
            .collect();
    }
}

impl Source {
    pub(super) async fn fetch(&mut self, top_n: u32, accounts: &[u64]) -> Result<Data, CtfdError> {
        match self {
            Source::Demo(world) => {
                world.tick(series::now());
                Ok(world.snapshot(top_n, accounts))
            }
            Source::Live(client, None, names) => {
                let (standings, top, challenges) = tokio::join!(
                    client.get_leaderboard(),
                    client.get_scoreboard_top(top_n),
                    client.get_challenges(),
                );
                let standings = standings?;
                let kind = AccountKind::from_standings(&standings);
                let mut data = Data {
                    standings,
                    // The graph and analysis degrade gracefully without these.
                    top: top.unwrap_or_default(),
                    challenges: challenges.unwrap_or_default(),
                    accounts: Default::default(),
                    names: Default::default(),
                };
                for &id in accounts {
                    if let Ok(solves) = client.get_account_solves(kind.path(), id).await {
                        data.accounts.insert(id, solves);
                    }
                }
                names.resolve(client, kind, &mut data).await;
                Ok(data)
            }
            Source::Live(client, Some(since), names) => {
                let since = *since;
                let (board, challenges) = tokio::join!(
                    standings::fetch_standings(client, Some(since)),
                    client.get_challenges(),
                );
                let (standings, history) = board?;
                let kind = AccountKind::from_standings(&standings);
                let mut top = history.unwrap_or_default();
                top.truncate(top_n as usize);
                let mut data = Data {
                    standings,
                    top,
                    challenges: challenges.unwrap_or_default(),
                    accounts: Default::default(),
                    names: Default::default(),
                };
                for &id in accounts {
                    if let Ok(solves) = client.get_account_solves(kind.path(), id).await {
                        names.learn(&solves);
                        data.accounts.insert(id, solves_since(solves, since));
                    }
                }
                names.resolve(client, kind, &mut data).await;
                Ok(data)
            }
        }
    }

    pub(super) async fn account(
        &mut self,
        kind: AccountKind,
        id: u64,
    ) -> Result<Vec<AccountSolve>, CtfdError> {
        match self {
            Source::Demo(world) => Ok(world.account(id)),
            Source::Live(client, since, _) => {
                let solves = client.get_account_solves(kind.path(), id).await?;
                Ok(match since {
                    Some(since) => solves_since(solves, *since),
                    None => solves,
                })
            }
        }
    }
}

/// Keep only the solves made at or after `since` (Unix seconds).
fn solves_since(mut solves: Vec<AccountSolve>, since: i64) -> Vec<AccountSolve> {
    solves.retain(|s| series::parse_time(&s.date).is_some_and(|t| t >= since));
    solves
}
