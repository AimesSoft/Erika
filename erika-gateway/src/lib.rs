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
//! The token in the path keeps others out; the server's own token stays
//! here, in the server's address.

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
use store::{Binding, Store};

/// How long an episode checked against the server is trusted.
const CHECKED_FOR: Duration = Duration::from_secs(600);

pub struct Gateway {
    /// The token clients put in the path.
    pub token: String,
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
            token,
            upstream: upstream.trim_end_matches('/').to_string(),
            store,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("an HTTP client builds"),
            checked: Mutex::new(HashMap::new()),
        }
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
        let Some(details) = self
            .get_json(&format!("bangumi/{}", binding.anime_id), None)
            .await
        else {
            return Some(binding.clone());
        };
        let has = details
            .pointer("/bangumi/episodes")
            .and_then(Value::as_array)
            .is_some_and(|episodes| {
                episodes.iter().any(|episode| {
                    episode.get("episodeId").and_then(Value::as_i64) == Some(binding.episode_id)
                })
            });
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
                    Some(Binding {
                        anime_id: anime.get("animeId").and_then(Value::as_i64)?,
                        episode_id: episode.get("episodeId").and_then(Value::as_i64)?,
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
    body: Bytes,
) -> Response {
    if token != gateway.token {
        return refused();
    }
    let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    if let Some((md5, size)) = fingerprint(&request)
        && let Some(binding) = gateway.store.find(&md5, size)
    {
        if let Some(found) = gateway.check(&binding).await {
            if found != binding
                && let Err(error) = gateway.store.rebind(&md5, size, &found)
            {
                eprintln!("erika-gateway: rebind: {error}");
            }
            return axum::Json(matched(&found)).into_response();
        }
    }
    forward(
        &gateway,
        Method::POST,
        "match",
        None,
        HeaderMap::new(),
        body,
    )
    .await
}

async fn confirm(
    State(gateway): State<Arc<Gateway>>,
    Path(token): Path<String>,
    axum::Json(request): axum::Json<Value>,
) -> Response {
    if token != gateway.token {
        return refused();
    }
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
        anime_id,
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
        .confirm(&md5, size, &field("fileName"), &binding)
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
    axum::Json(request): axum::Json<Value>,
) -> Response {
    if token != gateway.token {
        return refused();
    }
    if let Some((md5, size)) = fingerprint(&request)
        && let Err(error) = gateway.store.forget(&md5, size)
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
    if token != gateway.token {
        return refused();
    }
    forward(&gateway, method, &rest, query.as_deref(), headers, body).await
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
            (StatusCode::BAD_GATEWAY, axum::Json(json!({ "success": false, "errorCode": 502, "errorMessage": "the danmaku server is out of reach" }))).into_response()
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
        .with_state(gateway)
}
