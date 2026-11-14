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
            let mut args = args.clone();
            if args.cutoff.is_none() {
                args.cutoff = config::workspace_qualify()?;
            }
            // There is no text form of the demo: without a terminal the
            // terminal view explains what to run instead.
            return match commands::leaderboard::view_here(&args) {
                commands::leaderboard::View::Web => web::run_demo(args).await,
                _ => tui::run_demo(args).await,
            };
        }
        Commands::Web(ref args) if args.demo => {
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
