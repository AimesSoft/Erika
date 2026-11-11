//! Evidence-based resolution. File recognition is supplied by the dandanplay
//! adapter first; only its unresolved result proceeds to filename/TMDB search.
use crate::names::NameHints;
use crate::{BindingItem, Client, Kind, MediaId, MediaNode, Provider, ProviderRef, Result, tmdb};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Recognition {
    pub work: Option<ProviderRef>,
    pub episode: Option<ProviderRef>,
    pub tmdb: Vec<ProviderRef>,
    pub titles: Vec<String>,
    pub season_number: Option<u32>,
    pub episode_number: Option<u32>,
    /// Opaque provider season ID and original episode label, never TMDB positions.
    #[serde(default)]
    pub provider_season_id: Option<String>,
    #[serde(default)]
    pub episode_label: Option<String>,
    pub special_label: Option<String>,
    pub shift_seconds: f64,
    pub bangumi_subject: Option<ProviderRef>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Resolved {
    pub work: tmdb::Work,
    pub season: Option<tmdb::Season>,
    pub items: Vec<BindingItem>,
    pub recognition: Option<Recognition>,
    pub evidence: String,
}
impl Resolved {
    pub fn nodes(&self) -> Vec<MediaNode> {
        let mut nodes = vec![self.work.node.clone()];
        nodes.extend(self.work.seasons.clone());
        if let Some(season) = &self.season {
            if let Some(known) = nodes.iter_mut().find(|n| n.id == season.node.id) {
                *known = season.node.clone();
            }
            nodes.extend(season.episodes.clone());
        }
        nodes
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Resolution {
    Bound {
        result: Resolved,
    },
    Pending {
        candidates: Vec<MediaNode>,
        recognition: Option<Recognition>,
        reason: String,
    },
}

/// Independent recognition evidence survives a failed metadata request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileResolution {
    pub recognition: Option<Recognition>,
    pub recognition_error: Option<String>,
    pub resolution: Result<Resolution>,
}

pub fn tmdb_url(value: &str) -> Option<ProviderRef> {
    let url = url::Url::parse(value).ok()?;
    if !matches!(url.scheme(), "https" | "http")
        || !matches!(
            url.host_str(),
            Some("www.themoviedb.org" | "themoviedb.org")
        )
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    let pieces: Vec<_> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    if pieces.len() != 2 {
        return None;
    }
    let kind = match pieces[0] {
        "tv" => Kind::Tv,
        "movie" => Kind::Movie,
        _ => return None,
    };
    let id = pieces[1].split('-').next()?.parse().ok()?;
    ProviderRef::tmdb(kind, id).ok()
}
fn normal(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
/// Name equality alone is insufficient. An explicit year is required for an
/// automatic search binding; a verified source reference bypasses name search.
pub fn accept_search(hints: &NameHints, page: &tmdb::SearchPage) -> Option<ProviderRef> {
    if page.total_pages > 1 || page.page != 1 {
        return None;
    }
    let year = hints.year?.to_string();
    let candidates: Vec<_> = page
        .results
        .iter()
        .filter(|node| {
            node.date.starts_with(&format!("{year}-"))
                && hints.titles.iter().any(|name| {
                    let name = normal(name);
                    !name.is_empty()
                        && (name == normal(&node.title) || name == normal(&node.original_title))
                })
        })
        .collect();
    if candidates.len() != 1 {
        return None;
    }
    candidates[0].reference.clone()
}

#[derive(Clone, Debug)]
pub struct WorkSearch {
    pub candidates: Vec<MediaNode>,
    pub accepted: Option<ProviderRef>,
}

/// The file resolver and calendar enrichment use the same identity rule. A
/// competing failed, ambiguous or paginated query cannot prove uniqueness.
pub async fn search_work(
    client: &Client,
    hints: &NameHints,
    kind: Option<Kind>,
) -> Result<WorkSearch> {
    let kinds: Vec<_> = kind
        .filter(|k| matches!(k, Kind::Tv | Kind::Movie))
        .map_or(vec![Kind::Tv, Kind::Movie], |k| vec![k]);
    let mut pages = Vec::new();
    let mut last_error = None;
    for title in hints.titles.iter().take(3) {
        for kind in &kinds {
            match client.search(*kind, title, hints.year, 1).await {
                Ok(page) => pages.push(page),
                Err(error) => last_error = Some(error),
            }
        }
    }
    if pages.is_empty()
        && let Some(error) = last_error
    {
        return Err(error);
    }
    Ok(combine_search(hints, pages, last_error.is_none()))
}

fn combine_search(hints: &NameHints, pages: Vec<tmdb::SearchPage>, complete: bool) -> WorkSearch {
    let complete = complete && pages.iter().all(|p| p.page == 1 && p.total_pages <= 1);
    let mut candidates = Vec::new();
    for page in pages {
        for candidate in page.results {
            if !candidates.iter().any(|n: &MediaNode| n.id == candidate.id) {
                candidates.push(candidate);
            }
        }
    }
    let accepted = if complete {
        accept_search(
            hints,
            &tmdb::SearchPage {
                results: candidates.clone(),
                page: 1,
                total_pages: 1,
            },
        )
    } else {
        None
    };
    WorkSearch {
        candidates,
        accepted,
    }
}

pub async fn resolve(
    client: &Client,
    hints: NameHints,
    kind: Option<Kind>,
    recognition: Option<Recognition>,
    known: Option<ProviderRef>,
) -> Result<Resolution> {
    let mut references = Vec::new();
    if let Some(known) = known {
        references.push(known);
    } else if let Some(recognition) = &recognition {
        for reference in &recognition.tmdb {
            if !references.contains(reference) {
                references.push(reference.clone());
            }
        }
    }
    if references.len() == 1 {
        let work = client.work(&references[0]).await?;
        return locate(client, work, &hints, recognition, "明确TMDB身份").await;
    }
    if references.len() > 1 {
        return Ok(Resolution::Pending {
            candidates: Vec::new(),
            recognition,
            reason: "来源给出了互相冲突的 TMDB 身份，请确认作品".into(),
        });
    }
    let mut hints = hints;
    if let Some(recognition) = &recognition {
        for title in &recognition.titles {
            if !hints.titles.contains(title) {
                hints.titles.push(title.clone());
            }
        }
    }
    let search = search_work(client, &hints, kind).await?;
    if let Some(reference) = search.accepted {
        let work = client.work(&reference).await?;
        return locate(
            client,
            work,
            &hints,
            recognition,
            "文件名、年份及唯一TMDB候选一致",
        )
        .await;
    }
    Ok(Resolution::Pending {
        candidates: search.candidates,
        recognition,
        reason: "作品不唯一或缺少年份等证据，请选择作品".into(),
    })
}

pub async fn locate(
    client: &Client,
    work: tmdb::Work,
    hints: &NameHints,
    recognition: Option<Recognition>,
    evidence: &str,
) -> Result<Resolution> {
    if work.node.kind == Kind::Movie {
        if !hints.episodes.is_empty() || hints.season.is_some() {
            return Ok(Resolution::Pending {
                candidates: vec![work.node],
                recognition,
                reason: "电影候选与文件季集信息冲突".into(),
            });
        }
        let work = client.with_artwork(work).await;
        let items = vec![BindingItem {
            media: work.node.id.clone(),
            start_seconds: None,
            end_seconds: None,
        }];
        return Ok(Resolution::Bound {
            result: Resolved {
                work,
                season: None,
                items,
                recognition,
                evidence: evidence.into(),
            },
        });
    }
    let mut season_number = hints.season;
    if let Some(recognized) = &recognition {
        if season_number.is_some()
            && recognized.season_number.is_some()
            && season_number != recognized.season_number
        {
            return Ok(Resolution::Pending {
                candidates: vec![work.node],
                recognition,
                reason: "文件名与识别结果的季位置冲突".into(),
            });
        }
        season_number = season_number.or(recognized.season_number);
    }
    // SP1 is not S00E01. Only an explicit season/episode or verified mapping
    // locates specials. A single real regular season can disambiguate an
    // ordinary episode, without inventing season 1.
    let special = hints.special_label.is_some()
        || recognition
            .as_ref()
            .is_some_and(|r| r.special_label.is_some());
    if season_number.is_none() && !special {
        let regular: Vec<_> = work
            .seasons
            .iter()
            .filter(|s| s.season_number != Some(0))
            .collect();
        if regular.len() == 1 {
            season_number = regular[0].season_number;
        }
    }
    let mut numbers = hints.episodes.clone();
    if let Some(recognized) = &recognition {
        if numbers.is_empty() && !special {
            numbers.extend(recognized.episode_number);
        } else if numbers.len() == 1 && recognized.episode_number.is_some_and(|n| numbers[0] != n) {
            return Ok(Resolution::Pending {
                candidates: vec![work.node],
                recognition,
                reason: "文件名与识别结果的集位置冲突".into(),
            });
        }
    }
    let Some(number) = season_number.filter(|_| !numbers.is_empty()) else {
        return Ok(Resolution::Pending {
            candidates: vec![work.node],
            recognition,
            reason: "作品已找到，季集位置仍需确认".into(),
        });
    };
    if !work.seasons.iter().any(|s| s.season_number == Some(number)) {
        return Ok(Resolution::Pending {
            candidates: vec![work.node],
            recognition,
            reason: "作品没有指定的季".into(),
        });
    }
    let season = client.season(&work, number).await?;
    let items: Option<Vec<_>> = numbers
        .iter()
        .map(|number| {
            season
                .episodes
                .iter()
                .find(|e| e.episode_number == Some(*number))
                .map(|node| BindingItem {
                    media: node.id.clone(),
                    start_seconds: None,
                    end_seconds: None,
                })
        })
        .collect();
    let Some(items) = items else {
        return Ok(Resolution::Pending {
            candidates: vec![work.node],
            recognition,
            reason: "该季没有对应的分集，不能按编号生成".into(),
        });
    };
    let work = client.with_artwork(work).await;
    Ok(Resolution::Bound {
        result: Resolved {
            work,
            season: Some(season),
            items,
            recognition,
            evidence: evidence.into(),
        },
    })
}

/// A source's explicit identity may be used only at its own hierarchy level.
pub fn same_episode(node: &MediaNode, reference: &ProviderRef, root: &MediaId) -> bool {
    reference.provider == Provider::Tmdb
        && reference.kind == Kind::Episode
        && node.reference.as_ref() == Some(reference)
        && &node.root == root
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn exact_name_needs_year_and_complete_results() {
        let mut hints = crate::names::parse("Show (2024) S01E01.mkv");
        let mut page = tmdb::parse_search(Kind::Tv, &json!({"page":1,"total_pages":1,"results":[{"id":1,"name":"Show","first_air_date":"2024-01-01"}]})).unwrap();
        assert!(accept_search(&hints, &page).is_some());
        page.total_pages = 2;
        assert!(accept_search(&hints, &page).is_none());
        page.total_pages = 1;
        hints.year = None;
        assert!(accept_search(&hints, &page).is_none());
    }
    #[test]
    fn all_competing_searches_must_agree_before_an_automatic_binding() {
        let hints = crate::names::parse("Show (2024) S01E01.mkv");
        let page = |id| {
            tmdb::parse_search(
                Kind::Tv,
                &json!({
                    "page":1,"total_pages":1,
                    "results":[{"id":id,"name":"Show","first_air_date":"2024-01-01"}]
                }),
            )
            .unwrap()
        };
        let same = combine_search(&hints, vec![page(1), page(1)], true);
        assert_eq!(same.candidates.len(), 1);
        assert_eq!(same.accepted.unwrap().id, "1");
        let conflicting = combine_search(&hints, vec![page(1), page(2)], true);
        assert_eq!(conflicting.candidates.len(), 2);
        assert!(conflicting.accepted.is_none());
        assert!(
            combine_search(&hints, vec![page(1)], false)
                .accepted
                .is_none()
        );
        let mut paginated = page(1);
        paginated.total_pages = 2;
        assert!(
            combine_search(&hints, vec![page(1), paginated], true)
                .accepted
                .is_none()
        );
        let movie = tmdb::parse_search(
            Kind::Movie,
            &json!({
                "page":1,"total_pages":1,
                "results":[{"id":1,"title":"Show","release_date":"2024-01-01"}]
            }),
        )
        .unwrap();
        let mixed = combine_search(&hints, vec![page(1), movie], true);
        assert_eq!(mixed.candidates.len(), 2);
        assert!(mixed.accepted.is_none());
    }

    #[test]
    fn conflicting_explicit_references_never_fall_back_to_a_name_search() {
        let client = Client::new("http://127.0.0.1:1/", "zh-CN").unwrap();
        let recognition = Recognition {
            tmdb: vec![
                ProviderRef::tmdb(Kind::Tv, 1).unwrap(),
                ProviderRef::tmdb(Kind::Tv, 2).unwrap(),
            ],
            ..Default::default()
        };
        let result = futures_lite::future::block_on(resolve(
            &client,
            crate::names::parse("Show (2024) S01E01.mkv"),
            None,
            Some(recognition.clone()),
            None,
        ))
        .unwrap();
        let Resolution::Pending {
            candidates,
            recognition: kept,
            reason,
        } = result
        else {
            panic!("must require confirmation")
        };
        assert!(candidates.is_empty());
        assert_eq!(kept, Some(recognition));
        assert!(reason.contains("冲突"));
    }

    #[test]
    fn external_links_need_exact_host_and_work_level() {
        assert_eq!(
            tmdb_url("https://www.themoviedb.org/tv/1399-game-of-thrones")
                .unwrap()
                .kind,
            Kind::Tv
        );
        assert!(tmdb_url("https://www.themoviedb.org.evil.invalid/tv/1399").is_none());
        assert!(tmdb_url("https://www.themoviedb.org/tv/1399/season/1").is_none());
        assert!(tmdb_url("https://evil@www.themoviedb.org/tv/1399").is_none());
    }
}
