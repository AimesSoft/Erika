//! A sync repository on a WebDAV server, laid out as NipaPlay's
//! incremental sync lays its own:
//!
//! - `manifest.erika`: the index: the repository's id and passphrase key
//!   parameters, the current snapshot, and every patch;
//! - `snap_v{n}.erika`: the whole state at snapshot `n`;
//! - `patch_v{n}_{time}_{id}.erika`: one device's changes since then.
//!
//! A sync reads the manifest, rebuilds the remote state from the snapshot
//! (or from the base this device kept) and the patches it has not seen,
//! merges it with this device's state, applies to this device what came
//! from elsewhere, and uploads its own changes as a new patch. Patches are
//! never changed once written, so two devices syncing at once both keep
//! theirs; after 64 of them a new snapshot takes them in, and the files of
//! snapshots two generations old go.

use crate::format::{self, Document, FormatError, Kind, Op, State};
use crate::merge;
use crate::seal::{KeyParams, SealKey};
use crate::value::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const MANIFEST: &str = "manifest.erika";
const PATCHES_BEFORE_SNAPSHOT: usize = 64;
const SNAPSHOT_GENERATIONS: i64 = 2;
const CLEANUP_BATCH: usize = 4;

/// What went wrong talking to the server, said for the user.
#[derive(Clone, Debug, PartialEq)]
pub struct SyncError {
    pub message: String,
    /// The HTTP status, when the server answered.
    pub status: Option<u16>,
}

impl SyncError {
    pub fn new(message: impl Into<String>) -> SyncError {
        SyncError {
            message: message.into(),
            status: None,
        }
    }
}

impl From<FormatError> for SyncError {
    fn from(error: FormatError) -> Self {
        SyncError::new(error.to_string())
    }
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Where a repository's files are: a WebDAV folder, or memory in tests.
pub trait Transport {
    fn ensure_dir(&mut self, path: &str) -> Result<(), SyncError>;
    /// The names of the files in folder `path`.
    fn list(&mut self, path: &str) -> Result<Vec<String>, SyncError>;
    /// The file at `path`, or `None` when there is none.
    fn read(&mut self, path: &str) -> Result<Option<Vec<u8>>, SyncError>;
    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), SyncError>;
    fn delete(&mut self, path: &str) -> Result<(), SyncError>;
}

/// This device's side of a sync.
pub trait Local {
    /// What this device has for the repository, its secrets sealed with
    /// `key` (left out without one).
    fn collect(&mut self, key: Option<&SealKey>) -> State;
    /// Take in changes that came from other devices.
    fn apply(&mut self, ops: &[Op], key: Option<&SealKey>);
}

/// The keys of the passphrase, drawn once per salt: drawing one is slow
/// on purpose.
#[derive(Default)]
pub struct Keys {
    pub passphrase: Option<String>,
    pub rounds: Option<u32>,
    drawn: Vec<(KeyParams, SealKey)>,
}

impl Keys {
    pub fn new(passphrase: Option<String>) -> Keys {
        Keys {
            passphrase: passphrase.filter(|text| !text.is_empty()),
            ..Keys::default()
        }
    }

    /// The passphrase's key under `params`; `None` without a passphrase
    /// or with another one than the key was made with.
    pub fn open(&mut self, params: &KeyParams) -> Option<SealKey> {
        if let Some((_, key)) = self.drawn.iter().find(|(drawn, _)| drawn == params) {
            return Some(key.clone());
        }
        let key = SealKey::open(self.passphrase.as_deref()?, params)?;
        self.drawn.push((params.clone(), key.clone()));
        Some(key)
    }

    /// A key of the passphrase with a new salt.
    pub fn create(&mut self) -> Option<SealKey> {
        let key = SealKey::create(
            self.passphrase.as_deref()?,
            self.rounds.unwrap_or(crate::seal::ROUNDS),
        );
        self.drawn.push((key.params().clone(), key.clone()));
        Some(key)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PatchEntry {
    pub id: String,
    pub file: String,
    pub sha256: Vec<u8>,
    pub size: i64,
    pub created: i64,
    pub device: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub repository: String,
    pub key: Option<KeyParams>,
    pub snapshot: i64,
    pub snapshot_file: String,
    pub snapshot_sha256: Vec<u8>,
    /// The patches the snapshot already holds.
    pub snapshot_patches: BTreeSet<String>,
    pub patches: Vec<PatchEntry>,
    pub updated: i64,
}

impl Manifest {
    fn document(&self) -> Document {
        let patches = self
            .patches
            .iter()
            .map(|entry| {
                Value::map([
                    ("id", Value::text(&entry.id)),
                    ("file", Value::text(&entry.file)),
                    ("sha256", Value::Bytes(entry.sha256.clone())),
                    ("size", Value::Int(entry.size)),
                    ("created", Value::Int(entry.created)),
                    ("device", Value::text(&entry.device)),
                ])
            })
            .collect();
        let mut document = Document::new(Kind::Manifest)
            .with_head("repository", Value::text(&self.repository))
            .with_head("updated", Value::Int(self.updated));
        document.key = self.key.clone();
        document.index = Some(Value::map([
            (
                "snapshot",
                Value::map([
                    ("version", Value::Int(self.snapshot)),
                    ("file", Value::text(&self.snapshot_file)),
                    ("sha256", Value::Bytes(self.snapshot_sha256.clone())),
                    (
                        "patches",
                        Value::List(self.snapshot_patches.iter().map(Value::text).collect()),
                    ),
                ]),
            ),
            ("patches", Value::List(patches)),
        ]));
        document
    }

    fn from_document(document: &Document) -> Result<Manifest, SyncError> {
        let damaged = || SyncError::new("云端同步索引已损坏");
        if document.kind != Kind::Manifest {
            return Err(damaged());
        }
        let index = document.index.as_ref().ok_or_else(damaged)?;
        let snapshot = index.get("snapshot").ok_or_else(damaged)?;
        let list = |value: Option<&Value>| match value {
            Some(Value::List(items)) => items.clone(),
            _ => Vec::new(),
        };
        Ok(Manifest {
            repository: document.head_str("repository"),
            key: document.key.clone(),
            snapshot: snapshot.int_of("version").ok_or_else(damaged)?,
            snapshot_file: snapshot.str_of("file"),
            snapshot_sha256: snapshot
                .get("sha256")
                .and_then(Value::as_bytes)
                .unwrap_or_default()
                .to_vec(),
            snapshot_patches: list(snapshot.get("patches"))
                .iter()
                .filter_map(|id| id.as_str().map(String::from))
                .collect(),
            patches: list(index.get("patches"))
                .iter()
                .map(|entry| PatchEntry {
                    id: entry.str_of("id"),
                    file: entry.str_of("file"),
                    sha256: entry
                        .get("sha256")
                        .and_then(Value::as_bytes)
                        .unwrap_or_default()
                        .to_vec(),
                    size: entry.int_of("size").unwrap_or(0),
                    created: entry.int_of("created").unwrap_or(0),
                    device: entry.str_of("device"),
                })
                .filter(|entry| !entry.id.is_empty() && !entry.file.is_empty())
                .collect(),
            updated: document.head_int("updated").unwrap_or(0),
        })
    }
}

/// What this device last agreed with a repository: the state then, and
/// which snapshot and patches it had seen.
#[derive(Clone, Debug, PartialEq)]
pub struct Base {
    pub place: String,
    pub repository: String,
    pub snapshot: i64,
    pub snapshot_sha256: Vec<u8>,
    pub applied: BTreeSet<String>,
    pub state: State,
}

impl Base {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut document = Document::new(Kind::Base)
            .with_head("place", Value::text(&self.place))
            .with_head("repository", Value::text(&self.repository))
            .with_head("snapshot", Value::Int(self.snapshot))
            .with_head("sha256", Value::Bytes(self.snapshot_sha256.clone()));
        document.index = Some(Value::map([(
            "applied",
            Value::List(self.applied.iter().map(Value::text).collect()),
        )]));
        document.state = Some(self.state.clone());
        format::write(&document)
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Base> {
        let document = format::read(bytes).ok()?;
        if document.kind != Kind::Base {
            return None;
        }
        let applied = match document.index.as_ref()?.get("applied")? {
            Value::List(ids) => ids
                .iter()
                .filter_map(|id| id.as_str().map(String::from))
                .collect(),
            _ => return None,
        };
        Some(Base {
            place: document.head_str("place"),
            repository: document.head_str("repository"),
            snapshot: document.head_int("snapshot")?,
            snapshot_sha256: document
                .head
                .get("sha256")
                .and_then(Value::as_bytes)?
                .to_vec(),
            applied,
            state: document.state?,
        })
    }
}

/// How a sync went.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    /// The repository was made by this sync.
    pub created: bool,
    pub downloaded: usize,
    pub uploaded: usize,
    pub restored: usize,
    /// The passphrase is not the one the repository's secrets are sealed
    /// with: accounts were left out.
    pub key_mismatch: bool,
    /// No passphrase is set: accounts were left out.
    pub no_passphrase: bool,
}

/// One sync of a repository.
pub struct Sync<'a> {
    pub transport: &'a mut dyn Transport,
    pub local: &'a mut dyn Local,
    pub keys: &'a mut Keys,
    /// The folder of the repository on the server.
    pub root: String,
    /// The server, user and folder, which a base belongs to.
    pub place: String,
    pub device: String,
    pub now: i64,
    /// The categories this device syncs; others pass through untouched.
    pub categories: BTreeSet<String>,
}

fn join(root: &str, name: &str) -> String {
    format!("{}/{name}", root.trim_end_matches('/'))
}

fn snapshot_name(version: i64) -> String {
    format!("snap_v{version}.erika")
}

fn patch_version(file: &str) -> Option<i64> {
    file.strip_prefix("patch_v")?
        .split('_')
        .next()?
        .parse()
        .ok()
}

fn safe(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

impl Sync<'_> {
    pub fn run(&mut self, base: Option<Base>) -> Result<(Outcome, Base), SyncError> {
        let manifest_path = join(&self.root, MANIFEST);
        let Some(manifest_bytes) = self.transport.read(&manifest_path)? else {
            self.transport.ensure_dir(&self.root)?;
            return self.create(&manifest_path);
        };
        let mut manifest = Manifest::from_document(&format::read(&manifest_bytes)?)?;
        let mut outcome = Outcome::default();
        let key = match &manifest.key {
            Some(params) => {
                let key = self.keys.open(params);
                if key.is_none() {
                    outcome.no_passphrase = self.keys.passphrase.is_none();
                    outcome.key_mismatch = self.keys.passphrase.is_some();
                }
                key
            }
            // Nothing is sealed yet: the first device with a passphrase
            // gives the repository its key.
            None => {
                let key = self.keys.create();
                outcome.no_passphrase = key.is_none();
                key
            }
        };
        let key_added = manifest.key.is_none() && key.is_some();
        let mut categories = self.categories.clone();
        if key.is_none() {
            categories.remove(crate::local::ACCOUNTS);
        }

        let base = base.filter(|base| {
            base.place == self.place
                && base.repository == manifest.repository
                && base.snapshot == manifest.snapshot
                && base.snapshot_sha256 == manifest.snapshot_sha256
        });
        let (mut remote, mut applied, agreed) = match base {
            Some(base) => (base.state.clone(), base.applied, Some(base.state)),
            None => {
                let path = join(&self.root, &manifest.snapshot_file);
                let bytes = self.transport.read(&path)?.ok_or_else(|| {
                    SyncError::new(format!("云端快照不存在：{}", manifest.snapshot_file))
                })?;
                if format::sha256(&bytes)[..] != manifest.snapshot_sha256[..] {
                    return Err(SyncError::new("云端快照校验失败"));
                }
                let snapshot = format::read(&bytes)?;
                if snapshot.kind != Kind::Snapshot
                    || snapshot.head_str("repository") != manifest.repository
                    || snapshot.head_int("snapshot") != Some(manifest.snapshot)
                {
                    return Err(SyncError::new("云端快照与索引不一致"));
                }
                (
                    snapshot.state.unwrap_or_default(),
                    manifest.snapshot_patches.clone(),
                    None,
                )
            }
        };

        let entries = self.patch_entries(&manifest)?;
        for entry in &entries {
            if applied.contains(&entry.id) {
                continue;
            }
            let Some(bytes) = self.transport.read(&join(&self.root, &entry.file))? else {
                continue;
            };
            if !entry.sha256.is_empty() && format::sha256(&bytes)[..] != entry.sha256[..] {
                return Err(SyncError::new("云端同步补丁校验失败"));
            }
            let patch = format::read(&bytes)?;
            if patch.kind != Kind::Patch || patch.head_str("id") != entry.id {
                return Err(SyncError::new("补丁索引与文件内容不一致"));
            }
            if patch.head_int("snapshot").unwrap_or(0) > manifest.snapshot {
                continue;
            }
            merge::apply(&mut remote, patch.ops.unwrap_or_default());
            applied.insert(entry.id.clone());
            outcome.downloaded += 1;
        }

        let local = merge::select(&self.local.collect(key.as_ref()), &categories);
        let theirs = merge::select(&remote, &categories);
        let merged_selected = match &agreed {
            Some(agreed) => {
                merge::merge_three(&merge::select(agreed, &categories), &theirs, &local)
            }
            None => merge::merge_first(&theirs, &local),
        };
        let mut merged = remote.clone();
        for category in &categories {
            match merged_selected.get(category) {
                Some(entries) => {
                    merged.insert(category.clone(), entries.clone());
                }
                None => {
                    merged.remove(category);
                }
            }
        }

        let for_local = merge::diff(&local, &merged_selected);
        if !for_local.is_empty() {
            self.local.apply(&for_local, key.as_ref());
        }
        outcome.restored = for_local.len();

        let for_remote = merge::diff(&theirs, &merged_selected);
        let mut uploaded = None;
        if !for_remote.is_empty() {
            let id = format!(
                "{}-{}-{}",
                self.now,
                crate::random_hex(4),
                safe(&self.device)
            );
            let mut patch = Document::new(Kind::Patch)
                .with_head("id", Value::text(&id))
                .with_head("snapshot", Value::Int(manifest.snapshot))
                .with_head("created", Value::Int(self.now))
                .with_head("device", Value::text(&self.device));
            outcome.uploaded = for_remote.len();
            patch.ops = Some(for_remote);
            let bytes = format::write(&patch);
            let file = format!("patch_v{}_{}.erika", manifest.snapshot, id);
            self.transport.write(&join(&self.root, &file), &bytes)?;
            applied.insert(id.clone());
            uploaded = Some(PatchEntry {
                id,
                file,
                sha256: format::sha256(&bytes).to_vec(),
                size: bytes.len() as i64,
                created: self.now,
                device: self.device.clone(),
            });
        }

        // Read the index again just before writing it: patches another
        // device listed meanwhile stay listed.
        if let Some(bytes) = self.transport.read(&manifest_path)? {
            let latest = Manifest::from_document(&format::read(&bytes)?)?;
            if latest.repository != manifest.repository
                || latest.snapshot != manifest.snapshot
                || (key_added && latest.key.is_some())
            {
                return Err(SyncError::new("云端在同步期间换了快照，请再同步一次"));
            }
            manifest = latest;
        }
        let known = manifest.patches.len();
        let mut by_id: BTreeMap<String, PatchEntry> = manifest
            .patches
            .drain(..)
            .chain(entries)
            .chain(uploaded.clone())
            .map(|entry| (entry.id.clone(), entry))
            .collect();
        manifest.patches = std::mem::take(&mut by_id).into_values().collect();
        manifest
            .patches
            .sort_by(|a, b| a.created.cmp(&b.created).then_with(|| a.id.cmp(&b.id)));
        if key_added && let Some(key) = &key {
            manifest.key = Some(key.params().clone());
        }
        if uploaded.is_some() || manifest.patches.len() != known || key_added {
            manifest.updated = self.now;
            self.transport
                .write(&manifest_path, &format::write(&manifest.document()))?;
        }

        let pending = manifest
            .patches
            .iter()
            .filter(|entry| !manifest.snapshot_patches.contains(&entry.id))
            .count();
        if pending >= PATCHES_BEFORE_SNAPSHOT {
            manifest = self.compact(&manifest_path, manifest, &merged, &applied)?;
        }
        manifest = self.clean(&manifest_path, manifest, &mut applied);

        Ok((
            outcome,
            Base {
                place: self.place.clone(),
                repository: manifest.repository,
                snapshot: manifest.snapshot,
                snapshot_sha256: manifest.snapshot_sha256,
                applied,
                state: merged,
            },
        ))
    }

    /// A new repository holding this device's state.
    fn create(&mut self, manifest_path: &str) -> Result<(Outcome, Base), SyncError> {
        let key = self.keys.create();
        let mut categories = self.categories.clone();
        if key.is_none() {
            categories.remove(crate::local::ACCOUNTS);
        }
        let state = merge::select(&self.local.collect(key.as_ref()), &categories);
        let repository = crate::random_hex(10);
        let snapshot_file = snapshot_name(1);
        let mut snapshot = Document::new(Kind::Snapshot)
            .with_head("repository", Value::text(&repository))
            .with_head("snapshot", Value::Int(1))
            .with_head("created", Value::Int(self.now))
            .with_head("device", Value::text(&self.device));
        snapshot.state = Some(state.clone());
        let bytes = format::write(&snapshot);
        self.transport
            .write(&join(&self.root, &snapshot_file), &bytes)?;
        let manifest = Manifest {
            repository: repository.clone(),
            key: key.as_ref().map(|key| key.params().clone()),
            snapshot: 1,
            snapshot_file,
            snapshot_sha256: format::sha256(&bytes).to_vec(),
            snapshot_patches: BTreeSet::new(),
            patches: Vec::new(),
            updated: self.now,
        };
        self.transport
            .write(manifest_path, &format::write(&manifest.document()))?;
        Ok((
            Outcome {
                created: true,
                no_passphrase: key.is_none(),
                ..Outcome::default()
            },
            Base {
                place: self.place.clone(),
                repository,
                snapshot: 1,
                snapshot_sha256: manifest.snapshot_sha256,
                applied: BTreeSet::new(),
                state,
            },
        ))
    }

    /// The patches the manifest lists, and those a device wrote but could
    /// not list before another wrote the manifest.
    fn patch_entries(&mut self, manifest: &Manifest) -> Result<Vec<PatchEntry>, SyncError> {
        let mut by_file: BTreeMap<String, PatchEntry> = manifest
            .patches
            .iter()
            .map(|entry| (entry.file.clone(), entry.clone()))
            .collect();
        // The manifest is the index; listing only finds stragglers, so a
        // server that cannot list does not stop the sync.
        let names = match self.transport.list(&self.root) {
            Ok(names) => names,
            Err(error) if error.status.is_none_or(|status| status >= 500) => Vec::new(),
            Err(error) => return Err(error),
        };
        for name in names {
            if !name.starts_with("patch_v")
                || !name.ends_with(".erika")
                || by_file.contains_key(&name)
            {
                continue;
            }
            let Some(bytes) = self.transport.read(&join(&self.root, &name))? else {
                continue;
            };
            let Ok(patch) = format::read(&bytes) else {
                continue;
            };
            by_file.insert(
                name.clone(),
                PatchEntry {
                    id: patch.head_str("id"),
                    file: name,
                    sha256: format::sha256(&bytes).to_vec(),
                    size: bytes.len() as i64,
                    created: patch.head_int("created").unwrap_or(0),
                    device: patch.head_str("device"),
                },
            );
        }
        let mut entries: Vec<PatchEntry> = by_file
            .into_values()
            .filter(|entry| !entry.id.is_empty())
            .collect();
        entries.sort_by(|a, b| a.created.cmp(&b.created).then_with(|| a.id.cmp(&b.id)));
        Ok(entries)
    }

    fn compact(
        &mut self,
        manifest_path: &str,
        manifest: Manifest,
        state: &State,
        applied: &BTreeSet<String>,
    ) -> Result<Manifest, SyncError> {
        let version = manifest.snapshot + 1;
        let mut snapshot = Document::new(Kind::Snapshot)
            .with_head("repository", Value::text(&manifest.repository))
            .with_head("snapshot", Value::Int(version))
            .with_head("created", Value::Int(self.now))
            .with_head("device", Value::text(&self.device));
        snapshot.state = Some(state.clone());
        let bytes = format::write(&snapshot);
        let file = snapshot_name(version);
        self.transport.write(&join(&self.root, &file), &bytes)?;
        let compacted = Manifest {
            snapshot: version,
            snapshot_file: file,
            snapshot_sha256: format::sha256(&bytes).to_vec(),
            snapshot_patches: applied.clone(),
            updated: self.now,
            ..manifest
        };
        self.transport
            .write(manifest_path, &format::write(&compacted.document()))?;
        let obsolete = version - SNAPSHOT_GENERATIONS;
        if obsolete >= 1 {
            // A server refusing to delete leaves the file; the new
            // snapshot is in charge either way.
            let _ = self
                .transport
                .delete(&join(&self.root, &snapshot_name(obsolete)));
        }
        Ok(compacted)
    }

    /// Delete a few patches that snapshots two generations old took in.
    /// Housekeeping: it never fails a sync.
    fn clean(
        &mut self,
        manifest_path: &str,
        manifest: Manifest,
        applied: &mut BTreeSet<String>,
    ) -> Manifest {
        let oldest_kept = manifest.snapshot - SNAPSHOT_GENERATIONS + 1;
        if oldest_kept <= 1 {
            return manifest;
        }
        let candidates: Vec<PatchEntry> = manifest
            .patches
            .iter()
            .filter(|entry| {
                patch_version(&entry.file).is_some_and(|version| version < oldest_kept)
                    && manifest.snapshot_patches.contains(&entry.id)
            })
            .take(CLEANUP_BATCH)
            .cloned()
            .collect();
        let deleted: BTreeSet<String> = candidates
            .into_iter()
            .filter(|entry| {
                self.transport
                    .delete(&join(&self.root, &entry.file))
                    .is_ok()
            })
            .map(|entry| entry.id)
            .collect();
        if deleted.is_empty() {
            return manifest;
        }
        let latest = match self.transport.read(manifest_path) {
            Ok(Some(bytes)) => match format::read(&bytes)
                .map_err(SyncError::from)
                .and_then(|document| Manifest::from_document(&document))
            {
                Ok(latest) => latest,
                Err(_) => return manifest,
            },
            _ => return manifest,
        };
        if latest.repository != manifest.repository
            || latest.snapshot != manifest.snapshot
            || latest.snapshot_sha256 != manifest.snapshot_sha256
        {
            return manifest;
        }
        let deleted: BTreeSet<String> = deleted
            .into_iter()
            .filter(|id| latest.snapshot_patches.contains(id))
            .collect();
        let cleaned = Manifest {
            patches: latest
                .patches
                .iter()
                .filter(|entry| !deleted.contains(&entry.id))
                .cloned()
                .collect(),
            snapshot_patches: latest
                .snapshot_patches
                .difference(&deleted)
                .cloned()
                .collect(),
            updated: self.now,
            ..latest.clone()
        };
        if self
            .transport
            .write(manifest_path, &format::write(&cleaned.document()))
            .is_err()
        {
            return latest;
        }
        for id in &deleted {
            applied.remove(id);
        }
        cleaned
    }
}

/// A repository in memory, for tests.
#[derive(Clone, Debug, Default)]
pub struct MemoryTransport {
    pub files: BTreeMap<String, Vec<u8>>,
}

impl Transport for MemoryTransport {
    fn ensure_dir(&mut self, _path: &str) -> Result<(), SyncError> {
        Ok(())
    }
    fn list(&mut self, path: &str) -> Result<Vec<String>, SyncError> {
        let prefix = format!("{}/", path.trim_end_matches('/'));
        Ok(self
            .files
            .keys()
            .filter_map(|name| name.strip_prefix(&prefix))
            .filter(|name| !name.contains('/'))
            .map(String::from)
            .collect())
    }
    fn read(&mut self, path: &str) -> Result<Option<Vec<u8>>, SyncError> {
        Ok(self.files.get(path).cloned())
    }
    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), SyncError> {
        self.files.insert(path.into(), bytes.to_vec());
        Ok(())
    }
    fn delete(&mut self, path: &str) -> Result<(), SyncError> {
        self.files.remove(path);
        Ok(())
    }
}
