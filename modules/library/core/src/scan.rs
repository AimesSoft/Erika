//! Scanning a folder, as NipaPlay's scan does (its `file_scan.rs` and
//! `ConcurrentVideoProcessor`): walk the folder for videos; read again only
//! those whose size or change time differ from the last scan, taking their
//! fingerprints; then ask dandanplay, four at a time, which episode each
//! video not matched yet is. The work runs on other threads and hands what
//! it finds to the engine, which alone writes the database.

use erika_store::Fingerprint;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The containers liberika plays, as file extensions.
const VIDEO: &[&str] = &[
    "mkv", "mp4", "m4v", "mov", "avi", "webm", "flv", "f4v", "ts", "m2ts", "mts", "wmv", "rmvb",
    "rm", "mpg", "mpeg", "3gp", "ogv", "vob",
];
/// How many videos are read or asked about at once.
pub(super) const AT_ONCE: usize = 4;

/// A video the walk found.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub url: String,
    pub path: PathBuf,
    pub size: i64,
    pub modified: i64,
}

/// What the scan of a source comes back with, in order.
pub enum Scanned {
    /// Renew the source's scan lease while walking large directories.
    Heartbeat,
    /// The complete directory inventory. Pruning waits for Finished.
    Walked { urls: Vec<String> },
    /// How far reading the changed videos is.
    Progress { done: usize, total: usize },
    /// A video read: its fingerprint, `None` when it could not be read.
    Read {
        found: Found,
        fingerprint: Option<Fingerprint>,
    },
    /// The scan finished, or the folder could not be read.
    Finished { failed: bool },
}

/// One durable job; revisions are captured before either network service runs.
#[derive(Clone, Debug)]
pub struct Video {
    pub job: String,
    pub danmaku_revision: u64,
    pub name: String,
    pub directory: String,
    pub fingerprint: Fingerprint,
}

pub enum Matched {
    Resolved {
        job: String,
        danmaku_revision: u64,
        report: erika_catalog::resolve::FileResolution,
    },
    Finished,
}

/// Whether `path` names a video by its extension.
fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| VIDEO.contains(&extension.to_ascii_lowercase().as_str()))
}

/// The videos under `root`, by path. Links are not followed; hidden
/// folders and files (macOS's `._` copies among them) are skipped.
#[cfg(test)]
pub fn walk(root: &Path) -> std::io::Result<Vec<Found>> {
    walk_with_heartbeat(root, &|| {})
}

fn walk_with_heartbeat(root: &Path, heartbeat: &dyn Fn()) -> std::io::Result<Vec<Found>> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    let mut last_heartbeat = std::time::Instant::now();
    while let Some(folder) = folders.pop() {
        let entries = std::fs::read_dir(&folder)?;
        for entry in entries {
            // A partial directory listing is NOT proof that old files vanished.
            let entry = entry?;
            if last_heartbeat.elapsed() >= std::time::Duration::from_secs(10) {
                heartbeat();
                last_heartbeat = std::time::Instant::now();
            }
            let path = entry.path();
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                folders.push(path);
            } else if kind.is_file() && is_video(&path) {
                let metadata = entry.metadata()?;
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |since| since.as_millis() as i64);
                let url = bern::media::MediaSource::file(&path).url;
                found.push(Found {
                    url,
                    path,
                    size: metadata.len().min(i64::MAX as u64) as i64,
                    modified,
                });
            }
        }
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(found)
}

/// Run `work` on each of `items`, `AT_ONCE` at a time.
fn at_once<T: Send>(items: Vec<T>, work: impl Fn(T) + Sync) {
    let queue = Mutex::new(items.into_iter());
    std::thread::scope(|scope| {
        for _ in 0..AT_ONCE {
            scope.spawn(|| {
                loop {
                    let next = queue
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .next();
                    match next {
                        Some(item) => work(item),
                        None => break,
                    }
                }
            });
        }
    });
}

/// What the last scan left of a video: its size and change time, and
/// whether its fingerprint is known.
pub struct Known {
    pub size: i64,
    pub modified: i64,
    pub fingerprinted: bool,
}

/// Scan the folder at `root`, which held `known` at the last scan.
pub fn scan(root: &Path, known: &HashMap<String, Known>, send: &(dyn Fn(Scanned) + Sync)) {
    let found = match walk_with_heartbeat(root, &|| send(Scanned::Heartbeat)) {
        Ok(found) => found,
        Err(error) => {
            eprintln!("erika: scan {}: {error}", root.display());
            send(Scanned::Finished { failed: true });
            return;
        }
    };
    send(Scanned::Walked {
        urls: found.iter().map(|found| found.url.clone()).collect(),
    });
    let changed: Vec<Found> = found
        .into_iter()
        .filter(|found| {
            !known.get(&found.url).is_some_and(|known| {
                known.fingerprinted && known.size == found.size && known.modified == found.modified
            })
        })
        .collect();
    let total = changed.len();
    let done = Mutex::new(0usize);
    send(Scanned::Progress { done: 0, total });
    at_once(changed, |found| {
        let fingerprint = std::fs::File::open(&found.path)
            .and_then(|file| erika_store::fingerprint(file, found.size as u64))
            .map_err(|error| eprintln!("erika: read {}: {error}", found.path.display()))
            .ok();
        send(Scanned::Read { found, fingerprint });
        let mut done = done.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *done += 1;
        send(Scanned::Progress { done: *done, total });
    });
    send(Scanned::Finished { failed: false });
}

/// The same dandanplay → filename/TMDB pipeline used by direct playback.
/// The engine dispatches at most AT_ONCE claimed jobs in each batch.
pub fn match_videos(
    videos: Vec<Video>,
    client: &erika_catalog::Client,
    kind: Option<erika_catalog::Kind>,
    send: &(dyn Fn(Matched) + Sync),
) {
    at_once(videos, |video| {
        let report = futures_lite::future::block_on(erika_dandanplay::catalog::resolve_file(
            client,
            &video.name,
            &video.directory,
            Some((&video.fingerprint.md5, video.fingerprint.size)),
            kind,
            None,
        ));
        send(Matched::Resolved {
            job: video.job,
            danmaku_revision: video.danmaku_revision,
            report,
        });
    });
    send(Matched::Finished);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_walk_finds_videos_and_the_scan_reads_only_what_changed() {
        let root = std::env::temp_dir().join(format!(
            "erika-scan-{}-{}",
            std::process::id(),
            erika_store::now()
        ));
        std::fs::create_dir_all(root.join("葬送的芙莉莲")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::write(root.join("葬送的芙莉莲/01.mkv"), b"one").unwrap();
        std::fs::write(root.join("葬送的芙莉莲/02.MP4"), b"two").unwrap();
        std::fs::write(root.join("葬送的芙莉莲/notes.txt"), b"x").unwrap();
        std::fs::write(root.join(".hidden/03.mkv"), b"x").unwrap();
        std::fs::write(root.join("._04.mkv"), b"x").unwrap();
        let found = walk(&root).unwrap();
        assert_eq!(
            found
                .iter()
                .map(|found| found
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned())
                .collect::<Vec<_>>(),
            ["01.mkv", "02.MP4"]
        );
        assert!(found[0].url.starts_with("file://") && found[0].url.contains("%E8%91%AC"));

        // The first was scanned before and has not changed.
        let known = HashMap::from([(
            found[0].url.clone(),
            Known {
                size: found[0].size,
                modified: found[0].modified,
                fingerprinted: true,
            },
        )]);
        let sent = Mutex::new(Vec::new());
        scan(&root, &known, &|done| sent.lock().unwrap().push(done));
        let sent = sent.into_inner().unwrap();
        let read: Vec<&Found> = sent
            .iter()
            .filter_map(|done| {
                if let Scanned::Read { found, .. } = done {
                    Some(found)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(read, [&found[1]]);
        assert!(matches!(sent.first(), Some(Scanned::Walked { urls }) if urls.len() == 2));
        assert!(matches!(
            sent.last(),
            Some(Scanned::Finished { failed: false })
        ));

        let gone = Mutex::new(Vec::new());
        scan(&root.join("missing"), &HashMap::new(), &|done| {
            gone.lock().unwrap().push(done)
        });
        assert!(matches!(
            gone.into_inner().unwrap().as_slice(),
            [Scanned::Finished { failed: true }]
        ));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
