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

    // Resolve configuration.  For auto-submit (single-arg submit), the
    // resolution chain includes the challenge-root ctfd.toml as a final
    // fallback after flags, environment, and current-directory ctfd.toml.
    let is_auto_submit = matches!(
        &cli.command,
        crate::config::Commands::Submit { arg2: None, .. }
    );
    let resolved = if is_auto_submit {
        cli.global_args.resolve_for_auto_submit()
    } else {
        cli.global_args.resolve()
    };

    let resolved = match resolved {
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
