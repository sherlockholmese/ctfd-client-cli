//! Attachment manifests and filename planning. Existing attachment names are
//! stable across runs; files created by the user keep their names.

use crate::fs_util;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// An attachment recorded in `metadata.json` by a previous download.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct AttachmentRecord {
    /// Attachment URL without query string (CTFd signs file URLs with a
    /// per-request `?token=`, so the path is the stable identity).
    pub(super) url: String,
    /// Local filename the attachment was saved as.
    pub(super) file: String,
}

/// A planned attachment download.
#[derive(Debug, PartialEq)]
pub(super) struct PlannedAttachment {
    /// URL as returned by the API (including any signing token).
    pub(super) source_url: String,
    /// Stable identity of the attachment (URL without query/fragment).
    pub(super) key: String,
    /// Sanitized local filename.
    pub(super) file: String,
    /// This exact attachment was recorded by a previous download.
    pub(super) previously_downloaded: bool,
}

/// Decide the local filename for every attachment.
///
/// Attachments seen in a previous run keep their filename.  New
/// attachments get a sanitized name that does not collide with other
/// attachments of this challenge, nor — when a previous manifest exists —
/// with files the user created in the directory.  Names belonging to
/// attachments that disappeared from the challenge (e.g. an updated
/// handout with a new URL) may be reused and overwritten.
pub(super) fn plan_attachments(
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
pub(super) fn read_previous_attachments(dir: &Path) -> Option<Vec<AttachmentRecord>> {
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
}
