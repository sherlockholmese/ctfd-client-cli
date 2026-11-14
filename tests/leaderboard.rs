use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Get a command for the ctfdc binary.
fn ctfd() -> Command {
    Command::cargo_bin("ctfdc").expect("ctfdc binary should exist")
}

// ============================================================================
// VAL-LEADERBOARD-001: Requires configured CTFd URL
// ============================================================================

#[test]
fn missing_url_fails_before_request() {
    ctfd()
        .arg("leaderboard")
        .arg("--token")
        .arg("test-token")
        .env_remove("CTFD_URL")
        .assert()
        .failure()
        .stderr(predicate::str::contains("URL"));
}

// ============================================================================
// VAL-LEADERBOARD-003: Requires API-token authentication
// ============================================================================

#[test]
fn missing_token_fails_before_request() {
    ctfd()
        .arg("leaderboard")
        .arg("--url")
        .arg("http://localhost:9999")
        .env_remove("CTFD_TOKEN")
        .assert()
        .failure()
        .stderr(predicate::str::contains("token").or(predicate::str::contains("Token")));
}

// ============================================================================
// VAL-LEADERBOARD-002 — Accepts configured CTFd URL
// VAL-LEADERBOARD-004 — Sends API token on leaderboard request
// ============================================================================

#[tokio::test]
async fn configured_url_and_token_reach_mock() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .and(header("Authorization", "Token my-secret-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"pos": 1, "name": "Team A", "score": 1000}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("my-secret-token")
        .arg("leaderboard")
        .assert()
        .success()
        .stdout(predicate::str::contains("Team A"));
}

// ============================================================================
// VAL-LEADERBOARD-005 — Does not support interactive or password auth
// ============================================================================

#[tokio::test]
async fn uses_only_token_auth_no_password_prompt() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": []
        })))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .output()
        .expect("ctfd should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // No credential prompts
    assert!(!stdout.contains("Username"));
    assert!(!stdout.contains("Password"));
    assert!(!stdout.contains("Login"));
    assert!(!stderr.contains("Username"));
    assert!(!stderr.contains("Password"));
    assert!(!stderr.contains("Login"));
}

// ============================================================================
// VAL-LEADERBOARD-006 — Fetches leaderboard from mocked CTFd API
// ============================================================================

#[tokio::test]
async fn only_calls_leaderboard_endpoint() {
    let server = MockServer::start().await;

    // Only mock the leaderboard endpoint — any other request would be unmatched
    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"pos": 1, "name": "Solo", "score": 42}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .assert()
        .success()
        .stdout(predicate::str::contains("Solo"));

    // wiremock verifies exactly one request was made (to scoreboard)
}

// ============================================================================
// VAL-LEADERBOARD-007 — Displays ranked users or teams
// ============================================================================

#[tokio::test]
async fn displays_all_ranked_entries() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"pos": 1, "name": "Alpha", "score": 5000},
                {"pos": 2, "name": "Beta",  "score": 4200},
                {"pos": 3, "name": "Gamma", "score": 3800}
            ]
        })))
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .assert()
        .success()
        .stdout(predicate::str::contains("Alpha"))
        .stdout(predicate::str::contains("Beta"))
        .stdout(predicate::str::contains("Gamma"))
        .stdout(predicate::str::contains("5000"))
        .stdout(predicate::str::contains("4200"))
        .stdout(predicate::str::contains("3800"));
}

// ============================================================================
// VAL-LEADERBOARD-008 — Preserves leaderboard ordering
// ============================================================================

#[tokio::test]
async fn preserves_api_ordering() {
    let server = MockServer::start().await;

    // Return entries in a specific order: scores are NOT sorted by value
    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"pos": 1, "name": "First",   "score": 100},
                {"pos": 2, "name": "Second",  "score": 999},
                {"pos": 3, "name": "Third",   "score": 50}
            ]
        })))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .output()
        .expect("ctfd should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // "First" must appear before "Second", which must appear before "Third"
    let first_pos = stdout.find("First").expect("First should be in output");
    let second_pos = stdout.find("Second").expect("Second should be in output");
    let third_pos = stdout.find("Third").expect("Third should be in output");
    assert!(first_pos < second_pos, "First should appear before Second");
    assert!(second_pos < third_pos, "Second should appear before Third");
}

// ============================================================================
// VAL-LEADERBOARD-009 — Handles empty leaderboard
// ============================================================================

#[tokio::test]
async fn empty_leaderboard_shows_message() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": []
        })))
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .assert()
        .success()
        .stdout(predicate::str::contains("No leaderboard entries"));
}

// ============================================================================
// VAL-LEADERBOARD-010 — Handles unauthorized API response
// ============================================================================

#[tokio::test]
async fn unauthorized_401_reports_auth_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("bad-token")
        .arg("leaderboard")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unauthorized"));
}

#[tokio::test]
async fn forbidden_403_reports_auth_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("bad-token")
        .arg("leaderboard")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unauthorized"));
}

// ============================================================================
// VAL-LEADERBOARD-011 — Handles server error response
// ============================================================================

#[tokio::test]
async fn server_error_500_reports_failure() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .assert()
        .failure()
        .stderr(predicate::str::contains("server error").or(predicate::str::contains("500")));
}

// ============================================================================
// VAL-LEADERBOARD-012 — Handles malformed leaderboard JSON
// ============================================================================

#[tokio::test]
async fn malformed_json_reports_parse_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json at all"))
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("parse")
                .or(predicate::str::contains("Invalid").or(predicate::str::contains("format"))),
        );
}

#[tokio::test]
async fn success_false_envelope_is_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": false,
            "errors": ["Leaderboard not available"]
        })))
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Leaderboard not available"));
}

// ============================================================================
// VAL-LEADERBOARD-013 — Handles network failure
// ============================================================================

#[test]
fn unreachable_server_reports_network_error() {
    // Use a port in the ephemeral range that should not have a listener.
    // 127.0.0.1:1 is unlikely to have a listener and will refuse connection.
    ctfd()
        .arg("leaderboard")
        .arg("--url")
        .arg("http://127.0.0.1:1")
        .arg("--token")
        .arg("test-token")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("Network")
                .or(predicate::str::contains("connection").or(predicate::str::contains("request"))),
        );
}

// ============================================================================
// VAL-LEADERBOARD-014 — Keeps output user-facing and non-sensitive
// ============================================================================

#[tokio::test]
async fn token_not_leaked_on_success() {
    let server = MockServer::start().await;
    let secret = "super-secret-token-do-not-leak";

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"pos": 1, "name": "Team", "score": 100}
            ]
        })))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg(secret)
        .arg("leaderboard")
        .output()
        .expect("ctfd should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(secret), "stdout leaked token");
    assert!(!stderr.contains(secret), "stderr leaked token");
}

#[tokio::test]
async fn token_not_leaked_on_unauthorized() {
    let server = MockServer::start().await;
    let secret = "super-secret-token-do-not-leak";

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg(secret)
        .arg("leaderboard")
        .output()
        .expect("ctfd should run");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(secret), "stdout leaked token on 401");
    assert!(!stderr.contains(secret), "stderr leaked token on 401");
}

#[tokio::test]
async fn token_not_leaked_on_server_error() {
    let server = MockServer::start().await;
    let secret = "super-secret-token-do-not-leak";

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg(secret)
        .arg("leaderboard")
        .output()
        .expect("ctfd should run");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(secret), "stdout leaked token on 500");
    assert!(!stderr.contains(secret), "stderr leaked token on 500");
}

#[tokio::test]
async fn token_not_leaked_on_malformed_json() {
    let server = MockServer::start().await;
    let secret = "super-secret-token-do-not-leak";

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_string("garbage"))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg(secret)
        .arg("leaderboard")
        .output()
        .expect("ctfd should run");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(secret), "stdout leaked token on malformed");
    assert!(!stderr.contains(secret), "stderr leaked token on malformed");
}

#[test]
fn token_not_leaked_on_network_error() {
    let secret = "super-secret-token-do-not-leak";

    let output = ctfd()
        .arg("leaderboard")
        .arg("--url")
        .arg("http://127.0.0.1:1")
        .arg("--token")
        .arg(secret)
        .output()
        .expect("ctfd should run");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stdout.contains(secret),
        "stdout leaked token on network error"
    );
    assert!(
        !stderr.contains(secret),
        "stderr leaked token on network error"
    );
}

// ============================================================================
// VAL-LEADERBOARD-015 — Uses deterministic CLI output suitable for assertions
// ============================================================================

#[tokio::test]
async fn output_is_deterministic() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"pos": 1, "name": "A", "score": 100},
                {"pos": 2, "name": "B", "score": 200}
            ]
        })))
        .mount(&server)
        .await;

    let check_output = || {
        ctfd()
            .arg("--url")
            .arg(server.uri())
            .arg("--token")
            .arg("test-token")
            .arg("leaderboard")
            .output()
            .expect("ctfd should run")
    };

    let out1 = check_output();
    let out2 = check_output();

    assert_eq!(out1.status.code(), out2.status.code());
    assert_eq!(out1.stdout, out2.stdout);
    assert_eq!(out1.stderr, out2.stderr);
}

// ============================================================================
// VAL-CROSS-004 — API token auth header is consistent across commands
// ============================================================================

#[tokio::test]
async fn leaderboard_uses_token_auth_header() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .and(header("Authorization", "Token my-api-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"pos": 1, "name": "Team", "score": 100}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("my-api-token")
        .arg("leaderboard")
        .assert()
        .success();
}

// ============================================================================
// VAL-CROSS-006 — Leaderboard uses the configured mocked base URL
// ============================================================================

#[tokio::test]
async fn leaderboard_uses_configured_base_url() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"pos": 1, "name": "URL Test", "score": 777}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .assert()
        .success()
        .stdout(predicate::str::contains("URL Test"));
}

// ============================================================================
// VAL-CONFIG-004 — Base URL trailing slash is normalized
// ============================================================================

#[tokio::test]
async fn trailing_slash_url_works() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"pos": 1, "name": "Slash", "score": 1}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    // The server URI typically has no trailing slash. Append one.
    let url_with_slash = format!("{}/", server.uri());

    ctfd()
        .arg("--url")
        .arg(&url_with_slash)
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .assert()
        .success()
        .stdout(predicate::str::contains("Slash"));
}

// ============================================================================
// Additional: VAL-API-001 — Parses standard CTFd success envelopes
// ============================================================================

#[tokio::test]
async fn parses_standard_success_envelope() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(
                r#"{"success":true,"data":[{"pos":1,"name":"EnvTest","score":999}]}"#,
            ),
        )
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .assert()
        .success()
        .stdout(predicate::str::contains("EnvTest"));
}

// ============================================================================
// Additional: Error rendered on stderr, not stdout
// ============================================================================

#[tokio::test]
async fn error_output_on_stderr_only() {
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
        .expect("ctfd should run");

    assert!(!output.status.success());
    assert!(
        !output.stderr.is_empty(),
        "stderr should contain error message"
    );
    // stdout should be empty on error (no partial output)
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty on failure, got: '{}'",
        String::from_utf8_lossy(&output.stdout)
    );
}

// ============================================================================
// Dynamic column sizing: long names and large scores remain readable
// ============================================================================

#[tokio::test]
async fn long_names_are_not_truncated() {
    let server = MockServer::start().await;
    let long_name = "ThisIsAVeryLongTeamNameThatExceedsThirtyCharacters";

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"pos": 1, "name": "Short", "score": 100},
                {"pos": 2, "name": long_name, "score": 200}
            ]
        })))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .output()
        .expect("ctfd should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // The long name must appear in full (not truncated).
    assert!(
        stdout.contains(long_name),
        "Long name should appear in full. Output:\n{stdout}"
    );
    // Shorter name below should also be present and visible.
    assert!(stdout.contains("Short"), "Short name should appear");

    // The long-name row should come after the short row (preserves API order).
    let short_pos = stdout.find("Short").unwrap();
    let long_pos = stdout.find(long_name).unwrap();
    assert!(short_pos < long_pos, "Short should appear before long name");
}

#[tokio::test]
async fn large_scores_are_not_truncated() {
    let server = MockServer::start().await;
    let huge_score = 9_999_999_999_i64; // 10 digits

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"pos": 1, "name": "A", "score": huge_score},
                {"pos": 2, "name": "B", "score": 1}
            ]
        })))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .output()
        .expect("ctfd should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let score_str = huge_score.to_string();

    // The huge score must appear in full.
    assert!(
        stdout.contains(&score_str),
        "Huge score should appear in full. Output:\n{stdout}"
    );
    // The small score should also appear.
    assert!(stdout.contains('1'), "Small score should appear");
}

#[tokio::test]
async fn column_alignment_works_with_mixed_widths() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"pos": 1, "name": "X", "score": 1},
                {"pos": 2, "name": "VeryLongNameHere", "score": 999999},
                {"pos": 100, "name": "Y", "score": 50}
            ]
        })))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .output()
        .expect("ctfd should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // All names and scores must appear.
    assert!(stdout.contains("X"), "X should appear");
    assert!(
        stdout.contains("VeryLongNameHere"),
        "Long name should appear"
    );
    assert!(stdout.contains("Y"), "Y should appear");
    assert!(stdout.contains("999999"), "Large score should appear");

    // Output must be deterministic (two runs produce identical output).
    let output2 = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("leaderboard")
        .output()
        .expect("ctfd should run");
    assert_eq!(output.stdout, output2.stdout);
}

// ============================================================================
// Plain printout: header, qualifying line, your row
// ============================================================================

async fn mount_board(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"pos": 1, "name": "Alpha", "score": 5000, "account_id": 1, "account_type": "team"},
                {"pos": 2, "name": "Beta", "score": 4200, "account_id": 2, "account_type": "team"},
                {"pos": 3, "name": "Gamma", "score": 4200, "account_id": 3, "account_type": "team"}
            ]
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true, "data": {"id": 9, "name": "me", "team_id": 3}
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn print_draws_qualifying_line_from_config() {
    let server = MockServer::start().await;
    mount_board(&server).await;
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("ctfd.toml"),
        format!("url = \"{}\"\ntoken = \"t\"\ncutoff = 2\n", server.uri()),
    )
    .unwrap();

    let out = ctfd()
        .args(["lb", "--print"])
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(temp.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines[0].contains("Team") && lines[0].contains("Behind"),
        "{stdout}"
    );
    assert!(
        lines[2].contains("Beta") && lines[2].contains("800"),
        "{stdout}"
    );
    assert!(lines[3].starts_with("── top 2 qualify"), "{stdout}");
    assert!(
        lines[4].contains("Gamma") && lines[4].contains("tied"),
        "{stdout}"
    );
    assert!(lines[4].ends_with("<- you"), "{stdout}");
}

#[tokio::test]
async fn qualify_key_still_works() {
    let server = MockServer::start().await;
    mount_board(&server).await;
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("ctfd.toml"), "qualify = 1\n").unwrap();

    ctfd()
        .args(["--url", &server.uri(), "--token", "t", "lb", "-p"])
        .current_dir(temp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("── top 1 qualify"));
}

#[tokio::test]
async fn cutoff_flag_overrides_config() {
    let server = MockServer::start().await;
    mount_board(&server).await;
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("ctfd.toml"), "qualify = 2\n").unwrap();

    ctfd()
        .args([
            "--url",
            &server.uri(),
            "--token",
            "t",
            "lb",
            "-p",
            "--cutoff",
            "1",
        ])
        .current_dir(temp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("── top 1 qualify"))
        .stdout(predicate::str::contains("top 2 qualify").not());
}

#[tokio::test]
async fn invalid_qualify_is_reported() {
    let server = MockServer::start().await;
    mount_board(&server).await;
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("ctfd.toml"), "cutoff = \"twenty\"\n").unwrap();

    ctfd()
        .args(["--url", &server.uri(), "--token", "t", "lb", "-p"])
        .current_dir(temp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid cutoff value"));
}

// ============================================================================
// `since`: ignore points scored before a given time
// ============================================================================

async fn mount_since_board(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"pos": 1, "name": "Early Birds", "score": 600, "account_id": 1, "account_type": "team"},
                {"pos": 2, "name": "Late Night", "score": 300, "account_id": 2, "account_type": "team"}
            ]
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard/top/2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": {
                "1": {"id": 1, "name": "Early Birds", "solves": [
                    {"challenge_id": 1, "value": 500, "date": "2026-11-07T08:00:00Z"},
                    {"challenge_id": 2, "value": 100, "date": "2026-11-07T10:00:00Z"}
                ]},
                "2": {"id": 2, "name": "Late Night", "solves": [
                    {"challenge_id": 3, "value": 300, "date": "2026-11-07T11:00:00+00:00"}
                ]}
            }
        })))
        .expect(1)
        .mount(server)
        .await;
}

fn rows(stdout: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter(|l| l.contains("Early Birds") || l.contains("Late Night"))
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

#[tokio::test]
async fn since_flag_ignores_earlier_points_and_reranks() {
    let server = MockServer::start().await;
    mount_since_board(&server).await;

    let output = ctfd()
        .args(["--url", &server.uri(), "--token", "t"])
        .args(["lb", "--plain", "--since", "2026-11-07T09:00:00Z"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Counting points scored since"));
    assert_eq!(
        rows(&output.stdout),
        vec!["1 Late Night 300", "2 Early Birds 100 200"]
    );
}

#[tokio::test]
async fn since_in_ctfd_toml_is_used() {
    let server = MockServer::start().await;
    mount_since_board(&server).await;
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("ctfd.toml"),
        format!(
            "url = \"{}\"\ntoken = \"t\"\nsince = 2026-11-07T09:00:00Z\n",
            server.uri()
        ),
    )
    .unwrap();

    let output = ctfd()
        .args(["lb", "--plain"])
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        rows(&output.stdout),
        vec!["1 Late Night 300", "2 Early Birds 100 200"]
    );
}

#[test]
fn invalid_since_in_ctfd_toml_is_reported() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("ctfd.toml"),
        "url = \"http://127.0.0.1:9\"\ntoken = \"t\"\nsince = \"yesterday-ish\"\n",
    )
    .unwrap();

    ctfd()
        .args(["lb", "--plain"])
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(temp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid since value"));
}

// The drawn scoreboard: what a terminal gets, forced here with --color.

#[tokio::test]
async fn color_always_draws_the_scoreboard() {
    let server = MockServer::start().await;
    mount_board(&server).await;
    let out = ctfd()
        .args(["--url", &server.uri(), "--token", "t"])
        .args(["lb", "--color", "always", "--cutoff", "1"])
        .current_dir(tempfile::tempdir().unwrap().path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("\x1b["), "coloured");
    for needle in ["ctfdc", "top 1 qualify", "█", "Score", "Behind"] {
        assert!(text.contains(needle), "missing {needle:?}\n{text}");
    }
}

#[test]
fn demo_prints_a_fictional_scoreboard_without_a_server() {
    ctfd()
        .args(["lb", "--demo", "--top", "3"])
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(tempfile::tempdir().unwrap().path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Heap Hoppers"))
        .stdout(predicate::str::contains("Null Pointers"))
        .stdout(predicate::str::contains("<- you"));
}

#[test]
fn live_views_are_separate_commands() {
    // Without a terminal the live terminal view says what to run instead.
    ctfd()
        .args(["tui", "--demo"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("ctfdc lb"))
        .stderr(predicate::str::contains("ctfdc web"));
}
