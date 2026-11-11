//! Session recognition jobs retain the native CAS/lease policy. Network reads
//! run outside the store; applying their result is one rollback-capable edit.
use super::*;
use crate::catalog::CatalogJob;
use erika_catalog::resolve::FileResolution;
fn state_valid(state: &str) -> bool {
    matches!(
        state,
        "queued"
            | "running"
            | "retry"
            | "pending"
            | "bound"
            | "completed"
            | "failed"
            | "cancelled"
            | "stale"
    )
}
impl super::Catalog {
    pub(in crate::session) fn source_status(
        &self,
        item: crate::plugin_sources::StoredItem,
    ) -> Result<crate::plugin_sources::status::Status> {
        crate::plugin_sources::status::project(
            item.clone(),
            self.revision(item.file),
            self.bindings.get(&item.file).cloned(),
            self.jobs
                .iter()
                .rev()
                .find(|(j, _)| j.file == Some(item.file))
                .cloned(),
            crate::now(),
        )
    }
}
impl Db {
    pub fn catalog_job_enqueue(&self, job: &CatalogJob) -> Result<()> {
        MediaId::parse(&job.id)?;
        if job.state != "queued"
            || job.payload.len() > 1024 * 1024
            || job.phase.len() > 128
            || job.error.len() > 8192
            || job.next_attempt < 0
        {
            return Err(Error::invalid("识别任务格式不正确"));
        }
        self.edit(|s| {
            if s.catalog.jobs.iter().any(|(j, _)| j.id == job.id) {
                return Err(Error::conflict("识别任务已存在"));
            }
            if let Some(file) = job.file {
                s.require_file(file)?;
                if s.catalog.revision(file) != job.input_revision {
                    return Err(Error::conflict("任务所用文件版本已改变"));
                }
                let at = crate::now();
                for (previous, lease) in &mut s.catalog.jobs {
                    if previous.file != Some(file)
                        || !matches!(previous.state.as_str(), "queued" | "running" | "retry")
                    {
                        continue;
                    }
                    if previous.input_revision == job.input_revision
                        && (previous.state == "queued"
                            || (previous.state == "running" && *lease >= at)
                            || (previous.state == "retry" && previous.next_attempt > at))
                    {
                        return Err(Error::conflict("此文件已有识别任务"));
                    }
                    previous.state = "stale".into();
                    previous.error = "识别执行已被新任务替代".into();
                    *lease = 0;
                }
            }
            s.catalog.jobs.push((job.clone(), 0));
            Ok(())
        })
    }
    pub fn catalog_job(&self, id: &str) -> Result<Option<CatalogJob>> {
        Ok(self.read(|s| {
            s.catalog
                .jobs
                .iter()
                .find(|(j, _)| j.id == id)
                .map(|(j, _)| j.clone())
        }))
    }
    pub fn catalog_file_job(&self, file: i64, at: i64) -> Result<Option<CatalogJob>> {
        self.edit(|s| {
            let Some((j, lease)) = s
                .catalog
                .jobs
                .iter_mut()
                .rev()
                .find(|(j, _)| j.file == Some(file))
            else {
                return Ok(None);
            };
            if j.state == "running" && *lease < at {
                j.state = "stale".into();
                j.error = "识别执行已过期，可重新创建任务".into();
            }
            Ok(Some(j.clone()))
        })
    }
    pub fn catalog_jobs(&self, state: Option<&str>) -> Result<Vec<CatalogJob>> {
        if state.is_some_and(|s| !state_valid(s)) {
            return Err(Error::invalid("未知任务状态"));
        }
        Ok(self.read(|s| {
            let mut rows: Vec<_> = s
                .catalog
                .jobs
                .iter()
                .filter(|(j, _)| state.is_none_or(|s| j.state == s))
                .map(|(j, _)| j.clone())
                .collect();
            rows.sort_by(|a, b| {
                a.next_attempt
                    .cmp(&b.next_attempt)
                    .then_with(|| a.id.cmp(&b.id))
            });
            rows.truncate(1000);
            rows
        }))
    }
    pub fn catalog_job_transition(
        &self,
        id: &str,
        expected: &str,
        state: &str,
        phase: &str,
        error: &str,
        next_attempt: i64,
    ) -> Result<CatalogJob> {
        if !matches!(
            (expected, state),
            ("queued" | "retry", "running" | "cancelled")
                | (
                    "running",
                    "retry" | "pending" | "failed" | "cancelled" | "stale"
                )
                | ("pending", "queued" | "cancelled")
                | ("failed", "queued")
        ) || phase.len() > 128
            || error.len() > 8192
            || next_attempt < 0
        {
            return Err(Error::invalid("识别任务状态转换不正确"));
        }
        self.edit(|s| {
            let index = s
                .catalog
                .jobs
                .iter()
                .position(|(j, _)| j.id == id && j.state == expected)
                .ok_or_else(|| Error::conflict("任务已被其他执行者修改"))?;
            let mut j = s.catalog.jobs[index].0.clone();
            if state == "running" && j.next_attempt > crate::now() {
                return Err(Error::conflict("任务尚未到重试时间"));
            }
            j.state = state.into();
            j.phase = phase.into();
            j.error = error.into();
            j.next_attempt = next_attempt;
            if j.file
                .is_some_and(|f| s.catalog.revision(f) != j.input_revision)
            {
                j.state = "stale".into();
                j.error = "文件绑定已改变".into();
            } else if j.file.is_some()
                && matches!(j.state.as_str(), "queued" | "running")
                && s.catalog.jobs.iter().any(|(other, lease)| {
                    other.id != j.id
                        && other.file == j.file
                        && (other.state == "queued"
                            || (other.state == "running" && *lease >= crate::now()))
                })
            {
                return Err(Error::conflict("此文件已有识别任务"));
            }
            let lease = if j.state == "running" {
                crate::now().saturating_add(300)
            } else {
                0
            };
            s.catalog.jobs[index] = (j.clone(), lease);
            Ok(j)
        })
    }
    pub fn catalog_finish_resolution(
        &self,
        id: &str,
        dm_revision: u64,
        report: &FileResolution,
    ) -> Result<CatalogJob> {
        self.edit(|s| {
            let index = s
                .catalog
                .jobs
                .iter()
                .position(|(j, _)| j.id == id && j.state == "running")
                .ok_or_else(|| Error::conflict("识别任务已失效"))?;
            let mut j = s.catalog.jobs[index].0.clone();
            let file = j.file.ok_or_else(|| Error::invalid("识别任务没有文件"))?;
            let source = crate::plugin_sources::job_source(&j)?;
            let stale_source = source.is_some_and(|expected| {
                expected.file != file || s.plugin_item(&expected.url).as_ref() != Some(&expected)
            });
            if stale_source
                || s.catalog.revision(file) != j.input_revision
                || s.catalog
                    .bindings
                    .get(&file)
                    .is_some_and(|b| b.decision != Decision::Auto)
            {
                j.state = "stale".into();
                j.error = "文件绑定已改变，未应用过期结果".into();
            } else {
                let mut edit = crate::catalog_resolution::plan(
                    &mut j,
                    dm_revision,
                    s.catalog.danmaku_revision(file),
                    s.catalog.danmaku.get(&file),
                    report,
                )?;
                let identities = s.catalog.identities(&edit.nodes)?;
                edit.links.retain(|l| {
                    !s.catalog.links.iter().any(|old| {
                        old.media == identities.resolve(&l.media) && old.reference == l.reference
                    })
                });
                s.apply(&edit.nodes, &edit.links, &edit.bindings, &edit.danmaku)?;
            }
            s.catalog.jobs[index] = (j.clone(), 0);
            Ok(j)
        })
    }
}
