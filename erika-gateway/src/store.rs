//! What the gateway remembers: which episode a file is, by its fingerprint
//! (the MD5 of its first 16 MiB and its size), as each device confirmed it.
//! A device's own confirmation is what it is answered; another device is
//! answered with the episode most devices agree on, the owner's (trusted)
//! confirmations above all. Next to the danmaku server's ids every vote
//! keeps the titles and the episode's number, which find the episode again
//! when the server's ids change.
//!
//! It also keeps the details of every show it passed on: the danmaku
//! server lists only the episodes whose danmaku it fetched, so what was
//! seen of a show before fills in the rest.

use rusqlite::{Connection, params};
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
    /// How many devices agree on it.
    pub confirmations: i64,
}

/// Who asks: a device, and whether it is the owner's.
#[derive(Clone, Debug, PartialEq)]
pub struct Voter {
    pub device: String,
    pub trusted: bool,
}

pub struct Store {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS vote (
        md5 TEXT NOT NULL,
        size INTEGER NOT NULL,
        device TEXT NOT NULL,
        anime_id INTEGER NOT NULL,
        episode_id INTEGER NOT NULL,
        anime_title TEXT NOT NULL,
        episode_title TEXT NOT NULL,
        number INTEGER NOT NULL,
        kind TEXT NOT NULL,
        kind_description TEXT NOT NULL,
        image TEXT NOT NULL,
        file_name TEXT NOT NULL,
        trusted INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (md5, size, device)
    ) STRICT;
    CREATE TABLE IF NOT EXISTS show (
        anime_id INTEGER PRIMARY KEY,
        details TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    ) STRICT;
    -- The first version's table, from before votes; it was never filled.
    DROP TABLE IF EXISTS binding;
";

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64)
}

/// One vote as read back.
struct Vote {
    device: String,
    trusted: bool,
    updated_at: i64,
    binding: Binding,
}

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Store> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::ready(conn)
    }

    pub fn in_memory() -> Store {
        Self::ready(Connection::open_in_memory().expect("an in-memory database opens")).expect("the schema applies")
    }

    fn ready(conn: Connection) -> rusqlite::Result<Store> {
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn votes(&self, md5: &str, size: i64) -> Vec<Vote> {
        let conn = self.conn();
        let read = || -> rusqlite::Result<Vec<Vote>> {
            let mut statement = conn.prepare(
                "SELECT device, trusted, updated_at, anime_id, episode_id, anime_title, episode_title, number, kind, kind_description, image
                 FROM vote WHERE md5 = ?1 AND size = ?2",
            )?;
            let rows = statement.query_map(params![md5.to_ascii_lowercase(), size], |row| {
                Ok(Vote {
                    device: row.get(0)?,
                    trusted: row.get::<_, i64>(1)? != 0,
                    updated_at: row.get(2)?,
                    binding: Binding {
                        anime_id: row.get(3)?,
                        episode_id: row.get(4)?,
                        anime_title: row.get(5)?,
                        episode_title: row.get(6)?,
                        number: row.get(7)?,
                        kind: row.get(8)?,
                        kind_description: row.get(9)?,
                        image: row.get(10)?,
                        confirmations: 1,
                    },
                })
            })?;
            rows.collect()
        };
        read().unwrap_or_else(|error| {
            eprintln!("erika-gateway: read votes: {error}");
            Vec::new()
        })
    }

    /// The episode the file with this fingerprint is, for `device`: its own
    /// vote, else the episode with the owner's vote, else the one most
    /// devices agree on, the latest of equals.
    pub fn find(&self, md5: &str, size: i64, device: &str) -> Option<Binding> {
        let votes = self.votes(md5, size);
        let agreeing = |episode_id: i64| votes.iter().filter(|vote| vote.binding.episode_id == episode_id).count() as i64;
        if let Some(own) = votes.iter().find(|vote| vote.device == device) {
            return Some(Binding { confirmations: agreeing(own.binding.episode_id), ..own.binding.clone() });
        }
        let best = votes.iter().max_by_key(|vote| (vote.trusted, agreeing(vote.binding.episode_id), vote.updated_at))?;
        Some(Binding { confirmations: agreeing(best.binding.episode_id), ..best.binding.clone() })
    }

    /// `voter` says the file is this episode.
    pub fn confirm(&self, md5: &str, size: i64, voter: &Voter, file_name: &str, binding: &Binding) -> rusqlite::Result<()> {
        self.conn().execute(
            "INSERT INTO vote (md5, size, device, anime_id, episode_id, anime_title, episode_title, number, kind, kind_description, image, file_name, trusted, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT (md5, size, device) DO UPDATE SET
                 anime_id = excluded.anime_id, episode_id = excluded.episode_id,
                 anime_title = excluded.anime_title, episode_title = excluded.episode_title,
                 number = excluded.number, kind = excluded.kind, kind_description = excluded.kind_description,
                 image = excluded.image, file_name = excluded.file_name, trusted = excluded.trusted,
                 updated_at = excluded.updated_at",
            params![
                md5.to_ascii_lowercase(),
                size,
                voter.device,
                binding.anime_id,
                binding.episode_id,
                binding.anime_title,
                binding.episode_title,
                binding.number,
                binding.kind,
                binding.kind_description,
                binding.image,
                file_name,
                voter.trusted as i64,
                now()
            ],
        )?;
        Ok(())
    }

    /// The server's ids for episode `old` changed: every vote for it keeps
    /// pointing at it, by the new ids.
    pub fn rebind(&self, md5: &str, size: i64, old: i64, binding: &Binding) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE vote SET anime_id = ?4, episode_id = ?5, anime_title = ?6, episode_title = ?7, image = ?8
             WHERE md5 = ?1 AND size = ?2 AND episode_id = ?3",
            params![md5.to_ascii_lowercase(), size, old, binding.anime_id, binding.episode_id, binding.anime_title, binding.episode_title, binding.image],
        )?;
        Ok(())
    }

    /// `voter` unlinked the file: its own vote goes; the owner's unlinking
    /// takes every vote with it.
    pub fn forget(&self, md5: &str, size: i64, voter: &Voter) -> rusqlite::Result<()> {
        let md5 = md5.to_ascii_lowercase();
        if voter.trusted {
            self.conn().execute("DELETE FROM vote WHERE md5 = ?1 AND size = ?2", params![md5, size])?;
        } else {
            self.conn().execute("DELETE FROM vote WHERE md5 = ?1 AND size = ?2 AND device = ?3", params![md5, size, voter.device])?;
        }
        Ok(())
    }

    /// The details of show `anime_id` as last passed on, dandanplay's
    /// `bangumi` object.
    pub fn show(&self, anime_id: i64) -> Option<serde_json::Value> {
        let text: String = self
            .conn()
            .query_row("SELECT details FROM show WHERE anime_id = ?1", params![anime_id], |row| row.get(0))
            .ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn remember_show(&self, anime_id: i64, details: &serde_json::Value) -> rusqlite::Result<()> {
        self.conn().execute(
            "INSERT INTO show (anime_id, details, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (anime_id) DO UPDATE SET details = excluded.details, updated_at = excluded.updated_at",
            params![anime_id, details.to_string(), now()],
        )?;
        Ok(())
    }

    /// How many files have votes.
    pub fn count(&self) -> i64 {
        self.conn().query_row("SELECT count(*) FROM (SELECT DISTINCT md5, size FROM vote)", [], |row| row.get(0)).unwrap_or(0)
    }
}
