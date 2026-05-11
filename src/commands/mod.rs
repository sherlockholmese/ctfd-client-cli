pub mod download;
pub mod leaderboard;
pub mod status;
pub mod submit;

use crate::config::ResolvedConfig;

/// Shared entry point for all command handlers (async).
pub async fn run_command(
    config: ResolvedConfig,
    command: crate::config::Commands,
) -> anyhow::Result<()> {
    match command {
        crate::config::Commands::Leaderboard => leaderboard::run(config).await,
        crate::config::Commands::Status => status::run(config).await,
        crate::config::Commands::Download => download::run(config).await,
        crate::config::Commands::Submit { arg1, arg2 } => {
            submit::run(config, &arg1, arg2.as_deref()).await
        }
    }
}
