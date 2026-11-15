//! Parse a batch file once, resolve challenge names once, and submit each flag.

use super::resolve::{challenge_label, find_challenge};
use super::submit_with_wait;
use crate::client::CtfdClient;
use crate::models::SubmitOutcome;
use std::path::Path;

/// One parsed line of a batch file.
#[derive(Debug, PartialEq)]
struct BatchEntry {
    line: usize,
    challenge: String,
    flag: String,
}

/// Parse a batch file: `CHALLENGE<TAB>FLAG`, or `CHALLENGE FLAG` (split at
/// the first space) when the line has no tab.  Blank lines and lines
/// starting with `#` are skipped.
fn parse_batch(content: &str) -> anyhow::Result<Vec<BatchEntry>> {
    let mut entries = Vec::new();
    for (index, raw) in content.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let split = line
            .split_once('\t')
            .or_else(|| line.trim_start().split_once(' '));
        let (challenge, flag) = match split {
            Some((c, f)) if !c.trim().is_empty() && !f.trim().is_empty() => (c.trim(), f.trim()),
            _ => anyhow::bail!(
                "Line {}: expected `CHALLENGE<TAB>FLAG` or `ID FLAG`, got: {line}",
                index + 1
            ),
        };
        entries.push(BatchEntry {
            line: index + 1,
            challenge: challenge.to_string(),
            flag: flag.to_string(),
        });
    }
    Ok(entries)
}

pub(super) async fn run(client: &CtfdClient, file: &Path, wait: u64) -> anyhow::Result<()> {
    let content = if file == Path::new("-") {
        std::io::read_to_string(std::io::stdin())
            .map_err(|e| anyhow::anyhow!("Failed to read submissions from stdin: {e}"))?
    } else {
        std::fs::read_to_string(file)
            .map_err(|e| anyhow::anyhow!("Cannot read '{}': {e}", file.display()))?
    };
    let entries = parse_batch(&content)?;
    if entries.is_empty() {
        anyhow::bail!("No submissions found in '{}'.", file.display());
    }

    // Fetch the challenge list once, only if some line uses a name.
    let needs_list = entries.iter().any(|e| e.challenge.parse::<u64>().is_err());
    let challenges = if needs_list {
        client
            .get_challenges()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch challenge list: {e}"))?
    } else {
        Vec::new()
    };

    let (mut correct, mut already, mut incorrect, mut errors) = (0usize, 0usize, 0usize, 0usize);

    for entry in &entries {
        let prefix = format!("[line {}]", entry.line);
        let (id, label) = match entry.challenge.parse::<u64>() {
            Ok(id) => (
                id,
                challenges
                    .iter()
                    .find(|c| c.id == id)
                    .map(challenge_label)
                    .unwrap_or_else(|| format!("#{id}")),
            ),
            Err(_) => match find_challenge(&challenges, &entry.challenge) {
                Ok(ch) => (ch.id, challenge_label(ch)),
                Err(e) => {
                    eprintln!("{prefix} Error: {e}");
                    errors += 1;
                    continue;
                }
            },
        };

        match submit_with_wait(client, id, &entry.flag, wait).await {
            Ok(response) => {
                let msg = format!("{prefix} {label}: {}", response.user_message());
                match response.outcome() {
                    SubmitOutcome::Accepted => {
                        correct += 1;
                        println!("{msg}");
                    }
                    SubmitOutcome::AlreadySolved => {
                        already += 1;
                        println!("{msg}");
                    }
                    SubmitOutcome::Rejected => {
                        incorrect += 1;
                        eprintln!("{msg}");
                    }
                    SubmitOutcome::RateLimited => {
                        errors += 1;
                        eprintln!("{msg}");
                    }
                    SubmitOutcome::Paused => anyhow::bail!("{msg}"),
                }
            }
            Err(e) => {
                errors += 1;
                eprintln!("{prefix} {label}: Error: {e}");
            }
        }
    }

    println!(
        "Submitted {} flag{}: {correct} correct, {already} already solved, {incorrect} incorrect, {errors} error{}.",
        entries.len(),
        if entries.len() == 1 { "" } else { "s" },
        if errors == 1 { "" } else { "s" },
    );

    if errors > 0 {
        anyhow::bail!(
            "{errors} submission{} failed.",
            if errors == 1 { "" } else { "s" }
        );
    }
    if incorrect > 0 {
        std::process::exit(2);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_batch_lines() {
        let entries = parse_batch("# flags\n\n12 flag{a b}\nBaby Login\tflag{c}\r\n").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].challenge, "12");
        assert_eq!(entries[0].flag, "flag{a b}");
        assert_eq!(entries[0].line, 3);
        assert_eq!(entries[1].challenge, "Baby Login");
        assert_eq!(entries[1].flag, "flag{c}");
    }

    #[test]
    fn parse_batch_rejects_missing_flag() {
        assert!(parse_batch("12\n").is_err());
    }
}
