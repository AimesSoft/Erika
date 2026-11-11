//! Reviews come only from Bangumi. A missing mapping is not a request to
//! substitute another provider's reviews, and stale replies cannot change views.
use super::*;
#[cfg(test)]
use erika_catalog::{Kind, Relation};
use erika_catalog::{MediaId, Provider};
use erika_dandanplay::reviews::{self as api, Review};

pub(super) struct Finished {
    generation: u64,
    id: String,
    subject: i64,
    result: Result<Vec<Review>, erika_dandanplay::Error>,
    rating: Result<f64, erika_dandanplay::Error>,
}
#[derive(Default)]
pub(super) struct Runtime {
    generation: u64,
}
impl Runtime {
    pub fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }
}
pub(super) fn cached_rating(db: &erika_store::Db, media: &MediaId) -> f64 {
    sync::subject(db, media, Provider::Bangumi)
        .ok()
        .flatten()
        .and_then(|link| db.setting(&format!("bangumi.rating.{}", link.reference.id)))
        .and_then(|text| text.parse::<f64>().ok())
        .filter(|rating| rating.is_finite() && (0.0..=10.0).contains(rating))
        .unwrap_or(0.0)
}
impl AnimeEngine {
    pub(super) fn review_subject(&self) -> Option<i64> {
        let media = self.account_media().ok()?;
        sync::subject(&self.db, &media, Provider::Bangumi)
            .ok()??
            .reference
            .id
            .parse::<i64>()
            .ok()
            .filter(|id| *id > 0)
    }
    pub(super) fn load_reviews(&mut self) {
        self.reviews.cancel();
        self.state.comments.clear();
        self.state.has_comments = false;
        self.state.comments_loading = false;
        self.state.comments_source = "Bangumi".into();
        self.state.comments_error.clear();
        let Some(subject) = self.review_subject() else {
            return;
        };
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
            let rating = erika_dandanplay::bangumi::subject(subject)
                .await
                .map(|subject| subject.rating);
            inbox
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(Done::Reviews(Finished {
                    generation,
                    id,
                    subject,
                    result,
                    rating,
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
        if result.generation != self.reviews.generation
            || result.id != self.state.opened
            || self.review_subject() != Some(result.subject)
        {
            return;
        }
        self.state.comments_loading = false;
        match result.result {
            Ok(rows) => {
                if let Ok(text) = serde_json::to_string(&rows) {
                    self.db
                        .set_setting(&format!("bangumi.reviews.{}", result.subject), Some(&text));
                }
                self.show_reviews(&rows);
                self.state.comments_error.clear();
            }
            Err(e) => self.state.comments_error = e.message,
        }
        match result.rating {
            Ok(rating) if rating.is_finite() && (0.0..=10.0).contains(&rating) => {
                self.db.set_setting(
                    &format!("bangumi.rating.{}", result.subject),
                    Some(&rating.to_string()),
                );
                self.state.show.rating = rating;
            }
            Err(error) if self.state.comments_error.is_empty() => {
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
            subject: 123,
            result: Ok(row()),
            rating: Ok(8.5),
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
        invalid.rating = Ok(f64::NAN);
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
