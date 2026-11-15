mod board;
mod client;
mod commands;
mod config;
mod fs_util;
mod models;
mod tui;
mod web;
mod workspace;

use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    commands::run(config::Cli::parse()).await
}
