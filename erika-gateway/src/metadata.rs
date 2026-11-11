//! Anonymous, read-only metadata gateway. The TMDB credential never leaves this
//! process and is never sent to Bangumi. Fixed paths/parameters, bounded cache/body/concurrency and rates;
//! neither account cookies nor client-supplied upstream URLs are forwarded.
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Semaphore;

const MAX_BODY: usize = 8 * 1024 * 1024;
const MAX_CACHE: usize = 32 * 1024 * 1024;
const CACHE_TTL: Duration = Duration::from_secs(3600);

struct Cached {
    at: Instant,
    bytes: Vec<u8>,
}
struct Inner {
    base: reqwest::Url,
    bangumi: reqwest::Url,
    bearer: Option<String>,
    key: Option<String>,
    client: reqwest::Client,
    cache: Mutex<HashMap<String, Cached>>,
    rates: Mutex<HashMap<String, (Instant, u32)>>,
    slots: Semaphore,
}
#[derive(Clone)]
pub struct Metadata(Arc<Inner>);
impl Default for Metadata {
    fn default() -> Self {
        Self::new("https://api.themoviedb.org/3/", None, None).expect("valid TMDB endpoint")
    }
}
impl Metadata {
    pub fn new(
        base: &str,
        bearer: Option<String>,
        key: Option<String>,
    ) -> Result<Self, &'static str> {
        let base = reqwest::Url::parse(base).map_err(|_| "invalid TMDB endpoint")?;
        let local = matches!(base.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
        if (base.scheme() != "https" && !(local && base.scheme() == "http"))
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || !base.path().ends_with('/')
        {
            return Err(
                "TMDB endpoint must be HTTPS (or loopback for tests), without credentials or query",
            );
        }
        let clean = |value: Option<String>| value.filter(|v| !v.trim().is_empty());
        Ok(Self(Arc::new(Inner {
            base,
            bangumi: reqwest::Url::parse("https://next.bgm.tv/p1/")
                .expect("fixed public Bangumi URL"),
            bearer: clean(bearer),
            key: clean(key),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(18))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| "HTTP client failed")?,
            cache: Mutex::default(),
            rates: Mutex::default(),
            slots: Semaphore::new(8),
        })))
    }
    pub fn configured(&self) -> bool {
        self.0.bearer.is_some() || self.0.key.is_some()
    }
    pub fn router(self) -> Router {
        Router::new()
            .route("/metadata/v1/{*path}", get(metadata).options(preflight))
            .with_state(self)
    }
    fn allow(&self, headers: &HeaderMap) -> bool {
        // X-Real-IP must be set/overwritten by the loopback reverse proxy.
        // The global bucket still bounds callers forging their own device IDs.
        let ip = headers
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok())
            .filter(|v| v.len() <= 64)
            .unwrap_or("unknown");
        let mut rates = self.0.rates.lock().unwrap_or_else(|p| p.into_inner());
        rates.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(60));
        if rates.len() >= 10000 && !rates.contains_key(ip) {
            return false;
        }
        let mut allowed = true;
        for (key, limit) in [("global", 1200), (ip, 120)] {
            let (_, count) = rates.entry(key.into()).or_insert((Instant::now(), 0));
            *count = count.saturating_add(1);
            allowed &= *count <= limit;
        }
        allowed
    }
    async fn request(&self, route: Route) -> Result<Vec<u8>, Failure> {
        let cache_key = format!(
            "{:?}:{}?{}",
            route.upstream,
            route.path,
            serde_json::to_string(&route.query).unwrap_or_default()
        );
        if let Some(cached) = self
            .0
            .cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&cache_key)
            .filter(|entry| entry.at.elapsed() < CACHE_TTL)
        {
            return Ok(cached.bytes.clone());
        }
        let _permit = self.0.slots.try_acquire().map_err(|_| Failure::busy())?;
        let base = match route.upstream {
            Upstream::Tmdb => &self.0.base,
            Upstream::Bangumi => &self.0.bangumi,
        };
        let url = base.join(&route.path).map_err(|_| Failure::bad())?;
        let mut request = self
            .0
            .client
            .get(url)
            .query(&route.query)
            .header(header::ACCEPT, "application/json");
        match route.upstream {
            Upstream::Tmdb => {
                if let Some(token) = &self.0.bearer {
                    request = request.bearer_auth(token);
                } else if let Some(key) = &self.0.key {
                    request = request.query(&[("api_key", key)]);
                } else {
                    return Err(Failure::disabled());
                }
            }
            Upstream::Bangumi => {
                request = request.header(
                    header::USER_AGENT,
                    format!(
                        "AimesSoft/Erika/{} (https://github.com/AimesSoft/Erika)",
                        env!("CARGO_PKG_VERSION")
                    ),
                );
            }
        }
        let mut response = request.send().await.map_err(|_| Failure::upstream())?;
        let status = response.status();
        if !status.is_success() {
            return Err(match status.as_u16() {
                404 => Failure {
                    status: StatusCode::NOT_FOUND,
                    code: "not_found",
                    retry: None,
                },
                429 => Failure {
                    status: StatusCode::TOO_MANY_REQUESTS,
                    code: "rate_limited",
                    retry: response
                        .headers()
                        .get(header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse().ok())
                        .map(|n: u64| n.min(3600))
                        .or(Some(60)),
                },
                _ => Failure::upstream(),
            });
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_BODY as u64)
        {
            return Err(Failure::upstream());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Failure::upstream())? {
            if bytes.len() + chunk.len() > MAX_BODY {
                return Err(Failure::upstream());
            }
            bytes.extend_from_slice(&chunk);
        }
        let data: Value = serde_json::from_slice(&bytes).map_err(|_| Failure::upstream())?;
        if !data.is_object() || data.get("success") == Some(&Value::Bool(false)) {
            return Err(Failure::upstream());
        }
        let bytes = serde_json::to_vec(&json!({"schema_version":1,"fetched_at":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),"data":data})).map_err(|_| Failure::upstream())?;
        let mut cache = self.0.cache.lock().unwrap_or_else(|p| p.into_inner());
        cache.retain(|_, entry| entry.at.elapsed() < CACHE_TTL);
        let mut occupied: usize = cache.values().map(|v| v.bytes.len()).sum();
        while occupied + bytes.len() > MAX_CACHE || cache.len() >= 1024 {
            let oldest = cache
                .iter()
                .min_by_key(|(_, v)| v.at)
                .map(|(key, _)| key.clone());
            let Some(oldest) = oldest else {
                break;
            };
            if let Some(removed) = cache.remove(&oldest) {
                occupied = occupied.saturating_sub(removed.bytes.len());
            }
        }
        cache.insert(
            cache_key,
            Cached {
                at: Instant::now(),
                bytes: bytes.clone(),
            },
        );
        Ok(bytes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Upstream {
    Tmdb,
    Bangumi,
}
#[derive(Debug, PartialEq, Eq)]
struct Route {
    upstream: Upstream,
    path: String,
    query: BTreeMap<String, String>,
}
#[derive(Debug)]
struct Failure {
    status: StatusCode,
    code: &'static str,
    retry: Option<u64>,
}
impl Failure {
    fn bad() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_request",
            retry: None,
        }
    }
    fn busy() -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "rate_limited",
            retry: Some(60),
        }
    }
    fn disabled() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "not_configured",
            retry: Some(60),
        }
    }
    fn upstream() -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            code: "unavailable",
            retry: Some(30),
        }
    }
}
impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            Json(json!({"schema_version":1,"error":{"code":self.code,"retry_after":self.retry}})),
        )
            .into_response();
        if let Some(retry) = self.retry {
            response.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from_str(&retry.to_string()).unwrap(),
            );
        }
        cors(response)
    }
}
fn cors(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    response.headers_mut().insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("Retry-After"),
    );
    response
}
async fn preflight() -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, OPTIONS"),
    );
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Accept, Content-Type"),
    );
    cors(response)
}
async fn metadata(
    State(service): State<Metadata>,
    Path(path): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    if !service.allow(&headers) {
        return Failure::busy().into_response();
    }
    if path == "capabilities" {
        return cors(Json(json!({"schema_version":1,"data":{"tmdb":service.configured(),"images":service.configured(),"bangumi_reviews":true,"credentials_required":false}})).into_response());
    }
    let route = match route(&path, query.as_deref().unwrap_or_default()) {
        Ok(route) => route,
        Err(error) => return error.into_response(),
    };
    if route.upstream == Upstream::Tmdb && !service.configured() {
        return Failure::disabled().into_response();
    }
    match service.request(route).await {
        Ok(bytes) => {
            let mut response = bytes.into_response();
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=300"),
            );
            cors(response)
        }
        Err(error) => error.into_response(),
    }
}
fn positive(value: &str) -> bool {
    value.parse::<u64>().is_ok_and(|n| n > 0) && value.bytes().all(|c| c.is_ascii_digit())
}
fn position(value: &str) -> bool {
    value.parse::<u32>().is_ok_and(|n| n <= 10000) && value.bytes().all(|c| c.is_ascii_digit())
}
fn route(path: &str, query: &str) -> Result<Route, Failure> {
    if path.len() > 256 || query.len() > 2048 {
        return Err(Failure::bad());
    }
    let mut args = BTreeMap::new();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if args.insert(key.into_owned(), value.into_owned()).is_some() {
            return Err(Failure::bad());
        }
    }
    let pieces: Vec<_> = path.split('/').collect();
    if let ["bangumi", "subjects", id, "comments"] = pieces.as_slice() {
        if !positive(id) || !args.is_empty() {
            return Err(Failure::bad());
        }
        return Ok(Route {
            upstream: Upstream::Bangumi,
            path: format!("subjects/{id}/comments"),
            query: BTreeMap::from([("limit".into(), "20".into()), ("offset".into(), "0".into())]),
        });
    }
    let (upstream, allowed): (String, &[&str]) = match pieces.as_slice() {
        ["search"] => {
            let kind = args
                .remove("kind")
                .filter(|s| matches!(s.as_str(), "tv" | "movie"))
                .ok_or_else(Failure::bad)?;
            let title = args
                .get("query")
                .filter(|s| {
                    !s.trim().is_empty() && s.len() <= 512 && !s.chars().any(char::is_control)
                })
                .ok_or_else(Failure::bad)?;
            let _ = title;
            if let Some(year) = args.get("year") {
                if !year
                    .parse::<u16>()
                    .is_ok_and(|y| (1800..=2200).contains(&y))
                {
                    return Err(Failure::bad());
                }
            }
            if let Some(page) = args.get("page") {
                if !page.parse::<u32>().is_ok_and(|p| (1..=500).contains(&p)) {
                    return Err(Failure::bad());
                }
            }
            (
                format!("search/{kind}"),
                &["query", "year", "language", "page"],
            )
        }
        ["tmdb", "images", "configuration"] => ("configuration".into(), &[]),
        ["tmdb", kind @ ("tv" | "movie"), id] if positive(id) => {
            (format!("{kind}/{id}"), &["language"])
        }
        ["tmdb", kind @ ("tv" | "movie"), id, "images"] if positive(id) => (
            format!("{kind}/{id}/images"),
            &["language", "include_image_language"],
        ),
        ["tmdb", "tv", id, "seasons", number] if positive(id) && position(number) => {
            (format!("tv/{id}/season/{number}"), &["language"])
        }
        ["tmdb", "tv", id, "seasons", number, "images"] if positive(id) && position(number) => (
            format!("tv/{id}/season/{number}/images"),
            &["language", "include_image_language"],
        ),
        ["tmdb", "find"] => {
            let source = args
                .remove("source")
                .filter(|s| matches!(s.as_str(), "imdb_id" | "tvdb_id" | "wikidata_id"))
                .ok_or_else(Failure::bad)?;
            let id = args
                .remove("id")
                .filter(|s| {
                    !s.is_empty() && s.len() <= 64 && s.bytes().all(|c| c.is_ascii_alphanumeric())
                })
                .ok_or_else(Failure::bad)?;
            args.insert("external_source".into(), source);
            (format!("find/{id}"), &["external_source", "language"])
        }
        _ => return Err(Failure::bad()),
    };
    if args.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(Failure::bad());
    }
    if let Some(language) = args.get("language") {
        if language.is_empty()
            || language.len() > 16
            || !language
                .bytes()
                .all(|c| c.is_ascii_alphabetic() || c == b'-')
        {
            return Err(Failure::bad());
        }
    }
    if let Some(languages) = args.get_mut("include_image_language") {
        let mut values: Vec<_> = languages
            .split(',')
            .map(|v| v.trim().to_ascii_lowercase())
            .collect();
        if values.is_empty()
            || values.len() > 8
            || values.iter().any(|v| {
                v != "null" && (v.len() != 2 || !v.bytes().all(|c| c.is_ascii_alphabetic()))
            })
        {
            return Err(Failure::bad());
        }
        values.sort();
        values.dedup();
        *languages = values.join(",");
    }
    // Adult content isn't silently enabled by clients; requests stay in the
    // application's existing public catalogue policy.
    if upstream.starts_with("search/") {
        args.insert("include_adult".into(), "false".into());
    }
    Ok(Route {
        upstream: Upstream::Tmdb,
        path: upstream,
        query: args,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whitelist_has_types_specials_languages_and_no_open_proxy() {
        assert_eq!(
            route("tmdb/tv/1/seasons/0", "language=zh-CN").unwrap().path,
            "tv/1/season/0"
        );
        assert_eq!(
            route(
                "tmdb/movie/1/images",
                "include_image_language=zh,ja,en,null,zh"
            )
            .unwrap()
            .query["include_image_language"],
            "en,ja,null,zh"
        );
        for (path, query) in [
            ("https://evil.invalid/", ""),
            ("tmdb/tv/../1", ""),
            ("tmdb/tv/1", "api_key=stolen"),
            ("search", "kind=tv&query=A&query=B"),
            ("tmdb/tv/1/images", "url=http://localhost/private"),
            ("tmdb/find", "source=bangumi&id=1"),
        ] {
            assert!(route(path, query).is_err(), "{path}: {query}");
        }
    }
    #[test]
    fn disabled_metadata_does_not_require_an_account_or_leak_credentials() {
        let metadata = Metadata::default();
        assert!(!metadata.configured());
        assert!(Metadata::new("http://public.example/3/", Some("secret".into()), None).is_err());
        assert!(Metadata::new("https://user:secret@example.com/3/", None, None).is_err());
    }
    #[tokio::test]
    async fn browser_reviews_use_a_fixed_public_route_without_any_credentials() {
        let upstream = Router::new().route(
            "/p1/subjects/9/comments",
            get(|headers: HeaderMap, RawQuery(query): RawQuery| async move {
                assert!(!headers.contains_key(header::AUTHORIZATION));
                assert!(!headers.contains_key(header::COOKIE));
                assert!(!headers.contains_key("x-appid"));
                assert_eq!(query.as_deref(), Some("limit=20&offset=0"));
                Json(json!({"data":[]}))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, upstream).await.unwrap();
        });
        let mut service = Metadata::default();
        Arc::get_mut(&mut service.0).unwrap().bangumi =
            reqwest::Url::parse(&format!("http://{addr}/p1/")).unwrap();
        assert!(!service.configured());
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer client-test"),
        );
        headers.insert(header::COOKIE, HeaderValue::from_static("test=private"));
        let response = metadata(
            State(service.clone()),
            Path("bangumi/subjects/9/comments".into()),
            RawQuery(None),
            headers.clone(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        let bytes = axum::body::to_bytes(response.into_body(), MAX_BODY)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap()["data"],
            json!({"data":[]})
        );
        assert!(!String::from_utf8_lossy(&bytes).contains("private"));
        service.0.cache.lock().unwrap().clear();
        Arc::get_mut(&mut service.0).unwrap().bearer = Some("server-tmdb-test".into());
        assert!(
            service
                .request(route("bangumi/subjects/9/comments", "").unwrap())
                .await
                .is_ok()
        );
        for path in [
            "bangumi/subjects/0/comments",
            "bangumi/subjects/../comments",
            "bangumi/subjects/9/collection",
        ] {
            assert!(route(path, "").is_err());
        }
        for query in ["url=http://localhost/private", "limit=500", "token=secret"] {
            assert!(route("bangumi/subjects/9/comments", query).is_err());
        }
        let options = preflight().await;
        assert_eq!(
            options.headers()[header::ACCESS_CONTROL_ALLOW_METHODS],
            "GET, OPTIONS"
        );
        server.abort();
    }

    #[tokio::test]
    async fn requests_are_cached_and_credentials_stay_upstream() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let upstream = Router::new().route(
            "/3/tv/9",
            get(move |headers: HeaderMap| {
                let counted = counted.clone();
                async move {
                    assert_eq!(headers[header::AUTHORIZATION], "Bearer server-only-test");
                    counted.fetch_add(1, Ordering::SeqCst);
                    Json(json!({"id":9,"name":"作品","seasons":[]}))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, upstream).await.unwrap();
        });
        let service = Metadata::new(
            &format!("http://{addr}/3/"),
            Some("server-only-test".into()),
            None,
        )
        .unwrap();
        let first = service
            .request(route("tmdb/tv/9", "language=zh-CN").unwrap())
            .await
            .unwrap();
        let second = service
            .request(route("tmdb/tv/9", "language=zh-CN").unwrap())
            .await
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(
            !String::from_utf8(first)
                .unwrap()
                .contains("server-only-test")
        );
        server.abort();
    }
}
