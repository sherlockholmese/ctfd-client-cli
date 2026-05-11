use crate::client::CtfdClient;
use crate::config::ResolvedConfig;

/// Fetch and display the CTFd scoreboard.
///
/// Output is deterministic: rank, name, and score are printed in the order
/// returned by the API. An empty scoreboard produces a clear message.
pub async fn run(config: ResolvedConfig) -> anyhow::Result<()> {
    let client = CtfdClient::new(&config);
    let entries = client.get_leaderboard().await?;

    if entries.is_empty() {
        println!("No leaderboard entries available.");
        return Ok(());
    }

    // Deterministic aligned output: rank (right-aligned), name (left-aligned), score (right-aligned)
    for entry in &entries {
        println!("{:>4}. {:<30} {:>8}", entry.pos, entry.name, entry.score);
    }

    Ok(())
}
