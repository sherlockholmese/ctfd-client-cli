pub mod download;
pub mod leaderboard;
pub mod status;
pub mod submit;

use crate::config::ResolvedConfig;

/// Shared entry point for all command handlers.
pub fn run_command(config: ResolvedConfig, command: crate::config::Commands) -> anyhow::Result<()> {
    match command {
        crate::config::Commands::Leaderboard => leaderboard::run(config),
        crate::config::Commands::Status => status::run(config),
        crate::config::Commands::Download => download::run(config),
        crate::config::Commands::Submit { challenge_id, flag } => {
            submit::run(config, &challenge_id, &flag)
        }
    }
}
