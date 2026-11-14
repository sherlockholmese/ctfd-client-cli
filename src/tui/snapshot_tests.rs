//! Render the scoreboard UI headlessly and check what lands on screen.
//!
//! Set `CTFDC_SNAPSHOT_DIR=<dir>` to also export each frame as colored HTML
//! (dark and light terminal backgrounds) for visual review.

use super::app::{App, BoardLayout, View};
use super::view;
use crate::board::model::{BoardModel, Data, Update};
use crate::board::{demo, series};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier};
use std::time::Duration;

fn demo_app(cutoff: Option<u32>) -> App {
    let world = demo::DemoWorld::new(series::now());
    let mut app = App::new(BoardModel::new(
        "demo.ctfd.invalid".into(),
        true,
        Duration::from_secs(15),
        10,
        cutoff,
        Some(demo::ME_ID),
        Some(demo::ME_NAME.into()),
        world.snapshot(10, &[]),
    ));
    let wanted = app.wanted_accounts();
    for id in wanted {
        app.board.data.accounts.insert(id, world.account(id));
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
        "Catch-up targets",
        "↑ enough to pass",
        "Head to head",
        "Last hour",
        "Points by category",
        "Unsolved by both",
        "Your edge",
        // How far into the CTF the rival, and you, solved things.
        "Rival at",
        "You at",
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
    assert!(t.contains("Catch-up targets"));
    assert!(t.contains("Points by category"));
}

#[test]
fn rival_cycles_and_skips_you() {
    let mut app = demo_app(None);
    app.on_key(key(KeyCode::Tab));
    let me = app.board.me_index().unwrap();
    let first = app.board.rival_index().unwrap();
    assert_ne!(first, me);
    for _ in 0..3 {
        app.on_key(key(KeyCode::Right));
        assert_ne!(app.board.rival_index(), Some(me));
    }
    app.on_key(key(KeyCode::Char('m')));
    assert_eq!(app.board.rival_index(), Some(me - 1), "m = team above you");
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
    assert_eq!(app.board.data.standings[app.selected].name, "🦀 Rustaceans");
}

#[test]
fn live_update_marks_changes_and_keeps_selection() {
    let mut world = demo::DemoWorld::new(series::now());
    let mut app = demo_app(None);
    let selected_name = app.board.data.standings[app.selected].name.clone();
    for i in 0..40 {
        world.tick(series::now() + i);
    }
    app.apply(Update::Data(Box::new(world.snapshot(10, &[]))));
    assert_eq!(app.board.data.standings[app.selected].name, selected_name);
    assert!(!app.board.changes.is_empty(), "score changes are tracked");
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
    let mut app = App::new(BoardModel::new(
        "ctf.example.com".into(),
        false,
        Duration::from_secs(15),
        10,
        None,
        None,
        None,
        Data::default(),
    ));
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

#[test]
fn plan_ranks_targets_and_the_climb() {
    let mut app = demo_app(None);
    app.on_key(key(KeyCode::Char('3')));
    assert_eq!(app.view, View::Plan);
    let buf = draw(&mut app, 160, 44);
    export("plan-160x44", &buf);
    export("plan-100x30", &draw(&mut app, 100, 30));
    let t = text(&buf);
    for needle in [
        "2 solves to pass #5",
        "chance of landing all",
        "Level",
        "Pace",
        "Time",
        "You solve about every",
        // The highlighted target's details, solve times included.
        "●",
        "solves   first",
        "Solved by",
        "Your odds",
        "likely 2",
        "Time taken",
        "typically",
        "▲",
        "Solved at",
        "Best targets",
        "↑ passes",
        "Aim for",
        "Where to focus",
        "Quick win",
    ] {
        assert!(t.contains(needle), "missing {needle:?}\n{t}");
    }

    let plan = &app.board.plan;
    assert!(!plan.targets.is_empty());
    assert!(
        plan.targets
            .windows(2)
            .all(|w| w[0].expected >= w[1].expected),
        "most likely points first"
    );
    // The ladder holds the leaders and the teams around you, in rank order.
    let ranks: Vec<i64> = plan.ladder.iter().map(|r| r.pos).collect();
    assert_eq!(ranks, [1, 2, 3, 4, 5, 7, 8]);

    // s orders the targets off the route; the route stays first.
    use crate::board::analysis::Sort;
    app.on_key(key(KeyCode::Char('s')));
    assert_eq!(app.board.plan.sort, Sort::PerHour);
    let listed = app.board.plan.listed();
    let rest = &listed[app.board.plan.route().len()..];
    assert!(
        rest.windows(2)
            .all(|w| w[0].per_hour.unwrap_or(0.0) >= w[1].per_hour.unwrap_or(0.0)),
        "best points per hour first"
    );
    assert!(text(&draw(&mut app, 160, 44)).contains("/h↓"));
    for _ in 0..4 {
        app.on_key(key(KeyCode::Char('s')));
    }
    assert_eq!(app.board.plan.sort, Sort::Likely);
    let plan = &app.board.plan;

    // The route leads the list, and <- moves the goal up the board.
    assert_eq!(plan.goal, Some(5));
    assert_eq!(plan.route().len(), 2);
    app.on_key(key(KeyCode::Left));
    assert_eq!(app.board.plan.goal, Some(4));
    for _ in 0..10 {
        app.on_key(key(KeyCode::Left));
    }
    assert_eq!(app.board.plan.goal, Some(1));
    let t = text(&draw(&mut app, 160, 44));
    assert!(t.contains("to pass #1 Heap Hoppers"), "{t}");
    export("plan-goal-160x44", &draw(&mut app, 160, 44));
    app.on_key(key(KeyCode::Char('m')));
    assert_eq!(app.board.plan.goal, Some(5));

    // Tab cycles Board -> Analysis -> Plan -> Challenges -> Board.
    app.on_key(key(KeyCode::Tab));
    assert_eq!(app.view, View::Challenges);
    app.on_key(key(KeyCode::Tab));
    assert_eq!(app.view, View::Board);
    app.on_key(key(KeyCode::BackTab));
    assert_eq!(app.view, View::Challenges);
    app.on_key(key(KeyCode::BackTab));
    assert_eq!(app.view, View::Plan);
}

#[test]
fn download_keys_ask_for_the_target_or_everything() {
    use crate::board::model::{Cmd, Download};
    let mut app = demo_app(None);
    // d needs a highlighted target, so it only works on Plan.
    assert_eq!(app.on_key(key(KeyCode::Char('d'))), None);
    assert_eq!(
        app.on_key(key(KeyCode::Char('D'))),
        Some(Cmd::Download(Download::All))
    );
    app.on_key(key(KeyCode::Char('3')));
    app.on_key(key(KeyCode::Down));
    let second = app.board.plan.listed()[1].stat.id;
    assert_eq!(
        app.on_key(key(KeyCode::Char('d'))),
        Some(Cmd::Download(Download::One(second)))
    );
    let t = text(&draw(&mut app, 160, 44));
    assert!(t.contains("d download") && t.contains("D all"), "{t}");

    // While one runs, another cannot start; both ends are announced.
    app.apply(Update::Downloading("Downloading 3 challenges".into()));
    assert_eq!(app.on_key(key(KeyCode::Char('D'))), None);
    assert!(text(&draw(&mut app, 160, 44)).contains("Downloading 3 challenges"));
    app.apply(Update::Downloaded {
        ok: true,
        title: "Downloaded 3 challenges".into(),
        detail: "1 new, 2 files fetched".into(),
    });
    let buf = draw(&mut app, 160, 44);
    export("download-160x44", &buf);
    let t = text(&buf);
    assert!(t.contains("Downloaded 3 challenges") && t.contains("1 new, 2 files fetched"));
    assert!(app.on_key(key(KeyCode::Char('D'))).is_some());
}

#[tokio::test]
async fn demo_data_has_nothing_to_download() {
    use crate::board::model::Download;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    crate::board::request_download(None, Download::All, &tx);
    let mut app = demo_app(None);
    app.apply(rx.recv().await.unwrap());
    assert!(!app.board.downloading);
    assert!(text(&draw(&mut app, 160, 44)).contains("Demo data has no challenge files"));
}

#[test]
fn graph_range_cycles_and_names_itself() {
    let mut app = demo_app(None);
    assert!(text(&draw(&mut app, 160, 44)).contains("Score over time"));
    app.on_key(key(KeyCode::Char('t')));
    assert!(text(&draw(&mut app, 160, 44)).contains("Score over time, last 24h"));
    for _ in 0..6 {
        app.on_key(key(KeyCode::Char('t')));
    }
    let t = text(&draw(&mut app, 160, 44));
    assert!(!t.contains(", last "), "back to the whole graph\n{t}");
}

#[test]
fn notices_show_in_the_corner_when_something_happens() {
    let mut app = demo_app(None);
    assert_eq!(app.board.live_notices().count(), 0, "nothing new on load");

    app.apply(Update::Failed("server said no".into()));
    let buf = draw(&mut app, 160, 44);
    export("notice-160x44", &buf);
    let t = text(&buf);
    assert!(t.contains("Couldn't update the scoreboard"), "{t}");
    assert!(t.contains("server said no"));
    // A second failure in a row is not news.
    app.apply(Update::Failed("server said no".into()));
    assert_eq!(app.board.live_notices().count(), 1);

    let world = demo::DemoWorld::new(series::now());
    app.apply(Update::Data(Box::new(world.snapshot(10, &[]))));
    let t = text(&draw(&mut app, 160, 44));
    assert!(t.contains("Scoreboard is updating again"), "{t}");
}

#[test]
fn challenges_tab_lists_opens_filters_and_takes_flags() {
    use crate::board::model::{ChallengeInfo, Cmd, Download};
    use crate::models::SubmitOutcome;
    let mut app = demo_app(None);
    // Opening the tab asks for the first challenge's details, once.
    let first = app.board.challenge_list()[0].id;
    assert_eq!(
        app.on_key(key(KeyCode::Char('4'))),
        Some(Cmd::Detail(first))
    );
    assert_eq!(app.view, View::Challenges);
    let t = text(&draw(&mut app, 160, 44));
    for needle in [
        "Challenges, 20 of 30 solved",
        "Crypto",
        "4/5",
        "✓",
        "Caesar Salad",
        "Loading",
    ] {
        assert!(t.contains(needle), "missing {needle:?}\n{t}");
    }
    app.apply(Update::Detail(
        first,
        Ok(Box::new(ChallengeInfo {
            description: "Decode the salad.\nSecond line.".into(),
            connection_info: Some("nc host 1".into()),
            files: vec!["salad.txt".into()],
            ..Default::default()
        })),
    ));
    let buf = draw(&mut app, 160, 44);
    export("challenges-160x44", &buf);
    let t = text(&buf);
    for needle in [
        "Decode the salad.",
        "Connect",
        "nc host 1",
        "Files",
        "salad.txt",
    ] {
        assert!(t.contains(needle), "missing {needle:?}\n{t}");
    }
    // Moving on asks for the next one; coming back does not ask again.
    let second = app.board.challenge_list()[1].id;
    assert_eq!(app.on_key(key(KeyCode::Down)), Some(Cmd::Detail(second)));
    assert_eq!(app.on_key(key(KeyCode::Up)), None);

    // u keeps only what is left to solve.
    app.on_key(key(KeyCode::Char('u')));
    assert!(app.challenges().iter().all(|c| !c.solved_by_me));
    assert!(text(&draw(&mut app, 160, 44)).contains("Challenges left to solve (10)"));
    let open = app.challenge_id().unwrap();

    // d downloads it; f types a flag, Enter sends it.
    assert_eq!(
        app.on_key(key(KeyCode::Char('d'))),
        Some(Cmd::Download(Download::One(open)))
    );
    app.on_key(key(KeyCode::Char('f')));
    for c in "flag{x}".chars() {
        assert_eq!(app.on_key(key(KeyCode::Char(c))), None);
    }
    assert!(text(&draw(&mut app, 160, 44)).contains("Flag: flag{x}"));
    assert_eq!(
        app.on_key(key(KeyCode::Enter)),
        Some(Cmd::Submit(open, "flag{x}".into()))
    );
    // Esc drops a flag without sending it.
    app.on_key(key(KeyCode::Char('f')));
    app.on_key(key(KeyCode::Char('q')));
    assert_eq!(app.on_key(key(KeyCode::Esc)), None);
    assert!(!app.quit && app.flag_input.is_none());

    // A wrong flag is reported and changes nothing; a right one counts.
    app.apply(Update::Submitted {
        id: open,
        outcome: SubmitOutcome::Rejected,
        message: "Flag rejected (incorrect).".into(),
    });
    let t = text(&draw(&mut app, 160, 44));
    assert!(t.contains("Last flag") && t.contains("Flag rejected (incorrect)."));
    let before = app.board.h2h.me.as_ref().unwrap().solves;
    app.apply(Update::Submitted {
        id: open,
        outcome: SubmitOutcome::Accepted,
        message: "Flag accepted! Challenge solved.".into(),
    });
    assert_eq!(
        app.board.h2h.me.as_ref().unwrap().solves,
        before.map(|n| n + 1)
    );
    assert!(
        app.challenges().iter().all(|c| c.id != open),
        "no longer left to solve"
    );
    assert!(text(&draw(&mut app, 160, 44)).contains("Flag accepted! Challenge solved."));
}

#[test]
fn a_plan_target_opens_in_challenges() {
    let mut app = demo_app(None);
    app.on_key(key(KeyCode::Char('3')));
    app.on_key(key(KeyCode::Down));
    let target = app.board.plan.listed()[1].stat.id;
    app.on_key(key(KeyCode::Char('o')));
    assert_eq!(app.view, View::Challenges);
    assert_eq!(app.challenge_id(), Some(target));
}
