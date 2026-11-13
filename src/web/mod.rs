//! Live scoreboard in the browser (`ctfdc lb`, the default view).
//!
//! Serves a single page plus a small JSON API on a local address.  The
//! server owns the [`BoardModel`] and does every CTFd request itself, so
//! the API token never reaches the browser.  The page gets a full snapshot
//! on load and after every change over server-sent events.
//!
//! Every API call must carry the per-run key from the printed URL (header
//! `x-ctfdc-key`, or `?k=` for the event stream), so other local users and
//! other sites open in the browser cannot read or drive the board.

mod snapshot;

use crate::board::model::{BoardModel, Cmd, Update};
use crate::board::{self, Board};
use crate::config::{LeaderboardArgs, ResolvedConfig};
use axum::extract::{Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use snapshot::Snapshot;
use std::convert::Infallible;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::WatchStream;

const INDEX_HTML: &str = include_str!("../../assets/web/index.html");
const APP_CSS: &str = include_str!("../../assets/web/app.css");
/// `assets/web/app.js`, minified by `build.rs`.
const APP_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/app.min.js"));
const ECHARTS_JS: &str = include_str!("../../assets/web/vendor/echarts.min.js");

/// Header carrying the per-run key on API calls.
const KEY_HEADER: &str = "x-ctfdc-key";

/// Content-Security-Policy for the page: only its own scripts, styles, and
/// API.  Inline style attributes are allowed because the chart library
/// positions its tooltip with them.
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
                   img-src 'self' data:; connect-src 'self'; base-uri 'none'; \
                   form-action 'none'; frame-ancestors 'none'";

struct Shared {
    model: Mutex<BoardModel>,
    host: String,
    key: String,
    snapshot: watch::Sender<Arc<str>>,
    commands: mpsc::UnboundedSender<Cmd>,
    sent_wanted: Mutex<Vec<u64>>,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, BoardModel> {
        self.model.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Rebuild the JSON snapshot and push it to every open page.
    fn publish(&self) {
        let json = {
            let model = self.lock();
            serde_json::to_string(&Snapshot::of(&model, &self.host))
        };
        if let Ok(json) = json {
            self.snapshot.send_replace(json.into());
        }
    }

    /// Ask the fetcher for solves of any newly needed account.
    fn sync_wanted(&self) {
        let wanted = self.lock().wanted_accounts(None);
        let mut sent = self.sent_wanted.lock().unwrap_or_else(|e| e.into_inner());
        if *sent != wanted {
            *sent = wanted.clone();
            let _ = self.commands.send(Cmd::Accounts(wanted));
        }
    }
}

/// Serve the live scoreboard for a CTFd server.
pub async fn run(config: ResolvedConfig, args: LeaderboardArgs) -> anyhow::Result<()> {
    serve(board::connect(&config, &args).await?, &args).await
}

/// Serve the live scoreboard on fictional, simulated data.
pub async fn run_demo(args: LeaderboardArgs) -> anyhow::Result<()> {
    serve(board::connect_demo(&args).await?, &args).await
}

async fn serve(board: Board, args: &LeaderboardArgs) -> anyhow::Result<()> {
    let addr = SocketAddr::new(args.bind, args.port);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| anyhow::anyhow!("Could not listen on {addr}: {e}"))?;
    let local = listener.local_addr()?;

    let key = new_key()?;
    let Board {
        source,
        model,
        server,
    } = board;
    let wanted = model.wanted_accounts(None);
    let (interval, kind) = (model.interval, model.kind);
    let first = serde_json::to_string(&Snapshot::of(&model, &server))?;
    let (snapshot_tx, _) = watch::channel::<Arc<str>>(first.into());
    let (update_tx, mut update_rx) = mpsc::unbounded_channel::<Update>();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<Cmd>();

    let shared = Arc::new(Shared {
        model: Mutex::new(model),
        host: server,
        key: key.clone(),
        snapshot: snapshot_tx,
        commands: cmd_tx,
        sent_wanted: Mutex::new(wanted.clone()),
    });

    let fetcher = tokio::spawn(board::fetch_loop(
        source, args.top, interval, kind, wanted, cmd_rx, update_tx,
    ));
    let applier = {
        let shared = shared.clone();
        tokio::spawn(async move {
            while let Some(update) = update_rx.recv().await {
                shared.lock().apply(update);
                shared.sync_wanted();
                shared.publish();
            }
        })
    };

    let app = router(shared);
    let url = page_url(local, &key);
    // Written without `println!`: whoever reads the link may close the
    // pipe right after it, and that must not stop the server.
    let mut out = std::io::stdout();
    let _ = writeln!(out, "Live scoreboard: {url}");
    if !args.bind.is_loopback() {
        eprintln!(
            "Warning: listening on {local}, so other machines on the network can open the \
             scoreboard with the link above."
        );
    }
    let _ = writeln!(out, "Press Ctrl-C to stop.");
    let _ = out.flush();
    if !args.no_open {
        open_browser(&url);
    }

    let result = tokio::select! {
        r = axum::serve(listener, app) => r.map_err(anyhow::Error::from),
        _ = tokio::signal::ctrl_c() => Ok(()),
    };
    fetcher.abort();
    applier.abort();
    result
}

fn router(shared: Arc<Shared>) -> Router {
    let api = Router::new()
        .route("/state", get(state))
        .route("/events", get(events))
        .route("/refresh", post(refresh))
        .route("/rival", post(set_rival))
        .route("/pin", post(set_pin))
        .layer(middleware::from_fn_with_state(shared.clone(), require_key));

    Router::new()
        .route("/", get(index))
        .route(
            "/app.css",
            get(|| asset("text/css; charset=utf-8", APP_CSS)),
        )
        .route(
            "/app.js",
            get(|| asset("text/javascript; charset=utf-8", APP_JS)),
        )
        .route(
            "/vendor/echarts.min.js",
            get(|| asset("text/javascript; charset=utf-8", ECHARTS_JS)),
        )
        .nest("/api", api)
        .layer(middleware::from_fn(security_headers))
        .with_state(shared)
}

/// The URL that opens the page with this run's key.
fn page_url(addr: SocketAddr, key: &str) -> String {
    let host = if addr.ip().is_unspecified() {
        match addr {
            SocketAddr::V4(_) => "127.0.0.1".to_string(),
            SocketAddr::V6(_) => "[::1]".to_string(),
        }
    } else {
        match addr {
            SocketAddr::V4(a) => a.ip().to_string(),
            SocketAddr::V6(a) => format!("[{}]", a.ip()),
        }
    };
    format!("http://{host}:{}/#k={key}", addr.port())
}

/// A random 128-bit key, hex encoded.
fn new_key() -> anyhow::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("No random source: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Compare without leaking how many leading characters matched.
fn same_key(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

#[derive(Deserialize)]
struct KeyQuery {
    k: Option<String>,
}

async fn require_key(
    State(shared): State<Arc<Shared>>,
    Query(query): Query<KeyQuery>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let given = headers
        .get(KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or(query.k);
    match given {
        Some(k) if same_key(&k, &shared.key) => next.run(request).await,
        _ => (
            StatusCode::UNAUTHORIZED,
            "Open the scoreboard with the link ctfdc printed.",
        )
            .into_response(),
    }
}

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let h = response.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.entry(header::CACHE_CONTROL)
        .or_insert(HeaderValue::from_static("no-store"));
    response
}

async fn index() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        INDEX_HTML,
    )
}

async fn asset(content_type: &'static str, body: &'static str) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
}

async fn state(State(shared): State<Arc<Shared>>) -> impl IntoResponse {
    let json = shared.snapshot.borrow().clone();
    (
        [(header::CONTENT_TYPE, "application/json")],
        json.to_string(),
    )
}

async fn events(
    State(shared): State<Arc<Shared>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let stream = WatchStream::new(shared.snapshot.subscribe())
        .map(|json| Ok(Event::default().event("board").data(json.as_ref())));
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn refresh(State(shared): State<Arc<Shared>>) -> StatusCode {
    let _ = shared.commands.send(Cmd::Refresh);
    StatusCode::ACCEPTED
}

#[derive(Deserialize)]
struct RivalBody {
    /// Account key from the snapshot; `null` resets to the team above you.
    key: Option<String>,
}

async fn set_rival(State(shared): State<Arc<Shared>>, Json(body): Json<RivalBody>) -> StatusCode {
    {
        let mut model = shared.lock();
        match body.key {
            Some(key) => {
                if model.index_of_key(&key).is_none() {
                    return StatusCode::NOT_FOUND;
                }
                model.set_rival(key);
            }
            None => model.reset_rival(),
        }
    }
    shared.sync_wanted();
    shared.publish();
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct PinBody {
    id: u64,
    pinned: bool,
}

async fn set_pin(State(shared): State<Arc<Shared>>, Json(body): Json<PinBody>) -> StatusCode {
    {
        let mut model = shared.lock();
        if !model
            .data
            .standings
            .iter()
            .any(|e| e.account_id == Some(body.id))
            || !model.can_pin(body.id)
        {
            return StatusCode::UNPROCESSABLE_ENTITY;
        }
        model.set_pin(body.id, body.pinned);
    }
    shared.sync_wanted();
    shared.publish();
    StatusCode::NO_CONTENT
}

/// What opens a URL on other Unix systems.
const UNIX_OPENER: &str = "xdg-open";

/// Whether a browser on this machine can show the page to whoever ran
/// ctfdc: not over SSH (the browser would open on the remote machine),
/// and on Linux and other Unix systems only with a desktop session and
/// `xdg-open`.
pub fn browser_available() -> bool {
    let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    let desktop = set("DISPLAY") || set("WAYLAND_DISPLAY");
    let opener = std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| dir.join(UNIX_OPENER).is_file())
    });
    can_open_browser(
        set("SSH_CONNECTION") || set("SSH_TTY"),
        cfg!(any(target_os = "macos", windows)) || (desktop && opener),
    )
}

fn can_open_browser(over_ssh: bool, local_opener: bool) -> bool {
    !over_ssh && local_opener
}

/// Open `url` in the default browser; failures only print a hint.
fn open_browser(url: &str) {
    use std::process::{Command, Stdio};
    let mut command = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(url);
        c
    } else if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", ""]).arg(url);
        c
    } else {
        let mut c = Command::new(UNIX_OPENER);
        c.arg(url);
        c
    };
    let opened = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok();
    if !opened {
        eprintln!("Could not open a browser. Open the link above yourself.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_random_hex() {
        let a = new_key().unwrap();
        let b = new_key().unwrap();
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn no_browser_over_ssh_or_without_an_opener() {
        assert!(can_open_browser(false, true));
        assert!(!can_open_browser(true, true));
        assert!(!can_open_browser(false, false));
    }

    #[test]
    fn compares_keys() {
        assert!(same_key("abc", "abc"));
        assert!(!same_key("abc", "abd"));
        assert!(!same_key("abc", "ab"));
    }

    #[test]
    fn page_url_uses_a_reachable_host() {
        let url = page_url("0.0.0.0:8080".parse().unwrap(), "k1");
        assert_eq!(url, "http://127.0.0.1:8080/#k=k1");
        let url = page_url("[::1]:9".parse().unwrap(), "k1");
        assert_eq!(url, "http://[::1]:9/#k=k1");
    }
}
