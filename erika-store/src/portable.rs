//! What the account sync carries between devices, row by row: the records
//! that mean the same on another device. Addresses, scans and pictures
//! belong to this device and are found again there.

use crate::Fingerprint;

/// Which file a record is about on any device: its fingerprint, or the id
/// a media server gave it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileKey {
    Hash { md5: String, size: u64 },
    Item(String),
}

impl FileKey {
    pub fn of(fingerprint: &Fingerprint) -> FileKey {
        FileKey::Hash {
            md5: fingerprint.md5.clone(),
            size: fingerprint.size,
        }
    }

    /// As text: `md5:size`, or `item:id`.
    pub fn text(&self) -> String {
        match self {
            FileKey::Hash { md5, size } => format!("{md5}:{size}"),
            FileKey::Item(item) => format!("item:{item}"),
        }
    }

    pub fn parse(text: &str) -> Option<FileKey> {
        if let Some(item) = text.strip_prefix("item:") {
            return Some(FileKey::Item(item.into()));
        }
        let (md5, size) = text.rsplit_once(':')?;
        Some(FileKey::Hash {
            md5: md5.into(),
            size: size.parse().ok()?,
        })
    }
}

/// A place the library scans.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub kind: String,
    pub name: String,
    pub location: String,
    pub content: String,
    pub added: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Anime {
    pub dandanplay_id: i64,
    pub bangumi_id: Option<i64>,
    pub title: String,
    pub image: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Episode {
    pub dandanplay_id: i64,
    /// Its anime's dandanplay id.
    pub anime: i64,
    pub number: Option<i64>,
    pub bangumi_id: Option<i64>,
    pub title: String,
}

/// Which episode a file is, and who said so: `auto`, `manual`, or `none`
/// for a file the user said is no episode.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub file: FileKey,
    /// The episode's dandanplay id.
    pub episode: Option<i64>,
    pub matched_by: Option<String>,
}

/// Where a file stopped.
#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub file: FileKey,
    pub position: f64,
    pub duration: f64,
    pub danmaku_offset: f64,
    pub at: i64,
}

/// An episode marked watched: by its dandanplay id, or by its number in
/// its anime when it has none.
#[derive(Clone, Debug, PartialEq)]
pub struct Watched {
    pub episode: Option<i64>,
    /// Its anime's dandanplay id.
    pub anime: i64,
    pub number: Option<i64>,
    pub at: i64,
    pub manual: bool,
}

impl Watched {
    /// The same episode on any device: its id, or `anime#number`.
    pub fn key(&self) -> String {
        match (self.episode, self.number) {
            (Some(id), _) => id.to_string(),
            (None, number) => format!("{}#{}", self.anime, number.unwrap_or(0)),
        }
    }
}

/// A video opened directly, among the recent ones.
#[derive(Clone, Debug, PartialEq)]
pub struct Recent {
    pub url: String,
    pub title: String,
    pub position: f64,
    pub duration: f64,
    pub at: i64,
    /// The file it was, when known.
    pub file: Option<FileKey>,
}

/// What the user marked an anime as, in the anime module's words, and the
/// score they gave it.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    /// Its dandanplay id.
    pub anime: i64,
    /// `None` when only rated.
    pub status: Option<String>,
    /// 1 to 10, or 0 when not rated.
    pub rating: i64,
    pub at: i64,
}
