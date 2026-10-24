//! The dandanplay service: signing in, the season's new anime, an anime's
//! details. Ported from NipaPlay (`lib/services/dandanplay_service_io.dart`,
//! `bangumi_service.dart`, `utils/network_settings.dart`).
//!
//! Requests go to NipaPlay's gateways, which hold the app secret and sign
//! for dandanplay: the one in Hong Kong first, the one in mainland China
//! when that fails. Every request but signing in carries the account's
//! token, as the gateways ask.
//!
//! The calls block; engines make them on their own threads and take the
//! results in when they are woken (`bern::wake`).

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::RwLock;

pub mod bangumi;
pub mod custom;
#[cfg(not(target_arch = "wasm32"))]
mod http;
pub mod reviews;

/// The app id the gateways know, and the secret they replace with theirs.
const APP_ID: &str = "nipaplayv1";
const APP_SECRET: &str = "server-managed";
const USER_AGENT: &str = "NipaPlay/1.0";

/// The gateways, in the order tried.
const GATEWAYS: &[&str] = &[
    "https://nipaplay.aimes-soft.com/dandanplay",
    "http://43.142.85.190/dandanplay",
];

/// What went wrong, in words for the user.
#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub message: String,
    /// The account's token is missing or no longer accepted.
    pub sign_in: bool,
}

impl Error {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            sign_in: false,
        }
    }

    fn sign_in() -> Self {
        Self {
            message: "请先登录弹弹play账号".into(),
            sign_in: true,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// The signed-in account's token, shared by the engines of the app.
static TOKEN: RwLock<Option<String>> = RwLock::new(None);

/// Use `token` for the requests from now on, or none after signing out.
pub fn set_token(token: Option<String>) {
    *TOKEN
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = token;
}

/// The token in use, if an account is signed in.
pub fn token() -> Option<String> {
    TOKEN
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// `X-Signature`: the SHA-256 of the app id, the time, the path and the
/// secret, in base64.
fn signature(timestamp: i64, path: &str) -> String {
    let digest = Sha256::digest(format!("{APP_ID}{timestamp}{path}{APP_SECRET}"));
    base64::engine::general_purpose::STANDARD.encode(digest)
}

/// The proof of the password the sign-in sends: the MD5 of the app id, the
/// password, the time, the user name and the secret, in hex.
fn login_hash(user: &str, password: &str, timestamp: i64) -> String {
    use md5::Md5;
    Md5::digest(format!("{APP_ID}{password}{timestamp}{user}{APP_SECRET}"))
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(target_arch = "wasm32")]
fn now() -> i64 {
    0
}

#[cfg(not(target_arch = "wasm32"))]
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

/// A signed-in account.
#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub token: String,
    /// The name signed in with, and the one others see.
    pub user_name: String,
    pub screen_name: String,
}

/// What dandanplay answers signing in and renewing with. The account's
/// names sit beside the token; NipaPlay looked for them under `user`, which
/// is read too in case a gateway nests them.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginReply {
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    screen_name: Option<String>,
    #[serde(default)]
    user: Option<LoginUser>,
    #[serde(default)]
    error_message: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginUser {
    #[serde(default)]
    screen_name: Option<String>,
}

impl LoginReply {
    /// The name others see, when the reply gives one.
    fn screen_name(&self) -> Option<String> {
        self.screen_name
            .clone()
            .or_else(|| self.user.as_ref()?.screen_name.clone())
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
    }

    /// The token, unless the reply says it failed.
    fn token(&self) -> Option<String> {
        self.token
            .clone()
            .filter(|token| !token.is_empty() && self.success != Some(false))
    }

    fn error(self, otherwise: &str) -> Error {
        Error::new(
            self.error_message
                .filter(|message| !message.is_empty())
                .unwrap_or_else(|| otherwise.into()),
        )
    }
}

/// Sign in with a dandanplay account.
pub fn login(user: &str, password: &str) -> Result<Account> {
    let user = user.trim();
    if user.is_empty() || password.is_empty() {
        return Err(Error::new("请填写用户名和密码"));
    }
    let timestamp = now();
    let body = serde_json::json!({
        "userName": user,
        "password": password,
        "appId": APP_ID,
        "unixTimestamp": timestamp,
        "hash": login_hash(user, password, timestamp),
    });
    let reply: LoginReply = request("POST", "/api/v2/login", "", Some(body), None)?;
    match reply.token() {
        Some(token) => Ok(Account {
            screen_name: reply.screen_name().unwrap_or_else(|| user.to_string()),
            user_name: user.to_string(),
            token,
        }),
        None => Err(reply.error("登录失败，请检查用户名和密码")),
    }
}

/// A renewed token, and the account's name as dandanplay gives it now.
#[derive(Clone, Debug, PartialEq)]
pub struct Renewed {
    pub token: String,
    pub screen_name: Option<String>,
}

/// A fresh token for `token`, which dandanplay keeps valid for a while
/// after each renewal. dandanplay takes it as a GET (a POST is answered
/// 405; NipaPlay tries POST first and retries).
pub fn renew(token: &str) -> Result<Renewed> {
    let reply: LoginReply = request("GET", "/api/v2/login/renew", "", None, Some(token))?;
    match reply.token() {
        Some(token) => Ok(Renewed {
            screen_name: reply.screen_name(),
            token,
        }),
        None => Err(reply.error("续期失败")),
    }
}

/// An anime of the season's list.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Intro {
    pub anime_id: i64,
    #[serde(default)]
    pub anime_title: String,
    #[serde(default)]
    pub image_url: String,
    #[serde(default, deserialize_with = "text_or_null")]
    pub summary: String,
    /// The day it airs: 0 is Sunday, 1 to 6 Monday to Saturday.
    #[serde(default)]
    pub air_day: Option<i64>,
    #[serde(default)]
    pub rating: f64,
    #[serde(default)]
    pub is_on_air: bool,
    #[serde(default)]
    pub is_favorited: bool,
    #[serde(default)]
    pub is_restricted: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShinReply {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    bangumi_list: Vec<Intro>,
    #[serde(default)]
    error_message: Option<String>,
}

/// The anime of this season, adult ones left out.
pub fn shin() -> Result<Vec<Intro>> {
    shin_from(false).or_else(|_| shin_from(true))
}

/// One source only, so the anime catalogue can try Bangumi before the cache.
pub fn shin_from(public: bool) -> Result<Vec<Intro>> {
    let token = token();
    if !public && token.is_none() {
        return Err(Error::sign_in());
    }
    let reply: ShinReply = request(
        "GET",
        if !public {
            "/api/v2/bangumi/shin"
        } else {
            "/public/season"
        },
        if !public {
            "?filterAdultContent=true"
        } else {
            ""
        },
        None,
        if public { None } else { token.as_deref() },
    )?;
    if reply.success {
        Ok(reply.bangumi_list)
    } else {
        Err(Error::new(
            reply
                .error_message
                .unwrap_or_else(|| "新番列表取不到".into()),
        ))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeInfo {
    pub episode_id: i64,
    #[serde(default)]
    pub episode_title: String,
    /// As dandanplay writes it: "1", "12", or "SP1" for a special.
    #[serde(default)]
    pub episode_number: String,
    #[serde(default)]
    pub air_date: Option<String>,
}

/// What dandanplay knows of one anime.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Details {
    pub anime_id: i64,
    #[serde(default)]
    pub anime_title: String,
    #[serde(default, deserialize_with = "text_or_null")]
    pub image_url: String,
    #[serde(default, deserialize_with = "text_or_null")]
    pub summary: String,
    /// The actual Bangumi subject link. `bangumiId` is dandanplay's own ID.
    #[serde(default, deserialize_with = "text_or_null")]
    pub bangumi_url: String,
    #[serde(default)]
    pub rating: f64,
    #[serde(default)]
    pub tags: Vec<Tag>,
    /// "TV动画", "剧场版" and the like.
    #[serde(default)]
    pub type_description: String,
    #[serde(default)]
    pub episodes: Vec<EpisodeInfo>,
    #[serde(default)]
    pub is_on_air: bool,
    #[serde(default)]
    pub air_day: Option<i64>,
    #[serde(default)]
    pub is_favorited: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DetailsReply {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    bangumi: Option<Details>,
    #[serde(default)]
    error_message: Option<String>,
}

/// The details of anime `anime_id`, its episodes among them; a negative
/// id is a show of the user's own server ([`custom`]).
pub fn details(anime_id: i64) -> Result<Details> {
    if custom::is_ours(anime_id) {
        return custom::details(anime_id);
    }
    details_from(anime_id, false).or_else(|_| details_from(anime_id, true))
}

/// Fetch authenticated details or the public snapshot, without crossing sources.
pub fn details_from(anime_id: i64, public: bool) -> Result<Details> {
    let token = token();
    if !public && token.is_none() {
        return Err(Error::sign_in());
    }
    let path = if !public {
        format!("/api/v2/bangumi/{anime_id}")
    } else {
        format!("/public/details/{anime_id}")
    };
    let reply: DetailsReply = request(
        "GET",
        &path,
        "",
        None,
        if public { None } else { token.as_deref() },
    )?;
    match reply.bangumi {
        Some(details) if reply.success => Ok(details),
        _ => Err(Error::new(
            reply
                .error_message
                .unwrap_or_else(|| "番剧详情取不到".into()),
        )),
    }
}

/// The episode a video is, as dandanplay matched it.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Matched {
    pub episode_id: i64,
    pub anime_id: i64,
    #[serde(default)]
    pub anime_title: String,
    #[serde(default)]
    pub episode_title: String,
    /// Its poster, when the service gives one (servers like Misaka do).
    #[serde(default, deserialize_with = "text_or_null")]
    pub image_url: String,
}

/// A text field that may be `null`.
fn text_or_null<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<String, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

impl Matched {
    /// Its number within the anime: dandanplay numbers an anime's
    /// episodes as the anime's id times 10000 plus the episode, and so do
    /// the servers that speak its API (their ids negative in Erika).
    pub fn number(&self) -> i64 {
        self.episode_id.abs() % 10_000
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MatchReply {
    #[serde(default)]
    is_matched: bool,
    #[serde(default)]
    matches: Vec<Matched>,
    #[serde(default)]
    error_message: Option<String>,
    #[serde(default = "succeeded")]
    success: bool,
}

fn succeeded() -> bool {
    true
}

/// Which episode a video is, by its name and the fingerprint of its head
/// (the MD5 of its first 16 MiB and its size). `None` when dandanplay is
/// not sure.
pub fn match_video(file_name: &str, md5: &str, size: u64) -> Result<Option<Matched>> {
    let token = token().ok_or_else(Error::sign_in)?;
    let body = serde_json::json!({
        "fileName": file_name,
        "fileHash": md5,
        "fileSize": size,
        "matchMode": "hashAndFileName",
    });
    let reply: MatchReply = request("POST", "/api/v2/match", "", Some(body), Some(&token))?;
    if !reply.success {
        return Err(Error::new(
            reply
                .error_message
                .unwrap_or_else(|| "弹弹play 没能匹配".into()),
        ));
    }
    Ok(reply
        .is_matched
        .then(|| reply.matches.into_iter().next())
        .flatten())
}

#[derive(Deserialize)]
struct CommentsReply {
    #[serde(default)]
    comments: Vec<Comment>,
}

#[derive(Deserialize)]
struct Comment {
    /// "time,mode,color,user": seconds into the episode, bilibili's mode
    /// (1 scrolls, 4 bottom, 5 top), the color as a number.
    #[serde(default)]
    p: String,
    #[serde(default)]
    m: String,
}

/// The danmaku of episode `episode_id`, with those of the sites dandanplay
/// relates to it, as JSON the playback kernel reads (`time`, `content`,
/// `mode_code`, `color`), and how many there are.
pub fn comments(episode_id: i64) -> Result<(String, usize)> {
    if custom::is_ours(episode_id) {
        return custom::comments(episode_id);
    }
    let token = token().ok_or_else(Error::sign_in)?;
    let reply: CommentsReply = request(
        "GET",
        &format!("/api/v2/comment/{episode_id}"),
        "?withRelated=true&chConvert=1",
        None,
        Some(&token),
    )?;
    Ok(kernel_comments(reply.comments))
}

/// Post exactly one comment; only the server's success is an acknowledgement.
pub fn send_comment(episode: i64, time: f64, comment: &str) -> Result<()> {
    if episode <= 0 {
        return Err(Error::new("此弹幕来源暂不支持发送，请匹配弹弹play集数"));
    }
    if !time.is_finite() || time < 0.0 || comment.trim().is_empty() || comment.chars().count() > 40
    {
        return Err(Error::new("弹幕内容或时间无效"));
    }
    let token = token().ok_or_else(Error::sign_in)?;
    let body =
        serde_json::json!({ "time": time, "mode": 1, "color": 16777215, "comment": comment });
    let reply: serde_json::Value = request(
        "POST",
        &format!("/api/v2/comment/{episode}"),
        "",
        Some(body),
        Some(&token),
    )?;
    if reply.get("success").and_then(|v| v.as_bool()) == Some(true) {
        Ok(())
    } else {
        Err(Error::new(
            reply
                .get("errorMessage")
                .and_then(|v| v.as_str())
                .unwrap_or("弹幕发送失败"),
        ))
    }
}

fn kernel_comments(comments: Vec<Comment>) -> (String, usize) {
    let items: Vec<serde_json::Value> = comments
        .into_iter()
        .filter(|comment| !comment.m.is_empty())
        .filter_map(|comment| {
            let mut parts = comment.p.split(',');
            let time: f64 = parts.next()?.parse().ok()?;
            let mode: i64 = parts.next().and_then(|mode| mode.parse().ok()).unwrap_or(1);
            let color: i64 = parts.next().and_then(|color| color.parse().ok()).unwrap_or(16_777_215);
            Some(serde_json::json!({ "time": time, "content": comment.m, "mode_code": mode, "color": color }))
        })
        .collect();
    let count = items.len();
    (serde_json::json!({ "comments": items }).to_string(), count)
}

/// An anime found by name.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub anime_id: i64,
    #[serde(default)]
    pub anime_title: String,
    #[serde(default, deserialize_with = "text_or_null")]
    pub image_url: String,
    #[serde(default, deserialize_with = "text_or_null")]
    pub summary: String,
    #[serde(default)]
    pub type_description: String,
    #[serde(default)]
    pub episode_count: i64,
    #[serde(default)]
    pub rating: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchReply {
    #[serde(default)]
    animes: Vec<Found>,
    #[serde(default = "succeeded")]
    success: bool,
    #[serde(default)]
    error_message: Option<String>,
}

/// Anime whose names contain `keyword`.
pub fn search(keyword: &str) -> Result<Vec<Found>> {
    let token = token().ok_or_else(Error::sign_in)?;
    let reply: SearchReply = request(
        "GET",
        "/api/v2/search/anime",
        &format!("?keyword={}", encode(keyword.trim())),
        None,
        Some(&token),
    )?;
    if reply.success {
        Ok(reply.animes)
    } else {
        Err(Error::new(
            reply.error_message.unwrap_or_else(|| "搜索失败".into()),
        ))
    }
}

/// `text` for a URL query: unreserved characters as they are, the rest as
/// percent-encoded UTF-8.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// One request to the first gateway that answers, as JSON.
fn request<T: serde::de::DeserializeOwned>(
    method: &str,
    path: &str,
    query: &str,
    body: Option<serde_json::Value>,
    token: Option<&str>,
) -> Result<T> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let text = http::send(method, path, query, body, token)?;
        serde_json::from_str(&text)
            .map_err(|error| Error::new(format!("弹弹play的回答看不懂：{error}")))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (method, path, query, body, token);
        Err(Error::new("网页版暂时连不上弹弹play"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_signed_as_nipaplay_signs_them() {
        // `generateSignature('nipaplayv1', 1700000000, '/api/v2/login', 'server-managed')`.
        assert_eq!(
            signature(1_700_000_000, "/api/v2/login"),
            base64::engine::general_purpose::STANDARD.encode(Sha256::digest(
                "nipaplayv11700000000/api/v2/loginserver-managed"
            ))
        );
        assert_eq!(signature(1, "/a").len(), 44);
        assert_eq!(
            login_hash("nipa", "pw", 1_700_000_000),
            format!(
                "{:x}",
                md5::Md5::digest("nipaplayv1pw1700000000nipaserver-managed")
            )
        );
    }

    #[test]
    fn replies_read_with_missing_fields() {
        let reply: ShinReply = serde_json::from_str(
            r#"{"success":true,"bangumiList":[{"animeId":17675,"animeTitle":"葬送的芙莉莲","imageUrl":"https://img/1.jpg","airDay":5,"rating":9.1,"isOnAir":true},{"animeId":2}]}"#,
        )
        .unwrap();
        assert_eq!(reply.bangumi_list.len(), 2);
        assert_eq!(reply.bangumi_list[0].air_day, Some(5));
        assert_eq!(reply.bangumi_list[1].anime_title, "");
        let details: DetailsReply = serde_json::from_str(
            r#"{"success":true,"bangumi":{"animeId":17675,"animeTitle":"葬送的芙莉莲","summary":"…","tags":[{"name":"奇幻"}],"episodes":[{"episodeId":176750001,"episodeTitle":"冒险结束","episodeNumber":"1","airDate":"2023-09-29T00:00:00"}]}}"#,
        )
        .unwrap();
        let details = details.bangumi.unwrap();
        assert_eq!(details.tags[0].name, "奇幻");
        assert_eq!(details.episodes[0].episode_id, 176_750_001);
    }

    #[test]
    fn matching_search_preserves_artwork_and_accepts_missing_summaries() {
        let reply: SearchReply = serde_json::from_str(
            r#"{"animes":[{"animeId":42,"animeTitle":"测试","imageUrl":"https://img/poster.jpg","summary":"作品简介","episodeCount":12},{"animeId":43,"imageUrl":null,"summary":null}]}"#,
        ).unwrap();
        assert_eq!(reply.animes[0].image_url, "https://img/poster.jpg");
        assert_eq!(reply.animes[0].summary, "作品简介");
        assert_eq!(reply.animes[0].episode_count, 12);
        assert!(reply.animes[1].summary.is_empty());
        assert!(reply.animes[1].image_url.is_empty());
    }

    #[test]
    fn matches_comments_and_names_read_as_the_kernel_and_the_url_need() {
        let reply: MatchReply = serde_json::from_str(
            r#"{"isMatched":true,"matches":[{"episodeId":176750003,"animeId":17675,"animeTitle":"葬送的芙莉莲","episodeTitle":"第3话 杀人魔法"}],"success":true}"#,
        )
        .unwrap();
        assert_eq!(reply.matches[0].number(), 3);
        let (json, count) = kernel_comments(vec![
            Comment {
                p: "12.5,1,16777215,u1".into(),
                m: "前方高能".into(),
            },
            Comment {
                p: "3,5,255,u2".into(),
                m: "顶".into(),
            },
            Comment {
                p: "broken".into(),
                m: "x".into(),
            },
        ]);
        assert_eq!(count, 2);
        assert!(json.contains(r#""content":"前方高能""#) && json.contains(r#""mode_code":5"#));
        assert_eq!(encode("葬送 a"), "%E8%91%AC%E9%80%81%20a");
    }

    #[test]
    fn the_names_are_read_beside_the_token_or_under_user() {
        let reply = |json: &str| serde_json::from_str::<LoginReply>(json).unwrap();
        let top = reply(
            r#"{"success":true,"token":"t","userName":"u1","screenName":" 小明 ","profileImage":"p"}"#,
        );
        assert_eq!(
            (top.token(), top.screen_name()),
            (Some("t".into()), Some("小明".into()))
        );
        assert_eq!(
            reply(r#"{"token":"t","user":{"screenName":"小红"}}"#).screen_name(),
            Some("小红".into())
        );
        assert_eq!(
            reply(r#"{"token":"t","screenName":""}"#).screen_name(),
            None
        );
        assert_eq!(
            reply(r#"{"success":false,"token":"t","errorMessage":"过期"}"#).token(),
            None
        );
    }

    #[test]
    fn private_actions_still_require_a_token() {
        set_token(None);
        assert!(comments(1).unwrap_err().sign_in);
        assert_eq!(login("", "x").unwrap_err().message, "请填写用户名和密码");
    }
}
