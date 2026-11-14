//! Rendering for the scoreboard UI.

use super::app::{App, BoardLayout, Momentum, View};
use super::theme;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::canvas::{Canvas, Context, Line as CanvasLine};
use ratatui::widgets::{
    Axis, Block, Cell, Chart, Clear, Dataset, GraphType, HighlightSpacing, LegendPosition,
    Paragraph, Row, Table, TableState, Wrap,
};
use unicode_width::UnicodeWidthChar;

/// Smallest usable terminal.
const MIN_WIDTH: u16 = 60;
const MIN_HEIGHT: u16 = 16;
/// Width from which the board puts the graph beside the standings.
const WIDE_BOARD: u16 = 110;
/// Gutter between side-by-side regions.
const GUTTER: u16 = 3;
/// Rank/score changes stand out for this long after they happen.
const FLASH_SECS: u64 = 10;

pub fn render(f: &mut Frame, app: &mut App) {
    let area = f.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let msg = format!(
            "Terminal too small: {}x{}, need {MIN_WIDTH}x{MIN_HEIGHT}. Press q to quit.",
            area.width, area.height
        );
        f.render_widget(
            Paragraph::new(msg)
                .wrap(Wrap { trim: true })
                .style(theme::muted()),
            area,
        );
        return;
    }

    let [header, _, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    render_header(f, header, app);
    match app.view {
        View::Board => render_board(f, body, app),
        View::Analysis => render_analysis(f, body, app),
        View::Plan => render_plan(f, body, app),
        View::Challenges => render_challenges(f, body, app),
    }
    render_footer(f, footer, app);
    render_notices(f, body, app);
    if app.show_help {
        render_help(f, area);
    }
}

// ----------------------------------------------------------------------
// Formatting helpers
// ----------------------------------------------------------------------

/// Group thousands: `12345` -> `12,345`.
pub fn pts(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if value < 0 { format!("-{out}") } else { out }
}

/// Signed points: `+300`, `-150`, `0`.
fn signed_pts(value: i64) -> String {
    if value > 0 {
        format!("+{}", pts(value))
    } else {
        pts(value)
    }
}

/// Human "time ago" for a Unix timestamp.
fn ago(now: i64, t: Option<i64>) -> String {
    let Some(t) = t else {
        return "no solves".to_string();
    };
    let secs = (now - t).max(0);
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", secs / 60),
        _ => format!("{}h {}m ago", secs / 3600, (secs % 3600) / 60),
    }
}

/// Truncate to `max` terminal columns, ending in `…` when cut.
fn fit(s: &str, max: usize) -> String {
    let total: usize = s.chars().map(|c| c.width().unwrap_or(0)).sum();
    if total <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = c.width().unwrap_or(0);
        if used + w + 1 > max {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

fn local_time(t: i64, with_date: bool) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_opt(t, 0).single() {
        Some(dt) if with_date => dt.format("%d %b %H:%M").to_string(),
        Some(dt) => dt.format("%H:%M").to_string(),
        None => String::new(),
    }
}

fn right(s: impl Into<String>) -> Line<'static> {
    Line::from(s.into()).alignment(Alignment::Right)
}

/// Draw a section heading centred at the top of `area`; returns the area
/// below it.
fn titled(f: &mut Frame, area: Rect, title: &str) -> Rect {
    let used = 1.min(area.height);
    f.render_widget(
        Line::styled(title.to_string(), theme::heading()).centered(),
        area,
    );
    Rect {
        y: area.y + used,
        height: area.height - used,
        ..area
    }
}

/// A muted one-line message placed inside `area` (empty states).
fn note(f: &mut Frame, area: Rect, text: impl Into<String>) {
    f.render_widget(
        Paragraph::new(text.into())
            .style(theme::muted())
            .wrap(Wrap { trim: true }),
        area,
    );
}

/// A decision line drawn across a ledger, labelled in `label_col`.
fn rule_row(label: String, columns: usize, label_col: usize) -> Row<'static> {
    let line = "─".repeat(240);
    let cells: Vec<Cell> = (0..columns)
        .map(|i| {
            let text = if i == label_col {
                format!("{label} {line}")
            } else {
                line.clone()
            };
            Cell::from(Line::styled(text, theme::rule()))
        })
        .collect();
    Row::new(cells)
}

// ----------------------------------------------------------------------
// Header and footer
// ----------------------------------------------------------------------

fn render_header(f: &mut Frame, area: Rect, app: &App) {
    let tab = |label: &'static str, view: View| {
        if app.view == view {
            Span::styled(
                label,
                theme::accent().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )
        } else {
            Span::styled(label, theme::muted())
        }
    };
    let left = Line::from(vec![
        Span::styled(" ctfdc  ", theme::heading()),
        tab("Board", View::Board),
        Span::raw("   "),
        tab("Analysis", View::Analysis),
        Span::raw("   "),
        tab("Plan", View::Plan),
        Span::raw("   "),
        tab("Challenges", View::Challenges),
        Span::styled(format!("   {}", app.board.host), theme::muted()),
    ]);
    f.render_widget(Paragraph::new(left), area);

    let mut status: Vec<Span> = Vec::new();
    if app.board.demo {
        status.push(Span::styled("Demo data   ", theme::attention()));
    }
    let time = app.board.updated_at.format("%H:%M:%S").to_string();
    if let Some(err) = &app.board.error {
        status.push(Span::styled(
            format!("Update failed: {}. Showing {time} ", fit(err, 48)),
            theme::attention(),
        ));
    } else if app.board.refreshing {
        status.push(Span::styled("Updating… ", theme::muted()));
    } else {
        status.push(Span::styled(
            format!("Updated {time}, next in {}s ", app.board.next_refresh_in()),
            theme::muted(),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(status)).right_aligned(), area);
}

fn render_footer(f: &mut Frame, area: Rect, app: &App) {
    let keys: &[(&str, &str)] = if let Some(ref flag) = app.flag_input {
        let line = Line::from(vec![
            Span::styled(" Flag: ", theme::heading()),
            Span::raw(flag.clone()),
            Span::styled("▏", theme::accent()),
            Span::styled("   Enter submit   Esc cancel", theme::muted()),
        ]);
        f.render_widget(Paragraph::new(line), area);
        return;
    } else if let Some(ref q) = app.search {
        let what = if app.view == View::Challenges {
            " Find challenge: "
        } else {
            " Find team: "
        };
        let line = Line::from(vec![
            Span::styled(what, theme::heading()),
            Span::raw(q.clone()),
            Span::styled("▏", theme::accent()),
            Span::styled("   Enter keep   Esc close", theme::muted()),
        ]);
        f.render_widget(Paragraph::new(line), area);
        return;
    } else {
        match app.view {
            View::Board => &[
                ("↑/↓", "select"),
                ("m", "you"),
                ("/", "find"),
                ("Enter", "graph team"),
                ("t", "range"),
                ("l", "layout"),
                ("Tab", "analysis"),
                ("?", "keys"),
                ("q", "quit"),
            ],
            View::Analysis => &[
                ("<-/->", "rival"),
                ("↑/↓", "targets"),
                ("m", "rival above you"),
                ("Tab", "plan"),
                ("?", "keys"),
                ("q", "quit"),
            ],
            View::Challenges => &[
                ("↑/↓", "select"),
                ("f", "submit flag"),
                ("d", "download"),
                ("D", "all"),
                ("u", "unsolved"),
                ("/", "find"),
                ("Tab", "board"),
                ("?", "keys"),
                ("q", "quit"),
            ],
            View::Plan => &[
                ("<-/->", "goal"),
                ("↑/↓", "targets"),
                ("s", "order"),
                ("o", "open"),
                ("d", "download"),
                ("D", "all"),
                ("Tab", "challenges"),
                ("?", "keys"),
                ("q", "quit"),
            ],
        }
    };
    let mut spans = vec![Span::raw(" ")];
    for (k, label) in keys {
        spans.push(Span::styled(*k, theme::accent()));
        spans.push(Span::styled(format!(" {label}   "), theme::muted()));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_help(f: &mut Frame, area: Rect) {
    const KEY_W: usize = 15;
    let rows: [(&str, &str); 27] = [
        ("Tab, 1, 2, 3, 4", "Board / Analysis / Plan / Challenges"),
        ("↑/↓, j/k", "Move selection or target list"),
        ("PgUp/PgDn", "Move ten rows"),
        ("Home/End, g/G", "First / last team"),
        ("m", "Jump to you (Board)"),
        ("", "Rival = team above you (Analysis)"),
        ("", "Goal = the default one (Plan)"),
        ("/", "Find a team (n: next match)"),
        ("Enter", "Add or remove team on the graph"),
        ("s", "Order targets: likely, per hour, odds ... (Plan)"),
        ("t", "Graph range: all, 24h ... 30m"),
        ("l", "Board layout: split, graph, table"),
        ("<-/->, h/l", "Rival one place up / down (Analysis)"),
        ("", "Goal one place up / down (Plan)"),
        ("a", "Analyse the selected team"),
        ("o", "Open the highlighted target in Challenges (Plan)"),
        ("f, Enter", "Submit a flag for the challenge (Challenges)"),
        ("u", "Only challenges left to solve (Challenges)"),
        ("PgUp/PgDn", "Scroll the description (Challenges)"),
        ("d", "Download the highlighted challenge"),
        ("D", "Download every challenge, as ctfdc dl does"),
        ("r", "Update now"),
        ("q, Ctrl-C", "Quit"),
        ("", ""),
        ("Update rate", "lb -n SECS"),
        ("Cut-off", "cutoff = N in ctfd.toml"),
        ("Count from", "since = TIME in ctfd.toml"),
    ];
    let content_w = rows
        .iter()
        .map(|(_, v)| KEY_W + 1 + unicode_width::UnicodeWidthStr::width(*v))
        .max()
        .unwrap_or(40) as u16;
    // Content + one space padding each side + borders.
    let width = (content_w + 4).min(area.width.saturating_sub(2));
    let height = (rows.len() as u16 + 2).min(area.height.saturating_sub(2));
    let rect = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    let lines: Vec<Line> = rows
        .iter()
        .map(|(k, v)| {
            Line::from(vec![
                Span::styled(format!(" {k:<KEY_W$} "), theme::accent()),
                Span::raw(*v),
            ])
        })
        .collect();
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .title(Line::styled(" Keys ", theme::heading()))
                .title_bottom(Line::styled(" Esc closes ", theme::muted())),
        ),
        rect,
    );
}

// ----------------------------------------------------------------------
// Board
// ----------------------------------------------------------------------

fn render_board(f: &mut Frame, area: Rect, app: &mut App) {
    let wide = area.width >= WIDE_BOARD;
    match (app.layout, wide) {
        (BoardLayout::Graph, _) => render_graph(f, area, app, true),
        (BoardLayout::Table, true) => {
            let [table, _, feed] = Layout::horizontal([
                Constraint::Percentage(60),
                Constraint::Length(GUTTER),
                Constraint::Min(0),
            ])
            .areas(area);
            render_standings(f, table, app);
            render_feed(f, feed, app);
        }
        (BoardLayout::Table, false) => render_standings(f, area, app),
        (BoardLayout::Split, true) => {
            let table_w = (area.width * 45 / 100).clamp(52, 72);
            let [table, _, right] = Layout::horizontal([
                Constraint::Length(table_w),
                Constraint::Length(GUTTER),
                Constraint::Min(0),
            ])
            .areas(area);
            let feed_h = (right.height / 3).clamp(5, 12);
            let [graph, _, feed] = Layout::vertical([
                Constraint::Min(8),
                Constraint::Length(1),
                Constraint::Length(feed_h),
            ])
            .areas(right);
            render_standings(f, table, app);
            render_graph(f, graph, app, false);
            render_feed(f, feed, app);
        }
        (BoardLayout::Split, false) => {
            let [graph, _, table] = Layout::vertical([
                Constraint::Percentage(42),
                Constraint::Length(1),
                Constraint::Min(5),
            ])
            .areas(area);
            render_graph(f, graph, app, false);
            render_standings(f, table, app);
        }
    }
}

fn render_standings(f: &mut Frame, area: Rect, app: &mut App) {
    let inner = titled(f, area, "Standings");

    if app.board.data.standings.is_empty() {
        note(
            f,
            inner,
            format!("No {}s on the scoreboard yet.", app.board.kind.noun()),
        );
        return;
    }

    let name_w = inner.width.saturating_sub(5 + 8 + 8 + 11 + 4) as usize;
    let cutoff = app.board.cutoff.map(|n| n as usize);
    let mut rows: Vec<Row> = Vec::new();
    let mut selected_row = 0;
    let standings = app.board.data.standings.clone();

    for (i, e) in standings.iter().enumerate() {
        if cutoff == Some(i) {
            rows.push(rule_row(format!("↑ top {i} qualify"), 5, 1));
        }
        if i == app.selected {
            selected_row = rows.len();
        }
        let is_me = app.board.is_me(e);
        let change = app.board.change_for(e);
        let flashing = change.is_some_and(|c| c.at.elapsed().as_secs() < FLASH_SECS);

        let name_style = match (is_me, app.line_color(e.account_id)) {
            (true, _) => theme::you(),
            (false, Some(color)) => Style::new().fg(color),
            (false, None) => theme::text(),
        };
        let mut name = vec![Span::styled(
            fit(&e.name, name_w.saturating_sub(if is_me { 4 } else { 0 })),
            name_style,
        )];
        if is_me {
            name.push(Span::styled(" you", theme::muted()));
        }
        let behind = i
            .checked_sub(1)
            .map(|j| standings[j].score - e.score)
            .map(|gap| {
                if gap == 0 {
                    "tied".to_string()
                } else {
                    pts(gap)
                }
            })
            .unwrap_or_default();

        let change_cell = match change {
            Some(c) => {
                let mut spans = Vec::new();
                if c.rank_delta > 0 {
                    spans.push(Span::styled(
                        format!("↑{}", c.rank_delta),
                        theme::positive(),
                    ));
                } else if c.rank_delta < 0 {
                    spans.push(Span::styled(
                        format!("↓{}", -c.rank_delta),
                        theme::negative(),
                    ));
                }
                if c.score_delta != 0 {
                    if !spans.is_empty() {
                        spans.push(Span::raw(" "));
                    }
                    spans.push(Span::styled(
                        signed_pts(c.score_delta),
                        theme::signed(c.score_delta),
                    ));
                }
                Line::from(spans)
            }
            None => Line::default(),
        };

        let score_style = if flashing {
            theme::attention().add_modifier(Modifier::BOLD)
        } else if is_me {
            theme::you()
        } else {
            theme::text()
        };
        rows.push(Row::new(vec![
            Cell::from(right(e.pos.to_string()).style(theme::muted())),
            Cell::from(Line::from(name)),
            Cell::from(right(pts(e.score)).style(score_style)),
            Cell::from(right(behind).style(theme::muted())),
            Cell::from(change_cell),
        ]));
    }

    let header = Row::new(vec![
        Cell::from(right("#")),
        Cell::from("Team"),
        Cell::from(right("Score")),
        Cell::from(right("Behind")),
        Cell::from("Last 15m"),
    ])
    .style(theme::muted());

    let table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Min(8),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(11),
        ],
    )
    .header(header)
    .column_spacing(1)
    .row_highlight_style(theme::selected())
    .highlight_symbol(Line::styled("▶", theme::accent()))
    .highlight_spacing(HighlightSpacing::Always);

    app.board_state.select(Some(selected_row));
    f.render_stateful_widget(table, inner, &mut app.board_state);
}

/// Draw the score graph.  The standings swatches act as the legend, so a
/// separate legend is only drawn when the graph is shown alone.
fn render_graph(f: &mut Frame, area: Rect, app: &App, legend: bool) {
    let inner = titled(f, area, &graph_title(app));

    if app.board.lines.iter().all(|l| l.points.len() <= 2) {
        note(
            f,
            inner,
            "No solves yet. The graph fills in as teams score.",
        );
        return;
    }

    let first = app
        .board
        .lines
        .iter()
        .filter_map(|l| l.points.first().map(|p| p.0))
        .fold(f64::MAX, f64::min);
    // With a range picked, start that long ago and from the lowest score
    // then, so the recent climb fills the graph.
    let x0 = match app.range.secs() {
        Some(secs) => first.max((app.board.now - secs) as f64),
        None => first,
    };
    let x1 = (app.board.now as f64).max(x0 + 60.0);
    let windowed: Vec<Vec<(f64, f64)>> = app
        .board
        .lines
        .iter()
        .map(|l| window(&l.points, x0))
        .collect();
    let y1 = windowed
        .iter()
        .flat_map(|l| l.iter().map(|p| p.1))
        .fold(0.0, f64::max)
        .max(1.0)
        * 1.08;
    let y0 = match app.range.secs() {
        Some(_) => windowed
            .iter()
            .filter_map(|l| l.first().map(|p| p.1))
            .fold(y1, f64::min),
        None => 0.0,
    };

    let names_w = (inner.width / 4).clamp(10, 22) as usize;
    let datasets: Vec<Dataset> = app
        .board
        .lines
        .iter()
        .zip(&windowed)
        .map(|(l, points)| {
            let name = if l.is_me {
                format!("{} (you)", fit(&l.name, names_w.saturating_sub(6)))
            } else {
                fit(&l.name, names_w)
            };
            let mut style =
                Style::new().fg(l.slot.map_or(ratatui::style::Color::Reset, theme::series));
            if l.is_me {
                style = style.add_modifier(Modifier::BOLD);
            }
            Dataset::default()
                .name(name)
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(style)
                .data(points)
        })
        .collect();

    let with_date = x1 - x0 > 20.0 * 3600.0;
    let mid = (x0 + x1) / 2.0;
    let x_labels: Vec<Line> = [x0, mid, x1]
        .iter()
        .map(|t| Line::styled(local_time(*t as i64, with_date), theme::muted()))
        .collect();
    let y_labels: Vec<Line> = [y0, (y0 + y1) / 2.0, y1]
        .iter()
        .map(|v| Line::styled(pts(*v as i64), theme::muted()))
        .collect();

    let chart = Chart::new(datasets)
        .x_axis(
            Axis::default()
                .bounds([x0, x1])
                .labels(x_labels)
                .style(theme::muted()),
        )
        .y_axis(
            Axis::default()
                .bounds([y0, y1])
                .labels(y_labels)
                .style(theme::muted()),
        )
        .legend_position(legend.then_some(LegendPosition::TopLeft))
        .hidden_legend_constraints((Constraint::Ratio(1, 2), Constraint::Ratio(3, 4)));
    f.render_widget(chart, inner);
}

fn graph_title(app: &App) -> String {
    match app.range.label() {
        Some(label) => format!("Score over time, last {label}"),
        None => "Score over time".into(),
    }
}

/// The part of a step line from `start` on, beginning at the score held
/// then.
fn window(points: &[(f64, f64)], start: f64) -> Vec<(f64, f64)> {
    let cut = points.partition_point(|p| p.0 < start);
    let mut out = Vec::with_capacity(points.len() - cut + 1);
    if cut > 0 {
        out.push((start, points[cut - 1].1));
    }
    out.extend_from_slice(&points[cut..]);
    out
}

fn render_feed(f: &mut Frame, area: Rect, app: &App) {
    let inner = titled(f, area, "Latest solves");

    if app.board.feed.is_empty() {
        note(f, inner, "No solves yet.");
        return;
    }
    let team_w = (inner.width as usize / 3).clamp(8, 24);
    let rows: Vec<Row> = app
        .board
        .feed
        .iter()
        .take(inner.height as usize)
        .map(|item| {
            let time_style = if item.fresh {
                theme::attention().add_modifier(Modifier::BOLD)
            } else {
                theme::muted()
            };
            let team_style = if item.is_me {
                theme::you()
            } else {
                theme::text()
            };
            Row::new(vec![
                Cell::from(Span::styled(local_time(item.t, false), time_style)),
                Cell::from(Span::styled(fit(&item.team, team_w), team_style)),
                Cell::from(Line::from(vec![
                    Span::raw(item.challenge.clone()),
                    Span::styled(
                        format!("  {}", item.category),
                        theme::category(&item.category),
                    ),
                ])),
                Cell::from(right(signed_pts(item.value)).style(theme::positive())),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Length(team_w as u16),
            Constraint::Min(8),
            Constraint::Length(6),
        ],
    )
    .column_spacing(2);
    f.render_widget(table, inner);
}

// ----------------------------------------------------------------------
// Analysis
// ----------------------------------------------------------------------

/// Width of the side column (head to head, categories, edge).
const SIDE_WIDTH: u16 = 50;
/// Smallest gap between analysis panels; extra width widens the gaps
/// instead of stretching columns, so related numbers stay together.
const PANEL_GAP: u16 = 4;
/// Terminal height from which table headers get a blank line below.
const AIRY_HEIGHT: u16 = 34;

/// Width of the challenge-name column for a list of names.
fn name_width(names: impl Iterator<Item = usize>) -> u16 {
    names.max().unwrap_or(16).clamp(16, 32) as u16
}

/// Header row, with breathing room on tall terminals.
fn header_row(cells: Vec<Cell<'static>>, airy: bool) -> Row<'static> {
    Row::new(cells)
        .style(theme::muted())
        .bottom_margin(u16::from(airy))
}

fn render_analysis(f: &mut Frame, area: Rect, app: &mut App) {
    let [verdict, _, rest] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);
    render_verdict(f, verdict, app);
    if app.board.h2h.me.is_none() {
        return;
    }

    let c = &app.board.h2h.comparison;
    let name_w = name_width(
        c.catch_up
            .iter()
            .chain(c.field.iter())
            .map(|t| fit(&t.name, 64).chars().count()),
    );
    // Pts, Solves, Top N, Category, Challenge, Rival at, Running + spacing.
    let left_w = 6 + 6 + 7 + 11 + name_w + 8 + 8 + 2 * 6;
    let side_stack = |f: &mut Frame, side: Rect, app: &mut App| {
        let cats_h = RADAR_HEIGHT.max(app.board.h2h.comparison.categories.len() as u16 + 3);
        let [h2h, _, cats, _, edge] = Layout::vertical([
            Constraint::Length(9),
            Constraint::Length(1),
            Constraint::Length(cats_h),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(side);
        render_head_to_head(f, h2h, app);
        render_categories(f, cats, app);
        render_edge(f, edge, app);
    };

    if area.width >= 2 * left_w + SIDE_WIDTH + 2 * PANEL_GAP {
        // Very wide: three columns, each full height.
        let [side, catch, field] = Layout::horizontal([
            Constraint::Length(SIDE_WIDTH),
            Constraint::Length(left_w),
            Constraint::Length(left_w),
        ])
        .flex(ratatui::layout::Flex::SpaceBetween)
        .spacing(PANEL_GAP)
        .areas(rest);
        render_catch_up(f, catch, app, name_w);
        render_field(f, field, app, name_w);
        side_stack(f, side, app);
    } else if area.width >= left_w + SIDE_WIDTH + PANEL_GAP {
        // Wide: the comparison rail, then catch-up over open challenges.
        let [side, main] =
            Layout::horizontal([Constraint::Length(SIDE_WIDTH), Constraint::Length(left_w)])
                .flex(ratatui::layout::Flex::SpaceBetween)
                .spacing(PANEL_GAP)
                .areas(rest);
        let catch_rows = c.catch_up.len().max(1) as u16 + 5;
        let [catch, _, field] = Layout::vertical([
            Constraint::Length(catch_rows.min(main.height / 2).max(5)),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(main);
        render_catch_up(f, catch, app, name_w);
        render_field(f, field, app, name_w);
        side_stack(f, side, app);
    } else {
        let catch_rows = c.catch_up.len().max(1) as u16 + 5;
        let [catch, _, cats] = Layout::vertical([
            Constraint::Length(catch_rows.min(rest.height / 2).max(5)),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(rest);
        let narrow = |r: Rect, w: u16| Rect {
            width: r.width.min(w),
            ..r
        };
        render_catch_up(f, narrow(catch, left_w), app, name_w);
        render_categories(f, narrow(cats, SIDE_WIDTH), app);
    }
}

/// The two lines that answer "where do we stand, and what now?".
fn render_verdict(f: &mut Frame, area: Rect, app: &App) {
    let h = &app.board.h2h;
    let Some(ref me) = h.me else {
        note(
            f,
            area,
            format!(
                "Your {noun} isn't on the scoreboard yet.",
                noun = app.board.kind.noun()
            ),
        );
        return;
    };

    // Line 1: standing.
    let mut first = vec![
        Span::styled(format!("#{} {}", me.pos, fit(&me.name, 30)), theme::you()),
        Span::raw(format!("  {} pts", pts(me.score))),
    ];
    let sep = || Span::styled("   |   ", theme::muted());
    if let Some(ref r) = h.rival {
        let diff = me.score - r.score;
        first.push(sep());
        let (n, word, style) = if diff >= 0 {
            (diff, "ahead of", theme::positive())
        } else {
            (-diff, "behind", theme::negative())
        };
        first.push(Span::styled(
            format!("{} {word}", pts(n)),
            style.add_modifier(Modifier::BOLD),
        ));
        first.push(Span::styled(
            format!(" #{} {}", r.pos, fit(&r.name, 28)),
            theme::heading(),
        ));
    }
    if let Some(ref cut) = h.cutoff {
        first.push(sep());
        let (word, style) = if cut.points >= 0 {
            ("clear of the cut-off", theme::positive())
        } else {
            ("short of the cut-off", theme::negative())
        };
        first.push(Span::styled(
            format!("{} {word}", pts(cut.points.abs())),
            style,
        ));
        first.push(Span::styled(format!(" ({})", cut.label), theme::muted()));
    }

    // Line 2: what to do.
    let c = &h.comparison;
    let second = match h.rival {
        None => vec![Span::styled("Pick a rival: <-/->", theme::muted())],
        Some(_) if h.rival_loading => vec![Span::styled("Loading…", theme::muted())],
        Some(ref r) if r.score > me.score => {
            let gap = r.score - me.score;
            match c.gap_closing_index(gap) {
                Some(i) => vec![
                    Span::styled("To pass: ", theme::muted()),
                    Span::styled(format!("top {} catch-up targets", i + 1), theme::heading()),
                    Span::styled(
                        format!("  +{}", pts(c.catch_up[..=i].iter().map(|t| t.value).sum())),
                        theme::positive(),
                    ),
                ],
                None => vec![
                    Span::styled("Catch-up targets are worth ", theme::muted()),
                    Span::styled(format!("+{}", pts(c.catch_up_points())), theme::positive()),
                    Span::styled(", not enough on its own", theme::muted()),
                ],
            }
        }
        Some(_) => vec![
            Span::styled("Your edge: ", theme::muted()),
            Span::styled(format!("+{}", pts(c.edge_points())), theme::positive()),
        ],
    };

    f.render_widget(
        Paragraph::new(vec![Line::from(first), Line::from(second)]).centered(),
        area,
    );
}

fn render_head_to_head(f: &mut Frame, area: Rect, app: &App) {
    let inner = titled(f, area, "Head to head");
    let h = &app.board.h2h;
    let Some(ref me) = h.me else { return };

    let cell_pts = |v: i64| Cell::from(right(signed_pts(v)).style(theme::signed(v)));
    let momentum = |m: Option<Momentum>| m.unwrap_or_default();
    let mine = momentum(h.me_momentum);
    let theirs = momentum(h.rival_momentum);
    let rival_name = h
        .rival
        .as_ref()
        .map(|r| fit(&r.name, 16))
        .unwrap_or_else(|| "Rival".into());

    let mut rows = vec![
        Row::new(vec![
            Cell::from("Rank"),
            Cell::from(right(format!("#{}", me.pos))),
            Cell::from(right(
                h.rival
                    .as_ref()
                    .map(|r| format!("#{}", r.pos))
                    .unwrap_or_default(),
            )),
        ]),
        Row::new(vec![
            Cell::from("Score"),
            Cell::from(right(pts(me.score)).style(theme::you())),
            Cell::from(right(
                h.rival.as_ref().map(|r| pts(r.score)).unwrap_or_default(),
            )),
        ]),
        Row::new(vec![
            Cell::from("Solves"),
            Cell::from(right(me.solves.map(|n| n.to_string()).unwrap_or_default())),
            Cell::from(right(
                h.rival
                    .as_ref()
                    .and_then(|r| r.solves)
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
            )),
        ]),
        Row::new(vec![
            Cell::from("Last hour"),
            cell_pts(mine.last_hour),
            cell_pts(theirs.last_hour),
        ]),
        Row::new(vec![
            Cell::from("Last 3 hours"),
            cell_pts(mine.last_3h),
            cell_pts(theirs.last_3h),
        ]),
        Row::new(vec![
            Cell::from("Last solve"),
            Cell::from(right(ago(app.board.now, mine.last_solve))),
            Cell::from(right(ago(app.board.now, theirs.last_solve))),
        ]),
    ];
    if h.rival.is_none() {
        rows.truncate(3);
    }
    let header = header_row(
        vec![
            Cell::from(""),
            Cell::from(right("You")),
            Cell::from(right(rival_name)),
        ],
        f.area().height >= AIRY_HEIGHT,
    );
    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Length(16),
            Constraint::Length(16),
        ],
    )
    .header(header)
    .column_spacing(2);
    f.render_widget(table, inner);
}

fn render_catch_up(f: &mut Frame, area: Rect, app: &mut App, name_w: u16) {
    let h = &app.board.h2h;
    let inner = titled(f, area, "Catch-up targets");

    let Some(ref rival) = h.rival else {
        note(f, inner, "Pick a rival: <-/->");
        return;
    };
    if h.rival_loading {
        note(f, inner, "Loading…");
        return;
    }
    if h.comparison.categories.is_empty() {
        note(f, inner, "Challenges aren't visible yet.");
        return;
    }
    let targets = &h.comparison.catch_up;
    if targets.is_empty() {
        note(
            f,
            inner,
            format!("Nothing: you have everything {} has.", fit(&rival.name, 24)),
        );
        return;
    }

    let me_score = h.me.as_ref().map_or(0, |m| m.score);
    let gap = rival.score - me_score;
    let closing = h.comparison.gap_closing_index(gap);
    let top_label = format!("Top {}", app.board.top_n);
    let mut rows: Vec<Row> = Vec::new();
    let mut running = 0;
    let mut selected_row = 0;
    let selected = app.catch_state.selected().unwrap_or(0);
    for (i, t) in targets.iter().enumerate() {
        running += t.value;
        if i == selected {
            selected_row = rows.len();
        }
        rows.push(Row::new(vec![
            Cell::from(right(pts(t.value)).style(theme::points())),
            Cell::from(right(t.solves.map(|s| s.to_string()).unwrap_or_default())),
            Cell::from(
                right(format!("{}/{}", t.top_solves, app.board.top_n))
                    .style(theme::popularity(t.top_solves, app.board.top_n)),
            ),
            Cell::from(Span::styled(
                fit(&t.category, 11),
                theme::category(&t.category),
            )),
            Cell::from(fit(&t.name, name_w as usize)),
            Cell::from(right(solved_in(t.at, app.board.plan.start)).style(theme::muted())),
            Cell::from(right(pts(running)).style(theme::muted())),
        ]));
        if closing == Some(i) {
            rows.push(rule_row("↑ enough to pass".to_string(), 7, 4));
        }
    }

    let header = header_row(
        vec![
            Cell::from(right("Pts")),
            Cell::from(right("Solves")),
            Cell::from(right(top_label)),
            Cell::from("Category"),
            Cell::from("Challenge"),
            Cell::from(right("Rival at")),
            Cell::from(right("Running")),
        ],
        f.area().height >= AIRY_HEIGHT,
    );
    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(7),
            Constraint::Length(11),
            Constraint::Length(name_w),
            Constraint::Length(8),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .column_spacing(2)
    .row_highlight_style(theme::selected())
    .highlight_symbol(Line::styled("▶", theme::accent()))
    .highlight_spacing(HighlightSpacing::Always);

    let mut state = app.catch_state;
    state.select(Some(selected_row));
    f.render_stateful_widget(table, inner, &mut state);
    *app.catch_state.offset_mut() = state.offset();
}

/// Rows the category radar wants (title, chart, legend, gap line).
const RADAR_HEIGHT: u16 = 18;

/// Points by category as a radar: each spoke is the share of that
/// category's points held, so big and small categories share one scale.
/// Falls back to a table when there are too few spokes or too little room.
fn render_categories(f: &mut Frame, area: Rect, app: &App) {
    let inner = titled(f, area, "Points by category");
    let cats = &app.board.h2h.comparison.categories;
    if cats.is_empty() {
        note(f, inner, "Challenges aren't visible yet.");
        return;
    }
    let spokes: Vec<_> = cats.iter().filter(|c| c.total_points > 0).collect();
    if spokes.len() < 3 || inner.height < 10 || inner.width < 30 {
        render_category_table(f, inner, app);
        return;
    }
    let h = &app.board.h2h;
    let rival = h.rival.as_ref().filter(|_| !h.rival_loading);
    let rival_color = theme::series(1);

    let [chart, legend, gap] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    // One x unit per column and two y units per row keeps the grid round,
    // since a terminal cell is about twice as tall as it is wide.
    let half_w = f64::from(chart.width) / 2.0;
    let half_h = f64::from(chart.height);
    let label_w = 10;
    let radius = (half_w - label_w as f64 - 1.0).min(half_h - 2.0).max(2.0);
    let n = spokes.len();
    let angle =
        |i: usize| std::f64::consts::FRAC_PI_2 - i as f64 * std::f64::consts::TAU / n as f64;
    let at = |i: usize, r: f64| (r * angle(i).cos(), r * angle(i).sin());
    let share = |pts: i64, total: i64| (pts as f64 / total as f64).clamp(0.0, 1.0);
    let mine: Vec<f64> = spokes
        .iter()
        .map(|c| share(c.mine_points, c.total_points))
        .collect();
    let theirs: Vec<f64> = spokes
        .iter()
        .map(|c| share(c.theirs_points, c.total_points))
        .collect();

    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([-half_w, half_w])
        .y_bounds([-half_h, half_h])
        .paint(|ctx| {
            let grid = Color::DarkGray;
            let polygon = |ctx: &mut Context, values: &dyn Fn(usize) -> f64, color: Color| {
                for i in 0..n {
                    let j = (i + 1) % n;
                    let (x1, y1) = at(i, radius * values(i));
                    let (x2, y2) = at(j, radius * values(j));
                    ctx.draw(&CanvasLine::new(x1, y1, x2, y2, color));
                }
            };
            polygon(ctx, &|_| 1.0, grid);
            for i in 0..n {
                let (x, y) = at(i, radius);
                ctx.draw(&CanvasLine::new(0.0, 0.0, x, y, grid));
            }
            ctx.layer();
            if rival.is_some() {
                polygon(ctx, &|i| theirs[i], rival_color);
                ctx.layer();
            }
            polygon(ctx, &|i| mine[i], Color::Reset);
            for (i, c) in spokes.iter().enumerate() {
                let name = fit(&c.category, label_w);
                let w = name
                    .chars()
                    .map(|ch| ch.width().unwrap_or(0))
                    .sum::<usize>() as f64;
                let (dx, dy) = (angle(i).cos(), angle(i).sin());
                let (x, y) = at(i, radius + 1.0);
                let x = if dx > 0.3 {
                    x + 1.0
                } else if dx < -0.3 {
                    x - w - 1.0
                } else {
                    x - w / 2.0
                };
                let y = y + dy.signum() * if dy.abs() > 0.3 { 1.0 } else { 0.0 };
                ctx.print(x, y, Span::styled(name, theme::category(&c.category)));
            }
        });
    f.render_widget(canvas, chart);

    let mut keys = vec![
        Span::styled("━ ", Style::new().add_modifier(Modifier::BOLD)),
        Span::styled("You", theme::you()),
    ];
    if let Some(r) = rival {
        keys.push(Span::raw("   "));
        keys.push(Span::styled("━ ", Style::new().fg(rival_color)));
        keys.push(Span::raw(fit(&r.name, 24)));
    }
    keys.push(Span::styled("   share of pts", theme::muted()));
    f.render_widget(Line::from(keys).centered(), legend);

    let summary = match rival {
        Some(_) => {
            let worst = spokes
                .iter()
                .min_by_key(|c| c.delta())
                .filter(|c| c.delta() < 0);
            let best = spokes
                .iter()
                .max_by_key(|c| c.delta())
                .filter(|c| c.delta() > 0);
            let mut parts = Vec::new();
            if let Some(c) = worst {
                parts.push(Span::styled("Behind most: ", theme::muted()));
                parts.push(Span::styled(
                    fit(&c.category, 12),
                    theme::category(&c.category),
                ));
                parts.push(Span::styled(
                    format!(" {}", signed_pts(c.delta())),
                    theme::negative(),
                ));
            }
            if let Some(c) = best {
                if !parts.is_empty() {
                    parts.push(Span::raw("  "));
                }
                parts.push(Span::styled("Ahead: ", theme::muted()));
                parts.push(Span::styled(
                    fit(&c.category, 12),
                    theme::category(&c.category),
                ));
                parts.push(Span::styled(
                    format!(" {}", signed_pts(c.delta())),
                    theme::positive(),
                ));
            }
            if parts.is_empty() {
                parts.push(Span::styled("Level in every category", theme::muted()));
            }
            Line::from(parts)
        }
        None => {
            let (got, of) = spokes
                .iter()
                .fold((0, 0), |(g, o), c| (g + c.mine_points, o + c.total_points));
            Line::from(vec![
                Span::raw(pts(got)),
                Span::styled(format!(" of {} pts", pts(of)), theme::muted()),
            ])
        }
    };
    f.render_widget(summary.centered(), gap);
}

fn render_category_table(f: &mut Frame, inner: Rect, app: &App) {
    let cats = &app.board.h2h.comparison.categories;
    let has_rival = app.board.h2h.rival.is_some() && !app.board.h2h.rival_loading;
    let rows: Vec<Row> = cats
        .iter()
        .map(|c| {
            let mut cells = vec![
                Cell::from(Span::styled(
                    fit(&c.category, 14),
                    theme::category(&c.category),
                )),
                Cell::from(right(format!("{}/{}", c.mine, c.total))),
            ];
            if has_rival {
                cells.push(Cell::from(right(format!("{}/{}", c.theirs, c.total))));
                cells.push(Cell::from(
                    right(signed_pts(c.delta())).style(theme::signed(c.delta())),
                ));
            } else {
                cells.push(Cell::from(right(pts(c.mine_points))));
                cells.push(Cell::from(right(pts(c.total_points)).style(theme::muted())));
            }
            Row::new(cells)
        })
        .collect();
    let header = if has_rival {
        ["Category", "You", "Them", "Pts diff"]
    } else {
        ["Category", "You", "Pts", "Of"]
    };
    let header = header_row(
        vec![
            Cell::from(header[0]),
            Cell::from(right(header[1])),
            Cell::from(right(header[2])),
            Cell::from(right(header[3])),
        ],
        f.area().height >= AIRY_HEIGHT,
    );
    let table = Table::new(
        rows,
        [
            Constraint::Min(10),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(9),
        ],
    )
    .header(header)
    .column_spacing(2);
    f.render_widget(table, inner);
}

fn render_field(f: &mut Frame, area: Rect, app: &App, name_w: u16) {
    let inner = titled(
        f,
        area,
        if app.board.h2h.rival.is_some() {
            "Unsolved by both"
        } else {
            "Open for you"
        },
    );
    let field = &app.board.h2h.comparison.field;
    if field.is_empty() {
        note(f, inner, "Nothing else open.");
        return;
    }
    let rows: Vec<Row> = field
        .iter()
        .take(inner.height.saturating_sub(1) as usize)
        .map(|c| {
            Row::new(vec![
                Cell::from(right(pts(c.value)).style(theme::points())),
                Cell::from(right(c.solves.map(|s| s.to_string()).unwrap_or_default())),
                Cell::from(
                    right(format!("{}/{}", c.top_solves, app.board.top_n))
                        .style(theme::popularity(c.top_solves, app.board.top_n)),
                ),
                Cell::from(Span::styled(
                    fit(&c.category, 11),
                    theme::category(&c.category),
                )),
                Cell::from(fit(&c.name, name_w as usize)),
            ])
        })
        .collect();
    let header = header_row(
        vec![
            Cell::from(right("Pts")),
            Cell::from(right("Solves")),
            Cell::from(right(format!("Top {}", app.board.top_n))),
            Cell::from("Category"),
            Cell::from("Challenge"),
        ],
        f.area().height >= AIRY_HEIGHT,
    );
    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(7),
            Constraint::Length(11),
            Constraint::Length(name_w),
        ],
    )
    .header(header)
    .column_spacing(2);
    f.render_widget(table, inner);
}

fn render_edge(f: &mut Frame, area: Rect, app: &App) {
    let h = &app.board.h2h;
    let inner = titled(f, area, "Your edge");
    if h.rival.is_none() || h.rival_loading {
        note(f, inner, "Pick a rival: <-/->");
        return;
    }
    let edge = &h.comparison.edge;
    if edge.is_empty() {
        note(f, inner, "None.");
        return;
    }
    let rows: Vec<Row> = edge
        .iter()
        .take(inner.height.saturating_sub(1) as usize)
        .map(|c| {
            Row::new(vec![
                Cell::from(right(pts(c.value)).style(theme::positive())),
                Cell::from(right(c.solves.map(|s| s.to_string()).unwrap_or_default())),
                Cell::from(Text::from(fit(&c.name, 26))),
                Cell::from(right(solved_in(c.at, app.board.plan.start)).style(theme::muted())),
            ])
        })
        .collect();
    let header = header_row(
        vec![
            Cell::from(right("Pts")),
            Cell::from(right("Solves")),
            Cell::from("Challenge"),
            Cell::from(right("You at")),
        ],
        f.area().height >= AIRY_HEIGHT,
    );
    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Min(10),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .column_spacing(2);
    f.render_widget(table, inner);
}

// ----------------------------------------------------------------------
// Plan
// ----------------------------------------------------------------------

/// The goal and how to reach it, the ladder to pick the goal from, and
/// every open challenge ranked by likely points.
fn render_plan(f: &mut Frame, area: Rect, app: &mut App) {
    let [verdict, _, rest] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);
    render_plan_verdict(f, verdict, app);
    if app.board.h2h.me.is_none() {
        return;
    }

    let name_w = name_width(
        app.board
            .plan
            .targets
            .iter()
            .map(|t| fit(&t.stat.name, 64).chars().count()),
    );
    // Step, Pts, Odds, Time, Per hour, Level, Category, Challenge; the
    // "Why" column takes what is left, up to its full width.
    let name_w = name_w.max(22);
    let least_w = 2 + 6 + 5 + 8 + 5 + 7 + 11 + name_w + 2 * 8 + 2;
    let main_w = least_w + WHY_WIDTH;
    // The ladder, a blank line, and the two lines on pace.
    let ladder_h = app.board.plan.ladder.len() as u16 + 4 + 4;
    if area.width >= least_w + PLAN_SIDE + PANEL_GAP {
        // The rail first, as on the Analysis tab: pick, then act.
        let [side, main] =
            Layout::horizontal([Constraint::Length(PLAN_SIDE), Constraint::Max(main_w)])
                .flex(ratatui::layout::Flex::SpaceBetween)
                .spacing(PANEL_GAP)
                .areas(rest);
        let [ladder, _, focus] = Layout::vertical([
            Constraint::Length(ladder_h),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(side);
        render_ladder(f, ladder, app);
        render_focus(f, focus, app);
        // The list, then everything known about the highlighted target.
        let facts_h = FACT_ROWS.min(main.height / 2);
        let [list, _, facts] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(facts_h),
        ])
        .areas(main);
        render_targets(f, list, app, name_w);
        render_facts(f, facts, app);
    } else {
        let [ladder, _, targets] = Layout::vertical([
            Constraint::Length(ladder_h.min(rest.height / 2)),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(rest);
        render_ladder(f, ladder, app);
        render_targets(f, targets, app, name_w);
    }
}

/// How far into the CTF something was solved, or nothing when unknown.
fn solved_in(at: Option<i64>, start: i64) -> String {
    at.map(|t| crate::board::analysis::span_text(t - start))
        .unwrap_or_default()
}

/// Width of the Plan rail: the ladder with its marker.
const PLAN_SIDE: u16 = 58;
/// Lines given to the highlighted target's details.
const FACT_ROWS: u16 = 7;
/// Width of the "Why" column.
const WHY_WIDTH: u16 = 19;

/// Chance as a whole percentage; anything under half a percent is "<1%".
fn percent(odds: f64) -> String {
    if odds > 0.0 && odds < 0.005 {
        "<1%".into()
    } else {
        format!("{:.0}%", odds * 100.0)
    }
}

fn solves(n: usize) -> String {
    format!("{n} {}", if n == 1 { "solve" } else { "solves" })
}

/// One line: what you aim for and what it takes.
fn render_plan_verdict(f: &mut Frame, area: Rect, app: &App) {
    if app.board.h2h.me.is_none() {
        note(
            f,
            area,
            format!(
                "Your {noun} isn't on the scoreboard yet.",
                noun = app.board.kind.noun()
            ),
        );
        return;
    }
    let plan = &app.board.plan;
    let goal = plan.goal_rung();
    let qualifying = |pos: i64| app.board.cutoff.is_some_and(|n| i64::from(n) == pos);

    let second = match goal {
        None if plan.targets.is_empty() => {
            vec![Span::styled("Nothing left to solve.", theme::muted())]
        }
        None => vec![Span::styled("You lead the board.", theme::positive())],
        Some(g) => {
            let aim = if qualifying(g.pos) {
                format!("reach top {}", g.pos)
            } else {
                format!("pass #{} {}", g.pos, fit(&g.name, 28))
            };
            match (g.to_pass, g.route_odds) {
                (Some((n, gain)), Some(odds)) => vec![
                    Span::styled(
                        format!("{} to {aim}", solves(n)),
                        theme::heading().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("   +{}", pts(gain)), theme::positive()),
                    Span::styled(
                        format!(
                            "   {} chance of landing {}",
                            percent(odds),
                            if n == 1 { "it" } else { "all" }
                        ),
                        theme::muted(),
                    ),
                ],
                _ => vec![Span::styled(
                    format!("Not enough open points to {aim}"),
                    theme::attention(),
                )],
            }
        }
    };
    f.render_widget(Paragraph::new(Line::from(second)).centered(), area);
}

fn render_targets(f: &mut Frame, area: Rect, app: &mut App, name_w: u16) {
    let inner = titled(f, area, "Best targets");
    let plan = &app.board.plan;
    if plan.targets.is_empty() {
        note(
            f,
            inner,
            if app.board.h2h.comparison.categories.is_empty() {
                "Challenges aren't visible yet."
            } else {
                "Nothing left to solve."
            },
        );
        return;
    }
    // The route to the goal first, in the order to try it, closed by a
    // line; then everything else, most likely points first.
    let route = plan.route();
    let rest = plan.off_route();
    let sort = plan.sort;
    let selected = app.plan_state.selected().unwrap_or(0);
    let mut selected_row = 0;
    let mut rows: Vec<Row> = Vec::new();
    let target_row = |t: &crate::board::analysis::Target, step: Option<usize>| {
        Row::new(vec![
            Cell::from(
                right(step.map(|n| n.to_string()).unwrap_or_default()).style(theme::muted()),
            ),
            Cell::from(right(pts(t.stat.value)).style(theme::points())),
            Cell::from(
                right(percent(t.odds))
                    .style(theme::popularity((t.odds * 10.0).round() as usize, 10)),
            ),
            // A time guessed from the difficulty is marked and quiet.
            Cell::from(match t.time {
                Some(secs) if t.time_guessed => {
                    right(format!("~{}", crate::board::analysis::span_text(secs)))
                        .style(theme::muted())
                }
                Some(secs) => right(crate::board::analysis::span_text(secs)),
                None => right("-").style(theme::muted()),
            }),
            Cell::from(right(
                t.per_hour
                    .map(|r| pts(r.round() as i64))
                    .unwrap_or_else(|| "-".into()),
            )),
            // An organiser's tag reads plainly, an estimate quietly.
            Cell::from(Span::styled(
                t.difficulty.level.label(),
                if t.difficulty.tagged {
                    theme::text()
                } else {
                    theme::muted()
                },
            )),
            Cell::from(Span::styled(
                fit(&t.stat.category, 11),
                theme::category(&t.stat.category),
            )),
            Cell::from(fit(&t.stat.name, name_w as usize)),
            Cell::from(Span::styled(
                fit(&t.why().unwrap_or_default(), WHY_WIDTH as usize),
                theme::muted(),
            )),
        ])
    };
    for (i, t) in route.iter().chain(rest.iter()).enumerate() {
        if i == selected {
            selected_row = rows.len();
        }
        rows.push(target_row(t, (i < route.len()).then_some(i + 1)));
        if i + 1 == route.len()
            && let Some(g) = plan.goal_rung()
        {
            let label = if app.board.cutoff.is_some_and(|n| i64::from(n) == g.pos) {
                format!("↑ top {}, past {}", g.pos, fit(&g.name, 18))
            } else {
                format!("↑ passes #{} {}", g.pos, fit(&g.name, 18))
            };
            rows.push(rule_row(label, 9, 7));
        }
    }
    // The column the list is ordered by carries an arrow.
    use crate::board::analysis::Sort;
    let sorted = |label: &str, by: Sort| {
        if sort == by {
            format!("{label}↓")
        } else {
            label.to_string()
        }
    };
    let header = header_row(
        vec![
            Cell::from(""),
            Cell::from(right(sorted("Pts", Sort::Points))),
            Cell::from(right(sorted("Odds", Sort::Odds))),
            Cell::from(right(sorted("Time", Sort::Time))),
            Cell::from(right(sorted("/h", Sort::PerHour))),
            Cell::from("Level"),
            Cell::from("Category"),
            Cell::from("Challenge"),
            Cell::from("Why"),
        ],
        f.area().height >= AIRY_HEIGHT,
    );
    let table = Table::new(
        rows,
        [
            Constraint::Length(2),
            Constraint::Length(6),
            Constraint::Length(5),
            Constraint::Length(8),
            Constraint::Length(5),
            Constraint::Length(7),
            Constraint::Length(11),
            Constraint::Length(name_w),
            Constraint::Max(WHY_WIDTH),
        ],
    )
    .header(header)
    .column_spacing(2)
    .row_highlight_style(theme::selected())
    .highlight_symbol(Line::styled("▶", theme::accent()))
    .highlight_spacing(HighlightSpacing::Always);
    let mut state = app.plan_state;
    state.select(Some(selected_row));
    f.render_stateful_widget(table, inner, &mut state);
    *app.plan_state.offset_mut() = state.offset();
}

/// A share as a filled bar `width` cells long.
fn meter(n: f64, of: f64, width: usize) -> String {
    let filled = if of > 0.0 {
        ((n / of).clamp(0.0, 1.0) * width as f64).round() as usize
    } else {
        0
    };
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

/// The cell a `fraction` (0..=1) of a `width`-cell track falls in.
fn cell_at(fraction: f64, width: usize) -> usize {
    ((fraction.clamp(0.0, 1.0) * (width - 1) as f64).round() as usize).min(width - 1)
}

/// Everything known about the highlighted target, drawn: a timeline of
/// its solves, then one small block each for how widely it is solved, your
/// odds, the time it takes, and who solved it when.
fn render_facts(f: &mut Frame, area: Rect, app: &App) {
    use crate::board::analysis::span_text;
    let plan = &app.board.plan;
    let listed = plan.listed();
    let Some(t) = listed.get(app.plan_state.selected().unwrap_or(0)) else {
        return;
    };
    let Some((field, start, ahead_tracked)) = plan.context() else {
        return;
    };
    let now = app.board.now;
    let quiet = |text: String| Span::styled(text, theme::muted());
    let [title, timeline, _, blocks] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);

    // The name, with the difficulty as four steps beside it.
    let level = t.difficulty.level as usize;
    f.render_widget(
        Line::from(vec![
            Span::styled(fit(&t.stat.name, 40), theme::heading()),
            Span::raw(format!(
                "   {}{} ",
                "■".repeat(level + 1),
                "□".repeat(3 - level)
            )),
            quiet(format!(
                "{}{}",
                t.difficulty.level.label(),
                if t.difficulty.tagged {
                    ""
                } else {
                    ", estimated"
                }
            )),
        ])
        .centered(),
        title,
    );

    // Solves over the CTF: a mark per tracked solve from the start to now,
    // the last hour drawn dotted.
    // The words beside the track need about 62 cells.
    let track = (area.width as usize).saturating_sub(62).clamp(12, 44);
    let elapsed = (now - start).max(1) as f64;
    let mut cells: Vec<char> = (0..track)
        .map(|i| {
            if (i as f64 + 0.5) / track as f64 >= 1.0 - 3600.0 / elapsed {
                '┄'
            } else {
                '─'
            }
        })
        .collect();
    for &at in &t.timing.times {
        cells[cell_at((at - start) as f64 / elapsed, track)] = '●';
    }
    let summary = match (t.timing.first(), t.timing.last()) {
        (Some(first), Some(last)) => format!(
            "   {} solves   first {} in   latest {} ago",
            t.timing.times.len(),
            span_text(first - start),
            span_text(now - last)
        ),
        _ => "   no tracked team has solved it".into(),
    };
    f.render_widget(
        Line::from(vec![
            quiet("start ".into()),
            Span::raw(cells.into_iter().collect::<String>()),
            quiet(" now".into()),
            quiet(summary),
        ])
        .centered(),
        timeline,
    );

    // Four blocks side by side, each a quiet heading over three lines.
    const BAR: usize = 10;
    let block = |heading: &str, rows: Vec<Line<'static>>| {
        let mut lines = vec![Line::styled(heading.to_string(), theme::muted())];
        lines.extend(rows);
        Paragraph::new(lines)
    };
    let [reach_area, odds_area, time_area, who_area] = Layout::horizontal([
        Constraint::Length(23),
        Constraint::Length(24),
        Constraint::Length(23),
        Constraint::Min(0),
    ])
    .spacing(3)
    .areas(blocks);
    // The last block only where there is room for a name and a time.
    let who = who_area.width >= 18;

    let share = |name: &str, n: usize, of: usize| {
        Line::from(vec![
            quiet(format!("{name:<7}")),
            Span::raw(meter(n as f64, of as f64, BAR)),
            Span::raw(format!(" {n}/{of}")),
        ])
    };
    let mut reach = Vec::new();
    if let Some(n) = t.stat.solves.filter(|_| field.ranked > 0) {
        reach.push(share("all", n as usize, field.ranked));
    }
    if field.top_n > 0 {
        reach.push(share(
            &format!("top {}", field.top_n),
            t.stat.top_solves,
            field.top_n,
        ));
    }
    if ahead_tracked > 0 {
        reach.push(share("ahead", t.ahead, ahead_tracked));
    }
    f.render_widget(block("Solved by", reach), reach_area);

    // Your odds: the estimate inside the range it plausibly lies in.
    const ODDS: usize = 16;
    let (low, high) = t.odds_range;
    let mut cells = vec!['░'; ODDS];
    for cell in &mut cells[cell_at(low, ODDS)..=cell_at(high, ODDS)] {
        *cell = '▒';
    }
    cells[cell_at(t.odds, ODDS)] = '█';
    let (mine, total) = t.category_record;
    f.render_widget(
        block(
            "Your odds",
            vec![
                Line::from(vec![
                    Span::raw(cells.into_iter().collect::<String>()),
                    Span::raw(format!(" {}", percent(t.odds))),
                ]),
                Line::from(quiet(format!(
                    "likely {} to {}",
                    percent(low),
                    percent(high)
                ))),
                Line::from(quiet(format!(
                    "{}: you {mine}/{total}, field {}",
                    fit(&t.stat.category, 8),
                    percent(t.category_share)
                ))),
            ],
        ),
        odds_area,
    );

    // Time: shortest to longest as a line, the middle half as a box, the
    // median as a bar, and your time as an arrow under it.
    const TIME: usize = 22;
    let mut time_rows = Vec::new();
    if let Some(you) = t.time {
        let spans = t.timing.extent().zip(t.timing.typical());
        let top = spans.map_or(0, |((_, max), _)| max).max(you) as f64 * 1.08;
        let at = |secs: i64| cell_at(secs as f64 / top.max(1.0), TIME);
        let mut cells = vec![' '; TIME];
        let mut typical = "no solve times yet".to_string();
        if let Some(((min, max), median)) = spans {
            for cell in &mut cells[at(min)..=at(max)] {
                *cell = '─';
            }
            if let Some((q1, q3)) = t.timing.spread() {
                for cell in &mut cells[at(q1)..=at(q3)] {
                    *cell = '▒';
                }
            }
            cells[at(median)] = '┃';
            typical = format!("typically {}", span_text(median));
        }
        let mut arrow = vec![' '; TIME];
        arrow[at(you)] = '▲';
        time_rows.push(Line::raw(cells.into_iter().collect::<String>()));
        time_rows.push(Line::raw(arrow.into_iter().collect::<String>()));
        time_rows.push(Line::from(vec![
            quiet(format!("{typical}, ")),
            Span::raw(format!(
                "you {}{}",
                if t.time_guessed { "~" } else { "" },
                span_text(you)
            )),
        ]));
    } else {
        time_rows.push(Line::from(quiet("no solve times yet".into())));
    }
    f.render_widget(block("Time taken", time_rows), time_area);

    // Who solved it how far in: the names the timeline cannot carry.
    if who && !t.timing.times.is_empty() {
        const SHOWN: usize = 3;
        let count = t.timing.times.len();
        let named = if count > SHOWN { SHOWN - 1 } else { count };
        let mut rows: Vec<Line> = t
            .timing
            .solvers
            .iter()
            .zip(&t.timing.times)
            .take(named)
            .map(|(name, &at)| {
                Line::from(vec![
                    Span::raw(format!("{:<12}", fit(name, 11))),
                    quiet(span_text(at - start)),
                ])
            })
            .collect();
        if count > named {
            rows.push(Line::from(quiet(format!("and {} more", count - named))));
        }
        f.render_widget(block("Solved at", rows), who_area);
    }
}

/// The teams to pass, with the one the route aims at marked.
fn render_ladder(f: &mut Frame, area: Rect, app: &App) {
    let inner = titled(f, area, "Aim for");
    let Some(ref me) = app.board.h2h.me else {
        return;
    };
    let plan = &app.board.plan;
    let rung = |pos: i64, name: &str, cells: [Cell<'static>; 4]| {
        let [gap, hour, pass, odds] = cells;
        Row::new(vec![
            Cell::from(right(format!("#{pos}"))),
            Cell::from(fit(name, 16)),
            gap,
            hour,
            pass,
            odds,
        ])
    };
    // Your row carries your pace, to read against theirs.
    let pace_cell = |pace: Option<f64>| {
        Cell::from(right(
            pace.map(|p| pts(p.round() as i64)).unwrap_or_default(),
        ))
        .style(theme::muted())
    };
    let blank = || {
        [
            Cell::from(""),
            pace_cell(plan.my_pace),
            Cell::from(""),
            Cell::from(""),
        ]
    };
    let mut rows: Vec<Row> = Vec::new();
    let mut goal_row = None;
    let mut me_shown = false;
    for r in &plan.ladder {
        if !me_shown && r.pos > me.pos {
            rows.push(rung(me.pos, &me.name, blank()).style(theme::you()));
            me_shown = true;
        }
        if Some(r.pos) == plan.goal {
            goal_row = Some(rows.len());
        }
        let gap = -r.ahead;
        let hour = pace_cell(r.pace);
        let (pass, odds) = match (r.to_pass, r.route_odds) {
            (Some((n, _)), Some(odds)) => (
                Cell::from(right(solves(n))),
                Cell::from(right(percent(odds)).style(theme::muted())),
            ),
            _ if r.ahead < 0 => (Cell::from(""), Cell::from("")),
            _ => (Cell::from(right("-").style(theme::muted())), Cell::from("")),
        };
        rows.push(rung(
            r.pos,
            &r.name,
            [
                Cell::from(right(signed_pts(gap)).style(theme::signed(gap))),
                hour,
                pass,
                odds,
            ],
        ));
    }
    if !me_shown {
        rows.push(rung(me.pos, &me.name, blank()).style(theme::you()));
    }
    let header = header_row(
        vec![
            Cell::from(right("#")),
            Cell::from(match app.board.kind {
                crate::board::model::AccountKind::Teams => "Team",
                crate::board::model::AccountKind::Users => "Player",
            }),
            Cell::from(right("Gap")),
            Cell::from(right("Pace")),
            Cell::from(right("To pass")),
            Cell::from(right("Odds")),
        ],
        f.area().height >= AIRY_HEIGHT,
    );
    let table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Length(16),
            Constraint::Length(7),
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Length(5),
        ],
    )
    .header(header)
    .column_spacing(2)
    .row_highlight_style(theme::selected())
    .highlight_symbol(Line::styled("▶", theme::accent()))
    .highlight_spacing(HighlightSpacing::Always);
    let rows_h = (plan.ladder.len() as u16 + 3).min(inner.height);
    let [table_area, _, note_area] = Layout::vertical([
        Constraint::Length(rows_h),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(inner);
    let mut state = TableState::default().with_selected(goal_row);
    f.render_stateful_widget(table, table_area, &mut state);
    // How often you solve, and whether you are closing on the goal.
    let note_area = Rect {
        x: note_area.x + 2,
        width: note_area.width.saturating_sub(2),
        ..note_area
    };
    note(f, note_area, plan.pace_note());
}

fn render_focus(f: &mut Frame, area: Rect, app: &App) {
    let inner = titled(f, area, "Where to focus");
    let focus = &app.board.plan.focus;
    if focus.is_empty() {
        note(f, inner, "Nothing left to solve.");
        return;
    }
    let rows: Vec<Row> = focus
        .iter()
        .take(inner.height.saturating_sub(1) as usize)
        .map(|c| {
            Row::new(vec![
                Cell::from(Span::styled(
                    fit(&c.category, 14),
                    theme::category(&c.category),
                )),
                Cell::from(right(format!("{}/{}", c.mine, c.total)).style(theme::muted())),
                Cell::from(right(c.open.to_string())),
                Cell::from(right(pts(c.points)).style(theme::points())),
                Cell::from(right(pts(c.expected.round() as i64))),
            ])
        })
        .collect();
    let header = header_row(
        vec![
            Cell::from("Category"),
            Cell::from(right("Solved")),
            Cell::from(right("Open")),
            Cell::from(right("Pts")),
            Cell::from(right("Likely")),
        ],
        f.area().height >= AIRY_HEIGHT,
    );
    let table = Table::new(
        rows,
        [
            Constraint::Min(10),
            Constraint::Length(6),
            Constraint::Length(5),
            Constraint::Length(7),
            Constraint::Length(6),
        ],
    )
    .header(header)
    .column_spacing(2);
    // Line up with the ladder above, past its marker column.
    let inner = Rect {
        x: inner.x + 2,
        width: inner.width.saturating_sub(2),
        ..inner
    };
    f.render_widget(table, inner);
}

// ----------------------------------------------------------------------
// Challenges
// ----------------------------------------------------------------------

/// Width of the challenge list beside its details.
const CHALLENGE_LIST: u16 = 52;

/// Every challenge on the left; on the right, the highlighted one: what
/// it says, how to reach it, its files, and the last flag tried.
fn render_challenges(f: &mut Frame, area: Rect, app: &mut App) {
    // Owned, so the list's scroll position can be written back below.
    let list: Vec<crate::models::Challenge> = app.challenges().into_iter().cloned().collect();
    if app.board.data.challenges.is_empty() {
        note(f, area, "Challenges aren't visible yet.");
        return;
    }
    let selected = app
        .chal_state
        .selected()
        .unwrap_or(0)
        .min(list.len().saturating_sub(1));
    let wide = area.width >= CHALLENGE_LIST + PANEL_GAP + 40;
    let [list_area, detail_area] = if wide {
        Layout::horizontal([Constraint::Length(CHALLENGE_LIST), Constraint::Min(0)])
            .spacing(PANEL_GAP)
            .areas(area)
    } else {
        Layout::vertical([Constraint::Percentage(45), Constraint::Min(0)])
            .spacing(1)
            .areas(area)
    };

    let solved = list.iter().filter(|c| c.solved_by_me).count();
    let title = if app.unsolved_only {
        format!("Challenges left to solve ({})", list.len())
    } else {
        format!("Challenges, {solved} of {} solved", list.len())
    };
    let inner = titled(f, list_area, &title);
    let name_w = (inner.width as usize).saturating_sub(2 + 2 + 6 + 7 + 2 * 3);
    // Grouped by category: a heading with how much of it is solved, then
    // its challenges, each with one mark for solved or downloaded.
    let mut rows: Vec<Row> = Vec::new();
    let mut selected_row = 0;
    let mut category: Option<&str> = None;
    for (i, c) in list.iter().enumerate() {
        if category != Some(c.category.as_str()) {
            category = Some(c.category.as_str());
            let all = app
                .board
                .data
                .challenges
                .iter()
                .filter(|o| o.category == c.category);
            let (done, total) = all.fold((0, 0), |(done, total), o| {
                (done + usize::from(o.solved_by_me), total + 1)
            });
            if !rows.is_empty() {
                rows.push(Row::new(vec![Cell::from("")]));
            }
            rows.push(Row::new(vec![
                Cell::from(""),
                Cell::from(Span::styled(
                    fit(&c.category, name_w),
                    theme::category(&c.category).add_modifier(Modifier::BOLD),
                )),
                Cell::from(""),
                Cell::from(right(format!("{done}/{total}")).style(theme::muted())),
            ]));
        }
        if i == selected {
            selected_row = rows.len();
        }
        let mark = if c.solved_by_me {
            Span::styled("✓", theme::positive())
        } else if app.board.on_disk.contains(&c.id) {
            Span::styled("↓", theme::muted())
        } else {
            Span::raw("")
        };
        let row = Row::new(vec![
            Cell::from(mark),
            Cell::from(fit(&c.name, name_w)),
            Cell::from(right(c.solves.map(|n| n.to_string()).unwrap_or_default())),
            Cell::from(right(pts(c.value)).style(theme::points())),
        ]);
        rows.push(if c.solved_by_me {
            row.style(theme::muted())
        } else {
            row
        });
    }
    let header = header_row(
        vec![
            Cell::from(""),
            Cell::from("Challenge"),
            Cell::from(right("Solves")),
            Cell::from(right("Pts")),
        ],
        f.area().height >= AIRY_HEIGHT,
    );
    let table = Table::new(
        rows,
        [
            Constraint::Length(2),
            Constraint::Min(10),
            Constraint::Length(6),
            Constraint::Length(7),
        ],
    )
    .header(header)
    .column_spacing(2)
    .row_highlight_style(theme::selected())
    .highlight_symbol(Line::styled("▶", theme::accent()))
    .highlight_spacing(HighlightSpacing::Always);
    let mut state = app.chal_state;
    state.select(Some(selected_row));
    f.render_stateful_widget(table, inner, &mut state);
    *app.chal_state.offset_mut() = state.offset();

    let Some(c) = list.get(selected) else {
        note(f, detail_area, "Nothing left to solve.");
        return;
    };
    let inner = titled(f, detail_area, &fit(&c.name, detail_area.width as usize));
    let quiet = |text: String| Span::styled(text, theme::muted());
    let mut meta = vec![
        Span::styled(c.category.clone(), theme::category(&c.category)),
        quiet("   ".into()),
        Span::styled(format!("{} pts", pts(c.value)), theme::points()),
    ];
    if let Some(n) = c.solves {
        meta.push(quiet(format!("   {n} solves")));
    }
    if c.solved_by_me {
        meta.push(Span::styled("   solved", theme::positive()));
    } else if app.board.on_disk.contains(&c.id) {
        meta.push(quiet("   on disk".into()));
    }
    let mut lines = vec![Line::from(meta).centered(), Line::raw("")];
    let label = |text: &str| Span::styled(format!("{text:<9}"), theme::muted());
    match app.board.details.get(&c.id) {
        Some(info) => {
            if let Some(ref how) = info.connection_info {
                lines.push(Line::from(vec![label("Connect"), Span::raw(how.clone())]));
            }
            if !info.files.is_empty() {
                lines.push(Line::from(vec![
                    label("Files"),
                    Span::raw(info.files.join(", ")),
                ]));
            }
            if !info.tags.is_empty() {
                lines.push(Line::from(vec![
                    label("Tags"),
                    Span::raw(info.tags.join(", ")),
                ]));
            }
            if let Some(n) = info.max_attempts {
                lines.push(Line::from(vec![
                    label("Attempts"),
                    Span::raw(format!("{n} at most")),
                ]));
            }
        }
        None => lines.push(Line::from(quiet("Loading…".into()))),
    }
    // The last flag tried for this challenge and what the server said.
    if let Some(attempt) = app.board.attempt.as_ref().filter(|a| a.id == c.id) {
        lines.push(Line::from(vec![
            label("Last flag"),
            Span::styled(
                attempt.message.clone(),
                if attempt.solved {
                    theme::positive()
                } else {
                    theme::negative()
                },
            ),
        ]));
    } else if app.board.submitting {
        lines.push(Line::from(vec![label("Flag"), quiet("Sending…".into())]));
    }
    let head_h = (lines.len() as u16 + 1).min(inner.height);
    let [head, body] =
        Layout::vertical([Constraint::Length(head_h), Constraint::Min(0)]).areas(inner);
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), head);
    if let Some(info) = app.board.details.get(&c.id) {
        let text = if info.description.trim().is_empty() {
            Text::styled("No description.", theme::muted())
        } else {
            Text::raw(info.description.replace('\r', ""))
        };
        f.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .scroll((app.detail_scroll, 0)),
            body,
        );
    }
}

// ----------------------------------------------------------------------
// Notices
// ----------------------------------------------------------------------

/// Notices shown at once.
const NOTICE_ROWS: usize = 3;
const NOTICE_WIDTH: u16 = 46;

/// What just happened, stacked in the bottom right corner, newest last.
fn render_notices(f: &mut Frame, area: Rect, app: &App) {
    use crate::board::model::NoticeKind;
    let mut live: Vec<_> = app.board.live_notices().collect();
    let skip = live.len().saturating_sub(NOTICE_ROWS);
    live.drain(..skip);
    if live.is_empty() || area.width < NOTICE_WIDTH + 2 {
        return;
    }
    let text_w = NOTICE_WIDTH as usize - 4;
    let mut lines: Vec<Line> = Vec::new();
    for n in &live {
        let (mark, style) = match n.kind {
            NoticeKind::YourSolve
            | NoticeKind::RankUp
            | NoticeKind::Recovered
            | NoticeKind::Downloaded
            | NoticeKind::FlagAccepted => ("+", theme::positive()),
            NoticeKind::FlagRejected => ("!", theme::negative()),
            NoticeKind::Downloading => ("↓", theme::muted()),
            NoticeKind::RankDown | NoticeKind::Failed => ("!", theme::negative()),
            NoticeKind::Solve => ("·", theme::muted()),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{mark} "), style),
            Span::styled(fit(&n.title, text_w), theme::heading()),
        ]));
        if !n.detail.is_empty() {
            lines.push(Line::styled(
                format!("  {}", fit(&n.detail, text_w)),
                theme::muted(),
            ));
        }
    }
    let height = (lines.len() as u16 + 2).min(area.height);
    let rect = Rect {
        x: area.x + area.width - NOTICE_WIDTH - 1,
        y: area.y + area.height - height,
        width: NOTICE_WIDTH,
        height,
    };
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(lines).block(Block::bordered().border_style(theme::muted())),
        rect,
    );
}
