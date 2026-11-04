use crate::client::CtfdClient;
use crate::config::{LeaderboardArgs, ResolvedConfig};
use std::io::IsTerminal;

/// Show the CTFd scoreboard.
///
/// On a terminal this opens the live full-screen view (see
/// [`crate::tui`]); with `--plain`, or when stdout is not a terminal, the
/// scoreboard is printed once as text.  The qualifying line comes from
/// `--cutoff`, else `cutoff` in the nearest `ctfd.toml`.
pub async fn run(config: ResolvedConfig, mut args: LeaderboardArgs) -> anyhow::Result<()> {
    if args.cutoff.is_none() {
        args.cutoff = crate::config::workspace_qualify()?;
    }
    if !args.plain && std::io::stdout().is_terminal() && std::io::stdin().is_terminal() {
        return crate::tui::run(config, args).await;
    }
    run_plain(config, args.cutoff).await
}

/// Print the scoreboard once: rank, team, score, and points behind the
/// team above, with your row marked and the qualifying line drawn.
async fn run_plain(config: ResolvedConfig, cutoff: Option<u32>) -> anyhow::Result<()> {
    let client = CtfdClient::new(&config);
    let entries = client.get_leaderboard().await?;

    if entries.is_empty() {
        println!("No leaderboard entries available.");
        return Ok(());
    }

    let me = client.get_me().await.ok();
    let is_me = |e: &crate::models::ScoreboardEntry| match (&me, e.account_type.as_deref()) {
        (Some(me), Some("user")) => e.account_id.is_some() && e.account_id == me.id,
        (Some(me), _) => e.account_id.is_some() && e.account_id == me.team_id,
        (None, _) => false,
    };

    let width = |s: &str| unicode_width::UnicodeWidthStr::width(s);
    let rank_w = entries
        .iter()
        .map(|e| e.pos.to_string().len())
        .max()
        .unwrap_or(1)
        .max(1);
    let name_w = entries
        .iter()
        .map(|e| width(&e.name))
        .max()
        .unwrap_or(4)
        .max(4);
    let score_w = entries
        .iter()
        .map(|e| e.score.to_string().len())
        .max()
        .unwrap_or(5)
        .max(5);

    println!(
        "{:>rank_w$}  Team{}  {:>score_w$}  {:>6}",
        "#",
        " ".repeat(name_w - 4),
        "Score",
        "Behind"
    );
    for (i, entry) in entries.iter().enumerate() {
        if cutoff == Some(i as u32) && i > 0 {
            println!(
                "{:─<1$}",
                format!("── top {i} qualify "),
                rank_w + name_w + score_w + 14
            );
        }
        let behind = i
            .checked_sub(1)
            .map(|j| entries[j].score - entry.score)
            .map(|gap| {
                if gap == 0 {
                    "tied".to_string()
                } else {
                    gap.to_string()
                }
            })
            .unwrap_or_default();
        let pad = " ".repeat(name_w - width(&entry.name));
        let marker = if is_me(entry) { "  <- you" } else { "" };
        println!(
            "{:>rank_w$}  {}{pad}  {:>score_w$}  {:>6}{marker}",
            entry.pos, entry.name, entry.score, behind
        );
    }

    Ok(())
}
