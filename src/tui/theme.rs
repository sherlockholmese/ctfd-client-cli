//! Semantic styles for the scoreboard UI.
//!
//! Screens use these roles, never raw colors, so meaning stays consistent:
//! the terminal's own foreground/background are kept (works on light and
//! dark terminals), one accent marks interactivity and selection, green
//! and red only mean gain and loss, yellow means "new / needs attention",
//! and series colors only index graph lines.  Color never carries meaning
//! alone: arrows, signs, and words accompany it.

use ratatui::style::{Color, Modifier, Style};

/// Normal text (terminal default colors).
pub fn text() -> Style {
    Style::new()
}

/// Secondary text: labels, units, timestamps, hints.
pub fn muted() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

/// Section headings.
pub fn heading() -> Style {
    Style::new()
        .fg(Color::LightBlue)
        .add_modifier(Modifier::BOLD)
}

/// Challenge point values.
pub fn points() -> Style {
    Style::new().fg(Color::Yellow)
}

/// How many of the top `of` teams solved a challenge: green when most
/// did (likely approachable), yellow for some, red for few or none.
pub fn popularity(count: usize, of: u32) -> Style {
    let ratio = count as f64 / f64::from(of.max(1));
    if ratio >= 0.6 {
        positive()
    } else if ratio >= 0.25 {
        Style::new().fg(Color::Yellow)
    } else {
        negative()
    }
}

/// Stable color per challenge category, so a category reads the same in
/// every table and in the live feed.
pub fn category(name: &str) -> Style {
    const PALETTE: [Color; 8] = [
        Color::Cyan,
        Color::Magenta,
        Color::LightGreen,
        Color::LightRed,
        Color::LightBlue,
        Color::LightMagenta,
        Color::LightCyan,
        Color::Blue,
    ];
    let hash = name
        .to_lowercase()
        .bytes()
        .fold(5381u32, |h, b| h.wrapping_mul(33) ^ u32::from(b));
    Style::new().fg(PALETTE[hash as usize % PALETTE.len()])
}

/// The one interactive accent: active tab, key names.
pub fn accent() -> Style {
    Style::new().fg(Color::Cyan)
}

/// Selected row: bold, marked with an accent `▶` (keeps cell colors
/// readable in any terminal theme, unlike reverse video).
pub fn selected() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

/// Your own team.
pub fn you() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

/// Gains, leads, moving up.
pub fn positive() -> Style {
    Style::new().fg(Color::Green)
}

/// Losses, deficits, moving down, failures.
pub fn negative() -> Style {
    Style::new().fg(Color::Red)
}

/// Fresh live events and states needing judgment (stale data, demo).
pub fn attention() -> Style {
    Style::new().fg(Color::Yellow)
}

/// Decision lines drawn through ledgers (cut-off, gap closed).
pub fn rule() -> Style {
    Style::new().fg(Color::Yellow).add_modifier(Modifier::DIM)
}

/// Graph series colors, assigned by rank among graphed teams.
const SERIES: [Color; 10] = [
    Color::Cyan,
    Color::Magenta,
    Color::Yellow,
    Color::Green,
    Color::Blue,
    Color::Red,
    Color::LightCyan,
    Color::LightMagenta,
    Color::LightGreen,
    Color::LightBlue,
];

/// Color for the `index`-th graphed team.
pub fn series(index: usize) -> Color {
    SERIES[index % SERIES.len()]
}

/// Style for a signed delta (`+` gain / `-` loss / zero neutral).
pub fn signed(value: i64) -> Style {
    match value.signum() {
        1 => positive(),
        -1 => negative(),
        _ => muted(),
    }
}
