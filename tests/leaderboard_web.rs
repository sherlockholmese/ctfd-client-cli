//! `ctfdc lb`'s web view: the browser scoreboard server, against a mocked CTFd.

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdout, Stdio};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A running `ctfdc lb --web`, killed on drop.
struct WebBoard {
    child: Child,
    /// Kept open: ctfdc keeps printing after the link, and a closed pipe
    /// would stop it.
    _stdout: BufReader<ChildStdout>,
    /// `http://127.0.0.1:PORT/`
    base: String,
    key: String,
}

/// Read the first line ctfdc prints, handing the pipe back still open.
async fn first_line(stdout: ChildStdout) -> (String, BufReader<ChildStdout>) {
    tokio::task::spawn_blocking(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        (line, reader)
    })
    .await
    .unwrap()
}

impl Drop for WebBoard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl WebBoard {
    async fn start(server: &MockServer, extra: &[&str]) -> Self {
        let mut command = std::process::Command::new(assert_cmd::cargo::cargo_bin("ctfdc"));
        command
            .args(["--url", &server.uri(), "--token", "test-token"])
            .args(["lb", "--web", "--no-open", "--port", "0"])
            .args(extra)
            .current_dir(tempfile::tempdir().unwrap().keep())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().expect("ctfdc should start");
        let (line, stdout) = first_line(child.stdout.take().unwrap()).await;
        let url = line
            .split_whitespace()
            .find(|w| w.starts_with("http://"))
            .unwrap_or_else(|| panic!("no URL in {line:?}"))
            .to_string();
        let (base, key) = url.split_once("#k=").expect("URL carries the key");
        Self {
            child,
            _stdout: stdout,
            base: base.to_string(),
            key: key.to_string(),
        }
    }

    fn client() -> reqwest::Client {
        reqwest::Client::new()
    }

    async fn state(&self) -> Value {
        let res = Self::client()
            .get(format!("{}api/state", self.base))
            .header("x-ctfdc-key", &self.key)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
        res.json().await.unwrap()
    }

    async fn post(&self, route: &str, body: Value) -> reqwest::StatusCode {
        Self::client()
            .post(format!("{}api/{route}", self.base))
            .header("x-ctfdc-key", &self.key)
            .json(&body)
            .send()
            .await
            .unwrap()
            .status()
    }
}

async fn mock_ctfd(server: &MockServer, top_count: u32) {
    let ok = |data: Value| {
        ResponseTemplate::new(200).set_body_json(json!({"success": true, "data": data}))
    };
    Mock::given(method("GET"))
        .and(path("/api/v1/scoreboard"))
        .respond_with(ok(json!([
            {"pos": 1, "name": "Early Birds", "score": 600, "account_id": 1, "account_type": "team"},
            {"pos": 2, "name": "<img src=x onerror=alert(1)>", "score": 300, "account_id": 2, "account_type": "team"},
            {"pos": 3, "name": "Third Place", "score": 100, "account_id": 3, "account_type": "team"}
        ])))
        .mount(server)
        .await;
    // Like CTFd, `top/{count}` returns only the first `count` accounts.
    let history = [
        json!({"id": 1, "name": "Early Birds", "solves": [
            {"challenge_id": 1, "value": 500, "date": "2026-11-07T08:00:00Z"},
            {"challenge_id": 2, "value": 100, "date": "2026-11-07T10:00:00Z"}
        ]}),
        json!({"id": 2, "name": "<img src=x onerror=alert(1)>", "solves": [
            {"challenge_id": 3, "value": 300, "date": "2026-11-07T11:00:00+00:00"}
        ]}),
        json!({"id": 3, "name": "Third Place", "solves": [
            {"challenge_id": 2, "value": 100, "date": "2026-11-07T12:00:00Z"}
        ]}),
    ];
    let top: serde_json::Map<String, Value> = history
        .into_iter()
        .take(top_count as usize)
        .enumerate()
        .map(|(i, entry)| ((i + 1).to_string(), entry))
        .collect();
    Mock::given(method("GET"))
        .and(path(format!("/api/v1/scoreboard/top/{top_count}")))
        .respond_with(ok(Value::Object(top)))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ok(json!([
            {"id": 1, "name": "Warmup", "category": "Misc", "value": 500, "solved_by_me": false},
            {"id": 2, "name": "Baby Pwn", "category": "Pwn", "value": 100, "solved_by_me": false},
            {"id": 3, "name": "Login", "category": "Web", "value": 300, "solved_by_me": true}
        ])))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users/me"))
        .respond_with(ok(json!({"id": 20, "name": "alice", "team_id": 2})))
        .mount(server)
        .await;
}

#[tokio::test]
async fn serves_the_board_to_key_holders_only() {
    let server = MockServer::start().await;
    mock_ctfd(&server, 10).await;
    let board = WebBoard::start(&server, &[]).await;
    assert!(board.base.starts_with("http://127.0.0.1:"));
    assert_eq!(board.key.len(), 32);

    // The page itself has no data and loads without the key.
    let page = WebBoard::client().get(&board.base).send().await.unwrap();
    assert_eq!(page.status(), 200);
    let csp = page.headers()["content-security-policy"].to_str().unwrap();
    assert!(csp.contains("script-src 'self'"), "{csp}");
    assert!(page.text().await.unwrap().contains("vendor/echarts.min.js"));
    let chart_lib = WebBoard::client()
        .get(format!("{}vendor/echarts.min.js", board.base))
        .send()
        .await
        .unwrap();
    assert_eq!(chart_lib.status(), 200);

    // The API needs the key.
    for headers in [None, Some("0".repeat(32))] {
        let mut req = WebBoard::client().get(format!("{}api/state", board.base));
        if let Some(k) = headers {
            req = req.header("x-ctfdc-key", k);
        }
        assert_eq!(req.send().await.unwrap().status(), 401);
    }
    let refresh = WebBoard::client()
        .post(format!("{}api/refresh", board.base))
        .send()
        .await
        .unwrap();
    assert_eq!(refresh.status(), 401);

    let state = board.state().await;
    let names: Vec<&str> = state["standings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["Early Birds", "<img src=x onerror=alert(1)>", "Third Place"]
    );
    assert_eq!(state["standings"][1]["is_me"], true);
    assert_eq!(state["noun"], "team");
    assert_eq!(state["lines"].as_array().unwrap().len(), 3);
    // You are #2, so the default rival is the team above.
    assert_eq!(state["h2h"]["rival"]["name"], "Early Birds");
    assert_eq!(state["h2h"]["catch_up"][0]["name"], "Baby Pwn");
    // Plan: unsolved challenges by likely points, and what passes #1.
    let plan = &state["plan"];
    let targets: Vec<&str> = plan["targets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(targets, ["Warmup", "Baby Pwn"]);
    assert_eq!(plan["targets"][0]["reach"], 1);
    assert_eq!(plan["ladder"][0]["name"], "Early Birds");
    assert_eq!(plan["ladder"][0]["ahead"], 300);
    assert_eq!(plan["ladder"][0]["solves"], 1);
    assert_eq!(plan["focus"][0]["category"], "Misc");
    // The CTFd token never reaches the page.
    assert!(!state.to_string().contains("test-token"));

    // The event stream needs the key too, and opens with it.
    let events = WebBoard::client()
        .get(format!("{}api/events?k={}", board.base, board.key))
        .send()
        .await
        .unwrap();
    assert_eq!(events.status(), 200);
    assert_eq!(events.headers()["content-type"], "text/event-stream");
}

#[tokio::test]
async fn page_actions_change_the_rival_and_pins() {
    let server = MockServer::start().await;
    mock_ctfd(&server, 1).await;
    let board = WebBoard::start(&server, &["--top", "1"]).await;

    assert_eq!(board.post("rival", json!({"key": "id:3"})).await, 204);
    let state = board.state().await;
    assert_eq!(state["h2h"]["rival"]["name"], "Third Place");
    assert_eq!(state["standings"][2]["is_rival"], true);

    assert_eq!(board.post("rival", json!({"key": "id:99"})).await, 404);
    assert_eq!(board.post("rival", json!({"key": null})).await, 204);
    assert_eq!(board.state().await["h2h"]["rival"]["name"], "Early Birds");

    // Only #1 is graphed with --top 1; you are always graphed.
    assert_eq!(
        board.post("pin", json!({"id": 1, "pinned": true})).await,
        422
    );
    assert_eq!(
        board.post("pin", json!({"id": 2, "pinned": true})).await,
        422
    );
    assert_eq!(
        board.post("pin", json!({"id": 3, "pinned": true})).await,
        204
    );
    let state = board.state().await;
    assert_eq!(state["standings"][2]["pinned"], true);

    assert_eq!(board.post("refresh", json!({})).await, 202);
}

#[tokio::test]
async fn since_reranks_the_web_board() {
    let server = MockServer::start().await;
    mock_ctfd(&server, 3).await;
    let board = WebBoard::start(&server, &["--since", "2026-11-07T09:00:00Z"]).await;
    let state = board.state().await;
    let ranked: Vec<(i64, &str, i64)> = state["standings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["pos"].as_i64().unwrap(),
                r["name"].as_str().unwrap(),
                r["score"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        ranked,
        [
            (1, "<img src=x onerror=alert(1)>", 300),
            (2, "Early Birds", 100),
            (3, "Third Place", 100)
        ]
    );
    assert!(state["since"].is_i64());
    assert_eq!(state["host"].as_str().unwrap(), &server.uri()[7..]);
}

#[test]
fn views_cannot_be_combined() {
    let lb = |flags: &[&str]| {
        let mut cmd = Command::cargo_bin("ctfdc").unwrap();
        cmd.args(["--url", "http://localhost:9", "--token", "t", "lb"])
            .args(flags);
        cmd
    };
    for flags in [
        &["--web", "--plain"][..],
        &["--web", "--tui"],
        &["--tui", "--plain"],
        &["--tui", "--port", "8080"],
        &["--tui", "--no-open"],
        &["--plain", "--port", "8080"],
    ] {
        lb(flags)
            .assert()
            .failure()
            .stderr(predicate::str::contains("cannot be used with"));
    }
}

#[tokio::test]
async fn web_options_serve_the_page_without_web() {
    let server = MockServer::start().await;
    mock_ctfd(&server, 2).await;
    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("ctfdc"))
        .args(["--url", &server.uri(), "--token", "t"])
        .args(["lb", "--no-open"])
        .current_dir(tempfile::tempdir().unwrap().keep())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let (line, _stdout) = first_line(child.stdout.take().unwrap()).await;
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        line.starts_with("Live scoreboard: http://127.0.0.1:"),
        "{line}"
    );
}

#[tokio::test]
async fn piped_output_without_flags_is_text() {
    let server = MockServer::start().await;
    mock_ctfd(&server, 2).await;
    Command::cargo_bin("ctfdc")
        .unwrap()
        .args(["--url", &server.uri(), "--token", "t", "lb"])
        .current_dir(tempfile::tempdir().unwrap().keep())
        .assert()
        .success()
        .stdout(predicate::str::contains("Score"))
        .stdout(predicate::str::contains("Live scoreboard").not());
}

#[tokio::test]
async fn names_challenges_from_solves_when_the_list_is_hidden() {
    // After a CTF ends, CTFd stops listing challenges, but account solves
    // still name them.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/challenges"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"success": false})))
        .with_priority(1)
        .mount(&server)
        .await;
    mock_ctfd(&server, 10).await;
    let solve = |id: u64, name: &str, category: &str, value: i64| {
        json!({"challenge_id": id, "date": "2026-11-07T08:00:00Z",
               "challenge": {"id": id, "name": name, "category": category, "value": value}})
    };
    for (team, solves) in [
        (
            1,
            json!([
                solve(1, "Warmup", "Misc", 500),
                solve(2, "Baby Pwn", "Pwn", 100)
            ]),
        ),
        (2, json!([solve(3, "Login", "Web", 300)])),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/teams/{team}/solves")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"success": true, "data": solves})),
            )
            .mount(&server)
            .await;
    }
    let board = WebBoard::start(&server, &[]).await;
    let state = board.state().await;

    let mut feed: Vec<&str> = state["feed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["challenge"].as_str().unwrap())
        .collect();
    feed.sort_unstable();
    feed.dedup();
    assert_eq!(feed, ["Baby Pwn", "Login", "Warmup"]);

    let h2h = &state["h2h"];
    assert_eq!(h2h["me"]["solves"], 1);
    let catch_up: Vec<&str> = h2h["catch_up"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    // Most-solved by the top teams first.
    assert_eq!(catch_up, ["Baby Pwn", "Warmup"]);
}
