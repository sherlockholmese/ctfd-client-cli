//! Workspace file discovery, validation, and configuration precedence.

use super::{ConfigError, GlobalArgs, Platform, ResolvedConfig, parse_since};

/// Attempt to read configuration from a `ctfd.toml` file.
///
/// Returns `Ok((None, None))` when the file does not exist or is empty.
/// Returns `Ok((Some(url), Some(token)))` or partial values when the
/// file is valid.  Token values are never included in error messages.
pub(super) fn read_ctfd_toml_at(
    path: &std::path::Path,
) -> Result<(Option<String>, Option<String>), ConfigError> {
    let Some(table) = read_table(path)? else {
        return Ok((None, None));
    };
    let url = credential(
        &table,
        "url",
        ConfigError::TomlInvalidUrl,
        ConfigError::TomlEmptyUrl,
    )?;
    let token = credential(
        &table,
        "token",
        ConfigError::TomlInvalidToken,
        ConfigError::TomlEmptyToken,
    )?;
    Ok((url, token))
}

fn credential(
    table: &toml::Table,
    key: &str,
    invalid: ConfigError,
    empty: ConfigError,
) -> Result<Option<String>, ConfigError> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::String(value)) if value.trim().is_empty() => Err(empty),
        Some(toml::Value::String(value)) => Ok(Some(value.trim().to_owned())),
        Some(_) => Err(invalid),
    }
}

fn read_table(path: &std::path::Path) -> Result<Option<toml::Table>, ConfigError> {
    if !path.exists() {
        return Ok(None);
    }
    let content =
        std::fs::read_to_string(path).map_err(|e| ConfigError::TomlReadError(e.to_string()))?;
    if content.trim().is_empty() {
        return Ok(None);
    }
    toml::from_str(&content)
        .map(Some)
        .map_err(|_| ConfigError::TomlMalformed)
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
    read_table(path)
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
    /// Empty values (whitespace-only) from flags or environment produce an
    /// immediate error; they do not fall through to `ctfd.toml`.
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
