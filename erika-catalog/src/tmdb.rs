//! TMDB wire data → typed catalogue. Every identity and parent is checked
//! against the request; an empty/malformed response is never a valid binding.
use crate::artwork::{self, Artwork, ArtworkKind};
use crate::{Error, Kind, MediaId, MediaNode, ProviderRef, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Work {
    pub node: MediaNode,
    pub seasons: Vec<MediaNode>,
    /// Series-level episode dates (`last_air_date`, `last_episode_to_air`,
    /// `next_episode_to_air`) for shows TMDB keeps inside a single season.
    #[serde(default)]
    pub active_dates: Vec<String>,
    /// Original-language TMDB overview when the requested locale has none;
    /// kept outside `node.overview` so Chinese provider summaries can take
    /// precedence before falling back to a foreign-language synopsis.
    #[serde(default)]
    pub original_overview: String,
    /// Pre-split seasons from TMDB `episode_groups` (`type == 6`, Seasons) for
    /// anime whose primary `seasons` list merges multiple seasons into Season 1.
    #[serde(default)]
    pub split_seasons: Vec<Season>,
}
impl Work {
    pub fn airs_in(&self, year: u16) -> bool {
        let prefix = format!("{year}-");
        self.seasons
            .iter()
            .any(|s| s.season_number.is_some_and(|n| n > 0) && s.date.starts_with(&prefix))
            || self.active_dates.iter().any(|d| d.starts_with(&prefix))
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Season {
    pub node: MediaNode,
    pub episodes: Vec<MediaNode>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchPage {
    pub results: Vec<MediaNode>,
    pub page: u32,
    pub total_pages: u32,
}

/// Pre-resolved daily season snapshot served by the metadata gateway so clients
/// can hydrate the entire calendar in one request instead of hundreds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SeasonBundle {
    #[serde(default)]
    pub images: Option<artwork::ImageConfiguration>,
    #[serde(default)]
    pub items: Vec<SeasonBundleItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeasonBundleItem {
    pub anime_id: i64,
    pub work: Work,
    #[serde(default)]
    pub season: Option<ProviderRef>,
    #[serde(default)]
    pub subjects: Vec<ProviderRef>,
    #[serde(default)]
    pub summary: String,
}
impl SeasonBundleItem {
    pub fn validate(&self) -> Result<()> {
        if self.anime_id <= 0 {
            return Err(Error::invalid("新番缓存条目标识不正确"));
        }
        self.work.node.validate()?;
        self.work.node.validate_artwork()?;
        for season in &self.work.seasons {
            season.validate()?;
            season.validate_artwork()?;
        }
        for split in &self.work.split_seasons {
            split.node.validate()?;
            split.node.validate_artwork()?;
            for episode in &split.episodes {
                episode.validate()?;
                episode.validate_artwork()?;
            }
        }
        if let Some(season) = &self.season {
            season.validate()?;
        }
        for subject in &self.subjects {
            subject.validate()?;
        }
        Ok(())
    }
}
fn id(body: &Value) -> Result<u64> {
    body["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| Error::invalid("TMDB响应缺少有效ID"))
}
fn number(body: &Value, key: &str) -> Result<u32> {
    body[key]
        .as_u64()
        .and_then(|n| n.try_into().ok())
        .ok_or_else(|| Error::invalid("TMDB响应缺少季集位置"))
}
fn text(body: &Value, key: &str) -> String {
    body[key].as_str().unwrap_or_default().trim().into()
}
fn node(
    body: &Value,
    reference: ProviderRef,
    parent: Option<MediaId>,
    root: Option<MediaId>,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<MediaNode> {
    let kind = reference.kind;
    let id = MediaId::for_ref(&reference);
    let movie = kind == Kind::Movie;
    let title = text(body, if movie { "title" } else { "name" });
    let original_title = text(
        body,
        if movie {
            "original_title"
        } else {
            "original_name"
        },
    );
    let title = if title.is_empty() {
        original_title.clone()
    } else {
        title
    };
    let value = MediaNode {
        root: root.unwrap_or_else(|| id.clone()),
        id,
        kind,
        parent,
        season_number: season,
        episode_number: episode,
        original_label: match (season, episode) {
            (Some(s), Some(e)) => format!("S{s:02}E{e:02}"),
            _ => String::new(),
        },
        title,
        original_title,
        overview: text(body, "overview"),
        date: text(
            body,
            if movie {
                "release_date"
            } else if kind == Kind::Tv {
                "first_air_date"
            } else {
                "air_date"
            },
        ),
        original_language: text(body, "original_language"),
        runtime_minutes: body["runtime"].as_u64().and_then(|v| v.try_into().ok()),
        artwork: Artwork {
            poster: artwork::path_asset(
                &reference,
                ArtworkKind::Poster,
                &text(body, "poster_path"),
            ),
            backdrop: artwork::path_asset(
                &reference,
                ArtworkKind::Backdrop,
                &text(body, "backdrop_path"),
            ),
            still: artwork::path_asset(&reference, ArtworkKind::Still, &text(body, "still_path")),
            logo: None,
        },
        reference: Some(reference),
        revision: 0,
    };
    value.validate()?;
    Ok(value)
}
pub fn parse_work(kind: Kind, expected: u64, body: &Value) -> Result<Work> {
    if !matches!(kind, Kind::Tv | Kind::Movie) || id(body)? != expected {
        return Err(Error::invalid("TMDB作品身份与请求不一致"));
    }
    let reference = ProviderRef::tmdb(kind, expected)?;
    let work = node(body, reference, None, None, None, None)?;
    let mut seasons = Vec::new();
    if kind == Kind::Tv {
        let rows = body["seasons"]
            .as_array()
            .ok_or_else(|| Error::invalid("TMDB剧集缺少季列表"))?;
        let mut positions = std::collections::HashSet::new();
        for row in rows {
            let number = number(row, "season_number")?;
            if !positions.insert(number) {
                return Err(Error::invalid("TMDB季位置重复"));
            }
            let reference = ProviderRef::tmdb(Kind::Season, id(row)?)?;
            let mut row = row.clone();
            if text(&row, "name").is_empty() {
                row["name"] = Value::String(if number == 0 {
                    "特别篇".into()
                } else {
                    format!("第 {number} 季")
                });
            }
            seasons.push(node(
                &row,
                reference,
                Some(work.id.clone()),
                Some(work.id.clone()),
                Some(number),
                None,
            )?);
        }
        seasons.sort_by_key(|s| s.season_number);
    }
    let mut active_dates = Vec::new();
    if kind == Kind::Tv {
        for candidate in [
            text(body, "last_air_date"),
            text(&body["last_episode_to_air"], "air_date"),
            text(&body["next_episode_to_air"], "air_date"),
        ] {
            if !candidate.is_empty() && !active_dates.contains(&candidate) {
                active_dates.push(candidate);
            }
        }
    }
    Ok(Work {
        node: work,
        seasons,
        active_dates,
        original_overview: String::new(),
        split_seasons: Vec::new(),
    })
}

/// Choose a TMDB `episode_groups` entry of type `6` (`Seasons`) that splits a
/// single-season TV entry into two or more seasons.
pub fn choose_seasons_group(body: &Value) -> Option<String> {
    let rows = body["results"].as_array()?;
    rows.iter()
        .filter(|row| {
            row["type"].as_u64() == Some(6)
                && row["group_count"].as_u64().unwrap_or(0) >= 2
                && row["id"]
                    .as_str()
                    .is_some_and(|s| crate::is_tmdb_group_id(&s.trim().to_ascii_lowercase()))
        })
        .max_by_key(|row| {
            (
                row["episode_count"].as_u64().unwrap_or(0),
                row["group_count"].as_u64().unwrap_or(0),
            )
        })
        .and_then(|row| row["id"].as_str())
        .map(|s| s.trim().to_ascii_lowercase())
}

impl Work {
    /// Expand a single-regular-season anime into its real broadcast seasons
    /// using TMDB's `/tv/episode_group/{id}` (`type == 6`) response.
    /// Season 1 retains its numeric TMDB Season `ProviderRef` so existing links
    /// stay valid; subsequent seasons use their immutable TMDB group IDs, and
    /// every episode retains its numeric TMDB Episode `ProviderRef` with
    /// season-relative numbering (`1..=N`).
    pub fn apply_episode_group(&mut self, body: &Value) -> Result<bool> {
        if self.node.kind != Kind::Tv || body["type"].as_u64() != Some(6) {
            return Ok(false);
        }
        let Some(groups) = body["groups"].as_array() else {
            return Ok(false);
        };
        let regular_groups = groups
            .iter()
            .filter(|g| g["order"].as_u64().is_some_and(|n| n >= 1))
            .count();
        if regular_groups < 2 {
            return Ok(false);
        }
        let existing_s0 = self
            .seasons
            .iter()
            .find(|s| s.season_number == Some(0))
            .cloned();
        let existing_s1 = self
            .seasons
            .iter()
            .find(|s| s.season_number == Some(1))
            .cloned();
        let mut new_seasons = Vec::new();
        let mut split_seasons = Vec::new();
        let mut seen_orders = std::collections::HashSet::new();
        let mut seen_episodes = std::collections::HashSet::new();

        for grp in groups {
            let Some(order_u64) = grp["order"].as_u64() else {
                continue;
            };
            let Ok(season_no) = u32::try_from(order_u64) else {
                continue;
            };
            if season_no > 1000 || !seen_orders.insert(season_no) {
                continue;
            }
            let Some(ep_rows) = grp["episodes"].as_array() else {
                continue;
            };
            if ep_rows.is_empty() {
                continue;
            }
            let season_ref = if season_no == 0 {
                if let Some(s0) = &existing_s0
                    && let Some(r) = &s0.reference
                {
                    r.clone()
                } else if let Some(gid) = grp["id"].as_str() {
                    ProviderRef::tmdb_group_season(gid)?
                } else {
                    continue;
                }
            } else if season_no == 1
                && let Some(s1) = &existing_s1
                && let Some(r) = &s1.reference
            {
                r.clone()
            } else if let Some(gid) = grp["id"].as_str() {
                ProviderRef::tmdb_group_season(gid)?
            } else {
                continue;
            };
            let season_id = MediaId::for_ref(&season_ref);
            let mut parsed_eps = Vec::new();
            let mut first_date = String::new();
            for (idx, row) in ep_rows.iter().enumerate() {
                let Ok(ep_id) = id(row) else {
                    continue;
                };
                if !seen_episodes.insert(ep_id) {
                    continue;
                }
                let ep_no = (idx as u32) + 1;
                let ep_node = node(
                    row,
                    ProviderRef::tmdb(Kind::Episode, ep_id)?,
                    Some(season_id.clone()),
                    Some(self.node.id.clone()),
                    Some(season_no),
                    Some(ep_no),
                )?;
                if first_date.is_empty() && !ep_node.date.is_empty() {
                    first_date.clone_from(&ep_node.date);
                }
                parsed_eps.push(ep_node);
            }
            if parsed_eps.is_empty() {
                continue;
            }
            let raw_name = text(grp, "name");
            let title = if season_no == 0 {
                existing_s0
                    .as_ref()
                    .map(|s| s.title.clone())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| "特别篇".into())
            } else if raw_name.is_empty()
                || raw_name.eq_ignore_ascii_case(&format!("Season {season_no}"))
            {
                format!("第 {season_no} 季")
            } else {
                raw_name.clone()
            };
            let season_node = MediaNode {
                id: season_id,
                kind: Kind::Season,
                parent: Some(self.node.id.clone()),
                root: self.node.id.clone(),
                season_number: Some(season_no),
                episode_number: None,
                original_label: raw_name,
                title,
                original_title: String::new(),
                overview: if season_no == 1 {
                    existing_s1
                        .as_ref()
                        .map(|s| s.overview.clone())
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| text(grp, "description"))
                } else {
                    text(grp, "description")
                },
                date: first_date,
                original_language: String::new(),
                runtime_minutes: None,
                reference: Some(season_ref),
                artwork: if season_no == 1 {
                    existing_s1
                        .as_ref()
                        .map(|s| s.artwork.clone())
                        .unwrap_or_default()
                } else if season_no == 0 {
                    existing_s0
                        .as_ref()
                        .map(|s| s.artwork.clone())
                        .unwrap_or_default()
                } else {
                    Artwork::default()
                },
                revision: 0,
            };
            season_node.validate()?;
            season_node.validate_artwork()?;
            new_seasons.push(season_node.clone());
            split_seasons.push(Season {
                node: season_node,
                episodes: parsed_eps,
            });
        }
        if split_seasons
            .iter()
            .filter(|s| s.node.season_number.is_some_and(|n| n > 0))
            .count()
            < 2
        {
            return Ok(false);
        }
        if !new_seasons.iter().any(|s| s.season_number == Some(0))
            && let Some(s0) = existing_s0
        {
            new_seasons.push(s0);
        }
        new_seasons.sort_by_key(|s| s.season_number);
        split_seasons.sort_by_key(|s| s.node.season_number);
        self.seasons = new_seasons;
        self.split_seasons = split_seasons;
        Ok(true)
    }
}
pub fn parse_season(work: &Work, requested: u32, body: &Value) -> Result<Season> {
    let known = work
        .seasons
        .iter()
        .find(|s| s.season_number == Some(requested))
        .ok_or_else(|| Error::invalid("作品没有这季"))?;
    if id(body)?.to_string()
        != known
            .reference
            .as_ref()
            .ok_or_else(|| Error::invalid("季缺少身份"))?
            .id
        || number(body, "season_number")? != requested
    {
        return Err(Error::invalid("TMDB季身份与请求不一致"));
    }
    let season = node(
        body,
        known.reference.clone().unwrap(),
        Some(work.node.id.clone()),
        Some(work.node.id.clone()),
        Some(requested),
        None,
    )?;
    let rows = body["episodes"]
        .as_array()
        .ok_or_else(|| Error::invalid("TMDB季缺少分集列表"))?;
    let mut episodes = Vec::new();
    let mut positions = std::collections::HashSet::new();
    let mut ids = std::collections::HashSet::new();
    for row in rows {
        let position = number(row, "episode_number")?;
        if number(row, "season_number")? != requested
            || !positions.insert(position)
            || !ids.insert(id(row)?)
        {
            return Err(Error::invalid("TMDB分集位置或身份冲突"));
        }
        if row.get("show_id").is_some()
            && row["show_id"].as_u64().map(|v| v.to_string()).as_deref()
                != work.node.reference.as_ref().map(|r| r.id.as_str())
        {
            return Err(Error::invalid("分集属于另一部作品"));
        }
        episodes.push(node(
            row,
            ProviderRef::tmdb(Kind::Episode, id(row)?)?,
            Some(season.id.clone()),
            Some(work.node.id.clone()),
            Some(requested),
            Some(position),
        )?);
    }
    episodes.sort_by_key(|e| e.episode_number);
    Ok(Season {
        node: season,
        episodes,
    })
}
pub fn parse_search(kind: Kind, body: &Value) -> Result<SearchPage> {
    if !matches!(kind, Kind::Tv | Kind::Movie) {
        return Err(Error::invalid("搜索类型只能是电影或剧集"));
    }
    let rows = body["results"]
        .as_array()
        .ok_or_else(|| Error::invalid("TMDB搜索响应不正确"))?;
    if rows.len() > 1000 {
        return Err(Error::invalid("TMDB搜索结果太多"));
    }
    let mut results = Vec::new();
    for row in rows {
        results.push(node(
            row,
            ProviderRef::tmdb(kind, id(row)?)?,
            None,
            None,
            None,
            None,
        )?);
    }
    Ok(SearchPage {
        results,
        page: number(body, "page")?,
        total_pages: number(body, "total_pages")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn zero_is_a_season_not_missing_and_movies_have_no_fake_episode() {
        let work = parse_work(Kind::Tv, 1, &json!({"id":1,"name":"Show","seasons":[{"id":20,"season_number":0,"name":"特别篇"},{"id":21,"season_number":1,"name":"第一季"}]})).unwrap();
        assert_eq!(work.seasons[0].season_number, Some(0));
        let season = parse_season(&work, 0, &json!({"id":20,"name":"特别篇","season_number":0,"episodes":[{"id":30,"name":"SP","season_number":0,"episode_number":1,"show_id":1}]})).unwrap();
        assert_eq!(season.episodes[0].original_label, "S00E01");
        assert!(
            parse_season(
                &work,
                1,
                &json!({"id":20,"name":"特别篇","season_number":0,"episodes":[]})
            )
            .is_err()
        );
        let movie = parse_work(Kind::Movie, 1, &json!({"id":1,"title":"Film"})).unwrap();
        assert!(movie.seasons.is_empty());
        assert!(movie.node.parent.is_none());
        assert_ne!(movie.node.id, work.node.id);
    }
    #[test]
    fn malformed_is_not_empty_and_wrong_identity_is_rejected() {
        assert!(parse_work(Kind::Tv, 1, &json!({"id":2,"name":"Other","seasons":[]})).is_err());
        assert!(parse_search(Kind::Tv, &json!({})).is_err());
        let continuous = parse_work(
            Kind::Tv,
            9,
            &json!({
                "id":9,"name":"Continuous","first_air_date":"2023-10-22",
                "last_air_date":"2026-11-06",
                "next_episode_to_air":{"air_date":"2026-11-13"},
                "seasons":[{"id":91,"season_number":1,"name":"第 1 季","air_date":"2023-10-22"}]
            }),
        )
        .unwrap();
        assert!(continuous.airs_in(2023));
        assert!(continuous.airs_in(2026));
        assert!(!continuous.airs_in(2025));
    }

    #[test]
    fn episode_group_expands_single_season_anime_into_real_seasons() {
        let groups_list = json!({
            "results": [
                {"id": "111111111111111111111111", "type": 3, "episode_count": 60, "group_count": 3},
                {"id": "6abfdb678348fc8997c4b3fe", "type": 6, "episode_count": 60, "group_count": 3}
            ]
        });
        assert_eq!(
            choose_seasons_group(&groups_list).as_deref(),
            Some("6abfdb678348fc8997c4b3fe")
        );

        let mut work = parse_work(
            Kind::Tv,
            220542,
            &json!({
                "id": 220542,
                "name": "药屋少女的呢喃",
                "original_name": "薬屋のひとりごと",
                "original_language": "ja",
                "seasons": [
                    {"id": 329445, "season_number": 0, "name": "特别篇"},
                    {"id": 329446, "season_number": 1, "name": "第 1 季", "air_date": "2023-10-22", "poster_path": "/s1.jpg"}
                ]
            }),
        )
        .unwrap();
        let applied = work
            .apply_episode_group(&json!({
                "type": 6,
                "groups": [
                    {
                        "id": "6533f8a10000000000000001",
                        "name": "Season 1",
                        "order": 1,
                        "episodes": [{"id": 4800001, "name": "猫猫", "air_date": "2023-10-22"}]
                    },
                    {
                        "id": "6533f8a10000000000000002",
                        "name": "Season 2",
                        "order": 2,
                        "episodes": [{"id": 5800001, "name": "后宫", "air_date": "2025-01-10"}]
                    },
                    {
                        "id": "6abfdb678348fc8997c4b3fe",
                        "name": "Season 3",
                        "order": 3,
                        "episodes": [{"id": 7810328, "name": "蝗虫", "air_date": "2026-10-02"}]
                    }
                ]
            }))
            .unwrap();
        assert!(applied);
        assert_eq!(work.seasons.len(), 4);
        assert_eq!(work.split_seasons.len(), 3);
        let s1 = work
            .seasons
            .iter()
            .find(|s| s.season_number == Some(1))
            .unwrap();
        let s3 = work
            .seasons
            .iter()
            .find(|s| s.season_number == Some(3))
            .unwrap();
        assert_eq!(s1.reference.as_ref().unwrap().id, "329446");
        assert_eq!(
            s3.reference.as_ref().unwrap().id,
            "6abfdb678348fc8997c4b3fe"
        );
        assert_eq!(s3.title, "第 3 季");
        assert_eq!(s3.date, "2026-10-02");
        let s3_eps = &work
            .split_seasons
            .iter()
            .find(|s| s.node.season_number == Some(3))
            .unwrap()
            .episodes;
        assert_eq!(s3_eps[0].season_number, Some(3));
        assert_eq!(s3_eps[0].episode_number, Some(1));
        assert_eq!(s3_eps[0].reference.as_ref().unwrap().id, "7810328");
    }
}
