//! Read-only metadata workers share the durable file-job lease/CAS machinery.
//! Only the UI engine commits results; plugin shutdown invalidates its attempt.
use super::*;
use erika_catalog::{MediaId, resolve::FileResolution};
use erika_store::{
    catalog::CatalogJob,
    plugin_sources::{StoredItem, job_source},
};

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "metadata_tests.rs"]
mod tests;

#[derive(Clone)]
pub(in crate::plugins) struct Work {
    pub entry: StoredItem,
    pub job: String,
    pub danmaku_revision: u64,
}
#[derive(Default)]
pub(in crate::plugins) struct Queue {
    // URL -> (owner, earliest retry). Repeated finishScan acknowledgements dedup.
    pending: BTreeMap<String, (String, i64)>,
    active: Option<Work>,
}

impl ErikaEngine {
    pub(crate) fn recognize_source_item(&mut self, url: &str) -> Result<()> {
        if let Some(entry) = self.db.plugin_source_item(url)?
            && self.source_metadata_allowed(&entry.owner)
        {
            self.plugins
                .source_metadata
                .pending
                .insert(entry.url, (entry.owner, 0));
        }
        Ok(())
    }

    pub(in crate::plugins) fn queue_source_metadata(
        &mut self,
        owner: &str,
        source: i64,
    ) -> Result<()> {
        let mut request = erika_store::plugin_sources::page::ListRequest::new(source);
        loop {
            let page = self.db.plugin_source_page(owner, &request)?;
            for entry in page.items {
                self.plugins
                    .source_metadata
                    .pending
                    .insert(entry.url, (owner.into(), 0));
            }
            let Some(cursor) = page.next_cursor else {
                break;
            };
            request.cursor = Some(cursor);
        }
        Ok(())
    }
    pub(in crate::plugins) fn resume_source_metadata(&mut self, owner: &str) {
        let result = (|| -> Result<()> {
            for source in self.db.plugin_source_ids(owner)? {
                self.queue_source_metadata(owner, source)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.plugin_failed(owner, error.message);
        }
    }
    pub(in crate::plugins) fn stop_source_metadata(&mut self, owner: &str) {
        self.plugins
            .source_metadata
            .pending
            .retain(|_, (id, _)| id != owner);
        if self
            .plugins
            .source_metadata
            .active
            .as_ref()
            .is_some_and(|w| w.entry.owner == owner)
        {
            let work = self.plugins.source_metadata.active.take().unwrap();
            let _ = self.db.catalog_job_transition(
                &work.job,
                "running",
                "cancelled",
                "source",
                "来源插件已停止",
                0,
            );
        }
    }
    fn source_metadata_allowed(&self, owner: &str) -> bool {
        self.state.features.plugins
            && self.plugins.enabled(owner)
            && self.plugins.runtimes.contains_key(owner)
            && self
                .plugins
                .find(owner)
                .is_some_and(|p| p.loaded && p.pack.has("library.sources"))
    }
    /// At most one HTTP chain per engine and one lease per file across engines.
    pub(in crate::plugins) fn drive_source_metadata(&mut self) {
        if !self.state.preferences.auto_match || self.plugins.source_metadata.active.is_some() {
            return;
        }
        for _ in 0..16 {
            let now = erika_store::now();
            let next = self
                .plugins
                .source_metadata
                .pending
                .iter()
                .find(|(_, (_, at))| *at <= now)
                .map(|(url, (owner, _))| (url.clone(), owner.clone()));
            let Some((url, owner)) = next else {
                return;
            };
            self.plugins.source_metadata.pending.remove(&url);
            if !self.source_metadata_allowed(&owner) {
                continue;
            }
            let prepared = self.prepare_source_metadata(&owner, &url, now);
            let work = match prepared {
                Ok(Some(work)) => work,
                Ok(None) => continue,
                Err(error) => {
                    // Contention with another engine is not a failed metadata read.
                    if error.code == "conflict" {
                        self.plugins
                            .source_metadata
                            .pending
                            .insert(url, (owner, now + 5));
                    } else {
                        self.plugin_failed(&owner, error.message);
                    }
                    continue;
                }
            };
            self.plugins.source_metadata.active = Some(work.clone());
            let client = self.metadata.clone();
            let inbox = self.plugins.inbox.clone();
            let waker = bern::wake::waker();
            erika_catalog::client::spawn(async move {
                let report = FileResolution {
                    recognition: None,
                    recognition_error: None,
                    resolution: erika_catalog::source::resolve(&client, &work.entry.item.hint())
                        .await,
                };
                inbox
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(Done::SourceMetadata(work, report));
                if let Some(waker) = waker {
                    waker.wake();
                }
            });
            return;
        }
        // Bounded local work also advances catalogues containing many manual binds.
        if let Some(waker) = bern::wake::waker() {
            waker.wake();
        }
    }
    fn prepare_source_metadata(
        &mut self,
        owner: &str,
        url: &str,
        now: i64,
    ) -> Result<Option<Work>> {
        let Some(entry) = self
            .db
            .plugin_source_item(url)?
            .filter(|e| e.owner == owner)
        else {
            return Ok(None);
        };
        // Any established local decision wins over an upstream claim, including
        // explicit Blocked and a previously verified automatic association.
        if self.db.catalog_binding(entry.file)?.is_some() {
            return Ok(None);
        }
        let revision = self.db.catalog_revision(entry.file)?;
        let previous = self.db.catalog_file_job(entry.file, now)?;
        let job = if let Some(job) = previous.filter(|j| j.input_revision == revision) {
            let previous_source = job_source(&job)?;
            let same = previous_source.as_ref() == Some(&entry);
            match job.state.as_str() {
                "pending" | "failed" if same => return Ok(None),
                "running" => return Err(Error::conflict("此文件正在识别")),
                "retry" if same && job.next_attempt > now => {
                    self.plugins
                        .source_metadata
                        .pending
                        .insert(url.into(), (owner.into(), job.next_attempt));
                    return Ok(None);
                }
                "queued" if same => job,
                "queued" | "retry" if previous_source.is_some() && !same => {
                    self.db.catalog_job_transition(
                        &job.id,
                        &job.state,
                        "cancelled",
                        "source",
                        "来源快照已更新",
                        0,
                    )?;
                    self.new_source_metadata_job(&entry, revision)?
                }
                "queued" => return Err(Error::conflict("此文件已有识别任务")),
                _ => self.new_source_metadata_job(&entry, revision)?,
            }
        } else {
            self.new_source_metadata_job(&entry, revision)?
        };
        let claimed = self
            .db
            .catalog_job_transition(&job.id, "queued", "running", "source", "", 0)?;
        if claimed.state != "running" {
            return Ok(None);
        }
        Ok(Some(Work {
            danmaku_revision: self.db.catalog_danmaku_revision(entry.file)?,
            entry,
            job: claimed.id,
        }))
    }
    fn new_source_metadata_job(&self, entry: &StoredItem, revision: u64) -> Result<CatalogJob> {
        let job = CatalogJob {
            id: MediaId::new().0,
            file: Some(entry.file),
            input_revision: revision,
            phase: "source".into(),
            state: "queued".into(),
            payload: json!({"sourceItem":entry}).to_string(),
            error: String::new(),
            next_attempt: 0,
        };
        self.db.catalog_job_enqueue(&job)?;
        Ok(job)
    }
    pub(in crate::plugins) fn source_metadata_finished(
        &mut self,
        work: Work,
        report: FileResolution,
    ) {
        if !self
            .plugins
            .source_metadata
            .active
            .as_ref()
            .is_some_and(|active| active.job == work.job)
        {
            return;
        }
        self.plugins.source_metadata.active = None;
        if !self.state.preferences.auto_match || !self.source_metadata_allowed(&work.entry.owner) {
            let _ = self.db.catalog_job_transition(
                &work.job,
                "running",
                "cancelled",
                "source",
                "来源识别已取消",
                0,
            );
            return;
        }
        match self
            .db
            .catalog_finish_resolution(&work.job, work.danmaku_revision, &report)
        {
            Ok(job) => {
                if job.state == "retry" {
                    self.plugins
                        .source_metadata
                        .pending
                        .insert(work.entry.url, (work.entry.owner, job.next_attempt));
                }
                self.catalogue_changed();
            }
            Err(error) => self.plugin_failed(&work.entry.owner, error.message),
        }
    }
}
