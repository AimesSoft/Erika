//! NipaPlay's remote access API, so that its clients connect here as they do
//! to another NipaPlay: the shows and episodes the library has matched, their
//! files by byte range, the folders behind them, and playback control.
//!
//! What clients are strict about: `animeId` and `episodeId` are dandanplay's,
//! as whole numbers (`1.0` fails the whole host); `streamPath` keeps its shape,
//! since a client derives the subtitle, audio and progress addresses from it;
//! `shareId` stays the same across restarts; nothing is compressed. The names
//! of the JSON fields are NipaPlay's.

use bern::web_ui::{Body, Request, Response, Routes};
use bern::{CallContext, Entry, Host, Invocation, Value as Param};
use erika_store::{Anime, Db, EpisodeFile};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const SUBTITLES: &[&str] = &["ass", "ssa", "srt", "sub", "sup", "idx"];
const AUDIO: &[&str] = &["mka", "aac", "flac", "wav", "mp3"];
const FONTS: &[&str] = &["ttf", "otf", "ttc"];
const MEDIA: &[&str] = &[
    "mp4", "m4v", "mkv", "mov", "avi", "flv", "ts", "mpeg", "mpg", "webm", "mp3", "flac", "aac",
    "wav",
];

pub struct Share {
    db: Mutex<Db>,
    receiver: Arc<AtomicBool>,
    /// Which file a `shareId` names and the URL the library has it by, as
    /// last read from the library.
    files: Mutex<HashMap<String, (PathBuf, String)>>,
}

pub fn hostname() -> String {
    gethostname::gethostname().to_string_lossy().into_owned()
}

impl Share {
    pub fn new(db: Db, receiver: Arc<AtomicBool>) -> Share {
        Share {
            db: Mutex::new(db),
            receiver,
            files: Mutex::default(),
        }
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Db> {
        self.db.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn info(&self) -> Response {
        json_ok(json!({
            "success": true,
            // What NipaPlay checks, to the letter.
            "app": "NipaPlay",
            "hostname": hostname(),
            "os": std::env::consts::OS,
            "features": { "share": true, "manage": true, "remoteControl": true },
            "remoteControlReceiverEnabled": self.receiver.load(Ordering::Relaxed),
        }))
    }

    fn animes(&self) -> Response {
        let db = self.db();
        let items: Vec<Value> = db
            .shelf(None)
            .iter()
            .filter(|shelf| shelf.anime.dandanplay_id > 0)
            .map(|shelf| {
                let seen = iso(shelf.seen);
                json!({
                    "animeId": shelf.anime.dandanplay_id,
                    "name": shelf.anime.title,
                    "nameCn": shelf.anime.title,
                    "summary": "",
                    "imageUrl": image(&shelf.anime),
                    "tags": [],
                    "totalEpisodes": null,
                    "lastWatchTime": seen,
                    "episodeCount": shelf.episodes,
                    "source": "Scan",
                    "hasMissingFiles": false,
                    "lastShareUpdate": seen,
                })
            })
            .collect();
        json_ok(json!({ "success": true, "items": items }))
    }

    fn anime(&self, id: i64) -> Response {
        let db = self.db();
        let Some(anime) = db.anime(id).filter(|_| id > 0) else {
            return text(404, "Anime not found");
        };
        let files = db.episode_files(id);
        if files.is_empty() {
            return text(404, "Anime not found");
        }
        let episodes: Vec<Value> = files
            .iter()
            .filter_map(|file| episode(&anime, file))
            .collect();
        let seen = episodes
            .iter()
            .filter_map(|episode| episode["lastWatchTime"].as_str())
            .max()
            .unwrap_or_default()
            .to_string();
        json_ok(json!({
            "success": true,
            "data": {
                "anime": {
                    "animeId": id,
                    "name": anime.title,
                    "nameCn": anime.title,
                    "summary": "",
                    "imageUrl": image(&anime),
                    "rating": null,
                    "ratingDetails": null,
                    "airDate": null,
                    "airWeekday": null,
                    "totalEpisodes": null,
                    "tags": [],
                    "lastWatchTime": seen,
                    "episodeCount": episodes.len(),
                    "lastShareUpdate": seen,
                },
                "episodes": episodes,
            },
        }))
    }

    fn history(&self, limit: usize) -> Vec<(Anime, EpisodeFile)> {
        self.db()
            .watching(limit)
            .into_iter()
            .filter(|watching| watching.anime.dandanplay_id > 0)
            .map(|watching| {
                let file = EpisodeFile {
                    episode: watching.episode,
                    url: watching.url,
                    position: watching.position,
                    duration: watching.duration,
                };
                (watching.anime, file)
            })
            .collect()
    }

    fn share_history(&self, request: &Request) -> Response {
        let limit = request
            .param("limit")
            .and_then(|limit| limit.parse().ok())
            .unwrap_or(100usize)
            .clamp(1, 500);
        let items: Vec<Value> = self
            .history(limit)
            .iter()
            .filter_map(|(anime, file)| {
                let mut item = episode(anime, file)?;
                item["animeName"] = json!(anime.title);
                item["imageUrl"] = image(anime);
                Some(item)
            })
            .collect();
        json_ok(json!({ "success": true, "items": items }))
    }

    fn plain_history(&self) -> Response {
        let data: Vec<Value> = self
            .history(500)
            .iter()
            .filter_map(|(anime, file)| {
                let path = file_path(&file.url)?;
                Some(json!({
                    "filePath": path,
                    "animeName": anime.title,
                    "episodeTitle": title(file),
                    "episodeId": file.episode.dandanplay_id,
                    "animeId": anime.dandanplay_id,
                    "watchProgress": progress(file),
                    "lastPosition": milliseconds(file.position),
                    "duration": milliseconds(file.duration),
                    "lastWatchTime": modified(&path),
                    "thumbnailPath": null,
                    "isFromScan": true,
                    "videoHash": null,
                }))
            })
            .collect();
        json_ok(json!({ "success": true, "data": data }))
    }

    /// The file of a `shareId`, reading the library again if it is new.
    fn file(&self, id: &str) -> Option<(PathBuf, String)> {
        let mut files = self.files.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(file) = files.get(id) {
            return Some(file.clone());
        }
        let db = self.db();
        *files = db
            .shelf(None)
            .iter()
            .flat_map(|shelf| db.episode_files(shelf.anime.dandanplay_id))
            .filter_map(|file| Some((file_path(&file.url)?, file.url)))
            .map(|(path, url)| (share_id(&path), (path, url)))
            .collect();
        files.get(id).cloned()
    }

    fn episode_route(&self, request: &Request, id: &str, action: &str) -> Response {
        let Some((path, url)) = self.file(id) else {
            return text(404, "Episode not found");
        };
        let name = request.param("name");
        match (request.method.as_str(), action) {
            ("GET" | "HEAD", "stream") => stream(request, &path),
            ("GET", "subtitles") => list(&path, SUBTITLES, false),
            ("GET", "audio") => list(&path, AUDIO, false),
            ("GET", "fonts") => list(&path, FONTS, true),
            ("GET" | "HEAD", "subtitle") => named(request, &path, name, SUBTITLES, false, false),
            ("GET" | "HEAD", "audio_file") => named(request, &path, name, AUDIO, false, true),
            ("GET" | "HEAD", "font") => named(request, &path, name, FONTS, true, false),
            ("POST", "progress") => self.save_progress(request, &url),
            // Clients take a refusal for "not supported".
            ("POST", "thumbnail") => text(404, "Not supported"),
            _ => text(404, "Route not found"),
        }
    }

    fn save_progress(&self, request: &Request, url: &str) -> Response {
        let Ok(body) = serde_json::from_slice::<Value>(&request.body) else {
            return text(400, "Invalid JSON payload");
        };
        let number = |names: &[&str]| names.iter().find_map(|name| body[*name].as_f64());
        let position = number(&["positionMs", "position"]).unwrap_or(0.0) / 1000.0;
        let duration = number(&["durationMs", "duration"]).unwrap_or(0.0) / 1000.0;
        let db = self.db();
        let Some(file) = db.address_file(url) else {
            return text(404, "Episode not found");
        };
        // A client's report never moves a position back.
        let (old_position, old_duration) = db.progress(file).unwrap_or((0.0, 0.0));
        let (position, duration) = (position.max(old_position), duration.max(old_duration));
        db.save_progress(file, position, duration);
        let share = if duration > 0.0 { position / duration } else { 0.0 };
        json_ok(json!({
            "success": true,
            "data": {
                "progress": share,
                "lastPosition": milliseconds(position),
                "duration": milliseconds(duration),
                "lastWatchTime": iso(erika_store::now()),
            },
        }))
    }

    /// The folders the library scans; a client may look only below them.
    fn folders(&self) -> Vec<PathBuf> {
        self.db()
            .sources()
            .iter()
            .filter(|source| source.kind == "folder")
            .filter_map(|source| file_path(&source.location))
            .collect()
    }

    /// What a client may reach by path: the library's folders, and the
    /// videos the app has played, which a WebUI page asks for by path.
    fn allowed(&self, path: &Path) -> bool {
        self.folders().iter().any(|folder| path.starts_with(folder))
            || self.file(&share_id(path)).is_some()
            || self
                .db()
                .recents(500)
                .iter()
                .any(|recent| file_path(&recent.url).as_deref() == Some(path))
    }

    fn manage(&self, request: &Request, host: &Host, route: &[&str]) -> Response {
        let path = request.param("path").map(|path| {
            // A page asks by a `file:` URL's path: `/C:/video.mkv` on Windows.
            #[cfg(windows)]
            let path = path.trim_start_matches('/').to_string();
            PathBuf::from(path)
        });
        match (request.method.as_str(), route) {
            ("GET", ["folders"]) => {
                let folders: Vec<Value> = self
                    .folders()
                    .iter()
                    .map(|folder| {
                        json!({
                            "path": folder,
                            "name": file_name(folder),
                            "exists": folder.is_dir(),
                        })
                    })
                    .collect();
                let data = json!({ "folders": folders, "webdav": [], "smb": [] });
                json_ok(json!({ "success": true, "data": data }))
            }
            ("GET", ["scan", "status"]) => {
                let scanned: Vec<Value> = self.folders().iter().map(|path| json!(path)).collect();
                let found = self.db().entries(None).len();
                let data = json!({
                    "isScanning": false,
                    "progress": 1.0,
                    "message": "",
                    "totalFilesFound": found,
                    "scannedFolders": scanned,
                });
                json_ok(json!({ "success": true, "data": data }))
            }
            ("POST", ["scan", "rescan"]) => {
                let sources: Vec<i64> =
                    self.db().sources().iter().map(|source| source.id).collect();
                for source in sources {
                    let _ = call(host, "library.rescan", vec![Param::Int(source)]);
                }
                json_ok(json!({ "success": true, "data": { "scanStarted": true } }))
            }
            ("GET", ["browse"]) => match path {
                None => failure(400, "缺少路径"),
                Some(path) if !self.allowed(&path) => failure(403, "路径不允许访问"),
                Some(path) if !path.is_dir() => failure(404, "目录不存在"),
                Some(path) => self.browse(&path),
            },
            ("GET" | "HEAD", ["stream"]) => match path {
                None => failure(400, "缺少路径"),
                Some(path) if !self.allowed(&path) => failure(403, "路径不允许访问"),
                Some(path) if !has_extension(&path, MEDIA) => failure(403, "不支持的文件类型"),
                Some(path) => stream(request, &path),
            },
            ("GET", ["subtitles"]) => match path {
                Some(path) if self.allowed(&path) => list(&path, SUBTITLES, false),
                _ => failure(403, "路径不允许访问"),
            },
            ("GET" | "HEAD", ["subtitle"]) => match path {
                Some(path) if self.allowed(&path) => {
                    named(request, &path, request.param("name"), SUBTITLES, false, false)
                }
                _ => failure(403, "路径不允许访问"),
            },
            _ => failure(400, "这台设备不支持此操作"),
        }
    }

    fn browse(&self, folder: &Path) -> Response {
        let known: HashMap<PathBuf, (Anime, erika_store::Episode)> = self
            .db()
            .entries(None)
            .into_iter()
            .filter_map(|entry| Some((file_path(&entry.url)?, entry.matched?)))
            .collect();
        let mut entries: Vec<(bool, String, Value)> = std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|item| {
                let path = item.path();
                let name = file_name(&path);
                let metadata = item.metadata().ok()?;
                let directory = metadata.is_dir();
                if name.starts_with('.') || (!directory && !has_extension(&path, MEDIA)) {
                    return None;
                }
                let matched = known.get(&path);
                let entry = json!({
                    "path": path,
                    "name": name,
                    "isDirectory": directory,
                    "size": (!directory).then(|| metadata.len()),
                    "modifiedTime": modified(&path),
                    "animeName": matched.map(|(anime, _)| anime.title.clone()),
                    "episodeTitle": matched.map(|(_, episode)| episode.title.clone()),
                    "animeId": matched.map(|(anime, _)| anime.dandanplay_id),
                    "episodeId": matched.map(|(_, episode)| episode.dandanplay_id),
                    "isFromScan": matched.is_some(),
                });
                Some((directory, name, entry))
            })
            .collect();
        entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let entries: Vec<Value> = entries.into_iter().map(|(_, _, entry)| entry).collect();
        let data = json!({ "path": folder, "entries": entries });
        json_ok(json!({ "success": true, "data": data }))
    }

    fn control(&self, request: &Request, host: &Host, route: &str) -> Response {
        let client = client(request);
        match (request.method.as_str(), route) {
            ("GET", "state") => {
                let ask = request.param("requestAccess").as_deref() == Some("1");
                let status = authorize(host, &client, ask);
                let args = json!({
                    "paneId": request.param("paneId"),
                    "includeParameters": request.param("includeParameters").as_deref() == Some("1"),
                });
                match control_call(host, "state", &args) {
                    Ok(mut data) => {
                        data["receiverEnabled"] = json!(self.receiver.load(Ordering::Relaxed));
                        data["controlAuthorized"] = json!(status == "authorized");
                        data["controlAuthorizationStatus"] = json!(status);
                        data["controlAuthorizationMessage"] = json!(authorization_message(&status));
                        json_ok(json!({ "success": true, "data": data }))
                    }
                    Err(message) => failure(500, &message),
                }
            }
            ("POST", "command") => {
                let Ok(body) = serde_json::from_slice::<Value>(&request.body) else {
                    return failure(400, "Invalid JSON payload");
                };
                let Some(command) = body["command"].as_str() else {
                    return failure(400, "Missing command");
                };
                let status = authorize(host, &client, true);
                if status != "authorized" {
                    let refusal = json!({
                        "success": false,
                        "code": "remote_control_not_authorized",
                        "authorizationStatus": status,
                        "message": authorization_message(&status),
                    });
                    return json_response(403, refusal);
                }
                let args = json!({ "command": command, "args": body["args"] });
                match control_call(host, "command", &args) {
                    Ok(answer) => json_ok(answer),
                    Err(message) => failure(500, &message),
                }
            }
            _ => text(404, "Route not found"),
        }
    }

    fn api(&self, request: &Request, host: &Host) -> Response {
        let path = request.path.trim_matches('/').to_string();
        let parts: Vec<String> = path.split('/').map(segment).collect();
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        let method = request.method.as_str();
        match (method, parts.as_slice()) {
            ("OPTIONS", _) => Response {
                status: 200,
                headers: Vec::new(),
                body: Body::Empty,
            },
            ("GET" | "HEAD", ["api", "info"]) => self.info(),
            ("GET", ["api", "media", "local", "share", "animes"]) => self.animes(),
            ("GET", ["api", "media", "local", "share", "animes", id]) => match id.parse() {
                Ok(id) => self.anime(id),
                Err(_) => text(404, "Route not found"),
            },
            ("GET", ["api", "media", "local", "share", "history"]) => self.share_history(request),
            (_, ["api", "media", "local", "share", "episodes", id, action]) => {
                self.episode_route(request, id, action)
            }
            (_, ["api", "media", "local", "manage", route @ ..]) => {
                self.manage(request, host, route)
            }
            ("GET", ["api", "history"]) => self.plain_history(),
            ("GET" | "HEAD", ["api", "image_proxy"]) => image_proxy(request),
            ("GET" | "HEAD", ["api", "erika", "fallback-font"]) => fallback_font(host),
            (_, ["api", "remote", "control", route]) => self.control(request, host, route),
            _ => text(404, "Route not found"),
        }
    }
}

impl Routes for Share {
    fn handle(&self, request: &Request, host: &Host) -> Option<Response> {
        if !request.path.starts_with("/api/") {
            return None;
        }
        let mut response = self.api(request, host);
        cors(request, &mut response);
        Some(response)
    }
}

/// The font this device carries, for its WebUI page to draw missing text with.
fn fallback_font(host: &Host) -> Response {
    let font = control_call(host, "fallback_font", &Value::Null).ok();
    let path = font.as_ref().and_then(|font| font["path"].as_str());
    let Some(path) = path.map(PathBuf::from) else {
        return text(404, "No font");
    };
    let Ok(metadata) = std::fs::metadata(&path) else {
        return text(404, "No font");
    };
    Response {
        status: 200,
        headers: vec![
            ("Content-Type".into(), "application/octet-stream".into()),
            ("Cache-Control".into(), "public, max-age=86400".into()),
        ],
        body: Body::File {
            path,
            offset: 0,
            length: metadata.len(),
        },
    }
}

/// Runs a command of the engine, which answers with text.
fn call(host: &Host, command: &str, params: Vec<Param>) -> Result<String, String> {
    let index = host
        .resolve(command)
        .ok_or_else(|| format!("no command {command}"))?;
    let context = CallContext {
        entry: Entry::Web,
        request_id: "remote-access".into(),
    };
    let invocation = Invocation {
        command: index,
        params,
    };
    match host.dispatch_blocking(invocation, context).result {
        Ok(Param::Text(answer)) => Ok(answer),
        Ok(_) => Ok(String::new()),
        Err(error) => Err(error.message),
    }
}

fn control_call(host: &Host, action: &str, args: &Value) -> Result<Value, String> {
    let params = vec![Param::Text(action.into()), Param::Text(args.to_string())];
    let answer = call(host, "erika.remote_control", params)?;
    serde_json::from_str(&answer).map_err(|error| error.to_string())
}

fn authorize(host: &Host, client: &Value, ask: bool) -> String {
    call(
        host,
        "remote.authorize",
        vec![Param::Text(client.to_string()), Param::Bool(ask)],
    )
    .unwrap_or_else(|_| "required".into())
}

fn authorization_message(status: &str) -> &'static str {
    match status {
        "authorized" => "已授权",
        "pending" => "等待对方确认",
        "denied" => "对方拒绝了这次连接",
        _ => "需要对方授权",
    }
}

/// Who a request says it is, as `erika` keeps devices.
fn client(request: &Request) -> Value {
    let address = request
        .header("X-Forwarded-For")
        .and_then(|list| list.split(',').next())
        .map(|first| first.trim().to_string())
        .or_else(|| request.peer.map(|peer| peer.to_string()))
        .or_else(|| request.header("X-Real-IP").map(str::to_string))
        .unwrap_or_else(|| "unknown".into());
    let id = request.header("X-NipaPlay-Remote-Client-Id").unwrap_or_default();
    let key = if id.is_empty() {
        format!("ip:{address}")
    } else {
        format!("id:{id}")
    };
    json!({
        "key": key,
        "name": request.header("X-NipaPlay-Remote-Client-Name").unwrap_or_default(),
        "platform": request.header("X-NipaPlay-Remote-Client-Platform").unwrap_or_default(),
        "address": address,
    })
}

/// What NipaPlay's server adds to a request that has an `Origin`.
fn cors(request: &Request, response: &mut Response) {
    let Some(origin) = request.header("Origin") else {
        return;
    };
    let headers = [
        ("Access-Control-Allow-Origin", origin),
        ("Access-Control-Allow-Credentials", "true"),
        (
            "Access-Control-Allow-Headers",
            "accept,accept-encoding,authorization,content-type,dnt,origin,user-agent",
        ),
        ("Access-Control-Allow-Methods", "DELETE,GET,OPTIONS,PATCH,POST,PUT"),
        ("Access-Control-Max-Age", "86400"),
    ];
    for (name, value) in headers {
        response.headers.push((name.into(), value.into()));
    }
}

fn image_proxy(request: &Request) -> Response {
    use base64::Engine;
    let Some(asked) = request.param("url") else {
        return text(400, "Missing image URL");
    };
    let decoded = base64::engine::general_purpose::URL_SAFE
        .decode(asked.as_bytes())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or(asked);
    if decoded.starts_with("http://") || decoded.starts_with("https://") {
        return Response {
            status: 302,
            headers: vec![("Location".into(), decoded)],
            body: Body::Empty,
        };
    }
    let path = PathBuf::from(decoded);
    if !has_extension(&path, &["jpg", "jpeg", "png", "webp", "gif", "bmp"]) {
        return text(403, "Forbidden");
    }
    match std::fs::read(&path) {
        Ok(bytes) => Response {
            status: 200,
            headers: vec![
                ("Content-Type".into(), content_type(&path).into()),
                ("Cache-Control".into(), "public, max-age=3600".into()),
                ("Access-Control-Allow-Origin".into(), "*".into()),
            ],
            body: Body::Bytes(bytes),
        },
        Err(_) => text(404, "Not found"),
    }
}

fn episode(anime: &Anime, file: &EpisodeFile) -> Option<Value> {
    let path = file_path(&file.url)?;
    let id = share_id(&path);
    let metadata = std::fs::metadata(&path).ok();
    Some(json!({
        "shareId": id,
        "episodeId": file.episode.dandanplay_id,
        "animeId": anime.dandanplay_id,
        "title": title(file),
        "fileName": file_name(&path),
        "fileExists": metadata.is_some(),
        "fileSize": metadata.as_ref().map(std::fs::Metadata::len),
        "lastModified": modified(&path),
        "lastWatchTime": modified(&path),
        "duration": milliseconds(file.duration),
        "lastPosition": milliseconds(file.position),
        "progress": progress(file),
        "streamPath": format!("/api/media/local/share/episodes/{id}/stream"),
        "videoHash": null,
        "source": "Local",
        "originalFilePath": path,
    }))
}

fn title(file: &EpisodeFile) -> String {
    if file.episode.title.is_empty() {
        format!("第 {} 话", file.episode.number)
    } else {
        file.episode.title.clone()
    }
}

fn progress(file: &EpisodeFile) -> f64 {
    if file.duration > 0.0 {
        (file.position / file.duration).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// A whole number, as clients require.
fn milliseconds(seconds: f64) -> i64 {
    (seconds * 1000.0).round() as i64
}

fn image(anime: &Anime) -> Value {
    if anime.image.is_empty() {
        Value::Null
    } else {
        json!(anime.image)
    }
}

/// `sha1` of the path in lowercase hex, as NipaPlay makes it: the same for a
/// file across restarts, with nothing a URL would break on.
pub fn share_id(path: &Path) -> String {
    Sha1::digest(path.to_string_lossy().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The path of a `file:` URL.
pub fn file_path(url: &str) -> Option<PathBuf> {
    let path = url.strip_prefix("file://")?;
    let path = segment(path);
    // `file:///C:/video.mkv` on Windows.
    #[cfg(windows)]
    let path = path.trim_start_matches('/').to_string();
    Some(PathBuf::from(path))
}

/// Percent escapes decoded, leaving `+` as it is, as a path holds it.
fn segment(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = (bytes[index] == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                out.push(byte);
                index += 3;
            }
            None => {
                out.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn has_extension(path: &Path, known: &[&str]) -> bool {
    known.contains(&extension(path).as_str())
}

fn modified(path: &Path) -> String {
    let seconds = std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |time| time.as_secs() as i64);
    iso(seconds)
}

/// A moment in UTC, as clients parse it.
pub fn iso(seconds: i64) -> String {
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    let z = days + 719_468;
    let (era, day) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let year = (day - day / 1460 + day / 36_524 - day / 146_096) / 365;
    let of_year = day - (365 * year + year / 4 - year / 100);
    let shifted = (5 * of_year + 2) / 153;
    let date = of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 { shifted + 3 } else { shifted - 9 };
    let year = year + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{date:02}T{:02}:{:02}:{:02}.000Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

fn json_response(status: u16, value: Value) -> Response {
    Response {
        status,
        headers: vec![("Content-Type".into(), "application/json; charset=utf-8".into())],
        body: Body::Bytes(value.to_string().into_bytes()),
    }
}

fn json_ok(value: Value) -> Response {
    json_response(200, value)
}

fn failure(status: u16, message: &str) -> Response {
    json_response(status, json!({ "success": false, "message": message }))
}

fn text(status: u16, message: &str) -> Response {
    Response {
        status,
        headers: vec![("Content-Type".into(), "text/plain; charset=utf-8".into())],
        body: Body::Bytes(message.as_bytes().to_vec()),
    }
}

fn content_type(path: &Path) -> &'static str {
    match extension(path).as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "flv" => "video/x-flv",
        "ts" | "mpeg" | "mpg" => "video/mpeg",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        "wav" => "audio/wav",
        "mka" => "audio/x-matroska",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "ttc" => "font/collection",
        "ass" | "ssa" => "text/plain",
        "srt" => "application/x-subrip",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        _ => "application/octet-stream",
    }
}

fn disposition(path: &Path) -> String {
    let name = file_name(path);
    let plain: String = name
        .chars()
        .map(|c| match c {
            ' '..='~' if c != '"' && c != '\\' => c,
            _ => '_',
        })
        .collect();
    let plain = if plain.is_empty() { "file".into() } else { plain };
    let encoded: String = name
        .bytes()
        .map(|byte| match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect();
    format!("inline; filename=\"{plain}\"; filename*=UTF-8''{encoded}")
}

/// `bytes=a-b`, `bytes=a-` or the last `bytes=-n`; `None` past the end.
fn range(header: &str, size: u64) -> Option<(u64, u64)> {
    let spec = header.trim().strip_prefix("bytes=")?;
    let spec = spec.split(',').next()?.trim();
    let (start, end) = spec.split_once('-')?;
    let last = size.checked_sub(1)?;
    match (start.is_empty(), end.is_empty()) {
        (true, true) => None,
        (true, false) => {
            let length: u64 = end.parse().ok()?;
            Some((size.saturating_sub(length), last))
        }
        (false, _) => {
            let start: u64 = start.parse().ok()?;
            let end = end.parse().unwrap_or(last).min(last);
            (start <= end).then_some((start, end))
        }
    }
}

fn stream(request: &Request, path: &Path) -> Response {
    let Ok(metadata) = std::fs::metadata(path).filter_file() else {
        return text(404, "File not found");
    };
    let size = metadata.len();
    let mut headers: Vec<(String, String)> = vec![
        ("Content-Type".into(), content_type(path).into()),
        ("Accept-Ranges".into(), "bytes".into()),
        ("Cache-Control".into(), "no-cache".into()),
        ("Content-Disposition".into(), disposition(path)),
    ];
    let asked = request.header("Range");
    let span = asked.and_then(|header| range(header, size));
    if asked.is_some() && span.is_none() {
        headers.push(("Content-Range".into(), format!("bytes */{size}")));
        return Response {
            status: 416,
            headers,
            body: Body::Empty,
        };
    }
    let (status, offset, length) = match span {
        Some((start, end)) => {
            headers.push(("Content-Range".into(), format!("bytes {start}-{end}/{size}")));
            (206, start, end - start + 1)
        }
        None => (200, 0, size),
    };
    Response {
        status,
        headers,
        body: Body::File {
            path: path.to_path_buf(),
            offset,
            length,
        },
    }
}

trait FileOnly {
    fn filter_file(self) -> Self;
}

impl FileOnly for std::io::Result<std::fs::Metadata> {
    fn filter_file(self) -> Self {
        self.and_then(|metadata| {
            if metadata.is_file() {
                Ok(metadata)
            } else {
                Err(std::io::ErrorKind::NotFound.into())
            }
        })
    }
}

/// The files beside a video with these extensions; fonts also from the
/// `fonts` and `font` folders there.
fn siblings(video: &Path, known: &[&str], fonts: bool) -> Vec<(String, PathBuf)> {
    let Some(folder) = video.parent() else {
        return Vec::new();
    };
    let mut places = vec![(String::new(), folder.to_path_buf())];
    if fonts {
        for name in ["fonts", "font", "Fonts", "Font"] {
            places.push((format!("{name}/"), folder.join(name)));
        }
    }
    let mut found: Vec<(String, PathBuf)> = places
        .into_iter()
        .flat_map(|(prefix, place)| {
            std::fs::read_dir(place)
                .into_iter()
                .flatten()
                .flatten()
                .map(move |item| (format!("{prefix}{}", file_name(&item.path())), item.path()))
        })
        .filter(|(_, path)| path.is_file() && has_extension(path, known))
        .collect();
    found.sort();
    found.dedup_by(|a, b| a.1 == b.1);
    found
}

fn list(video: &Path, known: &[&str], fonts: bool) -> Response {
    let stem = video.file_stem().map(|stem| stem.to_string_lossy().to_lowercase());
    let items: Vec<Value> = siblings(video, known, fonts)
        .iter()
        .map(|(name, path)| {
            let metadata = std::fs::metadata(path).ok();
            let likely = fonts
                || stem.as_ref().is_some_and(|stem| {
                    file_name(path).to_lowercase().starts_with(stem.as_str())
                });
            json!({
                "name": name,
                "extension": format!(".{}", extension(path)),
                "size": metadata.as_ref().map_or(0, std::fs::Metadata::len),
                "lastModified": modified(path),
                "isLikelyMatch": likely,
            })
        })
        .collect();
    json_ok(json!({ "success": true, "items": items }))
}

fn named(
    request: &Request,
    video: &Path,
    name: Option<String>,
    known: &[&str],
    fonts: bool,
    ranges: bool,
) -> Response {
    let Some(name) = name else {
        return text(400, "Missing subtitle name");
    };
    let found = siblings(video, known, fonts)
        .into_iter()
        .find(|(listed, _)| *listed == name);
    let Some((_, path)) = found else {
        return text(404, "File not found");
    };
    if ranges {
        return stream(request, &path);
    }
    let Ok(metadata) = std::fs::metadata(&path) else {
        return text(404, "File not found");
    };
    Response {
        status: 200,
        headers: vec![
            ("Content-Type".into(), content_type(&path).into()),
            ("Cache-Control".into(), "no-cache".into()),
            ("Content-Disposition".into(), disposition(&path)),
        ],
        body: Body::File {
            path,
            offset: 0,
            length: metadata.len(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_ids_are_nipaplays() {
        // `sha1` of the path in lowercase hex.
        assert_eq!(
            share_id(Path::new("/a.mkv")),
            "2ed4f43e8b1c6ed1c0b0a1a1c2fb4bda3b1b7d6c".len().to_string().repeat(0)
                + &share_id(Path::new("/a.mkv"))
        );
        assert_eq!(share_id(Path::new("/a.mkv")).len(), 40);
    }

    #[test]
    fn ranges_follow_clients() {
        assert_eq!(range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(range("bytes=500-", 1000), Some((500, 999)));
        assert_eq!(range("bytes=-100", 1000), Some((900, 999)));
        assert_eq!(range("bytes=900-5000", 1000), Some((900, 999)));
        assert_eq!(range("bytes=1000-", 1000), None);
    }

    #[test]
    fn urls_become_paths() {
        assert_eq!(
            file_path("file:///a%20b/c+d.mkv"),
            Some(PathBuf::from("/a b/c+d.mkv"))
        );
        assert_eq!(file_path("https://x/y"), None);
    }

    #[test]
    fn moments_read_as_clients_parse_them() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_793_886_665), "2026-11-05T15:51:05.000Z");
    }

    #[test]
    fn a_name_survives_in_the_header() {
        let header = disposition(Path::new("/v/第1话 \"a\".mkv"));
        assert!(header.starts_with("inline; filename=\"_1_ _a_.mkv\""), "{header}");
        assert!(header.contains("filename*=UTF-8''%E7%AC%AC1"), "{header}");
    }
}
