//! rCTF (v2 API) support, against a mocked rCTF server.

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::{Value, json};
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TEAM_TOKEN: &str = "team+token/1=";
const AUTH: &str = "Bearer auth-1";

fn ctfdc(dir: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("ctfdc").unwrap();
    cmd.env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .env_remove("CTFD_PLATFORM")
        .current_dir(dir);
    cmd
}

fn good(kind: &str, data: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"kind": kind, "message": "ok", "data": data}))
}

fn bad(status: u16, kind: &str, message: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(json!({"kind": kind, "message": message}))
}

/// An rCTF v2 server: two challenges with string IDs, three teams, and you
/// ("kessoku band", #2) having solved `web-login`.
async fn mock_rctf(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/v1/integrations/client/config"))
        .respond_with(good(
            "goodClientConfig",
            json!({"ctfName": "Mock CTF", "divisions": {"open": "Open"}, "startTime": 0, "endTime": 1}),
        ))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/auth/login"))
        .and(body_json(json!({"teamToken": TEAM_TOKEN})))
        .respond_with(good("goodLogin", json!({"authToken": "auth-1"})))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/auth/login"))
        .respond_with(bad(401, "badTokenVerification", "Invalid token"))
        .with_priority(10)
        .mount(server)
        .await;

    let solve = |id: &str, name: &str, category: &str, points: i64, at: i64| json!({"id": id, "name": name, "category": category, "points": points, "solves": 3, "createdAt": at});
    Mock::given(method("GET"))
        .and(path("/api/v2/users/me"))
        .and(header("Authorization", AUTH))
        .respond_with(good(
            "goodUserSelfDataV2",
            json!({
                "id": "team-kessoku", "name": "kessoku band", "score": 300,
                "globalPlace": 2, "division": "open",
                "solves": [solve("web-login", "Login", "web", 300, 1_794_000_000_000_i64)]
            }),
        ))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v2/challs"))
        .and(header("Authorization", AUTH))
        .respond_with(good(
            "goodChallengesV2",
            json!([
                {"id": "baby-rev", "name": "Baby Rev", "category": "rev", "author": "otter",
                 "description": "Reverse it.", "points": 500, "solves": 1, "sortWeight": 0,
                 "files": [{"name": "baby_rev", "url": format!("{}/uploads/9f8e7d", server.uri()), "size": 6}]},
                {"id": "web-login", "name": "Login", "category": "web", "author": "otter",
                 "description": "Log in.", "points": 300, "solves": 3, "sortWeight": 0,
                 "files": [], "yourScore": 300}
            ]),
        ))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/uploads/9f8e7d"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"\x7fELF..".to_vec()))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v1/challs/baby-rev/submit"))
        .and(header("Authorization", AUTH))
        .and(body_json(json!({"flag": "flag{right}"})))
        .respond_with(good("goodFlag", Value::Null))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/challs/baby-rev/submit"))
        .respond_with(bad(400, "badFlag", "The flag was incorrect."))
        .with_priority(10)
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v2/leaderboard/now"))
        .and(query_param("offset", "0"))
        .respond_with(good(
            "goodLeaderboardV2",
            json!({"total": 3, "leaderboard": [
                {"id": "team-otter", "name": "otters", "score": 800, "globalPlace": 1,
                 "solves": [{"id": "baby-rev", "solveTime": 1_793_990_000_000_i64},
                            {"id": "web-login", "solveTime": 1_793_995_000_000_i64}]},
                {"id": "team-kessoku", "name": "kessoku band", "score": 300, "globalPlace": 2,
                 "solves": [{"id": "web-login", "solveTime": 1_794_000_000_000_i64}]},
                {"id": "team-late", "name": "late team", "score": 0, "globalPlace": 3, "solves": []}
            ]}),
        ))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v2/leaderboard/challs"))
        .respond_with(good(
            "goodLeaderboardChallengesV2",
            json!({"challenges": {
                "baby-rev": {"name": "Baby Rev", "category": "rev", "solves": 1, "points": 500},
                "web-login": {"name": "Login", "category": "web", "solves": 3, "points": 300}
            }}),
        ))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v2/users/team-otter"))
        .respond_with(good(
            "goodUserDataV2",
            json!({"name": "otters", "score": 800, "solves": [
                solve("baby-rev", "Baby Rev", "rev", 500, 1_793_990_000_000_i64),
                solve("web-login", "Login", "web", 300, 1_793_995_000_000_i64)
            ]}),
        ))
        .mount(server)
        .await;
}

fn write_config(dir: &std::path::Path, server: &MockServer) {
    std::fs::write(
        dir.join("ctfd.toml"),
        format!(
            "url = \"{}\"\ntoken = \"{TEAM_TOKEN}\"\nplatform = \"rctf\"\n",
            server.uri()
        ),
    )
    .unwrap();
}

#[tokio::test]
async fn init_takes_an_rctf_login_link() {
    let server = MockServer::start().await;
    mock_rctf(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let link = format!(
        "{}/login?token={}",
        server.uri(),
        urlencoding::encode(TEAM_TOKEN)
    );
    ctfdc(dir.path())
        .args(["init", &link])
        .assert()
        .success()
        .stdout(predicate::str::contains("runs rCTF"))
        .stdout(predicate::str::contains("as kessoku band"))
        .stdout(predicate::str::contains(TEAM_TOKEN).not());

    let table: toml::Table =
        toml::from_str(&std::fs::read_to_string(dir.path().join("ctfd.toml")).unwrap()).unwrap();
    assert_eq!(table["url"].as_str(), Some(server.uri().as_str()));
    assert_eq!(table["token"].as_str(), Some(TEAM_TOKEN));
    assert_eq!(table["platform"].as_str(), Some("rctf"));
}

#[tokio::test]
async fn init_detects_rctf_from_the_url() {
    let server = MockServer::start().await;
    mock_rctf(&server).await;
    let dir = tempfile::tempdir().unwrap();
    ctfdc(dir.path())
        .args(["init", &server.uri(), "--token", TEAM_TOKEN])
        .assert()
        .success()
        .stdout(predicate::str::contains("runs rCTF"));
    let config = std::fs::read_to_string(dir.path().join("ctfd.toml")).unwrap();
    assert!(config.contains("platform = \"rctf\""), "{config}");

    // A wrong team token is reported as such.
    ctfdc(dir.path())
        .args(["init", &server.uri(), "--token", "nope"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("rCTF rejected this team token"));
}

#[tokio::test]
async fn lists_downloads_and_submits() {
    let server = MockServer::start().await;
    mock_rctf(&server).await;
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), &server);

    ctfdc(dir.path())
        .args(["ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Solved 1/2 challenges, 300/800 pts",
        ))
        .stdout(predicate::str::contains("Baby Rev (500 pts)"))
        .stdout(predicate::str::contains("[✓]"));

    ctfdc(dir.path()).args(["dl"]).assert().success();
    let chal = dir.path().join("rev").join("Baby Rev");
    assert_eq!(
        std::fs::read(chal.join("baby_rev")).unwrap(),
        b"\x7fELF..",
        "saved under its rCTF file name"
    );
    let readme = std::fs::read_to_string(chal.join("README.md")).unwrap();
    assert!(readme.contains("Reverse it."), "{readme}");
    assert!(readme.contains("Author: otter"), "{readme}");

    // Attachments come from public storage: no credentials sent there.
    let requests = server.received_requests().await.unwrap();
    let upload = requests
        .iter()
        .find(|r| r.url.path() == "/uploads/9f8e7d")
        .expect("attachment fetched");
    assert!(!upload.headers.contains_key("authorization"));

    // Submitting from the challenge folder resolves the rCTF string ID.
    ctfdc(&chal)
        .args(["s", "flag{wrong}", "--wait", "0"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("rejected"));
    ctfdc(dir.path())
        .args(["s", "baby rev", "flag{right}"])
        .assert()
        .success()
        .stdout(predicate::str::contains("accepted"));
}

#[tokio::test]
async fn prints_the_rctf_scoreboard() {
    let server = MockServer::start().await;
    mock_rctf(&server).await;
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), &server);

    ctfdc(dir.path())
        .args(["lb", "--print"])
        .assert()
        .success()
        .stdout(predicate::str::contains("otters"))
        .stdout(predicate::str::contains("kessoku band"))
        .stdout(predicate::str::contains("800"));
}

#[tokio::test]
async fn the_web_board_works_on_rctf() {
    let server = MockServer::start().await;
    mock_rctf(&server).await;
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), &server);

    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("ctfdc"))
        .args(["lb", "--web", "--no-open", "--port", "0"])
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .current_dir(dir.path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let line = tokio::task::spawn_blocking(move || {
        use std::io::BufRead;
        let mut line = String::new();
        std::io::BufReader::new(stdout)
            .read_line(&mut line)
            .unwrap();
        line
    })
    .await
    .unwrap();
    let url = line
        .split_whitespace()
        .find(|w| w.starts_with("http://"))
        .unwrap_or_else(|| panic!("no URL in {line:?}"));
    let (base, key) = url.split_once("#k=").unwrap();
    let state: Value = reqwest::Client::new()
        .get(format!("{base}api/state"))
        .header("x-ctfdc-key", key)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(state["standings"][1]["name"], "kessoku band");
    assert_eq!(state["standings"][1]["is_me"], true);
    // The feed names challenges from the leaderboard's solve lists.
    let feed: Vec<&str> = state["feed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["challenge"].as_str().unwrap())
        .collect();
    assert!(feed.contains(&"Baby Rev"), "{feed:?}");
    // Analysis against the team above: Baby Rev is the catch-up target.
    assert_eq!(state["h2h"]["rival"]["name"], "otters");
    assert_eq!(state["h2h"]["catch_up"][0]["name"], "Baby Rev");
    // Baby Rev only ties the leader, who got there first.
    assert_eq!(state["plan"]["ladder"][0]["ahead"], 500);
    assert_eq!(state["plan"]["ladder"][0]["solves"], Value::Null);
    assert_eq!(state["plan"]["targets"][0]["reach"], 2);
}
