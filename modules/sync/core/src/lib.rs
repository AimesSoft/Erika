//! Engine of the sync module: everything the user keeps in Erika in one
//! binary file (see `format`), exported and imported, or synced through a
//! WebDAV folder as NipaPlay's incremental sync does (see `repository`).
//!
//! What is carried, and what stays, is in `local`; how the accounts'
//! secrets are sealed, in `seal`. Extensions such as plugins keep their
//! data with `erika_store::Db::set_extension(id, json)`: any JSON under a
//! stable id, carried in the `extensions` category, and read back with
//! `Db::extension(id)` after the `restored` event.

pub mod format;
pub mod local;
pub mod merge;
pub mod repository;
pub mod seal;
pub mod value;
#[cfg(not(target_arch = "wasm32"))]
pub mod webdav;

use bern::CommandError;
use bern::media::MediaSource;
use bern::storage::Store;
use erika_store::Db;
use erika_sync_protocol::sync::{
    self, State, SyncInterval, SyncKind, SyncNote, SyncPart, SyncPhase, SyncResult, SyncWarning,
};
use format::{Document, Kind};
use local::{Applied, Device, Scope, SystemVault, Vault};
use repository::{Base, Keys, Outcome};
use std::sync::{Arc, Mutex};
use value::Value;

/// Settings of the sync itself.
const URL: &str = "sync.webdav.url";
const USER: &str = "sync.webdav.user";
const PATH: &str = "sync.webdav.path";
const AUTO: &str = "sync.auto";
const INTERVAL: &str = "sync.interval";
const LAST_SYNC: &str = "sync.last_at";
const HAS_PASSPHRASE: &str = "sync.passphrase_set";
/// Where a repository goes when the user names none.
pub const DEFAULT_PATH: &str = "/Erika/sync";
/// What this device last agreed with the repository, in its data folder.
const BASE_FILE: &str = "sync-base.erika";

/// Bytes nobody can guess: salts and ids. The system's randomness seeds
/// the hasher behind them, with the time and a count mixed in.
pub fn random_bytes(len: usize) -> Vec<u8> {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        let mut seed = Vec::new();
        for _ in 0..4 {
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_u64(COUNT.fetch_add(1, Ordering::Relaxed));
            seed.extend_from_slice(&hasher.finish().to_le_bytes());
        }
        seed.extend_from_slice(&erika_store::now().to_le_bytes());
        seed.extend_from_slice(&(&seed as *const _ as usize).to_le_bytes());
        out.extend_from_slice(&format::sha256(&seed));
    }
    out.truncate(len);
    out
}

pub fn random_hex(len: usize) -> String {
    format::hex(&random_bytes(len))
}

fn app_name() -> String {
    format!("Erika {}", env!("CARGO_PKG_VERSION"))
}

/// The name of an export made at `now` (local seconds).
pub fn export_name(now: bern::date::Now) -> String {
    let (date, time) = local_time(now.unix + now.offset);
    format!("erika-backup-{date}-{time}.{}", format::EXTENSION)
}

/// `seconds` since 1970 as `yyyymmdd` and `hhmmss`.
fn local_time(seconds: i64) -> (String, String) {
    let (year, month, day) = civil(seconds.div_euclid(86_400));
    let second = seconds.rem_euclid(86_400);
    (
        format!("{year:04}{month:02}{day:02}"),
        format!(
            "{:02}{:02}{:02}",
            second / 3600,
            second / 60 % 60,
            second % 60
        ),
    )
}

/// Days since 1970 as a date (Howard Hinnant's `civil_from_days`).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// When something happened, as the user reads it.
fn shown_time(unix: i64, offset: i64) -> String {
    let local = unix + offset;
    let (year, month, day) = civil(local.div_euclid(86_400));
    let second = local.rem_euclid(86_400);
    format!(
        "{year}-{month:02}-{day:02} {:02}:{:02}",
        second / 3600,
        second / 60 % 60
    )
}

/// Everything this device keeps, as one export file. With a passphrase
/// the accounts come too, sealed with its key under a new salt.
pub fn export(db: &Db, store: &Store, vault: &dyn Vault, keys: &mut Keys, device: &str) -> Vec<u8> {
    let key = keys.create();
    let state = Device::new(db, store, vault, Scope::Export).collect(key.as_ref());
    let mut document = Document::new(Kind::Export)
        .with_head("created", Value::Int(erika_store::now()))
        .with_head("device", Value::text(device))
        .with_head("app", Value::text(app_name()));
    document.key = key.map(|key| key.params().clone());
    document.state = Some(state);
    format::write(&document)
}

/// What importing a file did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Imported {
    pub applied: Applied,
    /// The file has accounts and no passphrase is set here.
    pub no_passphrase: bool,
    /// The file has accounts sealed with another passphrase.
    pub wrong_passphrase: bool,
}

/// Take in an export: what this device lacks comes in, and where both
/// have a record the file's wins unless this device's is later. Nothing
/// is removed here.
pub fn import(
    bytes: &[u8],
    db: &Db,
    store: &Store,
    vault: &dyn Vault,
    keys: &mut Keys,
) -> Result<Imported, String> {
    let document = format::read(bytes).map_err(|error| error.to_string())?;
    if !matches!(document.kind, Kind::Export | Kind::Snapshot) {
        return Err("这个文件不是 Erika 的导出文件".into());
    }
    let mut incoming = document.state.unwrap_or_default();
    let mut imported = Imported::default();
    let key = match &document.key {
        Some(params) => {
            let key = keys.open(params);
            if key.is_none() {
                imported.no_passphrase = keys.passphrase.is_none();
                imported.wrong_passphrase = keys.passphrase.is_some();
                incoming.remove(local::ACCOUNTS);
            }
            key
        }
        None => None,
    };
    let mut device = Device::new(db, store, vault, Scope::Export);
    let mine = device.collect(key.as_ref());
    let merged = merge::merge_first(&incoming, &mine);
    let ops = merge::diff(&mine, &merged);
    device.apply(&ops, key.as_ref());
    imported.applied = device.applied;
    Ok(imported)
}

/// How many records of each kind `state` holds.
fn parts(state: &format::State, accounts: i64) -> Vec<SyncPart> {
    let count = |category: &str| {
        state
            .get(category)
            .map_or(0, |entries| entries.len() as i64)
    };
    [
        (
            SyncKind::Settings,
            count(local::SETTINGS) + count(local::STORE),
        ),
        (SyncKind::Accounts, accounts),
        (SyncKind::Sources, count(local::SOURCES)),
        (SyncKind::Links, count(local::LINKS)),
        (SyncKind::Progress, count(local::PROGRESS)),
        (SyncKind::Recents, count(local::RECENTS)),
        (SyncKind::Watched, count(local::WATCHED)),
        (
            SyncKind::Anime,
            count(local::ANIME) + count(local::EPISODES),
        ),
        (SyncKind::Marks, count(local::MARKS)),
        (SyncKind::Extensions, count(local::EXTENSIONS)),
    ]
    .into_iter()
    .enumerate()
    .map(|(id, (kind, count))| SyncPart {
        id: id as i64,
        kind,
        count,
    })
    .collect()
}

/// What work on another thread came back with.
enum Done {
    Synced(Result<(Outcome, Applied), String>),
    Tested(Result<(), String>),
    Exported(Result<String, String>),
    Imported(Result<Imported, String>),
    Parts(Vec<SyncPart>),
    /// The passphrase was saved, or could not be.
    Passphrase(Result<bool, String>),
}

/// Run `work` on another thread; what it returns waits in `inbox` and the
/// engine is woken to take it in. The browser has no threads: there the
/// work runs at once.
fn in_background(inbox: &Arc<Mutex<Vec<Done>>>, work: impl FnOnce() -> Done + Send + 'static) {
    let inbox = Arc::clone(inbox);
    let waker = bern::wake::waker();
    let run = move || {
        let done = work();
        inbox
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(done);
        if let Some(waker) = waker {
            waker.wake();
        }
    };
    if cfg!(target_arch = "wasm32") {
        run();
    } else {
        std::thread::spawn(run);
    }
}

/// The WebDAV folder, as the settings keep it.
#[derive(Clone)]
struct Place {
    url: String,
    user: String,
    path: String,
}

impl Place {
    fn name(&self) -> String {
        format!("{}|{}|{}", self.url, self.user, self.path)
    }
}

fn interval_seconds(interval: &SyncInterval) -> i64 {
    match interval {
        SyncInterval::Quarter => 15 * 60,
        SyncInterval::Half => 30 * 60,
        SyncInterval::Hour => 60 * 60,
        SyncInterval::SixHours => 6 * 60 * 60,
    }
}

fn interval_of(text: Option<&str>) -> SyncInterval {
    match text {
        Some("Quarter") => SyncInterval::Quarter,
        Some("Hour") => SyncInterval::Hour,
        Some("SixHours") => SyncInterval::SixHours,
        _ => SyncInterval::Half,
    }
}

/// The keys of the passphrase this device keeps, read from the credential
/// store when work needs them.
fn device_keys(cache: &Arc<Mutex<Keys>>) -> std::sync::MutexGuard<'_, Keys> {
    let mut keys = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let passphrase = SystemVault.read(local::PASSPHRASE);
    if keys.passphrase != passphrase {
        *keys = Keys::new(passphrase);
    }
    keys
}

/// Sync with the WebDAV folder at `place`, from another thread.
#[cfg(not(target_arch = "wasm32"))]
fn sync_with(place: Place, keys: &Arc<Mutex<Keys>>) -> Result<(Outcome, Applied), String> {
    let db = Db::open();
    let store = Store::host();
    let password = SystemVault.read(local::WEBDAV_PASSWORD).unwrap_or_default();
    let mut transport =
        webdav::WebDav::new(&place.url, &place.user, &password).map_err(|error| error.message)?;
    let base_path = bern::storage::data_dir().map(|dir| dir.join(BASE_FILE));
    let base = base_path
        .as_ref()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| Base::from_bytes(&bytes));
    let device_id = db.setting("device.id").unwrap_or_else(|| "device".into());
    let mut device = Device::new(&db, &store, &SystemVault, Scope::Repository);
    let mut keys = device_keys(keys);
    let (outcome, base) = repository::Sync {
        transport: &mut transport,
        local: &mut device,
        keys: &mut keys,
        root: place.path.clone(),
        place: place.name(),
        device: device_id,
        now: erika_store::now(),
        categories: local::CATEGORIES
            .iter()
            .filter(|category| **category != local::SYNC)
            .map(|category| category.to_string())
            .collect(),
    }
    .run(base)
    .map_err(|error| error.message)?;
    if let Some(path) = base_path
        && let Err(error) = std::fs::write(&path, base.to_bytes())
    {
        eprintln!("erika-sync: keep the sync base: {error}");
    }
    db.set_setting(LAST_SYNC, Some(&erika_store::now().to_string()));
    Ok((outcome, device.applied))
}

#[cfg(target_arch = "wasm32")]
fn sync_with(_place: Place, _keys: &Arc<Mutex<Keys>>) -> Result<(Outcome, Applied), String> {
    Err("网页版不能连接 WebDAV".into())
}

#[cfg(not(target_arch = "wasm32"))]
fn test_place(place: Place) -> Result<(), String> {
    use repository::Transport;
    let password = SystemVault.read(local::WEBDAV_PASSWORD).unwrap_or_default();
    let mut transport =
        webdav::WebDav::new(&place.url, &place.user, &password).map_err(|error| error.message)?;
    transport
        .ensure_dir(&place.path)
        .map_err(|error| error.message)?;
    let probe = format!("{}/.erika-probe", place.path.trim_end_matches('/'));
    transport
        .write(&probe, b"erika")
        .map_err(|error| error.message)?;
    let _ = transport.delete(&probe);
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn test_place(_place: Place) -> Result<(), String> {
    Err("网页版不能连接 WebDAV".into())
}

pub struct SyncEngine {
    state: State,
    db: Db,
    inbox: Arc<Mutex<Vec<Done>>>,
    keys: Arc<Mutex<Keys>>,
    /// When the automatic sync runs next.
    next_auto: i64,
    /// The sync running was started by the clock: it speaks only of
    /// failures.
    quiet: bool,
    started: bool,
}

impl Default for SyncEngine {
    fn default() -> Self {
        let mut engine = SyncEngine {
            state: State {
                webdav_available: !cfg!(target_arch = "wasm32"),
                url: String::new(),
                user: String::new(),
                path: DEFAULT_PATH.into(),
                configured: false,
                editing: false,
                has_passphrase: false,
                auto: false,
                interval: SyncInterval::Half,
                phase: SyncPhase::Idle,
                busy: false,
                result: SyncResult::Never,
                notes: Vec::new(),
                last_sync: String::new(),
                parts: Vec::new(),
                notice: None,
                notice_revision: 0,
            },
            db: Db::open(),
            inbox: Arc::default(),
            keys: Arc::default(),
            next_auto: 0,
            quiet: false,
            started: false,
        };
        engine.load_settings();
        engine
    }
}

impl SyncEngine {
    fn load_settings(&mut self) {
        let setting = |key| self.db.setting(key).filter(|value| !value.is_empty());
        self.state.url = setting(URL).unwrap_or_default();
        self.state.user = setting(USER).unwrap_or_default();
        self.state.path = setting(PATH).unwrap_or_else(|| DEFAULT_PATH.into());
        self.state.configured = !self.state.url.is_empty();
        self.state.auto = setting(AUTO).as_deref() == Some("1");
        self.state.interval = interval_of(setting(INTERVAL).as_deref());
        self.state.has_passphrase = setting(HAS_PASSPHRASE).as_deref() == Some("1");
        self.state.last_sync = setting(LAST_SYNC)
            .and_then(|at| at.parse().ok())
            .map(|at| shown_time(at, bern::date::now().offset))
            .unwrap_or_default();
    }

    fn place(&self) -> Option<Place> {
        self.state.configured.then(|| Place {
            url: self.state.url.clone(),
            user: self.state.user.clone(),
            path: self.state.path.clone(),
        })
    }

    fn require_idle(&self) -> Result<(), CommandError> {
        if self.state.busy {
            Err(CommandError::new("sync_busy", "正在同步，请稍候"))
        } else {
            Ok(())
        }
    }

    fn begin(&mut self, phase: SyncPhase) {
        self.state.busy = true;
        self.state.phase = phase;
        self.quiet = false;
    }

    fn finish(&mut self, result: SyncResult, warnings: Vec<SyncWarning>) {
        self.state.busy = false;
        self.state.phase = SyncPhase::Idle;
        self.state.result = result.clone();
        self.state.notes = warnings
            .into_iter()
            .enumerate()
            .map(|(id, warning)| SyncNote {
                id: id as i64,
                warning,
            })
            .collect();
        if !self.quiet || matches!(result, SyncResult::Failed { .. }) {
            self.state.notice = Some(result);
            self.state.notice_revision += 1;
        }
        self.quiet = false;
    }

    fn start_sync(&mut self, quiet: bool) {
        let Some(place) = self.place() else { return };
        self.begin(SyncPhase::Syncing);
        self.quiet = quiet;
        let keys = Arc::clone(&self.keys);
        in_background(&self.inbox, move || Done::Synced(sync_with(place, &keys)));
    }

    fn start_parts(&mut self) {
        in_background(&self.inbox, || {
            let db = Db::open();
            let state =
                Device::new(&db, &Store::host(), &local::NoVault, Scope::Export).collect(None);
            let accounts = ["dandanplay.user", "bangumi.user"]
                .iter()
                .filter(|key| db.setting(key).is_some_and(|user| !user.is_empty()))
                .count() as i64;
            Done::Parts(parts(&state, accounts))
        });
    }

    /// Tell the shell and modules what came in.
    fn announce(&mut self, applied: &Applied) -> Vec<SyncWarning> {
        let mut warnings = Vec::new();
        if applied.changes == 0 {
            return warnings;
        }
        if applied.sync {
            self.load_settings();
        }
        sync::emit::restored(
            applied.settings,
            applied.accounts,
            applied.library || applied.marks,
            applied.history,
            applied.new_sources.clone(),
        );
        // The shell, the library and the anime pages read theirs anew;
        // other modules read their settings as they start.
        if applied.settings {
            warnings.push(SyncWarning::Restart);
        }
        self.start_parts();
        warnings
    }

    fn take_done(&mut self) {
        let done = std::mem::take(
            &mut *self
                .inbox
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        for done in done {
            match done {
                Done::Synced(Ok((outcome, applied))) => {
                    let mut warnings = Vec::new();
                    if outcome.no_passphrase {
                        warnings.push(SyncWarning::NoPassphrase);
                    }
                    if outcome.key_mismatch || applied.locked > 0 {
                        warnings.push(SyncWarning::WrongPassphrase);
                    }
                    warnings.extend(self.announce(&applied));
                    self.load_settings();
                    self.next_auto = erika_store::now() + interval_seconds(&self.state.interval);
                    let result = if outcome.created {
                        SyncResult::Created
                    } else {
                        SyncResult::Synced {
                            downloaded: outcome.downloaded as i64,
                            uploaded: outcome.uploaded as i64,
                            restored: outcome.restored as i64,
                        }
                    };
                    self.finish(result, warnings);
                }
                Done::Synced(Err(detail)) => {
                    // Try again sooner than a whole interval, not at once.
                    self.next_auto = erika_store::now() + 5 * 60;
                    self.finish(SyncResult::Failed { detail }, Vec::new());
                }
                Done::Tested(result) => self.finish(
                    match result {
                        Ok(()) => SyncResult::Connected,
                        Err(detail) => SyncResult::Failed { detail },
                    },
                    Vec::new(),
                ),
                Done::Exported(result) => {
                    let warnings = match &result {
                        Ok(_) if !self.state.has_passphrase => vec![SyncWarning::NoPassphrase],
                        _ => Vec::new(),
                    };
                    self.finish(
                        match result {
                            Ok(path) => SyncResult::Exported { path },
                            Err(detail) => SyncResult::Failed { detail },
                        },
                        warnings,
                    );
                }
                Done::Imported(Ok(imported)) => {
                    let mut warnings = Vec::new();
                    if imported.no_passphrase {
                        warnings.push(SyncWarning::NoPassphrase);
                    }
                    if imported.wrong_passphrase || imported.applied.locked > 0 {
                        warnings.push(SyncWarning::WrongPassphrase);
                    }
                    warnings.extend(self.announce(&imported.applied));
                    self.finish(
                        SyncResult::Imported {
                            changes: imported.applied.changes as i64,
                        },
                        warnings,
                    );
                }
                Done::Imported(Err(detail)) => {
                    self.finish(SyncResult::Failed { detail }, Vec::new())
                }
                Done::Parts(parts) => self.state.parts = parts,
                Done::Passphrase(result) => {
                    self.state.busy = false;
                    self.state.phase = SyncPhase::Idle;
                    match result {
                        Ok(set) => {
                            self.state.has_passphrase = set;
                            self.db.set_setting(HAS_PASSPHRASE, set.then_some("1"));
                        }
                        Err(detail) => self.finish(SyncResult::Failed { detail }, Vec::new()),
                    }
                }
            }
        }
    }
}

impl sync::Commands for SyncEngine {
    fn set_webdav(
        &mut self,
        url: String,
        user: String,
        password: String,
        path: String,
    ) -> Result<(), CommandError> {
        self.require_idle()?;
        let url = match url.trim().trim_end_matches('/') {
            "" => self.state.url.clone(),
            url => url.to_string(),
        };
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(CommandError::new(
                "invalid_url",
                "WebDAV 地址需要以 http:// 或 https:// 开头",
            ));
        }
        let user = match user.trim() {
            "" => self.state.user.clone(),
            user => user.to_string(),
        };
        let path = match path.trim().trim_end_matches('/') {
            "" if self.state.configured => self.state.path.clone(),
            "" => DEFAULT_PATH.to_string(),
            path if path.starts_with('/') => path.to_string(),
            path => format!("/{path}"),
        };
        if !password.is_empty() {
            bern::secrets::write(local::WEBDAV_PASSWORD, &password)?;
        }
        self.db.set_setting(URL, Some(&url));
        self.db.set_setting(USER, Some(&user));
        self.db.set_setting(PATH, Some(&path));
        self.load_settings();
        self.state.editing = false;
        self.state.result = SyncResult::Never;
        self.state.notes.clear();
        Ok(())
    }

    fn edit_webdav(&mut self, editing: bool) -> Result<(), CommandError> {
        self.state.editing = editing;
        Ok(())
    }

    fn clear_webdav(&mut self) -> Result<(), CommandError> {
        self.require_idle()?;
        for key in [URL, USER, PATH, AUTO, LAST_SYNC] {
            self.db.set_setting(key, None);
        }
        bern::secrets::delete(local::WEBDAV_PASSWORD)?;
        if let Some(dir) = bern::storage::data_dir() {
            let _ = std::fs::remove_file(dir.join(BASE_FILE));
        }
        self.load_settings();
        self.state.editing = false;
        self.state.result = SyncResult::Never;
        self.state.notes.clear();
        Ok(())
    }

    fn test_webdav(&mut self) -> Result<(), CommandError> {
        self.require_idle()?;
        let place = self
            .place()
            .ok_or_else(|| CommandError::new("not_configured", "请先填写 WebDAV 服务器"))?;
        self.begin(SyncPhase::Testing);
        in_background(&self.inbox, move || Done::Tested(test_place(place)));
        Ok(())
    }

    fn sync_now(&mut self) -> Result<(), CommandError> {
        self.require_idle()?;
        if !self.state.configured {
            return Err(CommandError::new(
                "not_configured",
                "请先填写 WebDAV 服务器",
            ));
        }
        self.start_sync(false);
        Ok(())
    }

    fn set_auto(&mut self, on: bool) -> Result<(), CommandError> {
        self.db.set_setting(AUTO, Some(if on { "1" } else { "0" }));
        self.state.auto = on;
        self.next_auto = erika_store::now();
        Ok(())
    }

    fn set_interval(&mut self, interval: SyncInterval) -> Result<(), CommandError> {
        self.db
            .set_setting(INTERVAL, Some(&format!("{interval:?}")));
        self.state.interval = interval;
        Ok(())
    }

    fn set_passphrase(&mut self, passphrase: String) -> Result<(), CommandError> {
        self.require_idle()?;
        if !passphrase.is_empty() && passphrase.chars().count() < 6 {
            return Err(CommandError::new(
                "passphrase_short",
                "同步口令至少 6 个字符",
            ));
        }
        self.begin(SyncPhase::Deriving);
        let keys = Arc::clone(&self.keys);
        in_background(&self.inbox, move || {
            let saved = if passphrase.is_empty() {
                bern::secrets::delete(local::PASSPHRASE).map(|()| false)
            } else {
                bern::secrets::write(local::PASSPHRASE, &passphrase).map(|()| true)
            };
            // Keys of the old passphrase go.
            drop(device_keys(&keys));
            Done::Passphrase(saved.map_err(|error| format!("无法保存同步口令：{}", error.message)))
        });
        Ok(())
    }

    fn export_to(&mut self, folder: MediaSource) -> Result<(), CommandError> {
        self.require_idle()?;
        let folder = bern::files::file_path(&folder.url)
            .ok_or_else(|| CommandError::new("invalid_folder", "请选择本机上的文件夹"))?;
        self.begin(SyncPhase::Exporting);
        let keys = Arc::clone(&self.keys);
        in_background(&self.inbox, move || {
            let db = Db::open();
            let device = db.setting("device.id").unwrap_or_default();
            let bytes = export(
                &db,
                &Store::host(),
                &SystemVault,
                &mut device_keys(&keys),
                &device,
            );
            let path = folder.join(export_name(bern::date::now()));
            Done::Exported(
                std::fs::write(&path, bytes)
                    .map(|()| path.display().to_string())
                    .map_err(|error| format!("无法写入 {}：{error}", path.display())),
            )
        });
        Ok(())
    }

    fn import_from(&mut self, file: MediaSource) -> Result<(), CommandError> {
        self.require_idle()?;
        let mut source = bern::files::open(&file.url)?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut source, &mut bytes)
            .map_err(|error| CommandError::new("unreadable_file", error.to_string()))?;
        self.begin(SyncPhase::Importing);
        let keys = Arc::clone(&self.keys);
        in_background(&self.inbox, move || {
            let db = Db::open();
            let result = import(
                &bytes,
                &db,
                &Store::host(),
                &SystemVault,
                &mut device_keys(&keys),
            );
            Done::Imported(result)
        });
        Ok(())
    }

    fn refresh_parts(&mut self) -> Result<(), CommandError> {
        self.start_parts();
        Ok(())
    }

    fn tick(&mut self) -> Result<(), CommandError> {
        if self.state.auto
            && self.state.configured
            && !self.state.busy
            && erika_store::now() >= self.next_auto
        {
            self.start_sync(true);
        }
        Ok(())
    }
}

impl sync::Engine for SyncEngine {
    fn state(&self) -> &State {
        &self.state
    }

    /// Work done on other threads comes back; once as the app starts, what
    /// an export would hold is counted.
    fn wake(&mut self) {
        if !self.started {
            self.started = true;
            self.start_parts();
        }
        self.take_done();
    }
}

#[cfg(test)]
mod tests;
