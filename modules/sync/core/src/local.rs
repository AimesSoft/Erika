//! This device's data as the sync carries it, and taking in what came:
//! every setting, the library's sources, what files are and where they
//! stopped, the recent videos, the anime marks, the accounts and the
//! extensions' data. What can be fetched or found again (pictures, posters,
//! scans, caches) and what only this device can use (its id, download
//! tasks) stay out.
//!
//! Each category is keyed so that a key means the same record on every
//! device: files by fingerprint, anime and episodes by dandanplay id,
//! sources by kind and place.

use crate::format::{Op, State};
use crate::seal::SealKey;
use crate::value::Value;
use bern::storage::Store;
use erika_store::Db;
use erika_store::portable::{
    Anime, Episode, FileKey, Link, Mark, Progress, Recent, Source, Watched,
};
use std::collections::BTreeMap;

pub const SETTINGS: &str = "settings";
pub const STORE: &str = "store";
pub const SOURCES: &str = "sources";
pub const ANIME: &str = "anime";
pub const EPISODES: &str = "episodes";
pub const LINKS: &str = "links";
pub const PROGRESS: &str = "progress";
pub const WATCHED: &str = "watched";
pub const RECENTS: &str = "recents";
pub const MARKS: &str = "marks";
pub const ACCOUNTS: &str = "accounts";
pub const EXTENSIONS: &str = "extensions";
/// Credentials keyed by the source's kind and location, not its local row id.
pub const SOURCE_SECRETS: &str = "source_secrets";
/// Opaque extension JSON may hold arbitrary credentials. It is sealed as a
/// whole rather than guessing which property names are passwords.
pub const SEALED_CATEGORIES: &[&str] = &[ACCOUNTS, SOURCE_SECRETS, EXTENSIONS, SYNC];
/// The WebDAV destination and credential, sealed together: in exports only.
pub const SYNC: &str = "sync";
const WEBDAV_CONFIGURATION: &str = "sync-webdav-configuration";

/// Every category, in the order changes are taken in: what others refer
/// to first.
pub const CATEGORIES: &[&str] = &[
    SETTINGS, STORE, SOURCES, SOURCE_SECRETS, ANIME, EPISODES, LINKS, PROGRESS, WATCHED, RECENTS, MARKS, ACCOUNTS,
    EXTENSIONS, SYNC,
    crate::catalog::NODES, crate::catalog::LINKS, crate::catalog::BINDINGS,
    crate::catalog::DANMAKU, crate::catalog::WATCHED, crate::catalog::MARKS,
];

/// The values the app keeps in its store rather than its database.
pub const STORE_KEYS: &[&str] = &["welcome-v1"];

/// The secrets and settings of the accounts (see erika-core's account.rs
/// and bangumi.rs), and the sync's own.
const DANDANPLAY_TOKEN: &str = "dandanplay-token";
const DANDANPLAY_SETTINGS: [(&str, &str); 3] = [
    ("user", "dandanplay.user"),
    ("name", "dandanplay.name"),
    ("renewed", "dandanplay.renewed"),
];
const BANGUMI_COOKIES: &str = "bangumi-cookies";
const BANGUMI_SETTINGS: [(&str, &str); 4] = [
    ("user", "bangumi.user"),
    ("email", "bangumi.email"),
    ("name", "bangumi.name"),
    ("avatar", "bangumi.avatar"),
];
pub const PASSPHRASE: &str = "sync-passphrase";
pub const WEBDAV_PASSWORD: &str = "sync-webdav-password";
pub const WEBDAV_SETTINGS: [(&str, &str); 5] = [
    ("url", "sync.webdav.url"),
    ("user", "sync.webdav.user"),
    ("path", "sync.webdav.path"),
    ("auto", "sync.auto"),
    ("interval", "sync.interval"),
];

/// Settings that are not settings of the user's: this device's id, the
/// accounts' (in their own category) and the sync's.
fn own_setting(key: &str) -> bool {
    key != "device.id"
        && !key.starts_with("dandanplay.")
        && !key.starts_with("bangumi.")
        && !key.starts_with("sync.")
        && !key.starts_with(erika_store::portable::DANDANPLAY_SOURCE_SETTINGS)
        && !key.starts_with("anime.tmdb.")
        && !key.starts_with("anime.calendar.")
        && !key.starts_with("anime.broadcast_schedule.")
}

/// What a sync carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// An export: everything, to restore this device or move to another.
    Export,
    /// A WebDAV repository: what means the same on the user's other
    /// devices; not this device's folders and files.
    Repository,
}

/// Whether an entry means the same on another device.
pub fn portable(category: &str, key: &str, value: &Value) -> bool {
    match category {
        SETTINGS => own_setting(key) && !matches!(key, "downloads.places" | "downloads.place"),
        SOURCES => value.get("kind").and_then(Value::as_str) != Some("folder"),
        RECENTS => web_address(key) || erika_store::plugin_sources::is_address(key),
        SYNC => false,
        _ => true,
    }
}

/// Whether `url` is a web address another device reaches too: not a
/// file, and not this device's own server, such as a download streamed
/// while it downloads.
fn web_address(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
    else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = match host.strip_prefix('[') {
        Some(bracketed) => bracketed.split(']').next().unwrap_or_default(),
        None => host.split(':').next().unwrap_or_default(),
    };
    !(host.eq_ignore_ascii_case("localhost")
        || host.starts_with("127.")
        || host == "::1"
        || host == "0.0.0.0")
}

/// Where secrets are kept: the system's credential store, or memory in
/// tests.
pub trait Vault {
    fn read(&self, name: &str) -> Option<String>;
    fn write(&self, name: &str, value: &str) -> bool;
    fn delete(&self, name: &str);
}

pub struct SystemVault;

impl Vault for SystemVault {
    fn read(&self, name: &str) -> Option<String> {
        bern::secrets::read(name)
            .ok()
            .flatten()
            .filter(|value| !value.is_empty())
    }
    fn write(&self, name: &str, value: &str) -> bool {
        bern::secrets::write(name, value)
            .map_err(|error| eprintln!("erika-sync: keep secret {name}: {}", error.message))
            .is_ok()
    }
    fn delete(&self, name: &str) {
        if let Err(error) = bern::secrets::delete(name) {
            eprintln!("erika-sync: forget secret {name}: {}", error.message);
        }
    }
}

/// No secrets at all: for counting what an export holds without asking
/// the credential store.
pub struct NoVault;

impl Vault for NoVault {
    fn read(&self, _name: &str) -> Option<String> {
        None
    }
    fn write(&self, _name: &str, _value: &str) -> bool {
        false
    }
    fn delete(&self, _name: &str) {}
}

#[derive(Default)]
pub struct MemoryVault(pub std::sync::Mutex<BTreeMap<String, String>>);

impl Vault for MemoryVault {
    fn read(&self, name: &str) -> Option<String> {
        self.0.lock().unwrap().get(name).cloned()
    }
    fn write(&self, name: &str, value: &str) -> bool {
        self.0.lock().unwrap().insert(name.into(), value.into());
        true
    }
    fn delete(&self, name: &str) {
        self.0.lock().unwrap().remove(name);
    }
}

/// What taking in changes did, for the app to show it anew.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Applied {
    pub settings: bool,
    pub accounts: bool,
    pub library: bool,
    pub history: bool,
    pub marks: bool,
    pub extensions: bool,
    pub sync: bool,
    /// Sources added, to scan.
    pub new_sources: Vec<i64>,
    /// Existing sources whose credentials changed and whose directory must reload.
    pub rescan_sources: Vec<i64>,
    /// Secrets that could not be opened: another passphrase sealed them.
    pub locked: usize,
    pub changes: usize,
}

/// This device's data.
pub struct Device<'a> {
    pub db: &'a Db,
    pub store: &'a Store,
    pub vault: &'a dyn Vault,
    pub scope: Scope,
    pub applied: Applied,
    collected_catalog: std::cell::RefCell<Option<erika_store::catalog::CatalogSnapshot>>,
}

fn some_int(value: Option<i64>) -> Value {
    value.map_or(Value::Null, Value::Int)
}

fn some_text(value: Option<&str>) -> Value {
    value.map_or(Value::Null, Value::text)
}

fn opt_int(value: &Value, field: &str) -> Option<i64> {
    value.int_of(field)
}

fn opt_text(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(String::from)
}

impl<'a> Device<'a> {
    pub fn new(db: &'a Db, store: &'a Store, vault: &'a dyn Vault, scope: Scope) -> Self {
        Device {
            db,
            store,
            vault,
            scope,
            applied: Applied::default(),
            collected_catalog: std::cell::RefCell::new(None),
        }
    }

    /// Everything this device has, its private records sealed with `key`;
    /// without a key those records stay out.
    pub fn collect(&self, key: Option<&SealKey>) -> Result<State, crate::repository::SyncError> {
        let snapshot = self.db.catalog_export().map_err(|e| crate::repository::SyncError::new(e.message))?;
        let mut state = crate::catalog::records(&snapshot)?;
        self.collected_catalog.replace(Some(snapshot));
        let mut put = |category: &str, key: String, value: Value| {
            if self.scope == Scope::Export || portable(category, &key, &value) {
                state
                    .entry(category.to_string())
                    .or_default()
                    .insert(key, value);
            }
        };
        for (key, value) in self.db.settings() {
            if own_setting(&key) {
                put(SETTINGS, key, Value::Text(value));
            }
        }
        for name in STORE_KEYS {
            if let Ok(Some(bytes)) = self.store.read(name) {
                put(STORE, name.to_string(), Value::Bytes(bytes));
            }
        }
        for source in self.db.portable_sources() {
            put(
                SOURCES,
                format!("{}\n{}", source.kind, source.location),
                Value::map([
                    ("kind", Value::text(source.kind)),
                    ("name", Value::text(source.name)),
                    ("location", Value::text(source.location)),
                    ("content", Value::text(source.content)),
                    ("added", Value::Int(source.added)),
                ]),
            );
        }
        for anime in self.db.portable_anime() {
            put(
                ANIME,
                anime.dandanplay_id.to_string(),
                Value::map([
                    ("title", Value::text(anime.title)),
                    ("image", Value::text(anime.image)),
                    ("bangumi", some_int(anime.bangumi_id)),
                ]),
            );
        }
        for episode in self.db.portable_episodes() {
            put(
                EPISODES,
                episode.dandanplay_id.to_string(),
                Value::map([
                    ("anime", Value::Int(episode.anime)),
                    ("number", some_int(episode.number)),
                    ("title", Value::text(episode.title)),
                    ("bangumi", some_int(episode.bangumi_id)),
                ]),
            );
        }
        for link in self.db.portable_links() {
            put(
                LINKS,
                link.file.text(),
                Value::map([
                    ("episode", some_int(link.episode)),
                    ("by", some_text(link.matched_by.as_deref())),
                ]),
            );
        }
        for progress in self.db.portable_progress() {
            put(
                PROGRESS,
                progress.file.text(),
                Value::map([
                    ("position", Value::Float(progress.position)),
                    ("duration", Value::Float(progress.duration)),
                    ("offset", Value::Float(progress.danmaku_offset)),
                    ("at", Value::Int(progress.at)),
                ]),
            );
        }
        for watched in self.db.portable_watched() {
            put(
                WATCHED,
                watched.key(),
                Value::map([
                    ("anime", Value::Int(watched.anime)),
                    ("number", some_int(watched.number)),
                    ("at", Value::Int(watched.at)),
                    ("manual", Value::Bool(watched.manual)),
                ]),
            );
        }
        for recent in self.db.portable_recents() {
            put(
                RECENTS,
                recent.url,
                Value::map([
                    ("title", Value::text(recent.title)),
                    ("position", Value::Float(recent.position)),
                    ("duration", Value::Float(recent.duration)),
                    ("at", Value::Int(recent.at)),
                    (
                        "file",
                        some_text(recent.file.map(|file| file.text()).as_deref()),
                    ),
                ]),
            );
        }
        for mark in self.db.anime_marks() {
            put(
                MARKS,
                mark.anime.to_string(),
                Value::map([
                    ("status", some_text(mark.status.as_deref())),
                    ("rating", Value::Int(mark.rating)),
                    ("at", Value::Int(mark.at)),
                ]),
            );
        }
        if let Some(key) = key {
            for source in self.db.sources().into_iter().filter(|s| s.kind == "dandanplay") {
                let setting = erika_store::portable::dandanplay_token_setting(source.id);
                if let Some(token) = self.db.setting(&setting).filter(|v| !v.is_empty()) {
                    let id = format!("{}\n{}", source.kind, source.location);
                    put(SOURCE_SECRETS, id.clone(), Value::map([(
                        "token", Value::Sealed(key.seal(&format!("source-token\n{id}"), token.as_bytes())),
                    )]));
                }
            }
            for (id, json) in self.db.extensions() {
                // Canonical serialization makes an unchanged extension compare
                // equal after import, irrespective of its original whitespace.
                let json: serde_json::Value = serde_json::from_str(&json)
                    .map_err(|_| crate::repository::SyncError::new("扩展数据不是有效 JSON，无法备份"))?;
                let json = json.to_string();
                put(EXTENSIONS, id.clone(), Value::map([(
                    "data", Value::Sealed(key.seal(&format!("extension\n{id}"), json.as_bytes())),
                )]));
            }
            if let Some(token) = self.vault.read(DANDANPLAY_TOKEN) {
                let mut account = self.fields(&DANDANPLAY_SETTINGS);
                account.insert(
                    "token".into(),
                    Value::Sealed(key.seal(DANDANPLAY_TOKEN, token.as_bytes())),
                );
                put(ACCOUNTS, "dandanplay".into(), Value::Map(account));
            }
            if let Some(cookies) = self.vault.read(BANGUMI_COOKIES) {
                let mut account = self.fields(&BANGUMI_SETTINGS);
                account.insert(
                    "cookies".into(),
                    Value::Sealed(key.seal(BANGUMI_COOKIES, cookies.as_bytes())),
                );
                put(ACCOUNTS, "bangumi".into(), Value::Map(account));
            }
            if self.scope == Scope::Export && self.db.setting("sync.webdav.url").is_some_and(|url| !url.is_empty()) {
                // Bind the complete destination to its password. An omitted
                // password must never reuse the receiving device's credential.
                let mut webdav = self.fields(&WEBDAV_SETTINGS);
                webdav.insert("password".into(), Value::text(self.vault.read(WEBDAV_PASSWORD).unwrap_or_default()));
                let json = Value::Map(webdav).to_json().to_string();
                put(SYNC, "webdav".into(), Value::map([(
                    "data", Value::Sealed(key.seal(WEBDAV_CONFIGURATION, json.as_bytes())),
                )]));
            }
        }
        Ok(state)
    }

    /// Settings `fields` names, by their names in a record.
    fn fields(&self, fields: &[(&str, &str)]) -> BTreeMap<String, Value> {
        fields
            .iter()
            .filter_map(|(field, setting)| {
                self.db
                    .setting(setting)
                    .map(|value| (field.to_string(), Value::Text(value)))
            })
            .collect()
    }

    fn put_fields(&self, value: &Value, fields: &[(&str, &str)]) {
        for (field, setting) in fields {
            self.db
                .set_setting(setting, value.get(field).and_then(Value::as_str));
        }
    }

    /// Open sealed field `field` of `value` for `purpose`.
    fn open(
        &mut self,
        key: Option<&SealKey>,
        value: &Value,
        field: &str,
        purpose: &str,
    ) -> Option<String> {
        let sealed = value.get(field)?.as_bytes()?;
        let text = key
            .and_then(|key| key.open_sealed(purpose, sealed))
            .and_then(|text| String::from_utf8(text).ok());
        if text.is_none() {
            self.applied.locked += 1;
        }
        text
    }

    /// Take in database records atomically, then apply store/vault side effects.
    pub fn apply(&mut self, ops: &[Op], key: Option<&SealKey>) -> Result<(), crate::repository::SyncError> {
        let media_count = ops.iter().filter(|op| crate::catalog::CATEGORIES.contains(&op.category.as_str())).count();
        let expected = if media_count > 0 {
            Some(self.collected_catalog.take().ok_or_else(|| crate::repository::SyncError::new("请先读取本地媒体库再同步"))?)
        } else {
            None
        };
        let mut ordered: Vec<&Op> = ops.iter().filter(|op| !crate::catalog::CATEGORIES.contains(&op.category.as_str())).collect();
        ordered.sort_by_key(|op| {
            CATEGORIES.iter().position(|category| *category == op.category).unwrap_or(CATEGORIES.len())
        });
        let external = |op: &&Op| matches!(op.category.as_str(), STORE | ACCOUNTS | SYNC);
        let applied = self.db.transaction(|db| {
            // Browser storage supplies a private draft; never capture self.db for writes.
            let mut staged = Device::new(db, self.store, self.vault, self.scope);
            staged.applied = self.applied.clone();
            if let Some(expected) = expected.as_ref() {
                crate::catalog::apply(db, expected, ops)
                    .map_err(|e| erika_store::catalog::Error::invalid(&e.message))?;
                staged.applied.library = true;
                staged.applied.history = true;
                staged.applied.marks = true;
                staged.applied.changes += media_count;
            }
            for op in ordered.iter().filter(|op| !external(op)) {
                staged.apply_one(op, key)
                    .map_err(|e| erika_store::catalog::Error::invalid(&e.message))?;
            }
            Ok(staged.applied)
        }).map_err(|e| crate::repository::SyncError::new(e.message))?;
        self.applied = applied;
        for op in ordered.into_iter().filter(external) {
            self.apply_one(op, key)?;
        }
        Ok(())
    }

    fn apply_one(&mut self, op: &Op, key: Option<&SealKey>) -> Result<(), crate::repository::SyncError> {
        // Apply the same boundary on input, including deletions: a remote record
        // must not replace this device's identity, account fields or caches via
        // the unsealed, generic settings category.
        if op.category == SETTINGS && (!own_setting(&op.key)
            || (self.scope == Scope::Repository
                && !portable(SETTINGS, &op.key, op.value.as_ref().unwrap_or(&Value::Null))))
        {
            return Ok(());
        }
        if SEALED_CATEGORIES.contains(&op.category.as_str()) && key.is_none() {
            // Without a verified key, absence means unavailable, not a request
            // to clear a credential or replace a local plugin configuration.
            self.applied.locked += usize::from(op.value.is_some());
            return Ok(());
        }
        if op.category == SOURCE_SECRETS || op.category == EXTENSIONS {
            return self.apply_private_record(op, key);
        }
        if op.category == SYNC {
            return if self.scope == Scope::Export && op.key == "webdav" {
                self.apply_webdav(op.value.as_ref(), key)
            } else {
                Ok(())
            };
        }
        let value = op.value.as_ref();
        let applied = &mut self.applied;
        applied.changes += 1;
        match op.category.as_str() {
            SETTINGS => {
                applied.settings = true;
                self.db.set_setting(&op.key, value.and_then(Value::as_str));
            }
            STORE => {
                applied.settings = true;
                if let (Some(bytes), true) = (
                    value.and_then(Value::as_bytes),
                    STORE_KEYS.contains(&op.key.as_str()),
                ) {
                    let _ = self.store.write(&op.key, bytes);
                }
            }
            SOURCES => {
                applied.library = true;
                match value {
                    Some(value) => {
                        let source = Source {
                            kind: value.str_of("kind"),
                            name: value.str_of("name"),
                            location: value.str_of("location"),
                            content: value.str_of("content"),
                            added: value.int_of("added").unwrap_or_else(erika_store::now),
                        };
                        if op.key != format!("{}\n{}", source.kind, source.location) {
                            return Err(crate::repository::SyncError::new("来源键与身份不一致"));
                        }
                        let (id, new) = self.db.put_source(&source)
                            .map_err(|e| crate::repository::SyncError::new(e.message))?;
                        if new {
                            applied.new_sources.push(id);
                        }
                    }
                    None => {
                        if let Some((kind, location)) = op.key.split_once('\n') {
                            if kind == "dandanplay" {
                                if let Some(source) = self.db.sources().into_iter()
                                    .find(|s| s.kind == kind && s.location == location)
                                {
                                    self.db.set_setting(&erika_store::portable::dandanplay_token_setting(source.id), None);
                                }
                            }
                            self.db.remove_source_at(kind, location);
                        }
                    }
                }
            }
            ANIME => {
                if let (Some(value), Ok(id)) = (value, op.key.parse()) {
                    applied.library = true;
                    self.db.put_anime(&Anime {
                        dandanplay_id: id,
                        bangumi_id: opt_int(value, "bangumi"),
                        title: value.str_of("title"),
                        image: value.str_of("image"),
                    });
                }
            }
            EPISODES => {
                if let (Some(value), Ok(id)) = (value, op.key.parse()) {
                    applied.library = true;
                    self.db.put_episode(&Episode {
                        dandanplay_id: id,
                        anime: value.int_of("anime").unwrap_or(0),
                        number: opt_int(value, "number"),
                        bangumi_id: opt_int(value, "bangumi"),
                        title: value.str_of("title"),
                    });
                }
            }
            LINKS => {
                let Some(file) = FileKey::parse(&op.key) else {
                    return Ok(());
                };
                applied.library = true;
                match value {
                    Some(value) => self.db.put_link(&Link {
                        file,
                        episode: opt_int(value, "episode"),
                        matched_by: opt_text(value, "by"),
                    }),
                    None => self.db.remove_link(&file),
                }
            }
            PROGRESS => {
                let Some(file) = FileKey::parse(&op.key) else {
                    return Ok(());
                };
                applied.history = true;
                match value {
                    Some(value) => self.db.put_progress(&Progress {
                        file,
                        position: value.float_of("position"),
                        duration: value.float_of("duration"),
                        danmaku_offset: value.float_of("offset"),
                        at: value.int_of("at").unwrap_or(0),
                    }),
                    None => self.db.remove_progress(&file),
                }
            }
            WATCHED => {
                // `id`, or `anime#number` for an episode without an id.
                let (episode, anime, number) = match op.key.split_once('#') {
                    Some((anime, number)) => (None, anime.parse().ok(), number.parse().ok()),
                    None => (op.key.parse().ok(), None, None),
                };
                if episode.is_none() && anime.is_none() {
                    return Ok(());
                }
                applied.history = true;
                let watched = Watched {
                    episode,
                    anime: anime
                        .or_else(|| value.and_then(|value| value.int_of("anime")))
                        .unwrap_or(0),
                    number: number.or_else(|| value.and_then(|value| opt_int(value, "number"))),
                    at: value.and_then(|value| value.int_of("at")).unwrap_or(0),
                    manual: value
                        .and_then(|value| value.get("manual"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                };
                match value {
                    Some(_) => self.db.put_watched(&watched),
                    None => self.db.remove_watched(&watched),
                }
            }
            RECENTS => {
                applied.history = true;
                match value {
                    Some(value) => self.db.put_recent(&Recent {
                        url: op.key.clone(),
                        title: value.str_of("title"),
                        position: value.float_of("position"),
                        duration: value.float_of("duration"),
                        at: value.int_of("at").unwrap_or(0),
                        file: opt_text(value, "file").and_then(|file| FileKey::parse(&file)),
                    }),
                    None => self.db.remove_recent(&op.key),
                }
            }
            MARKS => {
                let Ok(anime) = op.key.parse() else { return Ok(()) };
                applied.marks = true;
                match value {
                    Some(value) => self.db.set_anime_mark(&Mark {
                        anime,
                        status: opt_text(value, "status"),
                        rating: value.int_of("rating").unwrap_or(0),
                        at: value.int_of("at").unwrap_or(0),
                    }),
                    None => self.db.remove_anime_mark(anime),
                }
            }
            ACCOUNTS => self.apply_account(&op.key, value, key),
            // A category of a later version.
            _ => applied.changes -= 1,
        }
        Ok(())
    }

    fn apply_webdav(&mut self, value: Option<&Value>, key: Option<&SealKey>) -> Result<(), crate::repository::SyncError> {
        let Some(value) = value else { return Ok(()) };
        if !matches!(value.get("data"), Some(Value::Sealed(_))) {
            return Err(crate::repository::SyncError::new("WebDAV 配置必须经过同步口令加密"));
        }
        let text = self.open(key, value, "data", WEBDAV_CONFIGURATION)
            .ok_or_else(|| crate::repository::SyncError::new("无法解密 WebDAV 配置，已保留本机配置"))?;
        let fields: BTreeMap<String, String> = serde_json::from_str(&text)
            .map_err(|_| crate::repository::SyncError::new("加密的 WebDAV 配置格式不正确"))?;
        if !fields.get("url").is_some_and(|url| !url.is_empty()) {
            return Err(crate::repository::SyncError::new("加密的 WebDAV 配置缺少地址"));
        }
        let password = fields.get("password")
            .ok_or_else(|| crate::repository::SyncError::new("加密的 WebDAV 配置缺少凭据"))?;
        // Even an empty password replaces the previous one. Do not publish a
        // new endpoint until the vault accepts its matching credential.
        if !self.vault.write(WEBDAV_PASSWORD, password) {
            return Err(crate::repository::SyncError::new("无法保存 WebDAV 密码，已保留本机配置"));
        }
        self.put_fields(&Value::Map(fields.into_iter().map(|(key, text)| (key, Value::text(text))).collect()), &WEBDAV_SETTINGS);
        self.applied.sync = true;
        self.applied.changes += 1;
        Ok(())
    }

    fn apply_private_record(&mut self, op: &Op, key: Option<&SealKey>) -> Result<(), crate::repository::SyncError> {
        let source = op.category == SOURCE_SECRETS;
        let field = if source { "token" } else { "data" };
        let purpose = format!("{}\n{}", if source { "source-token" } else { "extension" }, op.key);
        let text = match op.value.as_ref() {
            Some(value) => {
                if !matches!(value.get(field), Some(Value::Sealed(_))) {
                    return Err(crate::repository::SyncError::new("私密数据必须经过同步口令加密"));
                }
                let text = self.open(key, value, field, &purpose)
                    .ok_or_else(|| crate::repository::SyncError::new("无法解密私密数据，已保留本机记录"))?;
                Some(text)
            }
            None => None,
        };
        if source {
            let Some(("dandanplay", location)) = op.key.split_once('\n') else {
                return Err(crate::repository::SyncError::new("来源凭据身份不正确"));
            };
            let target = self.db.sources().into_iter()
                .find(|s| s.kind == "dandanplay" && s.location == location);
            match target {
                Some(target) => {
                    let setting = erika_store::portable::dandanplay_token_setting(target.id);
                    if self.db.setting(&setting) != text {
                        self.db.set_setting(&setting, text.as_deref());
                        if !self.applied.rescan_sources.contains(&target.id) {
                            self.applied.rescan_sources.push(target.id);
                        }
                    }
                }
                None if text.is_none() => return Ok(()), // The source was deleted first.
                None => return Err(crate::repository::SyncError::new("来源凭据缺少对应来源")),
            }
            self.applied.library = true;
        } else {
            let json = text.map(|text| serde_json::from_str::<serde_json::Value>(&text)
                .map(|value| value.to_string())
                .map_err(|_| crate::repository::SyncError::new("加密的扩展数据不是有效 JSON")))
                .transpose()?;
            self.db.set_extension(&op.key, json.as_deref());
            self.applied.extensions = true;
        }
        self.applied.changes += 1;
        Ok(())
    }

    fn apply_account(&mut self, account: &str, value: Option<&Value>, key: Option<&SealKey>) {
        let (secret, field, fields): (&str, &str, &[(&str, &str)]) = match account {
            "dandanplay" => (DANDANPLAY_TOKEN, "token", &DANDANPLAY_SETTINGS),
            "bangumi" => (BANGUMI_COOKIES, "cookies", &BANGUMI_SETTINGS),
            _ => return,
        };
        match value {
            Some(value) => {
                let Some(text) = self.open(key, value, field, secret) else {
                    return;
                };
                if self.vault.write(secret, &text) {
                    self.put_fields(value, fields);
                    self.applied.accounts = true;
                }
            }
            None => {
                self.vault.delete(secret);
                for (_, setting) in fields {
                    self.db.set_setting(setting, None);
                }
                self.applied.accounts = true;
            }
        }
    }
}

impl crate::repository::Local for Device<'_> {
    fn collect(&mut self, key: Option<&SealKey>) -> Result<State, crate::repository::SyncError> {
        Device::collect(self, key)
    }
    fn apply(&mut self, ops: &[Op], key: Option<&SealKey>) -> Result<(), crate::repository::SyncError> {
        Device::apply(self, ops, key)
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod credentials_tests;

/// How many entries each category of `state` holds.
pub fn counts(state: &State) -> BTreeMap<String, usize> {
    state
        .iter()
        .map(|(category, entries)| (category.clone(), entries.len()))
        .collect()
}
