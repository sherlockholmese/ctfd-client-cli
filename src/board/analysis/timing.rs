//! Solve timing, quantiles, and uncertainty in solve shares.

/// When the accounts whose solves are known solved one challenge.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Timing {
    /// Unix time of each known solve, oldest first.
    pub times: Vec<i64>,
    /// For each of those solves, the seconds since that account's
    /// previous solve (or the start): the most it can have spent on it.
    pub spans: Vec<i64>,
    /// Who made each of those solves.
    pub solvers: Vec<String>,
}

impl Timing {
    pub fn first(&self) -> Option<i64> {
        self.times.first().copied()
    }

    pub fn last(&self) -> Option<i64> {
        self.times.last().copied()
    }

    /// Known solves in the `secs` before `now`.
    pub fn recent(&self, now: i64, secs: i64) -> usize {
        self.times.iter().filter(|&&t| t >= now - secs).count()
    }

    /// The median span: what the challenge typically took.
    pub fn typical(&self) -> Option<i64> {
        quantile(&self.spans, 0.5)
    }

    /// The shortest and longest span.
    pub fn extent(&self) -> Option<(i64, i64)> {
        Some((*self.spans.iter().min()?, *self.spans.iter().max()?))
    }

    /// The middle half of the spans: (lower quartile, upper quartile).
    pub fn spread(&self) -> Option<(i64, i64)> {
        if self.spans.len() < 3 {
            return None;
        }
        Some((quantile(&self.spans, 0.25)?, quantile(&self.spans, 0.75)?))
    }
}

/// The `q` quantile (0..=1) of `values`, interpolated; `None` when empty.
pub fn quantile(values: &[i64], q: f64) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let at = q.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let (lo, hi) = (at.floor() as usize, at.ceil() as usize);
    let part = at - lo as f64;
    Some((sorted[lo] as f64 + (sorted[hi] - sorted[lo]) as f64 * part).round() as i64)
}

/// The range a share of `n` out of `of` plausibly lies in (Wilson score
/// interval, 80%).
pub fn share_range(n: usize, of: usize) -> (f64, f64) {
    if of == 0 {
        return (0.0, 1.0);
    }
    const Z: f64 = 1.2816;
    let (n, of) = (n.min(of) as f64, of as f64);
    let p = n / of;
    let denom = 1.0 + Z * Z / of;
    let centre = p + Z * Z / (2.0 * of);
    let margin = Z * (p * (1.0 - p) / of + Z * Z / (4.0 * of * of)).sqrt();
    (
        ((centre - margin) / denom).clamp(0.0, 1.0),
        ((centre + margin) / denom).clamp(0.0, 1.0),
    )
}

/// A length of time in words: `45m`, `1h 05m`, `2d 3h`.
pub fn span_text(secs: i64) -> String {
    let secs = secs.max(0);
    if secs < 60 {
        "under 1m".into()
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h {:02}m", secs / 3600, secs % 3600 / 60)
    } else {
        format!("{}d {}h", secs / 86_400, secs % 86_400 / 3600)
    }
}
