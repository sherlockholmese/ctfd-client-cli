use crate::client::CtfdClient;
use crate::config::{DownloadArgs, ResolvedConfig};
use crate::fs_util;
use crate::models::{Challenge, ChallengeDetail};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// Bulk-download visible challenges.
///
/// For each selected challenge, creates a `category/challenge-name`
/// directory under the output directory containing `README.md`,
/// `metadata.json`, and any attachment files.  Challenges are fetched in
/// parallel (`--jobs`).
///
/// Re-running is cheap and safe: attachments recorded in a previous
/// `metadata.json` that still exist on disk are skipped (unless
/// `--force`), `README.md`/`metadata.json` are refreshed, and unrelated
/// files in challenge directories are preserved.  Unsafe
/// category/challenge/filename components are sanitized and path
/// traversal is prevented.
///
/// On failure (API, attachment, filesystem), reports the error, continues
/// with the remaining challenges, and exits non-zero.
pub async fn run(config: ResolvedConfig, args: DownloadArgs) -> anyhow::Result<()> {
    let client = Arc::new(CtfdClient::new(&config));

    // 1. Fetch challenge list
    let challenges = client.get_challenges().await?;

    if challenges.is_empty() {
        println!("No challenges available to download.");
        return Ok(());
    }

    // 2. Apply filters
    let total_visible = challenges.len();
    let selected: Vec<Challenge> = challenges
        .into_iter()
        .filter(|ch| matches_filters(ch, &args))
        .collect();

    if selected.is_empty() {
        anyhow::bail!(
            "No challenges matched the given filters ({total_visible} visible challenge{}).",
            plural(total_visible)
        );
    }

    // Default to the workspace (folder with ctfd.toml) so running from
    // inside a challenge folder doesn't nest downloads.
    let output_dir = args
        .output
        .clone()
        .or_else(crate::config::find_workspace_root)
        .unwrap_or_else(|| PathBuf::from("."));
    if args.output.is_none() && output_dir != Path::new(".") {
        let shown = std::fs::canonicalize(&output_dir).unwrap_or_else(|_| output_dir.clone());
        eprintln!("Downloading into workspace {}", shown.display());
    }

    std::fs::create_dir_all(&output_dir).map_err(|e| {
        anyhow::anyhow!(
            "Failed to create output directory '{}': {e}",
            output_dir.display()
        )
    })?;

    // 3. Download in parallel, bounded by --jobs.
    let semaphore = Arc::new(Semaphore::new(args.jobs as usize));
    let output = Arc::new(output_dir);
    let mut tasks = JoinSet::new();

    for (index, ch) in selected.into_iter().enumerate() {
        let client = Arc::clone(&client);
        let semaphore = Arc::clone(&semaphore);
        let output = Arc::clone(&output);
        let force = args.force;
        tasks.spawn(async move {
            let _permit = semaphore.acquire_owned().await;
            let result = download_one_challenge(&client, &output, ch.id, force).await;
            (index, ch, result)
        });
    }

    let mut results = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        results.push(joined.map_err(|e| anyhow::anyhow!("Download task failed: {e}"))?);
    }
    results.sort_by_key(|(index, _, _)| *index);

    // 4. Summarize
    let mut summary = Summary::default();
    let mut errors: Vec<String> = Vec::new();

    for (_, ch, result) in results {
        match result {
            Ok(outcome) => summary.add(&outcome),
            Err(e) => errors.push(format!("Challenge {} ({}): {e}", ch.id, ch.name)),
        }
    }

    if summary.challenges > 0 {
        println!(
            "Downloaded {n} challenge{s} successfully ({details}).",
            n = summary.challenges,
            s = plural(summary.challenges),
            details = summary.details()
        );
    }

    if !errors.is_empty() {
        for err in &errors {
            eprintln!("Error: {err}");
        }
        anyhow::bail!(
            "{} challenge download{} failed.",
            errors.len(),
            plural(errors.len())
        );
    }

    Ok(())
}

/// Whether a challenge passes the `download` filters.
fn matches_filters(ch: &Challenge, args: &DownloadArgs) -> bool {
    if args.unsolved && ch.solved_by_me {
        return false;
    }

    if !args.categories.is_empty() {
        let category = ch.category.trim().to_lowercase();
        if !args
            .categories
            .iter()
            .any(|c| c.trim().to_lowercase() == category)
        {
            return false;
        }
    }

    if !args.challenges.is_empty() {
        let name = ch.name.to_lowercase();
        return args.challenges.iter().any(|term| {
            let term = term.trim();
            term.parse::<u64>().is_ok_and(|id| id == ch.id) || name.contains(&term.to_lowercase())
        });
    }

    true
}

/// What happened while downloading one challenge.
#[derive(Debug, Default)]
struct ChallengeOutcome {
    /// The challenge directory did not exist (or had no metadata) before.
    is_new: bool,
    /// Attachments fetched from the server during this run.
    fetched: usize,
    /// Attachments already present locally and skipped.
    skipped: usize,
    /// Bytes written for fetched attachments.
    bytes: u64,
}

/// Aggregate totals across all downloaded challenges.
#[derive(Debug, Default)]
struct Summary {
    challenges: usize,
    new: usize,
    fetched: usize,
    skipped: usize,
    bytes: u64,
}

impl Summary {
    fn add(&mut self, outcome: &ChallengeOutcome) {
        self.challenges += 1;
        self.new += usize::from(outcome.is_new);
        self.fetched += outcome.fetched;
        self.skipped += outcome.skipped;
        self.bytes += outcome.bytes;
    }

    fn details(&self) -> String {
        let mut parts = vec![format!("{} new", self.new)];
        parts.push(format!(
            "{} file{} fetched",
            self.fetched,
            plural(self.fetched)
        ));
        if self.skipped > 0 {
            parts.push(format!("{} already present", self.skipped));
        }
        if self.bytes > 0 {
            parts.push(human_size(self.bytes));
        }
        parts.join(", ")
    }
}

/// An attachment recorded in `metadata.json` by a previous download.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AttachmentRecord {
    /// Attachment URL without query string (CTFd signs file URLs with a
    /// per-request `?token=`, so the path is the stable identity).
    url: String,
    /// Local filename the attachment was saved as.
    file: String,
}

/// Download a single challenge: fetch detail, create directory, write files.
async fn download_one_challenge(
    client: &CtfdClient,
    output: &Path,
    challenge_id: u64,
    force: bool,
) -> anyhow::Result<ChallengeOutcome> {
    let detail = client
        .get_challenge_detail(challenge_id)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch challenge detail: {e}"))?;

    let rel_path = fs_util::safe_challenge_path(&detail.category, &detail.name);
    let dir = output.join(&rel_path);
    let display_path = if output == Path::new(".") {
        rel_path.clone()
    } else {
        dir.display().to_string()
    };

    let previous = read_previous_attachments(&dir);
    let mut outcome = ChallengeOutcome {
        is_new: previous.is_none(),
        ..Default::default()
    };

    // Create output directory (no error if it already exists).
    std::fs::create_dir_all(&dir)
        .map_err(|e| anyhow::anyhow!("Failed to create directory '{display_path}': {e}"))?;

    write_readme(&dir, &detail)?;

    // Download attachments
    let files = detail.files.clone().unwrap_or_default();
    let plan = plan_attachments(&dir, &files, previous.as_deref());
    let mut records: Vec<AttachmentRecord> = Vec::new();
    let mut attachment_errors: Vec<String> = Vec::new();

    for item in plan {
        let dest = dir.join(&item.file);
        if !force && item.previously_downloaded && dest.is_file() {
            outcome.skipped += 1;
        } else {
            match client.download_attachment_to(&item.source_url, &dest).await {
                Ok(bytes) => {
                    outcome.fetched += 1;
                    outcome.bytes += bytes;
                }
                Err(e) => {
                    attachment_errors.push(format!("Attachment '{}': {e}", item.file));
                    continue;
                }
            }
        }
        records.push(AttachmentRecord {
            url: item.key,
            file: item.file,
        });
    }

    // Written last so it only records attachments that are really on disk;
    // a failed attachment is retried on the next run.
    write_metadata(&dir, &detail, &records)?;

    if !attachment_errors.is_empty() {
        let msg = attachment_errors.join("; ");
        eprintln!(
            "Warning: Some attachments failed for challenge '{}': {msg}",
            detail.name
        );
        return Err(anyhow::anyhow!("Attachment download errors: {msg}"));
    }

    let status = if outcome.is_new {
        "Downloaded [new]"
    } else if outcome.fetched > 0 {
        "Updated"
    } else {
        "Up to date"
    };
    let mut line = format!("{status}: {display_path}");
    if !files.is_empty() {
        line.push_str(&format!(
            " ({} file{} fetched",
            outcome.fetched,
            plural(outcome.fetched)
        ));
        if outcome.skipped > 0 {
            line.push_str(&format!(", {} already present", outcome.skipped));
        }
        if outcome.bytes > 0 {
            line.push_str(&format!(", {}", human_size(outcome.bytes)));
        }
        line.push(')');
    }
    eprintln!("{line}");

    Ok(outcome)
}

/// A planned attachment download.
#[derive(Debug, PartialEq)]
struct PlannedAttachment {
    /// URL as returned by the API (including any signing token).
    source_url: String,
    /// Stable identity of the attachment (URL without query/fragment).
    key: String,
    /// Sanitized local filename.
    file: String,
    /// This exact attachment was recorded by a previous download.
    previously_downloaded: bool,
}

/// Decide the local filename for every attachment.
///
/// Attachments seen in a previous run keep their filename.  New
/// attachments get a sanitized name that does not collide with other
/// attachments of this challenge, nor — when a previous manifest exists —
/// with files the user created in the directory.  Names belonging to
/// attachments that disappeared from the challenge (e.g. an updated
/// handout with a new URL) may be reused and overwritten.
fn plan_attachments(
    dir: &Path,
    files: &[String],
    previous: Option<&[AttachmentRecord]>,
) -> Vec<PlannedAttachment> {
    let previous = previous.unwrap_or_default();
    let prev_by_key: HashMap<&str, &str> = previous
        .iter()
        .map(|r| (r.url.as_str(), r.file.as_str()))
        .collect();
    let prev_names: HashSet<&str> = previous.iter().map(|r| r.file.as_str()).collect();
    let current_keys: HashSet<String> = files.iter().map(|u| attachment_key(u)).collect();

    // Names still owned by attachments that remain in this challenge.
    let mut taken: HashSet<String> = previous
        .iter()
        .filter(|r| current_keys.contains(&r.url))
        .map(|r| r.file.clone())
        .collect();
    let mut assigned: HashSet<String> = HashSet::new();
    let has_manifest = !previous.is_empty();

    let mut plan = Vec::new();
    for url in files {
        let key = attachment_key(url);

        if let Some(&file) = prev_by_key.get(key.as_str())
            && !assigned.contains(file)
        {
            assigned.insert(file.to_string());
            plan.push(PlannedAttachment {
                source_url: url.clone(),
                key,
                file: file.to_string(),
                previously_downloaded: true,
            });
            continue;
        }

        let raw_name = fs_util::attachment_filename(url);
        let is_free = |name: &str| {
            !taken.contains(name)
                && !assigned.contains(name)
                && !(has_manifest && !prev_names.contains(name) && dir.join(name).exists())
        };
        let file = disambiguate_filename(&raw_name, is_free);
        taken.insert(file.clone());
        assigned.insert(file.clone());
        plan.push(PlannedAttachment {
            source_url: url.clone(),
            key,
            file,
            previously_downloaded: false,
        });
    }
    plan
}

/// Stable identity of an attachment URL: strip query string and fragment.
fn attachment_key(url: &str) -> String {
    let no_query = url.split('?').next().unwrap_or(url);
    no_query.split('#').next().unwrap_or(no_query).to_string()
}

/// Read attachment records from an existing `metadata.json`.
///
/// Returns `None` when there is no readable metadata (a new challenge
/// directory) and `Some(records)` otherwise — possibly empty for
/// metadata written by older versions.
fn read_previous_attachments(dir: &Path) -> Option<Vec<AttachmentRecord>> {
    let content = std::fs::read_to_string(dir.join("metadata.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    let records = value
        .get("attachments")
        .and_then(|a| serde_json::from_value::<Vec<AttachmentRecord>>(a.clone()).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|r| {
            // Never trust a manifest entry that could escape the directory.
            fs_util::sanitize_name(&r.file) == r.file
        })
        .collect();
    Some(records)
}

/// Write README.md for a challenge.
fn write_readme(dir: &Path, detail: &ChallengeDetail) -> anyhow::Result<()> {
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
fn write_metadata(
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

/// Disambiguate a filename until `is_free` accepts it.
///
/// Appends `_1`, `_2`, etc. before the extension until a free name is found.
fn disambiguate_filename(name: &str, is_free: impl Fn(&str) -> bool) -> String {
    if is_free(name) {
        return name.to_string();
    }

    // Split into stem and extension.
    let (stem, ext) = match name.rfind('.') {
        Some(dot_pos) if dot_pos > 0 => (&name[..dot_pos], &name[dot_pos..]),
        _ => (name, ""),
    };

    let mut counter = 1u32;
    loop {
        let candidate = format!("{stem}_{counter}{ext}");
        if is_free(&candidate) {
            return candidate;
        }
        counter += 1;
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Format a byte count for humans (e.g. `1.5 MiB`).
fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(url: &str, file: &str) -> AttachmentRecord {
        AttachmentRecord {
            url: url.into(),
            file: file.into(),
        }
    }

    #[test]
    fn key_strips_signing_token() {
        assert_eq!(
            attachment_key("/files/abc/handout.zip?token=xyz"),
            "/files/abc/handout.zip"
        );
    }

    #[test]
    fn plan_reuses_previous_names() {
        let tmp = tempfile::tempdir().unwrap();
        let prev = vec![record("/files/a/x.zip", "x.zip")];
        let plan = plan_attachments(
            tmp.path(),
            &["/files/a/x.zip?token=new".into()],
            Some(&prev),
        );
        assert_eq!(plan[0].file, "x.zip");
        assert!(plan[0].previously_downloaded);
    }

    #[test]
    fn plan_overwrites_replaced_attachment_name() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("x.zip"), "old").unwrap();
        let prev = vec![record("/files/a/x.zip", "x.zip")];
        let plan = plan_attachments(tmp.path(), &["/files/b/x.zip".into()], Some(&prev));
        assert_eq!(plan[0].file, "x.zip");
        assert!(!plan[0].previously_downloaded);
    }

    #[test]
    fn plan_does_not_clobber_user_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("notes.txt"), "mine").unwrap();
        let prev = vec![record("/files/a/x.zip", "x.zip")];
        let plan = plan_attachments(
            tmp.path(),
            &["/files/a/x.zip".into(), "/files/c/notes.txt".into()],
            Some(&prev),
        );
        assert_eq!(plan[1].file, "notes_1.txt");
    }

    #[test]
    fn plan_disambiguates_duplicate_names() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = plan_attachments(
            tmp.path(),
            &["/files/a/x.zip".into(), "/files/b/x.zip".into()],
            None,
        );
        assert_eq!(plan[0].file, "x.zip");
        assert_eq!(plan[1].file, "x_1.zip");
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KiB");
        assert_eq!(human_size(3 * 1024 * 1024), "3.0 MiB");
    }
}
