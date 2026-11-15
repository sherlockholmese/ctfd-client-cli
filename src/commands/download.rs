//! Select challenges and report downloads requested by the command line.

use crate::client::CtfdClient;
use crate::config::{DownloadArgs, ResolvedConfig};
use crate::models::Challenge;
use crate::workspace::{
    default_output,
    download::{DownloadOptions, download_challenges},
};
use std::path::Path;
use std::sync::Arc;

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

    let challenges = client.get_challenges().await?;

    if challenges.is_empty() {
        println!("No challenges available to download.");
        return Ok(());
    }

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
    let output_dir = args.output.clone().unwrap_or_else(default_output);
    if args.output.is_none() && output_dir != Path::new(".") {
        let shown = std::fs::canonicalize(&output_dir).unwrap_or_else(|_| output_dir.clone());
        eprintln!("Downloading into workspace {}", shown.display());
    }

    let report = download_challenges(
        client,
        output_dir,
        selected,
        DownloadOptions {
            force: args.force,
            jobs: args.jobs,
            ..Default::default()
        },
    )
    .await?;

    if report.challenges > 0 {
        println!(
            "Downloaded {n} challenge{s} successfully ({details}).",
            n = report.challenges,
            s = plural(report.challenges),
            details = report.details()
        );
    }

    if !report.errors.is_empty() {
        for err in &report.errors {
            eprintln!("Error: {err}");
        }
        anyhow::bail!(
            "{} challenge download{} failed.",
            report.errors.len(),
            plural(report.errors.len())
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

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
