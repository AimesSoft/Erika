use super::*;
use crate::catalog::{CatalogBatch, CatalogReceipt};

impl Db {
    /// Preview runs the same validators and writes in a rolled-back transaction.
    /// A committed request is replayable without advancing any CAS counters.
    pub fn catalog_request(&self, request: &str, input: &CatalogBatch, preview: bool) -> Result<CatalogReceipt> {
        let request = MediaId::parse(request)?.0;
        input.validate()?;
        let tx = transaction(&self.conn)?;
        let previous: Option<(String, String)> = tx.query_row(
            "SELECT input,receipt FROM catalog_request WHERE id=?1", [&request],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        if let Some((old, receipt)) = previous {
            if decode::<CatalogBatch>(old)? != *input {
                return Err(Error::conflict("请求标识已用于另一项修改"));
            }
            return decode(receipt);
        }
        for link in &input.links {
            crate::catalog_requests::mapping_target(link, &require_node(&tx, &link.media)?)?;
        }
        let change = apply(&tx, &[], &input.links, &input.bindings, &input.danmaku)?;
        let mut stmt = tx.prepare("SELECT data FROM provider_link")?;
        let links = stmt.query_map([], |r| r.get::<_, String>(0))?
            .map(|r| decode::<ProviderLink>(r?)).collect::<Result<Vec<_>>>()?;
        drop(stmt);
        crate::catalog_requests::mapping_conflicts(input, &links)?;
        let links = links.into_iter().filter(|l| input.links.iter().any(|i| i.media == l.media && i.reference == l.reference)).collect();
        let receipt = CatalogReceipt { request: request.clone(), change, links };
        if !preview {
            tx.execute("INSERT INTO catalog_request(id,input,receipt) VALUES(?1,?2,?3)",
                params![request, json(input)?, json(&receipt)?])?;
            tx.commit()?;
        }
        Ok(receipt)
    }
    /// Finish a read-only resolution preview or a verified TMDB fetch atomically.
    pub fn catalog_complete_read(&self, id: &str, payload: &str, nodes: &[MediaNode]) -> Result<crate::catalog::CatalogJob> {
        if payload.len() > 4 * 1024 * 1024 { return Err(Error::invalid("任务结果过大")); }
        let tx = transaction(&self.conn)?;
        let mut job = self.catalog_job(id)?.filter(|j| j.state == "running")
            .ok_or_else(|| Error::conflict("任务已失效"))?;
        if job.file.map(|file| revision(&tx, file)).transpose()?.is_some_and(|r| r != job.input_revision) {
            job.state = "stale".into();
            job.error = "文件绑定已改变".into();
        } else {
            let stored = save_nodes(&tx, nodes)?;
            crate::catalog_requests::complete_read(&mut job, payload, &stored)?;
        }
        tx.execute("UPDATE catalog_job SET state=?2,data=?3,lease_until=0 WHERE id=?1", params![id, job.state, json(&job)?])?;
        tx.commit()?;
        Ok(job)
    }
    pub fn catalog_receipt(&self, request: &str) -> Result<Option<CatalogReceipt>> {
        let request = MediaId::parse(request)?.0;
        self.conn.query_row("SELECT receipt FROM catalog_request WHERE id=?1", [request],
            |r| r.get::<_, String>(0)).optional()?.map(decode).transpose()
    }
    /// Explicitly allow recognition again. The independent danmaku stays intact.
    pub fn catalog_allow_auto(&self, file: i64, expected: u64) -> Result<u64> {
        let tx = transaction(&self.conn)?;
        if revision(&tx, file)? != expected {
            return Err(Error::conflict("文件绑定已改变"));
        }
        tx.execute("DELETE FROM file_binding WHERE file_id=?1", [file])?;
        let revision = bump(&tx, file, expected)?;
        tx.execute("UPDATE file SET episode_id=NULL,matched_by=NULL WHERE id=?1", [file])?;
        tx.commit()?;
        Ok(revision)
    }
}
