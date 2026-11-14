use crate::board::series::parse_time;
use crate::client::{CtfdClient, CtfdError};
use crate::config::{ColorChoice, LeaderboardArgs, ResolvedConfig};
use crate::models::{ScoreboardEntry, TopEntry};
use std::collections::HashMap;
use std::io::IsTerminal;

/// How the scoreboard is printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Bare aligned text, for pipes and scripts.
    Plain,
    /// Drawn for a terminal: score bars, your row marked, a summary;
    /// with or without colour.
    Drawn { color: bool },
}

/// Pick the style: plain with `--plain` or when the output is not a
/// terminal (unless `--color always` asks for the drawn one), else drawn,
/// in colour unless `--color never` or `NO_COLOR` says otherwise.
pub fn choose_style(args: &LeaderboardArgs, terminal: bool, no_color: bool) -> Style {
    match args.color {
        _ if args.plain => Style::Plain,
        ColorChoice::Always => Style::Drawn { color: true },
        ColorChoice::Never if terminal => Style::Drawn { color: false },
        ColorChoice::Auto if terminal => Style::Drawn { color: !no_color },
        _ => Style::Plain,
    }
}

fn style_here(args: &LeaderboardArgs) -> Style {
    let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    choose_style(args, std::io::stdout().is_terminal(), no_color)
}

/// Columns to draw in: the terminal's, within reason.
fn width_here() -> usize {
    ratatui::crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(100)
        .clamp(60, 110)
}

/// What the printed scoreboard shows.
struct Standings {
    host: String,
    entries: Vec<ScoreboardEntry>,
    /// Index of your row.
    me: Option<usize>,
    cutoff: Option<u32>,
    since: Option<i64>,
    /// Only this many places (and your row).
    top: Option<u32>,
}

/// Print the CTFd scoreboard once.  The qualifying line comes from
/// `--cutoff`, else `cutoff` in the nearest `ctfd.toml`; the scoring start
/// time likewise from `--since`, else `since`.
pub async fn run(config: ResolvedConfig, mut args: LeaderboardArgs) -> anyhow::Result<()> {
    if args.cutoff.is_none() {
        args.cutoff = crate::config::workspace_qualify()?;
    }
    if args.since.is_none() {
        args.since = crate::config::workspace_since()?;
    }
    let client = CtfdClient::new(&config);
    let (entries, _) = fetch_standings(&client, args.since).await?;
    let me = match entries.is_empty() {
        true => None,
        false => client.get_me().await.ok(),
    };
    let mine = |e: &ScoreboardEntry| match (&me, e.account_type.as_deref()) {
        (Some(me), Some("user")) => e.account_id.is_some() && e.account_id == me.id,
        (Some(me), _) => e.account_id.is_some() && e.account_id == me.team_id,
        (None, _) => false,
    };
    let host = config
        .base_url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string();
    print(
        &Standings {
            host,
            me: entries.iter().position(mine),
            entries,
            cutoff: args.cutoff,
            since: args.since,
            top: args.top,
        },
        style_here(&args),
    );
    Ok(())
}

/// Print a fictional scoreboard, the one the live views' `--demo` shows.
pub fn run_demo(mut args: LeaderboardArgs) -> anyhow::Result<()> {
    use crate::board::{demo, series};
    if args.cutoff.is_none() {
        args.cutoff = crate::config::workspace_qualify()?;
    }
    let entries = demo::DemoWorld::new(series::now())
        .snapshot(10, &[])
        .standings;
    print(
        &Standings {
            host: "demo.ctfd.invalid".into(),
            me: entries
                .iter()
                .position(|e| e.account_id == Some(demo::ME_ID)),
            entries,
            cutoff: args.cutoff,
            since: None,
            top: args.top,
        },
        style_here(&args),
    );
    Ok(())
}

fn print(standings: &Standings, style: Style) {
    match style {
        Style::Plain => print!("{}", plain(standings)),
        Style::Drawn { color } => print!("{}", drawn(standings, color, width_here())),
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

/// Group thousands: `12345` -> `12,345`.
fn grouped(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if value < 0 {
        out.insert(0, '-');
    }
    out
}

fn text_width(s: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(s)
}

/// `s` cut to `max` columns, ending in an ellipsis when cut.
fn fit(s: &str, max: usize) -> String {
    if text_width(s) <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w > max.saturating_sub(1) {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// The rows to show: all of them, or the first `top` and yours.
fn shown(standings: &Standings) -> Vec<usize> {
    let n = standings.entries.len();
    let top = standings.top.map_or(n, |t| (t as usize).min(n));
    let mut rows: Vec<usize> = (0..top).collect();
    if let Some(me) = standings.me.filter(|&me| me >= top) {
        rows.push(me);
    }
    rows
}

/// The scoreboard as bare text: rank, team, score, and points behind the
/// team above, with your row marked and the qualifying line drawn.
fn plain(standings: &Standings) -> String {
    use std::fmt::Write;
    let entries = &standings.entries;
    let mut out = String::new();
    if let Some(since) = standings.since {
        let _ = writeln!(out, "Counting points scored since {}.", format_since(since));
    }
    if entries.is_empty() {
        let _ = writeln!(out, "No leaderboard entries available.");
        return out;
    }

    let rank_w = entries
        .iter()
        .map(|e| e.pos.to_string().len())
        .max()
        .unwrap_or(1)
        .max(1);
    let name_w = entries
        .iter()
        .map(|e| text_width(&e.name))
        .max()
        .unwrap_or(4)
        .max(4);
    let score_w = entries
        .iter()
        .map(|e| e.score.to_string().len())
        .max()
        .unwrap_or(5)
        .max(5);

    let _ = writeln!(
        out,
        "{:>rank_w$}  Team{}  {:>score_w$}  {:>6}",
        "#",
        " ".repeat(name_w - 4),
        "Score",
        "Behind"
    );
    for i in shown(standings) {
        let entry = &entries[i];
        if standings.cutoff == Some(i as u32) && i > 0 {
            let _ = writeln!(
                out,
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
        let pad = " ".repeat(name_w - text_width(&entry.name));
        let marker = if standings.me == Some(i) {
            "  <- you"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "{:>rank_w$}  {}{pad}  {:>score_w$}  {:>6}{marker}",
            entry.pos, entry.name, entry.score, behind
        );
    }
    out
}

/// Score bars are drawn in eighths of a cell.
const EIGHTHS: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];

/// A bar `fraction` (0..=1) of `width` cells long.
fn bar(fraction: f64, width: usize) -> String {
    let eighths = (fraction.clamp(0.0, 1.0) * (width * 8) as f64).round() as usize;
    let mut out = "█".repeat(eighths / 8);
    if !eighths.is_multiple_of(8) {
        out.push(EIGHTHS[eighths % 8]);
    }
    out
}

/// The scoreboard drawn for a terminal `width` columns wide: a heading, a
/// score bar per team, the podium and your row picked out, the qualifying
/// line, and a sentence on where you stand.
fn drawn(standings: &Standings, color: bool, width: usize) -> String {
    use std::fmt::Write;
    let paint = |codes: &str, text: &str| {
        if color && !codes.is_empty() {
            format!("\x1b[{codes}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    };
    const BOLD: &str = "1";
    const DIM: &str = "2";
    const YOU: &str = "1;36";
    const LINE: &str = "33";
    const GOOD: &str = "32";
    const BAD: &str = "31";

    let entries = &standings.entries;
    let noun = match entries.first().and_then(|e| e.account_type.as_deref()) {
        Some("user") => "players",
        _ => "teams",
    };
    let mut out = String::new();
    let count = format!("{} {noun}", grouped(entries.len() as i64));
    let _ = writeln!(
        out,
        "\n  {}  {}  {}",
        paint(BOLD, "ctfdc"),
        standings.host,
        paint(DIM, &count)
    );
    if let Some(since) = standings.since {
        let _ = writeln!(
            out,
            "  {}",
            paint(
                DIM,
                &format!("Counting points scored since {}", format_since(since))
            )
        );
    }
    out.push('\n');
    if entries.is_empty() {
        let _ = writeln!(
            out,
            "  {}\n",
            paint(DIM, "Nobody is on the scoreboard yet.")
        );
        return out;
    }

    // Every column before the name has a fixed width, and the name comes
    // last: terminals disagree on how wide CJK, emoji and some symbols
    // are, and this way no such disagreement can push a column out of line.
    let rows = shown(standings);
    let rank_w = entries
        .iter()
        .map(|e| e.pos.to_string().len())
        .max()
        .unwrap_or(1);
    let score_w = entries
        .iter()
        .map(|e| grouped(e.score).len())
        .max()
        .unwrap_or(5)
        .max(5);
    let behind_w = 6.max(
        entries
            .windows(2)
            .map(|w| grouped(w[0].score - w[1].score).len())
            .max()
            .unwrap_or(0),
    );
    // Marker, rank, score, behind, each followed by two spaces; the bar.
    let fixed = 2 + rank_w + 2 + score_w + 2 + behind_w + 2;
    let longest = rows
        .iter()
        .map(|&i| text_width(&entries[i].name))
        .max()
        .unwrap_or(4)
        + 5;
    let name_w = longest.clamp(12, 30);
    let bar_w = width.saturating_sub(fixed + 2 + name_w).clamp(8, 32);
    let total = fixed + bar_w + 2 + name_w;
    let best = entries.iter().map(|e| e.score).max().unwrap_or(0).max(1) as f64;

    let _ = writeln!(
        out,
        "{}",
        paint(
            DIM,
            &format!(
                "  {:>rank_w$}  {:>score_w$}  {:>behind_w$}  {:<bar_w$}  Team",
                "#", "Score", "Behind", ""
            )
        )
    );
    let mut previous: Option<usize> = None;
    for &i in &rows {
        let e = &entries[i];
        let mine = standings.me == Some(i);
        if previous.is_some_and(|p| p + 1 != i) {
            let _ = writeln!(out, "  {}", paint(DIM, &format!("{:>rank_w$}", "..")));
        }
        if standings.cutoff == Some(i as u32) && i > 0 {
            let label = format!("── top {i} qualify ");
            let rule = format!(
                "{label}{}",
                "─".repeat(total.saturating_sub(2 + text_width(&label)))
            );
            let _ = writeln!(out, "  {}", paint(LINE, &rule));
        }
        let behind = match i.checked_sub(1).map(|j| entries[j].score - e.score) {
            None => String::new(),
            Some(0) => "tied".into(),
            Some(gap) => grouped(gap),
        };
        let qualifying = standings.cutoff.is_none_or(|n| i < n as usize);
        let bar_style = match (mine, qualifying) {
            (true, _) => YOU,
            (false, true) => "",
            (false, false) => DIM,
        };
        let drawn_bar = bar(e.score.max(0) as f64 / best, bar_w);
        let bar_pad = " ".repeat(bar_w - drawn_bar.chars().count());
        let name = fit(&e.name, name_w - if mine { 5 } else { 0 });
        let name = match (mine, i < 3) {
            (true, _) => format!("{} {}", paint(YOU, &name), paint(DIM, " you")),
            (false, true) => paint(BOLD, &name),
            (false, false) => name,
        };
        let _ = writeln!(
            out,
            "{} {}  {}  {}  {}{bar_pad}  {name}",
            if mine { paint(YOU, ">") } else { " ".into() },
            paint(
                if i < 3 { BOLD } else { DIM },
                &format!("{:>rank_w$}", e.pos)
            ),
            paint(BOLD, &format!("{:>score_w$}", grouped(e.score))),
            paint(DIM, &format!("{behind:>behind_w$}")),
            paint(bar_style, &drawn_bar),
        );
        previous = Some(i);
    }
    let hidden = entries.len() - rows.len();
    if hidden > 0 {
        let _ = writeln!(
            out,
            "  {}",
            paint(
                DIM,
                &format!("{:rank_w$}  and {} more", "", grouped(hidden as i64))
            )
        );
    }

    // Where you stand, in a sentence.
    out.push('\n');
    if let Some(me) = standings.me {
        let e = &entries[me];
        let _ = writeln!(
            out,
            "  You are {} of {} with {} points.",
            paint(YOU, &format!("#{}", e.pos)),
            grouped(entries.len() as i64),
            paint(BOLD, &grouped(e.score))
        );
        let mut gaps = Vec::new();
        if let Some(above) = me.checked_sub(1).map(|j| &entries[j]) {
            gaps.push(format!(
                "{} behind #{} {}",
                paint(BAD, &grouped(above.score - e.score)),
                above.pos,
                fit(&above.name, 28)
            ));
        }
        if let Some(below) = entries.get(me + 1) {
            gaps.push(format!(
                "{} ahead of #{} {}",
                paint(GOOD, &grouped(e.score - below.score)),
                below.pos,
                fit(&below.name, 28)
            ));
        }
        if !gaps.is_empty() {
            let _ = writeln!(out, "  {}.", gaps.join(", "));
        }
        if let Some(n) = standings.cutoff.map(|n| n as usize).filter(|&n| n >= 1) {
            let margin = if me < n {
                entries.get(n).map(|out_| {
                    format!(
                        "  Qualifying by {}.",
                        paint(GOOD, &grouped(e.score - out_.score))
                    )
                })
            } else {
                entries.get(n - 1).map(|in_| {
                    format!(
                        "  {} short of the top {n}.",
                        paint(BAD, &grouped(in_.score - e.score))
                    )
                })
            };
            if let Some(margin) = margin {
                let _ = writeln!(out, "{margin}");
            }
        }
        out.push('\n');
    }
    out
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

    fn style(flags: &[&str], terminal: bool, no_color: bool) -> Style {
        let argv = ["ctfdc", "lb"].iter().chain(flags).copied();
        let cli = crate::config::Cli::try_parse_from(argv).unwrap();
        let crate::config::Commands::Leaderboard(args) = cli.command else {
            panic!("not the leaderboard command");
        };
        choose_style(&args, terminal, no_color)
    }

    #[test]
    fn drawn_on_a_terminal_and_plain_when_piped() {
        assert_eq!(style(&[], true, false), Style::Drawn { color: true });
        assert_eq!(style(&[], true, true), Style::Drawn { color: false });
        assert_eq!(style(&[], false, false), Style::Plain);
        assert_eq!(style(&["--plain"], true, false), Style::Plain);
        assert_eq!(
            style(&["--color", "never"], true, false),
            Style::Drawn { color: false }
        );
        assert_eq!(style(&["--color", "never"], false, false), Style::Plain);
        assert_eq!(
            style(&["--color", "always"], false, true),
            Style::Drawn { color: true }
        );
    }

    fn board(top: Option<u32>, cutoff: Option<u32>) -> Standings {
        Standings {
            host: "ctf.example.com".into(),
            entries: vec![
                entry(1, 1, "Alpha", 12_500),
                entry(2, 2, "Bravo", 9_000),
                entry(3, 3, "Charlie", 9_000),
                entry(4, 4, "Delta", 4_000),
                entry(5, 5, "Echo", 1_250),
            ],
            me: Some(3),
            cutoff,
            since: None,
            top,
        }
    }

    #[test]
    fn draws_bars_your_row_the_line_and_where_you_stand() {
        let text = drawn(&board(None, Some(2)), false, 100);
        assert!(!text.contains('\x1b'), "no colour asked for");
        for needle in [
            "ctfdc  ctf.example.com",
            "5 teams",
            "── top 2 qualify ─",
            "> 4   4,000   5,000",
            "Delta  you",
            "12,500",
            "tied",
            "You are #4 of 5 with 4,000 points.",
            "5,000 behind #3 Charlie, 2,750 ahead of #5 Echo.",
            "5,000 short of the top 2.",
        ] {
            assert!(text.contains(needle), "missing {needle:?}\n{text}");
        }
        // Bars are proportional: the leader's is full, yours about a third.
        let bar_of = |name: &str| {
            let line = text
                .lines()
                .find(|l| l.contains(name) && l.contains('█'))
                .unwrap();
            line.chars().filter(|c| *c == '█').count()
        };
        assert_eq!(bar_of("Alpha"), 32);
        assert_eq!(bar_of("Delta"), 10);

        let coloured = drawn(&board(None, Some(2)), true, 100);
        assert!(coloured.contains("\x1b[1;36m>\x1b[0m"));

        // Nothing of variable width comes before the bars or the names:
        // both start at the same character on every row, whatever the
        // names are.
        let mut wide = board(None, None);
        wide.entries[1].name = "東京ヒープ研究会".into();
        wide.entries[2].name = "🦀 Rustaceans".into();
        let text = drawn(&wide, false, 100);
        let column = |of: &dyn Fn(&str) -> Option<usize>| {
            text.lines()
                .filter(|l| l.contains('█'))
                .filter_map(of)
                .collect::<std::collections::HashSet<usize>>()
        };
        assert_eq!(column(&|l| l.chars().position(|c| c == '█')).len(), 1);
        let names = ["Alpha", "東京", "🦀", "Delta", "Echo"];
        let name_at = |l: &str| {
            names
                .iter()
                .find_map(|n| l.find(n))
                .map(|byte| l[..byte].chars().count())
        };
        assert_eq!(column(&name_at).len(), 1, "{text}");
    }

    #[test]
    fn top_keeps_your_row() {
        let text = drawn(&board(Some(2), None), false, 100);
        assert!(text.contains("  Bravo\n") && !text.contains("  Charlie\n"));
        assert!(text.contains("  ..\n") && text.contains("Delta  you"));
        assert!(text.contains("and 2 more"));
        let bare = plain(&board(Some(2), None));
        assert_eq!(bare.lines().count(), 4, "{bare}");
        assert!(bare.contains("Delta") && bare.contains("<- you"));
    }

    #[test]
    fn live_views_take_their_own_options() {
        use crate::config::{Cli, Commands};
        let cli = Cli::try_parse_from([
            "ctfdc",
            "web",
            "--port",
            "8080",
            "--no-open",
            "-n",
            "5",
            "--top",
            "3",
        ])
        .unwrap();
        let Commands::Web(web) = cli.command else {
            panic!("not the web command");
        };
        assert_eq!((web.port, web.no_open), (8080, true));
        assert_eq!((web.board.interval, web.board.top), (5, 3));

        let cli = Cli::try_parse_from(["ctfdc", "tui", "--cutoff", "8", "--demo"]).unwrap();
        let Commands::Tui(tui) = cli.command else {
            panic!("not the tui command");
        };
        assert_eq!((tui.board.cutoff, tui.board.demo), (Some(8), true));

        // The printed scoreboard is not a live view: none of their options.
        for flags in [
            &["--web"][..],
            &["--port", "1"],
            &["-n", "5"],
            &["--no-open"],
        ] {
            let argv = ["ctfdc", "lb"].iter().chain(flags).copied();
            assert!(Cli::try_parse_from(argv).is_err(), "{flags:?}");
        }
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
