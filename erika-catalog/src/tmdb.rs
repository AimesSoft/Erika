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
    Ok(Work {
        node: work,
        seasons,
    })
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
    }
}
