//! Reading and writing what the account sync carries, row by row.

use super::{Db, OPENED, logged};
use crate::now;
use crate::portable::{Anime, Episode, FileKey, Link, Mark, Progress, Recent, Source, Watched};
use rusqlite::{OptionalExtension, Result, Row, params};

fn file_key(md5: Option<String>, size: Option<i64>, item: Option<String>) -> Option<FileKey> {
    match (md5, size, item) {
        (Some(md5), Some(size), _) => Some(FileKey::Hash {
            md5,
            size: size as u64,
        }),
        (_, _, Some(item)) => Some(FileKey::Item(item)),
        _ => None,
    }
}

fn key_of(row: &Row, at: usize) -> Result<Option<FileKey>> {
    Ok(file_key(row.get(at)?, row.get(at + 1)?, row.get(at + 2)?))
}

impl Db {
    fn rows<T>(&self, sql: &str, what: &str, map: impl FnMut(&Row) -> Result<T>) -> Vec<T> {
        logged(
            (|| {
                let mut statement = self.conn.prepare(sql)?;
                let rows = statement.query_map([], map)?;
                rows.collect::<Result<Vec<T>>>()
            })(),
            what,
        )
        .unwrap_or_default()
    }

    /// Run `work` in one transaction, so a sync's changes land together
    /// and fast.
    pub fn transaction<T>(&self, work: impl FnOnce() -> T) -> T {
        let begun = logged(self.conn.execute_batch("BEGIN"), "begin a transaction").is_some();
        let result = work();
        if begun {
            logged(self.conn.execute_batch("COMMIT"), "commit a transaction");
        }
        result
    }

    /// Every small value the app keeps.
    pub fn settings(&self) -> Vec<(String, String)> {
        self.rows(
            "SELECT key, value FROM setting ORDER BY key",
            "read the settings",
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
    }

    /// The file `key` names, added when new.
    fn file_of(&self, key: &FileKey) -> Result<i64> {
        match key {
            FileKey::Hash { md5, size } => self.file(&crate::Fingerprint {
                md5: md5.clone(),
                size: *size,
            }),
            FileKey::Item(item) => {
                self.conn.execute(
                    "INSERT INTO file (server_item) VALUES (?1) ON CONFLICT (server_item) DO NOTHING",
                    [item],
                )?;
                self.conn.query_row(
                    "SELECT id FROM file WHERE server_item = ?1",
                    [item],
                    |row| row.get(0),
                )
            }
        }
    }

    /// The sources the user added, as another device adds them.
    pub fn portable_sources(&self) -> Vec<Source> {
        self.rows(
            "SELECT kind, name, location, content, added_at FROM source WHERE id != 1 ORDER BY added_at, id",
            "read the sources",
            |row| {
                Ok(Source {
                    kind: row.get(0)?,
                    name: row.get(1)?,
                    location: row.get(2)?,
                    content: row.get(3)?,
                    added: row.get(4)?,
                })
            },
        )
    }

    /// Add `source`, or bring the one at its place up to date. Returns its
    /// id, and whether it is new.
    pub fn put_source(&self, source: &Source) -> Option<(i64, bool)> {
        logged(
            (|| {
                let known: Option<i64> = self
                    .conn
                    .query_row(
                        "SELECT id FROM source WHERE kind = ?1 AND location = ?2 AND id != ?3",
                        params![source.kind, source.location, OPENED],
                        |row| row.get(0),
                    )
                    .optional()?;
                if let Some(id) = known {
                    self.conn.execute(
                        "UPDATE source SET name = ?2, content = ?3 WHERE id = ?1",
                        params![id, source.name, source.content],
                    )?;
                    return Ok((id, false));
                }
                self.conn.execute(
                    "INSERT INTO source (kind, name, location, added_at, content) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![source.kind, source.name, source.location, source.added, source.content],
                )?;
                Ok((self.conn.last_insert_rowid(), true))
            })(),
            "add a synced source",
        )
    }

    /// Forget the source of `kind` at `location`.
    pub fn remove_source_at(&self, kind: &str, location: &str) {
        logged(
            self.conn.execute(
                "DELETE FROM source WHERE kind = ?1 AND location = ?2 AND id != ?3",
                params![kind, location, OPENED],
            ),
            "remove a synced source",
        );
    }

    pub fn portable_anime(&self) -> Vec<Anime> {
        self.rows(
            "SELECT dandanplay_id, bangumi_id, title, image FROM anime WHERE dandanplay_id IS NOT NULL",
            "read the anime",
            |row| {
                Ok(Anime {
                    dandanplay_id: row.get(0)?,
                    bangumi_id: row.get(1)?,
                    title: row.get(2)?,
                    image: row.get(3)?,
                })
            },
        )
    }

    pub fn put_anime(&self, anime: &Anime) {
        logged(
            (|| {
                self.conn.execute(
                    "INSERT INTO anime (dandanplay_id, title, image, updated_at) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (dandanplay_id) DO UPDATE SET
                         title = excluded.title,
                         image = CASE WHEN excluded.image = '' THEN image ELSE excluded.image END",
                    params![anime.dandanplay_id, anime.title, anime.image, now()],
                )?;
                // Another anime may hold the Bangumi id; it keeps it.
                self.conn.execute(
                    "UPDATE anime SET bangumi_id = ?2 WHERE dandanplay_id = ?1 AND ?2 IS NOT NULL
                     AND NOT EXISTS (SELECT 1 FROM anime WHERE bangumi_id = ?2 AND dandanplay_id != ?1)",
                    params![anime.dandanplay_id, anime.bangumi_id],
                )
            })(),
            "add a synced anime",
        );
    }

    pub fn portable_episodes(&self) -> Vec<Episode> {
        self.rows(
            "SELECT e.dandanplay_id, a.dandanplay_id, e.number, e.bangumi_id, e.title
             FROM episode e JOIN anime a ON a.id = e.anime_id
             WHERE e.dandanplay_id IS NOT NULL AND a.dandanplay_id IS NOT NULL",
            "read the episodes",
            |row| {
                Ok(Episode {
                    dandanplay_id: row.get(0)?,
                    anime: row.get(1)?,
                    number: row.get(2)?,
                    bangumi_id: row.get(3)?,
                    title: row.get(4)?,
                })
            },
        )
    }

    /// Add `episode` under its anime, which must be known.
    pub fn put_episode(&self, episode: &Episode) {
        logged(
            (|| {
                self.conn.execute(
                    "INSERT INTO episode (anime_id, number, dandanplay_id, title)
                     SELECT id, ?2, ?3, ?4 FROM anime WHERE dandanplay_id = ?1
                     ON CONFLICT (dandanplay_id) DO UPDATE SET
                         anime_id = excluded.anime_id,
                         number = excluded.number,
                         title = excluded.title",
                    params![
                        episode.anime,
                        episode.number,
                        episode.dandanplay_id,
                        episode.title
                    ],
                )?;
                self.conn.execute(
                    "UPDATE episode SET bangumi_id = ?2 WHERE dandanplay_id = ?1 AND ?2 IS NOT NULL
                     AND NOT EXISTS (SELECT 1 FROM episode WHERE bangumi_id = ?2 AND dandanplay_id != ?1)",
                    params![episode.dandanplay_id, episode.bangumi_id],
                )
            })(),
            "add a synced episode",
        );
    }

    /// Which episode each file is, where it was matched or decided.
    pub fn portable_links(&self) -> Vec<Link> {
        self.rows(
            "SELECT f.md5, f.size, f.server_item, e.dandanplay_id, f.matched_by
             FROM file f LEFT JOIN episode e ON e.id = f.episode_id
             WHERE f.matched_by IS NOT NULL OR f.episode_id IS NOT NULL",
            "read what files are",
            |row| {
                Ok(key_of(row, 0)?.map(|file| Link {
                    file,
                    episode: row.get(3).ok().flatten(),
                    matched_by: row.get(4).ok().flatten(),
                }))
            },
        )
        .into_iter()
        .flatten()
        .collect()
    }

    pub fn put_link(&self, link: &Link) {
        logged(
            (|| {
                let file = self.file_of(&link.file)?;
                self.conn.execute(
                    "UPDATE file SET episode_id = (SELECT id FROM episode WHERE dandanplay_id = ?2), matched_by = ?3
                     WHERE id = ?1",
                    params![file, link.episode, link.matched_by],
                )
            })(),
            "link a synced file",
        );
    }

    /// The file `key` is no longer known to be an episode.
    pub fn remove_link(&self, key: &FileKey) {
        logged(
            (|| {
                let file = self.file_of(key)?;
                self.conn.execute(
                    "UPDATE file SET episode_id = NULL, matched_by = NULL WHERE id = ?1",
                    [file],
                )
            })(),
            "unlink a synced file",
        );
    }

    pub fn portable_progress(&self) -> Vec<Progress> {
        self.rows(
            "SELECT f.md5, f.size, f.server_item, p.position, p.duration, p.danmaku_offset, p.updated_at
             FROM progress p JOIN file f ON f.id = p.file_id",
            "read the progress",
            |row| {
                Ok(key_of(row, 0)?.map(|file| Progress {
                    file,
                    position: row.get(3).unwrap_or(0.0),
                    duration: row.get(4).unwrap_or(0.0),
                    danmaku_offset: row.get(5).unwrap_or(0.0),
                    at: row.get(6).unwrap_or(0),
                }))
            },
        )
        .into_iter()
        .flatten()
        .collect()
    }

    pub fn put_progress(&self, progress: &Progress) {
        logged(
            (|| {
                let file = self.file_of(&progress.file)?;
                self.conn.execute(
                    "INSERT INTO progress (file_id, position, duration, danmaku_offset, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT (file_id) DO UPDATE SET
                         position = excluded.position,
                         duration = excluded.duration,
                         danmaku_offset = excluded.danmaku_offset,
                         updated_at = excluded.updated_at",
                    params![
                        file,
                        progress.position,
                        progress.duration,
                        progress.danmaku_offset,
                        progress.at
                    ],
                )
            })(),
            "save synced progress",
        );
    }

    pub fn remove_progress(&self, key: &FileKey) {
        logged(
            (|| {
                let file = self.file_of(key)?;
                self.conn
                    .execute("DELETE FROM progress WHERE file_id = ?1", [file])
            })(),
            "forget synced progress",
        );
    }

    pub fn portable_watched(&self) -> Vec<Watched> {
        self.rows(
            "SELECT e.dandanplay_id, w.at, w.manual FROM watched w JOIN episode e ON e.id = w.episode_id
             WHERE e.dandanplay_id IS NOT NULL",
            "read the watched episodes",
            |row| {
                Ok(Watched {
                    episode: row.get(0)?,
                    at: row.get(1)?,
                    manual: row.get::<_, i64>(2)? != 0,
                })
            },
        )
    }

    pub fn put_watched(&self, watched: &Watched) {
        logged(
            self.conn.execute(
                "INSERT INTO watched (episode_id, at, manual)
                 SELECT id, ?2, ?3 FROM episode WHERE dandanplay_id = ?1
                 ON CONFLICT (episode_id) DO UPDATE SET at = excluded.at, manual = excluded.manual",
                params![watched.episode, watched.at, i64::from(watched.manual)],
            ),
            "mark a synced episode watched",
        );
    }

    pub fn remove_watched(&self, episode: i64) {
        logged(
            self.conn.execute(
                "DELETE FROM watched WHERE episode_id = (SELECT id FROM episode WHERE dandanplay_id = ?1)",
                [episode],
            ),
            "unmark a synced episode",
        );
    }

    /// The videos opened directly, with the files they were.
    pub fn portable_recents(&self) -> Vec<Recent> {
        self.rows(
            "SELECT r.path, r.title, r.position, r.duration, r.played_at, f.md5, f.size, f.server_item
             FROM recent r
             LEFT JOIN address ad ON ad.source_id = r.source_id AND ad.path = r.path
             LEFT JOIN file f ON f.id = ad.file_id
             WHERE r.source_id = 1
             ORDER BY r.played_at",
            "read the recent videos",
            |row| {
                Ok(Recent {
                    url: row.get(0)?,
                    title: row.get(1)?,
                    position: row.get(2)?,
                    duration: row.get(3)?,
                    at: row.get(4)?,
                    file: key_of(row, 5)?,
                })
            },
        )
    }

    pub fn put_recent(&self, recent: &Recent) {
        logged(
            (|| {
                let file = recent
                    .file
                    .as_ref()
                    .map(|key| self.file_of(key))
                    .transpose()?;
                self.conn.execute(
                    "INSERT INTO address (source_id, path, file_id, seen_at) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (source_id, path) DO UPDATE SET file_id = coalesce(excluded.file_id, file_id)",
                    params![OPENED, recent.url, file, recent.at],
                )?;
                self.conn.execute(
                    "INSERT INTO recent (source_id, path, title, position, duration, played_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT (source_id, path) DO UPDATE SET
                         title = excluded.title,
                         position = excluded.position,
                         duration = excluded.duration,
                         played_at = excluded.played_at",
                    params![
                        OPENED,
                        recent.url,
                        recent.title,
                        recent.position,
                        recent.duration,
                        recent.at
                    ],
                )
            })(),
            "add a synced recent video",
        );
    }

    pub fn remove_recent(&self, url: &str) {
        logged(
            self.conn.execute(
                "DELETE FROM recent WHERE source_id = ?1 AND path = ?2",
                params![OPENED, url],
            ),
            "forget a synced recent video",
        );
    }

    /// What the user thinks of each anime.
    pub fn anime_marks(&self) -> Vec<Mark> {
        self.rows(
            "SELECT dandanplay_id, status, rating, updated_at FROM anime_mark",
            "read the anime marks",
            |row| {
                Ok(Mark {
                    anime: row.get(0)?,
                    status: row.get(1)?,
                    rating: row.get(2)?,
                    at: row.get(3)?,
                })
            },
        )
    }

    pub fn set_anime_mark(&self, mark: &Mark) {
        logged(
            self.conn.execute(
                "INSERT INTO anime_mark (dandanplay_id, status, rating, updated_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (dandanplay_id) DO UPDATE SET
                     status = excluded.status,
                     rating = excluded.rating,
                     updated_at = excluded.updated_at",
                params![mark.anime, mark.status, mark.rating, mark.at],
            ),
            "mark an anime",
        );
    }

    pub fn remove_anime_mark(&self, anime: i64) {
        logged(
            self.conn
                .execute("DELETE FROM anime_mark WHERE dandanplay_id = ?1", [anime]),
            "unmark an anime",
        );
    }

    /// Every extension's data, by its id: JSON text.
    pub fn extensions(&self) -> Vec<(String, String)> {
        self.rows(
            "SELECT id, value FROM extension ORDER BY id",
            "read the extensions' data",
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
    }

    /// Extension `id`'s data, JSON text.
    pub fn extension(&self, id: &str) -> Option<String> {
        logged(
            self.conn
                .query_row("SELECT value FROM extension WHERE id = ?1", [id], |row| {
                    row.get(0)
                })
                .optional(),
            "read an extension's data",
        )
        .flatten()
    }

    /// Keep `json` as extension `id`'s data, or forget it with `None`. The
    /// account sync carries it to the user's other devices.
    pub fn set_extension(&self, id: &str, json: Option<&str>) {
        let result = match json {
            Some(json) => self.conn.execute(
                "INSERT INTO extension (id, value, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (id) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                params![id, json, now()],
            ),
            None => self.conn.execute("DELETE FROM extension WHERE id = ?1", [id]),
        };
        logged(result, "save an extension's data");
    }
}
