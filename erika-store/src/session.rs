//! The browser: no database file, so nothing is kept beyond the session;
//! the engines keep what they show in their own state.

use crate::{
    Anime, Entry, Episode, EpisodeFile, FileId, Fingerprint, Recent, Seen, Shelf, Source, Watching,
    portable,
};

pub struct Db;

impl Db {
    pub fn open() -> Db {
        Db
    }

    pub fn in_memory() -> Db {
        Db
    }

    pub fn opened(&self, _url: &str, _fingerprint: Option<&Fingerprint>) -> Option<FileId> {
        None
    }

    pub fn progress(&self, _file: FileId) -> Option<(f64, f64)> {
        None
    }

    pub fn save_progress(&self, _file: FileId, _position: f64, _duration: f64) {}

    pub fn remember(
        &self,
        _url: &str,
        _title: &str,
        _still: Option<&str>,
        _position: f64,
        _duration: f64,
    ) {
    }

    pub fn recents(&self, _limit: usize) -> Vec<Recent> {
        Vec::new()
    }

    pub fn stills(&self) -> Vec<String> {
        Vec::new()
    }

    pub fn setting(&self, _key: &str) -> Option<String> {
        None
    }

    pub fn set_setting(&self, _key: &str, _value: Option<&str>) {}

    pub fn remember_anime(&self, _anime: &Anime) {}

    pub fn anime(&self, _dandanplay_id: i64) -> Option<Anime> {
        None
    }

    pub fn link_episode(&self, _file: FileId, _anime: &Anime, _episode: &Episode, _manual: bool) {}

    pub fn file_episode(&self, _file: FileId) -> Option<(Anime, Episode)> {
        None
    }

    pub fn episode_files(&self, _dandanplay_id: i64) -> Vec<EpisodeFile> {
        Vec::new()
    }

    pub fn watching(&self, _limit: usize) -> Vec<Watching> {
        Vec::new()
    }

    pub fn shelf(&self, _source: Option<i64>) -> Vec<Shelf> {
        Vec::new()
    }

    pub fn file_id(&self, _fingerprint: &Fingerprint) -> Option<FileId> {
        None
    }

    pub fn add_source(
        &self,
        _kind: &str,
        _name: &str,
        _location: &str,
        _content: &str,
    ) -> Option<i64> {
        None
    }

    pub fn set_source_content(&self, _id: i64, _content: &str) {}

    pub fn fingerprint_of(&self, _file: FileId) -> Option<Fingerprint> {
        None
    }

    pub fn remove_source(&self, _id: i64) {}

    pub fn sources(&self) -> Vec<Source> {
        Vec::new()
    }

    pub fn seen(&self, _source: i64) -> Vec<Seen> {
        Vec::new()
    }

    pub fn saw(
        &self,
        _source: i64,
        _url: &str,
        _size: i64,
        _modified: i64,
        _fingerprint: Option<&Fingerprint>,
    ) -> Option<FileId> {
        None
    }

    pub fn forget(&self, _source: i64, _urls: &[String]) {}

    pub fn entries(&self, _source: Option<i64>) -> Vec<Entry> {
        Vec::new()
    }

    pub fn link_files(&self, _anime: &Anime, _links: &[(FileId, Episode)]) -> bool {
        false
    }

    pub fn unlink(&self, _file: FileId) {}
}

/// The account sync's rows: the browser keeps none.
impl Db {
    pub fn transaction<T>(&self, work: impl FnOnce() -> T) -> T {
        work()
    }
    pub fn settings(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    pub fn portable_sources(&self) -> Vec<portable::Source> {
        Vec::new()
    }
    pub fn put_source(&self, _source: &portable::Source) -> Option<(i64, bool)> {
        None
    }
    pub fn remove_source_at(&self, _kind: &str, _location: &str) {}
    pub fn portable_anime(&self) -> Vec<portable::Anime> {
        Vec::new()
    }
    pub fn put_anime(&self, _anime: &portable::Anime) {}
    pub fn portable_episodes(&self) -> Vec<portable::Episode> {
        Vec::new()
    }
    pub fn put_episode(&self, _episode: &portable::Episode) {}
    pub fn portable_links(&self) -> Vec<portable::Link> {
        Vec::new()
    }
    pub fn put_link(&self, _link: &portable::Link) {}
    pub fn remove_link(&self, _key: &portable::FileKey) {}
    pub fn portable_progress(&self) -> Vec<portable::Progress> {
        Vec::new()
    }
    pub fn put_progress(&self, _progress: &portable::Progress) {}
    pub fn remove_progress(&self, _key: &portable::FileKey) {}
    pub fn portable_watched(&self) -> Vec<portable::Watched> {
        Vec::new()
    }
    pub fn put_watched(&self, _watched: &portable::Watched) {}
    pub fn remove_watched(&self, _watched: &portable::Watched) {}
    pub fn portable_recents(&self) -> Vec<portable::Recent> {
        Vec::new()
    }
    pub fn put_recent(&self, _recent: &portable::Recent) {}
    pub fn remove_recent(&self, _url: &str) {}
    pub fn anime_marks(&self) -> Vec<portable::Mark> {
        Vec::new()
    }
    pub fn set_anime_mark(&self, _mark: &portable::Mark) {}
    pub fn remove_anime_mark(&self, _anime: i64) {}
    pub fn extensions(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    pub fn extension(&self, _id: &str) -> Option<String> {
        None
    }
    pub fn set_extension(&self, _id: &str, _json: Option<&str>) {}
}
