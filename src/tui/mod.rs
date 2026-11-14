//! Live full-screen scoreboard (`ctfdc tui`).
//!
//! A background task fetches CTFd data on an interval (or on demand) and
//! sends it to the UI loop, which redraws a few times per second.  The
//! last good data stays on screen when an update fails.

mod app;
mod theme;
mod view;

use crate::board::model::{Cmd, Update};
use crate::board::{self, Board};
use crate::config::{BoardArgs, ResolvedConfig};
use app::App;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use std::io::IsTerminal;
use std::time::Duration;
use tokio::sync::mpsc;

fn ensure_terminal() -> anyhow::Result<()> {
    if std::io::stdout().is_terminal() && std::io::stdin().is_terminal() {
        Ok(())
    } else {
        anyhow::bail!(
            "The live terminal view needs an interactive terminal. Use `ctfdc lb` to print \
             the scoreboard or `ctfdc web` for the page."
        )
    }
}

/// Run the live scoreboard against a CTFd server.
pub async fn run(config: ResolvedConfig, args: BoardArgs) -> anyhow::Result<()> {
    ensure_terminal()?;
    start(board::connect(&config, &args).await?, &args).await
}

/// Run the live scoreboard on fictional, simulated data.
pub async fn run_demo(args: BoardArgs) -> anyhow::Result<()> {
    ensure_terminal()?;
    start(board::connect_demo(&args).await?, &args).await
}

async fn start(board: Board, args: &BoardArgs) -> anyhow::Result<()> {
    let app = App::new(board.model);
    let workspace = board.workspace;
    let (update_tx, update_rx) = mpsc::unbounded_channel::<Update>();
    let action_tx = update_tx.clone();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<Cmd>();
    let fetcher = tokio::spawn(board::fetch_loop(
        board.source,
        args.top,
        app.board.interval,
        app.board.kind,
        app.wanted_accounts(),
        cmd_rx,
        update_tx,
    ));

    let result = tokio::task::spawn_blocking(move || {
        // Downloads, details and flags are the view's own business; the
        // rest goes to the fetcher.
        ui_loop(app, update_rx, cmd_tx, |cmd, model| {
            let workspace = workspace.as_ref();
            match cmd {
                Cmd::Download(what) => board::request_download(workspace, what, &action_tx),
                Cmd::Detail(id) => board::request_detail(workspace, id, &action_tx),
                Cmd::Submit(id, flag) => {
                    let challenge = model.data.challenges.iter().find(|c| c.id == id);
                    board::request_submit(workspace, challenge.cloned(), id, flag, &action_tx);
                }
                other => return Some(other),
            }
            None
        })
    })
    .await;
    fetcher.abort();
    result?
}

fn ui_loop(
    mut app: App,
    mut updates: mpsc::UnboundedReceiver<Update>,
    commands: mpsc::UnboundedSender<Cmd>,
    act: impl Fn(Cmd, &crate::board::model::BoardModel) -> Option<Cmd>,
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
                && let Some(cmd) = act(cmd, &app.board)
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
