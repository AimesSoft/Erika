//! Durable subject choices share their write lane with movie/episode deliveries.
use super::*;

impl AnimeEngine {
    pub(super) fn queue_subjects(&mut self) {
        for provider in [Provider::Bangumi, Provider::Dandanplay] {
            let Some(session) = self.account_session(provider) else {
                continue;
            };
            let mut tasks = match self.db.catalog_subject_queue(session.account()) {
                Ok(tasks) => tasks,
                Err(error) => {
                    self.state.sync_note = error.message;
                    continue;
                }
            };
            if tasks
                .iter()
                .any(|t| matches!(t.state.as_str(), "pending" | "sending" | "unknown"))
            {
                self.arm_watch_retry();
            }
            tasks.sort_by_key(|t| (t.state != "unknown", t.mark.revision));
            for task in tasks {
                if !matches!(task.state.as_str(), "pending" | "unknown") {
                    continue;
                }
                let key = (task.operation.clone(), task.revision);
                if self.syncs.subject_attempted.contains(&key)
                    || self.syncs.running.iter().chain(self.syncs.waiting.iter()).any(|j|
                        matches!(&j.intent, Intent::Subject(t) if t.operation == task.operation)) { continue; }
                self.syncs.subject_attempted.insert(key);
                self.queue_account(
                    task.mark.media.clone(),
                    session.clone(),
                    task.subject.clone(),
                    Intent::Subject(task),
                );
            }
        }
    }
    pub(super) fn take_subject(&mut self, mut job: Job, reply: Synced, current: bool) {
        let (Intent::Subject(task), Some(claim)) = (job.intent.clone(), job.claim.clone()) else {
            return;
        };
        let finish = |engine: &mut Self, observed, error: &str| {
            let result = engine
                .db
                .catalog_subject_finish(&claim, &task, observed, error);
            if result.is_err() {
                // Another engine may supersede an unsent task. Never release an
                // uncertain write merely because its local choice was replaced.
                let _ = engine
                    .db
                    .catalog_account_release(&claim, task.state != "pending");
            }
            result
        };
        if !current {
            let _ = finish(self, None, "账号或关联已改变");
            self.cancel_account_job(&job);
            return;
        }
        let remote = match reply.result {
            Ok(remote) => remote,
            Err(error) => {
                if let Ok(next) = finish(self, None, &error) {
                    // A failed read does not produce an immediate retry loop.
                    self.syncs
                        .subject_attempted
                        .insert((next.operation, next.revision));
                }
                self.account_error(&job, error);
                return;
            }
        };
        let matches = remote.satisfies(&job.intent);
        if task.state == "pending" && !matches {
            match self
                .db
                .catalog_account_begin(&claim, &Delivery::Subject(task.clone()))
            {
                Ok(Delivery::Subject(next)) => {
                    job.intent = Intent::Subject(next);
                    self.dispatch_account(job, Some(remote));
                    return;
                }
                Ok(_) => unreachable!(),
                Err(error) => {
                    let _ = finish(self, None, &error.message);
                    self.account_error(&job, error.message);
                }
            }
        } else {
            match finish(self, Some(matches), "") {
                Ok(next) => {
                    if next.state == "sent"
                        && self.job_visible(&job)
                        && job.review == self.syncs.review
                        && self.state.review_saving
                        && self
                            .db
                            .catalog_marks(&task.mark.media)
                            .is_ok_and(|marks| marks.contains(&task.mark))
                        && task
                            .mark
                            .review
                            .as_deref()
                            .is_some_and(|text| text == self.state.review_text.trim())
                    {
                        self.state.review_saving = false;
                        self.state.review_open = false;
                        self.state.review_error.clear();
                    }
                    self.syncs.observed.insert(
                        (job.media.clone(), job.subject.reference.provider),
                        Observation { job, remote },
                    );
                }
                Err(error) => self.account_error(&job, error.message),
            }
        }
        self.queue_subjects();
        self.show_opened();
        self.show_watching();
    }
}
