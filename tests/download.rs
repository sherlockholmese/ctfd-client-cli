use assert_cmd::Command;
use predicates::prelude::*;
use std::path::PathBuf;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Get a command for the ctfd-client binary.
fn ctfd() -> Command {
    Command::cargo_bin("ctfd-client").expect("ctfd-client binary should exist")
}

// ============================================================================
// Helper: set up a mock server with challenge list and detail responses
// for a simple single-challenge download scenario.
// ============================================================================

fn single_challenge_list() -> serde_json::Value {
    serde_json::json!({
        "success": true,
        "data": [
            {"id": 1, "name": "Baby Login", "category": "Web", "value": 100, "solved_by_me": false}
        ]
    })
}

/// Challenge detail WITHOUT attachment files (wrapped in CTFd envelope).
fn single_challenge_detail_no_files() -> serde_json::Value {
    serde_json::json!({
        "success": true,
        "data": {
            "id": 1,
            "name": "Baby Login",
            "category": "Web",
            "value": 100,
            "description": "Login as admin.\n\nFind the password.",
            "connection_info": "nc example.com 1337",
            "type": "standard",
            "state": "visible",
            "solved_by_me": false,
            "tags": ["web", "easy"],
            "max_attempts": 10
        }
    })
}

/// Challenge detail WITH a single attachment file (wrapped in CTFd envelope).
fn single_challenge_detail_with_file() -> serde_json::Value {
    serde_json::json!({
        "success": true,
        "data": {
            "id": 1,
            "name": "Baby Login",
            "category": "Web",
            "value": 100,
            "description": "Login as admin.\n\nFind the password.",
            "connection_info": "nc example.com 1337",
            "type": "standard",
            "state": "visible",
            "solved_by_me": false,
            "files": ["/files/abc123/handout.zip"],
            "tags": ["web", "easy"],
            "max_attempts": 10
        }
    })
}

fn multi_challenge_list() -> serde_json::Value {
    serde_json::json!({
        "success": true,
        "data": [
            {"id": 1, "name": "Baby Login", "category": "Web", "value": 100, "solved_by_me": false},
            {"id": 2, "name": "RSA 101", "category": "Crypto", "value": 200, "solved_by_me": true},
            {"id": 3, "name": "Forensics Intro", "category": "Forensics", "value": 50, "solved_by_me": false}
        ]
    })
}

/// Wrap a challenge detail value in the CTFd success envelope.
fn detail_response(detail: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "success": true,
        "data": detail
    })
}

fn challenge_detail_without_files(
    id: u64,
    name: &str,
    category: &str,
    value: i64,
) -> serde_json::Value {
    detail_response(serde_json::json!({
        "id": id,
        "name": name,
        "category": category,
        "value": value,
        "description": format!("Description for {name}"),
        "type": "standard",
        "state": "visible",
        "solved_by_me": false
    }))
}

async fn mount_single_challenge_no_files(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(single_challenge_list()))
        .expect(1)
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(single_challenge_detail_no_files()))
        .expect(1)
        .mount(server)
        .await;
}

/// Mount challenge list + one detail + one attachment on the given mock server.
async fn mount_single_challenge_with_attachment(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(single_challenge_list()))
        .expect(1)
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(single_challenge_detail_with_file()))
        .expect(1)
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/files/abc123/handout.zip"))
        .respond_with(
            ResponseTemplate::new(200).set_body_bytes(b"PK\x03\x04fake-zip-content".to_vec()),
        )
        .expect(1)
        .mount(server)
        .await;
}

// ============================================================================
// VAL-DOWNLOAD-001 — Requires API Token Authentication
// ============================================================================

#[test]
fn missing_token_fails_before_request() {
    ctfd()
        .arg("download")
        .arg("--url")
        .arg("http://localhost:9999")
        .env_remove("CTFD_TOKEN")
        .assert()
        .failure()
        .stderr(predicate::str::contains("token").or(predicate::str::contains("Token")));
}

// ============================================================================
// VAL-DOWNLOAD-021 — Requires configured CTFd URL
// ============================================================================

#[test]
fn missing_url_fails_before_request() {
    ctfd()
        .arg("download")
        .arg("--token")
        .arg("test-token")
        .env_remove("CTFD_URL")
        .assert()
        .failure()
        .stderr(predicate::str::contains("URL"));
}

// ============================================================================
// VAL-DOWNLOAD-002 — Uses Configured CTFd URL
// VAL-DOWNLOAD-003 — Fetches Challenge List
// ============================================================================

#[tokio::test]
async fn sends_requests_to_configured_url() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": []
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
        .success()
        .stdout(predicate::str::contains("No challenges"));
}

// ============================================================================
// VAL-DOWNLOAD-022 — Handles empty challenge list
// ============================================================================

#[tokio::test]
async fn empty_challenge_list_prints_message_and_creates_nothing() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": []
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
        .success()
        .stdout(predicate::str::contains("No challenges"));

    // No challenge directories should be created.
    let entries: Vec<_> = std::fs::read_dir(temp.path())
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        entries.is_empty(),
        "No files/dirs should be created for empty list"
    );
}

// ============================================================================
// VAL-DOWNLOAD-005 — Writes Under Current Directory
// ============================================================================

#[tokio::test]
async fn writes_under_current_directory() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    mount_single_challenge_no_files(&server).await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .current_dir(temp.path())
        .assert()
        .success();

    // Output should exist only under the temp dir.
    let readme_path = temp.path().join("Web").join("Baby Login").join("README.md");
    assert!(
        readme_path.exists(),
        "README.md should exist under temp dir"
    );

    // Nothing should be written to the repo root (check that no Web dir exists there).
    let repo_web = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Web");
    assert!(
        !repo_web.exists(),
        "No output should be written to repo root"
    );
}

// ============================================================================
// VAL-DOWNLOAD-006 — Creates Category And Challenge Directory
// VAL-DOWNLOAD-027 — Safe names preserved exactly
// ============================================================================

#[tokio::test]
async fn creates_category_challenge_directory() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    mount_single_challenge_no_files(&server).await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .current_dir(temp.path())
        .assert()
        .success();

    let dir = temp.path().join("Web").join("Baby Login");
    assert!(
        dir.is_dir(),
        "category/challenge-name directory should exist"
    );
}

// ============================================================================
// VAL-DOWNLOAD-007 — Writes README.md
// VAL-DOWNLOAD-024 — Writes exact core filenames
// ============================================================================

#[tokio::test]
async fn writes_readme() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    mount_single_challenge_no_files(&server).await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .current_dir(temp.path())
        .assert()
        .success();

    let readme_path = temp.path().join("Web").join("Baby Login").join("README.md");
    assert!(readme_path.exists(), "README.md should exist");

    let content = std::fs::read_to_string(&readme_path).unwrap();
    assert!(
        content.contains("# Baby Login"),
        "README should contain title"
    );
    assert!(
        content.contains("**Category:** Web"),
        "README should contain category"
    );
    assert!(
        content.contains("**Points:** 100"),
        "README should contain points"
    );
    assert!(
        content.contains("## Description"),
        "README should have description section"
    );
    assert!(
        content.contains("Login as admin."),
        "README should contain description text"
    );
    assert!(
        content.contains("**Connection:** `nc example.com 1337`"),
        "README should contain connection info"
    );
}

// ============================================================================
// VAL-DOWNLOAD-008 — Writes Metadata JSON
// ============================================================================

#[tokio::test]
async fn writes_metadata_json() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    mount_single_challenge_no_files(&server).await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .current_dir(temp.path())
        .assert()
        .success();

    let meta_path = temp
        .path()
        .join("Web")
        .join("Baby Login")
        .join("metadata.json");
    assert!(meta_path.exists(), "metadata.json should exist");

    let content = std::fs::read_to_string(&meta_path).unwrap();
    let meta: serde_json::Value =
        serde_json::from_str(&content).expect("metadata.json should be valid JSON");

    assert_eq!(meta["id"], 1);
    assert_eq!(meta["name"], "Baby Login");
    assert_eq!(meta["category"], "Web");
    assert_eq!(meta["value"], 100);
    assert_eq!(meta["type"], "standard");
    assert_eq!(meta["state"], "visible");
    assert_eq!(meta["connection_info"], "nc example.com 1337");
    assert_eq!(meta["tags"].as_array().unwrap().len(), 2);
    // No files in this fixture.
    assert!(meta["files"].is_null() || meta["files"].as_array().is_some_and(|a| a.is_empty()));
    assert_eq!(meta["max_attempts"], 10);
    assert_eq!(meta["solved_by_me"], false);
    assert!(!meta["description"].as_str().unwrap().is_empty());
}

// ============================================================================
// VAL-DOWNLOAD-009 — Downloads Attachments
// ============================================================================

#[tokio::test]
async fn downloads_attachment_files() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    let attachment_bytes = b"PK\x03\x04fake-zip-content".to_vec();

    mount_single_challenge_with_attachment(&server).await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .current_dir(temp.path())
        .assert()
        .success();

    let file_path = temp
        .path()
        .join("Web")
        .join("Baby Login")
        .join("handout.zip");
    assert!(file_path.exists(), "Attachment file should exist");

    let written = std::fs::read(&file_path).unwrap();
    assert_eq!(
        written, attachment_bytes,
        "Attachment bytes should match exactly"
    );
}

// ============================================================================
// VAL-DOWNLOAD-010 — Preserves Attachment Filenames Safely
// ============================================================================

#[tokio::test]
async fn preserves_safe_attachment_filenames() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(single_challenge_list()))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail_response(
            serde_json::json!({
                "id": 1,
                "name": "Baby Login",
                "category": "Web",
                "value": 100,
                "description": "Test",
                "files": ["/files/abc/safe_file.txt"]
            }),
        )))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/files/abc/safe_file.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("content"))
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

    let file_path = temp
        .path()
        .join("Web")
        .join("Baby Login")
        .join("safe_file.txt");
    assert!(
        file_path.exists(),
        "Safe filename should be preserved exactly"
    );
}

#[tokio::test]
async fn sanitizes_unsafe_attachment_filenames() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(single_challenge_list()))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail_response(
            serde_json::json!({
                "id": 1,
                "name": "Baby Login",
                "category": "Web",
                "value": 100,
                "description": "Test",
                "files": ["/files/hash/../secret.txt"]
            }),
        )))
        .mount(&server)
        .await;

    // The HTTP client normalizes ".." in paths, so /files/hash/../secret.txt
    // becomes /files/secret.txt when actually requested.
    Mock::given(method("GET"))
        .and(path("/files/secret.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("secret"))
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

    // The filename should be sanitized. The rsplit('/') on the original URL
    // gives "secret.txt" (after the ".." and "hash"), so the file should exist.
    // Actually: "/files/hash/../secret.txt" → rsplit('/') → ["secret.txt","..","hash","files",""]
    // .next() = "secret.txt" ← correct!
    let file_path = temp
        .path()
        .join("Web")
        .join("Baby Login")
        .join("secret.txt");
    assert!(file_path.exists(), "Sanitized filename should work");

    // Should NOT write outside the challenge directory.
    let outside = temp.path().join("secret.txt");
    assert!(!outside.exists(), "Should not write outside challenge dir");
}

// ============================================================================
// VAL-DOWNLOAD-023 — Downloads all visible challenges
// ============================================================================

#[tokio::test]
async fn downloads_all_visible_challenges() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(multi_challenge_list()))
        .expect(1)
        .mount(&server)
        .await;

    // Mount detail for each challenge.
    for (id, name, cat, val) in [
        (1u64, "Baby Login", "Web", 100),
        (2, "RSA 101", "Crypto", 200),
        (3, "Forensics Intro", "Forensics", 50),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/challenges/{id}")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(challenge_detail_without_files(id, name, cat, val)),
            )
            .expect(1)
            .mount(&server)
            .await;
    }

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .current_dir(temp.path())
        .output()
        .unwrap();

    assert!(output.status.success(), "download should succeed");

    // All three directories should exist.
    assert!(temp.path().join("Web/Baby Login/README.md").exists());
    assert!(temp.path().join("Crypto/RSA 101/README.md").exists());
    assert!(
        temp.path()
            .join("Forensics/Forensics Intro/README.md")
            .exists()
    );

    // Success output should mention the count.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("3"), "Should mention 3 downloads");
}

// ============================================================================
// VAL-DOWNLOAD-004 — Fetches Challenge Details
// ============================================================================

#[tokio::test]
async fn fetches_challenge_details_for_each_challenge() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(single_challenge_list()))
        .mount(&server)
        .await;

    // Expect exactly one detail request with auth header.
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .and(header("Authorization", "Token test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(single_challenge_detail_no_files()))
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

// ============================================================================
// VAL-DOWNLOAD-011 — Sanitizes Category And Challenge Names
// ============================================================================

#[tokio::test]
async fn sanitizes_unsafe_category_and_challenge_names() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"id": 1, "name": "evil/name", "category": "../pwn", "value": 100, "solved_by_me": false}
            ]
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail_response(
            serde_json::json!({
                "id": 1,
                "name": "evil/name",
                "category": "../pwn",
                "value": 100,
                "description": "Test"
            }),
        )))
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

    // Sanitized paths should be used.
    let sanitized_dir = temp.path().join("__pwn").join("evil_name");
    assert!(
        sanitized_dir.is_dir(),
        "Should use sanitized directory names"
    );

    // Original unsafe path components should not create directories outside
    // the output tree (the parent of temp should not have our challenge data).
    let parent = temp.path().parent().unwrap();
    assert!(
        !parent.join("__pwn").exists() && !parent.join("evil_name").exists(),
        "No challenge data should be written outside the output tree"
    );
}

// ============================================================================
// VAL-DOWNLOAD-012 — Prevents Path Traversal Writes
// ============================================================================

#[tokio::test]
async fn prevents_path_traversal_writes() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    let sentinel = temp.path().join("..").join("sentinel_test_file");

    // Ensure the sentinel doesn't already exist.
    let _ = std::fs::remove_file(&sentinel);

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"id": 1, "name": "/tmp/x", "category": "web/../../escape", "value": 100, "solved_by_me": false}
            ]
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail_response(
            serde_json::json!({
                "id": 1,
                "name": "/tmp/x",
                "category": "web/../../escape",
                "value": 100,
                "description": "Test"
            }),
        )))
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

    // The sentinel file outside the temp dir should NOT be created.
    assert!(
        !sentinel.exists(),
        "Sentinel outside temp dir should not be written"
    );
}

// ============================================================================
// VAL-DOWNLOAD-028 — Unsafe names become deterministic
// ============================================================================

#[tokio::test]
async fn unsafe_names_are_deterministic() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"id": 1, "name": "evil/../../escape", "category": "../pwn", "value": 100, "solved_by_me": false}
            ]
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail_response(
            serde_json::json!({
                "id": 1,
                "name": "evil/../../escape",
                "category": "../pwn",
                "value": 100,
                "description": "Test"
            }),
        )))
        .mount(&server)
        .await;

    // Run twice and verify same output path.
    let run = || {
        let t = tempfile::tempdir().unwrap();
        ctfd()
            .arg("--url")
            .arg(server.uri())
            .arg("--token")
            .arg("test-token")
            .arg("download")
            .current_dir(t.path())
            .assert()
            .success();
        t
    };

    let t1 = run();
    let t2 = run();

    // Both should have the same sanitized directory names.
    let expected_dir = "__pwn/evil_____escape";
    assert!(t1.path().join(expected_dir).is_dir());
    assert!(t2.path().join(expected_dir).is_dir());
}

// ============================================================================
// VAL-DOWNLOAD-013 — Handles Existing Directories
// ============================================================================

#[tokio::test]
async fn handles_existing_directory() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    // Pre-create the directory with an unrelated file.
    let chal_dir = temp.path().join("Web").join("Baby Login");
    std::fs::create_dir_all(&chal_dir).unwrap();
    std::fs::write(chal_dir.join("my-notes.txt"), "personal notes").unwrap();

    mount_single_challenge_no_files(&server).await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .current_dir(temp.path())
        .assert()
        .success();

    // README.md and metadata.json should be written.
    assert!(chal_dir.join("README.md").exists());
    assert!(chal_dir.join("metadata.json").exists());

    // Unrelated file should be preserved.
    assert!(chal_dir.join("my-notes.txt").exists());
    let notes = std::fs::read_to_string(chal_dir.join("my-notes.txt")).unwrap();
    assert_eq!(notes, "personal notes");
}

// ============================================================================
// VAL-DOWNLOAD-014 — Handles Existing Files Predictably (overwrites)
// ============================================================================

#[tokio::test]
async fn overwrites_existing_generated_files() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    // Pre-create README.md with old content.
    let chal_dir = temp.path().join("Web").join("Baby Login");
    std::fs::create_dir_all(&chal_dir).unwrap();
    std::fs::write(chal_dir.join("README.md"), "# Old Content\nThis is old.").unwrap();

    mount_single_challenge_no_files(&server).await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .current_dir(temp.path())
        .assert()
        .success();

    let content = std::fs::read_to_string(chal_dir.join("README.md")).unwrap();
    assert!(
        content.contains("# Baby Login"),
        "README.md should be overwritten with new content"
    );
    assert!(
        !content.contains("Old Content"),
        "Old content should be replaced"
    );
}

// ============================================================================
// VAL-DOWNLOAD-015 — Reports Challenge List API Errors
// ============================================================================

#[tokio::test]
async fn reports_challenge_list_api_error() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

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
        .arg("download")
        .current_dir(temp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("server error").or(predicate::str::contains("500")));

    // No challenge files should be created.
    let entries: Vec<_> = std::fs::read_dir(temp.path())
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        entries.is_empty(),
        "No files should be created on list error"
    );
}

#[tokio::test]
async fn reports_unauthorized_on_challenge_list() {
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
        .arg("download")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unauthorized"));
}

#[tokio::test]
async fn reports_malformed_challenge_list() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
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
        .stderr(
            predicate::str::contains("parse")
                .or(predicate::str::contains("Invalid"))
                .or(predicate::str::contains("format")),
        );
}

// ============================================================================
// VAL-DOWNLOAD-016 — Reports Challenge Detail API Errors
// ============================================================================

#[tokio::test]
async fn reports_challenge_detail_api_error() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(single_challenge_list()))
        .mount(&server)
        .await;

    // Detail endpoint returns 500.
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(ResponseTemplate::new(500))
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
        .stderr(
            predicate::str::contains("Challenge 1")
                .and(predicate::str::contains("Baby Login").or(predicate::str::contains("detail"))),
        );
}

// ============================================================================
// VAL-DOWNLOAD-017 — Reports Attachment Download Errors
// ============================================================================

#[tokio::test]
async fn reports_attachment_download_error() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(single_challenge_list()))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(single_challenge_detail_with_file()))
        .mount(&server)
        .await;

    // Attachment returns 404.
    Mock::given(method("GET"))
        .and(path("/files/abc123/handout.zip"))
        .respond_with(ResponseTemplate::new(404))
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
        .stderr(predicate::str::contains("handout.zip").or(predicate::str::contains("attachment")));
}

// ============================================================================
// VAL-DOWNLOAD-018 — Reports Filesystem Creation Errors
// ============================================================================

#[tokio::test]
async fn reports_filesystem_creation_error() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    // Create a file where a directory should be created.
    std::fs::write(temp.path().join("Web"), "blocking file").unwrap();

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(single_challenge_list()))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(single_challenge_detail_no_files()))
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
        .stderr(predicate::str::contains("create").or(predicate::str::contains("directory")));
}

// ============================================================================
// VAL-DOWNLOAD-019 — Avoids Partial Metadata On Failure
// ============================================================================

#[tokio::test]
async fn avoids_partial_metadata_on_detail_failure() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"id": 1, "name": "Good", "category": "Web", "value": 100, "solved_by_me": false},
                {"id": 2, "name": "Bad", "category": "Web", "value": 200, "solved_by_me": false}
            ]
        })))
        .mount(&server)
        .await;

    // Good challenge detail.
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(challenge_detail_without_files(1, "Good", "Web", 100)),
        )
        .mount(&server)
        .await;

    // Bad challenge detail fails.
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/2"))
        .respond_with(ResponseTemplate::new(500))
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
        .failure();

    // Good challenge should have complete metadata.
    let good_meta = temp.path().join("Web/Good/metadata.json");
    assert!(good_meta.exists(), "Good challenge metadata should exist");
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&good_meta).unwrap()).unwrap();
    assert_eq!(meta["id"], 1);

    // Bad challenge directory should either not exist at all, or if it was
    // created by create_dir_all before the API error, its metadata should
    // not exist (since we fetch detail first, then create dir).
    // Actually we fetch detail BEFORE creating directory, so no dir
    // should be created for the bad challenge.
    let bad_dir = temp.path().join("Web").join("Bad");
    assert!(
        !bad_dir.exists() || !bad_dir.join("metadata.json").exists(),
        "Bad challenge should not have metadata.json"
    );
}

// ============================================================================
// VAL-DOWNLOAD-020 — Emits Useful Success Output
// ============================================================================

#[tokio::test]
async fn emits_useful_success_output() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    mount_single_challenge_no_files(&server).await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .current_dir(temp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Downloaded 1 challenge"),
        "Should report count: '{stdout}'"
    );
    assert!(
        stdout.contains("successfully"),
        "Should mention success: '{stdout}'"
    );
}

// ============================================================================
// VAL-DOWNLOAD-025 — Resolves relative and absolute attachment URLs
// ============================================================================

#[tokio::test]
async fn resolves_relative_attachment_urls() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(single_challenge_list()))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail_response(
            serde_json::json!({
                "id": 1,
                "name": "Baby Login",
                "category": "Web",
                "value": 100,
                "description": "Test",
                "files": ["/files/abc/relative-file.bin"]
            }),
        )))
        .mount(&server)
        .await;

    // The relative URL should be resolved against the base URL.
    Mock::given(method("GET"))
        .and(path("/files/abc/relative-file.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"relative content".to_vec()))
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

    let file = temp
        .path()
        .join("Web")
        .join("Baby Login")
        .join("relative-file.bin");
    assert!(file.exists());
    assert_eq!(std::fs::read(&file).unwrap(), b"relative content");
}

#[tokio::test]
async fn resolves_absolute_attachment_urls() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    // Use the mock server URL as an absolute attachment URL.
    let abs_url = format!("{}/absolute/hint.txt", server.uri());

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(single_challenge_list()))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail_response(
            serde_json::json!({
                "id": 1,
                "name": "Baby Login",
                "category": "Web",
                "value": 100,
                "description": "Test",
                "files": [abs_url]
            }),
        )))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/absolute/hint.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"absolute content".to_vec()))
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

    let file = temp.path().join("Web").join("Baby Login").join("hint.txt");
    assert!(file.exists());
    assert_eq!(std::fs::read(&file).unwrap(), b"absolute content");
}

// ============================================================================
// VAL-DOWNLOAD-026 — Handles sanitized filename collisions
// ============================================================================

#[tokio::test]
async fn handles_filename_collision_with_disambiguation() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(single_challenge_list()))
        .mount(&server)
        .await;

    // Challenge with two attachments that resolve to the same filename.
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail_response(
            serde_json::json!({
                "id": 1,
                "name": "Collision Test",
                "category": "Misc",
                "value": 100,
                "description": "Test",
                "files": [
                    "/files/aaa/same-name.txt",
                    "/files/bbb/same-name.txt"
                ]
            }),
        )))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/files/aaa/same-name.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("first"))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/files/bbb/same-name.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("second"))
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

    let dir = temp.path().join("Misc").join("Collision Test");
    // First file: same-name.txt
    assert!(
        dir.join("same-name.txt").exists(),
        "First file should exist"
    );
    // Second file: same-name_1.txt (disambiguated)
    assert!(
        dir.join("same-name_1.txt").exists(),
        "Second file should be disambiguated"
    );

    // Verify contents are distinct.
    assert_eq!(
        std::fs::read_to_string(dir.join("same-name.txt")).unwrap(),
        "first"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("same-name_1.txt")).unwrap(),
        "second"
    );
}

// ============================================================================
// VAL-DOWNLOAD-024 — Writes exact core filenames
// ============================================================================

#[tokio::test]
async fn writes_exact_core_filenames() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    mount_single_challenge_no_files(&server).await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .current_dir(temp.path())
        .assert()
        .success();

    let dir = temp.path().join("Web").join("Baby Login");
    assert!(
        dir.join("README.md").exists(),
        "README.md must exist with exact name"
    );
    assert!(
        dir.join("metadata.json").exists(),
        "metadata.json must exist with exact name"
    );
}

// ============================================================================
// Token secrecy checks
// ============================================================================

#[tokio::test]
async fn token_not_leaked_on_success() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    let secret = "super-secret-token-dl";

    mount_single_challenge_no_files(&server).await;

    let output = ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg(secret)
        .arg("download")
        .current_dir(temp.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(secret), "stdout leaked token");
    assert!(!stderr.contains(secret), "stderr leaked token");
}

#[tokio::test]
async fn token_not_leaked_on_api_error() {
    let server = MockServer::start().await;
    let secret = "super-secret-token-dl-err";

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
        .arg("download")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(secret), "stdout leaked token on error");
    assert!(!stderr.contains(secret), "stderr leaked token on error");
}

#[tokio::test]
async fn token_not_leaked_in_metadata_json() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    let secret = "meta-leak-token-test";

    mount_single_challenge_no_files(&server).await;

    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg(secret)
        .arg("download")
        .current_dir(temp.path())
        .assert()
        .success();

    let meta_path = temp
        .path()
        .join("Web")
        .join("Baby Login")
        .join("metadata.json");
    let meta_content = std::fs::read_to_string(&meta_path).unwrap();
    assert!(
        !meta_content.contains(secret),
        "metadata.json must not contain the API token"
    );
}
