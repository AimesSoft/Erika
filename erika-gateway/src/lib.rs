//! A gateway before a dandanplay-style danmaku server (Misaka, which
//! matches by file name only), speaking the same API: `/{token}/api/v2/…`.
//!
//! - `POST match`: a file whose fingerprint (`fileHash`, `fileSize`) the
//!   user confirmed before is answered here, its episode checked against the
//!   server and found again by title and number if the server's ids changed;
//!   any other goes to the server.
//! - `POST match/confirm`: the user says which episode a file is.
//! - `POST match/forget`: the user unlinked a file.
//! - Everything else goes to the server as it is: searching, details,
//!   danmaku.
//!
//! Two keys open the path: the owner's token, whose confirmations every
//! device trusts, and a public key built into Erika, under which each
//! device (`X-Erika-Device`) votes for itself and is held to a request
//! rate, so the danmaku server and the machine are not overrun. The danmaku
//! server's own token stays here, in the server's address.

pub mod metadata;
pub mod store;
#[cfg(test)]
mod tests;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use store::{Binding, Store, Voter};

/// How long an episode checked against the server is trusted.
const CHECKED_FOR: Duration = Duration::from_secs(600);
/// How long Misaka waits for a video site's danmaku before it answers none.
const GAVE_UP_AFTER: Duration = Duration::from_secs(20);
/// The window requests under the public key are counted in.
const WINDOW: Duration = Duration::from_secs(60);

pub struct Gateway {
    /// Independent public catalogue: never gated on a dandanplay account.
    pub metadata: metadata::Metadata,
    /// The owner's token, and the public key built into Erika (none: only
    /// the owner gets in).
    pub token: String,
    pub public_key: Option<String>,
    /// How many requests a device makes under the public key in a minute.
    pub per_minute: u32,
    /// The requests each device made in the current window.
    counted: Mutex<HashMap<String, (Instant, u32)>>,
    /// The danmaku server's dandanplay-style root, before `/api/v2`, such as
    /// `http://127.0.0.1:7768/api/v1/<its token>`.
    pub upstream: String,
    pub store: Store,
    pub client: reqwest::Client,
    /// Episodes the server was seen to have lately.
    checked: Mutex<HashMap<i64, Instant>>,
}

impl Gateway {
    pub fn new(token: String, upstream: String, store: Store) -> Gateway {
        Gateway {
            metadata: metadata::Metadata::default(),
            token,
            upstream: upstream.trim_end_matches('/').to_string(),
            store,
            client: reqwest::Client::builder()
                // Below nginx's 120 s: Misaka's first search for a show and
                // its first danmaku take tens of seconds.
                .timeout(Duration::from_secs(110))
                .build()
                .expect("an HTTP client builds"),
            checked: Mutex::new(HashMap::new()),
            public_key: None,
            per_minute: 120,
            counted: Mutex::new(HashMap::new()),
        }
    }

    /// Who asks with `token`: the owner, a device under the public key (by
    /// its `X-Erika-Device`, else its address), or nobody. Refused when a
    /// device went over its rate.
    fn caller(&self, token: &str, headers: &HeaderMap) -> Result<Voter, Response> {
        if token == self.token {
            return Ok(Voter { device: "owner".into(), trusted: true });
        }
        if self.public_key.as_deref() != Some(token) {
            return Err(refused());
        }
        let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok()).map(str::trim).filter(|value| !value.is_empty());
        let address = format!("address:{}", header("x-real-ip").unwrap_or("unknown"));
        let device = match header("x-erika-device").filter(|id| id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
            Some(id) => format!("device:{id}"),
            None => address.clone(),
        };
        // A device is held to its rate; its address, which a device naming
        // itself anew each time cannot leave, to ten devices' worth.
        let mut counted = self.counted.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if counted.len() > 10_000 {
            counted.retain(|_, (since, _)| since.elapsed() < WINDOW);
        }
        let mut over = false;
        for (key, limit) in [(&device, self.per_minute), (&address, self.per_minute.saturating_mul(10))] {
            let (since, count) = counted.entry(key.clone()).or_insert((Instant::now(), 0));
            if since.elapsed() >= WINDOW {
                *since = Instant::now();
                *count = 0;
            }
            *count += 1;
            over |= *count > limit;
            if device == address {
                break;
            }
        }
        if over {
            return Err((StatusCode::TOO_MANY_REQUESTS, axum::Json(json!({ "success": false, "errorCode": 429, "errorMessage": "too many requests" }))).into_response());
        }
        Ok(Voter { device, trusted: false })
    }

    fn url(&self, rest: &str, query: Option<&str>) -> String {
        match query.filter(|query| !query.is_empty()) {
            Some(query) => format!("{}/api/v2/{rest}?{query}", self.upstream),
            None => format!("{}/api/v2/{rest}", self.upstream),
        }
    }

    async fn get_json(&self, rest: &str, query: Option<&str>) -> Option<Value> {
        let response = self.client.get(self.url(rest, query)).send().await.ok()?;
        response.json().await.ok()
    }

    /// The server's details of show `id` (its own id, or a passing one from
    /// a search), with the show's lasting id and plain title; `None` when
    /// the server is out of reach.
    async fn fetch_details(&self, id: i64) -> Option<Value> {
        let mut answer = self.get_json(&format!("bangumi/A{id}"), None).await?;
        if let Some(details) = answer.get_mut("bangumi").filter(|details| details.is_object()) {
            settle_details(details);
        }
        Some(answer)
    }

    /// The details of show `id` as passed on to clients: the server's,
    /// with the episodes seen of the show before, which the server lists
    /// only once it fetched their danmaku.
    async fn details(&self, id: i64) -> Option<Value> {
        let mut answer = self.fetch_details(id).await?;
        let fresh = answer.get("bangumi").filter(|details| details.is_object()).cloned();
        let lasting = fresh
            .as_ref()
            .and_then(|details| details.get("animeId"))
            .and_then(Value::as_i64)
            .unwrap_or(id);
        let merged = match (fresh, self.store.show(lasting)) {
            (Some(fresh), Some(seen)) => merge_details(fresh, &seen),
            (Some(fresh), None) => fresh,
            (None, Some(seen)) => seen,
            (None, None) => return Some(answer),
        };
        if let Err(error) = self.store.remember_show(lasting, &merged) {
            eprintln!("erika-gateway: remember show {lasting}: {error}");
        }
        answer["bangumi"] = merged;
        answer["success"] = json!(true);
        answer["errorCode"] = json!(0);
        answer["errorMessage"] = json!("");
        Some(answer)
    }

    /// The binding as the server knows the episode now: as it is, found
    /// again by title and number when the server's ids changed, or `None`
    /// when the server no longer has the episode. A server out of reach
    /// changes nothing.
    async fn check(&self, binding: &Binding) -> Option<Binding> {
        let fresh = |id: i64| {
            self.checked
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(&id)
                .is_some_and(|at| at.elapsed() < CHECKED_FOR)
        };
        if fresh(binding.episode_id) {
            return Some(binding.clone());
        }
        let Some(details) = self.fetch_details(binding.anime_id).await else {
            return Some(binding.clone());
        };
        // A show the server still has keeps its episodes' ids, listed or
        // not (it lists those whose danmaku it fetched).
        let has = details.get("bangumi").is_some_and(Value::is_object);
        let found = if has {
            Some(binding.clone())
        } else {
            let query = format!("anime={}", encode(&binding.anime_title));
            let results = self.get_json("search/episodes", Some(&query)).await?;
            results
                .get("animes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find_map(|anime| {
                    let title = anime
                        .get("animeTitle")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if title != binding.anime_title {
                        return None;
                    }
                    let episode = anime
                        .get("episodes")
                        .and_then(Value::as_array)?
                        .iter()
                        .find(|episode| {
                            episode
                                .get("episodeId")
                                .and_then(Value::as_i64)
                                .is_some_and(|id| id % 10_000 == binding.number)
                        })?;
                    let episode_id = episode.get("episodeId").and_then(Value::as_i64)?;
                    Some(Binding {
                        anime_id: show_of(episode_id)
                            .or_else(|| anime.get("animeId").and_then(Value::as_i64))?,
                        episode_id,
                        episode_title: episode
                            .get("episodeTitle")
                            .and_then(Value::as_str)
                            .unwrap_or(&binding.episode_title)
                            .to_string(),
                        image: anime
                            .get("imageUrl")
                            .and_then(Value::as_str)
                            .filter(|image| !image.is_empty())
                            .unwrap_or(&binding.image)
                            .to_string(),
                        ..binding.clone()
                    })
                })
        };
        if let Some(found) = &found {
            self.checked
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(found.episode_id, Instant::now());
        }
        found
    }
}

/// `text` for a query string.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            byte => format!("%{byte:02X}"),
        })
        .collect()
}

fn refused() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        axum::Json(json!({ "success": false, "errorCode": 401, "errorMessage": "token" })),
    )
        .into_response()
}

fn unreachable_server() -> Response {
    (StatusCode::BAD_GATEWAY, axum::Json(json!({ "success": false, "errorCode": 502, "errorMessage": "the danmaku server is out of reach" }))).into_response()
}

/// The show Misaka's episode `id` belongs to: its episode ids are `25`,
/// the show's id in six digits, the source's in two and the episode's
/// number in four. Its matching and searching name shows by passing ids
/// (900000 and up) that soon name other shows; the episodes' ids last.
fn show_of(episode_id: i64) -> Option<i64> {
    (25_000_000_000_000..26_000_000_000_000)
        .contains(&episode_id)
        .then(|| episode_id / 1_000_000 % 1_000_000)
}

/// A show's title without the source Misaka adds to what it found by
/// searching, `琅琊榜 （来源：iqiyi）`.
fn plain_title(title: &str) -> &str {
    match title.rfind(" （来源：") {
        Some(at) if title.ends_with('）') => &title[..at],
        _ => title,
    }
}

/// Misaka's details of a show, under the show's lasting id and plain title.
fn settle_details(details: &mut Value) {
    let lasting = details
        .pointer("/episodes/0/episodeId")
        .and_then(Value::as_i64)
        .and_then(show_of);
    if let Some(lasting) = lasting {
        details["animeId"] = json!(lasting);
        details["bangumiId"] = json!(format!("A{lasting}"));
    }
    if let Some(title) = details.get("animeTitle").and_then(Value::as_str) {
        let title = plain_title(title).to_string();
        details["animeTitle"] = json!(title);
    }
}

/// Fresh details of a show with the episodes only `seen` still lists, in
/// the order of their ids (the source, then the number).
fn merge_details(mut fresh: Value, seen: &Value) -> Value {
    let mut episodes: Vec<Value> = fresh
        .get("episodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let id = |episode: &Value| episode.get("episodeId").and_then(Value::as_i64);
    for episode in seen.get("episodes").and_then(Value::as_array).into_iter().flatten() {
        if !episodes.iter().any(|known| id(known) == id(episode)) {
            episodes.push(episode.clone());
        }
    }
    episodes.sort_by_key(|episode| id(episode).unwrap_or(i64::MAX));
    fresh["episodes"] = json!(episodes);
    for key in ["imageUrl", "summary", "type", "typeDescription"] {
        let empty = fresh.get(key).and_then(Value::as_str).is_none_or(str::is_empty)
            || (key.starts_with("type") && fresh.get("type").and_then(Value::as_str) == Some("other"));
        if empty && let Some(value) = seen.get(key) {
            fresh[key] = value.clone();
        }
    }
    fresh
}

/// Misaka's answer to `match`, its shows under their lasting ids.
fn settle_matches(answer: &mut Value) {
    for found in answer.get_mut("matches").and_then(Value::as_array_mut).into_iter().flatten() {
        if let Some(lasting) = found.get("episodeId").and_then(Value::as_i64).and_then(show_of) {
            found["animeId"] = json!(lasting);
        }
        if let Some(title) = found.get("animeTitle").and_then(Value::as_str) {
            let title = plain_title(title).to_string();
            found["animeTitle"] = json!(title);
        }
    }
}

fn matched(binding: &Binding) -> Value {
    json!({
        "success": true,
        "errorCode": 0,
        "errorMessage": "",
        "isMatched": true,
        "matches": [{
            "episodeId": binding.episode_id,
            "animeId": binding.anime_id,
            "animeTitle": binding.anime_title,
            "episodeTitle": binding.episode_title,
            "type": binding.kind,
            "typeDescription": binding.kind_description,
            "shift": 0,
            "imageUrl": binding.image,
        }],
    })
}

/// A file's fingerprint in a request.
fn fingerprint(body: &Value) -> Option<(String, i64)> {
    let md5 = body
        .get("fileHash")
        .and_then(Value::as_str)
        .filter(|hash| hash.len() == 32 && hash.chars().all(|c| c.is_ascii_hexdigit()))?;
    let size = body
        .get("fileSize")
        .and_then(Value::as_i64)
        .filter(|size| *size > 0)?;
    Some((md5.to_ascii_lowercase(), size))
}

async fn health() -> &'static str {
    "ok"
}

async fn match_file(
    State(gateway): State<Arc<Gateway>>,
    Path(token): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let voter = match gateway.caller(&token, &headers) {
        Ok(voter) => voter,
        Err(refusal) => return refusal,
    };
    let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    if let Some((md5, size)) = fingerprint(&request)
        && let Some(binding) = gateway.store.find(&md5, size, &voter.device)
    {
        if let Some(found) = gateway.check(&binding).await {
            if found.episode_id != binding.episode_id
                && let Err(error) = gateway.store.rebind(&md5, size, binding.episode_id, &found)
            {
                eprintln!("erika-gateway: rebind: {error}");
            }
            return axum::Json(matched(&found)).into_response();
        }
    }
    let answer = gateway
        .client
        .post(gateway.url("match", None))
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await;
    match answer {
        Ok(answer) => {
            let status = StatusCode::from_u16(answer.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            match answer.json::<Value>().await {
                Ok(mut answer) => {
                    settle_matches(&mut answer);
                    (status, axum::Json(answer)).into_response()
                }
                Err(_) => unreachable_server(),
            }
        }
        Err(error) => {
            eprintln!("erika-gateway: match: {error}");
            unreachable_server()
        }
    }
}

async fn confirm(
    State(gateway): State<Arc<Gateway>>,
    Path(token): Path<String>,
    headers: HeaderMap,
    axum::Json(request): axum::Json<Value>,
) -> Response {
    let voter = match gateway.caller(&token, &headers) {
        Ok(voter) => voter,
        Err(refusal) => return refusal,
    };
    let field = |key: &str| {
        request
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let (Some((md5, size)), Some(anime_id), Some(episode_id)) = (
        fingerprint(&request),
        request.get("animeId").and_then(Value::as_i64),
        request.get("episodeId").and_then(Value::as_i64),
    ) else {
        return (StatusCode::BAD_REQUEST, axum::Json(json!({ "success": false, "errorCode": 400, "errorMessage": "fileHash, fileSize, animeId and episodeId" })))
            .into_response();
    };
    let binding = Binding {
        anime_id: show_of(episode_id).unwrap_or(anime_id),
        episode_id,
        anime_title: field("animeTitle"),
        episode_title: field("episodeTitle"),
        number: request
            .get("episodeNumber")
            .and_then(Value::as_i64)
            .unwrap_or(episode_id % 10_000),
        kind: request
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("tvseries")
            .to_string(),
        kind_description: request
            .get("typeDescription")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        image: field("imageUrl"),
        confirmations: 1,
    };
    match gateway
        .store
        .confirm(&md5, size, &voter, &field("fileName"), &binding)
    {
        Ok(()) => axum::Json(json!({ "success": true, "errorCode": 0, "errorMessage": "" }))
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(
                json!({ "success": false, "errorCode": 500, "errorMessage": error.to_string() }),
            ),
        )
            .into_response(),
    }
}

async fn forget(
    State(gateway): State<Arc<Gateway>>,
    Path(token): Path<String>,
    headers: HeaderMap,
    axum::Json(request): axum::Json<Value>,
) -> Response {
    let voter = match gateway.caller(&token, &headers) {
        Ok(voter) => voter,
        Err(refusal) => return refusal,
    };
    if let Some((md5, size)) = fingerprint(&request)
        && let Err(error) = gateway.store.forget(&md5, size, &voter)
    {
        eprintln!("erika-gateway: forget: {error}");
    }
    axum::Json(json!({ "success": true, "errorCode": 0, "errorMessage": "" })).into_response()
}

async fn pass(
    State(gateway): State<Arc<Gateway>>,
    Path((token, rest)): Path<(String, String)>,
    method: Method,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(refusal) = gateway.caller(&token, &headers) {
        return refusal;
    }
    // A show's details: Misaka takes its own ids as `A<id>` (plain digits
    // are bgm.tv's), and lists only some episodes.
    if let Some(id) = rest
        .strip_prefix("bangumi/")
        .map(|id| id.strip_prefix('A').unwrap_or(id))
        .and_then(|id| id.parse::<i64>().ok())
    {
        return match gateway.details(id).await {
            Some(answer) => axum::Json(answer).into_response(),
            None => unreachable_server(),
        };
    }
    if method == Method::GET && rest.starts_with("comment/") {
        return comments(&gateway, &rest, query.as_deref()).await;
    }
    forward(&gateway, method, &rest, query.as_deref(), headers, body).await
}

/// An episode's danmaku. The first time, Misaka waits half a minute for
/// the video site, then answers none and fetches on; the danmaku are in
/// moments later, so those are asked for again.
async fn comments(gateway: &Gateway, rest: &str, query: Option<&str>) -> Response {
    let started = Instant::now();
    let mut tries = 0;
    loop {
        let Some(answer) = gateway.get_json(rest, query).await else {
            return unreachable_server();
        };
        let none = answer.get("count").and_then(Value::as_i64) == Some(0);
        if !none || tries == 4 || started.elapsed() < GAVE_UP_AFTER {
            return axum::Json(answer).into_response();
        }
        tries += 1;
        tokio::time::sleep(Duration::from_secs(4)).await;
    }
}

/// Hand a request to the danmaku server and its answer back.
async fn forward(
    gateway: &Gateway,
    method: Method,
    rest: &str,
    query: Option<&str>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut request = gateway
        .client
        .request(method, gateway.url(rest, query))
        .body(body);
    let kind = headers
        .get(header::CONTENT_TYPE)
        .cloned()
        .unwrap_or(HeaderValue::from_static("application/json"));
    request = request.header(header::CONTENT_TYPE, kind);
    if let Some(accept) = headers.get(header::ACCEPT) {
        request = request.header(header::ACCEPT, accept);
    }
    match request.send().await {
        Ok(answer) => {
            let status =
                StatusCode::from_u16(answer.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let kind = answer
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|kind| HeaderValue::from_bytes(kind.as_bytes()).ok());
            let bytes = answer.bytes().await.unwrap_or_default();
            let mut response = (status, bytes).into_response();
            if let Some(kind) = kind {
                response.headers_mut().insert(header::CONTENT_TYPE, kind);
            }
            response
        }
        Err(error) => {
            eprintln!("erika-gateway: the danmaku server: {error}");
            unreachable_server()
        }
    }
}

/// The gateway's routes.
pub fn app(gateway: Arc<Gateway>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/{token}/api/v2/match", post(match_file))
        .route("/{token}/api/v2/match/confirm", post(confirm))
        .route("/{token}/api/v2/match/forget", post(forget))
        .route("/{token}/api/v2/{*rest}", any(pass))
        .with_state(gateway.clone())
        .merge(gateway.metadata.clone().router())
}
