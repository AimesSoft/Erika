//! What the gateway remembers: which episode a file is, by its fingerprint
//! (the MD5 of its first 16 MiB and its size), as the user confirmed it.
//! Next to the danmaku server's ids it keeps the titles and the episode's
//! number, which find the episode again when the server's ids change.

use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::sync::Mutex;

/// A file bound to an episode of the danmaku server.
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub anime_id: i64,
    pub episode_id: i64,
    pub anime_title: String,
    pub episode_title: String,
    /// The episode's number in its anime, as the server numbers it.
    pub number: i64,
    /// dandanplay's `type` and `typeDescription`.
    pub kind: String,
    pub kind_description: String,
    pub image: String,
    /// How many times it was confirmed.
    pub confirmations: i64,
}

pub struct Store {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS binding (
        md5 TEXT NOT NULL,
        size INTEGER NOT NULL,
        anime_id INTEGER NOT NULL,
        episode_id INTEGER NOT NULL,
        anime_title TEXT NOT NULL,
        episode_title TEXT NOT NULL,
        number INTEGER NOT NULL,
        kind TEXT NOT NULL,
        kind_description TEXT NOT NULL,
        image TEXT NOT NULL,
        file_name TEXT NOT NULL,
        confirmations INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (md5, size)
    ) STRICT;
";

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
}

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Store> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::ready(conn)
    }

    pub fn in_memory() -> Store {
        Self::ready(Connection::open_in_memory().expect("an in-memory database opens"))
            .expect("the schema applies")
    }

    fn ready(conn: Connection) -> rusqlite::Result<Store> {
        conn.execute_batch(SCHEMA)?;
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The episode the file with this fingerprint was bound to.
    pub fn find(&self, md5: &str, size: i64) -> Option<Binding> {
        self.conn()
            .query_row(
                "SELECT anime_id, episode_id, anime_title, episode_title, number, kind, kind_description, image, confirmations
                 FROM binding WHERE md5 = ?1 AND size = ?2",
                params![md5.to_ascii_lowercase(), size],
                |row| {
                    Ok(Binding {
                        anime_id: row.get(0)?,
                        episode_id: row.get(1)?,
                        anime_title: row.get(2)?,
                        episode_title: row.get(3)?,
                        number: row.get(4)?,
                        kind: row.get(5)?,
                        kind_description: row.get(6)?,
                        image: row.get(7)?,
                        confirmations: row.get(8)?,
                    })
                },
            )
            .optional()
            .unwrap_or_else(|error| {
                eprintln!("erika-gateway: read a binding: {error}");
                None
            })
    }

    /// The user says the file is this episode: confirmed again when it was
    /// bound to it already, bound anew otherwise.
    pub fn confirm(
        &self,
        md5: &str,
        size: i64,
        file_name: &str,
        binding: &Binding,
    ) -> rusqlite::Result<()> {
        self.conn().execute(
            "INSERT INTO binding (md5, size, anime_id, episode_id, anime_title, episode_title, number, kind, kind_description, image, file_name, confirmations, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, ?12)
             ON CONFLICT (md5, size) DO UPDATE SET
                 confirmations = CASE WHEN episode_id = excluded.episode_id THEN confirmations + 1 ELSE 1 END,
                 anime_id = excluded.anime_id, episode_id = excluded.episode_id,
                 anime_title = excluded.anime_title, episode_title = excluded.episode_title,
                 number = excluded.number, kind = excluded.kind, kind_description = excluded.kind_description,
                 image = excluded.image, file_name = excluded.file_name, updated_at = excluded.updated_at",
            params![
                md5.to_ascii_lowercase(),
                size,
                binding.anime_id,
                binding.episode_id,
                binding.anime_title,
                binding.episode_title,
                binding.number,
                binding.kind,
                binding.kind_description,
                binding.image,
                file_name,
                now()
            ],
        )?;
        Ok(())
    }

    /// The server's ids for the episode changed: keep the binding, with the
    /// new ids.
    pub fn rebind(&self, md5: &str, size: i64, binding: &Binding) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE binding SET anime_id = ?3, episode_id = ?4, anime_title = ?5, episode_title = ?6, image = ?7, updated_at = ?8
             WHERE md5 = ?1 AND size = ?2",
            params![md5.to_ascii_lowercase(), size, binding.anime_id, binding.episode_id, binding.anime_title, binding.episode_title, binding.image, now()],
        )?;
        Ok(())
    }

    /// The user unlinked the file.
    pub fn forget(&self, md5: &str, size: i64) -> rusqlite::Result<()> {
        self.conn().execute(
            "DELETE FROM binding WHERE md5 = ?1 AND size = ?2",
            params![md5.to_ascii_lowercase(), size],
        )?;
        Ok(())
    }

    pub fn count(&self) -> i64 {
        self.conn()
            .query_row("SELECT count(*) FROM binding", [], |row| row.get(0))
            .unwrap_or(0)
    }
}
