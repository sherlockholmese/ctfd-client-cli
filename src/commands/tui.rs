//! `ctfdc tui`: the live scoreboard, rival analysis and planning in the
//! terminal.  The view itself is [`crate::tui`].

use crate::config::{ResolvedConfig, TuiArgs};

/// Run the live terminal view against a CTFd server.
pub async fn run(config: ResolvedConfig, args: TuiArgs) -> anyhow::Result<()> {
    crate::tui::run(config, args.board.with_workspace_defaults()?).await
}

/// Run the live terminal view on fictional, simulated data.
pub async fn run_demo(args: TuiArgs) -> anyhow::Result<()> {
    crate::tui::run_demo(args.board.with_workspace_defaults()?).await
}
