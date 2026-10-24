//! Public Bangumi catalogue. Website login cookies are never sent to the API.
use crate::{Details, EpisodeInfo, Error, Found, Intro, Result, Tag};
use serde_json::Value;

// Separate from positive dandanplay IDs and negative custom-server IDs, and
// exactly representable by JavaScript. Never send these IDs to dandanplay.
const ID_BASE: i64 = 1_000_000_000_000;

pub fn local_id(subject: i64) -> Option<i64> {
    (subject > 0 && subject < ID_BASE).then(|| ID_BASE + subject)
}

pub fn subject_id(id: i64) -> Option<i64> {
    (id > ID_BASE && id < 2 * ID_BASE).then(|| id - ID_BASE)
}

pub fn user_agent() -> String {
    format!(
        "MCDFsteve/Erika/{} ({}) (https://github.com/MCDFsteve/Erika)",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS
    )
}

/// Public cover endpoint, including its CDN redirect. No login credentials are
/// attached; callers keep their existing cover until these bytes are validated.
pub fn poster(subject: i64) -> Result<Vec<u8>> {
    if local_id(subject).is_none() {
        return Err(Error::new("无效的 Bangumi 条目"));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(15)))
            .user_agent(user_agent())
            .build()
            .into();
        let mut response = agent
            .get(format!(
                "https://api.bgm.tv/v0/subjects/{subject}/image?type=large"
            ))
            .header("Accept", "image/*")
            .call()
            .map_err(|_| Error::new("暂时取不到 Bangumi 高清海报"))?;
        if !response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("image/"))
        {
            return Err(Error::new("Bangumi 海报格式不正确"));
        }
        response
            .body_mut()
            .with_config()
            .limit(12 * 1024 * 1024)
            .read_to_vec()
            .map_err(|_| Error::new("Bangumi 海报下载未完成"))
    }
    #[cfg(target_arch = "wasm32")]
    Err(Error::new("网页版暂时连不上 Bangumi"))
}

fn request(path: &str, body: Option<Value>) -> Result<Value> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(10)))
            .user_agent(user_agent())
            .build()
            .into();
        let url = format!("https://api.bgm.tv{path}");
        let response = match body {
            Some(body) => agent
                .post(&url)
                .header("Accept", "application/json")
                .send_json(body),
            None => agent.get(&url).header("Accept", "application/json").call(),
        };
        let text = response
            .and_then(|mut r| r.body_mut().read_to_string())
            .map_err(|_| Error::new("暂时连不上 Bangumi"))?;
        serde_json::from_str(&text).map_err(|_| Error::new("Bangumi 数据格式不正确"))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (path, body);
        Err(Error::new("网页版暂时连不上 Bangumi"))
    }
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
}

fn title(value: &Value) -> String {
    let chinese = text(value, "name_cn");
    if chinese.is_empty() {
        text(value, "name")
    } else {
        chinese
    }
    .into()
}

fn image(value: &Value) -> String {
    ["large", "common", "medium", "small"]
        .into_iter()
        .map(|key| text(&value["images"], key))
        .find(|url| !url.is_empty())
        .unwrap_or_default()
        .replacen("http://", "https://", 1)
}

fn anime_id(value: &Value) -> Option<i64> {
    if value["type"].as_i64() != Some(2) || value["nsfw"].as_bool() == Some(true) {
        return None;
    }
    local_id(value["id"].as_i64()?)
}

pub fn season() -> Result<Vec<Intro>> {
    parse_calendar(&request("/calendar", None)?)
}

fn parse_calendar(body: &Value) -> Result<Vec<Intro>> {
    let days = body
        .as_array()
        .ok_or_else(|| Error::new("Bangumi 新番列表格式不正确"))?;
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for day in days {
        let weekday = day["weekday"]["id"]
            .as_i64()
            .filter(|n| (1..=7).contains(n));
        for item in day["items"].as_array().into_iter().flatten() {
            let Some(anime_id) = anime_id(item) else {
                continue;
            };
            if !seen.insert(anime_id) {
                continue;
            }
            result.push(Intro {
                anime_id,
                anime_title: title(item),
                image_url: image(item),
                summary: text(item, "summary").into(),
                air_day: weekday.map(|n| n % 7),
                rating: item["rating"]["score"].as_f64().unwrap_or_default(),
                is_on_air: true,
                is_favorited: false,
                is_restricted: false,
            });
        }
    }
    if result.is_empty() {
        return Err(Error::new("Bangumi 暂时没有新番数据"));
    }
    Ok(result)
}

pub fn search(keyword: &str) -> Result<Vec<Found>> {
    let body = request(
        "/v0/search/subjects?limit=6",
        Some(serde_json::json!({
            "keyword": keyword.trim(), "sort": "match", "filter": {"type": [2], "nsfw": false}
        })),
    )?;
    parse_search(&body)
}

fn parse_search(body: &Value) -> Result<Vec<Found>> {
    let rows = body["data"]
        .as_array()
        .ok_or_else(|| Error::new("Bangumi 搜索结果格式不正确"))?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            Some(Found {
                anime_id: anime_id(row)?,
                anime_title: title(row),
                image_url: image(row),
                summary: text(row, "summary").into(),
                type_description: text(row, "platform").into(),
                episode_count: row["eps"]
                    .as_i64()
                    .or_else(|| row["total_episodes"].as_i64())
                    .unwrap_or_default(),
                rating: row["rating"]["score"].as_f64().unwrap_or_default(),
            })
        })
        .collect())
}

pub fn details(subject: i64) -> Result<Details> {
    let body = request(&format!("/v0/subjects/{subject}"), None)?;
    let mut details = parse_details(subject, &body)?;
    // Metadata remains useful even if the separate episode endpoint is down.
    let mut episodes = Vec::new();
    for offset in (0..2000).step_by(100) {
        let Ok(page) = request(
            &format!("/v0/episodes?subject_id={subject}&type=0&limit=100&offset={offset}"),
            None,
        ) else {
            break;
        };
        let Some(rows) = page["data"].as_array() else {
            break;
        };
        episodes.extend(rows.iter().filter_map(parse_episode));
        if rows.len() < 100
            || offset + rows.len() >= page["total"].as_u64().unwrap_or_default() as usize
        {
            break;
        }
    }
    if !episodes.is_empty() {
        episodes.sort_by_key(|e| e.episode_number.parse::<i64>().unwrap_or_default());
        episodes.dedup_by(|a, b| a.episode_number == b.episode_number);
        details.episodes = episodes;
    }
    Ok(details)
}

fn parse_episode(row: &Value) -> Option<EpisodeInfo> {
    let number = row["sort"].as_f64()?;
    // The player addresses normal episodes by integer number. Specials stay out.
    if row["type"].as_i64() != Some(0) || number < 1.0 || number.fract() != 0.0 {
        return None;
    }
    Some(EpisodeInfo {
        episode_id: local_id(row["id"].as_i64()?)?,
        episode_title: title(row),
        episode_number: (number as i64).to_string(),
        air_date: date(text(row, "airdate")),
    })
}

fn date(date: &str) -> Option<String> {
    (!date.is_empty()).then(|| format!("{date}T00:00:00"))
}

fn parse_details(subject: i64, body: &Value) -> Result<Details> {
    let id = anime_id(body)
        .filter(|id| subject_id(*id) == Some(subject))
        .ok_or_else(|| Error::new("未找到对应的 Bangumi 动画条目"))?;
    let count = body["eps"]
        .as_i64()
        .or_else(|| body["total_episodes"].as_i64())
        .unwrap_or_default()
        .clamp(0, 2000);
    Ok(Details {
        anime_id: id,
        anime_title: title(body),
        image_url: image(body),
        summary: text(body, "summary").into(),
        bangumi_url: format!("https://bgm.tv/subject/{subject}"),
        rating: body["rating"]["score"].as_f64().unwrap_or_default(),
        tags: body["tags"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|tag| Tag {
                name: text(tag, "name").into(),
            })
            .collect(),
        type_description: text(body, "platform").into(),
        episodes: (1..=count)
            .map(|number| EpisodeInfo {
                episode_id: 0,
                episode_number: number.to_string(),
                episode_title: String::new(),
                air_date: if number == 1 {
                    date(text(body, "date"))
                } else {
                    None
                },
            })
            .collect(),
        is_on_air: false,
        air_day: None,
        is_favorited: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ids_do_not_overlap_other_providers_or_lose_web_precision() {
        assert_eq!(subject_id(local_id(393010).unwrap()), Some(393010));
        for id in [-393010, 0, 17527, ID_BASE, 2 * ID_BASE, i64::MAX] {
            assert_eq!(subject_id(id), None);
        }
        assert!(local_id(0).is_none());
        assert!(local_id(ID_BASE).is_none());
        let id = local_id(ID_BASE - 1).unwrap();
        assert_eq!((id as f64) as i64, id);
    }

    #[test]
    fn calendar_normalizes_sunday_images_and_filters_other_types() {
        let item = json!({"id": 393010, "type": 2, "name": "original", "name_cn": "中文", "images":{"large":"http://lain.bgm.tv/a.jpg"}});
        let rows = parse_calendar(&json!([{"weekday":{"id":7},"items":[item, item, {"id":4,"type":1}, {"id":5,"type":2,"nsfw":true}]}])).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].air_day, Some(0));
        assert_eq!(rows[0].anime_title, "中文");
        assert!(rows[0].image_url.starts_with("https://"));
        assert!(parse_calendar(&json!({"error":"offline"})).is_err());
    }

    #[test]
    fn search_and_details_preserve_summary_and_episode_identity() {
        let body = json!({"id":393010,"type":2,"name":"原名","summary":"简介","eps":13,"date":"2026-10-07","tags":[{"name":"奇幻"}]});
        let result = parse_search(&json!({"data":[body]})).unwrap();
        assert_eq!(result[0].summary, "简介");
        let details = parse_details(393010, &body).unwrap();
        assert_eq!(details.episodes.len(), 13);
        assert_eq!(details.bangumi_url, "https://bgm.tv/subject/393010");
        assert!(parse_details(17527, &body).is_err());
        assert!(parse_episode(&json!({"id":1,"sort":1.5,"type":0})).is_none());
        let episode = parse_episode(
            &json!({"id":1743433,"sort":1,"type":0,"name_cn":"第一集","airdate":"2026-10-07"}),
        )
        .unwrap();
        assert_eq!(episode.episode_number, "1");
        assert_eq!(episode.air_date.as_deref(), Some("2026-10-07T00:00:00"));
        assert_ne!(episode.episode_id, 1743433);
    }
}
