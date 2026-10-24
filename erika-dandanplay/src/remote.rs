//! The remote access of dandanplay's PC client (弹弹play 远程访问): the
//! client serves its media library, a stream of each video, a thumbnail
//! of it and its danmaku over HTTP. Ported from NipaPlay
//! (`lib/services/dandanplay_remote_service.dart`), with the API as
//! dandanplay documents it (`docs/弹弹play远程访问API（PC版）` there).
//!
//! When the user turned on the API key, every call but `/api/v1/welcome`
//! needs it: requests carry `Authorization: Bearer`, and the addresses
//! handed to the player and the image views carry `?token=`.

use serde::Deserialize;
use url::Url;

/// What `/api/v1/welcome` says.
#[derive(Clone, Debug, PartialEq)]
pub struct Welcome {
    pub version: String,
    /// The API key is turned on.
    pub token_required: bool,
}

/// A video of the client's media library (`/api/v1/library`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    /// Its id in the library, a GUID.
    pub id: String,
    /// The MD5 of its first 16 MiB, in hex, as dandanplay fingerprints.
    pub hash: String,
    /// The anime and episode the client matched it to; 0 when unmatched.
    pub anime_id: i64,
    pub episode_id: i64,
    pub anime_title: String,
    pub episode_title: String,
    /// Its file name, and its path on the client's disk.
    pub name: String,
    pub path: String,
    pub size: u64,
    /// Seconds.
    pub duration: i64,
}

impl Item {
    /// Its number within the anime, as dandanplay numbers episodes: the
    /// anime's id times 10000 plus the episode.
    pub fn number(&self) -> i64 {
        self.episode_id.abs() % 10_000
    }

    /// The file name, or the last part of its path when the client gave
    /// none.
    pub fn file_name(&self) -> String {
        if !self.name.trim().is_empty() {
            return self.name.trim().to_string();
        }
        self.path
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or("")
            .trim()
            .to_string()
    }

    /// Whether `hash` is a fingerprint other files can share.
    pub fn has_hash(&self) -> bool {
        self.hash.len() == 32 && self.hash.bytes().all(|b| b.is_ascii_hexdigit())
    }
}

/// The client's library holds field names in PascalCase; NipaPlay also
/// reads them in camelCase.
#[derive(Deserialize)]
struct RawItem {
    #[serde(alias = "id", default, deserialize_with = "text")]
    #[serde(rename = "Id")]
    id: String,
    #[serde(alias = "hash", default, deserialize_with = "text")]
    #[serde(rename = "Hash")]
    hash: String,
    #[serde(alias = "animeId", default, deserialize_with = "number")]
    #[serde(rename = "AnimeId")]
    anime_id: i64,
    #[serde(alias = "episodeId", default, deserialize_with = "number")]
    #[serde(rename = "EpisodeId")]
    episode_id: i64,
    #[serde(alias = "animeTitle", default, deserialize_with = "text")]
    #[serde(rename = "AnimeTitle")]
    anime_title: String,
    #[serde(alias = "episodeTitle", default, deserialize_with = "text")]
    #[serde(rename = "EpisodeTitle")]
    episode_title: String,
    #[serde(alias = "name", default, deserialize_with = "text")]
    #[serde(rename = "Name")]
    name: String,
    #[serde(alias = "path", default, deserialize_with = "text")]
    #[serde(rename = "Path")]
    path: String,
    #[serde(alias = "size", default, deserialize_with = "number")]
    #[serde(rename = "Size")]
    size: i64,
    #[serde(alias = "duration", default, deserialize_with = "number")]
    #[serde(rename = "Duration")]
    duration: i64,
}

/// A text field that may be `null` or a number.
fn text<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(
        match Option::<serde_json::Value>::deserialize(deserializer)? {
            Some(serde_json::Value::String(text)) => text.trim().to_string(),
            Some(serde_json::Value::Number(number)) => number.to_string(),
            _ => String::new(),
        },
    )
}

/// A number field that may be `null` or text (`EpisodeId` is text in
/// `/api/v1/current/video`).
fn number<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    Ok(
        match Option::<serde_json::Value>::deserialize(deserializer)? {
            Some(serde_json::Value::Number(number)) => number
                .as_i64()
                .or_else(|| number.as_f64().map(|value| value as i64))
                .unwrap_or(0),
            Some(serde_json::Value::String(text)) => text.trim().parse().unwrap_or(0),
            _ => 0,
        },
    )
}

/// The address of a client as the user typed it: `http://` when no scheme
/// is given, no trailing slash. The client listens on port 80 unless the
/// user changed it.
pub fn address(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("请填写弹弹play远程访问的地址".into());
    }
    let text = if input.contains("://") {
        input.to_owned()
    } else {
        format!("http://{input}")
    };
    let url = Url::parse(&text).map_err(|_| "访问地址格式不正确".to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none_or(str::is_empty)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("请填写弹弹play远程访问的 HTTP 或 HTTPS 地址".into());
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

/// `base` with `segments` after its path (a reverse proxy's path stays)
/// and the API key, when there is one, as `?token=`.
fn api_url(base: &str, segments: &[&str], token: &str) -> Option<String> {
    let mut url = Url::parse(base).ok()?;
    url.path_segments_mut()
        .ok()?
        .pop_if_empty()
        .extend(segments);
    if !token.is_empty() {
        url.query_pairs_mut().append_pair("token", token);
    }
    Some(url.into())
}

/// Where the player streams `item` from: by its id, which is unique, or
/// by its fingerprint, which several copies may share.
pub fn stream_url(base: &str, item: &Item, token: &str) -> Option<String> {
    if !item.id.is_empty() {
        api_url(base, &["api", "v1", "stream", "id", &item.id], token)
    } else if !item.hash.is_empty() {
        api_url(base, &["api", "v1", "stream", &item.hash], token)
    } else {
        None
    }
}

/// The thumbnail the client took of `item`, a JPEG.
pub fn image_url(base: &str, item: &Item, token: &str) -> Option<String> {
    if !item.id.is_empty() {
        api_url(base, &["api", "v1", "image", "id", &item.id], token)
    } else if !item.hash.is_empty() {
        api_url(base, &["api", "v1", "image", &item.hash], token)
    } else {
        None
    }
}

/// Whether `url` is a thumbnail a client took, which a poster may replace.
pub fn is_thumbnail(url: &str) -> bool {
    url.split(['?', '#'])
        .next()
        .is_some_and(|path| path.contains("/api/v1/image/"))
}

/// Where the danmaku of the video streamed from `stream` are: the client
/// fetches them from dandanplay and answers in BiliBili's XML.
pub fn comment_url(stream: &str) -> Option<String> {
    let url = Url::parse(stream).ok()?;
    let path = url.path();
    let at = path.find("/api/v1/stream/")?;
    let rest = &path[at + "/api/v1/stream/".len()..];
    if rest.is_empty() {
        return None;
    }
    let mut comments = url.clone();
    comments.set_path(&format!("{}/api/v1/comment/{rest}", &path[..at]));
    Some(comments.into())
}

pub fn parse_welcome(body: &str) -> Result<Welcome, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "这个地址不是弹弹play远程访问".to_string())?;
    if !value.is_object() || (value.get("version").is_none() && value.get("message").is_none()) {
        return Err("这个地址不是弹弹play远程访问".into());
    }
    Ok(Welcome {
        version: match &value["version"] {
            serde_json::Value::String(version) => version.clone(),
            serde_json::Value::Null => String::new(),
            other => other.to_string(),
        },
        token_required: value["tokenRequired"].as_bool().unwrap_or(false),
    })
}

/// The videos of the client's library; those without an id or a
/// fingerprint cannot be streamed and are left out.
pub fn parse_library(body: &str) -> Result<Vec<Item>, String> {
    let value: serde_json::Value = serde_json::from_str(body.trim_start_matches('\u{feff}'))
        .map_err(|_| "弹弹play返回的媒体库读不懂".to_string())?;
    let list = value
        .as_array()
        .ok_or_else(|| "弹弹play返回的媒体库读不懂".to_string())?;
    Ok(list
        .iter()
        .filter_map(|item| serde_json::from_value::<RawItem>(item.clone()).ok())
        .map(|raw| Item {
            id: raw.id,
            hash: raw.hash,
            anime_id: raw.anime_id.max(0),
            episode_id: raw.episode_id.max(0),
            anime_title: raw.anime_title,
            episode_title: raw.episode_title,
            name: raw.name,
            path: raw.path,
            size: raw.size.max(0) as u64,
            duration: raw.duration.max(0),
        })
        .filter(|item| !item.id.is_empty() || !item.hash.is_empty())
        .collect())
}

/// Undo XML's escapes in an attribute or text.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|end| *end <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The danmaku of BiliBili's XML (`<d p="time,mode,size,color,…">text</d>`)
/// as JSON the playback kernel reads (`time`, `content`, `mode_code`,
/// `color`), and how many there are. Scrolling, top and bottom danmaku are
/// kept; BiliBili's advanced and code danmaku are not.
pub fn parse_comments(xml: &str) -> (String, usize) {
    let mut items = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<d ") {
        rest = &rest[at + 3..];
        let Some(close) = rest.find('>') else {
            break;
        };
        let attributes = &rest[..close];
        rest = &rest[close + 1..];
        if attributes.ends_with('/') {
            continue;
        }
        let Some(end) = rest.find("</d>") else {
            break;
        };
        let content = unescape(rest[..end].trim());
        rest = &rest[end + 4..];
        let Some(p) = attributes.find("p=").and_then(|at| {
            let value = &attributes[at + 2..];
            let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
            let value = &value[1..];
            value.find(quote).map(|end| unescape(&value[..end]))
        }) else {
            continue;
        };
        let parts: Vec<&str> = p.split(',').map(str::trim).collect();
        let Some(time) = parts
            .first()
            .and_then(|time| time.parse::<f64>().ok())
            .filter(|time| time.is_finite() && *time >= 0.0)
        else {
            continue;
        };
        let mode_code = match parts.get(1).and_then(|mode| mode.parse::<i64>().ok()) {
            None | Some(1..=3) => 1,
            Some(4) => 4,
            Some(5) => 5,
            Some(_) => continue,
        };
        let color = parts
            .get(3)
            .and_then(|color| color.parse::<i64>().ok())
            .unwrap_or(16_777_215);
        if content.is_empty() {
            continue;
        }
        items.push(serde_json::json!({
            "time": time,
            "content": content,
            "mode_code": mode_code,
            "color": color,
        }));
    }
    let count = items.len();
    (serde_json::json!({ "comments": items }).to_string(), count)
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::time::Duration;

    fn agent(seconds: u64) -> ureq::Agent {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(seconds)))
            .http_status_as_error(false)
            // The client is usually on the same network; a system proxy
            // would not reach it.
            .proxy(None)
            .build()
            .into()
    }

    fn get(url: &str, token: &str, seconds: u64) -> Result<(u16, String), String> {
        let mut request = agent(seconds)
            .get(url)
            .header("User-Agent", crate::USER_AGENT);
        if !token.is_empty() {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        let mut response = request.call().map_err(|_| {
            "连不上弹弹play，请检查地址，并确认电脑上的弹弹play开着远程访问".to_string()
        })?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_string()
            .map_err(|_| "弹弹play的回应没有读完".to_string())?;
        Ok((status, body))
    }

    /// Whether `base` is a client's remote access, and whether it wants
    /// the API key.
    pub fn welcome(base: &str) -> Result<Welcome, String> {
        let url = api_url(base, &["api", "v1", "welcome"], "")
            .ok_or_else(|| "访问地址格式不正确".to_string())?;
        match get(&url, "", 10)? {
            (200, body) => parse_welcome(&body),
            (status, _) => Err(format!("连不上弹弹play远程访问（HTTP {status}）")),
        }
    }

    /// The videos of the client's library.
    pub fn library(base: &str, token: &str) -> Result<Vec<Item>, String> {
        let url = api_url(base, &["api", "v1", "library"], "")
            .ok_or_else(|| "访问地址格式不正确".to_string())?;
        match get(&url, token, 20)? {
            (200, body) => parse_library(&body),
            (401, _) if token.is_empty() => {
                Err("这台电脑的弹弹play开启了 API 密钥验证，请填写 API 密钥".into())
            }
            (401, _) => Err("API 密钥不对，弹弹play拒绝了访问".into()),
            (status, _) => Err(format!("读不到弹弹play的媒体库（HTTP {status}）")),
        }
    }

    /// Check `base` as NipaPlay connects: the welcome first, which says
    /// whether the key is needed, then the library. Returns the key to
    /// keep (none when the client does not ask for one) and the videos.
    pub fn connect(base: &str, token: &str) -> Result<(String, Vec<Item>), String> {
        let welcome = welcome(base)?;
        let token = token.trim();
        if welcome.token_required && token.is_empty() {
            return Err("这台电脑的弹弹play开启了 API 密钥验证，请填写 API 密钥".into());
        }
        let token = if welcome.token_required { token } else { "" };
        Ok((token.to_string(), library(base, token)?))
    }

    /// The danmaku of the video streamed from `stream`. The client may
    /// take a while: it fetches the newest from dandanplay first.
    pub fn comments(stream: &str) -> Result<(String, usize), String> {
        let url = comment_url(stream).ok_or_else(|| "这个视频没有弹幕地址".to_string())?;
        match get(&url, "", 60)? {
            (200, body) => Ok(parse_comments(&body)),
            (401, _) => Err("API 密钥不对，弹弹play拒绝了访问".into()),
            (status, _) => Err(format!("读不到弹弹play的弹幕（HTTP {status}）")),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{comments, connect, library, welcome};

#[cfg(target_arch = "wasm32")]
pub fn connect(_: &str, _: &str) -> Result<(String, Vec<Item>), String> {
    Err("请在桌面客户端或连接桌面客户端的 WebUI 中挂载弹弹play共享媒体".into())
}

#[cfg(target_arch = "wasm32")]
pub fn library(_: &str, _: &str) -> Result<Vec<Item>, String> {
    connect("", "").map(|(_, items)| items)
}

#[cfg(target_arch = "wasm32")]
pub fn comments(_: &str) -> Result<(String, usize), String> {
    Err("这里还不能读弹弹play共享媒体的弹幕".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `/api/v1/library` as dandanplay's documentation records it, with an
    /// unmatched video, a camelCase one (as NipaPlay also reads) and one
    /// that cannot be streamed.
    const LIBRARY: &str = r#"[
      {"AnimeId": 14198, "EpisodeId": 141980003, "AnimeTitle": "佐贺偶像是传奇",
       "EpisodeTitle": "第3话 DEAD OR LIVE SAGA", "Id": "c004e475-d9bb-41e1-976b-7fce00997f3a",
       "Hash": "03778309A0E8A09C2F43603A490F2E98", "Name": "[Zombieland Saga][03][BIG5][1080P].mp4",
       "Path": "Y:\\[Zombieland Saga][03][BIG5][1080P].mp4", "Size": 518754774, "Rate": 0,
       "IsStandalone": false, "Created": "2018-10-21T23:14:59", "LastMatch": "0001-01-01T00:00:00",
       "LastPlay": "2019-04-25T18:00:22", "LastThumbnail": "2019-03-05T22:03:20", "Duration": 1420},
      {"AnimeId": null, "EpisodeId": null, "AnimeTitle": null, "EpisodeTitle": null,
       "Id": "686b4d07-d8de-4f2e-b716-3cf0843cc903", "Hash": "334763177A0B9A5647B263AA74E0A013",
       "Name": "", "Path": "Y:\\新建文件夹\\unknown [01].mkv", "Size": 128023969, "Duration": null},
      {"animeId": 13031, "episodeId": "130310010", "animeTitle": "Princess Principal",
       "episodeTitle": "第10话 Comfort Comrade", "id": "07c0d08a-6191-49aa-866c-5ea91cf91f23",
       "hash": "432A9EC48627FFA2F092B418840A5D98", "name": "[HYSUB]Princess Principal[10].mp4",
       "path": "Y:\\Princess Principal\\[HYSUB]Princess Principal[10].mp4", "size": 158818535},
      {"AnimeId": 1, "EpisodeId": 10001, "Name": "lost.mp4"}
    ]"#;

    #[test]
    fn library_reads_both_casings_nulls_and_skips_unstreamable_videos() {
        let items = parse_library(LIBRARY).unwrap();
        assert_eq!(items.len(), 3);
        let saga = &items[0];
        assert_eq!(saga.anime_id, 14198);
        assert_eq!(saga.episode_id, 141980003);
        assert_eq!(saga.number(), 3);
        assert_eq!(saga.anime_title, "佐贺偶像是传奇");
        assert_eq!(saga.size, 518754774);
        assert_eq!(saga.duration, 1420);
        assert!(saga.has_hash());
        let unknown = &items[1];
        assert_eq!((unknown.anime_id, unknown.episode_id), (0, 0));
        assert!(unknown.anime_title.is_empty());
        assert_eq!(unknown.file_name(), "unknown [01].mkv");
        let principal = &items[2];
        assert_eq!(principal.episode_id, 130310010);
        assert_eq!(principal.number(), 10);
        assert_eq!(principal.file_name(), "[HYSUB]Princess Principal[10].mp4");
        assert!(parse_library("{}").is_err());
        assert!(parse_library("<html>").is_err());
        assert_eq!(parse_library("\u{feff}[]").unwrap(), Vec::new());
    }

    #[test]
    fn welcome_says_whether_the_key_is_needed() {
        let welcome = parse_welcome(
            r#"{"message": "Hello dandanplay user!", "version": "9.4.7.517",
                "time": "05/19/2019 11:45:45", "tokenRequired": true}"#,
        )
        .unwrap();
        assert_eq!(welcome.version, "9.4.7.517");
        assert!(welcome.token_required);
        assert!(
            !parse_welcome(r#"{"message": "Hello", "version": "12.0"}"#)
                .unwrap()
                .token_required
        );
        assert!(parse_welcome("[]").is_err());
        assert!(parse_welcome(r#"{"success": true}"#).is_err());
    }

    #[test]
    fn addresses_are_normalized_and_keep_proxy_paths() {
        for (input, expected) in [
            (" 192.168.31.100 ", "http://192.168.31.100"),
            ("192.168.31.100:8080/", "http://192.168.31.100:8080"),
            (
                "https://media.example.com/dandan/",
                "https://media.example.com/dandan",
            ),
            ("http://[::1]:80", "http://[::1]"),
        ] {
            assert_eq!(address(input).unwrap(), expected);
        }
        for input in [
            "",
            "ftp://host",
            "http://",
            "http://a@host",
            "http://host/?x=1",
        ] {
            assert!(address(input).is_err(), "{input}");
        }
    }

    #[test]
    fn urls_carry_the_key_and_lead_from_streams_to_danmaku() {
        let items = parse_library(LIBRARY).unwrap();
        let base = "https://media.example.com/dandan";
        let stream = stream_url(base, &items[0], "a b&c").unwrap();
        assert_eq!(
            stream,
            "https://media.example.com/dandan/api/v1/stream/id/c004e475-d9bb-41e1-976b-7fce00997f3a?token=a+b%26c"
        );
        assert_eq!(
            comment_url(&stream).unwrap(),
            "https://media.example.com/dandan/api/v1/comment/id/c004e475-d9bb-41e1-976b-7fce00997f3a?token=a+b%26c"
        );
        let by_hash = Item {
            hash: "03778309A0E8A09C2F43603A490F2E98".into(),
            ..Item::default()
        };
        let plain = stream_url("http://192.168.1.2", &by_hash, "").unwrap();
        assert_eq!(
            plain,
            "http://192.168.1.2/api/v1/stream/03778309A0E8A09C2F43603A490F2E98"
        );
        assert_eq!(
            comment_url(&plain).unwrap(),
            "http://192.168.1.2/api/v1/comment/03778309A0E8A09C2F43603A490F2E98"
        );
        let image = image_url(base, &items[0], "k").unwrap();
        assert!(is_thumbnail(&image));
        assert!(image.ends_with("/api/v1/image/id/c004e475-d9bb-41e1-976b-7fce00997f3a?token=k"));
        assert!(!is_thumbnail("https://img.dandanplay.net/anime/14198.jpg"));
        assert!(comment_url("https://example.com/video.mp4").is_none());
        assert!(stream_url(base, &Item::default(), "").is_none());
    }

    #[test]
    fn bilibili_xml_becomes_kernel_danmaku() {
        let xml = r#"<?xml version="1.0"?>
<i xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <chatserver>chat.bilibili.com</chatserver>
  <d p="37.26,1,25,16777215,1500358682,0,0,0">艾尔之光</d>
  <d p="825.54,5,25,255,1500479598,0,0,0">顶部 &amp; &lt;红&gt; &#x1F600;</d>
  <d p="451.43,4,25,65280,1500654750,0,0,0">底部</d>
  <d p="10,7,25,16777215,0,0,0,0">[0,0,"advanced"]</d>
  <d p="oops,1,25,16777215">坏的</d>
  <d p="12.5,1,25,16777215,0,0,0,0"></d>
</i>"#;
        let (json, count) = parse_comments(xml);
        assert_eq!(count, 3);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let comments = value["comments"].as_array().unwrap();
        assert_eq!(comments[0]["time"], 37.26);
        assert_eq!(comments[0]["content"], "艾尔之光");
        assert_eq!(comments[0]["mode_code"], 1);
        assert_eq!(comments[0]["color"], 16777215);
        assert_eq!(comments[1]["content"], "顶部 & <红> 😀");
        assert_eq!(comments[1]["mode_code"], 5);
        assert_eq!(comments[1]["color"], 255);
        assert_eq!(comments[2]["mode_code"], 4);
        assert_eq!(parse_comments("<i></i>").1, 0);
    }
}
