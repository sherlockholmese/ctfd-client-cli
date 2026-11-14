//! The live scoreboard behind `ctfdc tui` and `ctfdc web`, independent of
//! how it is drawn.
//!
//! [`connect`] / [`connect_demo`] load the first snapshot; [`fetch_loop`]
//! then refreshes it on an interval (or on demand) and sends updates to a
//! [`model::BoardModel`] owned by the terminal ([`crate::tui`]) or web
//! ([`crate::web`]) front end.

pub mod analysis;
pub mod demo;
pub mod model;
pub mod series;

use crate::client::{CtfdClient, CtfdError};
use crate::config::{BoardArgs, ResolvedConfig};
use crate::models::AccountSolve;
use model::{AccountKind, BoardModel, Cmd, Data, Update};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

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
    async fn fetch(&mut self, top_n: u32, accounts: &[u64]) -> Result<Data, CtfdError> {
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
                    crate::commands::leaderboard::fetch_standings(client, Some(since)),
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

    async fn account(
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

fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .trim_end_matches('/')
        .to_string()
}

/// A scoreboard ready to show: its first snapshot and where updates come
/// from.
pub struct Board {
    pub source: Source,
    pub model: BoardModel,
    /// Server host without the scoring-start suffix.
    pub server: String,
    /// Fetches challenge files; `None` on demo data, which has none.
    pub workspace: Option<Workspace>,
}

/// Downloads challenge files for the live views, as `ctfdc dl` does: into
/// the workspace, skipping what is already on disk.
#[derive(Clone)]
pub struct Workspace {
    client: std::sync::Arc<CtfdClient>,
}

impl Workspace {
    /// Download in the background, reporting the start and the outcome on
    /// `updates`.  Nothing is printed.
    fn start(&self, what: model::Download, updates: UnboundedSender<Update>) {
        let client = self.client.clone();
        tokio::spawn(async move {
            let outcome = Self::run(client, what, &updates).await;
            let _ = updates.send(match outcome {
                Ok((ok, title, detail)) => Update::Downloaded { ok, title, detail },
                Err(e) => Update::Downloaded {
                    ok: false,
                    title: "Download failed".into(),
                    detail: e.to_string().lines().next().unwrap_or_default().to_string(),
                },
            });
        });
    }

    async fn run(
        client: std::sync::Arc<CtfdClient>,
        what: model::Download,
        updates: &UnboundedSender<Update>,
    ) -> anyhow::Result<(bool, String, String)> {
        use crate::commands::download;
        let mut challenges = client.get_challenges().await?;
        if let model::Download::One(id) = what {
            challenges.retain(|c| c.id == id);
        }
        let name = match (what, challenges.as_slice()) {
            (_, []) => anyhow::bail!("No challenges are visible to download"),
            (model::Download::One(_), [one]) => one.name.clone(),
            (_, all) => format!("{} challenges", all.len()),
        };
        let _ = updates.send(Update::Downloading(format!("Downloading {name}")));
        let asked = challenges.len();
        let client_for_scan = client.clone();
        let report = download::fetch_into(
            client,
            download::default_output(),
            challenges,
            false,
            download::DEFAULT_JOBS,
            true,
        )
        .await?;
        let dir = std::fs::canonicalize(&report.dir).unwrap_or_else(|_| report.dir.clone());
        let mut detail = format!("{}, in {}", report.details(), dir.display());
        if let Some(first) = report.errors.first() {
            detail = format!("{first}. {detail}");
        }
        // What is on disk now, for the Challenges tab.
        if let Ok(all) = client_for_scan.get_challenges().await {
            let _ = updates.send(Update::OnDisk(Self::on_disk(&all)));
        }
        let title = if report.errors.is_empty() {
            format!("Downloaded {name}")
        } else if asked == 1 {
            format!("Couldn't download {name}")
        } else {
            format!("Downloaded {} of {asked} challenges", report.challenges)
        };
        Ok((report.errors.is_empty(), title, detail))
    }
}

impl Workspace {
    fn root() -> std::path::PathBuf {
        crate::commands::download::default_output()
    }

    /// Which of `challenges` have a folder with metadata in the workspace.
    pub fn on_disk(challenges: &[crate::models::Challenge]) -> HashSet<u64> {
        let root = Self::root();
        challenges
            .iter()
            .filter(|c| {
                root.join(crate::fs_util::safe_challenge_path(&c.category, &c.name))
                    .join("metadata.json")
                    .is_file()
            })
            .map(|c| c.id)
            .collect()
    }

    async fn detail(client: &CtfdClient, id: u64) -> Result<model::ChallengeInfo, String> {
        let d = client
            .get_challenge_detail(id)
            .await
            .map_err(|e| e.to_string().lines().next().unwrap_or_default().to_string())?;
        // File URLs carry a signed token; only the name is for showing.
        let name_of = |url: &String| {
            let path = url.split(['?', '#']).next().unwrap_or(url);
            path.rsplit('/').next().unwrap_or(path).to_string()
        };
        Ok(model::ChallengeInfo {
            description: d.description,
            connection_info: d.connection_info.filter(|c| !c.trim().is_empty()),
            files: d.files.iter().flatten().map(name_of).collect(),
            tags: d.tags.unwrap_or_default(),
            max_attempts: d.max_attempts.filter(|&n| n > 0),
        })
    }

    async fn submit(
        client: &CtfdClient,
        challenge: Option<crate::models::Challenge>,
        id: u64,
        flag: &str,
    ) -> Update {
        use crate::models::SubmitOutcome;
        match client.submit_flag(id, flag).await {
            Ok(response) => {
                let outcome = response.outcome();
                if matches!(
                    outcome,
                    SubmitOutcome::Accepted | SubmitOutcome::AlreadySolved
                ) && let Some(c) = challenge
                {
                    // Keep a downloaded copy's metadata in step.
                    crate::commands::submit::mark_local_solved(
                        &Self::root()
                            .join(crate::fs_util::safe_challenge_path(&c.category, &c.name))
                            .join("metadata.json"),
                    );
                }
                Update::Submitted {
                    id,
                    message: response.user_message(),
                    outcome,
                }
            }
            Err(e) => Update::Submitted {
                id,
                outcome: SubmitOutcome::Rejected,
                message: format!(
                    "Not sent: {}",
                    e.to_string().lines().next().unwrap_or_default()
                ),
            },
        }
    }
}

/// Start a download for a live view, or say why there is none to start.
pub fn request_download(
    workspace: Option<&Workspace>,
    what: model::Download,
    updates: &UnboundedSender<Update>,
) {
    match workspace {
        Some(d) => d.start(what, updates.clone()),
        None => {
            let _ = updates.send(Update::Downloaded {
                ok: false,
                title: "Nothing to download".into(),
                detail: "Demo data has no challenge files".into(),
            });
        }
    }
}

/// Fetch a challenge's description and files for a live view.  Demo data
/// gets a stand-in, so the tab can be tried.
pub fn request_detail(workspace: Option<&Workspace>, id: u64, updates: &UnboundedSender<Update>) {
    let updates = updates.clone();
    match workspace {
        Some(w) => {
            let client = w.client.clone();
            tokio::spawn(async move {
                let result = Workspace::detail(&client, id).await.map(Box::new);
                let _ = updates.send(Update::Detail(id, result));
            });
        }
        None => {
            let _ = updates.send(Update::Detail(
                id,
                Ok(Box::new(model::ChallengeInfo {
                    description: "A fictional challenge. On a real CTF its description, \
                                  connection details and files show here."
                        .into(),
                    connection_info: Some("nc demo.ctfd.invalid 31337".into()),
                    files: vec!["handout.zip".into()],
                    ..Default::default()
                })),
            ));
        }
    }
}

/// Send a flag for a live view and report the server's answer.
pub fn request_submit(
    workspace: Option<&Workspace>,
    challenge: Option<crate::models::Challenge>,
    id: u64,
    flag: String,
    updates: &UnboundedSender<Update>,
) {
    let updates = updates.clone();
    match workspace {
        Some(w) => {
            let client = w.client.clone();
            let _ = updates.send(Update::Submitting);
            tokio::spawn(async move {
                let _ = updates.send(Workspace::submit(&client, challenge, id, &flag).await);
            });
        }
        None => {
            let _ = updates.send(Update::Submitted {
                id,
                outcome: crate::models::SubmitOutcome::Rejected,
                message: "Demo data: there is no server to send flags to.".into(),
            });
        }
    }
}

/// Load the first snapshot from a CTFd server.
pub async fn connect(config: &ResolvedConfig, args: &BoardArgs) -> anyhow::Result<Board> {
    let client = CtfdClient::new(config);
    let me = client.get_me().await.ok();
    let workspace = Workspace {
        client: std::sync::Arc::new(client.clone()),
    };
    let mut source = Source::Live(client, args.since, Box::default());
    let data = source.fetch(args.top, &[]).await?;
    let kind = AccountKind::from_standings(&data.standings);
    let me_account = me.as_ref().and_then(|m| match kind {
        AccountKind::Teams => m.team_id,
        AccountKind::Users => m.id,
    });
    let me_name = match kind {
        AccountKind::Users => me.map(|m| m.name),
        AccountKind::Teams => None,
    };
    let server = host_of(&config.base_url);
    let mut host = server.clone();
    if let Some(since) = args.since {
        host = format!(
            "{host}   since {}",
            crate::commands::leaderboard::format_since(since)
        );
    }
    let mut board = finish(source, host, false, args, me_account, me_name, data).await?;
    board.model.since = args.since;
    board.server = server;
    board.workspace = Some(workspace);
    board.model.on_disk = Workspace::on_disk(&board.model.data.challenges);
    Ok(board)
}

/// Load the first snapshot of the fictional demo CTF.
pub async fn connect_demo(args: &BoardArgs) -> anyhow::Result<Board> {
    let world = demo::DemoWorld::new(series::now());
    let data = world.snapshot(args.top, &[]);
    finish(
        Source::Demo(Box::new(world)),
        "demo.ctfd.invalid".to_string(),
        true,
        args,
        Some(demo::ME_ID),
        Some(demo::ME_NAME.to_string()),
        data,
    )
    .await
}

async fn finish(
    mut source: Source,
    host: String,
    is_demo: bool,
    args: &BoardArgs,
    me_account: Option<u64>,
    me_name: Option<String>,
    data: Data,
) -> anyhow::Result<Board> {
    let mut model = BoardModel::new(
        host,
        is_demo,
        Duration::from_secs(args.interval),
        args.top,
        args.cutoff,
        me_account,
        me_name,
        data,
    );
    // Fetch solves for you / the default rival before the first frame.
    for id in model.wanted_accounts(None) {
        if let Ok(solves) = source.account(model.kind, id).await {
            model.data.accounts.insert(id, solves);
        }
    }
    model.rebuild(false);
    let server = model.host.clone();
    Ok(Board {
        source,
        model,
        server,
        workspace: None,
    })
}

/// Refresh the board every `interval` (or on [`Cmd::Refresh`]) and fetch
/// the solves of accounts the UI asks for, sending results to `updates`.
/// Returns when either channel closes.
pub async fn fetch_loop(
    mut source: Source,
    top_n: u32,
    interval: Duration,
    kind: AccountKind,
    mut accounts: Vec<u64>,
    mut commands: UnboundedReceiver<Cmd>,
    updates: UnboundedSender<Update>,
) {
    let mut next = tokio::time::Instant::now() + interval;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(next) => {}
            cmd = commands.recv() => match cmd {
                None => return,
                Some(Cmd::Refresh) => {}
                // Views start downloads themselves; nothing to fetch here.
                Some(Cmd::Download(_) | Cmd::Detail(_) | Cmd::Submit(..)) => continue,
                Some(Cmd::Accounts(wanted)) => {
                    let new: Vec<u64> =
                        wanted.iter().copied().filter(|id| !accounts.contains(id)).collect();
                    accounts = wanted;
                    for id in new {
                        if let Ok(solves) = source.account(kind, id).await
                            && updates.send(Update::Account(id, solves)).is_err()
                        {
                            return;
                        }
                    }
                    continue;
                }
            },
        }

        if updates.send(Update::Refreshing).is_err() {
            return;
        }
        let update = match source.fetch(top_n, &accounts).await {
            Ok(data) => Update::Data(Box::new(data)),
            Err(e) => Update::Failed(e.to_string().lines().next().unwrap_or_default().to_string()),
        };
        if updates.send(update).is_err() {
            return;
        }
        next = tokio::time::Instant::now() + interval;
    }
}
