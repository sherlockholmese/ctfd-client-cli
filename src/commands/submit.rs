use crate::client::CtfdClient;
use crate::config::{ResolvedConfig, SubmitArgs};
use crate::models::{Challenge, SubmitOutcome, SubmitResponse};
use serde::Deserialize;
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
        return run_batch(&client, file, args.wait).await;
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

/// Resolve a challenge argument to `(id, label)`.
///
/// Numeric arguments are used as IDs directly (no network request);
/// anything else is looked up in the challenge list by name.
async fn resolve_term(client: &CtfdClient, term: &str) -> anyhow::Result<(u64, Option<String>)> {
    if let Ok(id) = term.trim().parse::<u64>() {
        return Ok((id, None));
    }
    let challenges = client
        .get_challenges()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch challenge list: {e}"))?;
    let ch = find_challenge(&challenges, term)?;
    Ok((ch.id, Some(challenge_label(ch))))
}

fn challenge_label(ch: &Challenge) -> String {
    format!("{}/{} (#{})", ch.category, ch.name, ch.id)
}

/// Find a challenge by name: exact (case-insensitive) name or
/// `Category/Name` first, then a unique case-insensitive name fragment.
pub fn find_challenge<'a>(
    challenges: &'a [Challenge],
    term: &str,
) -> anyhow::Result<&'a Challenge> {
    let needle = term.trim().to_lowercase();
    let pick = |matches: Vec<&'a Challenge>| -> anyhow::Result<Option<&'a Challenge>> {
        match matches.len() {
            0 => Ok(None),
            1 => Ok(Some(matches[0])),
            _ => {
                let list: Vec<String> = matches
                    .iter()
                    .take(10)
                    .map(|c| challenge_label(c))
                    .collect();
                anyhow::bail!(
                    "'{term}' matches several challenges: {}. Be more specific or use the ID.",
                    list.join(", ")
                )
            }
        }
    };

    let exact = challenges
        .iter()
        .filter(|c| {
            c.name.to_lowercase() == needle
                || format!("{}/{}", c.category, c.name).to_lowercase() == needle
        })
        .collect();
    if let Some(ch) = pick(exact)? {
        return Ok(ch);
    }

    let partial = challenges
        .iter()
        .filter(|c| c.name.to_lowercase().contains(&needle))
        .collect();
    pick(partial)?.ok_or_else(|| {
        anyhow::anyhow!(
            "No challenge named '{term}'. Run `ctfdc status` to list challenges and IDs."
        )
    })
}

/// Best-effort: mark `metadata.json` as solved after a correct submission.
fn mark_local_solved(path: &Path) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(mut meta) = serde_json::from_str::<serde_json::Value>(&content) else {
        return;
    };
    if meta.get("solved_by_me") == Some(&serde_json::Value::Bool(true)) {
        return;
    }
    meta["solved_by_me"] = serde_json::Value::Bool(true);
    if let Ok(json) = serde_json::to_string_pretty(&meta) {
        let _ = std::fs::write(path, json);
    }
}

// ---------------------------------------------------------------------------
// Batch submission
// ---------------------------------------------------------------------------

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

async fn run_batch(client: &CtfdClient, file: &Path, wait: u64) -> anyhow::Result<()> {
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

// ---------------------------------------------------------------------------
// Auto-detect helpers
// ---------------------------------------------------------------------------

/// Minimal metadata structure for reading challenge ID from metadata.json.
#[derive(Debug, Deserialize)]
struct ChallengeMetadata {
    id: u64,
}

/// Detect the numeric CTFd challenge ID from the current directory.
///
/// Prefers `metadata.json` in the current directory.  Falls back to
/// matching the `category/challenge-name` path against the fetched
/// challenge list.
async fn detect_challenge_id(client: &CtfdClient) -> anyhow::Result<u64> {
    // 1. Try metadata.json in the current directory.
    if let Some(id) = read_metadata_challenge_id()? {
        return Ok(id);
    }

    // 2. Infer category/challenge-name from the current directory path.
    let (category, challenge_name) = infer_challenge_path()?;

    // 3. Fetch challenges and match by path.
    let challenges = client
        .get_challenges()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch challenge list: {e}"))?;

    match_challenge_by_path(&challenges, &category, &challenge_name)
}

/// Try to read the numeric challenge ID from `metadata.json` in the
/// current directory.
///
/// Returns `Ok(None)` when the file does not exist.
/// Returns `Err` when the file exists but is malformed or missing a
/// valid numeric ID — this is a hard error that prevents fallback to
/// path matching, since the user likely expects metadata to work.
fn read_metadata_challenge_id() -> anyhow::Result<Option<u64>> {
    let path = Path::new("metadata.json");
    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Cannot read metadata.json: {e}"))?;

    let meta: ChallengeMetadata = serde_json::from_str(&content).map_err(|e| {
        anyhow::anyhow!("metadata.json is malformed or missing a valid challenge ID: {e}")
    })?;

    Ok(Some(meta.id))
}

/// Infer the `(category, challenge_name)` from the current directory
/// path.
///
/// The current directory is assumed to be the challenge directory
/// `category/challenge-name`.  Returns an error when the path cannot
/// be decomposed (e.g., at the filesystem root or only one level deep).
fn infer_challenge_path() -> anyhow::Result<(String, String)> {
    let cwd = std::env::current_dir()
        .map_err(|e| anyhow::anyhow!("Cannot determine current directory: {e}"))?;

    let challenge_name = cwd
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .map(|n| n.to_string())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Cannot auto-detect challenge: current directory has no usable name. \
                 Run this command from a downloaded challenge directory \
                 (category/challenge-name) or use the explicit form: \
                 ctfdc submit <challenge-id> <flag>"
            )
        })?;

    let category = cwd
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .map(|n| n.to_string())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Cannot auto-detect challenge: current directory '{}' does not appear \
                 to be inside a category directory. \
                 Run from a downloaded challenge directory (category/challenge-name) \
                 or use ctfdc submit <challenge-id> <flag>",
                challenge_name
            )
        })?;

    Ok((category, challenge_name))
}

/// Match the inferred `category` and `challenge_name` against the
/// fetched challenge list.
///
/// Returns the numeric challenge ID when exactly one match is found.
/// Reports clear errors for zero matches or ambiguous matches.
fn match_challenge_by_path(
    challenges: &[crate::models::Challenge],
    category: &str,
    name: &str,
) -> anyhow::Result<u64> {
    let matches: Vec<&crate::models::Challenge> = challenges
        .iter()
        .filter(|c| c.category == category && c.name == name)
        .collect();

    match matches.len() {
        0 => Err(anyhow::anyhow!(
            "Cannot auto-detect challenge: no challenge matches \
             category '{category}' and name '{name}' from the current directory. \
             Make sure you are in a downloaded challenge directory or use \
             ctfdc submit <challenge-id> <flag>"
        )),
        1 => Ok(matches[0].id),
        _ => Err(anyhow::anyhow!(
            "Cannot auto-detect challenge: multiple challenges match \
             category '{category}' and name '{name}'. \
             Use ctfdc submit <challenge-id> <flag> to specify the exact challenge ID"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(id: u64, name: &str, category: &str) -> Challenge {
        serde_json::from_value(serde_json::json!({
            "id": id, "name": name, "category": category, "value": 100, "solved_by_me": false
        }))
        .unwrap()
    }

    #[test]
    fn find_prefers_exact_name() {
        let list = vec![ch(1, "RSA", "Crypto"), ch(2, "RSA 2", "Crypto")];
        assert_eq!(find_challenge(&list, "rsa").unwrap().id, 1);
        assert_eq!(find_challenge(&list, "rsa 2").unwrap().id, 2);
    }

    #[test]
    fn find_accepts_category_slash_name_and_fragments() {
        let list = vec![ch(1, "Baby Login", "Web"), ch(2, "Baby Heap", "Pwn")];
        assert_eq!(find_challenge(&list, "pwn/baby heap").unwrap().id, 2);
        assert_eq!(find_challenge(&list, "login").unwrap().id, 1);
        assert!(find_challenge(&list, "baby").is_err());
        assert!(find_challenge(&list, "nope").is_err());
    }

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
