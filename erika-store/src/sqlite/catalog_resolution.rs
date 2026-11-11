//! Commit recognition plus the job's terminal state in one transaction.
use super::{
    Db,
    catalog::{apply, binding, decode, json, revision, transaction},
};
use crate::catalog::{CatalogJob, Error, Result};
use erika_catalog::{Decision, resolve::FileResolution};
use rusqlite::{OptionalExtension, params};

impl Db {
    pub fn catalog_job(&self, id: &str) -> Result<Option<CatalogJob>> {
        self.conn
            .query_row("SELECT data FROM catalog_job WHERE id=?1", [id], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .map(decode)
            .transpose()
    }
    /// File revisions invalidate the whole result; danmaku revisions invalidate
    /// only that independently editable choice. No partial result is committed.
    pub fn catalog_finish_resolution(
        &self,
        job_id: &str,
        input_danmaku_revision: u64,
        report: &FileResolution,
    ) -> Result<CatalogJob> {
        let tx = transaction(&self.conn)?;
        let mut job = self
            .catalog_job(job_id)?
            .filter(|j| j.state == "running")
            .ok_or_else(|| Error::conflict("识别任务已失效"))?;
        let file = job.file.ok_or_else(|| Error::invalid("识别任务没有文件"))?;
        let source = crate::plugin_sources::job_source(&job)?;
        let stale_source = if let Some(expected) = source {
            expected.file != file
                || self.plugin_source_item(&expected.url)?.as_ref() != Some(&expected)
        } else {
            false
        };
        if stale_source
            || revision(&tx, file)? != job.input_revision
            || binding(&tx, file)?.is_some_and(|b| b.decision != Decision::Auto)
        {
            job.state = "stale".into();
            job.error = "文件绑定已改变，未应用过期结果".into();
        } else {
            let mut edit = crate::catalog_resolution::plan(
                &mut job,
                input_danmaku_revision,
                self.catalog_danmaku_revision(file)?,
                self.catalog_danmaku(file)?.as_ref(),
                report,
            )?;
            let identities = super::catalog::identities(&tx, &edit.nodes)?;
            let mut links = Vec::new();
            for link in edit.links.drain(..) {
                if !self
                    .catalog_links(&identities.resolve(&link.media))?
                    .iter()
                    .any(|l| l.reference == link.reference)
                {
                    links.push(link);
                }
            }
            apply(&tx, &edit.nodes, &links, &edit.bindings, &edit.danmaku)?;
        }
        tx.execute(
            "UPDATE catalog_job SET state=?2,next_attempt=?3,data=?4 WHERE id=?1",
            params![job.id, job.state, job.next_attempt, json(&job)?],
        )?;
        tx.commit()?;
        Ok(job)
    }
}
