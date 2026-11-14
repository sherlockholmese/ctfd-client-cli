//! `ctfdc web`: the scoreboard, rival analysis and planning in the
//! browser.  The server itself is [`crate::web`]; this resolves the
//! command's options, which are the terminal view's plus where to serve.

use crate::commands::leaderboard::with_workspace_defaults;
use crate::config::{ResolvedConfig, WebArgs};

/// Serve the web view for a CTFd server.
pub async fn run(config: ResolvedConfig, args: WebArgs) -> anyhow::Result<()> {
    crate::web::run(config, with_workspace_defaults(args.into())?).await
}

/// Serve the web view on fictional, simulated data.  Only the qualifying
/// line is read from the workspace: the demo has its own clock.
pub async fn run_demo(args: WebArgs) -> anyhow::Result<()> {
    let mut args: crate::config::LeaderboardArgs = args.into();
    if args.cutoff.is_none() {
        args.cutoff = crate::config::workspace_qualify()?;
    }
    crate::web::run_demo(args).await
}
