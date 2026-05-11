use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Get a command for the ctfd binary.
fn ctfd() -> Command {
    Command::cargo_bin("ctfd").expect("ctfd binary should exist")
}

/// Start a wiremock server that serves a valid leaderboard response.
async fn start_leaderboard_mock() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"pos": 1, "name": "Test Team", "score": 5000}
            ]
        })))
        .mount(&server)
        .await;
    server
}

// ============================================================================
// VAL-CROSS-003: Help output makes shared configuration discoverable
// ============================================================================

#[test]
fn help_output_lists_all_subcommands() {
    ctfd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("leaderboard"))
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("download"))
        .stdout(predicate::str::contains("submit"));
}

#[test]
fn help_output_mentions_url_config() {
    ctfd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--url"))
        .stdout(predicate::str::contains("CTFD_URL"));
}

#[test]
fn help_output_mentions_token_config() {
    ctfd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--token"))
        .stdout(predicate::str::contains("CTFD_TOKEN"));
}

#[test]
fn subcommand_help_leaderboard_mentions_config() {
    ctfd()
        .arg("leaderboard")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--url"))
        .stdout(predicate::str::contains("--token"));
}

#[test]
fn subcommand_help_status_mentions_config() {
    ctfd()
        .arg("status")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--url"))
        .stdout(predicate::str::contains("--token"));
}

#[test]
fn subcommand_help_download_mentions_config() {
    ctfd()
        .arg("download")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--url"))
        .stdout(predicate::str::contains("--token"));
}

#[test]
fn subcommand_help_submit_mentions_config() {
    ctfd()
        .arg("submit")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--url"))
        .stdout(predicate::str::contains("--token"));
}

// ============================================================================
// VAL-CROSS-012: Help output does not echo token
// ============================================================================

#[test]
fn help_output_does_not_leak_token_value() {
    ctfd()
        .arg("--token")
        .arg("super-secret-token-123")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("super-secret-token-123").not());
}

// ============================================================================
// VAL-CONFIG-003: Empty configuration values are rejected
// ============================================================================

#[test]
fn missing_url_reports_config_error() {
    ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("CTFd base URL")
                .or(predicate::str::contains("URL").and(predicate::str::contains("configure"))),
        );
}

#[test]
fn missing_token_reports_config_error() {
    ctfd()
        .arg("leaderboard")
        .arg("--url")
        .arg("http://localhost:9999")
        .env_remove("CTFD_TOKEN")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("token")
                .or(predicate::str::contains("Token")
                    .or(predicate::str::contains("authentication"))),
        );
}

#[test]
fn empty_url_flag_is_rejected() {
    ctfd()
        .arg("leaderboard")
        .arg("--url")
        .arg("   ")
        .arg("--token")
        .arg("test-token")
        .assert()
        .failure()
        .stderr(predicate::str::contains("empty").or(predicate::str::contains("Empty")));
}

#[test]
fn empty_token_flag_is_rejected() {
    ctfd()
        .arg("leaderboard")
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg("  ")
        .assert()
        .failure()
        .stderr(predicate::str::contains("empty").or(predicate::str::contains("Empty")));
}

#[test]
fn empty_url_env_is_rejected() {
    ctfd()
        .arg("leaderboard")
        .arg("--token")
        .arg("test-token")
        .env("CTFD_URL", "   ")
        .assert()
        .failure()
        .stderr(predicate::str::contains("empty").or(predicate::str::contains("Empty")));
}

#[test]
fn empty_token_env_is_rejected() {
    ctfd()
        .arg("leaderboard")
        .arg("--url")
        .arg("http://localhost:9999")
        .env("CTFD_TOKEN", "  ")
        .assert()
        .failure()
        .stderr(predicate::str::contains("empty").or(predicate::str::contains("Empty")));
}

// ============================================================================
// VAL-CROSS-013: Global flag placement is consistent
// ============================================================================

#[tokio::test]
async fn global_flags_before_subcommand_leaderboard() {
    let server = start_leaderboard_mock().await;
    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .assert()
        .success();
}

#[test]
fn global_flags_before_subcommand_status() {
    ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg("test-token")
        .arg("status")
        .assert()
        .success();
}

#[test]
fn global_flags_before_subcommand_download() {
    ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .assert()
        .success();
}

#[test]
fn global_flags_before_subcommand_submit() {
    ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .assert()
        .success();
}

// ============================================================================
// VAL-CROSS-014: stdout/stderr separation
// ============================================================================

#[tokio::test]
async fn successful_command_prints_to_stdout_not_stderr() {
    let server = start_leaderboard_mock().await;
    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .output()
        .expect("ctfd should run");

    assert!(output.status.success());
    assert!(!output.stdout.is_empty(), "stdout should have output");
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty on success, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn config_error_prints_to_stderr() {
    let output = ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .output()
        .expect("ctfd should run");

    assert!(!output.status.success());
    assert!(
        !output.stderr.is_empty(),
        "stderr should have error message"
    );
}

// ============================================================================
// VAL-CROSS-010: Missing token failures do not leak secrets
// ============================================================================

#[test]
fn missing_config_error_does_not_leak_token_in_env() {
    // Set a token in env but no URL — error should be about missing URL, not leak the token
    let output = ctfd()
        .arg("leaderboard")
        .env("CTFD_TOKEN", "super-secret-token-123")
        .env_remove("CTFD_URL")
        .output()
        .expect("ctfd should run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("super-secret-token-123"),
        "stderr should not contain token: {stderr}"
    );
}

// ============================================================================
// VAL-CONFIG-001: Supported configuration sources are explicit
// ============================================================================

#[tokio::test]
async fn flags_provide_config() {
    let server = start_leaderboard_mock().await;
    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .assert()
        .success();
}

#[tokio::test]
async fn env_vars_provide_config() {
    let server = start_leaderboard_mock().await;
    ctfd()
        .arg("leaderboard")
        .env("CTFD_URL", server.uri())
        .env("CTFD_TOKEN", "test-token")
        .assert()
        .success();
}

// ============================================================================
// VAL-CONFIG-002: CLI flags override environment variables
// ============================================================================

#[tokio::test]
async fn flag_url_overrides_env_url() {
    let flag_server = start_leaderboard_mock().await;
    // flag_server is the one the flag URL points to, so it should be used.
    ctfd()
        .arg("--url")
        .arg(flag_server.uri())
        .arg("--token")
        .arg("flag-token")
        .arg("leaderboard")
        .env("CTFD_URL", "http://env-host:8888")
        .env("CTFD_TOKEN", "env-token")
        .assert()
        .success();
}

// ============================================================================
// VAL-CROSS-001: Shared global configuration is accepted by every subcommand
// ============================================================================

#[tokio::test]
async fn all_subcommands_accept_global_flags() {
    let server = start_leaderboard_mock().await;
    let url = server.uri();

    // leaderboard needs the mock server
    ctfd()
        .arg("--url")
        .arg(&url)
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .assert()
        .success();

    // status, download, submit are stubs that don't make network calls
    ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg("test-token")
        .arg("status")
        .assert()
        .success();

    ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .assert()
        .success();

    ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .assert()
        .success();
}

// ============================================================================
// VAL-CROSS-002: Environment configuration matches flag configuration behavior
// ============================================================================

#[tokio::test]
async fn env_and_flag_config_produce_same_acceptance() {
    let server = start_leaderboard_mock().await;
    let url = server.uri();

    // With flags
    ctfd()
        .arg("--url")
        .arg(&url)
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .assert()
        .success();

    // With env
    ctfd()
        .arg("leaderboard")
        .env("CTFD_URL", &url)
        .env("CTFD_TOKEN", "test-token")
        .assert()
        .success();
}

// ============================================================================
// Error message clarity
// ============================================================================

#[test]
fn config_error_messages_are_user_friendly() {
    // Missing URL
    let output = ctfd()
        .arg("leaderboard")
        .arg("--token")
        .arg("test-token")
        .env_remove("CTFD_URL")
        .output()
        .expect("ctfd should run");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    // Error should mention URL/base URL and be in plain English
    assert!(
        stderr.to_lowercase().contains("url") || stderr.to_lowercase().contains("base"),
        "Error should mention URL: {stderr}"
    );
}
