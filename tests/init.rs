//! `ctfd-client init`: verify and save workspace configuration.

use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn ctfd() -> Command {
    let mut cmd = Command::cargo_bin("ctfd-client").expect("ctfd-client binary should exist");
    cmd.env_remove("CTFD_URL").env_remove("CTFD_TOKEN");
    cmd
}

async fn mount_me(server: &MockServer, token: &str) {
    Mock::given(method("GET"))
        .and(path("/api/v1/users/me"))
        .and(header("Authorization", format!("Token {token}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true, "data": {"id": 3, "name": "sherlock"}
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users/me"))
        .respond_with(ResponseTemplate::new(401))
        .with_priority(10)
        .mount(server)
        .await;
}

#[tokio::test]
async fn init_verifies_and_writes_config() {
    let server = MockServer::start().await;
    mount_me(&server, "good-token").await;
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join(".git")).unwrap();

    ctfd()
        .args(["init", &format!("{}/challenges", server.uri())])
        .args(["--token", "good-token"])
        .current_dir(temp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("as sherlock"))
        .stdout(predicate::str::contains("Next steps"))
        .stdout(predicate::str::contains("good-token").not());

    let config = std::fs::read_to_string(temp.path().join("ctfd.toml")).unwrap();
    let table: toml::Table = toml::from_str(&config).unwrap();
    assert_eq!(table["url"].as_str(), Some(server.uri().as_str()));
    assert_eq!(table["token"].as_str(), Some("good-token"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(temp.path().join("ctfd.toml"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    let gitignore = std::fs::read_to_string(temp.path().join(".gitignore")).unwrap();
    assert!(gitignore.lines().any(|l| l == "ctfd.toml"));
}

#[tokio::test]
async fn init_rejects_bad_token_and_writes_nothing() {
    let server = MockServer::start().await;
    mount_me(&server, "good-token").await;
    let temp = tempfile::tempdir().unwrap();

    ctfd()
        .args(["init", &server.uri(), "--token", "bad-token"])
        .current_dir(temp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("rejected this token"))
        .stderr(predicate::str::contains("bad-token").not());
    assert!(!temp.path().join("ctfd.toml").exists());
}

#[test]
fn init_without_url_and_no_terminal_explains_usage() {
    let temp = tempfile::tempdir().unwrap();
    ctfd()
        .arg("init")
        .current_dir(temp.path())
        .write_stdin("")
        .assert()
        .failure()
        .stderr(predicate::str::contains("ctfd-client init <url>"));
}

#[tokio::test]
async fn commands_find_config_from_challenge_folder() {
    let server = MockServer::start().await;
    mount_me(&server, "good-token").await;
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .and(header("Authorization", "Token good-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": [{"id": 1, "name": "Baby Login", "category": "Web", "value": 100, "solved_by_me": false}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": {"id": 1, "name": "Baby Login", "category": "Web", "value": 100,
                     "description": "d", "solved_by_me": false}
        })))
        .mount(&server)
        .await;

    let temp = tempfile::tempdir().unwrap();
    ctfd()
        .args(["init", &server.uri(), "--token", "good-token"])
        .current_dir(temp.path())
        .assert()
        .success();

    ctfd().arg("dl").current_dir(temp.path()).assert().success();

    // From inside the challenge folder: status works and a re-download
    // lands in the workspace root rather than nesting.
    let chal = temp.path().join("Web/Baby Login");
    ctfd()
        .arg("ls")
        .current_dir(&chal)
        .assert()
        .success()
        .stdout(predicate::str::contains("#1"));
    ctfd()
        .arg("dl")
        .current_dir(&chal)
        .assert()
        .success()
        .stderr(predicate::str::contains("Up to date: "));
    assert!(!chal.join("Web").exists(), "download must not nest");
}
