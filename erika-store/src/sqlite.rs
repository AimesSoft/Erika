//! The database on devices with files.

use crate::{
    Anime, Entry, Episode, EpisodeFile, FileId, Fingerprint, Mark, Recent, Seen, Shelf, Source,
    Watching, now,
};
use rusqlite::{Connection, OptionalExtension, Result, TransactionBehavior, params};
use std::path::Path;

mod portable;

/// The source of the videos the user opens one by one; every other source
/// is a folder or a server the user adds.
const OPENED: i64 = 1;

/// The schema, one step per version; a database at version `n` has run the
/// first `n` steps.
const MIGRATIONS: &[&str] = &[
    // 1: sources, addresses, files and episodes; progress, watched, recents.
    "
    CREATE TABLE source (
        id INTEGER PRIMARY KEY,
        kind TEXT NOT NULL,
        name TEXT NOT NULL,
        location TEXT NOT NULL,
        added_at INTEGER NOT NULL
    ) STRICT;
    INSERT INTO source (id, kind, name, location, added_at) VALUES (1, 'opened', '', '', 0);

    CREATE TABLE anime (
        id INTEGER PRIMARY KEY,
        dandanplay_id INTEGER UNIQUE,
        bangumi_id INTEGER UNIQUE,
        title TEXT NOT NULL,
        image TEXT NOT NULL DEFAULT '',
        updated_at INTEGER NOT NULL DEFAULT 0
    ) STRICT;

    CREATE TABLE episode (
        id INTEGER PRIMARY KEY,
        anime_id INTEGER NOT NULL REFERENCES anime (id) ON DELETE CASCADE,
        number INTEGER,
        dandanplay_id INTEGER UNIQUE,
        bangumi_id INTEGER UNIQUE,
        title TEXT NOT NULL DEFAULT ''
    ) STRICT;

    CREATE TABLE file (
        id INTEGER PRIMARY KEY,
        md5 TEXT,
        size INTEGER,
        server_item TEXT UNIQUE,
        episode_id INTEGER REFERENCES episode (id) ON DELETE SET NULL,
        matched_by TEXT,
        UNIQUE (md5, size)
    ) STRICT;

    CREATE TABLE address (
        source_id INTEGER NOT NULL REFERENCES source (id) ON DELETE CASCADE,
        path TEXT NOT NULL,
        file_id INTEGER REFERENCES file (id) ON DELETE SET NULL,
        size INTEGER,
        modified INTEGER,
        seen_at INTEGER NOT NULL,
        PRIMARY KEY (source_id, path)
    ) STRICT;

    CREATE TABLE progress (
        file_id INTEGER PRIMARY KEY REFERENCES file (id) ON DELETE CASCADE,
        position REAL NOT NULL,
        duration REAL NOT NULL,
        danmaku_offset REAL NOT NULL DEFAULT 0,
        updated_at INTEGER NOT NULL
    ) STRICT;

    CREATE TABLE watched (
        episode_id INTEGER PRIMARY KEY REFERENCES episode (id) ON DELETE CASCADE,
        at INTEGER NOT NULL,
        manual INTEGER NOT NULL
    ) STRICT;

    CREATE TABLE recent (
        source_id INTEGER NOT NULL,
        path TEXT NOT NULL,
        title TEXT NOT NULL,
        still TEXT,
        position REAL NOT NULL,
        duration REAL NOT NULL,
        played_at INTEGER NOT NULL,
        PRIMARY KEY (source_id, path),
        FOREIGN KEY (source_id, path) REFERENCES address (source_id, path) ON DELETE CASCADE
    ) STRICT;
    CREATE INDEX recent_by_time ON recent (played_at DESC);

    CREATE TABLE setting (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    ) STRICT;
    ",
    // 2: what a source holds, which says where its videos are recognized:
    // `anime` by dandanplay, `series` and `movie` by the user's own server.
    "ALTER TABLE source ADD COLUMN content TEXT NOT NULL DEFAULT 'anime';",
];

/// Tables added beside the numbered steps rather than as one: each is
/// created where it is missing, so changes that add steps of their own
/// cannot have one skipped by landing in another order. `extension` keeps
/// the data of extensions such as plugins, any JSON by an id, which the
/// account sync carries.
const BESIDE: &str = "
    -- Where the user is with an anime (the anime module's word for it)
    -- and the score they gave it, 1 to 10 or 0.
    CREATE TABLE IF NOT EXISTS anime_mark (
        anime_id INTEGER PRIMARY KEY REFERENCES anime (id) ON DELETE CASCADE,
        status TEXT,
        rating INTEGER NOT NULL DEFAULT 0,
        updated_at INTEGER NOT NULL
    ) STRICT;
    CREATE TABLE IF NOT EXISTS extension (
        id TEXT PRIMARY KEY,
        value TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    ) STRICT;
";

/// One connection to the app's database.
pub struct Db {
    conn: Connection,
}

/// Storage is best effort: a failure is reported and the app carries on
/// with what it has in memory.
fn logged<T>(result: Result<T>, what: &str) -> Option<T> {
    result
        .map_err(|error| eprintln!("erika-store: {what}: {error}"))
        .ok()
}

impl Db {
    /// The database in the app's data folder, or one in memory where the
    /// app has none (tests) or it cannot be opened.
    pub fn open() -> Db {
        match bern::storage::data_dir() {
            Some(dir) => {
                let path = dir.join("erika.db");
                logged(Self::at(&path), &format!("open {}", path.display()))
                    .unwrap_or_else(Self::in_memory)
            }
            None => Self::in_memory(),
        }
    }

    /// The database at `path`, created and brought up to date.
    pub fn at(path: &Path) -> Result<Db> {
        let conn = Connection::open(path)?;
        // Other engines of the app hold connections to the same file.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::ready(conn)
    }

    /// A database of its own in memory.
    pub fn in_memory() -> Db {
        Connection::open_in_memory()
            .and_then(Self::ready)
            .expect("an in-memory database opens")
    }

    fn ready(conn: Connection) -> Result<Db> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let mut db = Db { conn };
        db.migrate()?;
        db.conn.execute_batch(BESIDE)?;
        Ok(db)
    }

    /// Run the schema steps this database has not run, in one transaction,
    /// so engines opening it together run each step once.
    fn migrate(&mut self) -> Result<()> {
        let transaction = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = transaction.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        for step in MIGRATIONS.iter().skip(version as usize) {
            transaction.execute_batch(step)?;
        }
        if (version as usize) < MIGRATIONS.len() {
            transaction.pragma_update(None, "user_version", MIGRATIONS.len() as i64)?;
        }
        transaction.commit()
    }

    /// The file with `fingerprint`, added when new.
    fn file(&self, fingerprint: &Fingerprint) -> Result<FileId> {
        self.conn.execute(
            "INSERT INTO file (md5, size) VALUES (?1, ?2) ON CONFLICT (md5, size) DO NOTHING",
            params![fingerprint.md5, fingerprint.size as i64],
        )?;
        self.conn.query_row(
            "SELECT id FROM file WHERE md5 = ?1 AND size = ?2",
            params![fingerprint.md5, fingerprint.size as i64],
            |row| row.get(0),
        )
    }

    /// Note that the video at `url` was opened directly, and which file it
    /// is when its fingerprint is known, or when a source holds it there (a
    /// stream a shared device serves cannot be read for its fingerprint).
    /// Returns that file.
    pub fn opened(&self, url: &str, fingerprint: Option<&Fingerprint>) -> Option<FileId> {
        logged(
            (|| {
                let file = match fingerprint {
                    Some(fingerprint) => Some(self.file(fingerprint)?),
                    None => self
                        .conn
                        .query_row(
                            "SELECT file_id FROM address
                             WHERE path = ?1 AND source_id != ?2 AND file_id IS NOT NULL
                             ORDER BY seen_at DESC LIMIT 1",
                            params![url, OPENED],
                            |row| row.get(0),
                        )
                        .optional()?,
                };
                self.conn.execute(
                    "INSERT INTO address (source_id, path, file_id, size, seen_at) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT (source_id, path) DO UPDATE SET
                         file_id = coalesce(excluded.file_id, file_id),
                         size = coalesce(excluded.size, size),
                         seen_at = excluded.seen_at",
                    params![
                        OPENED,
                        url,
                        file,
                        fingerprint.map(|fingerprint| fingerprint.size as i64),
                        now()
                    ],
                )?;
                Ok(file)
            })(),
            "remember an opened video",
        )
        .flatten()
    }

    /// Which file the video at `url` is, wherever it was seen.
    pub fn address_file(&self, url: &str) -> Option<FileId> {
        logged(
            self.conn
                .query_row(
                    "SELECT file_id FROM address
                     WHERE path = ?1 AND file_id IS NOT NULL
                     ORDER BY seen_at DESC LIMIT 1",
                    [url],
                    |row| row.get(0),
                )
                .optional(),
            "find an address's file",
        )
        .flatten()
    }

    /// Where file `file` stopped and how long it is, in seconds.
    pub fn progress(&self, file: FileId) -> Option<(f64, f64)> {
        logged(
            self.conn
                .query_row(
                    "SELECT position, duration FROM progress WHERE file_id = ?1",
                    [file],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional(),
            "read progress",
        )
        .flatten()
    }

    pub fn save_progress(&self, file: FileId, position: f64, duration: f64) {
        logged(
            self.conn.execute(
                "INSERT INTO progress (file_id, position, duration, updated_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (file_id) DO UPDATE SET
                     position = excluded.position,
                     duration = excluded.duration,
                     updated_at = excluded.updated_at",
                params![file, position, duration, now()],
            ),
            "save progress",
        );
    }

    /// Put the video at `url`, opened directly, at the top of the recent
    /// ones, with where it stopped and its picture.
    pub fn remember(
        &self,
        url: &str,
        title: &str,
        still: Option<&str>,
        position: f64,
        duration: f64,
    ) {
        logged(
            (|| {
                self.conn.execute(
                    "INSERT INTO address (source_id, path, seen_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT (source_id, path) DO UPDATE SET seen_at = excluded.seen_at",
                    params![OPENED, url, now()],
                )?;
                self.conn.execute(
                    // Later than every other, even within one second.
                    "INSERT INTO recent (source_id, path, title, still, position, duration, played_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, max(?7, (SELECT coalesce(max(played_at), 0) + 1 FROM recent)))
                     ON CONFLICT (source_id, path) DO UPDATE SET
                         title = excluded.title,
                         still = coalesce(excluded.still, still),
                         position = excluded.position,
                         duration = excluded.duration,
                         played_at = excluded.played_at",
                    params![OPENED, url, title, still, position, duration, now()],
                )
            })(),
            "remember a recent video",
        );
    }

    /// The `limit` videos played last, newest first.
    pub fn recents(&self, limit: usize) -> Vec<Recent> {
        logged(
            (|| {
                let mut statement = self.conn.prepare(
                    "SELECT path, title, still, position, duration FROM recent
                     ORDER BY played_at DESC LIMIT ?1",
                )?;
                let rows = statement.query_map([limit as i64], |row| {
                    Ok(Recent {
                        url: row.get(0)?,
                        title: row.get(1)?,
                        still: row.get(2)?,
                        position: row.get(3)?,
                        duration: row.get(4)?,
                    })
                })?;
                rows.collect()
            })(),
            "read recent videos",
        )
        .unwrap_or_default()
    }

    /// Every picture a recent video names.
    pub fn stills(&self) -> Vec<String> {
        logged(
            (|| {
                let mut statement = self
                    .conn
                    .prepare("SELECT still FROM recent WHERE still IS NOT NULL")?;
                let rows = statement.query_map([], |row| row.get(0))?;
                rows.collect()
            })(),
            "read pictures",
        )
        .unwrap_or_default()
    }

    /// A small value the app keeps, such as the signed-in account's name.
    pub fn setting(&self, key: &str) -> Option<String> {
        logged(
            self.conn
                .query_row("SELECT value FROM setting WHERE key = ?1", [key], |row| {
                    row.get(0)
                })
                .optional(),
            "read a setting",
        )
        .flatten()
    }

    /// Keep `value` as `key`, or forget `key` when `value` is `None`.
    pub fn set_setting(&self, key: &str, value: Option<&str>) {
        let result = match value {
            Some(value) => self.conn.execute(
                "INSERT INTO setting (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            ),
            None => self
                .conn
                .execute("DELETE FROM setting WHERE key = ?1", [key]),
        };
        logged(result, "save a setting");
    }

    /// Keep what the app knows of an anime, by its dandanplay id.
    pub fn remember_anime(&self, anime: &Anime) {
        logged(
            self.conn.execute(
                "INSERT INTO anime (dandanplay_id, title, image, updated_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (dandanplay_id) DO UPDATE SET
                     title = excluded.title,
                     image = excluded.image,
                     updated_at = excluded.updated_at",
                params![anime.dandanplay_id, anime.title, anime.image, now()],
            ),
            "remember an anime",
        );
    }

    /// File `file` is `episode` of `anime`; `manual` when the user said so.
    /// Automatic matching leaves a file the user matched or unlinked alone.
    pub fn link_episode(&self, file: FileId, anime: &Anime, episode: &Episode, manual: bool) {
        logged(
            self.link(file, anime, episode, manual),
            "link a file to its episode",
        );
    }

    fn link(&self, file: FileId, anime: &Anime, episode: &Episode, manual: bool) -> Result<usize> {
        self.conn.execute(
            "INSERT INTO anime (dandanplay_id, title, image, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (dandanplay_id) DO UPDATE SET
                 title = excluded.title,
                 image = CASE WHEN excluded.image = '' THEN image ELSE excluded.image END,
                 updated_at = excluded.updated_at",
            params![anime.dandanplay_id, anime.title, anime.image, now()],
        )?;
        self.conn.execute(
            "INSERT INTO episode (anime_id, number, dandanplay_id, title)
             VALUES ((SELECT id FROM anime WHERE dandanplay_id = ?1), ?2, ?3, ?4)
             ON CONFLICT (dandanplay_id) DO UPDATE SET
                 anime_id = excluded.anime_id,
                 number = excluded.number,
                 title = excluded.title",
            params![
                anime.dandanplay_id,
                episode.number,
                episode.dandanplay_id,
                episode.title
            ],
        )?;
        self.conn.execute(
            "UPDATE file SET
                 episode_id = (SELECT id FROM episode WHERE dandanplay_id = ?2),
                 matched_by = ?3
             WHERE id = ?1 AND (?3 = 'manual' OR matched_by IS NULL OR matched_by = 'auto')",
            params![
                file,
                episode.dandanplay_id,
                if manual { "manual" } else { "auto" }
            ],
        )
    }

    /// Which episode of which anime file `file` is, once it was matched.
    pub fn file_episode(&self, file: FileId) -> Option<(Anime, Episode)> {
        logged(
            self.conn
                .query_row(
                    "SELECT a.dandanplay_id, a.title, a.image, e.dandanplay_id, e.number, e.title
                     FROM file f JOIN episode e ON e.id = f.episode_id JOIN anime a ON a.id = e.anime_id
                     WHERE f.id = ?1",
                    [file],
                    |row| {
                        Ok((
                            Anime { dandanplay_id: row.get(0)?, title: row.get(1)?, image: row.get(2)? },
                            Episode { dandanplay_id: row.get(3)?, number: row.get(4)?, title: row.get(5)? },
                        ))
                    },
                )
                .optional(),
            "read a file's episode",
        )
        .flatten()
    }

    /// The files the user has of anime `dandanplay_id`, one per episode:
    /// the address seen last, with the file's progress.
    pub fn episode_files(&self, dandanplay_id: i64) -> Vec<EpisodeFile> {
        let rows = logged(
            (|| {
                let mut statement = self.conn.prepare(
                    "SELECT e.dandanplay_id, e.number, e.title, ad.path,
                            coalesce(p.position, 0), coalesce(p.duration, 0)
                     FROM episode e
                     JOIN anime a ON a.id = e.anime_id
                     JOIN file f ON f.episode_id = e.id
                     JOIN address ad ON ad.file_id = f.id
                     LEFT JOIN progress p ON p.file_id = f.id
                     WHERE a.dandanplay_id = ?1
                     ORDER BY e.number, ad.seen_at DESC",
                )?;
                let rows = statement.query_map([dandanplay_id], |row| {
                    Ok(EpisodeFile {
                        episode: Episode {
                            dandanplay_id: row.get(0)?,
                            number: row.get(1)?,
                            title: row.get(2)?,
                        },
                        url: row.get(3)?,
                        position: row.get(4)?,
                        duration: row.get(5)?,
                    })
                })?;
                rows.collect::<Result<Vec<_>>>()
            })(),
            "read an anime's files",
        )
        .unwrap_or_default();
        let mut files: Vec<EpisodeFile> = Vec::new();
        for file in rows {
            if files
                .last()
                .is_none_or(|last| last.episode.dandanplay_id != file.episode.dandanplay_id)
            {
                files.push(file);
            }
        }
        files
    }

    /// The anime being watched, the one played last first: the recent
    /// videos that are episodes, one per anime.
    pub fn watching(&self, limit: usize) -> Vec<Watching> {
        let rows = logged(
            (|| {
                let mut statement = self.conn.prepare(
                    "SELECT a.dandanplay_id, a.title, a.image, e.dandanplay_id, e.number, e.title,
                            r.path, r.still, coalesce(p.position, r.position), coalesce(p.duration, r.duration)
                     FROM recent r
                     JOIN address ad ON ad.source_id = r.source_id AND ad.path = r.path
                     JOIN file f ON f.id = ad.file_id
                     JOIN episode e ON e.id = f.episode_id
                     JOIN anime a ON a.id = e.anime_id
                     LEFT JOIN progress p ON p.file_id = f.id
                     ORDER BY r.played_at DESC",
                )?;
                let rows = statement.query_map([], |row| {
                    Ok(Watching {
                        anime: Anime { dandanplay_id: row.get(0)?, title: row.get(1)?, image: row.get(2)? },
                        episode: Episode { dandanplay_id: row.get(3)?, number: row.get(4)?, title: row.get(5)? },
                        url: row.get(6)?,
                        still: row.get(7)?,
                        position: row.get(8)?,
                        duration: row.get(9)?,
                    })
                })?;
                rows.collect::<Result<Vec<_>>>()
            })(),
            "read what is being watched",
        )
        .unwrap_or_default();
        let mut watching: Vec<Watching> = Vec::new();
        for row in rows {
            if watching.len() < limit
                && !watching
                    .iter()
                    .any(|seen| seen.anime.dandanplay_id == row.anime.dandanplay_id)
            {
                watching.push(row);
            }
        }
        watching
    }

    /// The anime the user has files of, in `source` or anywhere, the one
    /// seen last first.
    pub fn shelf(&self, source: Option<i64>) -> Vec<Shelf> {
        logged(
            (|| {
                let mut statement = self.conn.prepare(
                    "SELECT a.dandanplay_id, a.title, a.image,
                            count(DISTINCT e.id),
                            count(DISTINCT CASE WHEN p.duration > 0 AND p.position >= 0.9 * p.duration THEN e.id END),
                            min(ad.seen_at), max(ad.seen_at)
                     FROM anime a
                     JOIN episode e ON e.anime_id = a.id
                     JOIN file f ON f.episode_id = e.id
                     JOIN address ad ON ad.file_id = f.id
                     LEFT JOIN progress p ON p.file_id = f.id
                     WHERE ?1 IS NULL OR ad.source_id = ?1
                     GROUP BY a.id
                     ORDER BY max(ad.seen_at) DESC, a.id DESC",
                )?;
                let rows = statement.query_map([source], |row| {
                    Ok(Shelf {
                        anime: Anime { dandanplay_id: row.get(0)?, title: row.get(1)?, image: row.get(2)? },
                        episodes: row.get(3)?,
                        watched: row.get(4)?,
                        added: row.get(5)?,
                        seen: row.get(6)?,
                    })
                })?;
                rows.collect()
            })(),
            "read the library",
        )
        .unwrap_or_default()
    }

    /// The file with `fingerprint`, added when new.
    pub fn file_id(&self, fingerprint: &Fingerprint) -> Option<FileId> {
        logged(self.file(fingerprint), "find a file")
    }

    /// Add a source of kind `kind` at `location` holding `content`, or find
    /// the one there.
    pub fn add_source(&self, kind: &str, name: &str, location: &str, content: &str) -> Option<i64> {
        logged(
            (|| {
                let known = self
                    .conn
                    .query_row(
                        "SELECT id FROM source WHERE kind = ?1 AND location = ?2",
                        params![kind, location],
                        |row| row.get(0),
                    )
                    .optional()?;
                if let Some(id) = known {
                    return Ok(id);
                }
                self.conn.execute(
                    "INSERT INTO source (kind, name, location, added_at, content) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![kind, name, location, now(), content],
                )?;
                Ok(self.conn.last_insert_rowid())
            })(),
            "add a source",
        )
    }

    /// Source `id` is called `name` and is at `location` from now on.
    pub fn set_source(&self, id: i64, name: &str, location: &str) {
        logged(
            self.conn.execute(
                "UPDATE source SET name = ?2, location = ?3 WHERE id = ?1 AND id != ?4",
                params![id, name, location, OPENED],
            ),
            "change a source",
        );
    }

    /// Source `id` holds `content` from now on.
    pub fn set_source_content(&self, id: i64, content: &str) {
        logged(
            self.conn.execute(
                "UPDATE source SET content = ?2 WHERE id = ?1",
                params![id, content],
            ),
            "change what a source holds",
        );
    }

    /// The fingerprint of file `file`.
    pub fn fingerprint_of(&self, file: FileId) -> Option<Fingerprint> {
        logged(
            self.conn
                .query_row(
                    "SELECT md5, size FROM file WHERE id = ?1 AND md5 IS NOT NULL",
                    [file],
                    |row| {
                        Ok(Fingerprint {
                            md5: row.get(0)?,
                            size: row.get::<_, i64>(1)? as u64,
                        })
                    },
                )
                .optional(),
            "read a fingerprint",
        )
        .flatten()
    }

    /// Forget source `id` and its addresses; the files and what they were
    /// matched to stay, for the same videos elsewhere.
    pub fn remove_source(&self, id: i64) {
        logged(
            self.conn.execute(
                "DELETE FROM source WHERE id = ?1 AND id != ?2",
                params![id, OPENED],
            ),
            "remove a source",
        );
    }

    /// The sources the user added, the first added first.
    pub fn sources(&self) -> Vec<Source> {
        logged(
            (|| {
                let mut statement = self.conn.prepare(
                    "SELECT s.id, s.kind, s.name, s.location, s.content, count(ad.path),
                            count(CASE WHEN ad.path IS NOT NULL AND f.episode_id IS NULL THEN 1 END)
                     FROM source s
                     LEFT JOIN address ad ON ad.source_id = s.id
                     LEFT JOIN file f ON f.id = ad.file_id
                     WHERE s.id != ?1
                     GROUP BY s.id
                     ORDER BY s.added_at, s.id",
                )?;
                let rows = statement.query_map([OPENED], |row| {
                    Ok(Source {
                        id: row.get(0)?,
                        kind: row.get(1)?,
                        name: row.get(2)?,
                        location: row.get(3)?,
                        content: row.get(4)?,
                        files: row.get(5)?,
                        unmatched: row.get(6)?,
                    })
                })?;
                rows.collect()
            })(),
            "read the sources",
        )
        .unwrap_or_default()
    }

    /// The videos source `source` held when it was last scanned.
    pub fn seen(&self, source: i64) -> Vec<Seen> {
        logged(
            (|| {
                let mut statement = self.conn.prepare(
                    "SELECT ad.path, coalesce(ad.size, -1), coalesce(ad.modified, -1), f.md5, f.size,
                            f.episode_id IS NOT NULL, coalesce(f.matched_by IN ('manual', 'none'), 0)
                     FROM address ad LEFT JOIN file f ON f.id = ad.file_id
                     WHERE ad.source_id = ?1",
                )?;
                let rows = statement.query_map([source], |row| {
                    let md5: Option<String> = row.get(3)?;
                    let size: Option<i64> = row.get(4)?;
                    Ok(Seen {
                        url: row.get(0)?,
                        size: row.get(1)?,
                        modified: row.get(2)?,
                        fingerprint: md5.zip(size).map(|(md5, size)| Fingerprint { md5, size: size as u64 }),
                        matched: row.get(5)?,
                        decided: row.get(6)?,
                    })
                })?;
                rows.collect()
            })(),
            "read what a source held",
        )
        .unwrap_or_default()
    }

    /// The video at `url` in `source` is `size` bytes, changed at
    /// `modified`, and is the file with `fingerprint` when it could be
    /// read. Returns that file.
    pub fn saw(
        &self,
        source: i64,
        url: &str,
        size: i64,
        modified: i64,
        fingerprint: Option<&Fingerprint>,
    ) -> Option<FileId> {
        logged(
            (|| {
                let file = fingerprint.map(|fingerprint| self.file(fingerprint)).transpose()?;
                self.conn.execute(
                    "INSERT INTO address (source_id, path, file_id, size, modified, seen_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT (source_id, path) DO UPDATE SET
                         file_id = excluded.file_id,
                         size = excluded.size,
                         modified = excluded.modified,
                         seen_at = excluded.seen_at",
                    params![source, url, file, size, modified, now()],
                )?;
                Ok(file)
            })(),
            "note a scanned video",
        )
        .flatten()
    }

    /// Source `source` no longer holds the videos at `urls`.
    pub fn forget(&self, source: i64, urls: &[String]) {
        logged(
            (|| {
                let transaction = self.conn.unchecked_transaction()?;
                for url in urls {
                    transaction.execute(
                        "DELETE FROM address WHERE source_id = ?1 AND path = ?2",
                        params![source, url],
                    )?;
                }
                transaction.commit()
            })(),
            "forget videos gone from a source",
        );
    }

    /// The videos of `source`, or of every source the user added, by
    /// address, with what each was matched to.
    pub fn entries(&self, source: Option<i64>) -> Vec<Entry> {
        logged(
            (|| {
                let mut statement = self.conn.prepare(
                    "SELECT ad.rowid, ad.source_id, ad.path, ad.file_id, a.dandanplay_id, a.title, a.image,
                            e.dandanplay_id, e.number, e.title, coalesce(f.matched_by = 'manual', 0)
                     FROM address ad
                     LEFT JOIN file f ON f.id = ad.file_id
                     LEFT JOIN episode e ON e.id = f.episode_id
                     LEFT JOIN anime a ON a.id = e.anime_id
                     WHERE ad.source_id != ?1 AND (?2 IS NULL OR ad.source_id = ?2)
                     ORDER BY ad.source_id, ad.path",
                )?;
                let rows = statement.query_map(params![OPENED, source], |row| {
                    let anime: Option<i64> = row.get(4)?;
                    let matched = match anime {
                        Some(dandanplay_id) => Some((
                            Anime { dandanplay_id, title: row.get(5)?, image: row.get(6)? },
                            Episode { dandanplay_id: row.get(7)?, number: row.get::<_, Option<i64>>(8)?.unwrap_or(0), title: row.get(9)? },
                        )),
                        None => None,
                    };
                    Ok(Entry { key: row.get(0)?, source: row.get(1)?, url: row.get(2)?, file: row.get(3)?, matched, manual: row.get(10)? })
                })?;
                rows.collect()
            })(),
            "read a source's videos",
        )
        .unwrap_or_default()
    }

    /// Files `links` are those episodes of `anime`, as the user said, all
    /// of them or, when one cannot be written, none.
    pub fn link_files(&self, anime: &Anime, links: &[(FileId, Episode)]) -> bool {
        logged(
            (|| {
                let transaction = self.conn.unchecked_transaction()?;
                for (file, episode) in links {
                    self.link(*file, anime, episode, true)?;
                }
                transaction.commit()
            })(),
            "match files",
        )
        .is_some()
    }

    /// The user says file `file` is no episode they know; scans leave it so.
    pub fn unlink(&self, file: FileId) {
        logged(
            self.conn.execute(
                "UPDATE file SET episode_id = NULL, matched_by = 'none' WHERE id = ?1",
                [file],
            ),
            "unlink a file",
        );
    }

    pub fn anime(&self, dandanplay_id: i64) -> Option<Anime> {
        logged(
            self.conn
                .query_row(
                    "SELECT dandanplay_id, title, image FROM anime WHERE dandanplay_id = ?1",
                    [dandanplay_id],
                    |row| {
                        Ok(Anime {
                            dandanplay_id: row.get(0)?,
                            title: row.get(1)?,
                            image: row.get(2)?,
                        })
                    },
                )
                .optional(),
            "read an anime",
        )
        .flatten()
    }

    /// What the user marked anime `dandanplay_id` as, and its score.
    pub fn mark(&self, dandanplay_id: i64) -> Mark {
        logged(
            self.conn
                .query_row(
                    "SELECT m.status, m.rating FROM anime_mark m JOIN anime a ON a.id = m.anime_id
                     WHERE a.dandanplay_id = ?1",
                    [dandanplay_id],
                    |row| {
                        Ok(Mark {
                            status: row.get(0)?,
                            rating: row.get(1)?,
                        })
                    },
                )
                .optional(),
            "read an anime's mark",
        )
        .flatten()
        .unwrap_or_default()
    }

    /// Keep `mark` as what the user marked `anime` as.
    pub fn set_mark(&self, anime: &Anime, mark: &Mark) {
        logged(
            (|| {
                self.keep_anime(anime)?;
                self.conn.execute(
                    "INSERT INTO anime_mark (anime_id, status, rating, updated_at)
                     VALUES ((SELECT id FROM anime WHERE dandanplay_id = ?1), ?2, ?3, ?4)
                     ON CONFLICT (anime_id) DO UPDATE SET
                         status = excluded.status,
                         rating = excluded.rating,
                         updated_at = excluded.updated_at",
                    params![anime.dandanplay_id, mark.status, mark.rating, now()],
                )
            })(),
            "save an anime's mark",
        );
    }

    /// The numbers of the episodes of anime `dandanplay_id` marked watched.
    pub fn watched_numbers(&self, dandanplay_id: i64) -> Vec<i64> {
        logged(
            (|| {
                let mut statement = self.conn.prepare(
                    "SELECT DISTINCT e.number FROM watched w
                     JOIN episode e ON e.id = w.episode_id
                     JOIN anime a ON a.id = e.anime_id
                     WHERE a.dandanplay_id = ?1 AND e.number IS NOT NULL
                     ORDER BY e.number",
                )?;
                let rows = statement.query_map([dandanplay_id], |row| row.get(0))?;
                rows.collect()
            })(),
            "read the watched episodes",
        )
        .unwrap_or_default()
    }

    /// Mark `episode` of `anime` watched, or not; `manual` when the user
    /// marked it. An episode without a file gets a row of its own, by its
    /// number, and by its id when it has one.
    pub fn set_watched(&self, anime: &Anime, episode: &Episode, watched: bool, manual: bool) {
        logged(
            self.mark_watched(anime, episode, watched, manual),
            "mark an episode watched",
        );
    }

    fn mark_watched(
        &self,
        anime: &Anime,
        episode: &Episode,
        watched: bool,
        manual: bool,
    ) -> Result<()> {
        self.keep_anime(anime)?;
        let anime_row: i64 = self.conn.query_row(
            "SELECT id FROM anime WHERE dandanplay_id = ?1",
            [anime.dandanplay_id],
            |row| row.get(0),
        )?;
        if !watched {
            self.conn.execute(
                "DELETE FROM watched WHERE episode_id IN
                     (SELECT id FROM episode WHERE anime_id = ?1 AND number = ?2)",
                params![anime_row, episode.number],
            )?;
            return Ok(());
        }
        let id = (episode.dandanplay_id != 0).then_some(episode.dandanplay_id);
        let find = |conn: &Connection| {
            conn.query_row(
                "SELECT id FROM episode
                 WHERE (anime_id = ?1 AND number = ?2) OR (?3 IS NOT NULL AND dandanplay_id = ?3)
                 ORDER BY dandanplay_id IS NULL, id LIMIT 1",
                params![anime_row, episode.number, id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
        };
        let row = match find(&self.conn)? {
            Some(row) => row,
            None => {
                self.conn.execute(
                    "INSERT INTO episode (anime_id, number, dandanplay_id, title) VALUES (?1, ?2, ?3, ?4)",
                    params![anime_row, episode.number, id, episode.title],
                )?;
                self.conn.last_insert_rowid()
            }
        };
        self.conn.execute(
            "INSERT INTO watched (episode_id, at, manual) VALUES (?1, ?2, ?3)
             ON CONFLICT (episode_id) DO UPDATE SET manual = max(manual, excluded.manual)",
            params![row, now(), manual],
        )?;
        Ok(())
    }

    /// The anime's row, its picture kept when `anime` has none.
    fn keep_anime(&self, anime: &Anime) -> Result<usize> {
        self.conn.execute(
            "INSERT INTO anime (dandanplay_id, title, image, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (dandanplay_id) DO UPDATE SET
                 title = CASE WHEN excluded.title = '' THEN title ELSE excluded.title END,
                 image = CASE WHEN excluded.image = '' THEN image ELSE excluded.image END",
            params![anime.dandanplay_id, anime.title, anime.image, now()],
        )
    }
}
