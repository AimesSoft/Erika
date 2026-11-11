//! Pure recognition-to-edit policy shared by SQLite and the browser session.
use crate::catalog::{CatalogJob, Error, Result};
use erika_catalog::{
    DanmakuBinding, Decision, FileBinding, MediaNode, ProviderLink, Relation,
    client::ErrorCode,
    resolve::{FileResolution, Resolution},
};

pub(crate) struct ResolutionEdit {
    pub nodes: Vec<MediaNode>,
    pub bindings: Vec<FileBinding>,
    pub links: Vec<ProviderLink>,
    pub danmaku: Vec<DanmakuBinding>,
}
pub(crate) fn plan(
    job: &mut CatalogJob,
    input_danmaku_revision: u64,
    current_danmaku_revision: u64,
    previous: Option<&DanmakuBinding>,
    report: &FileResolution,
) -> Result<ResolutionEdit> {
    let file = job.file.ok_or_else(|| Error::invalid("识别任务没有文件"))?;
    let mut edit = ResolutionEdit {
        nodes: vec![],
        bindings: vec![],
        links: vec![],
        danmaku: vec![],
    };
    let mut media = None;
    match &report.resolution {
        Ok(Resolution::Bound { result }) => {
            edit.nodes = result.nodes();
            edit.bindings.push(FileBinding {
                file,
                decision: Decision::Auto,
                items: result.items.clone(),
                evidence: result.evidence.clone(),
                revision: job.input_revision,
            });
            if result.items.len() == 1 {
                media = Some(result.items[0].media.clone());
            }
            if let Some(recognized) = &report.recognition {
                // Neither a URL nor a shared title proves the provider's scope
                // or numbering. Keep the evidence without enabling account writes.
                for reference in recognized
                    .work
                    .iter()
                    .chain(recognized.bangumi_subject.iter())
                {
                    edit.links.push(ProviderLink {
                        media: result.work.node.id.clone(),
                        reference: reference.clone(),
                        relation: Relation::Related,
                        coverage: vec![],
                        evidence: "文件识别关联，覆盖范围待确认".into(),
                        confirmed: false,
                        manual: false,
                        revision: 0,
                    });
                }
                if let (Some(media), Some(reference)) = (&media, &recognized.episode) {
                    edit.links.push(ProviderLink {
                        media: media.clone(),
                        reference: reference.clone(),
                        relation: Relation::Related,
                        coverage: vec![],
                        evidence: "文件弹幕候选，分集对应待确认".into(),
                        confirmed: false,
                        manual: false,
                        revision: 0,
                    });
                }
            }
            job.state = "bound".into();
            job.phase = "complete".into();
            job.error.clear();
            job.next_attempt = 0;
        }
        Ok(Resolution::Pending { reason, .. }) => {
            job.state = "pending".into();
            job.phase = "confirmation".into();
            job.error = reason.clone();
            job.next_attempt = 0;
        }
        Err(error) => {
            let transient = matches!(error.code, ErrorCode::Unavailable | ErrorCode::RateLimited);
            job.state = if transient { "retry" } else { "failed" }.into();
            job.phase = "metadata".into();
            job.error = error.message.clone();
            job.next_attempt = if transient {
                crate::now().saturating_add(error.retry_after.unwrap_or(60).clamp(1, 86400) as i64)
            } else {
                0
            };
        }
    }
    if let Some(recognized) = &report.recognition {
        if let Some(reference) = &recognized.episode {
            if current_danmaku_revision == input_danmaku_revision
                && !previous.is_some_and(|b| b.manual)
            {
                edit.danmaku.push(DanmakuBinding {
                    file,
                    reference: reference.clone(),
                    media,
                    manual: false,
                    fingerprint_verified: false,
                    shift_seconds: recognized.shift_seconds,
                    user_shift_seconds: previous.map_or(0., |b| b.user_shift_seconds),
                    revision: input_danmaku_revision,
                });
            }
        }
    }
    let input =
        serde_json::from_str::<serde_json::Value>(&job.payload).unwrap_or(serde_json::Value::Null);
    let input = input.get("input").cloned().unwrap_or(input);
    job.payload = serde_json::to_string(&serde_json::json!({"input": input, "result": report}))?;
    Ok(edit)
}
