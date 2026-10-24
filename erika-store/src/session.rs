//! The browser: no database file, so nothing is kept beyond the session;
//! the engines keep what they show in their own state.

use crate::{
    Anime, Entry, Episode, EpisodeFile, FileId, Fingerprint, Recent, Seen, Shelf, Source, Watching,
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

    pub fn set_source(&self, _id: i64, _name: &str, _location: &str) {}

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
