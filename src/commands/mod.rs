//! Command dispatch. Configuration is resolved only for commands that need it.

pub mod download;
pub mod init;
pub mod leaderboard;
pub mod status;
pub mod submit;
pub mod tui;
pub mod web;

use crate::config::{Cli, Commands, GlobalArgs, ResolvedConfig};
use clap::CommandFactory;

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    let Cli {
        global_args,
        command,
    } = cli;
    match command {
        Commands::Init(args) => init::run(&global_args, args).await,
        Commands::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "ctfdc", &mut std::io::stdout());
            Ok(())
        }
        Commands::Leaderboard(args) if args.demo => leaderboard::run_demo(args),
        Commands::Tui(args) if args.board.demo => tui::run_demo(args).await,
        Commands::Web(args) if args.board.demo => web::run_demo(args).await,
        Commands::Leaderboard(args) => leaderboard::run(resolve(&global_args), args).await,
        Commands::Tui(args) => tui::run(resolve(&global_args), args).await,
        Commands::Web(args) => web::run(resolve(&global_args), args).await,
        Commands::Status(args) => status::run(resolve(&global_args), args).await,
        Commands::Download(args) => download::run(resolve(&global_args), args).await,
        Commands::Submit(args) => submit::run(resolve(&global_args), args).await,
    }
}

fn resolve(args: &GlobalArgs) -> ResolvedConfig {
    args.resolve().unwrap_or_else(|error| {
        eprintln!("Configuration error: {error}");
        std::process::exit(1);
    })
}
