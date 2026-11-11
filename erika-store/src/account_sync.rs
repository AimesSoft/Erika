//! Device-local account writes. A subject lane is held from preflight through
//! readback, including movie watch writes which edit the same remote collection.
//! These records are neither catalogue content nor portable settings.
use crate::catalog::{Error, MediaMark, Result, WatchSync};
use erika_catalog::{Kind, MediaId, MediaNode, Provider, ProviderLink, ProviderRef, Relation};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubjectSync {
    pub operation: String,
    pub account: String,
    pub subject: ProviderLink,
    pub mark: MediaMark,
    pub state: String,
    pub revision: u64,
    pub error: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountClaim {
    pub account: String,
    pub subject: ProviderLink,
    pub operation: String,
    pub token: String,
    pub lease_until: i64,
    pub uncertain: bool,
    /// Captured under the lane lock; a movie's watch also edits collection status.
    pub movie_mark: Option<MediaMark>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Delivery {
    Subject(SubjectSync),
    Watch(WatchSync),
}
impl Delivery {
    pub fn operation(&self) -> &str {
        match self {
            Self::Subject(t) => &t.operation,
            Self::Watch(t) => &t.operation,
        }
    }
    pub fn recovering(&self) -> bool {
        match self {
            Self::Subject(t) => t.state == "unknown",
            Self::Watch(t) => t.state == "unknown",
        }
    }
    pub fn account(&self) -> &str {
        match self {
            Self::Subject(t) => &t.account,
            Self::Watch(t) => &t.account,
        }
    }
}

pub(crate) trait View {
    fn node(&self, id: &MediaId) -> Result<Option<MediaNode>>;
    fn links(&self, id: &MediaId) -> Result<Vec<ProviderLink>>;
    fn assigned(&self, reference: &ProviderRef) -> Result<Vec<MediaId>>;
    fn mark(&self, id: &MediaId) -> Result<Option<MediaMark>>;
    fn watched(&self, id: &MediaId) -> Result<Option<erika_catalog::Watched>>;
    fn watches(&self, account: &str) -> Result<Vec<WatchSync>>;
    fn transition_watch(
        &mut self,
        task: &WatchSync,
        state: &str,
        error: &str,
        observed: Option<bool>,
    ) -> Result<WatchSync>;
}
pub(crate) fn exact(view: &dyn View, link: &ProviderLink) -> Result<bool> {
    let links = view.links(&link.media)?;
    let candidates: Vec<_> = links
        .iter()
        .filter(|l| {
            l.confirmed
                && l.relation == Relation::Exact
                && l.reference.provider == link.reference.provider
                && l.reference.scope == link.reference.scope
                && l.reference.kind == link.reference.kind
        })
        .collect();
    Ok(link.confirmed
        && link.relation == Relation::Exact
        && candidates.as_slice() == [link]
        && view.assigned(&link.reference)? == [link.media.clone()])
}
fn movie_subject(view: &dyn View, subject: &ProviderLink) -> Result<bool> {
    Ok(subject.reference.provider == Provider::Bangumi
        && view
            .node(&subject.media)?
            .is_some_and(|n| n.kind == Kind::Movie))
}
fn movie_consistent(view: &dyn View, subject: &ProviderLink) -> Result<()> {
    if movie_subject(view, subject)?
        && let Some(status) = view.mark(&subject.media)?.and_then(|m| m.status)
        && let Some(watched) = view.watched(&subject.media)?
        && (status == "watched") != watched.watched
    {
        return Err(Error::conflict(
            "电影收藏状态与已看记录冲突；请统一这两项后再同步 Bangumi",
        ));
    }
    Ok(())
}
fn current(view: &dyn View, task: &SubjectSync) -> Result<bool> {
    Ok(exact(view, &task.subject)? && view.mark(&task.mark.media)?.as_ref() == Some(&task.mark))
}
fn bump(task: &mut SubjectSync) -> Result<()> {
    task.revision = task
        .revision
        .checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or_else(|| Error::conflict("账号同步版本超出范围"))?;
    Ok(())
}
fn terminal(state: &str) -> bool {
    matches!(state, "sent" | "cancelled" | "conflict" | "failed")
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Queue {
    pub tasks: Vec<SubjectSync>,
    lanes: Vec<AccountClaim>,
}
impl Queue {
    pub fn original_subject(&self, operation: &str) -> Option<ProviderLink> {
        self.lanes
            .iter()
            .find(|l| l.operation == operation && l.uncertain)
            .map(|l| l.subject.clone())
    }
    pub fn enqueue(
        &mut self,
        view: &dyn View,
        account: &str,
        mark: &MediaMark,
        subject: &ProviderLink,
    ) -> Result<SubjectSync> {
        let node = view
            .node(&mark.media)?
            .ok_or_else(|| Error::invalid("媒体不存在"))?;
        if account.trim().is_empty()
            || account.len() > 256
            || account.chars().any(char::is_control)
            || mark.reference.is_some()
            || subject.media != mark.media
            || !matches!(node.kind, Kind::Tv | Kind::Season | Kind::Movie)
            || subject.reference.kind != Kind::Subject
            || !subject.reference.scope.is_empty()
            || !matches!(
                subject.reference.provider,
                Provider::Bangumi | Provider::Dandanplay
            )
            || (subject.reference.provider == Provider::Dandanplay && mark.status.is_none())
            || (mark.status.is_none() && mark.rating.is_none() && mark.review.is_none())
        {
            return Err(Error::invalid("收藏同步目标或内容不正确"));
        }
        subject.reference.validate()?;
        let task = SubjectSync {
            operation: MediaId::new().0,
            account: account.into(),
            subject: subject.clone(),
            mark: mark.clone(),
            state: "pending".into(),
            revision: 1,
            error: String::new(),
        };
        if !current(view, &task)? {
            return Err(Error::conflict("收藏记录或精确关联已改变"));
        }
        if let Some(previous) = self
            .tasks
            .iter()
            .find(|t| t.account == account && t.subject == *subject && t.mark == *mark)
        {
            return Ok(previous.clone());
        }
        // Unsent superseded choices cannot later overtake the new one. A write
        // with an uncertain result must instead remain available for readback.
        for old in &mut self.tasks {
            if old.mark.media == mark.media
                && old.subject.reference == subject.reference
                && old.state == "pending"
            {
                old.state = "cancelled".into();
                bump(old)?;
            }
        }
        self.tasks.push(task.clone());
        Ok(task)
    }
    pub fn recover(&mut self, view: &dyn View, account: &str, at: i64) -> Result<()> {
        if at < 0 {
            return Err(Error::invalid("恢复时间不正确"));
        }
        let watches = view.watches(account)?;
        for task in &mut self.tasks {
            let active = self
                .lanes
                .iter()
                .any(|l| l.operation == task.operation && l.lease_until >= at);
            if task.state == "sending" && !active {
                task.state = "unknown".into();
                task.error = "发送中断，需回读远端状态".into();
                bump(task)?;
            }
        }
        self.lanes.retain(|l| {
            if l.lease_until >= at {
                return true;
            }
            let done = self
                .tasks
                .iter()
                .any(|t| t.operation == l.operation && terminal(&t.state))
                || watches
                    .iter()
                    .any(|t| t.operation == l.operation && terminal(&t.state));
            l.uncertain && !done
        });
        Ok(())
    }
    pub fn claim(
        &mut self,
        view: &dyn View,
        account: &str,
        subject: &ProviderLink,
        task: &Delivery,
        at: i64,
    ) -> Result<AccountClaim> {
        self.recover(view, account, at)?;
        MediaId::parse(task.operation())?;
        // Revoking a mapping forbids new writes, not reading back a request
        // already sent to that identity. Only the recorded uncertain operation
        // may use its original mapping; begin() still requires a current link.
        let recovering = task.recovering()
            && self
                .lanes
                .iter()
                .any(|l| l.operation == task.operation() && l.subject == *subject && l.uncertain);
        if task.account() != account
            || subject.reference.kind != Kind::Subject
            || (!recovering && !exact(view, subject)?)
        {
            return Err(Error::conflict("账号条目关联已改变或不唯一"));
        }
        match task {
            Delivery::Subject(t) => {
                if t.subject != *subject
                    || !self.tasks.contains(t)
                    || !matches!(t.state.as_str(), "pending" | "unknown")
                {
                    return Err(Error::conflict("收藏同步任务已改变"));
                }
            }
            Delivery::Watch(t) => {
                if !view.watches(account)?.contains(t)
                    || !matches!(t.state.as_str(), "pending" | "unknown")
                {
                    return Err(Error::conflict("观看同步任务已改变"));
                }
                let node = view
                    .node(&t.media)?
                    .ok_or_else(|| Error::invalid("媒体不存在"))?;
                let owner = if node.kind == Kind::Movie {
                    Some(node.id)
                } else {
                    node.parent
                };
                if owner.as_ref() != Some(&subject.media)
                    || t.target.provider != subject.reference.provider
                {
                    return Err(Error::invalid("观看记录与所属条目不一致"));
                }
            }
        }
        if !task.recovering() {
            movie_consistent(view, subject)?;
        }
        let previous = self
            .lanes
            .iter()
            .find(|l| l.subject.reference == subject.reference);
        if previous.is_some_and(|l| {
            l.lease_until >= at || (l.uncertain && l.operation != task.operation())
        }) {
            return Err(Error {
                code: "busy".into(),
                message: "此账号条目仍有未确认的写入".into(),
            });
        }
        let claim = AccountClaim {
            account: account.into(),
            subject: subject.clone(),
            operation: task.operation().into(),
            token: MediaId::new().0,
            lease_until: at.saturating_add(300),
            uncertain: previous.is_some_and(|l| l.uncertain),
            movie_mark: if let Some(original) = previous.filter(|l| l.uncertain) {
                original.movie_mark.clone()
            } else if movie_subject(view, subject)? {
                view.mark(&subject.media)?
            } else {
                None
            },
        };
        self.lanes
            .retain(|l| l.subject.reference != subject.reference);
        self.lanes.push(claim.clone());
        Ok(claim)
    }
    pub fn holds(&self, claim: &AccountClaim, at: i64) -> bool {
        self.lanes.iter().any(|l| {
            l.token == claim.token
                && l.operation == claim.operation
                && l.account == claim.account
                && l.subject == claim.subject
                && l.movie_mark == claim.movie_mark
                && l.lease_until >= at
        })
    }
    fn require(&self, claim: &AccountClaim) -> Result<()> {
        if !self.holds(claim, crate::now()) {
            return Err(Error::conflict("账号发送执行已失效"));
        }
        Ok(())
    }
    pub fn release(&mut self, claim: &AccountClaim, uncertain: bool) -> Result<()> {
        self.require(claim)?;
        if uncertain {
            let lane = self
                .lanes
                .iter_mut()
                .find(|l| l.token == claim.token)
                .unwrap();
            lane.uncertain = true;
            lane.lease_until = 0;
        } else {
            self.lanes.retain(|l| l.token != claim.token);
        }
        Ok(())
    }
    pub fn begin(
        &mut self,
        view: &mut dyn View,
        claim: &AccountClaim,
        task: &Delivery,
    ) -> Result<Delivery> {
        self.require(claim)?;
        if claim.operation != task.operation()
            || claim.account != task.account()
            || !exact(view, &claim.subject)?
        {
            return Err(Error::conflict("账号或精确关联已改变"));
        }
        movie_consistent(view, &claim.subject)?;
        if movie_subject(view, &claim.subject)?
            && view.mark(&claim.subject.media)? != claim.movie_mark
        {
            return Err(Error::conflict("电影收藏记录在预读后已改变"));
        }
        let next = match task {
            Delivery::Subject(t) => {
                if t.state != "pending" || !current(view, t)? || !self.tasks.contains(t) {
                    return Err(Error::conflict("收藏记录或发送任务已改变"));
                }
                let saved = self
                    .tasks
                    .iter_mut()
                    .find(|p| p.operation == t.operation)
                    .unwrap();
                saved.state = "sending".into();
                bump(saved)?;
                Delivery::Subject(saved.clone())
            }
            Delivery::Watch(t) => {
                // Validate both directions again in the same write transaction.
                let target = view
                    .links(&t.media)?
                    .into_iter()
                    .find(|l| l.reference == t.target && l.revision == t.mapping_revision)
                    .ok_or_else(|| Error::conflict("观看关联已改变"))?;
                if !exact(view, &target)? {
                    return Err(Error::conflict("观看关联不再唯一"));
                }
                let next = view.transition_watch(t, "sending", "", None)?;
                if next.state != "sending" {
                    return Err(Error::conflict("观看记录已改变"));
                }
                Delivery::Watch(next)
            }
        };
        self.lanes
            .iter_mut()
            .find(|l| l.token == claim.token)
            .unwrap()
            .uncertain = true;
        Ok(next)
    }
    pub fn finish_watch(
        &mut self,
        view: &mut dyn View,
        claim: &AccountClaim,
        expected: &WatchSync,
        state: &str,
        error: &str,
        observed: Option<bool>,
    ) -> Result<WatchSync> {
        self.require(claim)?;
        if claim.operation != expected.operation
            || claim.account != expected.account
            || state == "sending"
        {
            return Err(Error::conflict("观看发送执行已失效"));
        }
        let next = view.transition_watch(expected, state, error, observed)?;
        self.release(claim, next.state == "unknown")?;
        Ok(next)
    }
    pub fn finish(
        &mut self,
        view: &dyn View,
        claim: &AccountClaim,
        expected: &SubjectSync,
        observed: Option<bool>,
        error: &str,
    ) -> Result<SubjectSync> {
        self.require(claim)?;
        if claim.operation != expected.operation
            || claim.account != expected.account
            || !self.tasks.contains(expected)
            || error.len() > 8192
        {
            return Err(Error::conflict("收藏同步任务已改变"));
        }
        let is_current = current(view, expected)?;
        let state = match (expected.state.as_str(), observed) {
            ("pending", _) if !is_current => "cancelled",
            ("pending" | "sending" | "unknown", Some(true)) => "sent",
            ("sending", _) => "unknown",
            ("unknown", Some(false)) if is_current => "pending",
            ("unknown", Some(false)) => "conflict",
            ("pending", _) => "pending",
            ("unknown", None) => "unknown",
            _ => return Err(Error::invalid("收藏同步状态转换不正确")),
        };
        let saved = self
            .tasks
            .iter_mut()
            .find(|t| t.operation == expected.operation)
            .unwrap();
        saved.state = state.into();
        saved.error = error.into();
        bump(saved)?;
        let saved = saved.clone();
        self.release(claim, state == "unknown")?;
        Ok(saved)
    }
}

// Both adapters execute the policy under one write transaction / session lock.
macro_rules! account_api {
    () => {
        pub fn catalog_subject_enqueue(
            &self,
            account: &str,
            mark: &$crate::catalog::MediaMark,
            subject: &erika_catalog::ProviderLink,
        ) -> $crate::catalog::Result<$crate::account_sync::SubjectSync> {
            self.edit_account(account, |q, v| q.enqueue(v, account, mark, subject))
        }
        pub fn catalog_subject_queue(
            &self,
            account: &str,
        ) -> $crate::catalog::Result<Vec<$crate::account_sync::SubjectSync>> {
            self.edit_account(account, |q, v| {
                q.recover(v, account, $crate::now())?;
                Ok(q.tasks.clone())
            })
        }
        pub fn catalog_account_subject(
            &self,
            account: &str,
            operation: &str,
        ) -> $crate::catalog::Result<Option<erika_catalog::ProviderLink>> {
            self.edit_account(account, |q, _| Ok(q.original_subject(operation)))
        }
        pub fn catalog_account_claim(
            &self,
            account: &str,
            subject: &erika_catalog::ProviderLink,
            task: &$crate::account_sync::Delivery,
        ) -> $crate::catalog::Result<$crate::account_sync::AccountClaim> {
            self.edit_account(account, |q, v| {
                q.claim(v, account, subject, task, $crate::now())
            })
        }
        pub fn catalog_account_holds(
            &self,
            claim: &$crate::account_sync::AccountClaim,
        ) -> $crate::catalog::Result<bool> {
            self.edit_account(&claim.account, |q, _| Ok(q.holds(claim, $crate::now())))
        }
        pub fn catalog_account_begin(
            &self,
            claim: &$crate::account_sync::AccountClaim,
            task: &$crate::account_sync::Delivery,
        ) -> $crate::catalog::Result<$crate::account_sync::Delivery> {
            self.edit_account(&claim.account, |q, v| q.begin(v, claim, task))
        }
        pub fn catalog_account_release(
            &self,
            claim: &$crate::account_sync::AccountClaim,
            uncertain: bool,
        ) -> $crate::catalog::Result<()> {
            self.edit_account(&claim.account, |q, _| q.release(claim, uncertain))
        }
        pub fn catalog_account_finish_watch(
            &self,
            claim: &$crate::account_sync::AccountClaim,
            expected: &$crate::catalog::WatchSync,
            state: &str,
            error: &str,
            observed: Option<bool>,
        ) -> $crate::catalog::Result<$crate::catalog::WatchSync> {
            self.edit_account(&claim.account, |q, v| {
                q.finish_watch(v, claim, expected, state, error, observed)
            })
        }
        pub fn catalog_subject_finish(
            &self,
            claim: &$crate::account_sync::AccountClaim,
            expected: &$crate::account_sync::SubjectSync,
            observed: Option<bool>,
            error: &str,
        ) -> $crate::catalog::Result<$crate::account_sync::SubjectSync> {
            self.edit_account(&claim.account, |q, v| {
                q.finish(v, claim, expected, observed, error)
            })
        }
    };
}
pub(crate) use account_api;
