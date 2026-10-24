//! Cached reviews stay visible while their preferred source refreshes.
use super::*;
use erika_dandanplay::reviews::{self as api, Review};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Source {
    Server,
    Dandanplay,
    Bangumi,
}

impl Source {
    fn key(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Dandanplay => "dandanplay",
            Self::Bangumi => "bangumi",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Server => "弹弹play · 每日缓存",
            Self::Dandanplay => "弹弹play",
            Self::Bangumi => "Bangumi",
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    source: Source,
    rows: Vec<Review>,
}

pub(super) struct Finished {
    generation: u64,
    id: i64,
    source: Source,
    result: Result<Vec<Review>, erika_dandanplay::Error>,
}

pub(super) struct Runtime {
    generation: u64,
    preferred: Source,
    primary_done: bool,
    primary_ok: bool,
    fallback_started: bool,
    fallback_done: bool,
    fallback: Option<Vec<Review>>,
    token: Option<String>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            generation: 0,
            preferred: Source::Server,
            primary_done: false,
            primary_ok: false,
            fallback_started: false,
            fallback_done: false,
            fallback: None,
            token: None,
        }
    }
}

impl Runtime {
    pub fn cancel(&mut self) {
        self.generation += 1;
    }
}

fn cached<T: serde::de::DeserializeOwned>(name: &str) -> Option<T> {
    serde_json::from_slice(&std::fs::read(bern::storage::cache_dir()?.join(name)).ok()?).ok()
}
fn keep<T: Serialize>(name: &str, value: &T) {
    if let (Some(dir), Ok(bytes)) = (bern::storage::cache_dir(), serde_json::to_vec(value)) {
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(name);
        let temporary = path.with_extension("tmp");
        if std::fs::write(&temporary, bytes).is_ok() {
            let _ = std::fs::rename(temporary, path);
        }
    }
}
pub(super) fn keep_details(details: &Details) {
    keep(&format!("anime-details-{}.json", details.anime_id), details);
}
pub(super) fn cached_details(id: i64) -> Option<Details> {
    cached(&format!("anime-details-{id}.json"))
}
fn cache_name(id: i64, source: Source) -> String {
    format!("anime-reviews-{id}-{}.json", source.key())
}

impl AnimeEngine {
    pub(super) fn load_reviews(&mut self) {
        self.reviews.cancel();
        let id = self.state.opened;
        if id <= 0 {
            return;
        }
        let bangumi_only = erika_dandanplay::bangumi::subject_id(id).is_some();
        let preferred = if self.accounts.bangumi || bangumi_only {
            Source::Bangumi
        } else if self.accounts.dandanplay {
            Source::Dandanplay
        } else {
            Source::Server
        };
        let old = [preferred, Source::Dandanplay, Source::Server]
            .into_iter()
            .filter(|source| preferred != Source::Server || *source == Source::Server)
            .find_map(|source| cached::<Snapshot>(&cache_name(id, source)));
        self.state.comments.clear();
        self.state.has_comments = false;
        self.state.comments_error.clear();
        self.state.comments_source = preferred.label().into();
        self.state.comments_loading = true;
        let had_cache = old.is_some();
        if let Some(old) = old {
            self.show_reviews(old.source, &old.rows);
        }
        let generation = self.reviews.generation;
        self.reviews = Runtime {
            generation,
            preferred,
            token: erika_dandanplay::token(),
            ..Runtime::default()
        };
        self.request_reviews(preferred);
        if preferred == Source::Bangumi && !had_cache && !bangumi_only {
            self.request_reviews(Source::Dandanplay);
        }
    }

    fn request_reviews(&mut self, source: Source) {
        if source == Source::Dandanplay && self.reviews.preferred == Source::Bangumi {
            self.reviews.fallback_started = true;
        }
        let id = self.state.opened;
        let generation = self.reviews.generation;
        let subject = self
            .details
            .get(&id)
            .and_then(|d| api::subject_id(&d.bangumi_url))
            .or_else(|| self.links.subject(id));
        let token = self.reviews.token.clone();
        in_background(&self.inbox, move || {
            let result = match source {
                Source::Server => api::dandanplay(id, false, None),
                Source::Dandanplay => api::dandanplay(id, true, token.as_deref()),
                Source::Bangumi => {
                    let subject = subject.or_else(|| {
                        erika_dandanplay::details(id)
                            .ok()
                            .and_then(|d| api::subject_id(&d.bangumi_url))
                    });
                    match subject {
                        Some(subject) => api::bangumi(subject),
                        None => Err(erika_dandanplay::Error {
                            message: "未找到对应的 Bangumi 条目".into(),
                            sign_in: false,
                        }),
                    }
                }
            };
            Some(Done::Reviews(Finished {
                generation,
                id,
                source,
                result,
            }))
        });
    }

    fn show_reviews(&mut self, source: Source, rows: &[Review]) {
        self.state.comments_source = source.label().into();
        self.state.has_comments = !rows.is_empty();
        self.state.comments = rows
            .iter()
            .map(|r| {
                let updated = if source == Source::Bangumi {
                    r.updated
                        .parse::<i64>()
                        .ok()
                        .filter(|n| *n > 0)
                        .map(|unix| {
                            let date =
                                clock::date_of((unix + Clock::now().offset).div_euclid(86400));
                            format!("{:04}-{:02}-{:02}", date.year, date.month, date.day)
                        })
                        .unwrap_or_default()
                } else {
                    r.updated.get(..10).unwrap_or(&r.updated).into()
                };
                anime::Review {
                    id: r.id.clone(),
                    author: r.author.clone(),
                    avatar: MediaSource::new(&r.avatar),
                    text: r.text.clone(),
                    rating: r.rating,
                    updated,
                }
            })
            .collect();
    }

    pub(super) fn take_reviews(&mut self, finished: Finished) {
        let Finished {
            generation,
            id,
            source,
            result,
        } = finished;
        if generation != self.reviews.generation || id != self.state.opened {
            return;
        }
        let primary = source == self.reviews.preferred;
        if primary {
            self.reviews.primary_done = true;
        } else {
            self.reviews.fallback_done = true;
        }
        match result {
            Ok(rows) => {
                keep(
                    &cache_name(id, source),
                    &Snapshot {
                        source,
                        rows: rows.clone(),
                    },
                );
                if primary {
                    self.reviews.primary_ok = true;
                } else {
                    self.reviews.fallback = Some(rows.clone());
                }
                // A late fallback must never overwrite a successful Bangumi response,
                // including a valid response with no reviews.
                if primary || !self.reviews.primary_ok {
                    self.show_reviews(source, &rows);
                    self.state.comments_error.clear();
                }
            }
            Err(error) => {
                if primary
                    && self.reviews.preferred == Source::Bangumi
                    && erika_dandanplay::bangumi::subject_id(id).is_none()
                {
                    if let Some(rows) = self.reviews.fallback.clone() {
                        self.show_reviews(Source::Dandanplay, &rows);
                    } else if !self.reviews.fallback_started {
                        self.request_reviews(Source::Dandanplay);
                    }
                }
                if !self.reviews.primary_ok && self.reviews.fallback.is_none() {
                    self.state.comments_error = error.message;
                }
            }
        }
        self.state.comments_loading = !self.reviews.primary_done
            || (!self.reviews.primary_ok
                && self.reviews.fallback_started
                && !self.reviews.fallback_done);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(text: &str) -> Vec<Review> {
        vec![Review {
            id: "1".into(),
            author: "读者".into(),
            avatar: "".into(),
            text: text.into(),
            rating: 8,
            updated: "2026-10-24".into(),
        }]
    }
    fn engine() -> AnimeEngine {
        let mut e = AnimeEngine::default();
        e.state.opened = 7;
        e.reviews.preferred = Source::Bangumi;
        e.reviews.fallback_started = true;
        e
    }
    fn done(source: Source, rows: Vec<Review>) -> Finished {
        Finished {
            generation: 0,
            id: 7,
            source,
            result: Ok(rows),
        }
    }
    #[test]
    fn fallback_during_first_load_is_replaced_by_bangumi() {
        let mut e = engine();
        e.take_reviews(done(Source::Dandanplay, row("waiting")));
        assert_eq!(e.state.comments[0].text, "waiting");
        assert!(e.state.comments_loading);
        e.take_reviews(done(Source::Bangumi, row("primary")));
        assert_eq!(e.state.comments[0].text, "primary");
        assert!(!e.state.comments_loading);
    }
    #[test]
    fn late_fallback_cannot_replace_empty_success_or_a_different_show() {
        let mut e = engine();
        e.take_reviews(done(Source::Bangumi, vec![]));
        e.take_reviews(done(Source::Dandanplay, row("late")));
        assert!(e.state.comments.is_empty());
        assert_eq!(e.state.comments_source, "Bangumi");
        e.reviews.cancel();
        e.take_reviews(done(Source::Bangumi, row("old account")));
        assert!(e.state.comments.is_empty());
        e.state.opened = 8;
        e.take_reviews(done(Source::Bangumi, row("other show")));
        assert!(e.state.comments.is_empty());
    }
    #[test]
    fn failed_refresh_keeps_old_rows_and_finishes_on_fallback_failure() {
        let mut e = engine();
        e.show_reviews(Source::Bangumi, &row("cached"));
        for source in [Source::Bangumi, Source::Dandanplay] {
            e.take_reviews(Finished {
                generation: 0,
                id: 7,
                source,
                result: Err(erika_dandanplay::Error {
                    message: "offline".into(),
                    sign_in: false,
                }),
            });
        }
        assert_eq!(e.state.comments[0].text, "cached");
        assert!(!e.state.comments_loading);
    }

    #[test]
    fn bangumi_only_ids_never_reach_dandanplay_on_failure() {
        let mut e = engine();
        e.state.opened = erika_dandanplay::bangumi::local_id(393010).unwrap();
        e.reviews.fallback_started = false;
        e.take_reviews(Finished {
            generation: 0,
            id: e.state.opened,
            source: Source::Bangumi,
            result: Err(erika_dandanplay::Error {
                message: "offline".into(),
                sign_in: false,
            }),
        });
        assert!(!e.reviews.fallback_started);
        assert!(!e.state.comments_loading);
    }
}
