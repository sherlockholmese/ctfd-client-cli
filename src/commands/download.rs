use crate::client::CtfdClient;
use crate::config::ResolvedConfig;
use crate::fs_util;
use std::path::Path;

/// Download all visible challenges to the current directory.
///
/// For each challenge, creates a `category/challenge-name` directory
/// containing `README.md`, `metadata.json`, and any attachment files.
/// Existing generated files are overwritten; unrelated files in
/// existing directories are preserved.  Unsafe category/challenge/filename
/// components are sanitized and path traversal is prevented.
///
/// On success, prints a summary of what was downloaded.
/// On failure (API, attachment, filesystem), reports the error, skips
/// the affected challenge or attachment, and exits non-zero.
pub async fn run(config: ResolvedConfig) -> anyhow::Result<()> {
    let client = CtfdClient::new(&config);

    // 1. Fetch challenge list
    let challenges = client.get_challenges().await?;

    if challenges.is_empty() {
        println!("No challenges available to download.");
        return Ok(());
    }

    let mut errors: Vec<String> = Vec::new();
    let mut downloaded_count = 0u32;

    for ch in &challenges {
        match download_one_challenge(&client, ch.id).await {
            Ok(()) => {
                downloaded_count += 1;
            }
            Err(e) => {
                errors.push(format!(
                    "Challenge {} ({ch_name}): {e}",
                    ch.id,
                    ch_name = ch.name
                ));
            }
        }
    }

    // Summary
    if downloaded_count > 0 {
        println!(
            "Downloaded {downloaded_count} challenge{} successfully.",
            if downloaded_count == 1 { "" } else { "s" }
        );
    }

    if !errors.is_empty() {
        for err in &errors {
            eprintln!("Error: {err}");
        }
        anyhow::bail!(
            "{} challenge download{} failed.",
            errors.len(),
            if errors.len() == 1 { "" } else { "s" }
        );
    }

    Ok(())
}

/// Download a single challenge: fetch detail, create directory, write files.
async fn download_one_challenge(client: &CtfdClient, challenge_id: u64) -> anyhow::Result<()> {
    let detail = client
        .get_challenge_detail(challenge_id)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch challenge detail: {e}"))?;

    let safe_path = fs_util::safe_challenge_path(&detail.category, &detail.name);

    // Create output directory (no error if it already exists).
    std::fs::create_dir_all(&safe_path)
        .map_err(|e| anyhow::anyhow!("Failed to create directory '{safe_path}': {e}"))?;

    // Write README.md
    write_readme(&safe_path, &detail)?;

    // Write metadata.json
    write_metadata(&safe_path, &detail)?;

    // Download attachments
    if let Some(ref files) = detail.files {
        let mut attachment_errors: Vec<String> = Vec::new();
        // Track written filenames to detect collisions.
        let mut written_names: Vec<String> = Vec::new();

        for file_url in files {
            match download_attachment_file(client, &safe_path, file_url, &mut written_names).await {
                Ok(()) => {}
                Err(e) => {
                    attachment_errors.push(format!("Attachment '{file_url}': {e}"));
                }
            }
        }

        if !attachment_errors.is_empty() {
            let msg = attachment_errors.join("; ");
            eprintln!(
                "Warning: Some attachments failed for challenge '{}': {msg}",
                detail.name
            );
            return Err(anyhow::anyhow!("Attachment download errors: {msg}"));
        }
    }

    eprintln!("Downloaded: {safe_path}");
    Ok(())
}

/// Write README.md for a challenge.
fn write_readme(dir: &str, detail: &crate::models::ChallengeDetail) -> anyhow::Result<()> {
    let path = Path::new(dir).join("README.md");

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
fn write_metadata(dir: &str, detail: &crate::models::ChallengeDetail) -> anyhow::Result<()> {
    let path = Path::new(dir).join("metadata.json");

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

    let json = serde_json::to_string_pretty(&meta)
        .map_err(|e| anyhow::anyhow!("Failed to serialize metadata: {e}"))?;

    std::fs::write(&path, json)
        .map_err(|e| anyhow::anyhow!("Failed to write metadata.json: {e}"))?;

    Ok(())
}

/// Download a single attachment file.
///
/// Sanitizes the filename, handles collisions by appending a numeric suffix,
/// and writes the file bytes into the challenge directory.
async fn download_attachment_file(
    client: &CtfdClient,
    dir: &str,
    file_url: &str,
    written_names: &mut Vec<String>,
) -> anyhow::Result<()> {
    let raw_name = fs_util::attachment_filename(file_url);
    let safe_name = disambiguate_filename(&raw_name, written_names);

    let bytes = client
        .download_attachment(file_url)
        .await
        .map_err(|e| anyhow::anyhow!("Download failed: {e}"))?;

    let path = Path::new(dir).join(&safe_name);
    std::fs::write(&path, &bytes)
        .map_err(|e| anyhow::anyhow!("Failed to write attachment '{safe_name}': {e}"))?;

    eprintln!("  Downloaded: {safe_name}");
    written_names.push(safe_name);
    Ok(())
}

/// Disambiguate a filename if it collides with already-written names.
///
/// Appends `_1`, `_2`, etc. before the extension until a unique name is found.
fn disambiguate_filename(name: &str, existing: &[String]) -> String {
    if !existing.iter().any(|e| e == name) {
        return name.to_string();
    }

    // Split into stem and extension.
    let (stem, ext) = match name.rfind('.') {
        Some(dot_pos) => (&name[..dot_pos], &name[dot_pos..]),
        None => (name, ""),
    };

    let mut counter = 1u32;
    loop {
        let candidate = format!("{stem}_{counter}{ext}");
        if !existing.iter().any(|e| e == &candidate) {
            return candidate;
        }
        counter += 1;
    }
}
