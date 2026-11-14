mod board;
mod client;
mod commands;
mod config;
mod fs_util;
mod models;
mod tui;
mod web;

use clap::{CommandFactory, Parser};
use config::{Cli, Commands};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Commands that do not need a resolved configuration.
    match cli.command {
        Commands::Init(args) => return commands::init::run(&cli.global_args, args).await,
        Commands::Leaderboard(ref args) if args.demo => {
            return commands::leaderboard::run_demo(args.clone());
        }
        Commands::Tui(ref args) if args.board.demo => {
            return commands::tui::run_demo(args.clone()).await;
        }
        Commands::Web(ref args) if args.board.demo => {
            return commands::web::run_demo(args.clone()).await;
        }
        Commands::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "ctfdc", &mut std::io::stdout());
            return Ok(());
        }
        _ => {}
    }

    // Resolve configuration (flags > env > nearest ctfd.toml).
    let resolved = match cli.global_args.resolve() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Configuration error: {e}");
            std::process::exit(1);
        }
    };

    // Dispatch to the appropriate command handler (async)
    commands::run_command(resolved, cli.command).await?;

    Ok(())
}
