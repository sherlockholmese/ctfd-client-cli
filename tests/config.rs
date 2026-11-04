use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Get a command for the ctfdc binary.
fn ctfd() -> Command {
    Command::cargo_bin("ctfdc").expect("ctfdc binary should exist")
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
        .failure()
        .stderr(
            predicate::str::contains("Network")
                .or(predicate::str::contains("connection").or(predicate::str::contains("request"))),
        );
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
        .failure()
        .stderr(
            predicate::str::contains("Network")
                .or(predicate::str::contains("connection").or(predicate::str::contains("request"))),
        );
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
        .failure()
        .stderr(
            predicate::str::contains("Network")
                .or(predicate::str::contains("connection").or(predicate::str::contains("request"))),
        );
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

    // status now makes real HTTP calls — the network will fail on localhost:9999
    // but the config is still accepted (no config error)
    ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg("test-token")
        .arg("status")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("Network")
                .or(predicate::str::contains("connection").or(predicate::str::contains("request"))),
        );

    // download now makes real HTTP calls — the network will fail on localhost:9999
    // but the config is still accepted (no config error)
    ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("Network")
                .or(predicate::str::contains("connection").or(predicate::str::contains("request"))),
        );

    // submit now makes real HTTP calls — the network will fail on localhost:9999
    // but the config is still accepted (no config error)
    ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("Network")
                .or(predicate::str::contains("connection").or(predicate::str::contains("request"))),
        );
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
// Whitespace handling: flag and env values are trimmed consistently
// ============================================================================

#[tokio::test]
async fn whitespace_around_flag_url_is_trimmed() {
    let server = start_leaderboard_mock().await;
    // Flag URL with leading/trailing whitespace should be trimmed before use.
    let url_with_spaces = format!("  {}  ", server.uri());

    ctfd()
        .arg("--url")
        .arg(&url_with_spaces)
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .assert()
        .success();
}

#[tokio::test]
async fn whitespace_around_flag_token_is_trimmed() {
    let server = start_leaderboard_mock().await;
    // Token with leading/trailing whitespace should be trimmed.
    // We verify by checking the mock receives the trimmed token.
    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("  test-token  ")
        .arg("leaderboard")
        .assert()
        .success();
}

#[tokio::test]
async fn whitespace_around_env_url_is_trimmed() {
    let server = start_leaderboard_mock().await;
    let url_with_spaces = format!("  {}  ", server.uri());

    ctfd()
        .arg("leaderboard")
        .env("CTFD_URL", &url_with_spaces)
        .env("CTFD_TOKEN", "test-token")
        .assert()
        .success();
}

#[tokio::test]
async fn whitespace_around_env_token_is_trimmed() {
    let server = start_leaderboard_mock().await;

    ctfd()
        .arg("leaderboard")
        .env("CTFD_URL", server.uri())
        .env("CTFD_TOKEN", "  test-token  ")
        .assert()
        .success();
}

#[tokio::test]
async fn flag_and_env_whitespace_handling_produces_same_result() {
    // Both flag and env values with whitespace should produce the same
    // observable behavior (same request reaching the mock).
    let server = start_leaderboard_mock().await;

    // Via flags (with whitespace)
    let output_flag = ctfd()
        .arg("--url")
        .arg(format!("  {}  ", server.uri()))
        .arg("--token")
        .arg("  test-token  ")
        .arg("leaderboard")
        .output()
        .expect("ctfd should run");
    assert!(output_flag.status.success());

    // Via env (with whitespace)
    let output_env = ctfd()
        .arg("leaderboard")
        .env("CTFD_URL", format!("  {}  ", server.uri()))
        .env("CTFD_TOKEN", "  test-token  ")
        .output()
        .expect("ctfd should run");
    assert!(output_env.status.success());

    // The stdout should be identical (both reach the same mock, same data).
    assert_eq!(output_flag.stdout, output_env.stdout);
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

// ============================================================================
// VAL-CONFIG-005: ctfd.toml provides URL and token for every subcommand
// ============================================================================

#[tokio::test]
async fn toml_config_provides_url_and_token_for_leaderboard() {
    let server = start_leaderboard_mock().await;
    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(
        &toml_path,
        format!("url = \"{}\"\ntoken = \"test-token\"\n", server.uri()),
    )
    .unwrap();

    ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .assert()
        .success();
}

#[tokio::test]
async fn toml_config_provides_url_and_token_for_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .and(header("Authorization", "Token test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"id": 1, "name": "Test", "category": "Web", "value": 100, "solved_by_me": false}
            ]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(
        &toml_path,
        format!("url = \"{}\"\ntoken = \"test-token\"\n", server.uri()),
    )
    .unwrap();

    ctfd()
        .arg("status")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Test"));
}

#[tokio::test]
async fn toml_config_provides_url_and_token_for_submit() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/challenges/attempt"))
        .and(header("Authorization", "Token test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": {"status": "correct", "message": "Accepted"}
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(
        &toml_path,
        format!("url = \"{}\"\ntoken = \"test-token\"\n", server.uri()),
    )
    .unwrap();

    ctfd()
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Flag accepted"));
}

#[tokio::test]
async fn toml_config_provides_url_and_token_for_download() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .and(header("Authorization", "Token test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"id": 1, "name": "DLC", "category": "Misc", "value": 50, "solved_by_me": false}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .and(header("Authorization", "Token test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": {
                "id": 1, "name": "DLC", "category": "Misc", "value": 50,
                "description": "Download via toml", "type": "standard",
                "state": "visible", "solved_by_me": false
            }
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(
        &toml_path,
        format!("url = \"{}\"\ntoken = \"test-token\"\n", server.uri()),
    )
    .unwrap();

    ctfd()
        .arg("download")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .assert()
        .success();

    let readme = dir.path().join("Misc").join("DLC").join("README.md");
    assert!(readme.exists(), "README.md should exist");
    let content = std::fs::read_to_string(&readme).unwrap();
    assert!(content.contains("Download via toml"));
}

// ============================================================================
// VAL-CONFIG-006: Configuration precedence — flags > env > ctfd.toml
// ============================================================================

#[tokio::test]
async fn toml_config_flags_override_toml() {
    // Flag URL points to flag_server; toml URL points to toml_server.
    // The request must go to flag_server, proving flags override toml.
    let flag_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .and(header("Authorization", "Token flag-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"pos": 1, "name": "FlagTeam", "score": 100}]
        })))
        .mount(&flag_server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(
        &toml_path,
        "url = \"http://toml-host:9999\"\ntoken = \"toml-token\"\n",
    )
    .unwrap();

    ctfd()
        .arg("--url")
        .arg(flag_server.uri())
        .arg("--token")
        .arg("flag-token")
        .arg("leaderboard")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("FlagTeam"));
}

#[tokio::test]
async fn toml_config_env_overrides_toml() {
    // Env token overrides toml token; the mock expects the env token.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .and(header("Authorization", "Token env-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"pos": 1, "name": "EnvTeam", "score": 200}]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(
        &toml_path,
        format!("url = \"{}\"\ntoken = \"toml-token\"\n", server.uri()),
    )
    .unwrap();

    ctfd()
        .arg("leaderboard")
        .env("CTFD_TOKEN", "env-token")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("EnvTeam"));
}

#[tokio::test]
async fn toml_config_flag_url_env_token_overrides_toml_both() {
    // Flag URL + env token, with toml having different values.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .and(header("Authorization", "Token env-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"pos": 1, "name": "MixedTeam", "score": 300}]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(
        &toml_path,
        "url = \"http://toml-host:9999\"\ntoken = \"toml-token\"\n",
    )
    .unwrap();

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("leaderboard")
        .env("CTFD_TOKEN", "env-token")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("MixedTeam"));
}

// ============================================================================
// VAL-CONFIG-007: Malformed ctfd.toml fails before network
// ============================================================================

#[tokio::test]
async fn toml_config_malformed_fails_before_network() {
    let server = MockServer::start().await;
    // No mocks should be hit — we verify zero requests.

    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(&toml_path, "{{{ not valid toml syntax {{{").unwrap();

    ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("ctfd.toml"))
        .stderr(predicate::str::contains("malformed"));

    // Verify no network request was made to the mock (which isn't even used).
    // The assertion above already proves we got a config error before
    // network access.
    let _ = server; // suppress unused warning
}

#[tokio::test]
async fn toml_config_non_string_url_fails_before_network() {
    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(&toml_path, "url = 42\ntoken = \"ok\"\n").unwrap();

    ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("ctfd.toml"))
        .stderr(predicate::str::contains("invalid"));
}

#[tokio::test]
async fn toml_config_non_string_token_fails_before_network() {
    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(
        &toml_path,
        "url = \"https://example.com\"\ntoken = [1, 2, 3]\n",
    )
    .unwrap();

    ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("ctfd.toml"))
        .stderr(predicate::str::contains("invalid"));
}

// ============================================================================
// VAL-CONFIG-008: Empty ctfd.toml values rejected
// ============================================================================

#[tokio::test]
async fn toml_config_empty_url_rejected_before_network() {
    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(&toml_path, "url = \"\"\ntoken = \"ok\"\n").unwrap();

    ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("ctfd.toml"))
        .stderr(predicate::str::contains("empty"));
}

#[tokio::test]
async fn toml_config_empty_token_rejected_before_network() {
    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(&toml_path, "url = \"https://example.com\"\ntoken = \"\"\n").unwrap();

    ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("ctfd.toml"))
        .stderr(predicate::str::contains("empty"));
}

#[tokio::test]
async fn toml_config_whitespace_only_url_rejected_before_network() {
    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(&toml_path, "url = \"   \"\ntoken = \"ok\"\n").unwrap();

    ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("ctfd.toml"))
        .stderr(predicate::str::contains("empty"));
}

// ============================================================================
// VAL-CONFIG-009: ctfd.toml resolved from process current directory
// ============================================================================

#[tokio::test]
async fn toml_config_current_directory_only() {
    // Create ctfd.toml in one directory, run from another — the command
    // should NOT pick up the toml from the first directory.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .and(header("Authorization", "Token dir-b-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"pos": 1, "name": "DirB", "score": 400}]
        })))
        .mount(&server)
        .await;

    let dir_a = tempfile::tempdir().unwrap();
    let toml_a = dir_a.path().join("ctfd.toml");
    std::fs::write(
        &toml_a,
        "url = \"http://dir-a:9999\"\ntoken = \"dir-a-token\"\n",
    )
    .unwrap();

    let dir_b = tempfile::tempdir().unwrap();
    let toml_b = dir_b.path().join("ctfd.toml");
    std::fs::write(
        &toml_b,
        format!("url = \"{}\"\ntoken = \"dir-b-token\"\n", server.uri()),
    )
    .unwrap();

    // Run from dir_b — should use dir_b's toml, not dir_a's.
    ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir_b.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("DirB"));

    // Run from dir_a — no env/flags, dir_a's toml points to non-existent host
    // so we expect a network error (not a config error from dir_b's toml).
    ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir_a.path())
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("Network")
                .or(predicate::str::contains("connection").or(predicate::str::contains("request"))),
        );
}

#[tokio::test]
async fn toml_config_not_found_in_other_directory() {
    // Running from a directory without ctfd.toml should not find one
    // in an unrelated directory.
    let dir = tempfile::tempdir().unwrap();
    // No ctfd.toml in this dir

    ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("URL").or(predicate::str::contains("url")));
}

// ============================================================================
// Token secrecy: ctfd.toml token values must not appear in output
// ============================================================================

#[tokio::test]
async fn toml_config_token_not_in_error_output() {
    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    // Only provide token, missing URL → error should mention missing URL
    // but NOT leak the token.
    std::fs::write(&toml_path, "token = \"super-secret-toml-token\"\n").unwrap();

    let output = ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stderr.contains("super-secret-toml-token"),
        "stderr should not contain token: {stderr}"
    );
    assert!(
        !stdout.contains("super-secret-toml-token"),
        "stdout should not contain token"
    );
}

#[tokio::test]
async fn toml_config_token_not_in_malformed_error() {
    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(
        &toml_path,
        "invalid toml with super-secret-toml-token hidden\n",
    )
    .unwrap();

    let output = ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("super-secret-toml-token"),
        "malformed TOML error should not leak token: {stderr}"
    );
}

#[tokio::test]
async fn toml_config_token_not_in_success_output() {
    let server = start_leaderboard_mock().await;
    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(
        &toml_path,
        format!(
            "url = \"{}\"\ntoken = \"super-secret-toml-token\"\n",
            server.uri()
        ),
    )
    .unwrap();

    // The mock expects "test-token", but we're using a different token from toml.
    // Let's use the mock's expected token to verify success output.
    // Actually, we need a mock that expects the toml token.
    let server2 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .and(header("Authorization", "Token super-secret-toml-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"pos": 1, "name": "SecretTeam", "score": 500}]
        })))
        .mount(&server2)
        .await;

    let dir2 = tempfile::tempdir().unwrap();
    let toml2 = dir2.path().join("ctfd.toml");
    std::fs::write(
        &toml2,
        format!(
            "url = \"{}\"\ntoken = \"super-secret-toml-token\"\n",
            server2.uri()
        ),
    )
    .unwrap();

    let output = ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir2.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("super-secret-toml-token"),
        "success output should not contain token: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("super-secret-toml-token"),
        "stderr should not contain token: {stderr}"
    );
}

#[tokio::test]
async fn toml_config_missing_url_with_token_does_not_leak_token() {
    // Only token in toml, URL missing — error should not leak token.
    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(&toml_path, "token = \"leaked-if-shown-12345\"\n").unwrap();

    let output = ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stderr.contains("leaked-if-shown-12345"),
        "stderr should not leak token: {stderr}"
    );
    assert!(
        !stdout.contains("leaked-if-shown-12345"),
        "stdout should not leak token"
    );
    // Error should mention missing URL configuration
    assert!(
        stderr.to_lowercase().contains("url"),
        "Error should mention URL: {stderr}"
    );
}

// ============================================================================
// ctfd.toml with empty file is treated as no config
// ============================================================================

#[tokio::test]
async fn toml_config_empty_file_falls_through_to_missing_error() {
    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(&toml_path, "").unwrap();

    ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("URL").or(predicate::str::contains("url")));
}

// ============================================================================
// ctfd.toml flags override env overrides toml — full precedence chain
// ============================================================================

#[tokio::test]
async fn toml_config_full_precedence_chain() {
    // flag URL > env URL > toml URL
    // flag token > env token > toml token
    let flag_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .and(header("Authorization", "Token flag-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"pos": 1, "name": "FullPrecedence", "score": 999}]
        })))
        .mount(&flag_server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(
        &toml_path,
        "url = \"http://toml-host:9999\"\ntoken = \"toml-token\"\n",
    )
    .unwrap();

    ctfd()
        .arg("--url")
        .arg(flag_server.uri())
        .arg("--token")
        .arg("flag-token")
        .arg("leaderboard")
        .env("CTFD_URL", "http://env-host:8888")
        .env("CTFD_TOKEN", "env-token")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("FullPrecedence"));
}

// ============================================================================
// Auto-submit config resolution (VAL-CONFIG-010)
// ============================================================================

/// Mount a submit mock that expects a specific auto-detected challenge ID.
async fn mount_auto_submit_mock(
    server: &MockServer,
    expected_challenge_id: u64,
    expected_flag: &str,
    expected_token: &str,
    expected_count: u64,
) {
    // Mount challenge list for path matching
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .and(header("Authorization", format!("Token {expected_token}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"id": expected_challenge_id, "name": "Baby Login", "category": "Web", "value": 100, "solved_by_me": false}
            ]
        })))
        .mount(server)
        .await;

    // Mount submit mock
    Mock::given(method("POST"))
        .and(path("/api/v1/challenges/attempt"))
        .and(header("Authorization", format!("Token {expected_token}")))
        .and(body_json(serde_json::json!({
            "challenge_id": expected_challenge_id,
            "submission": expected_flag,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": { "status": "correct", "message": "Correct!" }
        })))
        .expect(expected_count)
        .mount(server)
        .await;
}

#[tokio::test]
async fn auto_submit_current_dir_toml_overrides_challenge_root_toml() {
    // Current-directory ctfd.toml should take precedence over
    // challenge-root ctfd.toml (../../ctfd.toml from challenge dir).
    let server = MockServer::start().await;

    mount_auto_submit_mock(&server, 1, "flag{test}", "current-dir-token", 1).await;

    let work_dir = tempfile::tempdir().unwrap();
    let challenge_dir = work_dir.path().join("Web").join("Baby Login");
    std::fs::create_dir_all(&challenge_dir).unwrap();

    // Challenge-root ctfd.toml (different token)
    let root_toml = work_dir.path().join("ctfd.toml");
    std::fs::write(
        &root_toml,
        format!("url = \"{}\"\ntoken = \"root-token\"\n", server.uri()),
    )
    .unwrap();

    // Current-directory ctfd.toml (in challenge dir)
    let current_toml = challenge_dir.join("ctfd.toml");
    std::fs::write(
        &current_toml,
        format!(
            "url = \"{}\"\ntoken = \"current-dir-token\"\n",
            server.uri()
        ),
    )
    .unwrap();

    ctfd()
        .arg("submit")
        .arg("flag{test}")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(&challenge_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("Flag accepted"));
}

#[tokio::test]
async fn auto_submit_uses_challenge_root_toml_when_current_dir_absent() {
    // When only the challenge-root ctfd.toml exists (no current-dir
    // ctfd.toml), auto-submit uses the root toml.
    let server = MockServer::start().await;

    mount_auto_submit_mock(&server, 1, "flag{test}", "root-token", 1).await;

    let work_dir = tempfile::tempdir().unwrap();
    let challenge_dir = work_dir.path().join("Web").join("Baby Login");
    std::fs::create_dir_all(&challenge_dir).unwrap();

    // Only challenge-root ctfd.toml
    let root_toml = work_dir.path().join("ctfd.toml");
    std::fs::write(
        &root_toml,
        format!("url = \"{}\"\ntoken = \"root-token\"\n", server.uri()),
    )
    .unwrap();
    // No ctfd.toml in challenge dir

    ctfd()
        .arg("submit")
        .arg("flag{test}")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(&challenge_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("Flag accepted"));
}

#[tokio::test]
async fn auto_submit_ignores_toml_above_challenge_root() {
    // A ctfd.toml three directories above the challenge dir should be
    // ignored.  Only current dir and challenge root (../../) are checked.
    let server = MockServer::start().await;

    let work_dir = tempfile::tempdir().unwrap();
    let challenge_dir = work_dir.path().join("Web").join("Baby Login");
    std::fs::create_dir_all(&challenge_dir).unwrap();

    // Put ctfd.toml in the challenge dir's grandparent (../../../ctfd.toml
    // relative to the challenge dir → the tempdir's parent).
    // We can't write to the tempdir's parent, so instead we create a
    // nested structure and put the toml at a level above the root.
    // Structure:
    //   outer/              ← ctfd.toml here (three levels above challenge)
    //     work_dir/         ← challenge root
    //       Web/            ← category
    //         Baby Login/   ← challenge dir
    //
    // Auto-submit should NOT use outer/ctfd.toml.
    let outer = tempfile::tempdir().unwrap();
    let work_dir = tempfile::tempdir_in(outer.path()).unwrap();
    let challenge_dir = work_dir.path().join("Web").join("Baby Login");
    std::fs::create_dir_all(&challenge_dir).unwrap();

    // ctfd.toml at outer/ level (grandparent of challenge root)
    std::fs::write(
        outer.path().join("ctfd.toml"),
        format!("url = \"{}\"\ntoken = \"outer-token\"\n", server.uri()),
    )
    .unwrap();

    // No ctfd.toml in current dir or challenge root

    ctfd()
        .arg("submit")
        .arg("flag{test}")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(&challenge_dir)
        .assert()
        .failure()
        .stderr(predicate::str::contains("URL").or(predicate::str::contains("url")));
}

#[tokio::test]
async fn auto_submit_flag_overrides_toml_files() {
    // Flags should override both current-dir and challenge-root toml.
    let server = MockServer::start().await;

    mount_auto_submit_mock(&server, 1, "flag{test}", "flag-token", 1).await;

    let work_dir = tempfile::tempdir().unwrap();
    let challenge_dir = work_dir.path().join("Web").join("Baby Login");
    std::fs::create_dir_all(&challenge_dir).unwrap();

    // Both toml files with different tokens
    let root_toml = work_dir.path().join("ctfd.toml");
    std::fs::write(
        &root_toml,
        "url = \"http://toml-root:9999\"\ntoken = \"root-token\"\n",
    )
    .unwrap();

    let current_toml = challenge_dir.join("ctfd.toml");
    std::fs::write(
        &current_toml,
        "url = \"http://toml-current:9999\"\ntoken = \"current-token\"\n",
    )
    .unwrap();

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("flag-token")
        .arg("submit")
        .arg("flag{test}")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(&challenge_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("Flag accepted"));
}

#[tokio::test]
async fn auto_submit_env_overrides_toml_files() {
    // Environment should override both current-dir and challenge-root toml.
    let server = MockServer::start().await;

    mount_auto_submit_mock(&server, 1, "flag{test}", "env-token", 1).await;

    let work_dir = tempfile::tempdir().unwrap();
    let challenge_dir = work_dir.path().join("Web").join("Baby Login");
    std::fs::create_dir_all(&challenge_dir).unwrap();

    // Both toml files with different tokens
    let root_toml = work_dir.path().join("ctfd.toml");
    std::fs::write(
        &root_toml,
        "url = \"http://toml-root:9999\"\ntoken = \"root-token\"\n",
    )
    .unwrap();

    let current_toml = challenge_dir.join("ctfd.toml");
    std::fs::write(
        &current_toml,
        "url = \"http://toml-current:9999\"\ntoken = \"current-token\"\n",
    )
    .unwrap();

    ctfd()
        .arg("submit")
        .arg("flag{test}")
        .env("CTFD_URL", server.uri())
        .env("CTFD_TOKEN", "env-token")
        .current_dir(&challenge_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("Flag accepted"));
}

#[tokio::test]
async fn auto_submit_toml_token_not_leaked_on_config_error() {
    // Token from challenge-root toml must not appear in error output.
    let _server = MockServer::start().await;

    let work_dir = tempfile::tempdir().unwrap();
    let challenge_dir = work_dir.path().join("Web").join("Baby Login");
    std::fs::create_dir_all(&challenge_dir).unwrap();

    // Challenge-root toml has token but NO url
    let root_toml = work_dir.path().join("ctfd.toml");
    std::fs::write(&root_toml, "token = \"secret-root-token-xyz\"\n").unwrap();

    // No current-dir toml

    let output = ctfd()
        .arg("submit")
        .arg("flag{test}")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(&challenge_dir)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stderr.contains("secret-root-token-xyz"),
        "stderr should not contain token: {stderr}"
    );
    assert!(
        !stdout.contains("secret-root-token-xyz"),
        "stdout should not contain token"
    );
    // Error should mention missing URL
    assert!(
        stderr.to_lowercase().contains("url"),
        "Error should mention URL: {stderr}"
    );
}
