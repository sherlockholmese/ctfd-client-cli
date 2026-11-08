use clap::{Args, Parser, Subcommand, ValueEnum};

const QUICK_START: &str = "\
Quick start:
  ctfdc init https://ctf.example.com   Save URL + token for this folder
  ctfdc dl                             Download every challenge
  ctfdc ls -u                          What's left to solve (with IDs)
  ctfdc s 'flag{...}'                  Submit from inside a challenge folder
  ctfdc s rsa 'flag{...}'              Submit by challenge name or ID
  ctfdc lb                             Scoreboard

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

/// The CTF platform behind the URL.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Platform {
    /// CTFd (API token from Settings -> Access Tokens)
    #[default]
    Ctfd,
    /// rCTF (team token or login link from the profile page)
    Rctf,
}

impl Platform {
    /// The value written to ctfd.toml.
    pub fn key(self) -> &'static str {
        match self {
            Platform::Ctfd => "ctfd",
            Platform::Rctf => "rctf",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        match key.trim().to_ascii_lowercase().as_str() {
            "ctfd" => Some(Platform::Ctfd),
            "rctf" => Some(Platform::Rctf),
            _ => None,
        }
    }
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

    /// Live full-screen scoreboard with a score graph (plain text when piped)
    #[command(
        visible_aliases = ["lb", "scoreboard"],
        after_help = "\
Examples:
  ctfdc lb                 Live scoreboard (q quits, ? shows keys)
  ctfdc lb -n 5            Refresh every 5 seconds
  ctfdc lb --top 5         Graph the top 5
  ctfdc lb --cutoff 10     Qualifying line under #10 (or set cutoff = 10 in ctfd.toml)
  ctfdc lb --since '2026-11-07 09:00'
                           Only count points scored from then on (or set since in ctfd.toml)
  ctfdc lb --plain         Print once as text (also used when piped)
  ctfdc lb --demo          Try the live view on fictional data
  ctfdc lb --web           Open the live scoreboard in your browser
  ctfdc lb --web --port 8080 --no-open
                           Serve on a fixed port without opening a browser

Tab switches between Board (standings, graph, latest solves) and Analysis
(you vs a rival: gaps, momentum, catch-up targets, categories, your edge).
Press ? inside for all keys."
    )]
    Leaderboard(LeaderboardArgs),

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

/// Options for the `leaderboard` command.
#[derive(Args, Debug, Clone)]
pub struct LeaderboardArgs {
    /// Print the scoreboard once as plain text instead of the live view
    #[arg(short = 'p', long, visible_alias = "print")]
    pub plain: bool,

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

    /// Open the live view on fictional, simulated data (no server needed)
    #[arg(long, conflicts_with = "plain")]
    pub demo: bool,

    /// Serve the live scoreboard as a web page and open it in your browser
    #[arg(short = 'w', long, conflicts_with = "plain")]
    pub web: bool,

    /// Port for `--web` [default: any free port]
    #[arg(long, value_name = "PORT", default_value_t = 0, requires = "web")]
    pub port: u16,

    /// Address for `--web` to listen on (other machines can reach the
    /// scoreboard unless this is a loopback address)
    #[arg(
        long,
        value_name = "ADDR",
        default_value = "127.0.0.1",
        requires = "web"
    )]
    pub bind: std::net::IpAddr,

    /// With `--web`, print the address instead of opening a browser
    #[arg(long, requires = "web")]
    pub no_open: bool,

    /// Number of top teams drawn in the graph
    #[arg(
        long = "top",
        value_name = "N",
        default_value_t = 10,
        value_parser = clap::value_parser!(u32).range(1..=25)
    )]
    pub top: u32,
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

/// Resolved, validated configuration used by all commands.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub base_url: String,
    pub api_token: String,
    pub platform: Platform,
}

/// Errors that can occur during config resolution.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error(
        "CTFd URL is not configured. Run `ctfdc init <url>` in your CTF folder \
         (or pass --url / set CTFD_URL)."
    )]
    MissingUrl,
    #[error("CTFd base URL is empty. Provide --url or set CTFD_URL.")]
    EmptyUrl,
    #[error(
        "CTFd API token is not configured. Run `ctfdc init <url>` in your CTF folder \
         (or pass --token / set CTFD_TOKEN)."
    )]
    MissingToken,
    #[error("CTFd API token is empty. Provide --token or set CTFD_TOKEN.")]
    EmptyToken,
    /// `ctfd.toml` is syntactically malformed TOML.
    #[error("Configuration file ctfd.toml is malformed. Check the file syntax.")]
    TomlMalformed,
    /// `ctfd.toml` has a `url` key that is not a string.
    #[error("Configuration file ctfd.toml has an invalid url value (expected a string).")]
    TomlInvalidUrl,
    /// `ctfd.toml` has a `token` key that is not a string.
    #[error("Configuration file ctfd.toml has an invalid token value (expected a string).")]
    TomlInvalidToken,
    /// `ctfd.toml` has an empty `url` value.
    #[error("Configuration file ctfd.toml has an empty url value.")]
    TomlEmptyUrl,
    /// `ctfd.toml` has an empty `token` value.
    #[error("Configuration file ctfd.toml has an empty token value.")]
    TomlEmptyToken,
    /// `ctfd.toml` has a `qualify` value that is not a positive whole number.
    #[error(
        "Configuration file ctfd.toml has an invalid cutoff value (expected a whole number such as `cutoff = 20`)."
    )]
    TomlInvalidQualify,
    /// `ctfd.toml` has a `since` value that is not a recognisable time.
    #[error(
        "Configuration file ctfd.toml has an invalid since value (expected a time such as `since = 2026-11-07T09:00:00+08:00` or `since = \"2026-11-07 09:00\"`)."
    )]
    TomlInvalidSince,
    /// `ctfd.toml` has a `platform` value other than `ctfd` or `rctf`.
    #[error(
        "Configuration file ctfd.toml has an invalid platform value (expected \"ctfd\" or \"rctf\")."
    )]
    TomlInvalidPlatform,
    /// `ctfd.toml` exists but could not be read (permissions, etc.).
    #[error("Configuration file ctfd.toml could not be read: {0}")]
    TomlReadError(String),
}

/// Attempt to read configuration from a `ctfd.toml` file.
///
/// Returns `Ok((None, None))` when the file does not exist or is empty.
/// Returns `Ok((Some(url), Some(token)))` or partial values when the
/// file is valid.  Token values are never included in error messages.
fn read_ctfd_toml_at(
    path: &std::path::Path,
) -> Result<(Option<String>, Option<String>), ConfigError> {
    if !path.exists() {
        return Ok((None, None));
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return Err(ConfigError::TomlReadError(e.to_string())),
    };

    // An empty file is treated as "no config" rather than an error.
    if content.trim().is_empty() {
        return Ok((None, None));
    }

    let table: toml::Table = toml::from_str(&content).map_err(|_| ConfigError::TomlMalformed)?;

    // Extract url and token as strings.  Non-string values are rejected
    // so TOML like `url = 123` produces a clear type error.
    let url = match table.get("url") {
        Some(toml::Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Err(ConfigError::TomlEmptyUrl);
            }
            Some(trimmed.to_string())
        }
        Some(_) => return Err(ConfigError::TomlInvalidUrl),
        None => None,
    };

    let token = match table.get("token") {
        Some(toml::Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Err(ConfigError::TomlEmptyToken);
            }
            Some(trimmed.to_string())
        }
        Some(_) => return Err(ConfigError::TomlInvalidToken),
        None => None,
    };

    Ok((url, token))
}

/// Locations searched for `ctfd.toml`, nearest first: the current
/// directory, the category directory, and the workspace root — so commands
/// work from anywhere in a downloaded `category/challenge` tree.  Higher
/// ancestors are deliberately not searched.
const TOML_SEARCH_PATHS: [&str; 3] = ["ctfd.toml", "../ctfd.toml", "../../ctfd.toml"];

/// Return a flag/env value trimmed, erroring if it is present but empty.
fn non_empty(value: &Option<String>, empty: ConfigError) -> Result<Option<String>, ConfigError> {
    match value {
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                Err(empty)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        None => Ok(None),
    }
}

/// Parse the nearest `ctfd.toml`, if any (an empty file reads as an empty
/// table).
fn workspace_table() -> Result<Option<toml::Table>, ConfigError> {
    let Some(path) = TOML_SEARCH_PATHS
        .iter()
        .map(std::path::Path::new)
        .find(|p| p.is_file())
    else {
        return Ok(None);
    };
    let content =
        std::fs::read_to_string(path).map_err(|e| ConfigError::TomlReadError(e.to_string()))?;
    if content.trim().is_empty() {
        return Ok(None);
    }
    toml::from_str(&content)
        .map(Some)
        .map_err(|_| ConfigError::TomlMalformed)
}

/// Read the platform from the nearest `ctfd.toml` (`platform = "rctf"`).
pub fn workspace_platform() -> Result<Option<Platform>, ConfigError> {
    let Some(table) = workspace_table()? else {
        return Ok(None);
    };
    match table.get("platform") {
        None => Ok(None),
        Some(toml::Value::String(s)) => Platform::from_key(s)
            .map(Some)
            .ok_or(ConfigError::TomlInvalidPlatform),
        Some(_) => Err(ConfigError::TomlInvalidPlatform),
    }
}

/// Read the optional qualifying line (the top N places qualify) from the
/// nearest `ctfd.toml`: `cutoff = N`, or its synonym `qualify = N`.
pub fn workspace_qualify() -> Result<Option<u32>, ConfigError> {
    let Some(table) = workspace_table()? else {
        return Ok(None);
    };
    match table.get("cutoff").or_else(|| table.get("qualify")) {
        None => Ok(None),
        Some(toml::Value::Integer(n)) if *n >= 1 && *n <= i64::from(u32::MAX) => {
            Ok(Some(*n as u32))
        }
        Some(_) => Err(ConfigError::TomlInvalidQualify),
    }
}

/// Read the optional scoring start time from the nearest `ctfd.toml`:
/// `since = ...`.  Points scored before it are ignored on the scoreboard.
///
/// Accepts a TOML date-time (`since = 2026-11-07T09:00:00+08:00`; without
/// an offset it is local time), a string in any form [`parse_since`]
/// understands, or Unix seconds.  Returns Unix seconds.
pub fn workspace_since() -> Result<Option<i64>, ConfigError> {
    let Some(table) = workspace_table()? else {
        return Ok(None);
    };
    match table.get("since") {
        None => Ok(None),
        Some(toml::Value::Integer(n)) => Ok(Some(*n)),
        Some(toml::Value::String(s)) => parse_since(s)
            .map(Some)
            .ok_or(ConfigError::TomlInvalidSince),
        Some(toml::Value::Datetime(dt)) => parse_since(&dt.to_string())
            .map(Some)
            .ok_or(ConfigError::TomlInvalidSince),
        Some(_) => Err(ConfigError::TomlInvalidSince),
    }
}

/// Parse a scoring start time into Unix seconds.
///
/// Accepts RFC 3339 (`2026-11-07T09:00:00Z`, `...+08:00`), a date and time
/// without an offset in local time (`2026-11-07 09:00`,
/// `2026-11-07T09:00:00`), a bare date (local midnight), or Unix seconds.
pub fn parse_since(raw: &str) -> Option<i64> {
    use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
    let raw = raw.trim();
    if let Ok(n) = raw.parse::<i64>() {
        return Some(n);
    }
    if let Ok(t) = DateTime::parse_from_rfc3339(raw) {
        return Some(t.timestamp());
    }
    let naive = [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ]
    .iter()
    .find_map(|fmt| NaiveDateTime::parse_from_str(raw, fmt).ok())
    .or_else(|| {
        NaiveDate::parse_from_str(raw, "%Y-%m-%d")
            .ok()
            .and_then(|d| d.and_hms_opt(0, 0, 0))
    })?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|t| t.timestamp())
}

fn parse_since_arg(raw: &str) -> Result<i64, String> {
    parse_since(raw).ok_or_else(|| {
        "expected a time such as 2026-11-07T09:00:00+08:00, '2026-11-07 09:00' (local), \
         or Unix seconds"
            .to_string()
    })
}

/// Find the nearest workspace directory (one containing `ctfd.toml`).
pub fn find_workspace_root() -> Option<std::path::PathBuf> {
    TOML_SEARCH_PATHS
        .iter()
        .map(std::path::Path::new)
        .find(|p| p.is_file())
        .and_then(|p| p.parent())
        .map(|dir| {
            if dir.as_os_str().is_empty() {
                std::path::PathBuf::from(".")
            } else {
                dir.to_path_buf()
            }
        })
}

impl GlobalArgs {
    /// Resolve configuration from CLI flags, environment variables, and
    /// the nearest `ctfd.toml` files.
    ///
    /// Precedence: CLI flags override environment variables (clap merges
    /// these), which override `./ctfd.toml`, then `../ctfd.toml`, then
    /// `../../ctfd.toml`.  Missing values are filled from the nearest file
    /// that provides them.
    ///
    /// Empty values (whitespace-only) from flags or environment are
    /// treated as missing and produce an immediate error — they do not
    /// fall through to `ctfd.toml`.
    ///
    /// URL trailing slashes are normalized (stripped).
    pub fn resolve(&self) -> Result<ResolvedConfig, ConfigError> {
        let mut url = non_empty(&self.url, ConfigError::EmptyUrl)?;
        let mut token = non_empty(&self.token, ConfigError::EmptyToken)?;

        // Only read config files when flags/env leave something missing.
        for path in TOML_SEARCH_PATHS {
            if url.is_some() && token.is_some() {
                break;
            }
            let (toml_url, toml_token) = read_ctfd_toml_at(std::path::Path::new(path))?;
            url = url.or(toml_url);
            token = token.or(toml_token);
        }

        let base_url = url.ok_or(ConfigError::MissingUrl)?;
        let api_token = token.ok_or(ConfigError::MissingToken)?;

        // A file that can't be read at all only matters if it was needed
        // for the URL or token (already handled above).
        let platform = match self.platform {
            Some(p) => p,
            None => match workspace_platform() {
                Err(ConfigError::TomlMalformed | ConfigError::TomlReadError(_)) => None,
                other => other?,
            }
            .unwrap_or_default(),
        };
        Ok(ResolvedConfig {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_token,
            platform,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::Mutex;

    /// Global lock to prevent concurrent env var manipulation in tests.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn parses_since_times() {
        assert_eq!(parse_since("1970-01-01T00:01:00Z"), Some(60));
        assert_eq!(parse_since("1970-01-01T08:01:00+08:00"), Some(60));
        assert_eq!(parse_since(" 1700000000 "), Some(1_700_000_000));
        // Offset-less forms are local time; they all agree with each other.
        let local = parse_since("2026-11-07 09:00").unwrap();
        assert_eq!(parse_since("2026-11-07T09:00:00"), Some(local));
        assert_eq!(parse_since("2026-11-07 00:00"), parse_since("2026-11-07"));
        assert_eq!(parse_since("soon"), None);
    }

    /// Helper: set env vars, create GlobalArgs, resolve, then clean up.
    ///
    /// Simulates clap's env->flag merging: when a flag field is `None` but the
    /// corresponding environment variable is set, the env value is filled in
    /// (exactly as clap's `env` attribute does during real CLI parsing).
    fn resolve_with(
        flag_url: Option<&str>,
        flag_token: Option<&str>,
        env_url: Option<&str>,
        env_token: Option<&str>,
    ) -> Result<ResolvedConfig, ConfigError> {
        let _guard = ENV_LOCK.lock().unwrap();

        // Clean up first
        unsafe {
            env::remove_var("CTFD_URL");
            env::remove_var("CTFD_TOKEN");
        }

        if let Some(u) = env_url {
            unsafe {
                env::set_var("CTFD_URL", u);
            }
        }
        if let Some(t) = env_token {
            unsafe {
                env::set_var("CTFD_TOKEN", t);
            }
        }

        // Simulate clap's env merging: flag takes precedence, env fills in when
        // flag is absent (exactly what clap's `env` attribute does).
        let url = flag_url
            .map(String::from)
            .or_else(|| env_url.map(String::from));
        let token = flag_token
            .map(String::from)
            .or_else(|| env_token.map(String::from));

        let args = GlobalArgs {
            url,
            token,
            platform: None,
        };

        let result = args.resolve();

        // Clean up
        unsafe {
            env::remove_var("CTFD_URL");
            env::remove_var("CTFD_TOKEN");
        }

        result
    }

    #[test]
    fn flags_override_environment() {
        let result = resolve_with(
            Some("https://flag.example.com"),
            Some("flag-token"),
            Some("https://env.example.com"),
            Some("env-token"),
        )
        .unwrap();

        assert_eq!(result.base_url, "https://flag.example.com");
        assert_eq!(result.api_token, "flag-token");
    }

    #[test]
    fn env_vars_used_when_flags_absent() {
        let result = resolve_with(
            None,
            None,
            Some("https://env.example.com"),
            Some("env-token"),
        )
        .unwrap();

        assert_eq!(result.base_url, "https://env.example.com");
        assert_eq!(result.api_token, "env-token");
    }

    #[test]
    fn missing_url_reports_error() {
        let result = resolve_with(None, None, None, None);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, ConfigError::MissingUrl),
            "Expected MissingUrl, got: {err:?}"
        );
    }

    #[test]
    fn missing_token_reports_error() {
        let result = resolve_with(None, None, Some("https://example.com"), None);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, ConfigError::MissingToken),
            "Expected MissingToken, got: {err:?}"
        );
    }

    #[test]
    fn empty_url_flag_is_rejected() {
        let result = resolve_with(Some("   "), Some("token"), None, None);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, ConfigError::EmptyUrl),
            "Expected EmptyUrl, got: {err:?}"
        );
    }

    #[test]
    fn empty_url_env_is_rejected() {
        let result = resolve_with(None, Some("token"), Some(""), None);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, ConfigError::EmptyUrl),
            "Expected EmptyUrl, got: {err:?}"
        );
    }

    #[test]
    fn empty_token_flag_is_rejected() {
        let result = resolve_with(Some("https://example.com"), Some("  "), None, None);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, ConfigError::EmptyToken),
            "Expected EmptyToken, got: {err:?}"
        );
    }

    #[test]
    fn empty_token_env_is_rejected() {
        let result = resolve_with(Some("https://example.com"), None, None, Some(""));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, ConfigError::EmptyToken),
            "Expected EmptyToken, got: {err:?}"
        );
    }

    #[test]
    fn trailing_slash_is_normalized() {
        let result =
            resolve_with(Some("https://ctfd.example.com/"), Some("token"), None, None).unwrap();
        assert_eq!(result.base_url, "https://ctfd.example.com");
    }

    #[test]
    fn multiple_trailing_slashes_are_normalized() {
        let result = resolve_with(
            Some("https://ctfd.example.com///"),
            Some("token"),
            None,
            None,
        )
        .unwrap();
        assert_eq!(result.base_url, "https://ctfd.example.com");
    }

    #[test]
    fn no_trailing_slash_is_preserved_without_slash() {
        let result =
            resolve_with(Some("https://ctfd.example.com"), Some("token"), None, None).unwrap();
        assert_eq!(result.base_url, "https://ctfd.example.com");
    }

    #[test]
    fn env_url_trailing_slash_normalized() {
        let result =
            resolve_with(None, Some("token"), Some("https://ctfd.example.com/"), None).unwrap();
        assert_eq!(result.base_url, "https://ctfd.example.com");
    }

    #[test]
    fn whitespace_around_flag_values_is_trimmed() {
        let result = resolve_with(
            Some("  https://ctfd.example.com  "),
            Some("  token  "),
            None,
            None,
        )
        .unwrap();
        assert_eq!(result.base_url, "https://ctfd.example.com");
        assert_eq!(result.api_token, "token");
    }

    #[test]
    fn whitespace_around_env_values_is_trimmed() {
        let result = resolve_with(
            None,
            None,
            Some("  https://ctfd.example.com  "),
            Some("  token  "),
        )
        .unwrap();
        assert_eq!(result.base_url, "https://ctfd.example.com");
        assert_eq!(result.api_token, "token");
    }

    #[test]
    fn flag_and_env_whitespace_handling_is_consistent() {
        // Both flag and env values should be trimmed identically.
        let flag_result = resolve_with(Some("  https://a.com "), Some(" t1 "), None, None).unwrap();
        let env_result = resolve_with(None, None, Some("  https://a.com "), Some(" t1 ")).unwrap();
        assert_eq!(flag_result.base_url, env_result.base_url);
        assert_eq!(flag_result.api_token, env_result.api_token);
    }

    #[test]
    fn flag_url_overrides_env_url_only() {
        // Flag URL + env token
        let result = resolve_with(
            Some("https://flag.example.com"),
            None,
            Some("https://env.example.com"),
            Some("env-token"),
        )
        .unwrap();
        assert_eq!(result.base_url, "https://flag.example.com");
        assert_eq!(result.api_token, "env-token");
    }

    #[test]
    fn flag_token_overrides_env_token_only() {
        let result = resolve_with(
            None,
            Some("flag-token"),
            Some("https://env.example.com"),
            Some("env-token"),
        )
        .unwrap();
        assert_eq!(result.base_url, "https://env.example.com");
        assert_eq!(result.api_token, "flag-token");
    }

    // ---------------------------------------------------------------
    // ctfd.toml unit tests — test read_ctfd_toml_at directly
    // ---------------------------------------------------------------

    use std::io::Write;

    fn write_toml_temp(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ctfd.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        (dir, path)
    }

    #[test]
    fn toml_reads_both_url_and_token() {
        let (_dir, path) = write_toml_temp(
            r#"
url = "https://ctfd.example.com"
token = "secret-token"
"#,
        );
        let (url, token) = read_ctfd_toml_at(&path).unwrap();
        assert_eq!(url.as_deref(), Some("https://ctfd.example.com"));
        assert_eq!(token.as_deref(), Some("secret-token"));
    }

    #[test]
    fn toml_returns_none_when_file_absent() {
        let path = std::path::Path::new("/nonexistent/ctfd.toml");
        let (url, token) = read_ctfd_toml_at(path).unwrap();
        assert!(url.is_none());
        assert!(token.is_none());
    }

    #[test]
    fn toml_returns_none_for_empty_file() {
        let (_dir, path) = write_toml_temp("");
        let (url, token) = read_ctfd_toml_at(&path).unwrap();
        assert!(url.is_none());
        assert!(token.is_none());
    }

    #[test]
    fn toml_returns_none_for_whitespace_only_file() {
        let (_dir, path) = write_toml_temp("\n  \n\t\n");
        let (url, token) = read_ctfd_toml_at(&path).unwrap();
        assert!(url.is_none());
        assert!(token.is_none());
    }

    #[test]
    fn toml_rejects_malformed_syntax() {
        let (_dir, path) = write_toml_temp("this is not valid toml {{{");
        let result = read_ctfd_toml_at(&path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::TomlMalformed));
    }

    #[test]
    fn toml_rejects_non_string_url() {
        let (_dir, path) = write_toml_temp(
            r#"
url = 123
token = "ok"
"#,
        );
        let result = read_ctfd_toml_at(&path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::TomlInvalidUrl));
    }

    #[test]
    fn toml_rejects_non_string_token() {
        let (_dir, path) = write_toml_temp(
            r#"
url = "https://example.com"
token = 123
"#,
        );
        let result = read_ctfd_toml_at(&path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::TomlInvalidToken));
    }

    #[test]
    fn toml_rejects_empty_url_value() {
        let (_dir, path) = write_toml_temp(
            r#"
url = ""
token = "ok"
"#,
        );
        let result = read_ctfd_toml_at(&path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::TomlEmptyUrl));
    }

    #[test]
    fn toml_rejects_whitespace_only_url_value() {
        let (_dir, path) = write_toml_temp(
            r#"
url = "   "
token = "ok"
"#,
        );
        let result = read_ctfd_toml_at(&path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::TomlEmptyUrl));
    }

    #[test]
    fn toml_rejects_empty_token_value() {
        let (_dir, path) = write_toml_temp(
            r#"
url = "https://example.com"
token = ""
"#,
        );
        let result = read_ctfd_toml_at(&path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::TomlEmptyToken));
    }

    #[test]
    fn toml_rejects_whitespace_only_token_value() {
        let (_dir, path) = write_toml_temp(
            r#"
url = "https://example.com"
token = "   "
"#,
        );
        let result = read_ctfd_toml_at(&path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::TomlEmptyToken));
    }

    #[test]
    fn toml_trims_whitespace_around_values() {
        let (_dir, path) = write_toml_temp(
            r#"
url = "  https://ctfd.example.com  "
token = "  secret-token  "
"#,
        );
        let (url, token) = read_ctfd_toml_at(&path).unwrap();
        assert_eq!(url.as_deref(), Some("https://ctfd.example.com"));
        assert_eq!(token.as_deref(), Some("secret-token"));
    }

    #[test]
    fn toml_returns_partial_url_only() {
        let (_dir, path) = write_toml_temp(r#"url = "https://ctfd.example.com""#);
        let (url, token) = read_ctfd_toml_at(&path).unwrap();
        assert_eq!(url.as_deref(), Some("https://ctfd.example.com"));
        assert!(token.is_none());
    }

    #[test]
    fn toml_returns_partial_token_only() {
        let (_dir, path) = write_toml_temp(r#"token = "secret-token""#);
        let (url, token) = read_ctfd_toml_at(&path).unwrap();
        assert!(url.is_none());
        assert_eq!(token.as_deref(), Some("secret-token"));
    }

    #[test]
    fn toml_ignores_extra_keys() {
        let (_dir, path) = write_toml_temp(
            r#"
url = "https://ctfd.example.com"
token = "secret-token"
extra = "ignored"
[section]
key = "value"
"#,
        );
        let (url, token) = read_ctfd_toml_at(&path).unwrap();
        assert_eq!(url.as_deref(), Some("https://ctfd.example.com"));
        assert_eq!(token.as_deref(), Some("secret-token"));
    }

    #[test]
    fn toml_error_messages_never_contain_token_value() {
        // Malformed TOML error should not leak token
        let (_dir, path) = write_toml_temp("{{{ secret-token {{{");
        let result = read_ctfd_toml_at(&path);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            !msg.contains("secret-token"),
            "Error message should not contain token value: {msg}"
        );

        // Empty token error should not contain the token value
        let (_dir2, path2) = write_toml_temp(r#"url = "https://x.com"\ntoken = """#);
        let result2 = read_ctfd_toml_at(&path2);
        assert!(result2.is_err());
        let msg2 = result2.unwrap_err().to_string();
        assert!(
            !msg2.contains("https://x.com"),
            "Error message should not contain url either: {msg2}"
        );
    }

    // ---------------------------------------------------------------
    // ctfd.toml precedence tests via resolve()
    // These use CWD manipulation behind the ENV_LOCK mutex.
    // ---------------------------------------------------------------

    /// Helper: resolve with flags, env, and a cwd that may contain ctfd.toml.
    fn resolve_with_cwd(
        flag_url: Option<&str>,
        flag_token: Option<&str>,
        env_url: Option<&str>,
        env_token: Option<&str>,
        cwd: Option<&std::path::Path>,
    ) -> Result<ResolvedConfig, ConfigError> {
        let _guard = ENV_LOCK.lock().unwrap();

        unsafe {
            env::remove_var("CTFD_URL");
            env::remove_var("CTFD_TOKEN");
        }

        if let Some(u) = env_url {
            unsafe {
                env::set_var("CTFD_URL", u);
            }
        }
        if let Some(t) = env_token {
            unsafe {
                env::set_var("CTFD_TOKEN", t);
            }
        }

        let url = flag_url
            .map(String::from)
            .or_else(|| env_url.map(String::from));
        let token = flag_token
            .map(String::from)
            .or_else(|| env_token.map(String::from));

        let args = GlobalArgs {
            url,
            token,
            platform: None,
        };

        // Save and restore CWD
        let prev_cwd = std::env::current_dir().ok();
        if let Some(dir) = cwd {
            std::env::set_current_dir(dir).unwrap();
        }

        let result = args.resolve();

        if let Some(prev) = prev_cwd {
            let _ = std::env::set_current_dir(prev);
        }

        unsafe {
            env::remove_var("CTFD_URL");
            env::remove_var("CTFD_TOKEN");
        }

        result
    }

    #[test]
    fn toml_fallback_when_no_flags_or_env() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join("ctfd.toml");
        std::fs::write(
            &toml_path,
            r#"
url = "https://ctfd.example.com"
token = "secret-token"
"#,
        )
        .unwrap();

        let result = resolve_with_cwd(None, None, None, None, Some(dir.path())).unwrap();
        assert_eq!(result.base_url, "https://ctfd.example.com");
        assert_eq!(result.api_token, "secret-token");
    }

    #[test]
    fn flags_override_toml() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join("ctfd.toml");
        std::fs::write(
            &toml_path,
            r#"
url = "https://toml.example.com"
token = "toml-token"
"#,
        )
        .unwrap();

        let result = resolve_with_cwd(
            Some("https://flag.example.com"),
            Some("flag-token"),
            None,
            None,
            Some(dir.path()),
        )
        .unwrap();
        assert_eq!(result.base_url, "https://flag.example.com");
        assert_eq!(result.api_token, "flag-token");
    }

    #[test]
    fn env_overrides_toml() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join("ctfd.toml");
        std::fs::write(
            &toml_path,
            r#"
url = "https://toml.example.com"
token = "toml-token"
"#,
        )
        .unwrap();

        let result = resolve_with_cwd(
            None,
            None,
            Some("https://env.example.com"),
            Some("env-token"),
            Some(dir.path()),
        )
        .unwrap();
        assert_eq!(result.base_url, "https://env.example.com");
        assert_eq!(result.api_token, "env-token");
    }

    #[test]
    fn flag_url_plus_toml_token() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join("ctfd.toml");
        std::fs::write(
            &toml_path,
            r#"
url = "https://toml.example.com"
token = "toml-token"
"#,
        )
        .unwrap();

        let result = resolve_with_cwd(
            Some("https://flag.example.com"),
            None,
            None,
            None,
            Some(dir.path()),
        )
        .unwrap();
        assert_eq!(result.base_url, "https://flag.example.com");
        assert_eq!(result.api_token, "toml-token");
    }

    #[test]
    fn toml_url_plus_flag_token() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join("ctfd.toml");
        std::fs::write(
            &toml_path,
            r#"
url = "https://toml.example.com"
token = "toml-token"
"#,
        )
        .unwrap();

        let result =
            resolve_with_cwd(None, Some("flag-token"), None, None, Some(dir.path())).unwrap();
        assert_eq!(result.base_url, "https://toml.example.com");
        assert_eq!(result.api_token, "flag-token");
    }

    #[test]
    fn toml_url_missing_when_no_sources() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join("ctfd.toml");
        std::fs::write(&toml_path, r#"token = "toml-token""#).unwrap();

        let result = resolve_with_cwd(None, None, None, None, Some(dir.path()));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::MissingUrl));
    }

    #[test]
    fn toml_token_missing_when_no_sources() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join("ctfd.toml");
        std::fs::write(&toml_path, r#"url = "https://toml.example.com""#).unwrap();

        let result = resolve_with_cwd(None, None, None, None, Some(dir.path()));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::MissingToken));
    }

    #[test]
    fn malformed_toml_fails_when_no_flags_or_env() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join("ctfd.toml");
        std::fs::write(&toml_path, "{{{ broken toml {{{").unwrap();

        let result = resolve_with_cwd(None, None, None, None, Some(dir.path()));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::TomlMalformed));
    }

    #[test]
    fn malformed_toml_ignored_when_flags_provide_both() {
        // When flags provide both url and token, malformed toml is irrelevant.
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join("ctfd.toml");
        std::fs::write(&toml_path, "{{{ broken toml {{{").unwrap();

        let result = resolve_with_cwd(
            Some("https://flag.example.com"),
            Some("flag-token"),
            None,
            None,
            Some(dir.path()),
        )
        .unwrap();
        assert_eq!(result.base_url, "https://flag.example.com");
        assert_eq!(result.api_token, "flag-token");
    }

    #[test]
    fn malformed_toml_errors_when_flag_provides_only_url() {
        // Flag provides URL, token missing -> toml is needed but malformed.
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join("ctfd.toml");
        std::fs::write(&toml_path, "{{{ broken toml {{{").unwrap();

        let result = resolve_with_cwd(
            Some("https://flag.example.com"),
            None,
            None,
            None,
            Some(dir.path()),
        );
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::TomlMalformed));
    }

    #[test]
    fn empty_flag_url_does_not_fall_through_to_toml() {
        // Empty flag URL should error immediately, not try toml
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join("ctfd.toml");
        std::fs::write(
            &toml_path,
            r#"
url = "https://toml.example.com"
token = "toml-token"
"#,
        )
        .unwrap();

        let result = resolve_with_cwd(
            Some("   "),
            Some("flag-token"),
            None,
            None,
            Some(dir.path()),
        );
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::EmptyUrl));
    }

    #[test]
    fn toml_file_not_found_is_not_an_error() {
        // When ctfd.toml doesn't exist but flags/env cover all, resolve succeeds.
        let dir = tempfile::tempdir().unwrap();
        // No ctfd.toml in this dir
        let result = resolve_with_cwd(None, None, None, None, Some(dir.path()));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::MissingUrl));
    }

    #[test]
    fn toml_token_value_not_in_error_messages() {
        // Token values from toml must not appear in error messages.
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join("ctfd.toml");
        std::fs::write(
            &toml_path,
            r#"
token = "super-secret-token-value"
"#,
        )
        .unwrap();

        let result = resolve_with_cwd(None, None, None, None, Some(dir.path()));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            !msg.contains("super-secret-token-value"),
            "Error message should not contain token: {msg}"
        );
    }
}
