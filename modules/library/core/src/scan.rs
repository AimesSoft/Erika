//! Scanning a folder, as NipaPlay's scan does (its `file_scan.rs` and
//! `ConcurrentVideoProcessor`): walk the folder for videos; read again only
//! those whose size or change time differ from the last scan, taking their
//! fingerprints; then ask dandanplay, four at a time, which episode each
//! video not matched yet is. The work runs on other threads and hands what
//! it finds to the engine, which alone writes the database.

use erika_store::{Anime, Episode, Fingerprint};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The containers liberika plays, as file extensions.
const VIDEO: &[&str] = &[
    "mkv", "mp4", "m4v", "mov", "avi", "webm", "flv", "f4v", "ts", "m2ts", "mts", "wmv", "rmvb",
    "rm", "mpg", "mpeg", "3gp", "ogv", "vob",
];
/// How many videos are read or asked about at once.
const AT_ONCE: usize = 4;

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
    /// The videos the folder holds now; the others it held are gone.
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

/// What asking dandanplay comes back with, in order.
pub enum Matched {
    Progress {
        done: usize,
        total: usize,
    },
    /// The file with `fingerprint` is `episode` of `anime`.
    Episode {
        fingerprint: Fingerprint,
        anime: Anime,
        episode: Episode,
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
pub fn walk(root: &Path) -> std::io::Result<Vec<Found>> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    let mut first = true;
    while let Some(folder) = folders.pop() {
        let entries = match std::fs::read_dir(&folder) {
            Ok(entries) => entries,
            // The folder itself must be readable; a folder inside it may not be.
            Err(error) if first => return Err(error),
            Err(_) => continue,
        };
        first = false;
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                folders.push(path);
            } else if kind.is_file() && is_video(&path) {
                let Ok(metadata) = entry.metadata() else {
                    continue;
                };
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
    let found = match walk(root) {
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

/// Ask which episode each of `videos` (a name and a
/// fingerprint) is: dandanplay for anime, the user's own server for TV
/// series and movies (`custom`).
pub fn match_videos(
    videos: Vec<(String, Fingerprint)>,
    custom: bool,
    send: &(dyn Fn(Matched) + Sync),
) {
    let total = videos.len();
    let done = Mutex::new(0usize);
    send(Matched::Progress { done: 0, total });
    at_once(videos, |(name, fingerprint)| {
        let asked = if custom {
            erika_dandanplay::custom::match_video(&name, &fingerprint.md5, fingerprint.size)
        } else {
            erika_dandanplay::match_video(&name, &fingerprint.md5, fingerprint.size)
        };
        let found = match asked {
            Ok(found) => found,
            Err(error) => {
                eprintln!("erika: match {name}: {}", error.message);
                None
            }
        };
        if let Some(found) = found {
            let number = found.number();
            send(Matched::Episode {
                fingerprint,
                anime: Anime {
                    dandanplay_id: found.anime_id,
                    title: found.anime_title,
                    image: found.image_url,
                },
                episode: Episode {
                    dandanplay_id: found.episode_id,
                    number,
                    title: found.episode_title,
                },
            });
        }
        let mut done = done.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *done += 1;
        send(Matched::Progress { done: *done, total });
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
