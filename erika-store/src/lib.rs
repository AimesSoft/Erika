//! What Erika keeps on the device, in one SQLite database in the app's data
//! folder (`bern::storage::data_dir`), as design/index.html's storage plan
//! lays it out:
//!
//! - an **address** is where a video is: a source and a path in it;
//! - a **file** is which video it is: the MD5 of its first 16 MiB and its
//!   size, so the same video in two places, or moved, is one file;
//! - an **episode** of an **anime** is what the video is, linked from the
//!   file, with the ids of the services that know it.
//!
//! Playback progress and the danmaku offset belong to the file (versions of
//! one episode differ in timing); "watched" belongs to the episode. Copies
//! of what services return, which can be fetched again, go to the cache
//! folder instead, and secrets to the system's credential store.
//!
//! Every engine opens its own connection; the shell and the modules share
//! the file. An engine without a host (tests) gets a database in memory of
//! its own, and the browser keeps nothing beyond the session.

#[cfg(not(target_arch = "wasm32"))]
mod sqlite;
#[cfg(not(target_arch = "wasm32"))]
pub use sqlite::Db;

#[cfg(target_arch = "wasm32")]
mod session;
#[cfg(target_arch = "wasm32")]
pub use session::Db;

pub mod portable;

/// A file's identity: the MD5 of its first 16 MiB, in hex, and its size in
/// bytes. dandanplay matches a video by these two.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    pub md5: String,
    pub size: u64,
}

/// How much of a file its fingerprint reads.
pub const FINGERPRINT_BYTES: u64 = 16 << 20;

/// The fingerprint of a file of `size` bytes, read from its start.
pub fn fingerprint(file: impl std::io::Read, size: u64) -> std::io::Result<Fingerprint> {
    use md5::{Digest, Md5};
    use std::io::Read;
    let mut hasher = Md5::new();
    let mut head = file.take(FINGERPRINT_BYTES);
    let mut buffer = vec![0; 1 << 20];
    loop {
        let read = head.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let md5 = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(Fingerprint { md5, size })
}

/// The identity of a file row.
pub type FileId = i64;

/// A video played lately, newest first in [`Db::recents`].
#[derive(Clone, Debug, PartialEq)]
pub struct Recent {
    /// What plays it again: a `file:` or `http(s):` URL.
    pub url: String,
    pub title: String,
    /// A picture of it, a file in the cache folder, when one was taken.
    pub still: Option<String>,
    /// Where it stopped and how long it is, in seconds.
    pub position: f64,
    pub duration: f64,
}

/// An anime as the app last saw it, by its dandanplay id.
#[derive(Clone, Debug, PartialEq)]
pub struct Anime {
    pub dandanplay_id: i64,
    pub title: String,
    /// Its poster, an `https:` URL.
    pub image: String,
}

/// An episode, by its dandanplay id, and its number within its anime.
#[derive(Clone, Debug, PartialEq)]
pub struct Episode {
    pub dandanplay_id: i64,
    pub number: i64,
    pub title: String,
}

/// A file the user has of an episode: where it was seen last and where it
/// stopped.
#[derive(Clone, Debug, PartialEq)]
pub struct EpisodeFile {
    pub episode: Episode,
    pub url: String,
    pub position: f64,
    pub duration: f64,
}

/// An anime being watched: the episode played last, from where.
#[derive(Clone, Debug, PartialEq)]
pub struct Watching {
    pub anime: Anime,
    pub episode: Episode,
    pub url: String,
    pub still: Option<String>,
    pub position: f64,
    pub duration: f64,
}

/// An anime the user has files of: how many of its episodes, how many of
/// them watched to the end, and when one was first and last seen.
#[derive(Clone, Debug, PartialEq)]
pub struct Shelf {
    pub anime: Anime,
    pub episodes: i64,
    pub watched: i64,
    pub added: i64,
    pub seen: i64,
}

/// A place the library scans, a folder on this device, with how many
/// videos the last scan found there and how many of them are not matched.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub id: i64,
    /// What it is: `folder`.
    pub kind: String,
    pub name: String,
    /// Where it is: a `file:` URL.
    pub location: String,
    /// What it holds: `anime`, `series` or `movie`.
    pub content: String,
    pub files: i64,
    pub unmatched: i64,
}

/// A video a source holds, as the last scan left it: its address, how big
/// it was and when it was changed then, which file it is, and whether the
/// file is matched or the user decided what it is (matched it by hand or
/// unlinked it), which scans leave alone.
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub url: String,
    pub size: i64,
    pub modified: i64,
    pub fingerprint: Option<Fingerprint>,
    pub matched: bool,
    pub decided: bool,
}

/// A video in a source and what it was matched to, for browsing the source.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Which address it is, for as long as the address stays.
    pub key: i64,
    pub source: i64,
    pub url: String,
    pub file: Option<FileId>,
    pub matched: Option<(Anime, Episode)>,
    /// Whether the user matched it by hand.
    pub manual: bool,
}

/// The current time in seconds since 1970.
pub fn now() -> i64 {
    bern::date::now().unix
}

#[cfg(test)]
mod tests;
