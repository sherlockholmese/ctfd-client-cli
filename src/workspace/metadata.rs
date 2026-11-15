//! The downloaded challenge's README and metadata file. Metadata records only
//! attachments that reached disk and is updated after accepted submissions.

use super::attachments::AttachmentRecord;
use crate::models::ChallengeDetail;
use std::path::Path;

/// Write README.md for a challenge.
pub(super) fn write_readme(dir: &Path, detail: &ChallengeDetail) -> anyhow::Result<()> {
    let path = dir.join("README.md");

    let mut content = String::new();
    content.push_str(&format!("# {}\n\n", detail.name));
    content.push_str(&format!("**Category:** {}\n", detail.category));
    content.push_str(&format!("**Points:** {}\n", detail.value));

    if let Some(ref type_) = detail.challenge_type {
        content.push_str(&format!("**Type:** {type_}\n"));
    }

    if let Some(ref conn) = detail.connection_info {
        content.push_str(&format!("\n**Connection:** `{conn}`\n"));
    }

    content.push_str(&format!("\n## Description\n\n{}\n", detail.description));

    std::fs::write(&path, content)
        .map_err(|e| anyhow::anyhow!("Failed to write README.md: {e}"))?;

    Ok(())
}

/// Write metadata.json for a challenge.
pub(super) fn write_metadata(
    dir: &Path,
    detail: &ChallengeDetail,
    attachments: &[AttachmentRecord],
) -> anyhow::Result<()> {
    let path = dir.join("metadata.json");

    // Build a JSON object with all relevant fields.
    let mut meta = serde_json::json!({
        "id": detail.id,
        "name": detail.name,
        "category": detail.category,
        "value": detail.value,
        "description": detail.description,
        "solved_by_me": detail.solved_by_me,
    });

    if let Some(ref t) = detail.challenge_type {
        meta["type"] = serde_json::json!(t);
    }
    if let Some(ref s) = detail.state {
        meta["state"] = serde_json::json!(s);
    }
    if let Some(ref c) = detail.connection_info {
        meta["connection_info"] = serde_json::json!(c);
    }
    if let Some(ref f) = detail.files {
        meta["files"] = serde_json::json!(f);
    }
    if let Some(ref t) = detail.tags {
        meta["tags"] = serde_json::json!(t);
    }
    if let Some(ref m) = detail.max_attempts {
        meta["max_attempts"] = serde_json::json!(m);
    }
    if !attachments.is_empty() {
        meta["attachments"] = serde_json::json!(attachments);
    }

    let json = serde_json::to_string_pretty(&meta)
        .map_err(|e| anyhow::anyhow!("Failed to serialize metadata: {e}"))?;

    std::fs::write(&path, json)
        .map_err(|e| anyhow::anyhow!("Failed to write metadata.json: {e}"))?;

    Ok(())
}

/// Best-effort: mark `metadata.json` as solved after a correct submission.
pub(crate) fn mark_local_solved(path: &Path) {
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
