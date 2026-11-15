//! Difficulty from organizer tags, then solve shares and point values.

/// How hard a challenge is, in four steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Easy,
    Medium,
    Hard,
    Extreme,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::Easy => "Easy",
            Level::Medium => "Medium",
            Level::Hard => "Hard",
            Level::Extreme => "Extreme",
        }
    }

    /// Stable key for the web page.
    pub fn key(self) -> &'static str {
        match self {
            Level::Easy => "easy",
            Level::Medium => "medium",
            Level::Hard => "hard",
            Level::Extreme => "extreme",
        }
    }

    /// Chance that a team solves a challenge of this level, before any
    /// solve counts are in.
    pub(super) fn prior(self) -> f64 {
        match self {
            Level::Easy => 0.6,
            Level::Medium => 0.3,
            Level::Hard => 0.12,
            Level::Extreme => 0.04,
        }
    }

    /// The level an organiser's tag names, if it names one.
    fn from_tag(tag: &str) -> Option<Self> {
        let tag = tag.trim().to_lowercase();
        let tag = tag.strip_prefix("difficulty:").unwrap_or(&tag).trim();
        Some(match tag {
            "baby" | "warmup" | "warm-up" | "beginner" | "intro" | "trivial" | "easy" => {
                Level::Easy
            }
            "medium" | "normal" | "intermediate" | "moderate" => Level::Medium,
            "hard" | "difficult" | "advanced" => Level::Hard,
            "insane" | "expert" | "extreme" | "nightmare" | "very hard" | "brutal" => {
                Level::Extreme
            }
            _ => return None,
        })
    }
}

/// A challenge's difficulty and where the reading comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Difficulty {
    pub level: Level,
    /// From an organiser's tag, rather than estimated from solves and
    /// points.
    pub tagged: bool,
}

/// Solve share at or above which a challenge reads as easy on solves
/// alone, and at or below which as hard as it gets.
const SHARE_EASY: f64 = 0.5;
const SHARE_HARDEST: f64 = 0.02;

/// Read a challenge's difficulty: its tag when it has one, else an
/// estimate from how few solved it (`share`, when the board has solves)
/// and where its points sit between the cheapest and dearest challenge
/// (`points`, 0..=1, when values differ).  Solves count double.
pub(super) fn difficulty(tags: &[String], share: Option<f64>, points: Option<f64>) -> Difficulty {
    if let Some(level) = tags.iter().find_map(|t| Level::from_tag(t)) {
        return Difficulty {
            level,
            tagged: true,
        };
    }
    let by_share = share.map(|s| {
        let s = s.clamp(SHARE_HARDEST, SHARE_EASY);
        (SHARE_EASY / s).ln() / (SHARE_EASY / SHARE_HARDEST).ln()
    });
    let score = match (by_share, points) {
        (Some(s), Some(p)) => (2.0 * s + p) / 3.0,
        (Some(s), None) => s,
        (None, Some(p)) => p,
        (None, None) => 0.4,
    };
    let level = match score {
        s if s < 0.25 => Level::Easy,
        s if s < 0.5 => Level::Medium,
        s if s < 0.8 => Level::Hard,
        _ => Level::Extreme,
    };
    Difficulty {
        level,
        tagged: false,
    }
}
