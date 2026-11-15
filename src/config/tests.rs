use super::workspace::read_ctfd_toml_at;
use super::*;
use std::sync::Mutex;

/// Configuration resolution reads the process working directory.
static CWD_LOCK: Mutex<()> = Mutex::new(());

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

/// Model clap's flag/env precedence without mutating the process environment.
/// Integration tests exercise clap's actual environment handling.
fn resolve_with(
    flag_url: Option<&str>,
    flag_token: Option<&str>,
    env_url: Option<&str>,
    env_token: Option<&str>,
) -> Result<ResolvedConfig, ConfigError> {
    resolve_with_cwd(flag_url, flag_token, env_url, env_token, None)
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
    let result = resolve_with(Some("https://ctfd.example.com"), Some("token"), None, None).unwrap();
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
// These use CWD manipulation behind the CWD_LOCK mutex.
// ---------------------------------------------------------------

/// Restore the working directory even if resolution panics.
struct WorkingDirectory(std::path::PathBuf);

impl Drop for WorkingDirectory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).expect("restore working directory");
    }
}

fn resolve_with_cwd(
    flag_url: Option<&str>,
    flag_token: Option<&str>,
    env_url: Option<&str>,
    env_token: Option<&str>,
    cwd: Option<&std::path::Path>,
) -> Result<ResolvedConfig, ConfigError> {
    let _lock = CWD_LOCK.lock().unwrap();
    let isolated = tempfile::tempdir().unwrap();
    let _restore = WorkingDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(cwd.unwrap_or(isolated.path())).unwrap();

    GlobalArgs {
        url: flag_url.or(env_url).map(String::from),
        token: flag_token.or(env_token).map(String::from),
        platform: None,
    }
    .resolve()
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

    let result = resolve_with_cwd(None, Some("flag-token"), None, None, Some(dir.path())).unwrap();
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
