//! `ctfdc web`: the scoreboard, rival analysis and planning in the
//! browser.  The server itself is [`crate::web`].

use crate::config::{ResolvedConfig, WebArgs};

/// Serve the web view for a CTFd server.
pub async fn run(config: ResolvedConfig, mut args: WebArgs) -> anyhow::Result<()> {
    args.board = args.board.with_workspace_defaults()?;
    crate::web::run(config, args).await
}

/// Serve the web view on fictional, simulated data.
pub async fn run_demo(mut args: WebArgs) -> anyhow::Result<()> {
    args.board = args.board.with_workspace_defaults()?;
    crate::web::run_demo(args).await
}
