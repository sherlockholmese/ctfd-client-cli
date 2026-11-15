//! Configuration entry point. CLI declarations, workspace I/O, and time parsing
//! live in separate modules; callers use the types and helpers re-exported here.

mod cli;
mod time;
mod workspace;

pub use cli::{
    BoardArgs, Cli, ColorChoice, Commands, DownloadArgs, GlobalArgs, InitArgs, LeaderboardArgs,
    StatusArgs, SubmitArgs, TuiArgs, WebArgs,
};
pub use time::{format_since, parse_since};
pub use workspace::{find_workspace_root, workspace_qualify, workspace_since};

use clap::ValueEnum;

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

#[cfg(test)]
mod tests;
