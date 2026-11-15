//! The live scoreboard behind `ctfdc tui` and `ctfdc web`, independent of
//! how it is drawn.
//!
//! [`connect`] / [`connect_demo`] load the first snapshot; [`fetch_loop`]
//! then refreshes it on an interval (or on demand) and sends updates to a
//! [`model::BoardModel`] owned by the terminal ([`crate::tui`]) or web
//! ([`crate::web`]) front end.

mod actions;
pub mod analysis;
pub mod demo;
pub mod model;
pub mod series;
mod source;
pub mod standings;

pub use actions::{Workspace, request_detail, request_download, request_submit};
pub use source::Source;

use crate::client::CtfdClient;
use crate::config::{BoardArgs, ResolvedConfig};
use model::{AccountKind, BoardModel, Cmd, Data, Update};
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

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

/// Load the first snapshot from a CTFd server.
pub async fn connect(config: &ResolvedConfig, args: &BoardArgs) -> anyhow::Result<Board> {
    let client = CtfdClient::new(config);
    let me = client.get_me().await.ok();
    let workspace = Workspace::new(client.clone());
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
        host = format!("{host}   since {}", crate::config::format_since(since));
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

/// Stable identity of a scoreboard account.
pub fn account_key(id: Option<u64>, name: &str) -> String {
    match id {
        Some(id) => format!("id:{id}"),
        None => format!("name:{name}"),
    }
}
