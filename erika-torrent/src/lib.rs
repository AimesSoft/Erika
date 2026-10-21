//! BitTorrent downloads, ported from NipaPlay's `rust/src/api/torrent.rs`:
//! one librqbit session for the process, whose tasks are kept across starts
//! in a folder of the app's data (fast resume); magnet links and .torrent
//! files, read first to list their files; pausing, resuming, removing a task
//! with or without its files; and a small HTTP server on 127.0.0.1 that
//! streams a file while it downloads, so the player can start before it is
//! done. The browser and the television have no downloads: there every call
//! answers that they are unavailable.
//!
//! Every call blocks the calling thread; adding a magnet link waits for its
//! metadata from the swarm, which takes seconds or minutes.

use std::path::{Path, PathBuf};

#[cfg(not(any(target_arch = "wasm32", target_os = "tvos")))]
mod engine;
#[cfg(not(any(target_arch = "wasm32", target_os = "tvos")))]
mod stream;
#[cfg(test)]
mod tests;

pub type Result<T> = std::result::Result<T, String>;

/// What a task is doing, as librqbit says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    Initializing,
    Live,
    Paused,
    Error,
}

/// A file of a torrent.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskFile {
    /// Its place in the torrent, which names it to librqbit.
    pub index: usize,
    /// Its path inside the torrent, folders joined by `/`.
    pub path: String,
    pub length: u64,
}

impl TaskFile {
    /// The file's own name, without the folders.
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// Whether the player can play it, by its extension.
    pub fn is_video(&self) -> bool {
        const VIDEO: &[&str] = &["mkv", "mp4", "m4v", "mov", "avi", "webm", "flv", "f4v", "ts", "m2ts", "mts", "wmv", "rmvb", "rm", "mpg", "mpeg", "3gp", "ogv", "vob"];
        self.name().rsplit_once('.').is_some_and(|(_, extension)| VIDEO.contains(&extension.to_ascii_lowercase().as_str()))
    }
}

/// A download.
#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub id: usize,
    pub info_hash: String,
    pub name: String,
    /// The folder it writes into.
    pub folder: PathBuf,
    pub state: TaskState,
    /// Bytes downloaded, of all of them, and uploaded.
    pub progress: u64,
    pub total: u64,
    pub uploaded: u64,
    pub finished: bool,
    /// Speeds in megabytes a second while it runs.
    pub down: f64,
    pub up: f64,
    pub error: Option<String>,
}

/// A torrent read before it is added: what it would download.
#[derive(Clone, Debug)]
pub struct Preview {
    pub name: String,
    /// The folder a task of its own would get.
    pub folder_name: String,
    pub total: u64,
    pub files: Vec<TaskFile>,
    #[cfg_attr(any(target_arch = "wasm32", target_os = "tvos"), allow(dead_code))]
    pub(crate) torrent: Vec<u8>,
    #[cfg_attr(any(target_arch = "wasm32", target_os = "tvos"), allow(dead_code))]
    pub(crate) peers: Vec<std::net::SocketAddr>,
    #[cfg_attr(any(target_arch = "wasm32", target_os = "tvos"), allow(dead_code))]
    pub(crate) trackers: Vec<String>,
}

/// A name the file system takes for a folder: without the characters
/// systems refuse, the extension, dots and spaces at its ends. `None` when
/// nothing is left.
pub fn folder_name(name: &str) -> Option<String> {
    let stem = Path::new(name).file_stem().and_then(|stem| stem.to_str()).unwrap_or(name);
    let clean: String = stem
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let clean = clean.trim().trim_matches('.').trim().to_string();
    (!clean.is_empty()).then_some(clean)
}

/// Whether `link` is a magnet link.
pub fn is_magnet(link: &str) -> bool {
    link.trim().to_ascii_lowercase().starts_with("magnet:?")
}

#[cfg(not(any(target_arch = "wasm32", target_os = "tvos")))]
pub use engine::{add, delete, files, forget, pause, preview_file, preview_link, resume, start, stream_url, tasks};

/// Whether downloads run here.
pub fn available() -> bool {
    cfg!(not(any(target_arch = "wasm32", target_os = "tvos")))
}

#[cfg(any(target_arch = "wasm32", target_os = "tvos"))]
mod unavailable {
    use super::*;

    fn none<T>() -> Result<T> {
        Err("downloads are not available here".into())
    }

    pub fn start(_session: &Path, _folder: &Path) -> Result<()> {
        none()
    }
    pub fn preview_link(_link: &str) -> Result<Preview> {
        none()
    }
    pub fn preview_file(_path: &Path) -> Result<Preview> {
        none()
    }
    pub fn add(_preview: &Preview, _folder: &Path, _own_folder: bool) -> Result<usize> {
        none()
    }
    pub fn tasks() -> Vec<Task> {
        Vec::new()
    }
    pub fn files(_id: usize) -> Vec<TaskFile> {
        Vec::new()
    }
    pub fn pause(_id: usize) -> Result<()> {
        none()
    }
    pub fn resume(_id: usize) -> Result<()> {
        none()
    }
    pub fn forget(_id: usize) -> Result<()> {
        none()
    }
    pub fn delete(_id: usize) -> Result<()> {
        none()
    }
    pub fn stream_url(_id: usize, _file: usize, _name: &str) -> Result<String> {
        none()
    }
}

#[cfg(any(target_arch = "wasm32", target_os = "tvos"))]
pub use unavailable::*;
