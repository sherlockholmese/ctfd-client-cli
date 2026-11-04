//! Bulk-download behavior: incremental re-runs, filters, output directory,
//! parallelism, and retries.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn ctfd() -> Command {
    Command::cargo_bin("ctfdc").expect("ctfdc binary should exist")
}

fn download(server: &MockServer, dir: &Path, extra: &[&str]) -> assert_cmd::assert::Assert {
    ctfd()
        .arg("--url")
        .arg(server.uri())
        .arg("--token")
        .arg("test-token")
        .arg("download")
        .args(extra)
        .current_dir(dir)
        .assert()
}

/// Three challenges: two Web (one solved, with an attachment), one Crypto.
async fn mount_ctf(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [
                {"id": 1, "name": "Baby Login", "category": "Web", "value": 100, "solved_by_me": false},
                {"id": 2, "name": "RSA 101", "category": "Crypto", "value": 200, "solved_by_me": false},
                {"id": 3, "name": "Cookie Monster", "category": "Web", "value": 300, "solved_by_me": true}
            ]
        })))
        .mount(server)
        .await;

    for (id, name, category, files) in [
        (
            1,
            "Baby Login",
            "Web",
            vec!["/files/abc/handout.zip?token=signed"],
        ),
        (2, "RSA 101", "Crypto", vec![]),
        (3, "Cookie Monster", "Web", vec![]),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/challenges/{id}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "data": {
                    "id": id, "name": name, "category": category, "value": 100,
                    "description": "desc", "solved_by_me": id == 3, "files": files
                }
            })))
            .mount(server)
            .await;
    }
}

async fn mount_handout(server: &MockServer, expected_hits: u64) {
    Mock::given(method("GET"))
        .and(path("/files/abc/handout.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"zip-bytes".to_vec()))
        .expect(expected_hits)
        .mount(server)
        .await;
}

#[tokio::test]
async fn rerun_skips_existing_attachments_and_preserves_edits() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    mount_ctf(&server).await;
    mount_handout(&server, 1).await;

    download(&server, temp.path(), &[])
        .success()
        .stderr(predicate::str::contains("Downloaded [new]: Web/Baby Login"));

    let chal = temp.path().join("Web/Baby Login");
    std::fs::write(chal.join("solve.py"), "print('pwn')").unwrap();

    download(&server, temp.path(), &[])
        .success()
        .stdout(predicate::str::contains("0 new"))
        .stdout(predicate::str::contains("1 already present"))
        .stderr(predicate::str::contains("Up to date: Web/Baby Login"));

    assert_eq!(
        std::fs::read(chal.join("handout.zip")).unwrap(),
        b"zip-bytes"
    );
    assert_eq!(
        std::fs::read_to_string(chal.join("solve.py")).unwrap(),
        "print('pwn')"
    );
    // Handout fetched exactly once (verified by expect(1) on drop).
}

#[tokio::test]
async fn rerun_refetches_deleted_attachment() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    mount_ctf(&server).await;
    mount_handout(&server, 2).await;

    download(&server, temp.path(), &[]).success();
    std::fs::remove_file(temp.path().join("Web/Baby Login/handout.zip")).unwrap();
    download(&server, temp.path(), &[])
        .success()
        .stderr(predicate::str::contains("Updated: Web/Baby Login"));
    assert!(temp.path().join("Web/Baby Login/handout.zip").is_file());
}

#[tokio::test]
async fn force_refetches_attachments() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    mount_ctf(&server).await;
    mount_handout(&server, 2).await;

    download(&server, temp.path(), &[]).success();
    download(&server, temp.path(), &["--force"]).success();
}

#[tokio::test]
async fn metadata_records_attachments_without_signing_token() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    mount_ctf(&server).await;
    mount_handout(&server, 1).await;

    download(&server, temp.path(), &[]).success();
    let meta: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(temp.path().join("Web/Baby Login/metadata.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        meta["attachments"],
        serde_json::json!([{"url": "/files/abc/handout.zip", "file": "handout.zip"}])
    );
}

#[tokio::test]
async fn category_filter_is_case_insensitive() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    mount_ctf(&server).await;

    download(&server, temp.path(), &["-c", "crypto"])
        .success()
        .stdout(predicate::str::contains("Downloaded 1 challenge"));
    assert!(temp.path().join("Crypto/RSA 101").is_dir());
    assert!(!temp.path().join("Web").exists());
}

#[tokio::test]
async fn unsolved_skips_solved_challenges() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    mount_ctf(&server).await;
    mount_handout(&server, 1).await;

    download(&server, temp.path(), &["--unsolved"])
        .success()
        .stdout(predicate::str::contains("Downloaded 2 challenges"));
    assert!(!temp.path().join("Web/Cookie Monster").exists());
}

#[tokio::test]
async fn positional_filters_match_id_or_name() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    mount_ctf(&server).await;

    download(&server, temp.path(), &["2", "cookie"])
        .success()
        .stdout(predicate::str::contains("Downloaded 2 challenges"));
    assert!(temp.path().join("Crypto/RSA 101").is_dir());
    assert!(temp.path().join("Web/Cookie Monster").is_dir());
    assert!(!temp.path().join("Web/Baby Login").exists());
}

#[tokio::test]
async fn no_matching_challenges_fails() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    mount_ctf(&server).await;

    download(&server, temp.path(), &["-c", "pwn"])
        .failure()
        .stderr(predicate::str::contains("No challenges matched"));
}

#[tokio::test]
async fn output_directory_option() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    mount_ctf(&server).await;
    mount_handout(&server, 1).await;

    download(&server, temp.path(), &["-o", "ctf/chals", "-j", "1"]).success();
    assert!(
        temp.path()
            .join("ctf/chals/Web/Baby Login/handout.zip")
            .is_file()
    );
    assert!(temp.path().join("ctf/chals/Crypto/RSA 101").is_dir());
}

#[test]
fn rejects_zero_jobs() {
    ctfd()
        .args([
            "--url",
            "http://127.0.0.1:9",
            "--token",
            "t",
            "download",
            "-j",
            "0",
        ])
        .assert()
        .failure();
}

#[tokio::test]
async fn retries_transient_server_errors() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();

    // First request to the list endpoint is rejected with 503, then succeeds.
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    mount_ctf(&server).await;
    mount_handout(&server, 1).await;

    download(&server, temp.path(), &[])
        .success()
        .stdout(predicate::str::contains("Downloaded 3 challenges"));
}

#[tokio::test]
async fn failed_attachment_leaves_no_partial_file_and_retries_next_run() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    mount_ctf(&server).await;

    Mock::given(method("GET"))
        .and(path("/files/abc/handout.zip"))
        .respond_with(ResponseTemplate::new(404))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    mount_handout(&server, 1).await;

    download(&server, temp.path(), &[]).failure();
    let chal = temp.path().join("Web/Baby Login");
    assert!(!chal.join("handout.zip").exists());
    let leftovers: Vec<_> = std::fs::read_dir(&chal)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".part"))
        .collect();
    assert!(leftovers.is_empty(), "no .part files should remain");

    download(&server, temp.path(), &[]).success();
    assert!(chal.join("handout.zip").is_file());
}
