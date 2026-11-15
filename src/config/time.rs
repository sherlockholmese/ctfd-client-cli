//! Scoring-start parsing shared by CLI flags and workspace configuration.

/// Parse a scoring start time into Unix seconds.
///
/// Accepts RFC 3339 (`2026-11-07T09:00:00Z`, `...+08:00`), a date and time
/// without an offset in local time (`2026-11-07 09:00`,
/// `2026-11-07T09:00:00`), a bare date (local midnight), or Unix seconds.
pub fn parse_since(raw: &str) -> Option<i64> {
    use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
    let raw = raw.trim();
    if let Ok(n) = raw.parse::<i64>() {
        return Some(n);
    }
    if let Ok(t) = DateTime::parse_from_rfc3339(raw) {
        return Some(t.timestamp());
    }
    let naive = [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ]
    .iter()
    .find_map(|fmt| NaiveDateTime::parse_from_str(raw, fmt).ok())
    .or_else(|| {
        NaiveDate::parse_from_str(raw, "%Y-%m-%d")
            .ok()
            .and_then(|d| d.and_hms_opt(0, 0, 0))
    })?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|t| t.timestamp())
}

pub(super) fn parse_since_arg(raw: &str) -> Result<i64, String> {
    parse_since(raw).ok_or_else(|| {
        "expected a time such as 2026-11-07T09:00:00+08:00, '2026-11-07 09:00' (local), \
         or Unix seconds"
            .to_string()
    })
}

/// Show a scoring start time in local time, e.g. `2026-11-07 09:00 +08:00`.
pub fn format_since(since: i64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_opt(since, 0).single() {
        Some(t) => t.format("%Y-%m-%d %H:%M %:z").to_string(),
        None => since.to_string(),
    }
}
