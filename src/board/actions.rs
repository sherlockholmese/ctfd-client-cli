//! Translate board action requests into workspace downloads, details, and
//! submissions, then report their results through the board update channel.

use super::model::{self, Update};
use crate::client::CtfdClient;
use std::collections::HashSet;
use tokio::sync::mpsc::UnboundedSender;

/// Downloads challenge files for the live views, as `ctfdc dl` does: into
/// the workspace, skipping what is already on disk.
#[derive(Clone)]
pub struct Workspace {
    client: std::sync::Arc<CtfdClient>,
}

impl Workspace {
    pub(super) fn new(client: CtfdClient) -> Self {
        Self {
            client: std::sync::Arc::new(client),
        }
    }

    /// Download in the background, reporting the start and the outcome on
    /// `updates`.  Nothing is printed.
    fn start(&self, what: model::Download, updates: UnboundedSender<Update>) {
        let client = self.client.clone();
        tokio::spawn(async move {
            let outcome = Self::run(client, what, &updates).await;
            let _ = updates.send(match outcome {
                Ok((ok, title, detail)) => Update::Downloaded { ok, title, detail },
                Err(e) => Update::Downloaded {
                    ok: false,
                    title: "Download failed".into(),
                    detail: e.to_string().lines().next().unwrap_or_default().to_string(),
                },
            });
        });
    }

    async fn run(
        client: std::sync::Arc<CtfdClient>,
        what: model::Download,
        updates: &UnboundedSender<Update>,
    ) -> anyhow::Result<(bool, String, String)> {
        use crate::workspace::{self, download};
        let mut challenges = client.get_challenges().await?;
        if let model::Download::One(id) = what {
            challenges.retain(|c| c.id == id);
        }
        let name = match (what, challenges.as_slice()) {
            (_, []) => anyhow::bail!("No challenges are visible to download"),
            (model::Download::One(_), [one]) => one.name.clone(),
            (_, all) => format!("{} challenges", all.len()),
        };
        let _ = updates.send(Update::Downloading(format!("Downloading {name}")));
        let asked = challenges.len();
        let client_for_scan = client.clone();
        let report = download::download_challenges(
            client,
            workspace::default_output(),
            challenges,
            download::DownloadOptions {
                quiet: true,
                ..Default::default()
            },
        )
        .await?;
        let dir = std::fs::canonicalize(&report.dir).unwrap_or_else(|_| report.dir.clone());
        let mut detail = format!("{}, in {}", report.details(), dir.display());
        if let Some(first) = report.errors.first() {
            detail = format!("{first}. {detail}");
        }
        // What is on disk now, for the Challenges tab.
        if let Ok(all) = client_for_scan.get_challenges().await {
            let _ = updates.send(Update::OnDisk(Self::on_disk(&all)));
        }
        let title = if report.errors.is_empty() {
            format!("Downloaded {name}")
        } else if asked == 1 {
            format!("Couldn't download {name}")
        } else {
            format!("Downloaded {} of {asked} challenges", report.challenges)
        };
        Ok((report.errors.is_empty(), title, detail))
    }

    fn root() -> std::path::PathBuf {
        crate::workspace::default_output()
    }

    /// Which of `challenges` have a folder with metadata in the workspace.
    pub fn on_disk(challenges: &[crate::models::Challenge]) -> HashSet<u64> {
        let root = Self::root();
        challenges
            .iter()
            .filter(|c| {
                root.join(crate::fs_util::safe_challenge_path(&c.category, &c.name))
                    .join("metadata.json")
                    .is_file()
            })
            .map(|c| c.id)
            .collect()
    }

    async fn detail(client: &CtfdClient, id: u64) -> Result<model::ChallengeInfo, String> {
        let d = client
            .get_challenge_detail(id)
            .await
            .map_err(|e| e.to_string().lines().next().unwrap_or_default().to_string())?;
        // File URLs carry a signed token; only the name is for showing.
        let name_of = |url: &String| {
            let path = url.split(['?', '#']).next().unwrap_or(url);
            path.rsplit('/').next().unwrap_or(path).to_string()
        };
        Ok(model::ChallengeInfo {
            description: d.description,
            connection_info: d.connection_info.filter(|c| !c.trim().is_empty()),
            files: d.files.iter().flatten().map(name_of).collect(),
            tags: d.tags.unwrap_or_default(),
            max_attempts: d.max_attempts.filter(|&n| n > 0),
        })
    }

    async fn submit(
        client: &CtfdClient,
        challenge: Option<crate::models::Challenge>,
        id: u64,
        flag: &str,
    ) -> Update {
        use crate::models::SubmitOutcome;
        match client.submit_flag(id, flag).await {
            Ok(response) => {
                let outcome = response.outcome();
                if matches!(
                    outcome,
                    SubmitOutcome::Accepted | SubmitOutcome::AlreadySolved
                ) && let Some(c) = challenge
                {
                    // Keep a downloaded copy's metadata in step.
                    crate::workspace::mark_local_solved(
                        &Self::root()
                            .join(crate::fs_util::safe_challenge_path(&c.category, &c.name))
                            .join("metadata.json"),
                    );
                }
                Update::Submitted {
                    id,
                    message: response.user_message(),
                    outcome,
                }
            }
            Err(e) => Update::Submitted {
                id,
                outcome: SubmitOutcome::Rejected,
                message: format!(
                    "Not sent: {}",
                    e.to_string().lines().next().unwrap_or_default()
                ),
            },
        }
    }
}

/// Start a download for a live view, or say why there is none to start.
pub fn request_download(
    workspace: Option<&Workspace>,
    what: model::Download,
    updates: &UnboundedSender<Update>,
) {
    match workspace {
        Some(d) => d.start(what, updates.clone()),
        None => {
            let _ = updates.send(Update::Downloaded {
                ok: false,
                title: "Nothing to download".into(),
                detail: "Demo data has no challenge files".into(),
            });
        }
    }
}

/// Fetch a challenge's description and files for a live view.  Demo data
/// gets a stand-in, so the tab can be tried.
pub fn request_detail(workspace: Option<&Workspace>, id: u64, updates: &UnboundedSender<Update>) {
    let updates = updates.clone();
    match workspace {
        Some(w) => {
            let client = w.client.clone();
            tokio::spawn(async move {
                let result = Workspace::detail(&client, id).await.map(Box::new);
                let _ = updates.send(Update::Detail(id, result));
            });
        }
        None => {
            let _ = updates.send(Update::Detail(
                id,
                Ok(Box::new(model::ChallengeInfo {
                    description: "A fictional challenge. On a real CTF its description, \
                                  connection details and files show here."
                        .into(),
                    connection_info: Some("nc demo.ctfd.invalid 31337".into()),
                    files: vec!["handout.zip".into()],
                    ..Default::default()
                })),
            ));
        }
    }
}

/// Send a flag for a live view and report the server's answer.
pub fn request_submit(
    workspace: Option<&Workspace>,
    challenge: Option<crate::models::Challenge>,
    id: u64,
    flag: String,
    updates: &UnboundedSender<Update>,
) {
    let updates = updates.clone();
    match workspace {
        Some(w) => {
            let client = w.client.clone();
            let _ = updates.send(Update::Submitting);
            tokio::spawn(async move {
                let _ = updates.send(Workspace::submit(&client, challenge, id, &flag).await);
            });
        }
        None => {
            let _ = updates.send(Update::Submitted {
                id,
                outcome: crate::models::SubmitOutcome::Rejected,
                message: "Demo data: there is no server to send flags to.".into(),
            });
        }
    }
}
