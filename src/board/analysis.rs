//! Shared scoreboard analytics. Comparison, difficulty, timing, target ranking,
//! and route search are independent calculations over fetched data.

mod comparison;
mod difficulty;
mod route;
mod targets;
mod timing;

pub use comparison::{CategoryRow, ChallengeStat, Comparison, SolvedAt, compare};
pub use difficulty::{Difficulty, Level};
pub use route::route;
pub use targets::{Fact, Field, Focus, Signals, Sort, Target, targets};
pub use timing::{Timing, quantile, share_range, span_text};

#[cfg(test)]
mod tests;
