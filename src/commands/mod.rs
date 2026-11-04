pub mod download;
pub mod init;
pub mod leaderboard;
pub mod status;
pub mod submit;

use crate::config::{Commands, ResolvedConfig};

/// Shared entry point for command handlers that need a resolved
/// configuration (`init` and `completions` are handled in `main`).
pub async fn run_command(config: ResolvedConfig, command: Commands) -> anyhow::Result<()> {
    match command {
        Commands::Leaderboard(args) => leaderboard::run(config, args).await,
        Commands::Status(args) => status::run(config, args).await,
        Commands::Download(args) => download::run(config, args).await,
        Commands::Submit(args) => submit::run(config, args).await,
        Commands::Init(_) | Commands::Completions { .. } => {
            unreachable!("handled before configuration is resolved")
        }
    }
}
