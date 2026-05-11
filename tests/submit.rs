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
// VAL-SUBMIT-005: Single-arg submit in auto-detect mode fails outside
//                  challenge directory (no metadata, no path match)
// ============================================================================

#[test]
fn single_arg_submit_fails_outside_challenge_directory() {
    // `ctfd submit 1` with one arg enters auto-detect mode (flag="1").
    // Since we are not in a challenge directory and have no metadata.json,
    // it attempts to fetch the challenge list and match by path.
    // With an unreachable server, the error reports the fetch failure.
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
            predicate::str::contains("Failed to fetch challenge list")
                .or(predicate::str::contains("Cannot auto-detect"))
                .or(predicate::str::contains("challenge")),
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

// ============================================================================
// Auto-detect submit tests (VAL-SUBMIT-020 through VAL-SUBMIT-027)
// ============================================================================

/// Mount a mock for the challenge list endpoint.
async fn mount_challenge_list_mock(
    server: &MockServer,
    challenges: serde_json::Value,
    token: &str,
    expected_count: u64,
) {
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .and(header("Authorization", format!("Token {token}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": challenges,
        })))
        .expect(expected_count)
        .mount(server)
        .await;
}

// ============================================================================
// VAL-SUBMIT-020: Auto-submit reads metadata challenge ID
// ============================================================================

#[tokio::test]
async fn auto_submit_reads_metadata_challenge_id() {
    let server = MockServer::start().await;

    mount_submit_mock(
        &server,
        42,
        "flag{from-metadata}",
        "test-token",
        submit_response("correct", "Correct!"),
        1,
    )
    .await;

    // Create a temp challenge directory with metadata.json
    let dir = tempfile::tempdir().unwrap();
    let challenge_dir = dir.path().join("Web").join("Baby Login");
    std::fs::create_dir_all(&challenge_dir).unwrap();

    // Write metadata.json with a known challenge ID
    let metadata = serde_json::json!({
        "id": 42,
        "name": "Baby Login",
        "category": "Web",
        "value": 100,
        "description": "Login as admin",
        "solved_by_me": false
    });
    std::fs::write(
        challenge_dir.join("metadata.json"),
        serde_json::to_string_pretty(&metadata).unwrap(),
    )
    .unwrap();

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("flag{from-metadata}")
        .current_dir(&challenge_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("Flag accepted"));
}

// ============================================================================
// VAL-SUBMIT-021: Auto-submit falls back to path match
// ============================================================================

#[tokio::test]
async fn auto_submit_falls_back_to_path_match() {
    let server = MockServer::start().await;

    // Mount challenge list with exactly one matching challenge
    mount_challenge_list_mock(
        &server,
        serde_json::json!([
            {"id": 10, "name": "Baby Login", "category": "Web", "value": 100, "solved_by_me": false},
            {"id": 20, "name": "RSA 101", "category": "Crypto", "value": 200, "solved_by_me": false},
        ]),
        "test-token",
        1,
    )
    .await;

    // Mount submit mock for challenge ID 10 (the Web/Baby Login match)
    mount_submit_mock(
        &server,
        10,
        "flag{path-match}",
        "test-token",
        submit_response("correct", "Correct!"),
        1,
    )
    .await;

    // Create a challenge directory WITHOUT metadata.json
    let dir = tempfile::tempdir().unwrap();
    let challenge_dir = dir.path().join("Web").join("Baby Login");
    std::fs::create_dir_all(&challenge_dir).unwrap();
    // No metadata.json — forces path fallback

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("flag{path-match}")
        .current_dir(&challenge_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("Flag accepted"));
}

// ============================================================================
// VAL-SUBMIT-022: Auto-submit reports outside challenge directory
// ============================================================================

#[tokio::test]
async fn auto_submit_reports_outside_challenge_directory() {
    let server = MockServer::start().await;

    // Mount challenge list with no matching challenges for the current path
    mount_challenge_list_mock(
        &server,
        serde_json::json!([
            {"id": 1, "name": "Other", "category": "Misc", "value": 50, "solved_by_me": false},
        ]),
        "test-token",
        1,
    )
    .await;

    // Run from a directory that doesn't match any challenge
    let dir = tempfile::tempdir().unwrap();
    // The dir name will be random, so it won't match any challenge

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("flag{test}")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Cannot auto-detect"));
}

// ============================================================================
// VAL-SUBMIT-023: Auto-submit reports missing path match
// ============================================================================

#[tokio::test]
async fn auto_submit_reports_missing_path_match() {
    let server = MockServer::start().await;

    // Challenge list does NOT include a "Web/Baby Login" challenge
    mount_challenge_list_mock(
        &server,
        serde_json::json!([
            {"id": 1, "name": "Other Challenge", "category": "Misc", "value": 50, "solved_by_me": false},
        ]),
        "test-token",
        1,
    )
    .await;

    // Create a Web/Baby Login directory without metadata.json
    let dir = tempfile::tempdir().unwrap();
    let challenge_dir = dir.path().join("Web").join("Baby Login");
    std::fs::create_dir_all(&challenge_dir).unwrap();

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("flag{test}")
        .current_dir(&challenge_dir)
        .assert()
        .failure()
        .stderr(predicate::str::contains("no challenge matches"));
}

// ============================================================================
// VAL-SUBMIT-024: Auto-submit reports ambiguous path match
// ============================================================================

#[tokio::test]
async fn auto_submit_reports_ambiguous_path_match() {
    let server = MockServer::start().await;

    // Challenge list contains DUPLICATE category/name entries with different IDs
    mount_challenge_list_mock(
        &server,
        serde_json::json!([
            {"id": 10, "name": "Baby Login", "category": "Web", "value": 100, "solved_by_me": false},
            {"id": 20, "name": "Baby Login", "category": "Web", "value": 200, "solved_by_me": false},
        ]),
        "test-token",
        1,
    )
    .await;

    // Create a Web/Baby Login directory without metadata.json
    let dir = tempfile::tempdir().unwrap();
    let challenge_dir = dir.path().join("Web").join("Baby Login");
    std::fs::create_dir_all(&challenge_dir).unwrap();

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("flag{test}")
        .current_dir(&challenge_dir)
        .assert()
        .failure()
        .stderr(predicate::str::contains("multiple challenges match"));
}

// ============================================================================
// VAL-SUBMIT-025: Auto-submit reports malformed metadata
// ============================================================================

#[test]
fn auto_submit_reports_malformed_metadata() {
    // Create a challenge directory with malformed metadata.json
    let dir = tempfile::tempdir().unwrap();
    let challenge_dir = dir.path().join("Web").join("Baby Login");
    std::fs::create_dir_all(&challenge_dir).unwrap();

    // Write broken JSON as metadata
    std::fs::write(
        challenge_dir.join("metadata.json"),
        "this is not valid json {{{",
    )
    .unwrap();

    // The command should fail before network (no wiremock needed)
    ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("flag{test}")
        .current_dir(&challenge_dir)
        .assert()
        .failure()
        .stderr(predicate::str::contains("malformed").or(predicate::str::contains("metadata")));
}

#[test]
fn auto_submit_reports_metadata_missing_id() {
    // Create a challenge directory with metadata.json missing the id field
    let dir = tempfile::tempdir().unwrap();
    let challenge_dir = dir.path().join("Web").join("Baby Login");
    std::fs::create_dir_all(&challenge_dir).unwrap();

    let metadata = serde_json::json!({
        "name": "Baby Login",
        "category": "Web"
    });
    std::fs::write(
        challenge_dir.join("metadata.json"),
        serde_json::to_string(&metadata).unwrap(),
    )
    .unwrap();

    ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("flag{test}")
        .current_dir(&challenge_dir)
        .assert()
        .failure()
        .stderr(predicate::str::contains("malformed").or(predicate::str::contains("metadata")));
}

// ============================================================================
// VAL-SUBMIT-026: Auto-submit does not leak tokens
// ============================================================================

#[test]
fn auto_submit_token_not_leaked_malformed_metadata() {
    let token = "auto-leak-token-001";

    let dir = tempfile::tempdir().unwrap();
    let challenge_dir = dir.path().join("Web").join("Baby Login");
    std::fs::create_dir_all(&challenge_dir).unwrap();
    std::fs::write(challenge_dir.join("metadata.json"), "{{{ broken json {{{").unwrap();

    let output = ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg(token)
        .arg("submit")
        .arg("flag{test}")
        .current_dir(&challenge_dir)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(token));
    assert!(!stderr.contains(token));
}

#[test]
fn auto_submit_token_not_leaked_outside_directory() {
    let token = "auto-leak-token-002";

    let output = ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg(token)
        .arg("submit")
        .arg("flag{test}")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(token));
    assert!(!stderr.contains(token));
}

#[tokio::test]
async fn auto_submit_token_not_leaked_on_network_error() {
    let token = "auto-leak-token-003";

    let dir = tempfile::tempdir().unwrap();
    let challenge_dir = dir.path().join("Web").join("Baby Login");
    std::fs::create_dir_all(&challenge_dir).unwrap();

    let output = ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg(token)
        .arg("submit")
        .arg("flag{test}")
        .current_dir(&challenge_dir)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(token));
    assert!(!stderr.contains(token));
}

// ============================================================================
// VAL-SUBMIT-027: Explicit challenge ID submit remains supported
// ============================================================================

#[tokio::test]
async fn explicit_submit_still_works_without_auto_detect() {
    let server = MockServer::start().await;

    mount_submit_mock(
        &server,
        99,
        "flag{explicit}",
        "test-token",
        submit_response("correct", "Correct!"),
        1,
    )
    .await;

    // Run from a regular directory — explicit submit should NOT try
    // auto-detection. With 2 args, it goes straight to explicit mode.
    let dir = tempfile::tempdir().unwrap();

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("99")
        .arg("flag{explicit}")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Flag accepted"));
}

// ============================================================================
// Additional: Auto-submit with metadata.json present but in non-challenge dir
// ============================================================================

#[test]
fn auto_submit_with_metadata_in_non_challenge_dir_still_works() {
    // If metadata.json exists in the current directory, it should be used
    // regardless of whether the path structure is category/challenge-name.
    // This test just verifies metadata is preferred over path fallback.
    let dir = tempfile::tempdir().unwrap();

    let metadata = serde_json::json!({
        "id": 77,
        "name": "Some Challenge",
        "category": "Misc",
        "value": 50,
        "description": "Test",
        "solved_by_me": false
    });
    std::fs::write(
        dir.path().join("metadata.json"),
        serde_json::to_string_pretty(&metadata).unwrap(),
    )
    .unwrap();

    // With no server, it will fail on the network call, but the metadata
    // should have been read successfully (no metadata error).
    let output = ctfd()
        .arg("--url")
        .arg("http://localhost:9999")
        .arg("--token")
        .arg("test-token")
        .arg("submit")
        .arg("flag{test}")
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Should fail with a network/submit error, NOT a metadata error
    assert!(
        !stderr.contains("metadata"),
        "Should not report metadata error when metadata is valid: {stderr}"
    );
}
