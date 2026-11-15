//! Download execution shared by the CLI and live board. Each challenge reports
//! its own failure so the rest can finish; successful files are recorded last.

use super::attachments::{AttachmentRecord, plan_attachments, read_previous_attachments};
use super::metadata::{write_metadata, write_readme};
use crate::client::CtfdClient;
use crate::fs_util;
use crate::models::Challenge;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// How many challenges are downloaded at once unless told otherwise.
pub const DEFAULT_JOBS: u32 = 8;

/// Controls concurrency, replacement, and progress output for a download.
#[derive(Debug, Clone, Copy)]
pub struct DownloadOptions {
    pub force: bool,
    pub jobs: u32,
    pub quiet: bool,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            force: false,
            jobs: DEFAULT_JOBS,
            quiet: false,
        }
    }
}

/// Download `selected` into `output`, `jobs` at a time, and report what
/// happened.  Failures of single challenges are collected in the report
/// rather than stopping the rest.  With `options.quiet`, nothing is printed: the
/// live views call this while they own the screen.
pub async fn download_challenges(
    client: Arc<CtfdClient>,
    output: PathBuf,
    selected: Vec<Challenge>,
    options: DownloadOptions,
) -> anyhow::Result<Report> {
    std::fs::create_dir_all(&output).map_err(|e| {
        anyhow::anyhow!(
            "Failed to create output directory '{}': {e}",
            output.display()
        )
    })?;

    let semaphore = Arc::new(Semaphore::new(options.jobs.max(1) as usize));
    let output = Arc::new(output);
    let mut tasks = JoinSet::new();

    for (index, ch) in selected.into_iter().enumerate() {
        let client = Arc::clone(&client);
        let semaphore = Arc::clone(&semaphore);
        let output = Arc::clone(&output);
        tasks.spawn(async move {
            let _permit = semaphore.acquire_owned().await;
            let result =
                download_one_challenge(&client, &output, ch.id, options.force, options.quiet).await;
            (index, ch, result)
        });
    }

    let mut results = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        results.push(joined.map_err(|e| anyhow::anyhow!("Download task failed: {e}"))?);
    }
    results.sort_by_key(|(index, _, _)| *index);

    let mut report = Report {
        dir: output.as_ref().clone(),
        ..Default::default()
    };
    for (_, ch, result) in results {
        match result {
            Ok(outcome) => report.add(&outcome),
            Err(e) => report
                .errors
                .push(format!("Challenge {} ({}): {e}", ch.id, ch.name)),
        }
    }
    Ok(report)
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

/// What a download did: totals over the challenges that worked, and the
/// errors of those that did not.
#[derive(Debug, Default)]
pub struct Report {
    /// Challenges downloaded without error.
    pub challenges: usize,
    pub new: usize,
    pub fetched: usize,
    pub skipped: usize,
    pub bytes: u64,
    /// One line per challenge that failed.
    pub errors: Vec<String>,
    /// Where the files went.
    pub dir: PathBuf,
}

impl Report {
    fn add(&mut self, outcome: &ChallengeOutcome) {
        self.challenges += 1;
        self.new += usize::from(outcome.is_new);
        self.fetched += outcome.fetched;
        self.skipped += outcome.skipped;
        self.bytes += outcome.bytes;
    }

    /// The totals in words: `2 new, 5 files fetched, 1.2 MB`.
    pub fn details(&self) -> String {
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

/// Download a single challenge: fetch detail, create directory, write files.
async fn download_one_challenge(
    client: &CtfdClient,
    output: &Path,
    challenge_id: u64,
    force: bool,
    quiet: bool,
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
        if !quiet {
            eprintln!(
                "Warning: Some attachments failed for challenge '{}': {msg}",
                detail.name
            );
        }
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
    if !quiet {
        eprintln!("{line}");
    }

    Ok(outcome)
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

    #[test]
    fn human_sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KiB");
        assert_eq!(human_size(3 * 1024 * 1024), "3.0 MiB");
    }
}
