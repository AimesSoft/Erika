//! A dandanplay-style server of the user's own: the Erika gateway before a
//! danmaku server such as Misaka, for TV series and movies, matching by
//! name and by the fingerprints the user confirmed. Its address holds its
//! token, `https://host/erika/<token>`; requests go to `<address>/api/v2/…`
//! with no signature.
//!
//! Its ids live beside dandanplay's in Erika's database as negative
//! numbers: everything here hands them out negated and takes them back the
//! same way, and [`crate::details`] and [`crate::comments`] of a negative id
//! come here.

use crate::{
    CommentsReply, Details, DetailsReply, Error, Found, MatchReply, Matched, Result, SearchReply,
    encode, kernel_comments,
};
use std::sync::RwLock;

static SERVER: RwLock<Option<String>> = RwLock::new(None);

/// Use the server at `address`, or none.
pub fn set_server(address: Option<String>) {
    *SERVER
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = address
        .map(|address| address.trim().trim_end_matches('/').to_string())
        .filter(|address| !address.is_empty());
}

/// The server's address, when there is one.
pub fn server() -> Option<String> {
    SERVER
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Whether `id` is one of the server's, as Erika keeps it.
pub fn is_ours(id: i64) -> bool {
    id < 0
}

fn request<T: serde::de::DeserializeOwned>(
    method: &str,
    path: &str,
    query: &str,
    body: Option<serde_json::Value>,
) -> Result<T> {
    let address = server().ok_or_else(|| Error::new("还没有设置电视剧和电影的识别服务器"))?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .http_status_as_error(false)
            .build()
            .into();
        let url = format!("{address}/api/v2{path}{query}");
        let answer = match (method, body) {
            ("POST", Some(body)) => agent
                .post(&url)
                .header("User-Agent", "Erika")
                .send_json(body),
            ("POST", None) => agent.post(&url).header("User-Agent", "Erika").send_empty(),
            _ => agent.get(&url).header("User-Agent", "Erika").call(),
        };
        let mut answer =
            answer.map_err(|error| Error::new(format!("连不上识别服务器：{error}")))?;
        let status = answer.status().as_u16();
        let text = answer
            .body_mut()
            .read_to_string()
            .map_err(|error| Error::new(format!("识别服务器的回答读不完：{error}")))?;
        if status == 401 {
            return Err(Error::new("识别服务器不认这个地址里的令牌"));
        }
        if status >= 400 {
            return Err(Error::new(format!("识别服务器回答了 {status}")));
        }
        serde_json::from_str(&text)
            .map_err(|error| Error::new(format!("识别服务器的回答看不懂：{error}")))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (address, method, path, query, body);
        Err(Error::new("网页版暂时连不上识别服务器"))
    }
}

fn ours(mut matched: Matched) -> Matched {
    matched.anime_id = -matched.anime_id;
    matched.episode_id = -matched.episode_id;
    matched
}

/// Which episode a video is, by its name and fingerprint.
pub fn match_video(file_name: &str, md5: &str, size: u64) -> Result<Option<Matched>> {
    let body = serde_json::json!({ "fileName": file_name, "fileHash": md5, "fileSize": size, "matchMode": "hashAndFileName" });
    let reply: MatchReply = request("POST", "/match", "", Some(body))?;
    if !reply.success {
        return Err(Error::new(
            reply
                .error_message
                .unwrap_or_else(|| "识别服务器没能匹配".into()),
        ));
    }
    Ok(reply
        .is_matched
        .then(|| reply.matches.into_iter().next())
        .flatten()
        .map(ours))
}

/// The details of the server's show `id` (as Erika keeps it, negative).
pub fn details(id: i64) -> Result<Details> {
    let reply: DetailsReply = request("GET", &format!("/bangumi/{}", -id), "", None)?;
    match reply.bangumi {
        Some(mut details) if reply.success => {
            details.anime_id = -details.anime_id;
            for episode in &mut details.episodes {
                episode.episode_id = -episode.episode_id;
            }
            Ok(details)
        }
        _ => Err(Error::new(
            reply.error_message.unwrap_or_else(|| "详情取不到".into()),
        )),
    }
}

/// The danmaku of the server's episode `id` (negative), as the kernel reads
/// them.
pub fn comments(id: i64) -> Result<(String, usize)> {
    let reply: CommentsReply = request(
        "GET",
        &format!("/comment/{}", -id),
        "?withRelated=true&chConvert=1",
        None,
    )?;
    Ok(kernel_comments(reply.comments))
}

/// Shows whose names contain `keyword`.
pub fn search(keyword: &str) -> Result<Vec<Found>> {
    let reply: SearchReply = request(
        "GET",
        "/search/anime",
        &format!("?keyword={}", encode(keyword.trim())),
        None,
    )?;
    if !reply.success {
        return Err(Error::new(
            reply.error_message.unwrap_or_else(|| "搜索失败".into()),
        ));
    }
    Ok(reply
        .animes
        .into_iter()
        .map(|mut found| {
            found.anime_id = -found.anime_id;
            found
        })
        .collect())
}

/// What the user confirmed a file is, for the gateway to remember.
pub struct Confirmed<'a> {
    pub file_name: &'a str,
    pub md5: &'a str,
    pub size: u64,
    /// The show and the episode as Erika keeps them (negative).
    pub anime_id: i64,
    pub episode_id: i64,
    pub number: i64,
    pub anime_title: &'a str,
    pub episode_title: &'a str,
    pub image: &'a str,
}

#[derive(serde::Deserialize)]
struct Done {
    #[serde(default = "crate::succeeded")]
    success: bool,
    #[serde(default, rename = "errorMessage")]
    error_message: Option<String>,
}

/// Tell the gateway which episode a file is; next time its fingerprint is
/// enough.
pub fn confirm(confirmed: &Confirmed) -> Result<()> {
    let body = serde_json::json!({
        "fileName": confirmed.file_name,
        "fileHash": confirmed.md5,
        "fileSize": confirmed.size,
        "animeId": -confirmed.anime_id,
        "episodeId": -confirmed.episode_id,
        "episodeNumber": confirmed.number,
        "animeTitle": confirmed.anime_title,
        "episodeTitle": confirmed.episode_title,
        "imageUrl": confirmed.image,
    });
    let done: Done = request("POST", "/match/confirm", "", Some(body))?;
    if done.success {
        Ok(())
    } else {
        Err(Error::new(
            done.error_message.unwrap_or_else(|| "没能记下".into()),
        ))
    }
}

/// Tell the gateway a file is no episode it should remember.
pub fn forget(md5: &str, size: u64) -> Result<()> {
    let done: Done = request(
        "POST",
        "/match/forget",
        "",
        Some(serde_json::json!({ "fileHash": md5, "fileSize": size })),
    )?;
    if done.success {
        Ok(())
    } else {
        Err(Error::new(
            done.error_message.unwrap_or_else(|| "没能忘掉".into()),
        ))
    }
}
