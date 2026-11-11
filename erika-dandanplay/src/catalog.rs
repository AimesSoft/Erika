//! Lossless recognition-side DTOs. These are deliberately separate from the
//! legacy UI DTOs: service season IDs and labels are not canonical episode IDs.
use super::{Error, Result};
use erika_catalog::{Kind, Provider, ProviderRef, resolve::Recognition};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

fn nullable_vec<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Vec<T>, D::Error> {
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OnlineDatabase {
    #[serde(default, deserialize_with = "super::text_or_null")]
    pub name: String,
    #[serde(default, deserialize_with = "super::text_or_null")]
    pub url: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Season {
    #[serde(default, deserialize_with = "super::text_or_null")]
    pub id: String,
    #[serde(default, deserialize_with = "super::text_or_null")]
    pub name: String,
    #[serde(default)]
    pub air_date: Option<String>,
    #[serde(default)]
    pub episode_count: u32,
    #[serde(default, deserialize_with = "super::text_or_null")]
    pub summary: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Episode {
    pub episode_id: u64,
    #[serde(default, deserialize_with = "super::text_or_null")]
    pub season_id: String,
    #[serde(default, deserialize_with = "super::text_or_null")]
    pub episode_title: String,
    #[serde(default, deserialize_with = "super::text_or_null")]
    pub episode_number: String,
    #[serde(default)]
    pub last_watched: Option<String>,
    #[serde(default)]
    pub air_date: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Details {
    pub anime_id: u64,
    #[serde(default, deserialize_with = "super::text_or_null")]
    pub bangumi_id: String,
    #[serde(default, deserialize_with = "super::text_or_null")]
    pub anime_title: String,
    #[serde(default, deserialize_with = "super::text_or_null")]
    pub bangumi_url: String,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub online_databases: Vec<OnlineDatabase>,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub seasons: Vec<Season>,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub episodes: Vec<Episode>,
    #[serde(default)]
    pub is_favorited: bool,
    #[serde(default)]
    pub favorite_status: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Matched {
    pub episode_id: u64,
    pub anime_id: u64,
    #[serde(default, deserialize_with = "super::text_or_null")]
    pub anime_title: String,
    #[serde(default, deserialize_with = "super::text_or_null")]
    pub episode_title: String,
    #[serde(default, rename = "type", deserialize_with = "super::text_or_null")]
    pub media_type: String,
    #[serde(default)]
    pub shift: f64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MatchReply {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    is_matched: bool,
    #[serde(default, deserialize_with = "nullable_vec")]
    matches: Vec<Matched>,
}
#[derive(Deserialize)]
struct DetailsReply {
    success: bool,
    bangumi: Option<Details>,
}

/// Accept a validated external URL, not a number that happens to look like a
/// Bangumi subject ID. dandanplay's `bangumiId` lives in a different namespace.
pub fn bangumi_url(text: &str) -> Option<ProviderRef> {
    let url = url::Url::parse(text).ok()?;
    if !matches!(url.scheme(), "https" | "http")
        || !matches!(
            url.host_str(),
            Some(
                "bgm.tv"
                    | "bangumi.tv"
                    | "chii.in"
                    | "www.bgm.tv"
                    | "www.bangumi.tv"
                    | "www.chii.in"
            )
        )
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    let segments: Vec<_> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    if segments.len() != 2 || segments[0] != "subject" {
        return None;
    }
    let id = segments[1].parse::<u64>().ok().filter(|id| *id > 0)?;
    Some(ProviderRef {
        provider: Provider::Bangumi,
        scope: String::new(),
        kind: Kind::Subject,
        id: id.to_string(),
    })
}
impl Details {
    pub fn tmdb_refs(&self) -> Vec<ProviderRef> {
        let mut refs = Vec::new();
        for link in &self.online_databases {
            if let Some(reference) = erika_catalog::resolve::tmdb_url(&link.url) {
                if !refs.contains(&reference) {
                    refs.push(reference);
                }
            }
        }
        refs
    }
    pub fn bangumi_ref(&self) -> Option<ProviderRef> {
        let mut refs = Vec::new();
        for value in std::iter::once(self.bangumi_url.as_str())
            .chain(self.online_databases.iter().map(|d| d.url.as_str()))
        {
            if let Some(reference) = bangumi_url(value) {
                if !refs.contains(&reference) {
                    refs.push(reference);
                }
            }
        }
        (refs.len() == 1).then(|| refs.remove(0))
    }
}
/// Missing details still leave the service's danmaku candidate, but never
/// manufacture a canonical episode by calculating `% 10000`. A successful
/// hashAndFileName request does not prove which input the service matched.
pub fn evidence(matched: &Matched, details: Option<&Details>) -> Result<Recognition> {
    if matched.episode_id == 0 || matched.anime_id == 0 || !matched.shift.is_finite() {
        return Err(Error::new("弹弹play 识别结果格式不正确"));
    }
    if details.is_some_and(|d| d.anime_id != matched.anime_id) {
        return Err(Error::new("弹弹play 详情与识别结果不一致"));
    }
    let mut result = Recognition {
        work: Some(ProviderRef {
            provider: Provider::Dandanplay,
            scope: String::new(),
            kind: Kind::Subject,
            id: details
                .filter(|d| !d.bangumi_id.is_empty())
                .map(|d| d.bangumi_id.clone())
                .unwrap_or_else(|| matched.anime_id.to_string()),
        }),
        episode: Some(ProviderRef {
            provider: Provider::Dandanplay,
            scope: String::new(),
            kind: Kind::Episode,
            id: matched.episode_id.to_string(),
        }),
        shift_seconds: matched.shift,
        ..Default::default()
    };
    for title in
        std::iter::once(matched.anime_title.as_str()).chain(details.map(|d| d.anime_title.as_str()))
    {
        if !title.trim().is_empty() && !result.titles.iter().any(|t| t == title.trim()) {
            result.titles.push(title.trim().to_owned());
        }
    }
    if let Some(details) = details {
        result.tmdb = details.tmdb_refs();
        result.bangumi_subject = details.bangumi_ref();
        let episodes: Vec<_> = details
            .episodes
            .iter()
            .filter(|e| e.episode_id == matched.episode_id)
            .collect();
        if episodes.len() == 1 {
            let episode = episodes[0];
            result.provider_season_id =
                (!episode.season_id.is_empty()).then(|| episode.season_id.clone());
            result.episode_label =
                (!episode.episode_number.is_empty()).then(|| episode.episode_number.clone());
            let label = episode.episode_number.trim();
            if !label.is_empty() && label.bytes().all(|b| b.is_ascii_digit()) {
                result.episode_number = label.parse().ok();
            } else if !label.is_empty() {
                let hints = erika_catalog::names::parse(&format!("Episode {label}"));
                if hints.season.is_some() && hints.episodes.len() == 1 {
                    result.season_number = hints.season;
                    result.episode_number = hints.episodes.first().copied();
                } else {
                    result.special_label = Some(label.into());
                }
            }
        }
    }
    Ok(result)
}

/// Calendar membership comes from the server even without an account. Display
/// metadata is resolved separately through the canonical TMDB catalogue.
pub async fn season(public: bool) -> Result<Vec<super::Intro>> {
    let token = if public {
        None
    } else {
        Some(super::token().ok_or_else(Error::sign_in)?)
    };
    let reply: super::ShinReply = request(
        "GET",
        if public {
            "/public/season"
        } else {
            "/api/v2/bangumi/shin"
        },
        if public {
            ""
        } else {
            "?filterAdultContent=true"
        },
        None,
        token.as_deref(),
    )
    .await?;
    if reply.success {
        Ok(reply.bangumi_list)
    } else {
        Err(Error::new("新番列表取不到"))
    }
}

pub async fn details(id: &str, public: bool) -> Result<Details> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(Error::new("弹弹play 作品标识不正确"));
    }
    let token = super::token();
    if !public && token.is_none() {
        return Err(Error::sign_in());
    }
    let path = if public {
        format!("/public/details/{id}")
    } else {
        format!("/api/v2/bangumi/{id}")
    };
    let reply: DetailsReply = request(
        "GET",
        &path,
        "",
        None,
        if public { None } else { token.as_deref() },
    )
    .await?;
    reply
        .bangumi
        .filter(|_| reply.success)
        .ok_or_else(|| Error::new("弹弹play 详情取不到"))
}
/// Read a specific account with its captured credential, never the currently
/// signed-in account after a worker has already started.
pub async fn account_details(session: &super::auth::Session, id: &str) -> Result<Details> {
    session.check()?;
    if !session.account().starts_with("dandanplay:")
        || id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(Error::new("弹弹play 作品标识不正确"));
    }
    let reply: DetailsReply = request(
        "GET",
        &format!("/api/v2/bangumi/{id}"),
        "",
        None,
        Some(&session.secret),
    )
    .await?;
    session.check()?;
    reply
        .bangumi
        .filter(|d| reply.success && (d.anime_id.to_string() == id || d.bangumi_id == id))
        .ok_or_else(|| Error::new("弹弹play 返回的作品不一致"))
}

pub async fn account_watched(session: &super::auth::Session, episode: &str) -> Result<()> {
    session.check()?;
    if !session.account().starts_with("dandanplay:") {
        return Err(Error::new("账号来源不正确"));
    }
    let id = episode
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| Error::new("弹弹play 分集标识不正确"))?;
    let reply: serde_json::Value = request(
        "POST",
        "/api/v2/playhistory",
        "",
        Some(serde_json::json!({
            "episodeIdList":[id],"addToFavorite":false,"rating":0
        })),
        Some(&session.secret),
    )
    .await?;
    acknowledged(reply)
}

pub async fn account_follow(
    session: &super::auth::Session,
    anime: u64,
    status: &str,
) -> Result<()> {
    session.check()?;
    if !session.account().starts_with("dandanplay:")
        || anime == 0
        || !matches!(status, "favorited" | "finished" | "abandoned")
    {
        return Err(Error::new("弹弹play 收藏参数不正确"));
    }
    let reply: serde_json::Value = request(
        "POST",
        "/api/v2/favorite",
        "",
        Some(serde_json::json!({
            "animeId":anime,"favoriteStatus":status,"rating":0
        })),
        Some(&session.secret),
    )
    .await?;
    acknowledged(reply)
}
fn acknowledged(reply: serde_json::Value) -> Result<()> {
    if reply["success"].as_bool() == Some(true) {
        Ok(())
    } else {
        Err(Error::new("弹弹play 未确认保存结果，请回读后重试"))
    }
}

pub use erika_catalog::resolve::FileResolution;

/// Shared by folder scanning and direct playback. The default order is always
/// dandanplay recognition, then filename/directory evidence with TMDB search.
/// A trusted existing identity is validated without another recognition call.
pub async fn resolve_file(
    client: &erika_catalog::Client,
    filename: &str,
    directory: &str,
    fingerprint: Option<(&str, u64)>,
    kind: Option<Kind>,
    known: Option<ProviderRef>,
) -> FileResolution {
    let hints = erika_catalog::names::with_directory(filename, directory);
    let mut recognition_error = None;
    let recognition = if known.is_none() {
        if let Some((hash, size)) = fingerprint {
            match recognize(filename, hash, size).await {
                Ok(value) => value,
                Err(error) => {
                    recognition_error = Some(error.message);
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };
    // Keep the independently obtained danmaku evidence even if TMDB is down.
    let resolution =
        erika_catalog::resolve::resolve(client, hints, kind, recognition.clone(), known).await;
    FileResolution {
        recognition,
        recognition_error,
        resolution,
    }
}

pub async fn recognize(file_name: &str, md5: &str, size: u64) -> Result<Option<Recognition>> {
    if file_name.len() > 4096 || md5.len() != 32 || !md5.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::new("识别文件参数不正确"));
    }
    let token = super::token().ok_or_else(Error::sign_in)?;
    let reply:MatchReply=request("POST","/api/v2/match","",Some(serde_json::json!({"fileName":file_name,"fileHash":md5,"fileSize":size,"matchMode":"hashAndFileName"})),Some(&token)).await?;
    if !reply.success {
        return Err(Error::new("弹弹play 没能匹配"));
    }
    if !reply.is_matched || reply.matches.len() != 1 {
        return Ok(None);
    }
    let matched = &reply.matches[0];
    let details = details(&matched.anime_id.to_string(), false).await.ok();
    evidence(matched, details.as_ref()).map(Some)
}

/// Read online danmaku independently from the TMDB content binding.
pub async fn comments(episode: i64) -> Result<(String, usize)> {
    if episode <= 0 {
        return Err(Error::new("弹弹play 集数标识不正确"));
    }
    let token = super::token().ok_or_else(Error::sign_in)?;
    let reply: super::CommentsReply = request(
        "GET",
        &format!("/api/v2/comment/{episode}"),
        "?withRelated=true&chConvert=1",
        None,
        Some(&token),
    )
    .await?;
    Ok(super::kernel_comments(reply.comments))
}

const GATEWAYS: &[&str] = &[
    "https://nipaplay.aimes-soft.com/dandanplay",
    "http://43.142.85.190/dandanplay",
];

async fn read_from<T: DeserializeOwned>(
    gateways: &[&str],
    path: &str,
    query: &str,
    token: Option<&str>,
) -> Result<T> {
    use base64::Engine as _;
    use sha2::Digest as _;
    #[cfg(not(target_arch = "wasm32"))]
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    #[cfg(target_arch = "wasm32")]
    let timestamp = (js_sys::Date::now() / 1000.) as i64;
    let signature =
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha256::digest(format!(
            "{}{}{}{}",
            super::APP_ID,
            timestamp,
            path,
            super::APP_SECRET
        )));
    let timestamp = timestamp.to_string();
    let authorization = token
        .filter(|_| !path.starts_with("/public/"))
        .map(|token| format!("Bearer {token}"));
    let mut headers = vec![
        ("Accept", "application/json"),
        ("X-AppId", super::APP_ID),
        ("X-Timestamp", timestamp.as_str()),
        ("X-Signature", signature.as_str()),
    ];
    if let Some(value) = &authorization {
        headers.push(("Authorization", value));
    }
    let mut last = Error::new("弹弹play 暂时无法连接");
    for base in gateways {
        let response = erika_catalog::http::get(
            &format!("{base}{path}{query}"),
            &headers,
            std::time::Duration::from_secs(15),
            8 * 1024 * 1024,
            false,
        )
        .await;
        let Ok(response) = response else { continue };
        if matches!(response.status, 401 | 403) {
            return Err(Error::sign_in());
        }
        if !(200..300).contains(&response.status) {
            last = Error::new(format!("弹弹play 请求失败（{}）", response.status));
            continue;
        }
        match serde_json::from_slice(&response.body) {
            Ok(value) => return Ok(value),
            Err(_) => last = Error::new("弹弹play 回答格式不正确"),
        }
    }
    Err(last)
}

async fn request<T: DeserializeOwned>(
    method: &str,
    path: &str,
    query: &str,
    body: Option<serde_json::Value>,
    token: Option<&str>,
) -> Result<T> {
    if method == "GET" && body.is_none() {
        return read_from(GATEWAYS, path, query, token).await;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        super::request(method, path, query, body, token)
    }
    #[cfg(target_arch = "wasm32")]
    {
        use base64::Engine as _;
        use sha2::Digest as _;
        let timestamp = (js_sys::Date::now() / 1000.) as i64;
        let signature =
            base64::engine::general_purpose::STANDARD.encode(sha2::Sha256::digest(format!(
                "{}{}{}{}",
                super::APP_ID,
                timestamp,
                path,
                super::APP_SECRET
            )));
        let mut last = Error::new("弹弹play 暂时无法连接");
        for base in [
            "https://nipaplay.aimes-soft.com/dandanplay",
            "http://43.142.85.190/dandanplay",
        ]
        .into_iter()
        .take(if super::may_retry(method, path) { 2 } else { 1 })
        {
            let mut req = gloo_net::http::Request::get(&format!("{base}{path}{query}"))
                .method(
                    method
                        .try_into()
                        .map_err(|_| Error::new("请求方法不正确"))?,
                )
                .header("X-AppId", super::APP_ID)
                .header("X-Timestamp", &timestamp.to_string())
                .header("X-Signature", &signature);
            if let Some(token) = token {
                req = req.header("Authorization", &format!("Bearer {token}"));
            }
            let response = if let Some(body) = &body {
                req.json(body)
                    .map_err(|_| Error::new("请求格式不正确"))?
                    .send()
                    .await
            } else {
                req.send().await
            };
            let Ok(response) = response else { continue };
            if matches!(response.status(), 401 | 403) {
                return Err(Error::sign_in());
            }
            if !response.ok() {
                last = Error::new(format!("弹弹play 请求失败（{}）", response.status()));
                continue;
            }
            if response
                .headers()
                .get("Content-Length")
                .and_then(|n| n.parse::<usize>().ok())
                .is_some_and(|n| n > 8 * 1024 * 1024)
            {
                return Err(Error::new("弹弹play 回答过大"));
            }
            let text = response
                .text()
                .await
                .map_err(|_| Error::new("弹弹play 回答不完整"))?;
            if text.len() > 8 * 1024 * 1024 {
                return Err(Error::new("弹弹play 回答过大"));
            }
            return serde_json::from_str(&text).map_err(|_| Error::new("弹弹play 回答格式不正确"));
        }
        Err(last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn public_calendar_is_signed_but_never_gets_account_credentials() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            for public in [true, false] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut header = Vec::new();
                let mut byte = [0];
                while !header.ends_with(b"\r\n\r\n") {
                    assert!(header.len() < 8192);
                    stream.read_exact(&mut byte).unwrap();
                    header.push(byte[0]);
                }
                let header = String::from_utf8(header).unwrap().to_ascii_lowercase();
                assert!(header.contains("x-signature:"));
                assert!(header.contains("x-timestamp:"));
                assert_eq!(
                    header.contains("authorization: bearer test-only-token"),
                    !public
                );
                let body = "{\"success\":true,\"bangumiList\":[{\"animeId\":42,\"animeTitle\":\"Calendar\"}]}";
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        for path in ["/public/season", "/api/v2/bangumi/shin"] {
            let result: super::super::ShinReply = futures_lite::future::block_on(read_from(
                &[&base],
                path,
                "",
                Some("test-only-token"),
            ))
            .unwrap();
            assert!(result.success);
            assert_eq!(result.bangumi_list[0].anime_id, 42);
        }
        worker.join().unwrap();
    }

    #[test]
    fn service_ids_special_labels_and_shifts_survive_without_arithmetic() {
        let details:Details=serde_json::from_value(serde_json::json!({"animeId":42,"bangumiId":"tmdb-tv-100","animeTitle":"Title","seasons":[{"id":"opaque-season","name":"SP"}],"episodes":[{"episodeId":42009999,"seasonId":"opaque-season","episodeNumber":"12.5"}],"onlineDatabases":[{"name":"TMDB","url":"https://www.themoviedb.org/tv/100-title"},{"name":"Bangumi","url":"https://bgm.tv/subject/123"}]})).unwrap();
        let matched = Matched {
            anime_id: 42,
            episode_id: 42009999,
            shift: -2.5,
            ..Default::default()
        };
        let r = evidence(&matched, Some(&details)).unwrap();
        assert_eq!(r.season_number, None);
        assert_eq!(r.episode_number, None);
        assert_eq!(r.special_label.as_deref(), Some("12.5"));
        assert_eq!(r.provider_season_id.as_deref(), Some("opaque-season"));
        assert_eq!(r.shift_seconds, -2.5);
        assert_eq!(r.tmdb[0].id, "100");
        assert_eq!(r.bangumi_subject.unwrap().id, "123");
        assert_eq!(r.work.unwrap().id, "tmdb-tv-100");
        assert_eq!(evidence(&matched, None).unwrap().episode_number, None);
    }
    #[test]
    fn labels_are_not_inferred_from_provider_season_ids_or_url_lookalikes() {
        let mut details = Details {
            anime_id: 1,
            bangumi_id: "123".into(),
            episodes: vec![Episode {
                episode_id: 2,
                season_id: "0".into(),
                episode_number: "C1".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let matched = Matched {
            anime_id: 1,
            episode_id: 2,
            ..Default::default()
        };
        let r = evidence(&matched, Some(&details)).unwrap();
        assert!(r.bangumi_subject.is_none());
        assert!(r.season_number.is_none());
        assert!(r.episode_number.is_none());
        details.episodes[0].episode_number = "S00E01".into();
        let r = evidence(&matched, Some(&details)).unwrap();
        assert_eq!(r.season_number, Some(0));
        assert_eq!(r.episode_number, Some(1));
        assert!(bangumi_url("https://bgm.tv.evil.invalid/subject/123").is_none());
        assert!(bangumi_url("https://evil@bgm.tv/subject/123").is_none());
        details.anime_id = 3;
        assert!(evidence(&matched, Some(&details)).is_err());
        let nulls:Details=serde_json::from_value(serde_json::json!({"animeId":1,"bangumiId":null,"onlineDatabases":null,"episodes":null,"seasons":null})).unwrap();
        assert!(nulls.episodes.is_empty());
    }
}
