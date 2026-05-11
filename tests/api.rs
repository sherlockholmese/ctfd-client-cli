use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Get a command for the ctfd binary.
fn ctfd() -> Command {
    Command::cargo_bin("ctfd").expect("ctfd binary should exist")
}

// ============================================================================
// VAL-API-001: Parses standard CTFd success envelopes
//
// Each networked command parses CTFd-style `{"success":true,"data":...}`
// responses.  Verify that mocked success envelopes drive expected user-facing
// output for leaderboard, challenge list, challenge detail, and submit flows.
// ============================================================================

#[tokio::test]
async fn api_leaderboard_parses_success_envelope() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"pos": 1, "name": "API-Test-Team", "score": 9999}
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
        .stdout(predicate::str::contains("API-Test-Team"))
        .stdout(predicate::str::contains("9999"));
}

#[tokio::test]
async fn api_status_parses_success_envelope() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"id": 1, "name": "Envelope Challenge", "value": 500, "category": "API", "solved_by_me": true}
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
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("API"))
        .stdout(predicate::str::contains("Envelope Challenge"))
        .stdout(predicate::str::contains("[✓]"));
}

#[tokio::test]
async fn api_download_parses_success_envelopes() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    // Challenge list envelope
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"id": 1, "name": "EnvelopeDL", "category": "API", "value": 100, "solved_by_me": false}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    // Challenge detail envelope
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": {
                "id": 1,
                "name": "EnvelopeDL",
                "category": "API",
                "value": 100,
                "description": "Envelope download test",
                "type": "standard",
                "state": "visible",
                "solved_by_me": false
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .current_dir(temp.path())
        .assert()
        .success();

    // Verify files were written from the parsed envelope data.
    let readme = temp.path().join("API").join("EnvelopeDL").join("README.md");
    assert!(
        readme.exists(),
        "README.md should be created from envelope data"
    );
    let content = std::fs::read_to_string(&readme).unwrap();
    assert!(
        content.contains("Envelope download test"),
        "README should contain description from envelope"
    );
}

#[tokio::test]
async fn api_submit_parses_success_envelope() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/challenges/attempt"))
        .and(body_json(serde_json::json!({
            "challenge_id": 42,
            "submission": "flag{envelope-test}"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": {
                "status": "correct",
                "message": "Congratulations!"
            }
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
        .arg("42")
        .arg("flag{envelope-test}")
        .assert()
        .success()
        .stdout(predicate::str::contains("Flag accepted"));
}

// ============================================================================
// VAL-API-002: Reports standard CTFd error envelopes
//
// All networked commands treat `{"success":false,"errors":[...]}`
// as API failures even with HTTP 200.  Verify each command reports the
// error and exits non-zero without leaking the token.
// ============================================================================

#[tokio::test]
async fn api_leaderboard_reports_error_envelope() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": false,
            "errors": ["Scoreboard is disabled for this CTF"]
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
        .failure()
        .stderr(predicate::str::contains("Scoreboard is disabled"));
}

#[tokio::test]
async fn api_status_reports_error_envelope() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": false,
            "errors": ["Challenges are not yet visible"]
        })))
        .expect(1)
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("status")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Challenges are not yet visible"));
}

#[tokio::test]
async fn api_download_reports_error_envelope_on_list() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": false,
            "errors": ["Not authorized to view challenges"]
        })))
        .expect(1)
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Not authorized to view challenges",
        ));
}

#[tokio::test]
async fn api_download_reports_error_envelope_on_detail() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"id": 1, "name": "Hidden", "category": "Misc", "value": 100, "solved_by_me": false}
            ]
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": false,
            "errors": ["Challenge not available"]
        })))
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .current_dir(temp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Challenge not available"));
}

#[tokio::test]
async fn api_submit_reports_error_envelope() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/challenges/attempt"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": false,
            "errors": ["Maximum attempts exceeded"]
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
        .stderr(predicate::str::contains("Maximum attempts exceeded"));
}

// ============================================================================
// Token is never leaked in error envelope output
// ============================================================================

#[tokio::test]
async fn api_error_envelope_does_not_leak_token_leaderboard() {
    let server = MockServer::start().await;
    let secret = "secret-error-env-lb";

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": false,
            "errors": ["Something went wrong"]
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
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stdout.contains(secret),
        "stdout leaked token on error envelope"
    );
    assert!(
        !stderr.contains(secret),
        "stderr leaked token on error envelope"
    );
}

#[tokio::test]
async fn api_error_envelope_does_not_leak_token_status() {
    let server = MockServer::start().await;
    let secret = "secret-error-env-st";

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": false,
            "errors": ["Not available"]
        })))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg(secret)
        .arg("status")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(secret));
    assert!(!stderr.contains(secret));
}

#[tokio::test]
async fn api_error_envelope_does_not_leak_token_submit() {
    let server = MockServer::start().await;
    let secret = "secret-error-env-su";

    Mock::given(method("POST"))
        .and(path("/api/v1/challenges/attempt"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": false,
            "errors": ["Rate limited"]
        })))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg(secret)
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(secret));
    assert!(!stderr.contains(secret));
}

// ============================================================================
// VAL-API-003: Uses CTFd-compatible endpoint paths and methods
//
// Verify that each command uses the exact expected HTTP method and API path
// and that wiremock rejects unrelated/hard-coded paths.
// ============================================================================

#[tokio::test]
async fn api_leaderboard_uses_correct_method_and_path() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"pos": 1, "name": "T", "score": 1}]
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
        .success();
}

#[tokio::test]
async fn api_status_uses_correct_method_and_path() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"id": 1, "name": "T", "value": 1, "category": "X", "solved_by_me": false}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("status")
        .assert()
        .success();
}

#[tokio::test]
async fn api_download_uses_correct_method_and_paths() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    // GET /api/v1/challenges
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"id": 1, "name": "T", "category": "X", "value": 1, "solved_by_me": false}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    // GET /api/v1/challenges/1
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": {
                "id": 1, "name": "T", "category": "X", "value": 1,
                "description": "Desc", "type": "standard", "state": "visible",
                "solved_by_me": false
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .current_dir(temp.path())
        .assert()
        .success();
}

#[tokio::test]
async fn api_submit_uses_correct_method_and_path() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/challenges/attempt"))
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
        .arg("test-token")
        .arg("submit")
        .arg("1")
        .arg("flag{test}")
        .assert()
        .success();
}

// ============================================================================
// Verify that download detail endpoint uses the correct challenge ID in path
// ============================================================================

#[tokio::test]
async fn api_download_detail_path_uses_correct_challenge_id() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"id": 7, "name": "Seven", "category": "Misc", "value": 100, "solved_by_me": false}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    // Expect exactly /api/v1/challenges/7, not any other ID.
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": {
                "id": 7, "name": "Seven", "category": "Misc", "value": 100,
                "description": "Seven desc", "type": "standard", "state": "visible",
                "solved_by_me": false
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .current_dir(temp.path())
        .assert()
        .success();

    assert!(temp.path().join("Misc/Seven/README.md").exists());
}

// ============================================================================
// Verify submit body contains numeric challenge_id and string submission
// ============================================================================

#[tokio::test]
async fn api_submit_body_matches_ctfd_schema() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/challenges/attempt"))
        .and(body_json(serde_json::json!({
            "challenge_id": 9,
            "submission": "flag{schema-test}"
        })))
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
        .arg("test-token")
        .arg("submit")
        .arg("9")
        .arg("flag{schema-test}")
        .assert()
        .success();
}

// ============================================================================
// Verify auth header is Token type, not Bearer or Basic
// ============================================================================

#[tokio::test]
async fn api_auth_header_uses_token_prefix() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .and(header("Authorization", "Token api-token-xyz"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"id": 1, "name": "T", "value": 1, "category": "X", "solved_by_me": false}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("api-token-xyz")
        .arg("status")
        .assert()
        .success();
}

// ============================================================================
// Error envelope with multiple error messages is reported fully
// ============================================================================

#[tokio::test]
async fn api_error_envelope_with_multiple_errors() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": false,
            "errors": ["First error", "Second error"]
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
        .failure()
        .stderr(predicate::str::contains("First error"))
        .stderr(predicate::str::contains("Second error"));
}

// ============================================================================
// Challenge detail missing data field reports clear error
// ============================================================================

#[tokio::test]
async fn api_challenge_detail_missing_data_field() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"id": 1, "name": "T", "category": "X", "value": 1, "solved_by_me": false}]
        })))
        .mount(&server)
        .await;

    // Detail response with success:true but no data field
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true
        })))
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .current_dir(temp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("missing").or(predicate::str::contains("data")));
}
