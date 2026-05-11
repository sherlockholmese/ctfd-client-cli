use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Get a command for the ctfd-client binary.
fn ctfd() -> Command {
    Command::cargo_bin("ctfd-client").expect("ctfd-client binary should exist")
}

/// Return a standard multi-category challenge fixture JSON.
fn multi_category_challenges() -> serde_json::Value {
    serde_json::json!({
        "success": true,
        "data": [
            {"id": 1, "name": "Baby Login", "value": 100, "category": "Web", "solved_by_me": false},
            {"id": 2, "name": "SQL Injection", "value": 200, "category": "Web", "solved_by_me": true},
            {"id": 3, "name": "RSA 101", "value": 150, "category": "Crypto", "solved_by_me": false},
            {"id": 4, "name": "AES Attack", "value": 300, "category": "Crypto", "solved_by_me": true},
            {"id": 5, "name": "Forensics Intro", "value": 50, "category": "Forensics", "solved_by_me": false}
        ]
    })
}

// ============================================================================
// VAL-STATUS-001 — Lists challenge categories
// ============================================================================

#[tokio::test]
async fn lists_challenge_categories() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(multi_category_challenges()))
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
        .stdout(predicate::str::contains("Web"))
        .stdout(predicate::str::contains("Crypto"))
        .stdout(predicate::str::contains("Forensics"));
}

// ============================================================================
// VAL-STATUS-002 — Lists challenge names
// ============================================================================

#[tokio::test]
async fn lists_challenge_names() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(multi_category_challenges()))
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
        .stdout(predicate::str::contains("Baby Login"))
        .stdout(predicate::str::contains("SQL Injection"))
        .stdout(predicate::str::contains("RSA 101"))
        .stdout(predicate::str::contains("AES Attack"))
        .stdout(predicate::str::contains("Forensics Intro"));
}

// ============================================================================
// VAL-STATUS-003 — Shows solved challenge status
// ============================================================================

#[tokio::test]
async fn shows_solved_challenge_status() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"id": 1, "name": "Solved Challenge", "value": 100, "category": "Misc", "solved_by_me": true}
            ]
        })))
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
        .stdout(predicate::str::contains("Solved Challenge"))
        .stdout(predicate::str::contains("[✓]"));
}

// ============================================================================
// VAL-STATUS-004 — Shows unsolved challenge status
// ============================================================================

#[tokio::test]
async fn shows_unsolved_challenge_status() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"id": 1, "name": "Unsolved Challenge", "value": 200, "category": "Misc", "solved_by_me": false}
            ]
        })))
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
        .stdout(predicate::str::contains("Unsolved Challenge"))
        .stdout(predicate::str::contains("[ ]"));
}

// ============================================================================
// VAL-STATUS-005 — Preserves category-to-challenge grouping
// ============================================================================

#[tokio::test]
async fn challenges_grouped_by_category() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(multi_category_challenges()))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("status")
        .output()
        .expect("ctfd should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Categories are sorted alphabetically (BTreeMap), so Crypto comes
    // before Forensics, which comes before Web. Within a category,
    // challenges appear in API order.
    let crypto_pos = stdout
        .find("Crypto")
        .expect("Crypto category should appear");
    let rsa_pos = stdout.find("RSA 101").expect("RSA 101 should appear");
    let aes_pos = stdout.find("AES Attack").expect("AES Attack should appear");
    let forensics_pos = stdout
        .find("Forensics")
        .expect("Forensics category should appear");
    let forensics_intro_pos = stdout
        .find("Forensics Intro")
        .expect("Forensics Intro should appear");
    let web_pos = stdout.find("Web").expect("Web category should appear");
    let baby_pos = stdout.find("Baby Login").expect("Baby Login should appear");
    let sql_pos = stdout
        .find("SQL Injection")
        .expect("SQL Injection should appear");

    // Alphabetical: Crypto < Forensics < Web
    assert!(crypto_pos < forensics_pos, "Crypto before Forensics");
    assert!(forensics_pos < web_pos, "Forensics before Web");

    // Within Crypto: API order preserved
    assert!(crypto_pos < rsa_pos, "Crypto header before RSA 101");
    assert!(rsa_pos < aes_pos, "RSA 101 before AES Attack");

    // Within Web: API order preserved
    assert!(web_pos < baby_pos, "Web header before Baby Login");
    assert!(baby_pos < sql_pos, "Baby Login before SQL Injection");

    // Within Forensics
    assert!(
        forensics_pos < forensics_intro_pos,
        "Forensics header before Forensics Intro"
    );
}

// ============================================================================
// VAL-STATUS-006 — Handles empty challenge list
// ============================================================================

#[tokio::test]
async fn empty_challenge_list_shows_message() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
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
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("No challenges available"));
}

// ============================================================================
// VAL-STATUS-007 — Requires configured CTFd URL
// ============================================================================

#[test]
fn missing_url_fails_before_request() {
    ctfd()
        .arg("status")
        .arg("--token")
        .arg("test-token")
        .env_remove("CTFD_URL")
        .assert()
        .failure()
        .stderr(predicate::str::contains("URL"));
}

// ============================================================================
// VAL-STATUS-008 — Requires API-token authentication
// ============================================================================

#[test]
fn missing_token_fails_before_request() {
    ctfd()
        .arg("status")
        .arg("--url")
        .arg("http://localhost:9999")
        .env_remove("CTFD_TOKEN")
        .assert()
        .failure()
        .stderr(predicate::str::contains("token").or(predicate::str::contains("Token")));
}

// ============================================================================
// VAL-STATUS-009 — Sends API-token auth to CTFd
// ============================================================================

#[tokio::test]
async fn sends_token_auth_header() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .and(header("Authorization", "Token my-api-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"id": 1, "name": "Test", "value": 100, "category": "Misc", "solved_by_me": false}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("my-api-token")
        .arg("status")
        .assert()
        .success();
}

// ============================================================================
// VAL-STATUS-009 (additional) — No username/password auth
// ============================================================================

#[tokio::test]
async fn uses_only_token_auth_no_password_prompt() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
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
        .arg("status")
        .output()
        .expect("ctfd should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains("Username"));
    assert!(!stdout.contains("Password"));
    assert!(!stdout.contains("Login"));
    assert!(!stderr.contains("Username"));
    assert!(!stderr.contains("Password"));
    assert!(!stderr.contains("Login"));
}

// ============================================================================
// VAL-STATUS-010 — Handles unauthorized API response
// ============================================================================

#[tokio::test]
async fn unauthorized_401_reports_auth_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("bad-token")
        .arg("status")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unauthorized"));
}

#[tokio::test]
async fn forbidden_403_reports_auth_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("bad-token")
        .arg("status")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unauthorized"));
}

// ============================================================================
// VAL-STATUS-011 — Handles CTFd server errors
// ============================================================================

#[tokio::test]
async fn server_error_500_reports_failure() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(500))
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
        .stderr(predicate::str::contains("server error").or(predicate::str::contains("500")));
}

// ============================================================================
// VAL-STATUS-012 — Handles malformed API payloads
// ============================================================================

#[tokio::test]
async fn malformed_json_reports_parse_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json at all"))
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
        .stderr(
            predicate::str::contains("parse")
                .or(predicate::str::contains("Invalid").or(predicate::str::contains("format"))),
        );
}

#[tokio::test]
async fn success_false_envelope_is_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": false,
            "errors": ["Challenges not available"]
        })))
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
        .stderr(predicate::str::contains("Challenges not available"));
}

#[tokio::test]
async fn missing_data_field_is_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
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
        .arg("status")
        .assert()
        .failure()
        .stderr(predicate::str::contains("missing").or(predicate::str::contains("data")));
}

// ============================================================================
// VAL-STATUS-013 — Status handles network failure
// ============================================================================

#[test]
fn unreachable_server_reports_network_error() {
    ctfd()
        .arg("status")
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
// VAL-STATUS-014 — Status output is non-sensitive
// ============================================================================

#[tokio::test]
async fn token_not_leaked_on_success() {
    let server = MockServer::start().await;
    let secret = "super-secret-token-do-not-leak";

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(multi_category_challenges()))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg(secret)
        .arg("status")
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
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg(secret)
        .arg("status")
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
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg(secret)
        .arg("status")
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
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_string("garbage"))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg(secret)
        .arg("status")
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
        .arg("status")
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
// VAL-STATUS-015 — Status uses only read-only challenge endpoints
// ============================================================================

#[tokio::test]
async fn only_calls_challenge_endpoint() {
    let server = MockServer::start().await;

    // Only mock the challenges endpoint — any other request would be unmatched
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(multi_category_challenges()))
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

    // wiremock verifies exactly one request was made (to /api/v1/challenges)
}

// ============================================================================
// Deterministic output
// ============================================================================

#[tokio::test]
async fn output_is_deterministic() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"id": 1, "name": "A Challenge", "value": 100, "category": "CatA", "solved_by_me": false},
                {"id": 2, "name": "B Challenge", "value": 200, "category": "CatB", "solved_by_me": true}
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
            .arg("status")
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
// Error output on stderr, not stdout
// ============================================================================

#[tokio::test]
async fn error_output_on_stderr_only() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("status")
        .output()
        .expect("ctfd should run");

    assert!(!output.status.success());
    assert!(
        !output.stderr.is_empty(),
        "stderr should contain error message"
    );
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty on failure, got: '{}'",
        String::from_utf8_lossy(&output.stdout)
    );
}

// ============================================================================
// Category ordering is stable (alphabetical)
// ============================================================================

#[tokio::test]
async fn categories_sorted_alphabetically() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"id": 1, "name": "Z Challenge", "value": 100, "category": "Zeta", "solved_by_me": false},
                {"id": 2, "name": "A Challenge", "value": 200, "category": "Alpha", "solved_by_me": true}
            ]
        })))
        .mount(&server)
        .await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("status")
        .output()
        .expect("ctfd should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Alpha should appear before Zeta in the output
    let alpha_pos = stdout.find("Alpha").expect("Alpha should appear");
    let zeta_pos = stdout.find("Zeta").expect("Zeta should appear");
    assert!(
        alpha_pos < zeta_pos,
        "Alpha category should appear before Zeta"
    );
}

// ============================================================================
// VAL-CROSS-004 — API token auth header is consistent across commands
// ============================================================================

#[tokio::test]
async fn status_uses_token_auth_header() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .and(header("Authorization", "Token my-api-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"id": 1, "name": "Test", "value": 100, "category": "Misc", "solved_by_me": false}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("my-api-token")
        .arg("status")
        .assert()
        .success();
}

// ============================================================================
// VAL-CROSS-005 — Status uses the configured mocked base URL
// ============================================================================

#[tokio::test]
async fn status_uses_configured_base_url() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"id": 1, "name": "URL Test", "value": 100, "category": "Test", "solved_by_me": false}]
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
        .stdout(predicate::str::contains("URL Test"));
}

// ============================================================================
// Trailing slash normalization works with status
// ============================================================================

#[tokio::test]
async fn trailing_slash_url_works() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"id": 1, "name": "SlashTest", "value": 1, "category": "Test", "solved_by_me": false}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let url_with_slash = format!("{}/", server.uri());

    ctfd()
        .arg("--url")
        .arg(&url_with_slash)
        .arg("--token")
        .arg("test-token")
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("SlashTest"));
}
