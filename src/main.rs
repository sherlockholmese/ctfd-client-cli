mod client;
mod commands;
mod config;
mod fs_util;
mod models;

use clap::Parser;
use config::Cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Resolve configuration (flags > env, empty rejected, URL normalized)
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
