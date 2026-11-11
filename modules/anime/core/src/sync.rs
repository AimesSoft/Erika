//! Account operations use canonical media and explicit, versioned mappings.
//! Read and write are separate phases: the engine revalidates the captured
//! identity, account and local intent between them. Remote replies never replace
//! a newer local choice. Episode delivery lives in the durable watch outbox.
mod network;
mod subject;
#[cfg(test)]
mod tests;
mod watch;

use crate::{AnimeEngine, Done, content, marks::Level};
use bern::CommandError;
use erika_anime_protocol::anime::{Score, SyncTargets};
use erika_catalog::{Kind, MediaId, Provider, ProviderLink, Relation};
use erika_dandanplay::{auth::Session, bangumi::site};
use erika_store::{
    Db,
    account_sync::{AccountClaim, Delivery, SubjectSync},
    catalog::{MarkEdit, MediaMark, WatchSync},
};
use std::collections::{HashMap, HashSet, VecDeque};

pub(crate) const REVIEW_LIMIT: usize = 200;
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Intent {
    Read,
    ReadReview,
    Status(Level),
    Rate(i64),
    Review(String),
    Watch(WatchSync),
    Subject(SubjectSync),
}
impl Intent {
    fn field(&self) -> u8 {
        match self {
            Self::Read => 0,
            Self::ReadReview => 1,
            Self::Status(_) => 2,
            Self::Rate(_) => 3,
            Self::Review(_) => 4,
            Self::Watch(_) => 5,
            Self::Subject(_) => 6,
        }
    }
    fn writes(&self) -> bool {
        !matches!(self, Self::Read | Self::ReadReview)
    }
    fn delivery(&self) -> Option<Delivery> {
        match self {
            Self::Watch(task) => Some(Delivery::Watch(task.clone())),
            Self::Subject(task) => Some(Delivery::Subject(task.clone())),
            _ => None,
        }
    }
    fn only_bangumi(&self) -> bool {
        matches!(self, Self::Rate(_) | Self::Review(_) | Self::ReadReview)
    }
}
#[derive(Clone)]
struct Job {
    id: u64,
    media: MediaId,
    session: Session,
    subject: ProviderLink,
    intent: Intent,
    view: u64,
    review: u64,
    claim: Option<AccountClaim>,
    mark: Option<MediaMark>,
    /// Exact episode links and local revisions at the start of this read.
    episodes: Vec<(ProviderLink, Option<erika_catalog::Watched>)>,
}
impl Job {
    fn key(&self) -> (MediaId, Provider, u8) {
        (
            self.media.clone(),
            self.subject.reference.provider,
            self.intent.field(),
        )
    }
}
#[derive(Clone, Debug)]
enum Remote {
    Dandanplay(erika_dandanplay::catalog::Details),
    Bangumi(site::Collection),
}
impl Remote {
    fn level(&self) -> Option<Level> {
        match self {
            Self::Bangumi(c) => c.interest.map(Level::of_interest),
            Self::Dandanplay(d) => match d.favorite_status.as_deref() {
                Some("finished") => Some(Level::Watched),
                Some("abandoned") => Some(Level::Dropped),
                Some("favorited") => Some(Level::Want),
                _ => None,
            },
        }
    }
    fn rating(&self) -> i64 {
        match self {
            Self::Bangumi(c) => c.rating,
            _ => 0,
        }
    }
    fn watched(&self, target: &erika_catalog::ProviderRef) -> Option<bool> {
        match (self, target.provider, target.kind) {
            (Self::Bangumi(c), Provider::Bangumi, Kind::Subject) => {
                Some(c.interest == Some(site::Interest::Collect))
            }
            (Self::Bangumi(c), Provider::Bangumi, Kind::Episode) => {
                let mut matches = c.episodes.iter().filter(|e| e.id.to_string() == target.id);
                let value = matches.next()?.watched;
                matches.next().is_none().then_some(value)
            }
            (Self::Dandanplay(d), Provider::Dandanplay, Kind::Episode) => {
                let mut matches = d
                    .episodes
                    .iter()
                    .filter(|e| e.episode_id.to_string() == target.id);
                let value = matches
                    .next()?
                    .last_watched
                    .as_ref()
                    .is_some_and(|s| !s.trim().is_empty());
                matches.next().is_none().then_some(value)
            }
            _ => None,
        }
    }
    fn satisfies(&self, intent: &Intent) -> bool {
        match intent {
            Intent::Read | Intent::ReadReview => true,
            Intent::Status(level) => match self {
                Self::Bangumi(c) => c.interest == Some(level.interest()),
                Self::Dandanplay(d) => {
                    d.favorite_status.as_deref() == Some(network::follow(*level))
                }
            },
            Intent::Rate(points) => matches!(self, Self::Bangumi(c) if c.rating == *points),
            Intent::Review(text) => {
                matches!(self, Self::Bangumi(c) if c.comment.trim() == text.trim())
            }
            Intent::Watch(w) => self.watched(&w.target) == Some(w.watched),
            Intent::Subject(task) => {
                let mark = &task.mark;
                let status = mark.status.as_deref().and_then(Level::parse);
                match self {
                    Self::Bangumi(c) => {
                        status.is_none_or(|s| c.interest == Some(s.interest()))
                            && mark.rating.is_none_or(|n| c.rating == n)
                            && mark
                                .review
                                .as_ref()
                                .is_none_or(|s| c.comment.trim() == s.trim())
                    }
                    Self::Dandanplay(d) => status
                        .is_some_and(|s| d.favorite_status.as_deref() == Some(network::follow(s))),
                }
            }
        }
    }
}
pub(crate) struct Synced {
    id: u64,
    written: bool,
    result: Result<Remote, String>,
}
struct Observation {
    job: Job,
    remote: Remote,
}
#[derive(Default)]
pub(crate) struct Syncs {
    running: Option<Job>,
    waiting: VecDeque<Job>,
    next: u64,
    versions: HashMap<(MediaId, Provider, u8), u64>,
    observed: HashMap<(MediaId, Provider), Observation>,
    pub opened: Option<MediaId>,
    view: u64,
    review: u64,
    watch_attempted: HashSet<(String, u64)>,
    subject_attempted: HashSet<(String, u64)>,
    watch_timer: bool,
    #[cfg(test)]
    sessions: [Option<Session>; 2],
    #[cfg(test)]
    dispatched: Vec<(Job, Option<Remote>)>,
}
impl Syncs {
    pub fn change_view(&mut self, state: &mut erika_anime_protocol::anime::State) {
        state.view_generation = MediaId::new().0;
        self.view += 1;
        self.review += 1;
        self.opened = None;
    }
    pub fn change_review(&mut self) {
        self.review += 1;
    }
    pub fn busy(&self, media: &MediaId) -> bool {
        self.running.as_ref().is_some_and(|j| &j.media == media)
            || self.waiting.iter().any(|j| &j.media == media)
    }
}

/// A season never silently falls back to the series' account subject. A series
/// subject may cover several seasons, which is not permission to rate one of them.
pub(super) fn subject(
    db: &Db,
    media: &MediaId,
    provider: Provider,
) -> Result<Option<ProviderLink>, String> {
    let links = db.catalog_links(media).map_err(|e| e.message)?;
    let candidates: Vec<_> = links
        .into_iter()
        .filter(|l| {
            l.confirmed
                && l.relation == Relation::Exact
                && l.reference.provider == provider
                && l.reference.kind == Kind::Subject
                && l.reference.scope.is_empty()
        })
        .collect();
    if candidates.len() > 1 {
        return Err("关联了多个账户条目，请先确认对应关系".into());
    }
    let link = candidates.into_iter().next();
    if link.as_ref().is_some_and(|l| !link_current(db, l)) {
        return Err("账号条目对应多个媒体，请先确认对应关系".into());
    }
    Ok(link)
}
fn link_current(db: &Db, link: &ProviderLink) -> bool {
    db.catalog_links(&link.media).is_ok_and(|links| {
        let candidates: Vec<_> = links
            .iter()
            .filter(|l| {
                l.confirmed
                    && l.relation == Relation::Exact
                    && l.reference.provider == link.reference.provider
                    && l.reference.kind == link.reference.kind
                    && l.reference.scope == link.reference.scope
            })
            .collect();
        candidates.as_slice() == [link]
    }) && db
        .catalog_by_ref(&link.reference)
        .is_ok_and(|nodes| nodes.len() == 1 && nodes[0].id == link.media)
}

impl AnimeEngine {
    pub(super) fn account_media(&self) -> Result<MediaId, CommandError> {
        let id = if self.state.selected_season.is_empty() {
            &self.state.opened
        } else {
            &self.state.selected_season
        };
        let id =
            MediaId::parse(id).map_err(|_| CommandError::new("unknown_show", "请先打开作品"))?;
        self.db
            .catalog_node(&id)
            .map_err(content::command)?
            .map(|n| n.id)
            .ok_or_else(|| CommandError::new("unknown_show", "作品已不存在"))
    }
    fn account_session(&self, provider: Provider) -> Option<Session> {
        let index = match provider {
            Provider::Dandanplay if self.accounts.dandanplay => 0,
            Provider::Bangumi if self.accounts.bangumi => 1,
            _ => return None,
        };
        #[cfg(test)]
        {
            self.syncs.sessions[index]
                .clone()
                .filter(Session::is_current)
        }
        #[cfg(not(test))]
        {
            if index == 0 {
                erika_dandanplay::account_session()
            } else {
                site::account_session()
            }
        }
    }
    fn subject_targets(
        &self,
        media: &MediaId,
        intent: &Intent,
    ) -> Result<Vec<(Session, ProviderLink)>, String> {
        let mut targets = Vec::new();
        for provider in [Provider::Bangumi, Provider::Dandanplay] {
            if intent.only_bangumi() && provider != Provider::Bangumi {
                continue;
            }
            if let Some(session) = self.account_session(provider)
                && let Some(link) = subject(&self.db, media, provider)?
            {
                targets.push((session, link));
            }
        }
        Ok(targets)
    }
    pub(crate) fn targets(&self, media: &MediaId) -> SyncTargets {
        let targets = self
            .subject_targets(media, &Intent::Read)
            .unwrap_or_default();
        let has = |p| targets.iter().any(|(_, l)| l.reference.provider == p);
        match (has(Provider::Dandanplay), has(Provider::Bangumi)) {
            (true, true) => SyncTargets::Both,
            (true, false) => SyncTargets::Dandanplay,
            (false, true) => SyncTargets::Bangumi,
            _ => SyncTargets::Local,
        }
    }
    /// Keep explicit local choices immediately. A remote response only updates
    /// an account observation, never this row (including same-second changes).
    pub(crate) fn sync_intent(&mut self, intent: Intent) -> Result<(), CommandError> {
        let media = self.account_media()?;
        if matches!(intent, Intent::ReadReview) {
            let local = self
                .db
                .catalog_marks(&media)
                .map_err(content::command)?
                .into_iter()
                .find(|m| m.reference.is_none());
            if let Some(text) = local.and_then(|m| m.review) {
                self.state.review_text = text;
                self.state.review_loading = false;
                return Ok(());
            }
        }
        let targets = self
            .subject_targets(&media, &intent)
            .map_err(|e| CommandError::new("mapping_required", e))?;
        if intent.only_bangumi() && targets.is_empty() {
            return Err(CommandError::new(
                "bangumi_required",
                "请先登录 Bangumi 并确认此作品或季的对应条目",
            ));
        }
        let edit = match &intent {
            Intent::Status(level) => Some(MarkEdit::Status(level.key().into())),
            Intent::Rate(points) => Some(MarkEdit::Rating(*points)),
            Intent::Review(text) => Some(MarkEdit::Review(text.clone())),
            _ => None,
        };
        self.state.sync_note.clear();
        if let Some(edit) = edit {
            let mark = self
                .db
                .catalog_edit_mark(&media, &edit)
                .map_err(content::command)?;
            // Queue the entire latest local choice, not a stale per-engine row.
            // A later field edit then includes every earlier local field.
            for (session, link) in targets {
                self.db
                    .catalog_subject_enqueue(session.account(), &mark, &link)
                    .map_err(content::command)?;
            }
            self.queue_subjects();
        } else {
            for (session, link) in targets {
                self.queue_account(media.clone(), session, link, intent.clone());
            }
        }
        self.pump_accounts();
        self.show_opened();
        Ok(())
    }
    fn queue_account(
        &mut self,
        media: MediaId,
        session: Session,
        subject: ProviderLink,
        intent: Intent,
    ) {
        self.syncs.next += 1;
        let episodes = if matches!(intent, Intent::Read) {
            self.exact_episode_links(&media, subject.reference.provider)
        } else {
            Vec::new()
        };
        let mark = match self.db.catalog_marks(&media) {
            Ok(marks) => marks.into_iter().find(|m| m.reference.is_none()),
            Err(error) => {
                self.state.sync_note = error.message;
                return;
            }
        };
        let job = Job {
            id: self.syncs.next,
            media,
            session,
            subject,
            intent,
            view: self.syncs.view,
            review: self.syncs.review,
            claim: None,
            mark,
            episodes,
        };
        if job.intent.delivery().is_none() {
            self.syncs.versions.insert(job.key(), job.id);
            self.syncs.waiting.retain(|j| j.key() != job.key());
        }
        self.syncs.waiting.push_back(job);
    }
    fn account_job_current(&self, job: &Job) -> bool {
        job.session.is_current()
            && self
                .account_session(job.subject.reference.provider)
                .is_some_and(|s| s.account() == job.session.account())
            && (job.intent.delivery().is_some_and(|t| t.recovering())
                || link_current(&self.db, &job.subject))
            && match &job.intent {
                Intent::Status(level) => self.db.catalog_marks(&job.media).is_ok_and(|marks| {
                    marks
                        .iter()
                        .any(|m| m.reference.is_none() && m.status.as_deref() == Some(level.key()))
                }),
                Intent::Rate(points) => self.db.catalog_marks(&job.media).is_ok_and(|marks| {
                    marks
                        .iter()
                        .any(|m| m.reference.is_none() && m.rating == Some(*points))
                }),
                _ => true,
            }
            && (job.intent.delivery().is_some()
                || self.syncs.versions.get(&job.key()) == Some(&job.id))
    }
    fn job_visible(&self, job: &Job) -> bool {
        job.view == self.syncs.view && self.account_media().is_ok_and(|id| id == job.media)
    }
    fn dispatch_account(&mut self, job: Job, remote: Option<Remote>) {
        self.syncs.running = Some(job.clone());
        #[cfg(test)]
        self.syncs.dispatched.push((job, remote));
        #[cfg(not(test))]
        {
            let deliver = crate::receiver(&self.inbox);
            erika_catalog::client::spawn(async move {
                let written = remote.is_some();
                let result = match remote {
                    Some(remote) => network::write(&job, remote).await,
                    None => network::read(&job).await,
                };
                deliver(Done::Synced(Synced {
                    id: job.id,
                    written,
                    result,
                }));
            });
        }
    }
    fn pump_accounts(&mut self) {
        while self.syncs.running.is_none()
            && let Some(mut job) = self.syncs.waiting.pop_front()
        {
            if !self.account_job_current(&job) {
                self.cancel_account_job(&job);
                continue;
            }
            if let Some(task) = job.intent.delivery() {
                match self
                    .db
                    .catalog_account_claim(job.session.account(), &job.subject, &task)
                {
                    Ok(claim) => job.claim = Some(claim),
                    Err(error) => {
                        if error.code != "busy" {
                            self.account_error(&job, error.message);
                        }
                        self.arm_watch_retry();
                        continue;
                    }
                }
            }
            self.dispatch_account(job, None);
        }
        self.state.syncing = self.account_media().is_ok_and(|id| self.syncs.busy(&id));
    }
    pub(crate) fn take_synced(&mut self, reply: Synced) {
        let Some(job) = self
            .syncs
            .running
            .as_ref()
            .filter(|j| j.id == reply.id)
            .cloned()
        else {
            return;
        };
        self.syncs.running = None;
        if let Some(claim) = &job.claim
            && !self.db.catalog_account_holds(claim).unwrap_or(false)
        {
            self.arm_watch_retry();
            self.pump_accounts();
            return;
        }
        let current = self.account_job_current(&job);
        if matches!(job.intent, Intent::Subject(_)) {
            self.take_subject(job, reply, current);
            self.pump_accounts();
            return;
        }
        if matches!(job.intent, Intent::Watch(_)) {
            let claim = job.claim.clone();
            self.take_watch(job, reply, current);
            // take_watch retains the claim only when it dispatches the write.
            if self.syncs.running.is_none()
                && let Some(claim) = claim
            {
                let uncertain = self
                    .db
                    .catalog_watch_queue(&claim.account)
                    .map_or(true, |tasks| {
                        tasks.iter().any(|t| {
                            t.operation == claim.operation
                                && matches!(t.state.as_str(), "sending" | "unknown")
                        })
                    });
                let _ = self.db.catalog_account_release(&claim, uncertain);
            }
            self.pump_accounts();
            return;
        }
        if !current {
            self.cancel_account_job(&job);
            self.pump_accounts();
            return;
        }
        match reply.result {
            Ok(remote)
                if !reply.written && job.intent.writes() && !remote.satisfies(&job.intent) =>
            {
                self.dispatch_account(job, Some(remote));
            }
            result => {
                match result {
                    Ok(remote) => {
                        if matches!(job.intent, Intent::Read) {
                            self.import_account_watches(&job, &remote);
                        }
                        if self.job_visible(&job) {
                            if job.review == self.syncs.review && self.state.review_open {
                                match &job.intent {
                                    Intent::ReadReview => {
                                        match self.db.catalog_marks(&job.media) {
                                            Ok(marks) => {
                                                let local = marks
                                                    .into_iter()
                                                    .find(|m| m.reference.is_none());
                                                if let Some(text) =
                                                    local.as_ref().and_then(|m| m.review.as_ref())
                                                {
                                                    self.state.review_text = text.clone();
                                                } else if local == job.mark
                                                    && let Remote::Bangumi(c) = &remote
                                                {
                                                    self.state.review_text = c.comment.clone();
                                                }
                                            }
                                            Err(error) => self.account_error(&job, error.message),
                                        }
                                        self.state.review_loading = false;
                                    }
                                    Intent::Review(_) => {
                                        self.state.review_saving = false;
                                        self.state.review_open = false;
                                        self.state.review_error.clear();
                                    }
                                    _ => {}
                                }
                            }
                        }
                        self.syncs.observed.insert(
                            (job.media.clone(), job.subject.reference.provider),
                            Observation { job, remote },
                        );
                    }
                    Err(error) => self.account_error(&job, error),
                }
                self.show_opened();
                self.show_watching();
                self.pump_accounts();
            }
        }
    }
    fn account_error(&mut self, job: &Job, error: String) {
        if !self.job_visible(job) {
            return;
        }
        self.state.sync_note = error.clone();
        if job.review == self.syncs.review
            && (matches!(job.intent, Intent::Review(_) | Intent::ReadReview)
                || matches!(&job.intent, Intent::Subject(t) if t.mark.review.is_some() && self.state.review_saving))
        {
            self.state.review_loading = false;
            self.state.review_saving = false;
            self.state.review_error = error;
        }
    }
    fn cancel_account_job(&mut self, job: &Job) {
        if job.intent.delivery().is_none() && self.syncs.versions.get(&job.key()) != Some(&job.id) {
            return;
        }
        self.account_error(job, "账号、关联或本地操作已改变；旧请求没有继续发送".into());
    }
    pub(crate) fn show_account_marks(&mut self) {
        let Ok(media) = self.account_media() else {
            return;
        };
        let local = self
            .db
            .catalog_marks(&media)
            .unwrap_or_default()
            .into_iter()
            .find(|m| m.reference.is_none());
        let remote = [Provider::Bangumi, Provider::Dandanplay]
            .into_iter()
            .filter_map(|p| {
                self.syncs
                    .observed
                    .get(&(media.clone(), p))
                    .filter(|o| self.account_job_current(&o.job))
                    .map(|o| &o.remote)
            })
            .collect::<Vec<_>>();
        let status = local
            .as_ref()
            .and_then(|m| m.status.as_deref())
            .and_then(Level::parse)
            .or_else(|| remote.iter().find_map(|r| r.level()));
        let rating = local
            .as_ref()
            .and_then(|m| m.rating)
            .or_else(|| remote.iter().map(|r| r.rating()).find(|n| *n > 0));
        self.state.show.status = status
            .map(|level| Level::status(Some(level)))
            .unwrap_or_else(|| {
                let seen = self.state.episodes.iter().filter(|e| e.watched).count();
                Level::status(if seen == 0 {
                    None
                } else if seen == self.state.episodes.len() {
                    Some(Level::Watched)
                } else {
                    Some(Level::Watching)
                })
            });
        self.state.show.rating = crate::reviews::cached_rating(&self.db, &media);
        self.state.my_score = rating.map_or(Score::Unrated, crate::marks::score);
        self.state.sync_targets = self.targets(&media);
        self.state.syncing = self.syncs.busy(&media);
        self.state.can_rate = self
            .subject_targets(&media, &Intent::Rate(1))
            .is_ok_and(|t| !t.is_empty());
        self.state.can_review = self.state.can_rate;
        self.state.fine_scores = true;
    }
    pub(crate) fn sync_stored_marks(&mut self, only: Option<&MediaId>) {
        let marks = match self.db.catalog_local_marks() {
            Ok(marks) => marks,
            Err(error) => {
                self.state.sync_note = error.message;
                return;
            }
        };
        for mark in marks
            .into_iter()
            .filter(|m| only.is_none_or(|id| id == &m.media))
        {
            match self.subject_targets(&mark.media, &Intent::Read) {
                Ok(targets) => {
                    for (session, link) in targets {
                        if link.reference.provider == Provider::Dandanplay && mark.status.is_none()
                        {
                            continue;
                        }
                        if let Err(error) =
                            self.db
                                .catalog_subject_enqueue(session.account(), &mark, &link)
                        {
                            self.state.sync_note = error.message;
                        }
                    }
                }
                Err(error) => self.state.sync_note = error,
            }
        }
        self.queue_subjects();
        self.pump_accounts();
    }
    pub(crate) fn sync_opened(&mut self) {
        let Ok(media) = self.account_media() else {
            return;
        };
        if self.syncs.opened.as_ref() != Some(&media) {
            self.syncs.opened = Some(media);
            if let Err(error) = self.sync_intent(Intent::Read) {
                self.state.sync_note = error.message;
            }
        }
    }
    pub(crate) fn sync_all_again(&mut self) {
        self.syncs.change_view(&mut self.state);
        self.syncs.observed.clear();
        self.state.review_loading = false;
        self.state.review_saving = false;
        self.state.review_open = false;
        self.state.review_text.clear();
        self.syncs.watch_attempted.clear();
        self.syncs.subject_attempted.clear();
        self.sync_opened();
        self.sync_stored_marks(None);
        self.sync_played();
    }
}
