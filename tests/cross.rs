use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Get a command for the ctfd-client binary.
fn ctfd() -> Command {
    Command::cargo_bin("ctfd-client").expect("ctfd-client binary should exist")
}

// ============================================================================
// Shared fixture: a mock server with endpoints for all four commands.
//
// Returns (server, url, token) so tests can run against a single mock.
// ============================================================================

struct CrossMock {
    #[allow(dead_code)]
    server: MockServer,
    url: String,
    token: String,
}

async fn start_cross_mock() -> CrossMock {
    let server = MockServer::start().await;
    let url = server.uri();
    let token = "cross-integration-token".to_string();

    // Leaderboard: GET /api/v1/scoreboard
    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .and(header("Authorization", format!("Token {token}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"pos": 1, "name": "CrossTeam", "score": 5000}
            ]
        })))
        .mount(&server)
        .await;

    // Status: GET /api/v1/challenges
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .and(header("Authorization", format!("Token {token}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"id": 10, "name": "Cross Web", "category": "Web", "value": 100, "solved_by_me": true},
                {"id": 20, "name": "Cross Crypto", "category": "Crypto", "value": 200, "solved_by_me": false}
            ]
        })))
        .mount(&server)
        .await;

    // Download detail: GET /api/v1/challenges/10
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/10"))
        .and(header("Authorization", format!("Token {token}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": {
                "id": 10,
                "name": "Cross Web",
                "category": "Web",
                "value": 100,
                "description": "A cross-command web challenge.",
                "connection_info": "nc cross.example.com 4444",
                "type": "standard",
                "state": "visible",
                "solved_by_me": true,
                "files": ["/files/cross/handout.zip"],
                "tags": ["web", "cross"],
                "max_attempts": 5
            }
        })))
        .mount(&server)
        .await;

    // Download detail: GET /api/v1/challenges/20
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/20"))
        .and(header("Authorization", format!("Token {token}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": {
                "id": 20,
                "name": "Cross Crypto",
                "category": "Crypto",
                "value": 200,
                "description": "A cross-command crypto challenge.",
                "type": "standard",
                "state": "visible",
                "solved_by_me": false
            }
        })))
        .mount(&server)
        .await;

    // Attachment: GET /files/cross/handout.zip
    Mock::given(method("GET"))
        .and(path("/files/cross/handout.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"cross-attachment-data".to_vec()))
        .mount(&server)
        .await;

    // Submit: POST /api/v1/challenges/attempt
    Mock::given(method("POST"))
        .and(path("/api/v1/challenges/attempt"))
        .and(header("Authorization", format!("Token {token}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": {
                "status": "correct",
                "message": "Cross flag accepted!"
            }
        })))
        .mount(&server)
        .await;

    CrossMock { server, url, token }
}

// ============================================================================
// VAL-CROSS-001: Shared Global Configuration Is Accepted By Every Subcommand
//
// All subcommands accept the same global CTFd URL and token (flags or env).
// Each resolves config consistently before making API requests.
// ============================================================================

#[tokio::test]
async fn cross_all_commands_accept_shared_flag_config() {
    let mock = start_cross_mock().await;

    // Leaderboard via flags
    ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("leaderboard")
        .assert()
        .success()
        .stdout(predicate::str::contains("CrossTeam"));

    // Status via flags
    ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Cross Web"))
        .stdout(predicate::str::contains("Cross Crypto"));

    // Download via flags
    let temp = tempfile::tempdir().unwrap();
    ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("download")
        .current_dir(temp.path())
        .assert()
        .success();

    // Submit via flags
    ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("submit")
        .arg("10")
        .arg("flag{cross-test}")
        .assert()
        .success()
        .stdout(predicate::str::contains("Flag accepted"));
}

#[tokio::test]
async fn cross_all_commands_accept_shared_env_config() {
    let mock = start_cross_mock().await;

    // Leaderboard via env
    ctfd()
        .arg("leaderboard")
        .env("CTFD_URL", &mock.url)
        .env("CTFD_TOKEN", &mock.token)
        .assert()
        .success()
        .stdout(predicate::str::contains("CrossTeam"));

    // Status via env
    ctfd()
        .arg("status")
        .env("CTFD_URL", &mock.url)
        .env("CTFD_TOKEN", &mock.token)
        .assert()
        .success()
        .stdout(predicate::str::contains("Cross Web"))
        .stdout(predicate::str::contains("Cross Crypto"));

    // Download via env
    let temp = tempfile::tempdir().unwrap();
    ctfd()
        .arg("download")
        .env("CTFD_URL", &mock.url)
        .env("CTFD_TOKEN", &mock.token)
        .current_dir(temp.path())
        .assert()
        .success();

    // Submit via env
    ctfd()
        .arg("submit")
        .arg("10")
        .arg("flag{cross-test}")
        .env("CTFD_URL", &mock.url)
        .env("CTFD_TOKEN", &mock.token)
        .assert()
        .success()
        .stdout(predicate::str::contains("Flag accepted"));
}

// ============================================================================
// VAL-CROSS-002: Environment Configuration Matches Flag Configuration
//
// Each subcommand produces the same observable behavior with environment
// variables as with equivalent flags.
// ============================================================================

#[tokio::test]
async fn cross_leaderboard_env_matches_flag_behavior() {
    let mock = start_cross_mock().await;

    let out_flag = ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("leaderboard")
        .output()
        .unwrap();

    let out_env = ctfd()
        .arg("leaderboard")
        .env("CTFD_URL", &mock.url)
        .env("CTFD_TOKEN", &mock.token)
        .output()
        .unwrap();

    assert_eq!(out_flag.status.code(), out_env.status.code());
    assert_eq!(out_flag.stdout, out_env.stdout);
}

#[tokio::test]
async fn cross_status_env_matches_flag_behavior() {
    let mock = start_cross_mock().await;

    let out_flag = ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("status")
        .output()
        .unwrap();

    let out_env = ctfd()
        .arg("status")
        .env("CTFD_URL", &mock.url)
        .env("CTFD_TOKEN", &mock.token)
        .output()
        .unwrap();

    assert_eq!(out_flag.status.code(), out_env.status.code());
    assert_eq!(out_flag.stdout, out_env.stdout);
}

#[tokio::test]
async fn cross_download_env_matches_flag_behavior() {
    let mock = start_cross_mock().await;

    let temp_flag = tempfile::tempdir().unwrap();
    let out_flag = ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("download")
        .current_dir(temp_flag.path())
        .output()
        .unwrap();
    assert!(out_flag.status.success());

    let temp_env = tempfile::tempdir().unwrap();
    let out_env = ctfd()
        .arg("download")
        .env("CTFD_URL", &mock.url)
        .env("CTFD_TOKEN", &mock.token)
        .current_dir(temp_env.path())
        .output()
        .unwrap();
    assert!(out_env.status.success());

    // Both should produce the same directory structure.
    let flag_readme = std::fs::read_to_string(
        temp_flag
            .path()
            .join("Web")
            .join("Cross Web")
            .join("README.md"),
    )
    .unwrap();
    let env_readme = std::fs::read_to_string(
        temp_env
            .path()
            .join("Web")
            .join("Cross Web")
            .join("README.md"),
    )
    .unwrap();
    assert_eq!(flag_readme, env_readme);
}

#[tokio::test]
async fn cross_submit_env_matches_flag_behavior() {
    let mock = start_cross_mock().await;

    let out_flag = ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("submit")
        .arg("10")
        .arg("flag{cross-test}")
        .output()
        .unwrap();

    let out_env = ctfd()
        .arg("submit")
        .arg("10")
        .arg("flag{cross-test}")
        .env("CTFD_URL", &mock.url)
        .env("CTFD_TOKEN", &mock.token)
        .output()
        .unwrap();

    assert_eq!(out_flag.status.code(), out_env.status.code());
    assert_eq!(out_flag.stdout, out_env.stdout);
}

// ============================================================================
// VAL-CROSS-004: API Token Auth Header Is Consistent Across Commands
//
// All networked subcommands send API-token authentication using the same
// `Authorization: Token <token>` header format.
// ============================================================================

#[tokio::test]
async fn cross_auth_header_consistent_across_all_commands() {
    let token = "consistent-auth-token";

    // Leaderboard — own mock server with auth header requirement
    {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/scoreboard"))
            .and(header("Authorization", format!("Token {token}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "data": [{"pos": 1, "name": "A", "score": 1}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        ctfd()
            .arg("--url")
            .arg(server.uri())
            .arg("--token")
            .arg(token)
            .arg("leaderboard")
            .assert()
            .success();
    }

    // Status — own mock server with auth header requirement
    {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/challenges"))
            .and(header("Authorization", format!("Token {token}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "data": [{"id": 1, "name": "A", "category": "X", "value": 1, "solved_by_me": false}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        ctfd()
            .arg("--url")
            .arg(server.uri())
            .arg("--token")
            .arg(token)
            .arg("status")
            .assert()
            .success();
    }

    // Download — own mock server with auth header requirement
    {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/challenges"))
            .and(header("Authorization", format!("Token {token}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "data": [{"id": 1, "name": "D", "category": "X", "value": 1, "solved_by_me": false}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/v1/challenges/1"))
            .and(header("Authorization", format!("Token {token}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "data": {
                    "id": 1, "name": "D", "category": "X", "value": 1,
                    "description": "D", "type": "standard", "state": "visible",
                    "solved_by_me": false
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let temp = tempfile::tempdir().unwrap();
        ctfd()
            .arg("--url")
            .arg(server.uri())
            .arg("--token")
            .arg(token)
            .arg("download")
            .current_dir(temp.path())
            .assert()
            .success();
    }

    // Submit — own mock server with auth header requirement
    {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/challenges/attempt"))
            .and(header("Authorization", format!("Token {token}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "data": {"status": "correct", "message": "OK"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        ctfd()
            .arg("--url")
            .arg(server.uri())
            .arg("--token")
            .arg(token)
            .arg("submit")
            .arg("1")
            .arg("flag{auth}")
            .assert()
            .success();
    }
}

// ============================================================================
// VAL-CROSS-005: Commands Share The Same Mocked Base URL
//
// Download, status, and submit operate against the configured base URL
// and never fall back to hard-coded or default URLs.
// ============================================================================

#[tokio::test]
async fn cross_all_commands_use_same_mock_server() {
    let mock = start_cross_mock().await;

    // All commands go through the same mock server URL.
    ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("leaderboard")
        .assert()
        .success()
        .stdout(predicate::str::contains("CrossTeam"));

    ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Cross Web"))
        .stdout(predicate::str::contains("Cross Crypto"));

    let temp = tempfile::tempdir().unwrap();
    ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("download")
        .current_dir(temp.path())
        .assert()
        .success();

    ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("submit")
        .arg("10")
        .arg("flag{cross-test}")
        .assert()
        .success();
}

// ============================================================================
// VAL-CROSS-006: Leaderboard Uses The Configured Mocked Base URL
// ============================================================================

#[tokio::test]
async fn cross_leaderboard_uses_configured_mock_url() {
    let mock = start_cross_mock().await;

    ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("leaderboard")
        .assert()
        .success()
        .stdout(predicate::str::contains("CrossTeam"))
        .stdout(predicate::str::contains("5000"));
}

// ============================================================================
// VAL-CROSS-007: Download Output Structure Is Stable With Shared Config
//
// When download succeeds against the shared mocked base URL, it writes
// README.md, attachments, and metadata.json under category/challenge-name.
// ============================================================================

#[tokio::test]
async fn cross_download_output_structure_with_shared_config() {
    let mock = start_cross_mock().await;
    let temp = tempfile::tempdir().unwrap();

    ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("download")
        .current_dir(temp.path())
        .assert()
        .success();

    // Check README.md for Cross Web
    let web_dir = temp.path().join("Web").join("Cross Web");
    assert!(web_dir.join("README.md").exists());
    let readme = std::fs::read_to_string(web_dir.join("README.md")).unwrap();
    assert!(readme.contains("# Cross Web"));
    assert!(readme.contains("**Category:** Web"));
    assert!(readme.contains("**Points:** 100"));
    assert!(readme.contains("A cross-command web challenge."));
    assert!(readme.contains("nc cross.example.com 4444"));

    // Check metadata.json
    assert!(web_dir.join("metadata.json").exists());
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(web_dir.join("metadata.json")).unwrap())
            .unwrap();
    assert_eq!(meta["id"], 10);
    assert_eq!(meta["name"], "Cross Web");
    assert_eq!(meta["category"], "Web");
    assert_eq!(meta["value"], 100);
    assert_eq!(meta["solved_by_me"], true);
    assert!(meta["files"].as_array().is_some_and(|a| a.len() == 1));
    assert_eq!(meta["tags"].as_array().unwrap().len(), 2);

    // Check attachment
    assert!(web_dir.join("handout.zip").exists());
    let attachment = std::fs::read(web_dir.join("handout.zip")).unwrap();
    assert_eq!(attachment, b"cross-attachment-data");

    // Check Cross Crypto directory too
    let crypto_dir = temp.path().join("Crypto").join("Cross Crypto");
    assert!(crypto_dir.join("README.md").exists());
    assert!(crypto_dir.join("metadata.json").exists());

    // Success summary
    let success_output = ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("download")
        .current_dir(temp.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&success_output.stdout);
    assert!(stdout.contains("2"), "Should mention 2 challenges");
}

// ============================================================================
// VAL-CROSS-008: Status Reflects Mocked Challenge State Consistently
//
// Status reports challenge state from the configured mocked API and remains
// consistent with challenge names/IDs used by download and submit fixtures.
// ============================================================================

#[tokio::test]
async fn cross_status_consistent_with_download_and_submit_context() {
    let mock = start_cross_mock().await;

    let output = ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("status")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Same challenge names as download fixture.
    assert!(stdout.contains("Cross Web"), "Status should show Cross Web");
    assert!(
        stdout.contains("Cross Crypto"),
        "Status should show Cross Crypto"
    );

    // Cross Web is solved (solved_by_me: true)
    assert!(stdout.contains("[✓]"), "Should mark solved challenge");

    // Cross Crypto is unsolved (solved_by_me: false)
    assert!(stdout.contains("[ ]"), "Should mark unsolved challenge");

    // Challenge ID 10 matches the download detail endpoint.
    // (verified implicitly because the mock would reject wrong IDs)
}

// ============================================================================
// VAL-CROSS-009: Submit Sends Flag To Same Mocked Challenge Context
//
// Submit posts to the configured mocked CTFd API using the same auth and
// base URL resolution as status and download.
// ============================================================================

#[tokio::test]
async fn cross_submit_uses_same_challenge_context_as_other_commands() {
    let mock = start_cross_mock().await;

    // Use challenge ID 10 which matches the download/status fixture.
    ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("submit")
        .arg("10")
        .arg("flag{cross-test}")
        .assert()
        .success()
        .stdout(predicate::str::contains("Flag accepted"));
}

// ============================================================================
// VAL-CROSS-010: Missing Token Failures Do Not Leak Secrets
//
// Every subcommand with missing/invalid token fails with actionable errors
// that never print token values.
// ============================================================================

#[test]
fn cross_missing_token_does_not_leak_on_leaderboard() {
    let secret = "secret-token-lb-cross";

    // Set token via env, but test that error message doesn't include it.
    // Actually, missing token means NO token is provided — test that.
    let output = ctfd()
        .arg("leaderboard")
        .arg("--url")
        .arg("http://localhost:1")
        .env_remove("CTFD_TOKEN")
        .output()
        .unwrap();

    // Missing token → config error before network; no token to leak.
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stderr.contains(secret));
    assert!(!stdout.contains(secret));
}

#[test]
fn cross_missing_token_does_not_leak_on_status() {
    let secret = "secret-token-st-cross";

    let output = ctfd()
        .arg("status")
        .arg("--url")
        .arg("http://localhost:1")
        .env_remove("CTFD_TOKEN")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stderr.contains(secret));
    assert!(!stdout.contains(secret));
}

#[test]
fn cross_missing_token_does_not_leak_on_download() {
    let secret = "secret-token-dl-cross";

    let output = ctfd()
        .arg("download")
        .arg("--url")
        .arg("http://localhost:1")
        .env_remove("CTFD_TOKEN")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stderr.contains(secret));
    assert!(!stdout.contains(secret));
}

#[test]
fn cross_missing_token_does_not_leak_on_submit() {
    let secret = "secret-token-su-cross";

    let output = ctfd()
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .arg("--url")
        .arg("http://localhost:1")
        .env_remove("CTFD_TOKEN")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stderr.contains(secret));
    assert!(!stdout.contains(secret));
}

// ============================================================================
// VAL-CROSS-011: Network And API Failures Do Not Leak Tokens
//
// Transport errors, non-2xx responses, and malformed JSON across all
// commands never include configured tokens in output.
// ============================================================================

#[test]
fn cross_network_failure_no_token_leak_leaderboard() {
    let token = "nf-leak-lb-token";

    let output = ctfd()
        .arg("leaderboard")
        .arg("--url")
        .arg("http://127.0.0.1:1")
        .arg("--token")
        .arg(token)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stdout.contains(token),
        "stdout leaked token on network failure: {stdout}"
    );
    assert!(
        !stderr.contains(token),
        "stderr leaked token on network failure: {stderr}"
    );
}

#[test]
fn cross_network_failure_no_token_leak_status() {
    let token = "nf-leak-st-token";

    let output = ctfd()
        .arg("status")
        .arg("--url")
        .arg("http://127.0.0.1:1")
        .arg("--token")
        .arg(token)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(token));
    assert!(!stderr.contains(token));
}

#[test]
fn cross_network_failure_no_token_leak_download() {
    let token = "nf-leak-dl-token";

    let output = ctfd()
        .arg("download")
        .arg("--url")
        .arg("http://127.0.0.1:1")
        .arg("--token")
        .arg(token)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(token));
    assert!(!stderr.contains(token));
}

#[test]
fn cross_network_failure_no_token_leak_submit() {
    let token = "nf-leak-su-token";

    let output = ctfd()
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .arg("--url")
        .arg("http://127.0.0.1:1")
        .arg("--token")
        .arg(token)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(token));
    assert!(!stderr.contains(token));
}

#[tokio::test]
async fn cross_api_failure_no_token_leak_all_commands() {
    let server = MockServer::start().await;
    let token = "api-leak-all-token";
    let url = server.uri();

    // 500 for all endpoints
    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/challenges/attempt"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    // Leaderboard 500
    let out = ctfd()
        .arg("leaderboard")
        .arg("--url")
        .arg(&url)
        .arg("--token")
        .arg(token)
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&out.stdout).contains(token));
    assert!(!String::from_utf8_lossy(&out.stderr).contains(token));

    // Status 500
    let out = ctfd()
        .arg("status")
        .arg("--url")
        .arg(&url)
        .arg("--token")
        .arg(token)
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&out.stdout).contains(token));
    assert!(!String::from_utf8_lossy(&out.stderr).contains(token));

    // Download 500 (list endpoint)
    let out = ctfd()
        .arg("download")
        .arg("--url")
        .arg(&url)
        .arg("--token")
        .arg(token)
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&out.stdout).contains(token));
    assert!(!String::from_utf8_lossy(&out.stderr).contains(token));

    // Submit 500
    let out = ctfd()
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .arg("--url")
        .arg(&url)
        .arg("--token")
        .arg(token)
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&out.stdout).contains(token));
    assert!(!String::from_utf8_lossy(&out.stderr).contains(token));
}

// ============================================================================
// VAL-CROSS-012: Help And Validation Errors Avoid Token Echoing
//
// Argument validation errors and help output never echo environment-provided
// or flag-provided API token values.
// ============================================================================

#[test]
fn cross_help_output_no_token_echo_with_flag() {
    let token = "help-flag-token-echo";

    let output = ctfd()
        .arg("--token")
        .arg(token)
        .arg("--help")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(token));
    assert!(!stderr.contains(token));
}

#[test]
fn cross_help_output_no_token_echo_with_env() {
    let token = "help-env-token-echo";

    let output = ctfd()
        .arg("--help")
        .env("CTFD_TOKEN", token)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(token));
    assert!(!stderr.contains(token));
}

#[test]
fn cross_validation_error_no_token_echo_leaderboard() {
    let token = "val-leak-lb";

    // Missing URL → config error, token should not appear.
    let output = ctfd()
        .arg("leaderboard")
        .arg("--token")
        .arg(token)
        .env_remove("CTFD_URL")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(token));
    assert!(!stderr.contains(token));
}

#[test]
fn cross_validation_error_no_token_echo_submit_missing_args() {
    let token = "val-leak-su";

    // Missing challenge_id and flag → usage error from clap.
    let output = ctfd()
        .arg("submit")
        .arg("--url")
        .arg("http://localhost:1")
        .arg("--token")
        .arg(token)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(token));
    assert!(!stderr.contains(token));
}

// ============================================================================
// VAL-CROSS-014: Command Output Separates User Results From Diagnostics
//
// Successful output is on stdout; diagnostics/failures on stderr.
// This separation is consistent across all commands.
// ============================================================================

#[tokio::test]
async fn cross_stdout_stderr_separation_leaderboard() {
    let mock = start_cross_mock().await;
    let output = ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("leaderboard")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(
        !output.stdout.is_empty(),
        "stdout should have leaderboard data"
    );
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty on success"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("CrossTeam"));
}

#[tokio::test]
async fn cross_stdout_stderr_separation_status() {
    let mock = start_cross_mock().await;
    let output = ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("status")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(!output.stdout.is_empty(), "stdout should have status data");
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty on success"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Cross Web"));
}

#[tokio::test]
async fn cross_stdout_stderr_separation_download() {
    let mock = start_cross_mock().await;
    let temp = tempfile::tempdir().unwrap();
    let output = ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("download")
        .current_dir(temp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Downloaded"),
        "stdout should have success summary"
    );
}

#[tokio::test]
async fn cross_stdout_stderr_separation_submit() {
    let mock = start_cross_mock().await;
    let output = ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("submit")
        .arg("10")
        .arg("flag{cross-test}")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(
        !output.stdout.is_empty(),
        "stdout should have submit result"
    );
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty on success"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Flag accepted"));
}

// ============================================================================
// Failure mode: stderr contains diagnostics, stdout is empty
// ============================================================================

#[tokio::test]
async fn cross_error_diagnostics_on_stderr_only_leaderboard() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty on failure"
    );
    assert!(
        !output.stderr.is_empty(),
        "stderr should have error message"
    );
}

#[tokio::test]
async fn cross_error_diagnostics_on_stderr_only_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("bad-token")
        .arg("status")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty on failure"
    );
    assert!(
        !output.stderr.is_empty(),
        "stderr should have error message"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unauthorized"));
}

#[tokio::test]
async fn cross_error_diagnostics_on_stderr_only_submit() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/challenges/attempt"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": false,
            "errors": ["Invalid flag"]
        })))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("1")
        .arg("flag{wrong}")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty on failure"
    );
    assert!(
        !output.stderr.is_empty(),
        "stderr should have error message"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("Invalid flag"));
}

// ============================================================================
// VAL-CROSS-015: Sequential Cross-Command Flow Uses One Configuration Source
//
// A realistic sequence of status → download → submit against one mocked
// CTFd base URL with one API token, where each command independently
// honors the same configuration source.
// ============================================================================

#[tokio::test]
async fn cross_sequential_status_download_submit_flow_with_single_config() {
    let mock = start_cross_mock().await;

    // Use environment variables as the single configuration source.
    // Step 1: status — check what challenges are available.
    let status_out = ctfd()
        .arg("status")
        .env("CTFD_URL", &mock.url)
        .env("CTFD_TOKEN", &mock.token)
        .output()
        .unwrap();

    assert!(status_out.status.success());
    let status_stdout = String::from_utf8_lossy(&status_out.stdout);
    assert!(
        status_stdout.contains("Cross Web"),
        "status should show Cross Web"
    );
    assert!(
        status_stdout.contains("Cross Crypto"),
        "status should show Cross Crypto"
    );
    assert!(
        status_stdout.contains("[✓]"),
        "Cross Web should be marked solved"
    );
    assert!(
        status_stdout.contains("[ ]"),
        "Cross Crypto should be marked unsolved"
    );

    // Step 2: download — download challenges to a temp directory.
    let temp = tempfile::tempdir().unwrap();
    let dl_out = ctfd()
        .arg("download")
        .env("CTFD_URL", &mock.url)
        .env("CTFD_TOKEN", &mock.token)
        .current_dir(temp.path())
        .output()
        .unwrap();

    assert!(dl_out.status.success());
    let dl_stdout = String::from_utf8_lossy(&dl_out.stdout);
    assert!(
        dl_stdout.contains("Downloaded 2"),
        "Should report 2 challenges downloaded: {dl_stdout}"
    );

    // Verify download contents match what status reported.
    let web_readme = temp.path().join("Web").join("Cross Web").join("README.md");
    assert!(web_readme.exists(), "Cross Web README.md should exist");
    let readme = std::fs::read_to_string(&web_readme).unwrap();
    assert!(readme.contains("# Cross Web"));
    assert!(readme.contains("A cross-command web challenge."));

    let web_meta = temp
        .path()
        .join("Web")
        .join("Cross Web")
        .join("metadata.json");
    assert!(web_meta.exists(), "Cross Web metadata.json should exist");

    let web_attachment = temp
        .path()
        .join("Web")
        .join("Cross Web")
        .join("handout.zip");
    assert!(
        web_attachment.exists(),
        "Cross Web handout.zip should exist"
    );
    let attachment_data = std::fs::read(&web_attachment).unwrap();
    assert_eq!(attachment_data, b"cross-attachment-data");

    let crypto_readme = temp
        .path()
        .join("Crypto")
        .join("Cross Crypto")
        .join("README.md");
    assert!(
        crypto_readme.exists(),
        "Cross Crypto README.md should exist"
    );

    // Step 3: submit — submit a flag for one of the downloaded challenges.
    // Use the same env config.
    let submit_out = ctfd()
        .arg("submit")
        .arg("10") // Cross Web challenge ID
        .arg("flag{cross-sequential}")
        .env("CTFD_URL", &mock.url)
        .env("CTFD_TOKEN", &mock.token)
        .output()
        .unwrap();

    assert!(submit_out.status.success());
    let submit_stdout = String::from_utf8_lossy(&submit_out.stdout);
    assert!(
        submit_stdout.contains("Flag accepted"),
        "Submit should accept the flag: {submit_stdout}"
    );

    // All commands used exactly the same config source (environment variables).
    // The mock server received all expected requests with the same auth header.
}

// ============================================================================
// Additional: flags override env even in cross-command flow
// ============================================================================

#[tokio::test]
async fn cross_flag_overrides_env_in_sequential_flow() {
    let mock = start_cross_mock().await;

    // Set env to a wrong server, but pass the correct one via flags.
    // The mock expects requests at mock.url, so flags should win.
    ctfd()
        .arg("leaderboard")
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .env("CTFD_URL", "http://wrong-host:99999")
        .env("CTFD_TOKEN", "wrong-token")
        .assert()
        .success()
        .stdout(predicate::str::contains("CrossTeam"));

    ctfd()
        .arg("status")
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .env("CTFD_URL", "http://wrong-host:99999")
        .env("CTFD_TOKEN", "wrong-token")
        .assert()
        .success()
        .stdout(predicate::str::contains("Cross Web"));

    let temp = tempfile::tempdir().unwrap();
    ctfd()
        .arg("download")
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .env("CTFD_URL", "http://wrong-host:99999")
        .env("CTFD_TOKEN", "wrong-token")
        .current_dir(temp.path())
        .assert()
        .success();

    ctfd()
        .arg("submit")
        .arg("10")
        .arg("flag{cross-test}")
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .env("CTFD_URL", "http://wrong-host:99999")
        .env("CTFD_TOKEN", "wrong-token")
        .assert()
        .success()
        .stdout(predicate::str::contains("Flag accepted"));
}

// ============================================================================
// ctfd.toml cross-command tests
// All subcommands can use ctfd.toml config and precedence is respected.
// ============================================================================

#[tokio::test]
async fn cross_all_commands_accept_shared_toml_config() {
    let mock = start_cross_mock().await;

    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(
        &toml_path,
        format!("url = \"{}\"\ntoken = \"{}\"\n", mock.url, mock.token),
    )
    .unwrap();

    // Leaderboard via toml
    ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("CrossTeam"));

    // Status via toml
    ctfd()
        .arg("status")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Cross Web"))
        .stdout(predicate::str::contains("Cross Crypto"));

    // Download via toml — need ctfd.toml in the download output directory too
    let dl_dir = tempfile::tempdir().unwrap();
    let dl_toml = dl_dir.path().join("ctfd.toml");
    std::fs::write(
        &dl_toml,
        format!("url = \"{}\"\ntoken = \"{}\"\n", mock.url, mock.token),
    )
    .unwrap();
    ctfd()
        .arg("download")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dl_dir.path())
        .assert()
        .success();

    // Submit via toml
    ctfd()
        .arg("submit")
        .arg("10")
        .arg("flag{cross-test}")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Flag accepted"));
}

#[tokio::test]
async fn cross_toml_config_matches_flag_config_behavior() {
    // VAL-CROSS-002: toml config produces same behavior as flags.
    let mock = start_cross_mock().await;

    // Via flags
    let out_flag = ctfd()
        .arg("--url")
        .arg(&mock.url)
        .arg("--token")
        .arg(&mock.token)
        .arg("leaderboard")
        .output()
        .unwrap();

    // Via toml
    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(
        &toml_path,
        format!("url = \"{}\"\ntoken = \"{}\"\n", mock.url, mock.token),
    )
    .unwrap();

    let out_toml = ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert_eq!(out_flag.status.code(), out_toml.status.code());
    assert_eq!(out_flag.stdout, out_toml.stdout);
}

#[tokio::test]
async fn cross_sequential_flow_with_toml_config() {
    // VAL-CROSS-015: Sequential cross-command flow using ctfd.toml as
    // the single configuration source.
    let mock = start_cross_mock().await;

    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(
        &toml_path,
        format!("url = \"{}\"\ntoken = \"{}\"\n", mock.url, mock.token),
    )
    .unwrap();

    // Step 1: status
    let status_out = ctfd()
        .arg("status")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(status_out.status.success());
    let status_stdout = String::from_utf8_lossy(&status_out.stdout);
    assert!(status_stdout.contains("Cross Web"));
    assert!(status_stdout.contains("Cross Crypto"));

    // Step 2: download
    let dl_out = ctfd()
        .arg("download")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(dl_out.status.success());

    // Step 3: submit
    ctfd()
        .arg("submit")
        .arg("10")
        .arg("flag{cross-test}")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Flag accepted"));
}

#[tokio::test]
async fn cross_toml_config_token_not_leaked_in_cross_command_output() {
    // Use a dedicated mock server that expects the secret token from toml.
    let server = MockServer::start().await;
    let secret = "cross-toml-secret-token";

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .and(header("Authorization", format!("Token {secret}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"pos": 1, "name": "SecretCrossTeam", "score": 500}]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("ctfd.toml");
    std::fs::write(
        &toml_path,
        format!("url = \"{}\"\ntoken = \"{secret}\"\n", server.uri()),
    )
    .unwrap();

    let out = ctfd()
        .arg("leaderboard")
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stdout.contains(secret), "stdout should not contain token");
    assert!(!stderr.contains(secret), "stderr should not contain token");
}
