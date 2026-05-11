use crate::client::CtfdClient;
use crate::config::ResolvedConfig;
use crate::models::SubmitOutcome;
use serde::Deserialize;
use std::path::Path;

/// Submit a flag for a challenge.
///
/// Supports two modes:
///
/// **Explicit** (`arg2` is `Some(flag)`): `arg1` is the numeric CTFd
/// challenge ID, `arg2` is the flag to submit.  Validates that the
/// challenge ID is numeric before any network request.
///
/// **Auto-detect** (`arg2` is `None`): `arg1` is the flag to submit.
/// The challenge ID is detected from the current directory:
/// 1. Read `metadata.json` for the challenge ID (preferred).
/// 2. Fall back to matching the current `category/challenge-name` path
///    against the fetched challenge list.
///
/// Exit semantics:
/// - Accepted (correct): exits 0, success message on stdout.
/// - Already solved: exits 0, already-solved message on stdout.
/// - Rejected (incorrect): exits non-zero with a submission-outcome
///   message on stderr; not presented as a transport failure.
///
/// For auto-detect, this reuses a single `CtfdClient` instance for both
/// challenge detection and flag submission, and uses `Result`-based
/// error propagation for metadata reading instead of `process::exit`.
pub async fn run(config: ResolvedConfig, arg1: &str, arg2: Option<&str>) -> anyhow::Result<()> {
    let client = CtfdClient::new(&config);

    let (challenge_id, flag) = if let Some(explicit_flag) = arg2 {
        // Explicit mode: arg1 = challenge ID, arg2 = flag.
        let parsed_id: u64 = arg1
            .parse()
            .map_err(|_| anyhow::anyhow!("Challenge ID must be a numeric value, got: {arg1}"))?;
        (parsed_id, explicit_flag.to_string())
    } else {
        // Auto-detect mode: arg1 = flag.
        let flag = arg1.to_string();
        let id = detect_challenge_id(&client).await?;
        (id, flag)
    };

    // Reuse the same client instance for submission (no double instantiation).
    let response = client.submit_flag(challenge_id, &flag).await?;

    match response.outcome() {
        SubmitOutcome::Accepted => {
            println!("{}", response.user_message());
            Ok(())
        }
        SubmitOutcome::AlreadySolved => {
            println!("{}", response.user_message());
            Ok(())
        }
        SubmitOutcome::Rejected => {
            eprintln!("{}", response.user_message());
            std::process::exit(2);
        }
    }
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
                 ctfd-client submit <challenge-id> <flag>"
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
                 or use ctfd-client submit <challenge-id> <flag>",
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
             ctfd-client submit <challenge-id> <flag>"
        )),
        1 => Ok(matches[0].id),
        _ => Err(anyhow::anyhow!(
            "Cannot auto-detect challenge: multiple challenges match \
             category '{category}' and name '{name}'. \
             Use ctfd-client submit <challenge-id> <flag> to specify the exact challenge ID"
        )),
    }
}
