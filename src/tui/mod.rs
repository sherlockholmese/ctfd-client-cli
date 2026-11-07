//! Live full-screen scoreboard (`ctfdc lb`).
//!
//! A background task fetches CTFd data on an interval (or on demand) and
//! sends it to the UI loop, which redraws a few times per second.  The
//! last good data stays on screen when an update fails.

pub mod analysis;
mod app;
mod demo;
pub mod series;
mod theme;
mod view;

use crate::client::{CtfdClient, CtfdError};
use crate::config::{LeaderboardArgs, ResolvedConfig};
use app::{AccountKind, App, Cmd, Data, Update};
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use std::io::IsTerminal;
use std::sync::mpsc;
use std::time::Duration;

/// Where scoreboard data comes from.
enum Source {
    /// A CTFd server; with `since`, only points scored at or after that
    /// Unix time count.
    Live(CtfdClient, Option<i64>),
    Demo(Box<demo::DemoWorld>),
}

impl Source {
    async fn fetch(&mut self, top_n: u32, accounts: &[u64]) -> Result<Data, CtfdError> {
        match self {
            Source::Demo(world) => {
                world.tick(series::now());
                Ok(world.snapshot(top_n, accounts))
            }
            Source::Live(client, None) => {
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
                };
                for &id in accounts {
                    if let Ok(solves) = client.get_account_solves(kind.path(), id).await {
                        data.accounts.insert(id, solves);
                    }
                }
                Ok(data)
            }
            Source::Live(client, Some(since)) => {
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
                };
                for &id in accounts {
                    if let Ok(solves) = client.get_account_solves(kind.path(), id).await {
                        data.accounts.insert(id, solves_since(solves, since));
                    }
                }
                Ok(data)
            }
        }
    }

    async fn account(
        &mut self,
        kind: AccountKind,
        id: u64,
    ) -> Result<Vec<crate::models::AccountSolve>, CtfdError> {
        match self {
            Source::Demo(world) => Ok(world.account(id)),
            Source::Live(client, since) => {
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
fn solves_since(
    mut solves: Vec<crate::models::AccountSolve>,
    since: i64,
) -> Vec<crate::models::AccountSolve> {
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

fn ensure_terminal() -> anyhow::Result<()> {
    if std::io::stdout().is_terminal() && std::io::stdin().is_terminal() {
        Ok(())
    } else {
        anyhow::bail!("The live scoreboard needs an interactive terminal. Use `ctfdc lb --plain`.")
    }
}

/// Run the live scoreboard against a CTFd server.
pub async fn run(config: ResolvedConfig, args: LeaderboardArgs) -> anyhow::Result<()> {
    ensure_terminal()?;
    let client = CtfdClient::new(&config);
    let me = client.get_me().await.ok();
    let mut source = Source::Live(client, args.since);
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
    let mut host = host_of(&config.base_url);
    if let Some(since) = args.since {
        host = format!(
            "{host}   since {}",
            crate::commands::leaderboard::format_since(since)
        );
    }
    start(source, host, false, &args, me_account, me_name, data).await
}

/// Run the live scoreboard on fictional, simulated data.
pub async fn run_demo(args: LeaderboardArgs) -> anyhow::Result<()> {
    ensure_terminal()?;
    let world = demo::DemoWorld::new(series::now());
    let data = world.snapshot(args.top, &[]);
    start(
        Source::Demo(Box::new(world)),
        "demo.ctfd.invalid".to_string(),
        true,
        &args,
        Some(demo::ME_ID),
        Some(demo::ME_NAME.to_string()),
        data,
    )
    .await
}

async fn start(
    mut source: Source,
    host: String,
    is_demo: bool,
    args: &LeaderboardArgs,
    me_account: Option<u64>,
    me_name: Option<String>,
    data: Data,
) -> anyhow::Result<()> {
    let interval = Duration::from_secs(args.interval);
    let mut app = App::new(
        host,
        is_demo,
        interval,
        args.top,
        args.cutoff,
        me_account,
        me_name,
        data,
    );

    // Fetch solves for you / the default rival before the first frame.
    let wanted = app.wanted_accounts();
    for &id in &wanted {
        if let Ok(solves) = source.account(app.kind, id).await {
            app.data.accounts.insert(id, solves);
        }
    }
    app.rebuild(false);

    let (update_tx, update_rx) = mpsc::channel::<Update>();
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Cmd>();
    let fetcher = tokio::spawn(fetch_loop(
        source, args.top, interval, app.kind, wanted, cmd_rx, update_tx,
    ));

    let result = tokio::task::spawn_blocking(move || ui_loop(app, update_rx, cmd_tx)).await;
    fetcher.abort();
    result?
}

async fn fetch_loop(
    mut source: Source,
    top_n: u32,
    interval: Duration,
    kind: AccountKind,
    mut accounts: Vec<u64>,
    mut commands: tokio::sync::mpsc::UnboundedReceiver<Cmd>,
    updates: mpsc::Sender<Update>,
) {
    let mut next = tokio::time::Instant::now() + interval;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(next) => {}
            cmd = commands.recv() => match cmd {
                None => return,
                Some(Cmd::Refresh) => {}
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

fn ui_loop(
    mut app: App,
    updates: mpsc::Receiver<Update>,
    commands: tokio::sync::mpsc::UnboundedSender<Cmd>,
) -> anyhow::Result<()> {
    let mut terminal = ratatui::init();
    let result = (|| -> anyhow::Result<()> {
        while !app.quit {
            while let Ok(update) = updates.try_recv() {
                app.apply(update);
            }
            terminal.draw(|f| view::render(f, &mut app))?;
            if event::poll(Duration::from_millis(250))?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
                && let Some(cmd) = app.on_key(key)
            {
                let _ = commands.send(cmd);
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}

#[cfg(test)]
mod snapshot_tests;
