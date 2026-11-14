use crate::board::series::parse_time;
use crate::client::{CtfdClient, CtfdError};
use crate::config::{LeaderboardArgs, ResolvedConfig};
use crate::models::{ScoreboardEntry, TopEntry};
use std::collections::HashMap;
use std::io::IsTerminal;

/// Where the scoreboard is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// The live page, served to the browser (see [`crate::web`]).
    Web,
    /// The live full-screen terminal view (see [`crate::tui`]).
    Tui,
    /// Printed once as text.
    Plain,
}

/// Pick the view for `args`: text with `--plain` or when the output is
/// not an interactive terminal, else the terminal view.  The web view is
/// `ctfdc web`; its former flags here (`--web`, `--port`, `--bind`,
/// `--no-open`) still select it.
pub fn choose_view(args: &LeaderboardArgs, interactive: bool) -> View {
    let default_bind = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
    if args.plain {
        View::Plain
    } else if args.web || args.no_open || args.port != 0 || args.bind != default_bind {
        View::Web
    } else if interactive {
        View::Tui
    } else {
        View::Plain
    }
}

/// Pick the view for this process.
pub fn view_here(args: &LeaderboardArgs) -> View {
    let interactive = std::io::stdout().is_terminal() && std::io::stdin().is_terminal();
    choose_view(args, interactive)
}

/// Fill in what the workspace's `ctfd.toml` sets and the flags leave out:
/// the qualifying line (`cutoff`) and the scoring start (`since`).
pub fn with_workspace_defaults(mut args: LeaderboardArgs) -> anyhow::Result<LeaderboardArgs> {
    if args.cutoff.is_none() {
        args.cutoff = crate::config::workspace_qualify()?;
    }
    if args.since.is_none() {
        args.since = crate::config::workspace_since()?;
    }
    Ok(args)
}

/// Show the CTFd scoreboard in the terminal, or printed once (see
/// [`choose_view`]).
pub async fn run(config: ResolvedConfig, args: LeaderboardArgs) -> anyhow::Result<()> {
    let args = with_workspace_defaults(args)?;
    match view_here(&args) {
        View::Web => crate::web::run(config, args).await,
        View::Tui => crate::tui::run(config, args).await,
        View::Plain => run_plain(config, args.cutoff, args.since).await,
    }
}

/// Fetch the standings, counting only points scored at or after `since`
/// (Unix seconds) when it is set.
///
/// With `since`, the full score history comes from
/// `/api/v1/scoreboard/top/{N}` (N = every ranked account) and the board is
/// re-ranked; see [`rescore`].  The history of every ranked account is
/// returned alongside, already filtered and in the new rank order.
pub async fn fetch_standings(
    client: &CtfdClient,
    since: Option<i64>,
) -> Result<(Vec<ScoreboardEntry>, Option<Vec<TopEntry>>), CtfdError> {
    let standings = client.get_leaderboard().await?;
    let Some(since) = since else {
        return Ok((standings, None));
    };
    if standings.is_empty() {
        return Ok((standings, Some(Vec::new())));
    }
    let count = u32::try_from(standings.len()).unwrap_or(u32::MAX);
    let history = client.get_scoreboard_top(count).await?;
    let (standings, history) = rescore(standings, history, since);
    Ok((standings, Some(history)))
}

/// Re-rank `standings` counting only the points in `history` scored at or
/// after `since`.
///
/// Accounts are matched to their history by ID (by name when CTFd omits
/// IDs); an account without history scores 0.  Ties are broken like CTFd
/// does: whoever reached the score first ranks higher, then by the original
/// order.  Events with unparseable dates are dropped.  Returns the new
/// standings and the filtered history in the same order (accounts without
/// history are left out of it).
pub fn rescore(
    standings: Vec<ScoreboardEntry>,
    history: Vec<TopEntry>,
    since: i64,
) -> (Vec<ScoreboardEntry>, Vec<TopEntry>) {
    let key = |id: Option<u64>, name: &str| match id {
        Some(id) => format!("id:{id}"),
        None => format!("name:{name}"),
    };
    let mut by_key: HashMap<String, TopEntry> = history
        .into_iter()
        .map(|mut entry| {
            entry
                .solves
                .retain(|e| parse_time(&e.date).is_some_and(|t| t >= since));
            (key(entry.id, &entry.name), entry)
        })
        .collect();

    let mut rows: Vec<(ScoreboardEntry, Option<TopEntry>, i64)> = standings
        .into_iter()
        .map(|mut entry| {
            let hist = by_key.remove(&key(entry.account_id, &entry.name));
            let events = hist.as_ref().map(|h| h.solves.as_slice()).unwrap_or(&[]);
            entry.score = events.iter().map(|e| e.value).sum();
            let last = events
                .iter()
                .filter_map(|e| parse_time(&e.date))
                .max()
                .unwrap_or(i64::MAX);
            (entry, hist, last)
        })
        .collect();
    // Stable sort keeps CTFd's order among exact ties.
    rows.sort_by(|a, b| b.0.score.cmp(&a.0.score).then(a.2.cmp(&b.2)));

    let mut standings = Vec::with_capacity(rows.len());
    let mut history = Vec::new();
    for (i, (mut entry, hist, _)) in rows.into_iter().enumerate() {
        entry.pos = i as i64 + 1;
        standings.push(entry);
        history.extend(hist);
    }
    (standings, history)
}

/// Print the scoreboard once: rank, team, score, and points behind the
/// team above, with your row marked and the qualifying line drawn.
async fn run_plain(
    config: ResolvedConfig,
    cutoff: Option<u32>,
    since: Option<i64>,
) -> anyhow::Result<()> {
    let client = CtfdClient::new(&config);
    let (entries, _) = fetch_standings(&client, since).await?;

    if let Some(since) = since {
        println!("Counting points scored since {}.", format_since(since));
    }

    if entries.is_empty() {
        println!("No leaderboard entries available.");
        return Ok(());
    }

    let me = client.get_me().await.ok();
    let is_me = |e: &ScoreboardEntry| match (&me, e.account_type.as_deref()) {
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

/// Show a scoring start time in local time, e.g. `2026-11-07 09:00 +08:00`.
pub fn format_since(since: i64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_opt(since, 0).single() {
        Some(t) => t.format("%Y-%m-%d %H:%M %:z").to_string(),
        None => since.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ScoreEvent;
    use clap::Parser;

    fn view(flags: &[&str], interactive: bool) -> View {
        let argv = ["ctfdc", "lb"].iter().chain(flags).copied();
        let cli = crate::config::Cli::try_parse_from(argv).unwrap();
        let crate::config::Commands::Leaderboard(args) = cli.command else {
            panic!("not the leaderboard command");
        };
        choose_view(&args, interactive)
    }

    #[test]
    fn the_terminal_view_on_a_terminal_and_text_when_piped() {
        assert_eq!(view(&[], true), View::Tui);
        assert_eq!(view(&[], false), View::Plain);
        assert_eq!(view(&["--plain"], true), View::Plain);
    }

    #[test]
    fn the_former_web_flags_still_serve_the_page() {
        assert_eq!(view(&["--web"], false), View::Web);
        assert_eq!(view(&["--no-open"], true), View::Web);
        assert_eq!(view(&["--port", "8080"], false), View::Web);
        assert_eq!(view(&["--bind", "0.0.0.0"], true), View::Web);
    }

    #[test]
    fn web_takes_the_board_options_of_the_terminal_view() {
        let cli = crate::config::Cli::try_parse_from([
            "ctfdc",
            "web",
            "--port",
            "8080",
            "--no-open",
            "-n",
            "5",
            "--top",
            "3",
            "--cutoff",
            "8",
        ])
        .unwrap();
        let crate::config::Commands::Web(args) = cli.command else {
            panic!("not the web command");
        };
        let args = LeaderboardArgs::from(args);
        assert_eq!((args.port, args.no_open), (8080, true));
        assert_eq!((args.interval, args.top, args.cutoff), (5, 3, Some(8)));
        assert_eq!(choose_view(&args, false), View::Web);
    }

    fn entry(pos: i64, id: u64, name: &str, score: i64) -> ScoreboardEntry {
        ScoreboardEntry {
            pos,
            name: name.into(),
            score,
            account_id: Some(id),
            account_type: Some("team".into()),
        }
    }

    fn hist(id: u64, name: &str, events: &[(i64, &str)]) -> TopEntry {
        TopEntry {
            id: Some(id),
            name: name.into(),
            solves: events
                .iter()
                .map(|(value, date)| ScoreEvent {
                    challenge_id: None,
                    value: *value,
                    date: (*date).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn ignores_points_before_since_and_reranks() {
        let since = parse_time("2026-11-07T10:00:00Z").unwrap();
        let standings = vec![
            entry(1, 1, "A", 600),
            entry(2, 2, "B", 300),
            entry(3, 3, "C", 100),
        ];
        let history = vec![
            hist(
                1,
                "A",
                &[(500, "2026-11-07T09:00:00Z"), (100, "2026-11-07T11:00:00Z")],
            ),
            hist(2, "B", &[(300, "2026-11-07T10:30:00+00:00")]),
            hist(3, "C", &[(100, "2026-11-07T10:00:00Z")]),
        ];
        let (board, history) = rescore(standings, history, since);
        let got: Vec<(i64, &str, i64)> = board
            .iter()
            .map(|e| (e.pos, e.name.as_str(), e.score))
            .collect();
        // A and C tie on 100; C got there first.
        assert_eq!(got, vec![(1, "B", 300), (2, "C", 100), (3, "A", 100)]);
        let names: Vec<&str> = history.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, vec!["B", "C", "A"]);
        assert_eq!(history[2].solves.len(), 1);
    }

    #[test]
    fn ties_go_to_whoever_got_there_first() {
        let since = 0;
        let standings = vec![entry(1, 1, "A", 100), entry(2, 2, "B", 100)];
        let history = vec![
            hist(1, "A", &[(100, "2026-11-07T12:00:00Z")]),
            hist(2, "B", &[(100, "2026-11-07T11:00:00Z")]),
        ];
        let (board, _) = rescore(standings, history, since);
        assert_eq!(board[0].name, "B");
        assert_eq!(board[1].name, "A");
    }

    #[test]
    fn accounts_without_history_score_zero() {
        let standings = vec![entry(1, 1, "A", 100), entry(2, 2, "B", 50)];
        let history = vec![hist(2, "B", &[(50, "2026-11-07T11:00:00Z")])];
        let (board, _) = rescore(standings, history, 0);
        assert_eq!((board[0].name.as_str(), board[0].score), ("B", 50));
        assert_eq!((board[1].name.as_str(), board[1].score), ("A", 0));
    }
}
