use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Get a command for the ctfd binary.
fn ctfd() -> Command {
    Command::cargo_bin("ctfd").expect("ctfd binary should exist")
}

/// Build a mock submit response for a given status string.
fn submit_response(status: &str, message: &str) -> serde_json::Value {
    serde_json::json!({
        "success": true,
        "data": {
            "status": status,
            "message": message
        }
    })
}

/// Mount a submit mock that expects a specific challenge_id, flag, and token.
async fn mount_submit_mock(
    server: &MockServer,
    expected_challenge_id: u64,
    expected_flag: &str,
    expected_token: &str,
    response: serde_json::Value,
    expected_count: u64,
) {
    Mock::given(method("POST"))
        .and(path("/api/v1/challenges/attempt"))
        .and(header("Authorization", format!("Token {expected_token}")))
        .and(body_json(serde_json::json!({
            "challenge_id": expected_challenge_id,
            "submission": expected_flag,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(expected_count)
        .mount(server)
        .await;
}

// ============================================================================
// VAL-SUBMIT-001: Accepted flag reports success
// ============================================================================

#[tokio::test]
async fn accepted_flag_reports_success() {
    let server = MockServer::start().await;

    mount_submit_mock(
        &server,
        1,
        "flag{correct}",
        "test-token",
        submit_response("correct", "Correct!"),
        1,
    )
    .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("1")
        .arg("flag{correct}")
        .assert()
        .success()
        .stdout(predicate::str::contains("Flag accepted"));
}

// ============================================================================
// VAL-SUBMIT-002: Rejected flag reports failure without process error
// VAL-SUBMIT-016: Rejected flag exit semantics are stable
// ============================================================================

#[tokio::test]
async fn rejected_flag_reports_failure() {
    let server = MockServer::start().await;

    mount_submit_mock(
        &server,
        1,
        "flag{wrong}",
        "test-token",
        submit_response("incorrect", "Incorrect!"),
        1,
    )
    .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("1")
        .arg("flag{wrong}")
        .assert()
        .failure()
        .stderr(predicate::str::contains("rejected"));
}

// ============================================================================
// VAL-SUBMIT-003: Already-solved response is distinct
// VAL-SUBMIT-017: Already-solved exit semantics are stable
// ============================================================================

#[tokio::test]
async fn already_solved_reports_distinctly() {
    let server = MockServer::start().await;

    mount_submit_mock(
        &server,
        1,
        "flag{test}",
        "test-token",
        submit_response("already_solved", "You already solved this."),
        1,
    )
    .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .assert()
        .success()
        .stdout(predicate::str::contains("already solved"));
}

// ============================================================================
// VAL-SUBMIT-004: Challenge argument is required
// ============================================================================

#[test]
fn challenge_argument_required() {
    ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("required")
                .or(predicate::str::contains("challenge"))
                .or(predicate::str::contains("CHALLENGE_ID")),
        );
}

// ============================================================================
// VAL-SUBMIT-005: Flag argument is required
// ============================================================================

#[test]
fn flag_argument_required() {
    ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("1")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("required")
                .or(predicate::str::contains("flag"))
                .or(predicate::str::contains("FLAG")),
        );
}

// ============================================================================
// VAL-SUBMIT-006: Challenge and flag arguments are forwarded exactly
// ============================================================================

#[tokio::test]
async fn arguments_forwarded_exactly() {
    let server = MockServer::start().await;

    mount_submit_mock(
        &server,
        42,
        "FLAG{ExAcT_Value}",
        "test-token",
        submit_response("correct", "Correct!"),
        1,
    )
    .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("42")
        .arg("FLAG{ExAcT_Value}")
        .assert()
        .success()
        .stdout(predicate::str::contains("Flag accepted"));
}

// ============================================================================
// VAL-SUBMIT-007: API-token authentication is required
// ============================================================================

#[test]
fn missing_token_fails_before_request() {
    ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .env_remove("CTFD_TOKEN")
        .assert()
        .failure()
        .stderr(predicate::str::contains("token").or(predicate::str::contains("Token")));
}

// ============================================================================
// VAL-SUBMIT-008: Submit uses API-token auth only
// ============================================================================

#[tokio::test]
async fn submit_uses_token_auth_only() {
    let server = MockServer::start().await;

    mount_submit_mock(
        &server,
        1,
        "flag{test}",
        "test-token",
        submit_response("correct", "Correct!"),
        1,
    )
    .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .output()
        .expect("ctfd should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // No username/password prompts or basic auth
    assert!(!stdout.contains("Username"));
    assert!(!stdout.contains("Password"));
    assert!(!stdout.contains("Basic "));
    assert!(!stderr.contains("Username"));
    assert!(!stderr.contains("Password"));
    assert!(!stderr.contains("Basic "));
}

// ============================================================================
// VAL-SUBMIT-009: Configured CTFd URL controls submit endpoint
// ============================================================================

#[tokio::test]
async fn configured_url_controls_endpoint() {
    let server = MockServer::start().await;

    mount_submit_mock(
        &server,
        1,
        "flag{test}",
        "test-token",
        submit_response("correct", "Correct!"),
        1,
    )
    .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .assert()
        .success();
}

// ============================================================================
// VAL-SUBMIT-010: Missing CTFd URL is reported as configuration error
// ============================================================================

#[test]
fn missing_url_reports_configuration_error() {
    ctfd()
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .env_remove("CTFD_URL")
        .assert()
        .failure()
        .stderr(predicate::str::contains("URL"));
}

// ============================================================================
// VAL-SUBMIT-011: Unauthorized API response is reported clearly
// ============================================================================

#[tokio::test]
async fn unauthorized_response_reported_clearly() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/challenges/attempt"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("Unauthorized").or(predicate::str::contains("unauthorized")),
        );
}

// ============================================================================
// VAL-SUBMIT-012: Server-side API failure is reported clearly
// ============================================================================

#[tokio::test]
async fn server_error_reported_clearly() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/challenges/attempt"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .assert()
        .failure()
        .stderr(predicate::str::contains("server").or(predicate::str::contains("500")));
}

// ============================================================================
// VAL-SUBMIT-013: Malformed API response is reported clearly
// ============================================================================

#[tokio::test]
async fn malformed_response_reported_clearly() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/challenges/attempt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("this is not json {{{"))
        .expect(1)
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("parse")
                .or(predicate::str::contains("Invalid"))
                .or(predicate::str::contains("format")),
        );
}

// ============================================================================
// VAL-SUBMIT-014: Network failure is reported clearly
// ============================================================================

#[test]
fn network_failure_reported_clearly() {
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
// VAL-SUBMIT-015: Token is never printed
// ============================================================================

#[tokio::test]
async fn token_not_leaked_accepted() {
    let server = MockServer::start().await;
    let token = "my-secret-token-12345";

    mount_submit_mock(
        &server,
        1,
        "flag{correct}",
        token,
        submit_response("correct", "Correct!"),
        1,
    )
    .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg(token)
        .arg("submit")
        .arg("1")
        .arg("flag{correct}")
        .output()
        .expect("ctfd should run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(token));
    assert!(!stderr.contains(token));
}

#[tokio::test]
async fn token_not_leaked_rejected() {
    let server = MockServer::start().await;
    let token = "my-secret-token-67890";

    mount_submit_mock(
        &server,
        1,
        "flag{wrong}",
        token,
        submit_response("incorrect", "Incorrect!"),
        1,
    )
    .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg(token)
        .arg("submit")
        .arg("1")
        .arg("flag{wrong}")
        .output()
        .expect("ctfd should run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(token));
    assert!(!stderr.contains(token));
}

#[tokio::test]
async fn token_not_leaked_on_auth_failure() {
    let server = MockServer::start().await;
    let token = "my-secret-token-leaked";

    Mock::given(method("POST"))
        .and(path("/api/v1/challenges/attempt"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg(token)
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .output()
        .expect("ctfd should run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(token));
    assert!(!stderr.contains(token));
}

#[test]
fn token_not_leaked_on_network_error() {
    let token = "network-leak-token-xyz";

    let output = ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg(token)
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .output()
        .expect("ctfd should run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(token));
    assert!(!stderr.contains(token));
}

// ============================================================================
// VAL-SUBMIT-018: Submit requires numeric challenge ID
// ============================================================================

#[test]
fn non_numeric_challenge_id_fails_before_network() {
    ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("not-a-number")
        .arg("flag{test}")
        .assert()
        .failure()
        .stderr(predicate::str::contains("numeric"));
}

// ============================================================================
// VAL-SUBMIT-019: Flags with spaces are forwarded exactly
// ============================================================================

#[tokio::test]
async fn flag_with_spaces_forwarded_exactly() {
    let server = MockServer::start().await;
    let flag_with_spaces = "flag{hello world with spaces}";

    mount_submit_mock(
        &server,
        1,
        flag_with_spaces,
        "test-token",
        submit_response("correct", "Correct!"),
        1,
    )
    .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("1")
        .arg(flag_with_spaces)
        .assert()
        .success()
        .stdout(predicate::str::contains("Flag accepted"));
}

// ============================================================================
// VAL-API-002: success=false envelope is treated as API failure
// ============================================================================

#[tokio::test]
async fn success_false_envelope_treated_as_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/challenges/attempt"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": false,
            "errors": ["You have already solved this challenge"]
        })))
        .expect(1)
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

// ============================================================================
// Output is deterministic and token never appears in help/usage errors
// ============================================================================

#[test]
fn token_not_leaked_in_argument_errors() {
    let token = "arg-error-token-abc";

    // Missing challenge ID + flag
    let output = ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg(token)
        .arg("submit")
        .output()
        .expect("ctfd should run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(token));
    assert!(!stderr.contains(token));
}
