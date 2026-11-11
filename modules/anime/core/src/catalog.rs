//! Account-aware catalogue routing; provider IDs are never interchangeable.
use erika_anime_protocol::anime::SeasonSource;
use erika_dandanplay::{self as api, Details, Found, Intro, Result, bangumi};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) struct Accounts {
    pub dandanplay: bool,
    pub bangumi: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Dandanplay,
    Bangumi,
    Public,
}

impl Accounts {
    pub fn signed_in(self) -> bool {
        self.dandanplay || self.bangumi
    }

    pub fn can_choose(self) -> bool {
        self.dandanplay && self.bangumi
    }

    pub fn season_source(self, preferred: &SeasonSource) -> SeasonSource {
        if self.can_choose() {
            preferred.clone()
        } else if self.bangumi {
            SeasonSource::Bangumi
        } else {
            SeasonSource::Dandanplay
        }
    }

    fn season_sources(self, preferred: &SeasonSource) -> Vec<Source> {
        if self.can_choose() && *preferred == SeasonSource::Bangumi {
            vec![Source::Bangumi, Source::Dandanplay, Source::Public]
        } else {
            self.sources()
        }
    }

    fn sources(self) -> Vec<Source> {
        if self.dandanplay {
            vec![Source::Dandanplay, Source::Bangumi, Source::Public]
        } else if self.bangumi {
            vec![Source::Bangumi, Source::Public]
        } else {
            vec![Source::Public, Source::Bangumi]
        }
    }

    fn fetch<T>(self, request: impl FnMut(Source) -> Result<T>) -> Result<T> {
        fetch_from(self.sources(), request)
    }
}

fn fetch_from<T>(sources: Vec<Source>, mut request: impl FnMut(Source) -> Result<T>) -> Result<T> {
    let mut last = api::Error {
        message: "暂时无法获取番剧数据".into(),
        sign_in: false,
    };
    for source in sources {
        match request(source) {
            Ok(value) => return Ok(value),
            Err(error) => last = error,
        }
    }
    Err(last)
}

pub(super) const SEASON_SOURCE_SETTING: &str = "anime.season_source";

pub(super) fn saved_season_source(db: &erika_store::Db) -> SeasonSource {
    match db.setting(SEASON_SOURCE_SETTING).as_deref() {
        Some("bangumi") => SeasonSource::Bangumi,
        _ => SeasonSource::Dandanplay,
    }
}

pub(super) fn save_season_source(db: &erika_store::Db, source: &SeasonSource) {
    db.set_setting(
        SEASON_SOURCE_SETTING,
        Some(match source {
            SeasonSource::Dandanplay => "dandanplay",
            SeasonSource::Bangumi => "bangumi",
        }),
    );
}

/// Only links reported by the source's details are accepted; matching names or
/// numerically equal provider IDs is not evidence that two shows are the same.
#[derive(Clone, Default)]
pub(super) struct Links(HashMap<i64, HashSet<i64>>);

impl Links {
    pub fn cached() -> Self {
        let mut links = Self::default();
        if let Some(dir) = bern::storage::cache_dir()
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("anime-details-")
                    && name.ends_with(".json")
                    && let Ok(bytes) = std::fs::read(entry.path())
                    && let Ok(details) = serde_json::from_slice::<Details>(&bytes)
                {
                    links.remember(&details);
                }
            }
        }
        links
    }

    pub fn remember(&mut self, details: &Details) {
        if details.anime_id > 0
            && bangumi::subject_id(details.anime_id).is_none()
            && let Some(subject) = api::reviews::subject_id(&details.bangumi_url)
        {
            self.0.entry(subject).or_default().insert(details.anime_id);
        }
    }

    fn canonical(&self, id: i64) -> i64 {
        bangumi::subject_id(id)
            .and_then(|subject| self.0.get(&subject))
            .filter(|ids| ids.len() == 1)
            .and_then(|ids| ids.iter().next().copied())
            .unwrap_or(id)
    }

    pub fn subject(&self, id: i64) -> Option<i64> {
        bangumi::subject_id(id).or_else(|| {
            self.0
                .iter()
                .find_map(|(&subject, ids)| ids.contains(&id).then_some(subject))
        })
    }
}

async fn fetch_async<T, F: std::future::Future<Output = Result<T>>>(
    sources: Vec<Source>,
    mut request: impl FnMut(Source) -> F,
) -> Result<T> {
    let mut last = api::Error {
        message: "暂时无法获取番剧数据".into(),
        sign_in: false,
    };
    for source in sources {
        match request(source).await {
            Ok(value) => return Ok(value),
            Err(error) => last = error,
        }
    }
    Err(last)
}

pub(super) async fn season(
    accounts: Accounts,
    preferred: &SeasonSource,
    links: &Links,
) -> Result<Vec<Intro>> {
    fetch_async(accounts.season_sources(preferred), |source| async move {
        let rows = match source {
            Source::Dandanplay => api::catalog::season(false).await,
            Source::Bangumi => bangumi::season().await,
            Source::Public => api::catalog::season(true).await,
        }?;
        if rows.is_empty() {
            return Err(api::Error {
                message: "新番列表暂时为空".into(),
                sign_in: false,
            });
        }
        Ok(rows
            .into_iter()
            .map(|mut row| {
                row.anime_id = links.canonical(row.anime_id);
                row
            })
            .collect())
    })
    .await
}

pub(super) fn search(accounts: Accounts, links: &Links, text: &str) -> Result<Vec<Found>> {
    let result = if accounts.dandanplay {
        match api::search(text) {
            Ok(rows) if !rows.is_empty() => Ok(rows),
            _ => bangumi::search(text),
        }
    } else {
        bangumi::search(text)
    };
    result.map(|rows| {
        rows.into_iter()
            .map(|mut row| {
                row.anime_id = links.canonical(row.anime_id);
                row
            })
            .collect()
    })
}

pub(super) fn details(
    accounts: Accounts,
    links: &Links,
    id: i64,
    hint: Option<&Details>,
) -> Result<Details> {
    if api::custom::is_ours(id) {
        return api::details(id);
    }
    if let Some(subject) = bangumi::subject_id(id) {
        return bangumi::details(subject);
    }
    let mut snapshot: Option<Result<Details>> = None;
    accounts.fetch(|source| match source {
        Source::Dandanplay => api::details_from(id, false),
        Source::Public => snapshot
            .get_or_insert_with(|| api::details_from(id, true))
            .clone(),
        Source::Bangumi => {
            let subject = links
                .subject(id)
                .or_else(|| hint.and_then(|d| api::reviews::subject_id(&d.bangumi_url)))
                .or_else(|| {
                    snapshot
                        .get_or_insert_with(|| api::details_from(id, true))
                        .as_ref()
                        .ok()
                        .and_then(|d| api::reviews::subject_id(&d.bangumi_url))
                })
                .ok_or_else(|| api::Error {
                    message: "未找到对应的 Bangumi 条目".into(),
                    sign_in: false,
                })?;
            let mut details = bangumi::details(subject)?;
            details.anime_id = id;
            Ok(details)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn season_preference_applies_to_both_accounts_and_keeps_failure_fallback() {
        let both = Accounts {
            dandanplay: true,
            bangumi: true,
        };
        assert_eq!(
            both.season_sources(&SeasonSource::Bangumi),
            vec![Source::Bangumi, Source::Dandanplay, Source::Public]
        );
        assert_eq!(
            both.season_sources(&SeasonSource::Dandanplay),
            both.sources()
        );
        let mut attempted = Vec::new();
        let result = futures_lite::future::block_on(fetch_async(
            both.season_sources(&SeasonSource::Bangumi),
            |source| {
                attempted.push(source);
                async move {
                    futures_lite::future::yield_now().await;
                    if source == Source::Dandanplay {
                        Ok(42)
                    } else {
                        Err(api::Error {
                            message: "offline".into(),
                            sign_in: false,
                        })
                    }
                }
            },
        ));
        assert_eq!(result, Ok(42));
        assert_eq!(attempted, vec![Source::Bangumi, Source::Dandanplay]);
        for preferred in [SeasonSource::Bangumi, SeasonSource::Dandanplay] {
            for accounts in [
                Accounts::default(),
                Accounts {
                    dandanplay: true,
                    bangumi: false,
                },
                Accounts {
                    dandanplay: false,
                    bangumi: true,
                },
            ] {
                assert!(!accounts.can_choose());
                assert_eq!(accounts.season_sources(&preferred), accounts.sources());
                assert_eq!(
                    accounts.season_source(&preferred),
                    if accounts.bangumi {
                        SeasonSource::Bangumi
                    } else {
                        SeasonSource::Dandanplay
                    }
                );
            }
        }
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn season_preference_survives_database_reopen() {
        let path = std::env::temp_dir().join(format!(
            "erika-season-source-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let db = erika_store::Db::at(&path).unwrap();
            assert_eq!(saved_season_source(&db), SeasonSource::Dandanplay);
            db.set_setting(SEASON_SOURCE_SETTING, Some("unknown"));
            assert_eq!(saved_season_source(&db), SeasonSource::Dandanplay);
            save_season_source(&db, &SeasonSource::Bangumi);
        }
        {
            let db = erika_store::Db::at(&path).unwrap();
            assert_eq!(saved_season_source(&db), SeasonSource::Bangumi);
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn single_accounts_both_and_guest_choose_their_sources_and_fall_back() {
        for (accounts, expected) in [
            (Accounts::default(), vec![Source::Public, Source::Bangumi]),
            (
                Accounts {
                    dandanplay: true,
                    bangumi: false,
                },
                vec![Source::Dandanplay, Source::Bangumi, Source::Public],
            ),
            (
                Accounts {
                    dandanplay: false,
                    bangumi: true,
                },
                vec![Source::Bangumi, Source::Public],
            ),
            (
                Accounts {
                    dandanplay: true,
                    bangumi: true,
                },
                vec![Source::Dandanplay, Source::Bangumi, Source::Public],
            ),
        ] {
            assert_eq!(
                accounts.signed_in(),
                accounts.dandanplay || accounts.bangumi
            );
            let mut attempted = Vec::new();
            let result = accounts.fetch(|source| {
                attempted.push(source);
                if source == *expected.last().unwrap() {
                    Ok(42)
                } else {
                    Err(api::Error {
                        message: "offline".into(),
                        sign_in: true,
                    })
                }
            });
            assert_eq!(result, Ok(42));
            assert_eq!(attempted, expected);
            let mut calls = 0;
            assert_eq!(
                accounts.fetch(|_| {
                    calls += 1;
                    Ok(Vec::<Intro>::new())
                }),
                Ok(vec![])
            );
            assert_eq!(calls, 1);
        }
    }

    #[test]
    fn only_verified_links_reuse_dandanplay_identity() {
        let mut links = Links::default();
        let mut details: Details = serde_json::from_value(
            serde_json::json!({"animeId":17527,"bangumiUrl":"https://bgm.tv/subject/393010"}),
        )
        .unwrap();
        links.remember(&details);
        assert_eq!(links.canonical(bangumi::local_id(393010).unwrap()), 17527);
        assert_eq!(links.subject(17527), Some(393010));
        assert_eq!(links.canonical(393010), 393010);
        details.anime_id = -2;
        links.remember(&details);
        assert_eq!(links.subject(17527), Some(393010));
        details.anime_id = 17528;
        links.remember(&details);
        assert_eq!(
            links.canonical(bangumi::local_id(393010).unwrap()),
            bangumi::local_id(393010).unwrap()
        );
    }
}
