use super::*;
use crate::catalog::{CatalogBatch, CatalogReceipt};

fn request(s: &mut State, id: &str, input: &CatalogBatch) -> Result<CatalogReceipt> {
    let id = MediaId::parse(id)?.0;
    input.validate()?;
    if let Some((old, receipt)) = s.catalog.requests.get(&id) {
        if old != input { return Err(Error::conflict("请求标识已用于另一项修改")); }
        return Ok(receipt.clone());
    }
    for link in &input.links {
        crate::catalog_requests::mapping_target(link, s.catalog.node(&link.media)?)?;
    }
    let change = s.apply(&[], &input.links, &input.bindings, &input.danmaku)?;
    crate::catalog_requests::mapping_conflicts(input, &s.catalog.links)?;
    let links = s.catalog.links.iter().filter(|l| input.links.iter().any(|i| i.media == l.media && i.reference == l.reference)).cloned().collect();
    let receipt = CatalogReceipt { request: id.clone(), change, links };
    s.catalog.requests.insert(id, (input.clone(), receipt.clone()));
    Ok(receipt)
}
impl Db {
    pub fn catalog_request(&self, id: &str, input: &CatalogBatch, preview: bool) -> Result<CatalogReceipt> {
        if preview {
            self.read(|s| request(&mut s.clone(), id, input))
        } else {
            self.edit(|s| request(s, id, input))
        }
    }
    pub fn catalog_complete_read(&self, id: &str, payload: &str, nodes: &[MediaNode]) -> Result<crate::catalog::CatalogJob> {
        if payload.len() > 4 * 1024 * 1024 { return Err(Error::invalid("任务结果过大")); }
        self.edit(|s| {
            let index = s.catalog.jobs.iter().position(|(j, _)| j.id == id && j.state == "running")
                .ok_or_else(|| Error::conflict("任务已失效"))?;
            let mut job = s.catalog.jobs[index].0.clone();
            if job.file.is_some_and(|f| s.catalog.revision(f) != job.input_revision) {
                job.state = "stale".into();
                job.error = "文件绑定已改变".into();
            } else {
                let stored = s.catalog.save_nodes(nodes)?;
                crate::catalog_requests::complete_read(&mut job, payload, &stored)?;
            }
            s.catalog.jobs[index] = (job.clone(), 0);
            Ok(job)
        })
    }
    pub fn catalog_receipt(&self, id: &str) -> Result<Option<CatalogReceipt>> {
        let id = MediaId::parse(id)?.0;
        Ok(self.read(|s| s.catalog.requests.get(&id).map(|(_, r)| r.clone())))
    }
    pub fn catalog_allow_auto(&self, file: i64, expected: u64) -> Result<u64> {
        self.edit(|s| {
            s.require_file(file)?;
            if s.catalog.revision(file) != expected { return Err(Error::conflict("文件绑定已改变")); }
            s.catalog.bindings.remove(&file);
            s.links.remove(&file);
            s.catalog.bump(file)
        })
    }
}
