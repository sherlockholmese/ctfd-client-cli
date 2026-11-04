//! Render the scoreboard UI headlessly and check what lands on screen.
//!
//! Set `CTFDC_SNAPSHOT_DIR=<dir>` to also export each frame as colored HTML
//! (dark and light terminal backgrounds) for visual review.

use super::app::{App, BoardLayout, Data, Update, View};
use super::{demo, series, view};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier};
use std::time::Duration;

fn demo_app(cutoff: Option<u32>) -> App {
    let world = demo::DemoWorld::new(series::now());
    let mut app = App::new(
        "demo.ctfd.invalid".into(),
        true,
        Duration::from_secs(15),
        10,
        cutoff,
        Some(demo::ME_ID),
        Some(demo::ME_NAME.into()),
        world.snapshot(10, &[]),
    );
    let wanted = app.wanted_accounts();
    for id in wanted {
        app.data.accounts.insert(id, world.account(id));
    }
    app.rebuild(false);
    app
}

fn draw(app: &mut App, w: u16, h: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| view::render(f, app)).unwrap();
    terminal.backend().buffer().clone()
}

fn text(buf: &Buffer) -> String {
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Export a frame as HTML so it can be screenshotted and inspected.
fn export(name: &str, buf: &Buffer) {
    let Ok(dir) = std::env::var("CTFDC_SNAPSHOT_DIR") else {
        return;
    };
    for (theme, bg, fg) in [
        ("dark", "#1e1f22", "#d4d4d4"),
        ("light", "#fbfbfa", "#24292f"),
    ] {
        let css = |c: Color, default: &str| -> String {
            let dark = theme == "dark";
            match c {
                Color::Reset => default.to_string(),
                Color::Black => "#000".into(),
                Color::Red => if dark { "#f0716b" } else { "#c62d28" }.into(),
                Color::Green => if dark { "#7fd47f" } else { "#1f8a3b" }.into(),
                Color::Yellow => if dark { "#e5c07b" } else { "#9a6700" }.into(),
                Color::Blue => if dark { "#6cb6ff" } else { "#0b62c4" }.into(),
                Color::Magenta => if dark { "#d6a0f0" } else { "#8b3fb5" }.into(),
                Color::Cyan => if dark { "#56d4dd" } else { "#0a7f8c" }.into(),
                Color::Gray => "#9da5b4".into(),
                Color::DarkGray => "#6b7280".into(),
                Color::LightRed => if dark { "#ff9e9a" } else { "#e5534b" }.into(),
                Color::LightGreen => if dark { "#a8e6a3" } else { "#2da44e" }.into(),
                Color::LightYellow => "#f2d98c".into(),
                Color::LightBlue => if dark { "#9ccfff" } else { "#2f81f7" }.into(),
                Color::LightMagenta => if dark { "#e8bdf7" } else { "#a475f9" }.into(),
                Color::LightCyan => if dark { "#a5eef2" } else { "#1b9aaa" }.into(),
                Color::White => "#fff".into(),
                Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
                Color::Indexed(_) => default.to_string(),
            }
        };
        let mut html = format!(
            "<!doctype html><meta charset=utf-8><style>body{{margin:0;background:{bg}}}\
             pre{{margin:0;padding:12px;font:14px/1.25 'DejaVu Sans Mono',monospace;color:{fg}}}\
             span{{white-space:pre;display:inline-block;width:1ch;overflow:visible}}\
             span.w{{width:2ch}}</style><pre>"
        );
        for y in 0..buf.area.height {
            let mut skip = 0;
            for x in 0..buf.area.width {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let cell = &buf[(x, y)];
                let width = unicode_width::UnicodeWidthStr::width(cell.symbol()).max(1);
                skip = width - 1;
                let mut fg_c = css(cell.fg, fg);
                let mut bg_c = css(cell.bg, bg);
                if cell.modifier.contains(Modifier::REVERSED) {
                    std::mem::swap(&mut fg_c, &mut bg_c);
                }
                let mut style = format!("color:{fg_c};background:{bg_c};");
                if cell.modifier.contains(Modifier::BOLD) {
                    style.push_str("font-weight:bold;");
                }
                if cell.modifier.contains(Modifier::DIM) {
                    style.push_str("opacity:.6;");
                }
                if cell.modifier.contains(Modifier::UNDERLINED) {
                    style.push_str("text-decoration:underline;");
                }
                let sym = cell
                    .symbol()
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
                let class = if width > 1 { " class=w" } else { "" };
                html.push_str(&format!("<span{class} style=\"{style}\">{sym}</span>"));
            }
            html.push('\n');
        }
        html.push_str("</pre>");
        let path = format!("{dir}/{name}-{theme}.html");
        std::fs::write(&path, html).unwrap();
    }
}

#[test]
fn board_wide_shows_standings_graph_and_feed() {
    let mut app = demo_app(Some(10));
    let buf = draw(&mut app, 160, 44);
    export("board-160x44", &buf);
    let t = text(&buf);
    assert!(t.contains("Standings"), "{t}");
    assert!(t.contains("Score over time"));
    assert!(t.contains("Latest solves"));
    assert!(t.contains("Null Pointers"));
    assert!(t.contains("↑ top 10 qualify"));
    assert!(t.contains("Demo data"));
    assert!(
        !t.contains('→') && !t.contains('·'),
        "no arrows or centre dots"
    );
}

#[test]
fn board_narrow_stacks_graph_over_table() {
    let mut app = demo_app(None);
    let buf = draw(&mut app, 80, 24);
    export("board-80x24", &buf);
    let t = text(&buf);
    assert!(t.contains("Standings"));
    assert!(t.contains("Score over time"));
    assert!(!t.contains("Latest solves"), "feed hidden when narrow");
}

#[test]
fn analysis_wide_shows_every_section() {
    let mut app = demo_app(Some(10));
    app.on_key(key(KeyCode::Tab));
    assert_eq!(app.view, View::Analysis);
    let buf = draw(&mut app, 160, 44);
    export("analysis-160x44", &buf);
    export("analysis-250x60", &draw(&mut app, 250, 60));
    let t = text(&buf);
    for needle in [
        "#6 Null Pointers",
        "behind #5",
        "clear of the cut-off",
        "To pass: top",
        "Catch up",
        "↑ enough to pass",
        "Head to head",
        "Last hour",
        "Points by category",
        "Open for both",
        "Your edge",
    ] {
        assert!(t.contains(needle), "missing {needle:?}\n{t}");
    }
}

#[test]
fn analysis_narrow_keeps_primary_sections() {
    let mut app = demo_app(None);
    app.on_key(key(KeyCode::Tab));
    let buf = draw(&mut app, 100, 30);
    export("analysis-100x30", &buf);
    let t = text(&buf);
    assert!(t.contains("Catch up"));
    assert!(t.contains("Points by category"));
}

#[test]
fn rival_cycles_and_skips_you() {
    let mut app = demo_app(None);
    app.on_key(key(KeyCode::Tab));
    let me = app.me_index().unwrap();
    let first = app.rival_index().unwrap();
    assert_ne!(first, me);
    for _ in 0..3 {
        app.on_key(key(KeyCode::Right));
        assert_ne!(app.rival_index(), Some(me));
    }
    app.on_key(key(KeyCode::Char('m')));
    assert_eq!(app.rival_index(), Some(me - 1), "m = team above you");
}

#[test]
fn help_overlay_and_search() {
    let mut app = demo_app(None);
    app.on_key(key(KeyCode::Char('?')));
    let buf = draw(&mut app, 120, 36);
    export("help-120x36", &buf);
    let t = text(&buf);
    assert!(t.contains("Keys"));
    for line in [
        "Rival = team above you (Analysis)",
        "Add or remove team on the graph",
    ] {
        assert!(t.contains(line), "help line clipped: {line}\n{t}");
    }
    app.on_key(key(KeyCode::Esc));
    assert!(!app.show_help);

    app.on_key(key(KeyCode::Char('/')));
    for c in "rust".chars() {
        app.on_key(key(KeyCode::Char(c)));
    }
    let buf = draw(&mut app, 120, 36);
    export("search-120x36", &buf);
    assert!(text(&buf).contains("Find team: rust"));
    assert_eq!(app.data.standings[app.selected].name, "🦀 Rustaceans");
}

#[test]
fn live_update_marks_changes_and_keeps_selection() {
    let mut world = demo::DemoWorld::new(series::now());
    let mut app = demo_app(None);
    let selected_name = app.data.standings[app.selected].name.clone();
    for i in 0..40 {
        world.tick(series::now() + i);
    }
    app.apply(Update::Data(Box::new(world.snapshot(10, &[]))));
    assert_eq!(app.data.standings[app.selected].name, selected_name);
    assert!(!app.changes.is_empty(), "score changes are tracked");
    let buf = draw(&mut app, 160, 44);
    export("board-live-160x44", &buf);
}

#[test]
fn failed_update_keeps_data_and_says_so() {
    let mut app = demo_app(None);
    app.apply(Update::Failed("Network request failed".into()));
    let buf = draw(&mut app, 140, 30);
    let t = text(&buf);
    assert!(t.contains("Update failed: Network request failed"));
    assert!(t.contains("Null Pointers"), "last good data still shown");
}

#[test]
fn empty_and_tiny_states() {
    let mut app = App::new(
        "ctf.example.com".into(),
        false,
        Duration::from_secs(15),
        10,
        None,
        None,
        None,
        Data::default(),
    );
    let buf = draw(&mut app, 120, 30);
    export("empty-120x30", &buf);
    let t = text(&buf);
    assert!(t.contains("No teams on the scoreboard yet."));
    assert!(t.contains("No solves yet."));

    app.on_key(key(KeyCode::Tab));
    let t = text(&draw(&mut app, 120, 30));
    assert!(t.contains("Your team isn't on the scoreboard yet."), "{t}");

    let t = text(&draw(&mut app, 50, 12));
    assert!(t.contains("Terminal too small"));
}

#[test]
fn layouts_cycle() {
    let mut app = demo_app(None);
    app.on_key(key(KeyCode::Char('l')));
    assert_eq!(app.layout, BoardLayout::Graph);
    let buf = draw(&mut app, 120, 30);
    assert!(!text(&buf).contains("Standings"));
    app.on_key(key(KeyCode::Char('l')));
    assert_eq!(app.layout, BoardLayout::Table);
}
