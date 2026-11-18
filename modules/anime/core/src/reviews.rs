//! Reviews come only from Bangumi. A missing mapping is not a request to
//! substitute another provider's reviews, and stale replies cannot change views.
//! Reading them never grants permission to write an account.
use super::*;
use erika_catalog::{Kind, MediaId, Provider, ProviderLink, Relation};
use erika_dandanplay::reviews::{self as api, Review};
use std::collections::HashSet;

const AUTO_BRIDGE_EVIDENCE: &str = "弹弹/Bangumi 自动桥接，待确认";

pub(super) struct Finished {
    generation: u64,
    id: String,
    target: Option<MediaId>,
    bridged: Option<erika_dandanplay::catalog::Bridge>,
    subject: Option<i64>,
    result: Result<Vec<Review>, erika_dandanplay::Error>,
    subject_info: Result<erika_dandanplay::bangumi::Subject, erika_dandanplay::Error>,
}
#[derive(Default)]
pub(super) struct Runtime {
    generation: u64,
    attempted_bridge: HashSet<MediaId>,
}
impl Runtime {
    pub fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }
    pub fn reset_bridge(&mut self) {
        self.attempted_bridge.clear();
    }
}
/// Public reviews, scores and fallback Chinese summaries need no account
/// permission. They come from the confirmed link, or else from the single
/// unconfirmed subject recorded on exactly this media (the season a calendar
/// entry airs as). Writes still require a confirmed link (`sync::subject`); a
/// revoked link is never read.
pub(super) fn reading_provider_subject(
    db: &erika_store::Db,
    media: &MediaId,
    provider: Provider,
) -> Option<i64> {
    let positive = |id: &str| id.parse::<i64>().ok().filter(|id| *id > 0);
    match sync::subject(db, media, provider) {
        Ok(Some(link)) => return positive(&link.reference.id),
        Ok(None) => {}
        Err(_) => return None,
    }
    let links = db.catalog_links(media).ok()?;
    let mut ids: Vec<&str> = links
        .iter()
        .filter(|l| {
            !l.confirmed
                && !l.manual
                && l.reference.provider == provider
                && l.reference.kind == erika_catalog::Kind::Subject
                && l.reference.scope.is_empty()
        })
        .map(|l| l.reference.id.as_str())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    match ids.as_slice() {
        [id] => positive(id),
        _ => None,
    }
}
pub(super) fn reading_subject(db: &erika_store::Db, media: &MediaId) -> Option<i64> {
    reading_provider_subject(db, media, Provider::Bangumi)
}
pub(super) fn cached_rating(db: &erika_store::Db, media: &MediaId) -> f64 {
    reading_subject(db, media)
        .and_then(|subject| db.setting(&format!("bangumi.rating.{subject}")))
        .and_then(|text| text.parse::<f64>().ok())
        .filter(|rating| rating.is_finite() && (0.0..=10.0).contains(rating))
        .unwrap_or(0.0)
}
pub(super) fn cached_season_meta(
    db: &erika_store::Db,
    media: &MediaId,
) -> (Option<String>, Option<String>, Option<String>) {
    let Some(subject) = reading_subject(db, media) else {
        return (None, None, None);
    };
    let clean = |key: String| {
        db.setting(&key).and_then(|v| {
            let trimmed = v.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
    };
    (
        clean(format!("bangumi.title_ja.{subject}")),
        clean(format!("bangumi.title_cn.{subject}")),
        clean(format!("bangumi.cover.{subject}")).filter(|u| u.starts_with("https://")),
    )
}
pub(super) fn save_bangumi_subject(
    db: &erika_store::Db,
    subject: i64,
    info: &erika_dandanplay::bangumi::Subject,
) -> bool {
    let mut changed = false;
    if info.rating.is_finite() && (0.0..=10.0).contains(&info.rating) {
        db.set_setting(
            &format!("bangumi.rating.{subject}"),
            Some(&info.rating.to_string()),
        );
    }
    if !info.name.trim().is_empty() {
        db.set_setting(
            &format!("bangumi.title_ja.{subject}"),
            Some(info.name.trim()),
        );
        changed = true;
    }
    if !info.name_cn.trim().is_empty() {
        db.set_setting(
            &format!("bangumi.title_cn.{subject}"),
            Some(info.name_cn.trim()),
        );
        changed = true;
    }
    if info.image_url.trim().starts_with("https://") {
        db.set_setting(
            &format!("bangumi.cover.{subject}"),
            Some(info.image_url.trim()),
        );
        changed = true;
    }
    if !info.summary.trim().is_empty() {
        db.set_setting(
            &format!("bangumi.summary.{subject}"),
            Some(info.summary.trim()),
        );
        changed = true;
    }
    changed
}
/// Cached provider synopses `(Bangumi, dandanplay)` for `media`. Callers rank
/// Chinese text ahead of untranslated Japanese blurbs copied onto Bangumi.
pub(super) fn cached_summaries(db: &erika_store::Db, media: &MediaId) -> [Option<String>; 2] {
    [
        (Provider::Bangumi, "bangumi.summary"),
        (Provider::Dandanplay, "dandanplay.summary"),
    ]
    .map(|(provider, prefix)| {
        let id = reading_provider_subject(db, media, provider)?;
        let text = db.setting(&format!("{prefix}.{id}"))?;
        let trimmed = text.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}
impl AnimeEngine {
    pub(super) fn review_subject(&self) -> Option<i64> {
        let media = self.account_media().ok()?;
        if let Some(id) = reading_subject(&self.db, &media) {
            return Some(id);
        }
        // Single-season works recognized from local files store their
        // unconfirmed Bangumi link on the root work node.
        let regular_seasons = self.state.seasons.iter().filter(|s| s.number > 0).count();
        if regular_seasons <= 1
            && let Ok(root) = MediaId::parse(&self.state.opened)
            && root != media
        {
            return reading_subject(&self.db, &root);
        }
        None
    }
    pub(super) fn load_reviews(&mut self) {
        self.reviews.cancel();
        self.state.comments.clear();
        self.state.has_comments = false;
        self.state.comments_loading = false;
        self.state.comments_source = "Bangumi".into();
        self.state.comments_error.clear();
        if let Some(subject) = self.review_subject() {
            if let Some(rows) = self
                .db
                .setting(&format!("bangumi.reviews.{subject}"))
                .and_then(|text| serde_json::from_str::<Vec<Review>>(&text).ok())
            {
                self.show_reviews(&rows);
            }
            if !self.metadata.network_enabled() {
                return;
            }
            self.state.comments_loading = true;
            let generation = self.reviews.generation;
            let id = self.state.opened.clone();
            let inbox = self.inbox.clone();
            let wake = bern::wake::waker();
            erika_catalog::client::spawn(async move {
                let result = api::bangumi(subject).await;
                let subject_info = erika_dandanplay::bangumi::subject(subject).await;
                inbox
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(Done::Reviews(Finished {
                        generation,
                        id,
                        target: None,
                        bridged: None,
                        subject: Some(subject),
                        result,
                        subject_info,
                    }));
                if let Some(wake) = wake {
                    wake.wake();
                }
            });
            return;
        }
        // No stored Bangumi link yet: automatically bridge via dandanplay /
        // Bangumi unless the user explicitly dismissed Bangumi on this target.
        if !self.metadata.network_enabled() {
            return;
        }
        let Ok(target) = self.account_media() else {
            return;
        };
        let Ok(Some(work_node)) = MediaId::parse(&self.state.opened)
            .ok()
            .map(|root| self.db.catalog_node(&root))
            .transpose()
            .map(Option::flatten)
        else {
            return;
        };
        let Some(work_ref) = work_node.reference.clone() else {
            return;
        };
        // Wait for a search summary's real season nodes before bridging so the
        // link lands on the selected season rather than the bare work stub.
        if work_node.kind == Kind::Tv && self.state.seasons.is_empty() && self.state.details_loading
        {
            return;
        }
        let existing_links = self.db.catalog_links(&target).unwrap_or_default();
        if existing_links
            .iter()
            .any(|l| l.manual && !l.confirmed && l.reference.provider == Provider::Bangumi)
            || self.reviews.attempted_bridge.contains(&target)
        {
            return;
        }
        let season_node = (target != work_node.id)
            .then(|| self.db.catalog_node(&target).ok().flatten())
            .flatten();
        let premiere_date = season_node
            .as_ref()
            .map(|s| s.date.clone())
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| work_node.date.clone());
        let season_number = season_node.as_ref().and_then(|s| s.season_number);
        let existing_ddp = reading_provider_subject(&self.db, &target, Provider::Dandanplay)
            .or_else(|| {
                (season_number.unwrap_or(1) <= 1)
                    .then(|| {
                        reading_provider_subject(&self.db, &work_node.id, Provider::Dandanplay)
                    })
                    .flatten()
            })
            .map(|id| id.to_string());
        if premiere_date.is_empty() && existing_ddp.is_none() {
            return;
        }
        self.reviews.attempted_bridge.insert(target.clone());
        let mut titles = vec![work_node.title.clone()];
        if !work_node.original_title.is_empty() && !titles.contains(&work_node.original_title) {
            titles.insert(0, work_node.original_title.clone());
        }
        self.state.comments_loading = true;
        let generation = self.reviews.generation;
        let id = self.state.opened.clone();
        let inbox = self.inbox.clone();
        let wake = bern::wake::waker();
        erika_catalog::client::spawn(async move {
            let bridged = erika_dandanplay::catalog::bridge_bangumi(
                &work_ref,
                &titles,
                season_number,
                &premiere_date,
                existing_ddp.as_deref(),
            )
            .await;
            let Some(bridge) = bridged else {
                inbox
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(Done::Reviews(Finished {
                        generation,
                        id,
                        target: Some(target),
                        bridged: None,
                        subject: None,
                        result: Ok(Vec::new()),
                        subject_info: Err(erika_dandanplay::Error {
                            message: String::new(),
                            sign_in: false,
                        }),
                    }));
                if let Some(wake) = wake {
                    wake.wake();
                }
                return;
            };
            let subject: i64 = bridge.bangumi.id.parse().unwrap_or(0);
            let result = api::bangumi(subject).await;
            let subject_info = erika_dandanplay::bangumi::subject(subject).await;
            inbox
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(Done::Reviews(Finished {
                    generation,
                    id,
                    target: Some(target),
                    bridged: Some(bridge),
                    subject: Some(subject),
                    result,
                    subject_info,
                }));
            if let Some(wake) = wake {
                wake.wake();
            }
        });
    }
    fn show_reviews(&mut self, rows: &[Review]) {
        self.state.has_comments = !rows.is_empty();
        self.state.comments = rows
            .iter()
            .map(|r| anime::Review {
                id: r.id.clone(),
                author: r.author.clone(),
                avatar: MediaSource::new(&r.avatar),
                text: r.text.clone(),
                rating: r.rating,
                updated: r
                    .updated
                    .parse::<i64>()
                    .ok()
                    .filter(|v| *v > 0)
                    .map(|unix| {
                        let date = clock::date_of((unix + Clock::now().offset).div_euclid(86400));
                        format!("{:04}-{:02}-{:02}", date.year, date.month, date.day)
                    })
                    .unwrap_or_else(|| r.updated.get(..10).unwrap_or(&r.updated).into()),
            })
            .collect();
    }
    pub(super) fn take_reviews(&mut self, result: Finished) {
        if result.generation != self.reviews.generation || result.id != self.state.opened {
            return;
        }
        if let (Some(target), Some(bridge)) = (&result.target, &result.bridged)
            && self.account_media().ok().as_ref() == Some(target)
        {
            let existing = self.db.catalog_links(target).unwrap_or_default();
            let mut links = Vec::new();
            for reference in std::iter::once(&bridge.bangumi).chain(bridge.dandanplay.iter()) {
                let previous = existing.iter().find(|l| &l.reference == reference);
                if previous.is_some_and(|l| l.manual || l.confirmed) {
                    continue;
                }
                links.push(ProviderLink {
                    media: target.clone(),
                    reference: reference.clone(),
                    relation: Relation::Related,
                    coverage: Vec::new(),
                    evidence: AUTO_BRIDGE_EVIDENCE.into(),
                    confirmed: false,
                    manual: false,
                    revision: previous.map_or(0, |l| l.revision),
                });
            }
            if !links.is_empty() {
                self.catalog_written |= self.db.catalog_apply(&[], &links, &[], &[]).is_ok();
            }
            if !bridge.summary.trim().is_empty()
                && let Some(ddp) = &bridge.dandanplay
            {
                self.db.set_setting(
                    &format!("dandanplay.summary.{}", ddp.id),
                    Some(bridge.summary.trim()),
                );
                self.refresh_opened_summary();
            }
        }
        let Some(subject) = result.subject else {
            self.state.comments_loading = false;
            return;
        };
        if self.review_subject() != Some(subject) {
            return;
        }
        self.state.comments_loading = false;
        match result.result {
            Ok(rows) => {
                if let Ok(text) = serde_json::to_string(&rows) {
                    self.db
                        .set_setting(&format!("bangumi.reviews.{subject}"), Some(&text));
                }
                self.show_reviews(&rows);
                self.state.comments_error.clear();
            }
            Err(e) => self.state.comments_error = e.message,
        }
        match result.subject_info {
            Ok(info) => {
                if info.rating.is_finite() && (0.0..=10.0).contains(&info.rating) {
                    self.state.show.rating = info.rating;
                }
                let changed = save_bangumi_subject(&self.db, subject, &info);
                if !info.summary.trim().is_empty() {
                    self.refresh_opened_summary();
                }
                if changed {
                    self.catalog_written = true;
                    self.show_opened();
                    self.show_season();
                }
            }
            Err(error) if !error.message.is_empty() && self.state.comments_error.is_empty() => {
                self.state.comments_error = format!("评分读取失败：{}", error.message)
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use erika_catalog::{ProviderLink, ProviderRef, tmdb};
    fn engine() -> AnimeEngine {
        let mut engine = crate::tests::engine();
        let movie = tmdb::parse_work(Kind::Movie, 7, &serde_json::json!({"id":7,"title":"Movie"}))
            .unwrap()
            .node;
        engine.db.catalog_save_nodes(&[movie.clone()]).unwrap();
        engine.state.opened = movie.id.0.clone();
        engine
            .db
            .catalog_link(&ProviderLink {
                media: movie.id,
                reference: ProviderRef {
                    provider: Provider::Bangumi,
                    scope: String::new(),
                    kind: Kind::Subject,
                    id: "123".into(),
                },
                relation: Relation::Exact,
                coverage: Vec::new(),
                evidence: "test".into(),
                confirmed: true,
                manual: true,
                revision: 0,
            })
            .unwrap();
        engine
    }
    fn row() -> Vec<Review> {
        vec![Review {
            id: "1".into(),
            author: "读者".into(),
            avatar: String::new(),
            text: "评论".into(),
            rating: 8,
            updated: "2026-10-24".into(),
        }]
    }
    fn done(e: &AnimeEngine) -> Finished {
        Finished {
            generation: e.reviews.generation,
            id: e.state.opened.clone(),
            target: None,
            bridged: None,
            subject: Some(123),
            result: Ok(row()),
            subject_info: Ok(erika_dandanplay::bangumi::Subject {
                rating: 8.5,
                ..Default::default()
            }),
        }
    }
    #[test]
    fn empty_success_and_failure_never_substitute_other_review_providers() {
        let mut e = engine();
        e.show_reviews(&row());
        let mut result = done(&e);
        result.result = Err(erika_dandanplay::Error {
            message: "offline".into(),
            sign_in: false,
        });
        e.take_reviews(result);
        assert_eq!(e.state.comments[0].text, "评论");
        assert!(!e.state.comments_loading);
        let mut result = done(&e);
        result.result = Ok(Vec::new());
        e.take_reviews(result);
        assert!(e.state.comments.is_empty());
    }
    #[test]
    fn public_rating_is_bangumi_only_and_never_the_users_score() {
        let mut e = engine();
        assert!(!e.accounts.signed_in());
        e.show_opened();
        e.take_reviews(done(&e));
        assert_eq!(e.state.show.rating, 8.5);
        assert_eq!(e.state.my_score, Score::Unrated);
        e.show_opened();
        assert_eq!(e.state.show.rating, 8.5);
        let mut invalid = done(&e);
        invalid.subject_info = Ok(erika_dandanplay::bangumi::Subject {
            rating: f64::NAN,
            ..Default::default()
        });
        e.take_reviews(invalid);
        assert_eq!(e.state.show.rating, 8.5);
        let mut link =
            e.db.catalog_links(&MediaId(e.state.opened.clone()))
                .unwrap()
                .remove(0);
        link.confirmed = false;
        e.db.catalog_link(&link).unwrap();
        e.show_opened();
        assert_eq!(e.state.show.rating, 0.0);
    }

    #[test]
    fn changed_account_selection_or_mapping_invalidates_old_reviews() {
        let mut e = engine();
        let old = done(&e);
        e.reviews.cancel();
        e.take_reviews(old);
        assert!(e.state.comments.is_empty());
        let old = done(&e);
        e.state.opened = MediaId::new().0;
        e.take_reviews(old);
        assert!(e.state.comments.is_empty());
        let mut e = engine();
        let old = done(&e);
        let mut link =
            e.db.catalog_links(&MediaId(e.state.opened.clone()))
                .unwrap()
                .remove(0);
        link.confirmed = false;
        e.db.catalog_link(&link).unwrap();
        e.take_reviews(old);
        assert!(e.state.comments.is_empty());
    }
}
