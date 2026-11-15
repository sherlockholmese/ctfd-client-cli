//! Resolve explicit challenge names or infer a challenge from the current folder.

use crate::client::CtfdClient;
use crate::models::Challenge;
use serde::Deserialize;
use std::path::Path;

/// Resolve a challenge argument to `(id, label)`.
///
/// Numeric arguments are used as IDs directly (no network request);
/// anything else is looked up in the challenge list by name.
pub(super) async fn resolve_term(
    client: &CtfdClient,
    term: &str,
) -> anyhow::Result<(u64, Option<String>)> {
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

pub(super) fn challenge_label(ch: &Challenge) -> String {
    format!("{}/{} (#{})", ch.category, ch.name, ch.id)
}

/// Find a challenge by name: exact (case-insensitive) name or
/// `Category/Name` first, then a unique case-insensitive name fragment.
pub(super) fn find_challenge<'a>(
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
pub(super) async fn detect_challenge_id(client: &CtfdClient) -> anyhow::Result<u64> {
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
}
