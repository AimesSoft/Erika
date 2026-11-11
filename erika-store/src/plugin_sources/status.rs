//! Owner-scoped, read-only status. Never expose job inputs or another owner's job.
use super::{StoredItem, job_source};
use crate::catalog::{CatalogJob, Result};
use erika_catalog::{
    Decision, FileBinding, Kind, MediaId, ProviderRef,
    resolve::{FileResolution, Resolution},
};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub item: StoredItem,
    pub revision: u64,
    pub binding: Option<FileBinding>,
    pub job: Option<Job>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub state: String,
    pub phase: String,
    pub current: bool,
    pub error: String,
    pub next_attempt: i64,
    pub candidates: Vec<Candidate>,
    pub candidate_count: usize,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: MediaId,
    pub kind: Kind,
    pub title: String,
    pub reference: Option<ProviderRef>,
    pub season_number: Option<u32>,
    pub episode_number: Option<u32>,
}

pub(crate) fn project(
    item: StoredItem,
    revision: u64,
    binding: Option<FileBinding>,
    job: Option<(CatalogJob, i64)>,
    at: i64,
) -> Result<Status> {
    let job = if let Some((mut job, lease)) = job {
        // A shared fingerprint can make two unrelated plugin entries share a file.
        // That does not grant either owner access to the other's payload or errors.
        match job_source(&job)? {
            Some(expected) if expected.owner == item.owner && expected.url == item.url => {
                let applied = job.state == "bound"
                    && job.input_revision.checked_add(1) == Some(revision)
                    && binding
                        .as_ref()
                        .is_some_and(|b| b.decision == Decision::Auto);
                let current = expected == item && (job.input_revision == revision || applied);
                if job.state == "running" && lease < at {
                    job.state = "stale".into();
                    job.error = "识别执行已过期，可重新创建任务".into();
                }
                let mut candidates = Vec::new();
                let mut candidate_count = 0;
                if current && job.state == "pending" {
                    let payload: serde_json::Value = serde_json::from_str(&job.payload)?;
                    if let Some(result) = payload.get("result") {
                        let report: FileResolution = serde_json::from_value(result.clone())?;
                        if let Ok(Resolution::Pending {
                            candidates: nodes, ..
                        }) = report.resolution
                        {
                            candidate_count = nodes.len();
                            candidates = nodes
                                .into_iter()
                                .take(100)
                                .map(|node| Candidate {
                                    id: node.id,
                                    kind: node.kind,
                                    title: node.title,
                                    reference: node.reference,
                                    season_number: node.season_number,
                                    episode_number: node.episode_number,
                                })
                                .collect();
                        }
                    }
                }
                Some(Job {
                    id: job.id,
                    state: job.state,
                    phase: job.phase,
                    current,
                    error: job.error,
                    next_attempt: job.next_attempt,
                    candidates,
                    candidate_count,
                })
            }
            _ => None,
        }
    } else {
        None
    };
    Ok(Status {
        item,
        revision,
        binding,
        job,
    })
}
