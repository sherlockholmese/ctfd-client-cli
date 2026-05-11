use crate::client::CtfdClient;
use crate::config::ResolvedConfig;

/// Fetch and display the CTFd scoreboard.
///
/// Output is deterministic: rank, name, and score are printed in the order
/// returned by the API. Column widths adapt to the widest entry so long names
/// and large scores remain readable without truncation or misalignment.
pub async fn run(config: ResolvedConfig) -> anyhow::Result<()> {
    let client = CtfdClient::new(&config);
    let entries = client.get_leaderboard().await?;

    if entries.is_empty() {
        println!("No leaderboard entries available.");
        return Ok(());
    }

    // Compute dynamic column widths from the data (with reasonable minimums).
    let max_name_len = entries
        .iter()
        .map(|e| e.name.len())
        .max()
        .unwrap_or(1)
        .max(4);
    let max_score_len = entries
        .iter()
        .map(|e| e.score.to_string().len())
        .max()
        .unwrap_or(1)
        .max(5);
    // Rank width: base it on the width of the largest position number.
    let rank_width = entries
        .iter()
        .map(|e| e.pos.to_string().len())
        .max()
        .unwrap_or(1)
        .max(1);

    for entry in &entries {
        // Explicit positional format: {value_pos:>width_pos$}
        // rank_width=0, entry.pos=1, max_name_len=2, entry.name=3, max_score_len=4, entry.score=5
        println!(
            "{1:>0$}. {3:<2$} {5:>4$}",
            rank_width, entry.pos, max_name_len, entry.name, max_score_len, entry.score,
        );
    }

    Ok(())
}
