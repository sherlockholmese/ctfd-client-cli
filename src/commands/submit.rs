//! Single-flag submission, input extraction, and rate-limit retries.

mod batch;
mod resolve;

use crate::client::CtfdClient;
use crate::config::{ResolvedConfig, SubmitArgs};
use crate::models::{SubmitOutcome, SubmitResponse};
use crate::workspace::mark_local_solved;
use resolve::{detect_challenge_id, resolve_term};
use std::path::Path;

/// Maximum number of times a rate-limited submission is attempted.
const RATE_LIMIT_ATTEMPTS: u32 = 6;

/// Submit one or many flags.
///
/// Forms:
///
/// - **Auto-detect** (`arg1` only): `arg1` is the flag; the challenge is
///   detected from the current directory (`metadata.json`, then the
///   `category/challenge-name` path).  A correct flag marks the local
///   `metadata.json` as solved.
/// - **Explicit** (`arg1` + `arg2`): `arg1` is a numeric ID (no lookup
///   needed) or a name / unique name fragment / `Category/Name`.
/// - **Batch** (`--file`): one `CHALLENGE<TAB>FLAG` per line.
///
/// A flag of `-` is read from stdin; `--regex` extracts the flag from the
/// given text.  Rate-limited attempts are retried after `--wait` seconds.
///
/// Exit semantics:
/// - Accepted (correct) or already solved: exits 0.
/// - Rejected (incorrect): exits 2 with the outcome on stderr.
/// - Errors (network, API, rate limit exhausted, CTF paused): exits 1.
pub async fn run(config: ResolvedConfig, args: SubmitArgs) -> anyhow::Result<()> {
    let client = CtfdClient::new(&config);

    if let Some(ref file) = args.file {
        return batch::run(&client, file, args.wait).await;
    }

    let arg1 = args
        .arg1
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Missing challenge or flag argument"))?;

    let (challenge_term, raw_flag) = match args.arg2.as_deref() {
        Some(flag) => (Some(arg1), flag),
        None => (None, arg1),
    };

    // Read/extract the flag before any network request.
    let flag = read_flag(raw_flag, args.regex.as_deref())?;

    let (challenge_id, label) = match challenge_term {
        Some(term) => resolve_term(&client, term).await?,
        None => (detect_challenge_id(&client).await?, None),
    };

    let response = submit_with_wait(&client, challenge_id, &flag, args.wait).await?;
    let message = match label {
        Some(ref label) => format!("{label}: {}", response.user_message()),
        None => response.user_message(),
    };

    match response.outcome() {
        SubmitOutcome::Accepted | SubmitOutcome::AlreadySolved => {
            println!("{message}");
            if challenge_term.is_none() {
                mark_local_solved(Path::new("metadata.json"));
            }
            Ok(())
        }
        SubmitOutcome::Rejected => {
            eprintln!("{message}");
            std::process::exit(2);
        }
        SubmitOutcome::RateLimited | SubmitOutcome::Paused => anyhow::bail!("{message}"),
    }
}

/// Resolve the flag text: `-` reads stdin, `--regex` extracts the first
/// match.  A flag given as an argument without `--regex` is used exactly.
fn read_flag(raw: &str, regex: Option<&str>) -> anyhow::Result<String> {
    let from_stdin = raw == "-";
    let text = if from_stdin {
        std::io::read_to_string(std::io::stdin())
            .map_err(|e| anyhow::anyhow!("Failed to read flag from stdin: {e}"))?
    } else {
        raw.to_string()
    };

    if let Some(pattern) = regex {
        let re = regex::Regex::new(pattern)
            .map_err(|e| anyhow::anyhow!("Invalid --regex pattern: {e}"))?;
        return re
            .find(&text)
            .map(|m| m.as_str().to_string())
            .ok_or_else(|| anyhow::anyhow!("No flag matching /{pattern}/ found in the input."));
    }

    if !from_stdin {
        return Ok(text);
    }

    let trimmed = text.trim();
    if trimmed.is_empty() {
        anyhow::bail!("No flag received on stdin.");
    }
    if trimmed.contains('\n') {
        anyhow::bail!(
            "stdin contains several lines; use --regex to pick the flag, \
             e.g. --regex 'flag\\{{[^}}]*\\}}'"
        );
    }
    Ok(trimmed.to_string())
}

/// Submit a flag, waiting and retrying while CTFd reports `ratelimited`.
async fn submit_with_wait(
    client: &CtfdClient,
    challenge_id: u64,
    flag: &str,
    wait_secs: u64,
) -> anyhow::Result<SubmitResponse> {
    let mut attempt = 1;
    loop {
        let response = client.submit_flag(challenge_id, flag).await?;
        if response.outcome() != SubmitOutcome::RateLimited
            || wait_secs == 0
            || attempt >= RATE_LIMIT_ATTEMPTS
        {
            return Ok(response);
        }
        eprintln!("Rate limited by CTFd; retrying in {wait_secs}s...");
        tokio::time::sleep(std::time::Duration::from_secs(wait_secs)).await;
        attempt += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_extracts_flag() {
        let flag = read_flag("noise flag{x} more", Some(r"flag\{[^}]*\}")).unwrap();
        assert_eq!(flag, "flag{x}");
        assert!(read_flag("nothing", Some("flag")).is_err());
    }

    #[test]
    fn literal_flag_is_exact() {
        assert_eq!(read_flag(" flag{ x } ", None).unwrap(), " flag{ x } ");
    }
}
