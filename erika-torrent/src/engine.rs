//! The librqbit session, as NipaPlay keeps it: one per process on its own
//! Tokio runtime, created once (making it again can race the old DHT
//! socket for its saved port), the default folder only a fallback since
//! every task names its own.

use crate::{Preview, Result, Task, TaskFile, TaskState, folder_name};
use librqbit::ByteBufT;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, Api, ListOnlyResponse, Magnet, Session, SessionOptions, SessionPersistenceConfig};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::runtime::{Builder, Runtime};

pub(crate) struct Engine {
    pub(crate) runtime: Runtime,
    api: Mutex<Option<Api>>,
    session: Mutex<Option<Arc<Session>>>,
}

pub(crate) fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| Engine {
        runtime: Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .thread_name("erika-torrent")
            .build()
            .expect("the torrent runtime starts"),
        api: Mutex::new(None),
        session: Mutex::new(None),
    })
}

pub(crate) fn api() -> Result<Api> {
    engine().api.lock().map_err(|_| "torrent API lock poisoned".to_string())?.clone().ok_or_else(|| "downloads have not started".to_string())
}

fn session() -> Result<Arc<Session>> {
    engine().session.lock().map_err(|_| "torrent session lock poisoned".to_string())?.clone().ok_or_else(|| "downloads have not started".to_string())
}

/// An error and what caused it, in one line.
fn chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

/// Start the session, its tasks kept in `session` and resumed; `folder`
/// is where a task without a folder of its own would go. Starting again is
/// nothing.
pub fn start(session: &Path, folder: &Path) -> Result<()> {
    let engine = engine();
    let mut api = engine.api.lock().map_err(|_| "torrent API lock poisoned".to_string())?;
    if api.is_some() {
        return Ok(());
    }
    std::fs::create_dir_all(folder).map_err(|error| format!("create {}: {error}", folder.display()))?;
    std::fs::create_dir_all(session).map_err(|error| format!("create {}: {error}", session.display()))?;
    let started = engine.runtime.block_on(async {
        Session::new_with_opts(
            folder.to_path_buf(),
            SessionOptions {
                fastresume: true,
                persistence: Some(SessionPersistenceConfig::Json { folder: Some(session.to_path_buf()) }),
                disable_dht_persistence: cfg!(target_os = "android"),
                ..Default::default()
            },
        )
        .await
        .map_err(|error| format!("start the torrent session: {error:#}"))
    })?;
    *api = Some(Api::new(Arc::clone(&started), None));
    *engine.session.lock().map_err(|_| "torrent session lock poisoned".to_string())? = Some(started);
    Ok(())
}

/// Read a torrent without adding it: librqbit's `list_only`.
fn list_only(add: AddTorrent<'static>) -> Result<ListOnlyResponse> {
    let session = session()?;
    engine().runtime.block_on(async {
        match session.add_torrent(add, Some(AddTorrentOptions { list_only: true, overwrite: true, ..Default::default() })).await {
            Ok(AddTorrentResponse::ListOnly(response)) => Ok(response),
            Ok(_) => Err("the torrent was added instead of read".to_string()),
            Err(error) => Err(format!("read the torrent: {error:#}")),
        }
    })
}

fn preview(metadata: ListOnlyResponse, trackers: Vec<String>, fallback: Option<String>) -> Result<Preview> {
    let mut files = Vec::new();
    let mut total = 0u64;
    let details = metadata.info.iter_file_details().map_err(|error| format!("list the torrent's files: {error:#}"))?;
    let mut largest: Option<(u64, String)> = None;
    for (index, file) in details.enumerate() {
        total = total.saturating_add(file.len);
        let path = file.filename.to_string().unwrap_or_else(|_| format!("file-{}", index + 1));
        let stem = Path::new(&path).file_stem().and_then(|stem| stem.to_str()).map(str::to_string);
        if let Some(stem) = stem.as_deref().and_then(folder_name)
            && largest.as_ref().is_none_or(|(length, _)| file.len > *length)
        {
            largest = Some((file.len, stem));
        }
        files.push(TaskFile { index, path, length: file.len });
    }
    let name = metadata
        .info
        .name
        .as_ref()
        .map(|name| String::from_utf8_lossy(name.as_slice()).trim().to_string())
        .filter(|name| !name.is_empty());
    // NipaPlay's choice: the torrent's name, else its largest file's.
    let folder = name.as_deref().and_then(folder_name).or(largest.map(|(_, name)| name)).or(fallback).unwrap_or_else(|| "torrent".into());
    Ok(Preview {
        name: name.unwrap_or_else(|| folder.clone()),
        folder_name: folder,
        total,
        files,
        torrent: metadata.torrent_bytes.to_vec(),
        peers: metadata.seen_peers.clone(),
        trackers,
    })
}

/// Read the torrent of a magnet link from the swarm.
pub fn preview_link(link: &str) -> Result<Preview> {
    let link = link.trim();
    let magnet = Magnet::parse(link).map_err(|error| format!("not a magnet link: {error:#}"))?;
    let fallback = magnet.name.as_deref().and_then(folder_name).or_else(|| magnet.as_id20().map(|id| id.as_string()));
    let metadata = list_only(AddTorrent::from_url(link.to_string()))?;
    preview(metadata, magnet.trackers.clone(), fallback)
}

/// Read a .torrent file.
pub fn preview_file(path: &Path) -> Result<Preview> {
    let add = engine()
        .runtime
        .block_on(async { AddTorrent::from_local_filename(&path.to_string_lossy()) })
        .map_err(|error| format!("read {}: {error:#}", path.display()))?;
    let fallback = path.file_stem().and_then(|stem| stem.to_str()).and_then(folder_name);
    preview(list_only(add)?, Vec::new(), fallback)
}

/// Download what `preview` read into `folder`, or into a folder of its own
/// there. Returns the task.
pub fn add(preview: &Preview, folder: &Path, own_folder: bool) -> Result<usize> {
    let api = api()?;
    let output = if own_folder { folder.join(&preview.folder_name) } else { folder.to_path_buf() };
    let options = AddTorrentOptions {
        overwrite: true,
        output_folder: Some(output.to_string_lossy().into_owned()),
        initial_peers: (!preview.peers.is_empty()).then(|| preview.peers.clone()),
        trackers: (!preview.trackers.is_empty()).then(|| preview.trackers.clone()),
        ..Default::default()
    };
    let torrent = preview.torrent.clone();
    let response = engine()
        .runtime
        .block_on(async { api.api_add_torrent(AddTorrent::from_bytes(torrent), Some(options)).await })
        .map_err(|error| format!("add the torrent: {}", chain(&error)))?;
    let value = serde_json::to_value(&response).map_err(|error| error.to_string())?;
    value.get("id").and_then(|id| id.as_u64()).map(|id| id as usize).ok_or_else(|| format!("librqbit answered no task: {value}"))
}

fn text(value: &serde_json::Value, key: &str) -> String {
    value.get(key).and_then(|value| value.as_str()).unwrap_or_default().to_string()
}

fn number(value: &serde_json::Value, key: &str) -> u64 {
    value.get(key).and_then(|value| value.as_u64()).unwrap_or(0)
}

fn speed(live: Option<&serde_json::Value>, key: &str) -> f64 {
    live.and_then(|live| live.get(key)).and_then(|speed| speed.get("mbps")).and_then(|mbps| mbps.as_f64()).unwrap_or(0.0)
}

/// The tasks, in the order they were added.
pub fn tasks() -> Vec<Task> {
    let Ok(api) = api() else {
        return Vec::new();
    };
    let list = api.api_torrent_list_ext(librqbit::api::ApiTorrentListOpts { with_stats: true });
    let Ok(value) = serde_json::to_value(&list) else {
        return Vec::new();
    };
    let mut tasks: Vec<Task> = value
        .get("torrents")
        .and_then(|torrents| torrents.as_array())
        .into_iter()
        .flatten()
        .map(|torrent| {
            let empty = serde_json::Value::Null;
            let stats = torrent.get("stats").unwrap_or(&empty);
            let live = stats.get("live").filter(|live| !live.is_null());
            let error = stats.get("error").and_then(|error| error.as_str()).filter(|error| !error.is_empty()).map(str::to_string);
            Task {
                id: number(torrent, "id") as usize,
                info_hash: text(torrent, "info_hash"),
                name: text(torrent, "name"),
                folder: PathBuf::from(text(torrent, "output_folder")),
                state: match text(stats, "state").as_str() {
                    "live" => TaskState::Live,
                    "paused" => TaskState::Paused,
                    "error" => TaskState::Error,
                    _ if error.is_some() => TaskState::Error,
                    _ => TaskState::Initializing,
                },
                progress: number(stats, "progress_bytes"),
                total: number(stats, "total_bytes"),
                uploaded: number(stats, "uploaded_bytes"),
                finished: stats.get("finished").and_then(|finished| finished.as_bool()).unwrap_or(false),
                down: speed(live, "download_speed"),
                up: speed(live, "upload_speed"),
                error,
            }
        })
        .collect();
    tasks.sort_by_key(|task| task.id);
    tasks
}

/// The files of task `id`.
pub fn files(id: usize) -> Vec<TaskFile> {
    let Ok(details) = api().and_then(|api| api.api_torrent_details(id.into()).map_err(|error| format!("{error:#}"))) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::to_value(&details) else {
        return Vec::new();
    };
    value
        .get("files")
        .and_then(|files| files.as_array())
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, file)| file.get("included").and_then(|included| included.as_bool()) != Some(false))
        .map(|(index, file)| {
            let parts: Vec<String> = file
                .get("components")
                .and_then(|components| components.as_array())
                .map(|components| components.iter().filter_map(|part| part.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            TaskFile { index, path: if parts.is_empty() { text(file, "name") } else { parts.join("/") }, length: number(file, "length") }
        })
        .collect()
}

pub fn pause(id: usize) -> Result<()> {
    let api = api()?;
    engine().runtime.block_on(async { api.api_torrent_action_pause(id.into()).await }).map(|_| ()).map_err(|error| format!("pause: {error:#}"))
}

pub fn resume(id: usize) -> Result<()> {
    let api = api()?;
    engine().runtime.block_on(async { api.api_torrent_action_start(id.into()).await }).map(|_| ()).map_err(|error| format!("resume: {error:#}"))
}

/// Remove task `id`, keeping what it downloaded.
pub fn forget(id: usize) -> Result<()> {
    let api = api()?;
    engine().runtime.block_on(async { api.api_torrent_action_forget(id.into()).await }).map(|_| ()).map_err(|error| format!("remove: {error:#}"))
}

/// Remove task `id` and the files it downloaded.
pub fn delete(id: usize) -> Result<()> {
    let api = api()?;
    engine().runtime.block_on(async { api.api_torrent_action_delete(id.into()).await }).map(|_| ()).map_err(|error| format!("delete: {error:#}"))
}

/// An address the player can read file `file` of task `id` from while it
/// downloads; `name` ends the address so players see the extension.
pub fn stream_url(id: usize, file: usize, name: &str) -> Result<String> {
    api()?;
    let port = crate::stream::serve()?;
    let name: String = name
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            byte => format!("%{byte:02X}"),
        })
        .collect();
    Ok(format!("http://127.0.0.1:{port}/torrent/{id}/stream/{file}/{name}"))
}
