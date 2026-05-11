use clap::{Args, Parser, Subcommand};

/// CTFd CLI — interact with a CTFd instance from the terminal.
#[derive(Parser, Debug)]
#[command(
    name = "ctfd",
    about = "Interact with a CTFd instance from the terminal.",
    long_about = None,
    version
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
        hide_env_values = true
    )]
    pub url: Option<String>,

    /// CTFd API token
    #[arg(
        long = "token",
        env = "CTFD_TOKEN",
        help = "CTFd API token for authentication",
        global = true,
        hide_env_values = true
    )]
    pub token: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Display the CTF leaderboard / scoreboard
    Leaderboard,
    /// Show challenge status (categories, names, solved state)
    Status,
    /// Download challenges to the current directory
    Download,
    /// Submit a flag for a challenge
    Submit {
        /// Numeric CTFd challenge ID
        challenge_id: String,
        /// Flag value to submit
        flag: String,
    },
}

/// Resolved, validated configuration used by all commands.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub base_url: String,
    pub api_token: String,
}

/// Errors that can occur during config resolution.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error(
        "CTFd base URL is not configured. Provide --url, set CTFD_URL, or add url to ctfd.toml."
    )]
    MissingUrl,
    #[error("CTFd base URL is empty. Provide --url or set CTFD_URL.")]
    EmptyUrl,
    #[error(
        "CTFd API token is not configured. Provide --token, set CTFD_TOKEN, or add token to ctfd.toml."
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

impl GlobalArgs {
    /// Resolve configuration from CLI flags, environment variables, and
    /// current-directory `ctfd.toml`.
    ///
    /// Precedence: CLI flags override environment variables, and both
    /// override `ctfd.toml`.  Clap's `env` attribute merges environment
    /// values into the flag fields when the flag is not provided, so we
    /// only need to inspect `self.url` and `self.token`.
    ///
    /// Empty values (whitespace-only) from flags or environment are
    /// treated as missing and produce an immediate error — they do not
    /// fall through to `ctfd.toml`.
    ///
    /// URL trailing slashes are normalized (stripped).
    pub fn resolve(&self) -> Result<ResolvedConfig, ConfigError> {
        // Resolve URL: clap already merged flag + env, so inspect the field.
        let raw_url: Option<String> = match &self.url {
            Some(raw) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return Err(ConfigError::EmptyUrl);
                }
                Some(trimmed.to_string())
            }
            None => None,
        };

        // Resolve token: same logic — clap merged flag + env.
        let raw_token: Option<String> = match &self.token {
            Some(raw) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return Err(ConfigError::EmptyToken);
                }
                Some(trimmed.to_string())
            }
            None => None,
        };

        // If both URL and token are provided by flags or environment,
        // skip ctfd.toml entirely — no need to read or parse it.
        if let (Some(url), Some(token)) = (&raw_url, &raw_token) {
            let base_url = url.trim_end_matches('/').to_string();
            return Ok(ResolvedConfig {
                base_url,
                api_token: token.clone(),
            });
        }

        // One or both values are missing from flags/environment — try
        // reading ctfd.toml from the current directory as a fallback.
        let (toml_url, toml_token) = read_ctfd_toml_at(std::path::Path::new("ctfd.toml"))?;

        let base_url = match raw_url.or(toml_url) {
            Some(u) => u,
            None => return Err(ConfigError::MissingUrl),
        };

        let api_token = match raw_token.or(toml_token) {
            Some(t) => t,
            None => return Err(ConfigError::MissingToken),
        };

        // Normalize trailing slash: strip any trailing slashes
        let base_url = base_url.trim_end_matches('/').to_string();

        Ok(ResolvedConfig {
            base_url,
            api_token,
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

    /// Helper: set env vars, create GlobalArgs, resolve, then clean up.
    ///
    /// Simulates clap's env→flag merging: when a flag field is `None` but the
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

        let args = GlobalArgs { url, token };

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

        let args = GlobalArgs { url, token };

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
        // Flag provides URL, token missing → toml is needed but malformed.
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
