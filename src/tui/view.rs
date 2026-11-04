//! Rendering for the scoreboard UI.

use super::app::{App, BoardLayout, Momentum, View};
use super::theme;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Axis, Block, Cell, Chart, Clear, Dataset, GraphType, HighlightSpacing, LegendPosition,
    Paragraph, Row, Table, Wrap,
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
    }
    render_footer(f, footer, app);
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
        Span::styled(format!("   {}", app.host), theme::muted()),
    ]);
    f.render_widget(Paragraph::new(left), area);

    let mut status: Vec<Span> = Vec::new();
    if app.demo {
        status.push(Span::styled("Demo data   ", theme::attention()));
    }
    let time = app.updated_at.format("%H:%M:%S").to_string();
    if let Some(ref err) = app.error {
        status.push(Span::styled(
            format!("Update failed: {}. Showing {time} ", fit(err, 48)),
            theme::attention(),
        ));
    } else if app.refreshing {
        status.push(Span::styled("Updating… ", theme::muted()));
    } else {
        status.push(Span::styled(
            format!("Updated {time}, next in {}s ", app.next_refresh_in()),
            theme::muted(),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(status)).right_aligned(), area);
}

fn render_footer(f: &mut Frame, area: Rect, app: &App) {
    let keys: &[(&str, &str)] = if let Some(ref q) = app.search {
        let line = Line::from(vec![
            Span::styled(" Find team: ", theme::heading()),
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
                ("l", "layout"),
                ("Tab", "analysis"),
                ("?", "keys"),
                ("q", "quit"),
            ],
            View::Analysis => &[
                ("<-/->", "rival"),
                ("↑/↓", "targets"),
                ("m", "rival above you"),
                ("Tab", "board"),
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
    let rows: [(&str, &str); 16] = [
        ("Tab, 1, 2", "Switch Board / Analysis"),
        ("↑/↓, j/k", "Move selection or catch-up list"),
        ("PgUp/PgDn", "Move ten rows"),
        ("Home/End, g/G", "First / last team"),
        ("m", "Jump to you (Board)"),
        ("", "Rival = team above you (Analysis)"),
        ("/", "Find a team (n: next match)"),
        ("Enter", "Add or remove team on the graph"),
        ("l", "Board layout: split, graph, table"),
        ("<-/->, h/l", "Rival one place up / down"),
        ("a", "Analyse the selected team"),
        ("r", "Update now"),
        ("q, Ctrl-C", "Quit"),
        ("", ""),
        ("Update rate", "lb -n SECS"),
        ("Cut-off", "cutoff = N in ctfd.toml"),
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

    if app.data.standings.is_empty() {
        note(
            f,
            inner,
            format!("No {}s on the scoreboard yet.", app.kind.noun()),
        );
        return;
    }

    let name_w = inner.width.saturating_sub(5 + 8 + 8 + 11 + 4) as usize;
    let cutoff = app.cutoff.map(|n| n as usize);
    let mut rows: Vec<Row> = Vec::new();
    let mut selected_row = 0;
    let standings = app.data.standings.clone();

    for (i, e) in standings.iter().enumerate() {
        if cutoff == Some(i) {
            rows.push(rule_row(format!("↑ top {i} qualify"), 5, 1));
        }
        if i == app.selected {
            selected_row = rows.len();
        }
        let is_me = app.is_me(e);
        let change = app.change_for(e);
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
    let inner = titled(f, area, "Score over time");

    if app.lines.iter().all(|l| l.points.len() <= 2) {
        note(
            f,
            inner,
            "No solves yet. The graph fills in as teams score.",
        );
        return;
    }

    let x0 = app
        .lines
        .iter()
        .filter_map(|l| l.points.first().map(|p| p.0))
        .fold(f64::MAX, f64::min);
    let x1 = (app.now as f64).max(x0 + 60.0);
    let y1 = app
        .lines
        .iter()
        .flat_map(|l| l.points.iter().map(|p| p.1))
        .fold(0.0, f64::max)
        .max(1.0)
        * 1.08;

    let names_w = (inner.width / 4).clamp(10, 22) as usize;
    let datasets: Vec<Dataset> = app
        .lines
        .iter()
        .map(|l| {
            let name = if l.is_me {
                format!("{} (you)", fit(&l.name, names_w.saturating_sub(6)))
            } else {
                fit(&l.name, names_w)
            };
            let mut style = Style::new().fg(l.color);
            if l.is_me {
                style = style.add_modifier(Modifier::BOLD);
            }
            Dataset::default()
                .name(name)
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(style)
                .data(&l.points)
        })
        .collect();

    let with_date = x1 - x0 > 20.0 * 3600.0;
    let mid = (x0 + x1) / 2.0;
    let x_labels: Vec<Line> = [x0, mid, x1]
        .iter()
        .map(|t| Line::styled(local_time(*t as i64, with_date), theme::muted()))
        .collect();
    let y_labels: Vec<Line> = [0.0, y1 / 2.0, y1]
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
                .bounds([0.0, y1])
                .labels(y_labels)
                .style(theme::muted()),
        )
        .legend_position(legend.then_some(LegendPosition::TopLeft))
        .hidden_legend_constraints((Constraint::Ratio(1, 2), Constraint::Ratio(3, 4)));
    f.render_widget(chart, inner);
}

fn render_feed(f: &mut Frame, area: Rect, app: &App) {
    let inner = titled(f, area, "Latest solves");

    if app.feed.is_empty() {
        note(f, inner, "No solves yet.");
        return;
    }
    let team_w = (inner.width as usize / 3).clamp(8, 24);
    let rows: Vec<Row> = app
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
    if app.h2h.me.is_none() {
        return;
    }

    let c = &app.h2h.comparison;
    let name_w = name_width(
        c.catch_up
            .iter()
            .chain(c.field.iter())
            .map(|t| fit(&t.name, 64).chars().count()),
    );
    // Pts, Solves, Top N, Category, Challenge, Running + spacing.
    let left_w = 6 + 6 + 7 + 11 + name_w + 8 + 2 * 5;
    let side_stack = |f: &mut Frame, side: Rect, app: &mut App| {
        let cats_h = app.h2h.comparison.categories.len() as u16 + 3;
        let [h2h, _, cats, _, edge] = Layout::vertical([
            Constraint::Length(8),
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
        let [catch, field, side] = Layout::horizontal([
            Constraint::Length(left_w),
            Constraint::Length(left_w),
            Constraint::Length(SIDE_WIDTH),
        ])
        .flex(ratatui::layout::Flex::SpaceBetween)
        .spacing(PANEL_GAP)
        .areas(rest);
        render_catch_up(f, catch, app, name_w);
        render_field(f, field, app, name_w);
        side_stack(f, side, app);
    } else if area.width >= left_w + SIDE_WIDTH + PANEL_GAP {
        // Wide: catch-up over open challenges, side column on the right.
        let [main, side] =
            Layout::horizontal([Constraint::Length(left_w), Constraint::Length(SIDE_WIDTH)])
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
    let h = &app.h2h;
    let Some(ref me) = h.me else {
        note(
            f,
            area,
            format!(
                "Your {noun} isn't on the scoreboard yet.",
                noun = app.kind.noun()
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
                    Span::styled(format!("top {} in Catch up", i + 1), theme::heading()),
                    Span::styled(
                        format!("  +{}", pts(c.catch_up[..=i].iter().map(|t| t.value).sum())),
                        theme::positive(),
                    ),
                ],
                None => vec![
                    Span::styled("Catch up is worth ", theme::muted()),
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
    let h = &app.h2h;
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
            Cell::from(right(ago(app.now, mine.last_solve))),
            Cell::from(right(ago(app.now, theirs.last_solve))),
        ]),
    ];
    if h.rival.is_none() {
        rows.truncate(2);
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
    let h = &app.h2h;
    let inner = titled(f, area, "Catch up");

    let Some(ref rival) = h.rival else {
        note(f, inner, "Pick a rival: <-/->");
        return;
    };
    if h.rival_loading {
        note(f, inner, "Loading…");
        return;
    }
    if app.data.challenges.is_empty() {
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
    let top_label = format!("Top {}", app.top_n);
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
                right(format!("{}/{}", t.top_solves, app.top_n))
                    .style(theme::popularity(t.top_solves, app.top_n)),
            ),
            Cell::from(Span::styled(
                fit(&t.category, 11),
                theme::category(&t.category),
            )),
            Cell::from(fit(&t.name, name_w as usize)),
            Cell::from(right(pts(running)).style(theme::muted())),
        ]));
        if closing == Some(i) {
            rows.push(rule_row("↑ enough to pass".to_string(), 6, 4));
        }
    }

    let header = header_row(
        vec![
            Cell::from(right("Pts")),
            Cell::from(right("Solves")),
            Cell::from(right(top_label)),
            Cell::from("Category"),
            Cell::from("Challenge"),
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

fn render_categories(f: &mut Frame, area: Rect, app: &App) {
    let inner = titled(f, area, "Points by category");
    let cats = &app.h2h.comparison.categories;
    if cats.is_empty() {
        note(f, inner, "Challenges aren't visible yet.");
        return;
    }
    let has_rival = app.h2h.rival.is_some() && !app.h2h.rival_loading;
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
        if app.h2h.rival.is_some() {
            "Open for both"
        } else {
            "Open for you"
        },
    );
    let field = &app.h2h.comparison.field;
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
                    right(format!("{}/{}", c.top_solves, app.top_n))
                        .style(theme::popularity(c.top_solves, app.top_n)),
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
            Cell::from(right(format!("Top {}", app.top_n))),
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
    let h = &app.h2h;
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
            ])
        })
        .collect();
    let header = header_row(
        vec![
            Cell::from(right("Pts")),
            Cell::from(right("Solves")),
            Cell::from("Challenge"),
        ],
        f.area().height >= AIRY_HEIGHT,
    );
    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Min(10),
        ],
    )
    .header(header)
    .column_spacing(2);
    f.render_widget(table, inner);
}
