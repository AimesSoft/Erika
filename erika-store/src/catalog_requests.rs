//! Shared validation for explicit local media decisions. No account writes.
use crate::catalog::{CatalogBatch, Error, Result};
use erika_catalog::{Kind, MediaNode, Provider, ProviderLink, Relation};

pub(crate) fn complete_read(
    job: &mut crate::catalog::CatalogJob,
    payload: &str,
    stored: &[MediaNode],
) -> Result<()> {
    if payload.len() > 4 * 1024 * 1024 { return Err(Error::invalid("任务结果过大")); }
    let mut value: serde_json::Value = serde_json::from_str(payload)?;
    let fields = value.as_object_mut().ok_or_else(|| Error::invalid("任务结果必须是对象"))?;
    if job.phase == "fetch" {
        fields.insert("nodes".into(), serde_json::to_value(stored)?);
    }
    job.state = "completed".into();
    job.phase = if matches!(job.phase.as_str(), "fetch" | "reuse") { "complete" } else { "preview" }.into();
    job.error.clear();
    job.next_attempt = 0;
    if let Some(report) = fields.get("report") {
        let report: erika_catalog::resolve::FileResolution = serde_json::from_value(report.clone())?;
        if let Err(error) = report.resolution {
            job.state = "failed".into();
            job.error = error.message;
        }
    }
    job.payload = serde_json::to_string(&value)?;
    if job.payload.len() > 4 * 1024 * 1024 { return Err(Error::invalid("任务结果过大")); }
    Ok(())
}

pub(crate) fn mapping_target(link: &ProviderLink, node: &MediaNode) -> Result<()> {
    link.reference.validate()?;
    if matches!(link.reference.provider, Provider::Dandanplay | Provider::Bangumi) {
        let allowed = match link.reference.kind {
            Kind::Subject => matches!(node.kind, Kind::Tv | Kind::Season | Kind::Movie),
            Kind::Episode => node.kind.playable(),
            _ => false,
        };
        if !allowed {
            return Err(Error::invalid("条目关联和分集关联的层级不一致"));
        }
        if (link.reference.provider == Provider::Bangumi || link.reference.kind == Kind::Episode)
            && !link.reference.id.parse::<u64>().ok().is_some_and(|id| id > 0)
        {
            return Err(Error::invalid("提供方条目需要真实的正整数标识"));
        }
    }
    Ok(())
}

pub(crate) fn mapping_conflicts(batch: &CatalogBatch, all: &[ProviderLink]) -> Result<()> {
    for link in batch.links.iter().filter(|l| l.confirmed && l.relation == Relation::Exact) {
        if all.iter().any(|other| {
            other.confirmed && other.relation == Relation::Exact
                && other.reference.provider == link.reference.provider
                && other.reference.scope == link.reference.scope
                && other.reference.kind == link.reference.kind
                && ((other.media == link.media && other.reference != link.reference)
                    || (other.media != link.media && other.reference == link.reference))
        }) {
            return Err(Error::conflict("提供方精确关联冲突，请先撤销旧关联或使用非精确关系"));
        }
    }
    Ok(())
}
