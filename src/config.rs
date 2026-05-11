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
        global = true
    )]
    pub url: Option<String>,

    /// CTFd API token
    #[arg(
        long = "token",
        env = "CTFD_TOKEN",
        help = "CTFd API token for authentication",
        global = true
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
    #[error("CTFd base URL is not configured. Provide --url or set CTFD_URL.")]
    MissingUrl,
    #[error("CTFd base URL is empty. Provide --url or set CTFD_URL.")]
    EmptyUrl,
    #[error("CTFd API token is not configured. Provide --token or set CTFD_TOKEN.")]
    MissingToken,
    #[error("CTFd API token is empty. Provide --token or set CTFD_TOKEN.")]
    EmptyToken,
}

impl GlobalArgs {
    /// Resolve configuration from CLI flags and environment variables.
    ///
    /// Precedence: CLI flags override environment variables.
    /// Clap's `env` attribute merges environment values into the flag fields
    /// when the flag is not provided, so we only need to inspect `self.url`
    /// and `self.token`.  Empty values (whitespace-only) are treated as
    /// missing.  URL trailing slashes are normalized (stripped).
    pub fn resolve(&self) -> Result<ResolvedConfig, ConfigError> {
        // Resolve URL: clap already merged flag + env, so inspect the field.
        let raw_url = match &self.url {
            Some(raw) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return Err(ConfigError::EmptyUrl);
                }
                Some(trimmed.to_string())
            }
            // Neither --url nor CTFD_URL was provided (clap handled both).
            None => None,
        };

        // Resolve token: same logic — clap merged flag + env.
        let raw_token = match &self.token {
            Some(raw) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return Err(ConfigError::EmptyToken);
                }
                Some(trimmed.to_string())
            }
            // Neither --token nor CTFD_TOKEN was provided.
            None => None,
        };

        let base_url = raw_url.ok_or(ConfigError::MissingUrl)?;
        let api_token = raw_token.ok_or(ConfigError::MissingToken)?;

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
}
