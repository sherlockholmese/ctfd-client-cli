//! Command names, flags, help text, and argument validation.

use super::time::parse_since_arg;
use super::{ConfigError, Platform, workspace_qualify, workspace_since};
use clap::{Args, Parser, Subcommand, ValueEnum};

const QUICK_START: &str = "\
Quick start:
  ctfdc init https://ctf.example.com   Save URL + token for this folder
  ctfdc dl                             Download every challenge
  ctfdc ls -u                          What's left to solve (with IDs)
  ctfdc s 'flag{...}'                  Submit from inside a challenge folder
  ctfdc s rsa 'flag{...}'              Submit by challenge name or ID
  ctfdc lb                             Print the scoreboard
  ctfdc tui                            Live scoreboard and planning in the terminal
  ctfdc web                            The same in your browser

Run `ctfdc <command> -h` for examples of each command.";

/// CTFd client CLI — interact with a CTFd instance from the terminal.
#[derive(Parser, Debug)]
#[command(
    name = "ctfdc",
    about = "Download CTFd challenges, track progress, and submit flags from the terminal.",
    long_about = None,
    version,
    arg_required_else_help = true,
    after_help = QUICK_START
)]
pub struct Cli {
    #[command(flatten)]
    pub global_args: GlobalArgs,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Args, Debug, Clone)]
pub struct GlobalArgs {
    /// CTFd base URL (e.g. https://ctfd.example.com)
    #[arg(
        long = "url",
        env = "CTFD_URL",
        help = "CTFd base URL (e.g. https://ctfd.example.com)",
        global = true,
        hide_env_values = true,
        help_heading = "Connection (normally read from ctfd.toml)"
    )]
    pub url: Option<String>,

    /// CTFd API token
    #[arg(
        long = "token",
        env = "CTFD_TOKEN",
        help = "CTFd API token for authentication",
        global = true,
        hide_env_values = true,
        help_heading = "Connection (normally read from ctfd.toml)"
    )]
    pub token: Option<String>,

    /// CTF platform (detected by `init` and saved in ctfd.toml)
    #[arg(
        long = "platform",
        env = "CTFD_PLATFORM",
        value_enum,
        global = true,
        help_heading = "Connection (normally read from ctfd.toml)"
    )]
    pub platform: Option<Platform>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Set up this folder for a CTF: verify and save URL + token to ctfd.toml
    #[command(
        visible_alias = "login",
        after_help = "\
Examples:
  ctfdc init https://ctf.example.com   Prompts for the token (hidden)
  ctfdc init https://ctf.example.com --token ctfd_xxx

Create a token in CTFd under Settings -> Access Tokens. Every command run in
this folder, or in a category/challenge folder below it, then uses it."
    )]
    Init(InitArgs),

    /// Show challenges by category with IDs and solved state
    #[command(
        visible_aliases = ["ls", "st"],
        after_help = "\
Examples:
  ctfdc ls          Everything
  ctfdc ls -u       Only unsolved
  ctfdc ls -c web   One category"
    )]
    Status(StatusArgs),

    /// Download challenges (README, metadata, attachments) — safe to re-run
    #[command(
        visible_alias = "dl",
        long_about = "Download challenges (README, metadata, attachments).\n\n\
            Safe to re-run during a CTF: attachments already on disk are \
            skipped, newly released challenges are marked `[new]`, and your \
            own files inside challenge directories are never touched.\n\n\
            Downloads go to the folder containing ctfd.toml (or the current \
            directory), so it works from inside a challenge folder too.",
        after_help = "\
Examples:
  ctfdc dl                     Everything (re-run to fetch new challenges)
  ctfdc dl -u -c pwn -c rev    Unsolved pwn and rev only
  ctfdc dl 42 baby             Challenge #42 and names containing \"baby\"
  ctfdc dl -f \"Baby Login\"   Re-download one challenge's files"
    )]
    Download(DownloadArgs),

    /// Submit flags (auto-detects the challenge from the current folder)
    #[command(
        visible_alias = "s",
        long_about = "Submit flags.\n\n\
            With only a flag, the challenge is detected from the current \
            folder (a downloaded category/challenge directory). Otherwise \
            name the challenge by ID, name, unique part of the name, or \
            Category/Name.",
        after_help = "\
Examples:
  ctfdc s 'flag{...}'                           From inside a challenge folder
  ctfdc s 42 'flag{...}'                        By ID
  ctfdc s 'baby login' 'flag{...}'              By name (or a unique part of it)
  ./solve.py | ctfdc s - -r 'flag\\{[^}]*\\}'     Pull the flag out of output
  ctfdc s -F flags.txt                          Many at once: `CHALLENGE<TAB>FLAG` per line"
    )]
    Submit(SubmitArgs),

    /// Print the scoreboard
    #[command(
        visible_aliases = ["lb", "scoreboard"],
        long_about = "Print the scoreboard: rank, team, score and points behind \
            the team above, with your row marked and the qualifying line \
            drawn.\n\nOn a terminal it is drawn with colour and score bars; \
            piped or with --plain it is bare text for scripts.",
        after_help = "\
Examples:
  ctfdc lb                 The whole scoreboard
  ctfdc lb --top 20        The top 20, and your row
  ctfdc lb --cutoff 10     Qualifying line under #10 (or set cutoff = 10 in ctfd.toml)
  ctfdc lb --since '2026-11-07 09:00'
                           Only count points scored from then on (or set since in ctfd.toml)
  ctfdc lb --plain         Bare text (also what you get when piped)
  ctfdc lb --demo          Try it on fictional data

Live views: ctfdc tui (terminal) and ctfdc web (browser)"
    )]
    Leaderboard(LeaderboardArgs),

    /// Live scoreboard, rival analysis and planning in the terminal
    #[command(
        visible_alias = "live",
        after_help = "\
Examples:
  ctfdc tui                Live view (q quits, ? shows keys)
  ctfdc tui -n 5           Refresh every 5 seconds
  ctfdc tui --top 5        Graph the top 5
  ctfdc tui --cutoff 10    Qualifying line under #10 (or set cutoff = 10 in ctfd.toml)
  ctfdc tui --demo         Try it on fictional data

Four views: Board (standings, graph, latest solves), Analysis (you vs a
rival: gaps, momentum, catch-up targets, categories, your edge), Plan
(what to solve next) and Challenges (read, download, submit flags).  Tab
switches and ? lists all keys.

The same in your browser: ctfdc web"
    )]
    Tui(TuiArgs),

    /// The scoreboard, rival analysis and planning in your browser
    #[command(
        visible_alias = "ui",
        long_about = "The scoreboard, rival analysis and planning in your browser.\n\n\
            Serves a page on this machine and opens it. ctfdc makes every \
            request to the CTF itself, so your token never reaches the \
            browser, and the page only works with the key in the link \
            printed at start.",
        after_help = "\
Examples:
  ctfdc web                Serve the page and open it in your browser
  ctfdc web --demo         Try it on fictional data
  ctfdc web --port 8080 --no-open
                           Fixed port, print the link instead of opening it
  ctfdc web -n 5 --top 5   Refresh every 5 seconds, graph the top 5
  ctfdc web --cutoff 10    Qualifying line under #10 (or set cutoff = 10 in ctfd.toml)

Four tabs: Board (standings, graph, latest solves), Analysis (you vs a
rival), Plan (what to solve next) and Challenges (read, download, submit
flags).  Press ? on the page for its keys.

From another machine, forward the port: ssh -L 8080:127.0.0.1:8080 host,
then run with --port 8080 --no-open and open the printed link.

The same in the terminal: ctfdc tui"
    )]
    Web(WebArgs),

    /// Print a shell completion script (bash, zsh, fish, ...)
    #[command(after_help = "\
Examples:
  ctfdc completions bash > ~/.local/share/bash-completion/completions/ctfdc
  ctfdc completions zsh  > ~/.zfunc/_ctfdc
  ctfdc completions fish > ~/.config/fish/completions/ctfdc.fish")]
    Completions {
        /// Shell to generate completions for
        shell: clap_complete::Shell,
    },
}

/// Options for the `init` command.
#[derive(Args, Debug, Clone)]
pub struct InitArgs {
    /// CTFd URL (prompted for if omitted); `https://` is added if missing
    #[arg(value_name = "URL")]
    pub url: Option<String>,

    /// Save without checking the token against the server
    #[arg(long)]
    pub no_verify: bool,

    /// How many top places qualify (draws the qualifying line on the scoreboard)
    #[arg(
        long,
        visible_alias = "qualify",
        value_name = "N",
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub cutoff: Option<u32>,
}

/// When to draw the printed scoreboard with colour.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorChoice {
    /// On a terminal, unless `NO_COLOR` is set.
    #[default]
    Auto,
    Always,
    Never,
}

/// Options for the `leaderboard` command.
#[derive(Args, Debug, Clone)]
pub struct LeaderboardArgs {
    /// Bare text without colour or bars (the default when piped)
    #[arg(short = 'p', long, visible_alias = "print")]
    pub plain: bool,

    /// Draw with colour and score bars: on a terminal, always, or never
    #[arg(
        long,
        value_name = "WHEN",
        value_enum,
        default_value_t,
        conflicts_with = "plain"
    )]
    pub color: ColorChoice,

    /// Only show this many places, and your row
    #[arg(
        long = "top",
        value_name = "N",
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub top: Option<u32>,

    /// Draw the qualifying line under this place [default: `cutoff` in ctfd.toml]
    #[arg(
        long = "cutoff",
        value_name = "N",
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub cutoff: Option<u32>,

    /// Only count points scored at or after this time, re-ranking the
    /// scoreboard (RFC 3339, `YYYY-MM-DD HH:MM[:SS]` in local time, or Unix
    /// seconds) [default: `since` in ctfd.toml]
    #[arg(long = "since", value_name = "TIME", value_parser = parse_since_arg)]
    pub since: Option<i64>,

    /// Print a fictional scoreboard (no server needed)
    #[arg(long)]
    pub demo: bool,
}

/// What the live views (`tui` and `web`) share: how often to update, what
/// to count, and what to graph.
#[derive(Args, Debug, Clone)]
pub struct BoardArgs {
    /// Seconds between live updates
    #[arg(
        short = 'n',
        long = "interval",
        value_name = "SECS",
        default_value_t = 15,
        value_parser = clap::value_parser!(u64).range(2..=3600)
    )]
    pub interval: u64,

    /// Draw the qualifying line under this place [default: `cutoff` in ctfd.toml]
    #[arg(
        long = "cutoff",
        value_name = "N",
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub cutoff: Option<u32>,

    /// Only count points scored at or after this time, re-ranking the
    /// scoreboard (RFC 3339, `YYYY-MM-DD HH:MM[:SS]` in local time, or Unix
    /// seconds) [default: `since` in ctfd.toml]
    #[arg(long = "since", value_name = "TIME", value_parser = parse_since_arg)]
    pub since: Option<i64>,

    /// Use fictional, simulated data (no server needed)
    #[arg(long)]
    pub demo: bool,

    /// Number of top teams drawn in the graph
    #[arg(
        long = "top",
        value_name = "N",
        default_value_t = 10,
        value_parser = clap::value_parser!(u32).range(1..=25)
    )]
    pub top: u32,
}

impl BoardArgs {
    /// Fill in what the workspace's `ctfd.toml` sets and the flags leave
    /// out: the qualifying line (`cutoff`) and, unless on demo data with
    /// its own clock, the scoring start (`since`).
    pub fn with_workspace_defaults(mut self) -> Result<Self, ConfigError> {
        if self.cutoff.is_none() {
            self.cutoff = workspace_qualify()?;
        }
        if self.since.is_none() && !self.demo {
            self.since = workspace_since()?;
        }
        Ok(self)
    }
}

/// Options for the `tui` command.
#[derive(Args, Debug, Clone)]
pub struct TuiArgs {
    #[command(flatten)]
    pub board: BoardArgs,
}

/// Options for the `web` command.
#[derive(Args, Debug, Clone)]
pub struct WebArgs {
    #[command(flatten)]
    pub board: BoardArgs,

    /// Port to serve on [default: any free port]
    #[arg(long, value_name = "PORT", default_value_t = 0)]
    pub port: u16,

    /// Address to listen on (other machines can reach the page unless
    /// this is a loopback address)
    #[arg(long, value_name = "ADDR", default_value = "127.0.0.1")]
    pub bind: std::net::IpAddr,

    /// Print the link instead of opening a browser
    #[arg(long)]
    pub no_open: bool,
}

/// Options for the `status` command.
#[derive(Args, Debug, Clone)]
pub struct StatusArgs {
    /// Only show unsolved challenges
    #[arg(short = 'u', long)]
    pub unsolved: bool,

    /// Only show this category (case-insensitive, repeatable)
    #[arg(short = 'c', long = "category", value_name = "CATEGORY")]
    pub categories: Vec<String>,
}

/// Options for the `download` command.
#[derive(Args, Debug, Clone)]
pub struct DownloadArgs {
    /// Only download these challenges: numeric IDs or case-insensitive
    /// name substrings (all visible challenges when omitted)
    #[arg(value_name = "CHALLENGE")]
    pub challenges: Vec<String>,

    /// Directory to download into [default: folder with ctfd.toml, else .]
    #[arg(short = 'o', long = "output", value_name = "DIR")]
    pub output: Option<std::path::PathBuf>,

    /// Only download challenges in this category (case-insensitive,
    /// repeatable)
    #[arg(short = 'c', long = "category", value_name = "CATEGORY")]
    pub categories: Vec<String>,

    /// Skip challenges you have already solved
    #[arg(short = 'u', long)]
    pub unsolved: bool,

    /// Number of challenges to download in parallel
    #[arg(
        short = 'j',
        long,
        value_name = "N",
        default_value_t = 8,
        value_parser = clap::value_parser!(u32).range(1..=64)
    )]
    pub jobs: u32,

    /// Re-download attachments even if they already exist locally
    #[arg(short = 'f', long)]
    pub force: bool,
}

/// Options for the `submit` command.
#[derive(Args, Debug, Clone)]
pub struct SubmitArgs {
    /// Challenge ID or name — or, when it is the only argument, the flag
    #[arg(
        value_name = "CHALLENGE|FLAG",
        required_unless_present = "file",
        conflicts_with = "file"
    )]
    pub arg1: Option<String>,

    /// The flag (`-` reads it from stdin)
    #[arg(value_name = "FLAG", conflicts_with = "file")]
    pub arg2: Option<String>,

    /// Submit many flags from a file (`-` for stdin). One per line:
    /// `CHALLENGE<TAB>FLAG`, or `CHALLENGE FLAG` when the challenge is an
    /// ID or a single word. Blank lines and `#` comments are ignored.
    #[arg(short = 'F', long = "file", value_name = "PATH")]
    pub file: Option<std::path::PathBuf>,

    /// Extract the flag from the given text (argument or stdin) with this
    /// regular expression; the first match is submitted
    #[arg(
        short = 'r',
        long = "regex",
        value_name = "PATTERN",
        conflicts_with = "file"
    )]
    pub regex: Option<String>,

    /// Seconds to wait before retrying when CTFd rate-limits submissions
    /// (0 disables retrying)
    #[arg(long = "wait", value_name = "SECS", default_value_t = 10)]
    pub wait: u64,
}
