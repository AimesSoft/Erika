//! Exact episode IDs only. Neither display numbers nor file resume seconds are
//! account identities. Unknown writes are read back before retrying.
use super::*;
use erika_catalog::Watched;

impl AnimeEngine {
    pub(super) fn exact_episode_links(
        &self,
        media: &MediaId,
        provider: Provider,
    ) -> Vec<(ProviderLink, Option<Watched>)> {
        let Ok(Some(node)) = self.db.catalog_node(media) else {
            return Vec::new();
        };
        let Ok(items) = content::items(&self.db, &node) else {
            return Vec::new();
        };
        items
            .into_iter()
            .flat_map(|item| {
                let previous = self.db.catalog_watched(&item.id).ok().flatten();
                let candidates: Vec<_> = self
                    .db
                    .catalog_links(&item.id)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(move |l| {
                        l.confirmed
                            && l.relation == Relation::Exact
                            && l.reference.provider == provider
                            && (l.reference.kind == Kind::Episode
                                || (item.kind == Kind::Movie
                                    && provider == Provider::Bangumi
                                    && l.reference.kind == Kind::Subject))
                    })
                    .collect();
                if candidates.len() == 1 {
                    candidates
                        .into_iter()
                        .map(|l| (l, previous.clone()))
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                }
            })
            .collect()
    }
    /// Import only an account's positive evidence into an absent local record.
    /// An explicit local false (or any later revision) is never unioned away.
    pub(super) fn import_account_watches(&mut self, job: &Job, remote: &Remote) {
        let mut changed = false;
        for (link, previous) in &job.episodes {
            if previous.is_some()
                || remote.watched(&link.reference) != Some(true)
                || !link_current(&self.db, link)
            {
                continue;
            }
            if self
                .db
                .catalog_watched(&link.media)
                .ok()
                .flatten()
                .is_some()
            {
                continue;
            }
            match self.db.catalog_set_watched(&Watched {
                media: link.media.clone(),
                watched: true,
                manual: false,
                updated_at: erika_store::now(),
                revision: 0,
            }) {
                Ok(_) => changed = true,
                Err(error) => self.account_error(job, error.message),
            }
        }
        if changed {
            // The other enabled account need not wait for another file to play.
            // The source account's own outbox preflight observes the value and
            // finishes without issuing a redundant write.
            self.sync_played();
        }
    }
    fn watch_current(&self, task: &WatchSync) -> bool {
        self.db.catalog_watched(&task.media).is_ok_and(|w| {
            w.is_some_and(|w| w.revision == task.local_revision && w.watched == task.watched)
        }) && self.db.catalog_links(&task.media).is_ok_and(|links| {
            links.iter().any(|l| {
                l.confirmed
                    && l.relation == Relation::Exact
                    && l.reference == task.target
                    && l.revision == task.mapping_revision
            })
        })
    }
    fn watch_subject(&self, task: &WatchSync) -> Result<Option<ProviderLink>, String> {
        if task.state == "unknown"
            && let Some(subject) = self
                .db
                .catalog_account_subject(&task.account, &task.operation)
                .map_err(|e| e.message)?
        {
            return Ok(Some(subject));
        }
        let node = self
            .db
            .catalog_node(&task.media)
            .map_err(|e| e.message)?
            .ok_or_else(|| "同步的影片已不存在".to_string())?;
        // An episode's direct parent identifies its season's account subject.
        // No fallback to another season or a series-wide number grid.
        let owner = if node.kind == Kind::Movie {
            node.id
        } else {
            node.parent.ok_or_else(|| "分集缺少所属季".to_string())?
        };
        subject(&self.db, &owner, task.target.provider)
    }
    pub(crate) fn sync_played(&mut self) {
        match self.enqueue_watches() {
            Ok(()) => self.queue_watches(),
            Err(error) => self.state.sync_note = error,
        }
        self.pump_accounts();
    }
    fn enqueue_watches(&mut self) -> Result<(), String> {
        for watched in self.db.catalog_watched_all().map_err(|e| e.message)? {
            for provider in [Provider::Bangumi, Provider::Dandanplay] {
                let Some(session) = self.account_session(provider) else {
                    continue;
                };
                let links = self
                    .db
                    .catalog_links(&watched.media)
                    .map_err(|e| e.message)?;
                let candidates: Vec<_> = links
                    .into_iter()
                    .filter(|l| {
                        l.confirmed
                            && l.relation == Relation::Exact
                            && l.reference.provider == provider
                            && (l.reference.kind == Kind::Episode
                                || (provider == Provider::Bangumi
                                    && l.reference.kind == Kind::Subject))
                    })
                    .collect();
                if candidates.len() != 1 {
                    continue;
                }
                if provider == Provider::Dandanplay && !watched.watched {
                    if self.state.episodes.iter().any(|e| e.id == watched.media.0) {
                        self.state.sync_note = "弹弹play 不能撤销已看；本地未看状态已保留".into();
                    }
                    continue;
                }
                let link = &candidates[0];
                self.db
                    .catalog_watch_enqueue(&WatchSync {
                        operation: String::new(),
                        media: watched.media.clone(),
                        account: session.account().into(),
                        target: link.reference.clone(),
                        watched: watched.watched,
                        local_revision: watched.revision,
                        mapping_revision: link.revision,
                        state: "pending".into(),
                        error: String::new(),
                        remote_baseline: None,
                        revision: 0,
                        lease_until: 0,
                    })
                    .map_err(|e| e.message)?;
            }
        }
        Ok(())
    }
    fn queue_watches(&mut self) {
        for provider in [Provider::Bangumi, Provider::Dandanplay] {
            let Some(session) = self.account_session(provider) else {
                continue;
            };
            if let Err(error) = self
                .db
                .catalog_watch_recover(session.account(), erika_store::now())
            {
                self.state.sync_note = error.message;
                continue;
            }
            let mut tasks = match self.db.catalog_watch_queue(session.account()) {
                Ok(tasks) => tasks,
                Err(error) => {
                    self.state.sync_note = error.message;
                    continue;
                }
            };
            if tasks
                .iter()
                .any(|task| matches!(task.state.as_str(), "pending" | "unknown" | "sending"))
            {
                self.arm_watch_retry();
            }
            tasks.sort_by_key(|t| (t.state != "unknown", t.local_revision));
            for task in &tasks {
                if !matches!(task.state.as_str(), "unknown" | "pending") {
                    continue;
                }
                if task.state == "pending" && !self.watch_current(task) {
                    let _ = self.db.catalog_watch_transition(
                        task,
                        "cancelled",
                        "观看记录或关联已改变",
                        None,
                    );
                    continue;
                }
                if task.state == "pending"
                    && tasks.iter().any(|old| {
                        old.operation != task.operation
                            && old.target == task.target
                            && matches!(old.state.as_str(), "unknown" | "sending")
                    })
                {
                    continue;
                }
                let key = (task.operation.clone(), task.revision);
                if self.syncs.watch_attempted.contains(&key)
                    || self.syncs.running.iter().chain(self.syncs.waiting.iter()).any(|job| {
                        matches!(&job.intent, Intent::Watch(queued) if queued.operation == task.operation)
                    })
                {
                    continue;
                }
                let subject = match self.watch_subject(task) {
                    Ok(Some(subject)) => subject,
                    Ok(None) => {
                        self.state.sync_note = "分集已关联，所属季的账号条目仍待确认".into();
                        continue;
                    }
                    Err(error) => {
                        self.state.sync_note = error;
                        continue;
                    }
                };
                self.syncs.watch_attempted.insert(key);
                self.queue_account(
                    subject.media.clone(),
                    session.clone(),
                    subject,
                    Intent::Watch(task.clone()),
                );
            }
        }
    }
    pub(super) fn take_watch(&mut self, mut job: Job, reply: Synced, current: bool) {
        let (Intent::Watch(task), Some(claim)) = (job.intent.clone(), job.claim.clone()) else {
            return;
        };
        if !current {
            if reply.written && task.state == "sending" {
                let _ = self.db.catalog_account_finish_watch(
                    &claim,
                    &task,
                    "unknown",
                    "账号或关联已改变，需回读发送结果",
                    None,
                );
            } else if task.state == "pending" && !self.watch_current(&task) {
                let _ = self.db.catalog_account_finish_watch(
                    &claim,
                    &task,
                    "cancelled",
                    "账号或关联已改变",
                    None,
                );
            }
            self.cancel_account_job(&job);
            return;
        }
        let remote = match reply.result {
            Ok(remote) => remote,
            Err(error) => {
                if task.state == "sending" {
                    let _ = self
                        .db
                        .catalog_account_finish_watch(&claim, &task, "unknown", &error, None);
                }
                self.account_error(&job, error);
                return;
            }
        };
        let Some(observed) = remote.watched(&task.target) else {
            if task.state == "sending" {
                let _ = self.db.catalog_account_finish_watch(
                    &claim,
                    &task,
                    "unknown",
                    "回读缺少原分集 ID",
                    None,
                );
            }
            self.account_error(&job, "账号条目中没有这个分集 ID，无法确认观看记录".into());
            return;
        };
        let step = if task.state == "sending" {
            self.db.catalog_account_finish_watch(
                &claim,
                &task,
                if observed == task.watched {
                    "sent"
                } else {
                    "unknown"
                },
                "",
                Some(observed),
            )
        } else if task.state == "unknown" {
            let state = if observed == task.watched {
                "sent"
            } else if self.watch_current(&task) {
                "pending"
            } else {
                "conflict"
            };
            self.db
                .catalog_account_finish_watch(&claim, &task, state, "", Some(observed))
        } else {
            // The lane predates the read. Revalidate the task immediately before
            // sending, then finish every callback under that same lane's token.
            self.db
                .catalog_account_begin(&claim, &Delivery::Watch(task.clone()))
                .map(|next| {
                    let Delivery::Watch(next) = next else {
                        unreachable!()
                    };
                    next
                })
        };
        match step {
            Ok(next) if next.state == "sending" => {
                if observed == next.watched {
                    if let Err(error) = self.db.catalog_account_finish_watch(
                        &claim,
                        &next,
                        "sent",
                        "",
                        Some(observed),
                    ) {
                        self.account_error(&job, error.message);
                    }
                    self.queue_watches();
                } else {
                    job.intent = Intent::Watch(next);
                    self.dispatch_account(job, Some(remote));
                }
            }
            Ok(next) if next.state != "unknown" => self.queue_watches(),
            Ok(_) => {}
            Err(error) => self.account_error(&job, error.message),
        }
    }
    /// Recover offline deliveries and expired sends even when no anime view is
    /// mounted (including the headless host). At most one wake is armed, it does
    /// not retain the engine, and it never sends without the normal preflight.
    pub(super) fn arm_watch_retry(&mut self) {
        if self.syncs.watch_timer {
            return;
        }
        self.syncs.watch_timer = true;
        #[cfg(not(test))]
        {
            let inbox = std::sync::Arc::downgrade(&self.inbox);
            let wake = bern::wake::waker();
            erika_catalog::client::spawn(async move {
                #[cfg(not(target_arch = "wasm32"))]
                std::thread::sleep(std::time::Duration::from_secs(60));
                #[cfg(target_arch = "wasm32")]
                gloo_timers::future::TimeoutFuture::new(60_000).await;
                if let Some(inbox) = inbox.upgrade() {
                    inbox
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .push(Done::RetryWatches);
                    if let Some(wake) = wake {
                        wake.wake();
                    }
                }
            });
        }
    }
    pub(crate) fn resume_watch_retry(&mut self) {
        self.syncs.watch_timer = false;
        self.retry_watches();
    }
    pub(crate) fn retry_watches(&mut self) {
        self.syncs.watch_attempted.clear();
        self.syncs.subject_attempted.clear();
        self.sync_stored_marks(None);
        self.sync_played();
    }
}
