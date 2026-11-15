//! Downloaded challenge files. Commands and live views share this module so
//! downloads and accepted submissions keep the same on-disk representation.

mod attachments;
pub mod download;
mod metadata;

pub(crate) use metadata::mark_local_solved;

use std::path::PathBuf;

/// Where downloads go when no directory is given: the workspace (folder
/// with `ctfd.toml`), else the current directory.
pub fn default_output() -> PathBuf {
    crate::config::find_workspace_root().unwrap_or_else(|| PathBuf::from("."))
}
