//! Score-over-time series for the scoreboard graph.

use chrono::{DateTime, NaiveDateTime, Utc};

/// Parse a CTFd timestamp into Unix seconds.
///
/// CTFd normally sends RFC 3339 (`2026-11-04T12:00:00.123456+00:00` or
/// `...Z`); timestamps without an offset are treated as UTC.
pub fn parse_time(raw: &str) -> Option<i64> {
    if let Ok(t) = DateTime::parse_from_rfc3339(raw) {
        return Some(t.timestamp());
    }
    NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f"))
        .ok()
        .map(|t| t.and_utc().timestamp())
}

/// Build a step-shaped cumulative score line from `(time, points)` events.
///
/// The line starts at 0 at `start`, jumps at each event, and is extended
/// flat to `end` so every account's line reaches "now".
pub fn step_series(mut events: Vec<(i64, i64)>, start: i64, end: i64) -> Vec<(f64, f64)> {
    events.sort_by_key(|(t, _)| *t);
    let mut points = vec![(start as f64, 0.0)];
    let mut score = 0i64;
    for (t, value) in events {
        points.push((t as f64, score as f64));
        score += value;
        points.push((t as f64, score as f64));
    }
    points.push((end.max(start) as f64, score as f64));
    points
}

/// Current Unix time in seconds.
pub fn now() -> i64 {
    Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ctfd_timestamps() {
        assert_eq!(parse_time("1970-01-01T00:01:00Z"), Some(60));
        assert_eq!(parse_time("1970-01-01T00:01:00.5+00:00"), Some(60));
        assert_eq!(parse_time("1970-01-01T00:01:00.123456"), Some(60));
        assert_eq!(parse_time("garbage"), None);
    }

    #[test]
    fn builds_steps() {
        let pts = step_series(vec![(20, 100), (10, 50)], 0, 30);
        assert_eq!(
            pts,
            vec![
                (0.0, 0.0),
                (10.0, 0.0),
                (10.0, 50.0),
                (20.0, 50.0),
                (20.0, 150.0),
                (30.0, 150.0)
            ]
        );
    }
}
