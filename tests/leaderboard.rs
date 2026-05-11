use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Get a command for the ctfd binary.
fn ctfd() -> Command {
    Command::cargo_bin("ctfd").expect("ctfd binary should exist")
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
