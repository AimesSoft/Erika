//! Plugins: scripts the user installs from the market or imports, which
//! extend Erika with what they declared they may do. NipaPlay's plugins run
//! as they did there, their version requirement dropped while Erika has no
//! market of its own; Erika's come as a folder of `pack.json`, the script
//! and `icon.png` (see `pack.rs`).
//!
//! Each enabled plugin runs in a JavaScript context of its own. It cannot
//! call the engine back: before each call the engine hands it what it may
//! read, and afterwards takes the calls it queued (`host.js`). The
//! playback half of the interface, `player` and `danmaku`, is liberika's.

mod hooks;
mod links;
mod pack;
mod runtime;
pub(crate) mod store;

use crate::{ErikaEngine, in_background, markdown};
use bern::CommandError;
use bern::media::MediaSource;
use erika_protocol::erika::{self, *};
use liberika_plugin::{Call, PlayerState, PlayerStatus};
use pack::Pack;
use runtime::{Purpose, Runtime};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// What the plugins keep between runs: which are on, their settings and
/// storage, and whether one unlocked the downloader.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Saved {
    pub enabled: Vec<String>,
    pub text: BTreeMap<String, BTreeMap<String, String>>,
    pub switches: BTreeMap<String, BTreeMap<String, bool>>,
    pub storage: BTreeMap<String, BTreeMap<String, String>>,
    pub downloader: bool,
}

impl Saved {
    pub fn to_value(&self) -> Value {
        json!({
            "enabled": self.enabled,
            "text": self.text,
            "switches": self.switches,
            "storage": self.storage,
            "downloader": self.downloader,
        })
    }

    pub fn from_value(value: &Value) -> Saved {
        let read = |key: &str| value.get(key).cloned().unwrap_or(Value::Null);
        Saved {
            enabled: serde_json::from_value(read("enabled")).unwrap_or_default(),
            text: serde_json::from_value(read("text")).unwrap_or_default(),
            switches: serde_json::from_value(read("switches")).unwrap_or_default(),
            storage: serde_json::from_value(read("storage")).unwrap_or_default(),
            downloader: value.get("downloader").and_then(Value::as_bool).unwrap_or(false),
        }
    }

    /// What was kept: in the database, which the account sync carries, or
    /// in the plugins folder where an earlier Erika kept it.
    fn load(db: &erika_store::Db) -> Saved {
        db.extension(EXTENSION)
            .and_then(|text| serde_json::from_str(&text).ok())
            .or_else(|| {
                let path = store::folder()?.join("state.json");
                serde_json::from_slice(&std::fs::read(path).ok()?).ok()
            })
            .map(|value: Value| Saved::from_value(&value))
            .unwrap_or_default()
    }

    fn save(&self, db: &erika_store::Db) {
        db.set_extension(EXTENSION, Some(&self.to_value().to_string()));
    }
}

/// The plugins' part of the database's extensions.
const EXTENSION: &str = "plugins";

/// How much of a video's opening `extend.openingProfile` measures when the
/// plugin does not say.
const OPENING_SECONDS: f64 = 10.0;

/// What a running plugin added to Erika's controls (`extend`).
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Extensions {
    pub speeds: Vec<f64>,
    /// Names and `#RRGGBB` colors for sending danmaku.
    pub colors: Vec<(String, String)>,
    pub hex_input: bool,
    pub watched_button: bool,
    pub quick_select: bool,
    /// How many seconds of each video's opening to measure, 0 when no
    /// plugin asked.
    pub opening: f64,
}

/// An installed plugin.
struct Installed {
    pack: Pack,
    dir: PathBuf,
    /// `pluginUIEntries` and `pluginBlockWords` as the script left them.
    entries: Vec<Value>,
    block_words: Vec<String>,
    loaded: bool,
    error: String,
}

/// Work done on other threads.
enum Done {
    Market(Vec<store::Listing>, Vec<String>),
    Installed(String, Result<Pack, String>),
    Fetched(String, u64, Result<Value, String>),
    /// The first seconds of the video at the address, measured.
    #[cfg(any(target_os = "macos", target_os = "windows", all(target_os = "linux", not(target_env = "ohos"))))]
    Opening(String, Result<Value, String>),
    /// A request of a link's resolution answered.
    Resolved(u64, Result<Value, String>),
    /// A plugin's `README.md`, with where it was read from.
    Readme(String, Result<(String, String), String>),
}

pub(crate) struct Plugins {
    installed: Vec<Installed>,
    listings: Vec<store::Listing>,
    pub(crate) saved: Saved,
    runtimes: HashMap<String, Runtime>,
    inbox: Arc<Mutex<Vec<Done>>>,
    loading: bool,
    error: String,
    busy: Vec<String>,
    started: bool,
    /// Enabled plugins whose context could not start yet, as before the
    /// system's JavaScript host runs.
    waiting: Vec<String>,
    /// What `danmaku.addFilter` added.
    filters: Vec<String>,
    /// The plugin whose settings are open.
    open: Option<String>,
    /// What handlers' promises answer, by their tags.
    later: HashMap<u64, Purpose>,
    /// What each running plugin added to the controls.
    extensions: HashMap<String, Extensions>,
    next_tag: u64,
    /// The download whose folders the plugins are asked for.
    suggesting: u64,
    /// The link a plugin is resolving.
    resolution: Option<links::Resolution>,
    /// The comments of what plays, by track, and the track passing along
    /// the plugins.
    tracks: BTreeMap<String, hooks::Track>,
    last_track: String,
    chain: Option<hooks::Chain>,
    waiting_tracks: Vec<String>,
    /// Whether the anime module shows the episodes' mark buttons.
    watched_button: bool,
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    processes: HashMap<String, std::process::Child>,
}

impl Default for Plugins {
    fn default() -> Self {
        Plugins {
            installed: Vec::new(),
            listings: Vec::new(),
            saved: Saved::default(),
            runtimes: HashMap::new(),
            inbox: Arc::default(),
            loading: false,
            error: String::new(),
            busy: Vec::new(),
            started: false,
            waiting: Vec::new(),
            filters: Vec::new(),
            open: None,
            later: HashMap::new(),
            extensions: HashMap::new(),
            next_tag: 1,
            suggesting: 0,
            resolution: None,
            tracks: BTreeMap::new(),
            last_track: String::new(),
            chain: None,
            waiting_tracks: Vec::new(),
            watched_button: false,
            #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
            processes: HashMap::new(),
        }
    }
}

impl Plugins {
    fn find(&self, id: &str) -> Option<&Installed> {
        self.installed.iter().find(|plugin| plugin.pack.id == id)
    }

    fn find_mut(&mut self, id: &str) -> Option<&mut Installed> {
        self.installed.iter_mut().find(|plugin| plugin.pack.id == id)
    }

    /// The plugins whose contexts run.
    fn running(&self) -> Vec<String> {
        self.runtimes.keys().cloned().collect()
    }

    fn invoke(&mut self, id: &str, name: &str, arg: Option<&Value>, snapshot: &Value, tag: u64, purpose: Purpose) {
        if let Some(runtime) = self.runtimes.get_mut(id) {
            if tag != 0 {
                self.later.insert(tag, purpose.clone());
            }
            runtime.invoke(name, arg, snapshot, tag, purpose);
        }
    }

    fn enabled(&self, id: &str) -> bool {
        self.saved.enabled.iter().any(|enabled| enabled == id)
    }

    /// The block-list entries of every running plugin, and what plugins
    /// added since.
    pub(crate) fn block_words(&self) -> Vec<String> {
        self.installed
            .iter()
            .filter(|plugin| plugin.loaded && self.runtimes.contains_key(&plugin.pack.id))
            .flat_map(|plugin| plugin.block_words.iter().cloned())
            .chain(self.filters.iter().cloned())
            .collect()
    }

    /// What the running plugins added to the controls, together.
    pub(crate) fn extensions(&self) -> Extensions {
        let mut all = Extensions::default();
        for extension in self.extensions.values() {
            all.speeds.extend(&extension.speeds);
            for color in &extension.colors {
                if !all.colors.contains(color) {
                    all.colors.push(color.clone());
                }
            }
            all.hex_input |= extension.hex_input;
            all.watched_button |= extension.watched_button;
            all.quick_select |= extension.quick_select;
            all.opening = all.opening.max(extension.opening);
        }
        all
    }
}

/// The settings sheet with no plugin open.
/// No plugin's page open.
pub(crate) fn closed_info() -> PluginInfo {
    PluginInfo {
        open: false,
        plugin: None,
        loading: false,
        error: String::new(),
        blocks: Vec::new(),
    }
}

/// Where a plugin's `README.md` is: in its folder once installed, else
/// beside what the market lists, or in the package this build carries.
enum Readme {
    File(PathBuf),
    Address(String),
    Package(&'static [u8]),
}

impl Readme {
    fn of(installed: Option<&Installed>, listing: Option<&store::Listing>) -> Option<Readme> {
        if let Some(path) = installed.map(|plugin| plugin.dir.join("README.md")).filter(|path| path.is_file()) {
            return Some(Readme::File(path));
        }
        Some(match &listing?.files {
            store::Files::Local(package) => Readme::Package(package),
            store::Files::Folder(address) => Readme::Address(format!("{address}/README.md")),
            store::Files::Script(address) => Readme::Address(store::beside(address, "README.md")),
        })
    }

    /// Its text, and the address its relative links resolve beside.
    fn read(self) -> Result<(String, String), String> {
        let (bytes, base) = match self {
            Readme::File(path) => {
                let base = MediaSource::file(path.clone()).url;
                (std::fs::read(&path).map_err(|error| error.to_string())?, base)
            }
            Readme::Address(address) => (store::get(&address)?, address),
            Readme::Package(package) => (store::package_file(package, "README.md")?, String::new()),
        };
        Ok((String::from_utf8_lossy(&bytes).into_owned(), base))
    }
}

pub(crate) fn closed_panel() -> PluginPanel {
    PluginPanel {
        open: false,
        id: String::new(),
        name: String::new(),
        entries: Vec::new(),
        has_message: false,
        message_title: String::new(),
        message: String::new(),
    }
}

fn strings(value: Option<&Value>) -> Option<Vec<String>> {
    value.and_then(Value::as_array).map(|list| {
        list.iter()
            .filter_map(|item| match item {
                Value::String(text) => Some(text.trim().to_string()),
                Value::Number(number) => Some(number.to_string()),
                _ => None,
            })
            .filter(|text| !text.is_empty())
            .collect()
    })
}

fn text_of(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

impl ErikaEngine {
    /// Read what is installed and start the enabled ones; read the market.
    fn start_plugins(&mut self) {
        self.plugins.started = true;
        self.plugins.saved = Saved::load(&self.db);
        let running = self.state.features.plugins;
        if running && self.plugins.saved.downloader {
            self.unlock_downloader(true);
        }
        self.plugins.installed = store::installed()
            .into_iter()
            .map(|(pack, dir)| Installed {
                pack,
                dir,
                entries: Vec::new(),
                block_words: Vec::new(),
                loaded: false,
                error: String::new(),
            })
            .collect();
        self.plugins.waiting = self
            .plugins
            .installed
            .iter()
            .map(|plugin| plugin.pack.id.clone())
            .filter(|id| running && self.plugins.enabled(id))
            .collect();
        self.refresh_market();
        // `--load-js <file>` or `--load-js=<file>`: install and turn on a
        // script, as NipaPlay's desktop builds do, for trying one out.
        for script in load_js_arguments(std::env::args()) {
            let _ = self.import_plugin_command(MediaSource::file(script));
        }
    }

    fn refresh_market(&mut self) {
        if self.plugins.loading {
            return;
        }
        self.plugins.loading = true;
        self.plugins.error.clear();
        in_background(&self.plugins.inbox, || {
            let (listings, errors) = store::read_all();
            vec![Done::Market(listings, errors)]
        });
    }

    /// Take in what came back, start what can start, and read what the
    /// plugins' evaluations answered.
    pub(crate) fn plugins_wake(&mut self) {
        if !self.plugins.started {
            self.start_plugins();
        }
        let done: Vec<Done> = std::mem::take(
            &mut *self
                .plugins
                .inbox
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        for done in done {
            match done {
                Done::Market(listings, errors) => {
                    self.plugins.loading = false;
                    self.plugins.listings = listings;
                    self.plugins.error = errors.join("\n");
                }
                Done::Installed(id, result) => {
                    self.plugins.busy.retain(|busy| busy != &id);
                    match result {
                        Ok(pack) => self.installed_plugin(pack),
                        Err(reason) => self.notify(Notice::PluginFailed { reason }),
                    }
                }
                #[cfg(any(target_os = "macos", target_os = "windows", all(target_os = "linux", not(target_env = "ohos"))))]
                Done::Opening(url, Ok(frames)) => {
                    if self.source.as_ref().is_some_and(|source| source.url == url) {
                        self.plugin_event("openingProfiled", json!({ "url": url, "frames": frames }));
                    }
                }
                #[cfg(any(target_os = "macos", target_os = "windows", all(target_os = "linux", not(target_env = "ohos"))))]
                Done::Opening(url, Err(error)) => eprintln!("erika: opening of {url}: {error}"),
                Done::Resolved(tag, answer) => self.resolver_fetched(tag, answer),
                Done::Readme(id, readme) => self.readme_read(&id, readme),
                Done::Fetched(id, request, answer) => {
                    let snapshot = self.plugin_snapshot(&id);
                    if let Some(runtime) = self.plugins.runtimes.get_mut(&id) {
                        runtime.settle(request, answer, &snapshot);
                    }
                }
            }
        }
        let waiting = std::mem::take(&mut self.plugins.waiting);
        for id in waiting {
            self.start_runtime(&id);
        }
        self.poll_plugins();
        self.show_plugins();
    }

    /// Start `id`'s context; it waits when the JavaScript host does not run
    /// yet.
    fn start_runtime(&mut self, id: &str) {
        let Some(plugin) = self.plugins.find(id) else {
            return;
        };
        if !plugin.pack.unsupported().is_empty() {
            return;
        }
        let pack = plugin.pack.clone();
        let script = match std::fs::read_to_string(plugin.dir.join(&pack.main)) {
            Ok(script) => script,
            Err(error) => {
                self.plugin_failed(id, format!("读不到 {}：{error}", pack.main));
                return;
            }
        };
        let requires: Vec<String> = if pack.has("script.external") {
            (0..pack.requires.len())
                .filter_map(|index| {
                    std::fs::read_to_string(plugin.dir.join(format!("requires/{index}.js"))).ok()
                })
                .collect()
        } else {
            Vec::new()
        };
        let snapshot = self.plugin_snapshot(id);
        match Runtime::start(&pack, &script, &requires, &snapshot) {
            Ok(runtime) => {
                self.plugins.runtimes.insert(id.to_string(), runtime);
            }
            Err(error) if error.contains("not running") => {
                self.plugins.waiting.push(id.to_string());
            }
            Err(error) => self.plugin_failed(id, error),
        }
    }

    fn stop_runtime(&mut self, id: &str) {
        if let Some(mut runtime) = self.plugins.runtimes.remove(id) {
            // The context goes with it: `pluginOnDestroy` runs if it is quick.
            runtime.invoke("onDestroy", None, &Value::Null, 0, Purpose::Event);
        }
        self.stop_process(id);
        if self.plugins.extensions.remove(id).is_some() {
            self.apply_extensions();
        }
        if let Some(plugin) = self.plugins.find_mut(id) {
            plugin.loaded = false;
            plugin.entries.clear();
            plugin.block_words.clear();
        }
        self.plugins.waiting.retain(|waiting| waiting != id);
    }

    fn plugin_failed(&mut self, id: &str, error: String) {
        eprintln!("erika: plugin {id}: {error}");
        if let Some(plugin) = self.plugins.find_mut(id) {
            plugin.loaded = false;
            plugin.error = error;
        }
    }

    /// What a plugin may read, as its context sees it.
    fn plugin_snapshot(&self, id: &str) -> Value {
        let saved = &self.plugins.saved;
        let pack = self.plugins.find(id).map(|plugin| &plugin.pack);
        let mut snapshot = Map::new();
        let state = PlayerState {
            position: self.state.elapsed,
            duration: self.state.duration,
            status: if !self.state.playing {
                PlayerStatus::Idle
            } else if self.state.paused {
                PlayerStatus::Paused
            } else {
                PlayerStatus::Playing
            },
            has_video: self.state.playing,
            rate: self.plugin_rate(),
        };
        snapshot.insert("player".into(), json!(state));
        snapshot.insert("danmaku.offset".into(), json!(self.state.danmaku_settings.delay));
        snapshot.insert("text".into(), json!(saved.text.get(id).cloned().unwrap_or_default()));
        snapshot.insert(
            "switch".into(),
            json!(saved.switches.get(id).cloned().unwrap_or_default()),
        );
        snapshot.insert(
            "storage".into(),
            json!(saved.storage.get(id).cloned().unwrap_or_default()),
        );
        snapshot.insert("process.running".into(), json!(self.process_running(id)));
        if pack.is_some_and(|pack| pack.has("library.read")) {
            snapshot.insert("library".into(), self.library_for_plugins());
        }
        Value::Object(snapshot)
    }

    /// The rate playing now.
    fn plugin_rate(&self) -> f64 {
        self.rate()
    }

    /// The library's shows for `library.getShows()`: their ids, titles and
    /// the folders their files are in.
    fn library_for_plugins(&self) -> Value {
        let mut shows: BTreeMap<i64, (String, std::collections::BTreeSet<String>)> = BTreeMap::new();
        for entry in self.db.entries(None) {
            let Some((anime, _)) = entry.matched else {
                continue;
            };
            let show = shows
                .entry(anime.dandanplay_id)
                .or_insert_with(|| (anime.title.clone(), Default::default()));
            if let Some(folder) = bern::files::file_path(&entry.url)
                .and_then(|path| path.parent().map(|folder| folder.display().to_string()))
            {
                show.1.insert(folder);
            }
        }
        Value::Array(
            shows
                .into_iter()
                .map(|(id, (title, folders))| json!({ "id": id, "title": title, "folders": folders }))
                .collect(),
        )
    }

    /// Read what the plugins' evaluations answered.
    fn poll_plugins(&mut self) {
        let waker = runtime::engine_waker();
        let ids: Vec<String> = self.plugins.runtimes.keys().cloned().collect();
        for id in ids {
            let finished = match self.plugins.runtimes.get_mut(&id) {
                Some(runtime) => runtime.finished(&waker),
                None => continue,
            };
            for (purpose, result) in finished {
                self.plugin_answered(&id, purpose, result);
            }
        }
    }

    fn plugin_answered(&mut self, id: &str, purpose: Purpose, result: Result<Value, String>) {
        let reply = match result {
            Ok(reply) => reply,
            Err(error) => {
                match purpose {
                    Purpose::Load => self.plugin_failed(id, error),
                    Purpose::Resolve(_) | Purpose::Suggest(_) | Purpose::Danmaku(_) => {
                        self.plugin_hook_answered(id, purpose, None, Some(error))
                    }
                    _ => eprintln!("erika: plugin {id}: {error}"),
                }
                return;
            }
        };
        let mut words_changed = false;
        if let Some(plugin) = self.plugins.find_mut(id) {
            if let Some(entries) = reply.get("entries").and_then(Value::as_array) {
                plugin.entries = entries.clone();
            }
            if let Some(words) = strings(reply.get("blockWords"))
                && words != plugin.block_words
            {
                plugin.block_words = words;
                words_changed = purpose != Purpose::Load;
            }
        }
        if words_changed {
            self.reload_plugin_danmaku();
        }
        let error = reply.get("error").and_then(Value::as_str).map(String::from);
        if purpose == Purpose::Load {
            if let Some(manifest) = reply.get("manifest").filter(|m| m.is_object())
                && let Some(plugin) = self.plugins.find_mut(id)
                && plugin.pack.nipaplay
                && let Ok(mut pack) = Pack::from_manifest(manifest)
            {
                pack.main = plugin.pack.main.clone();
                plugin.pack = pack;
            }
            match &error {
                Some(error) => self.plugin_failed(id, error.clone()),
                None => {
                    if let Some(plugin) = self.plugins.find_mut(id) {
                        plugin.loaded = true;
                        plugin.error.clear();
                    }
                    if !self.plugins.tracks.is_empty() {
                        self.rerun_plugin_danmaku();
                    }
                }
            }
        } else if let Some(error) = &error {
            eprintln!("erika: plugin {id}: {error}");
        }
        if let Purpose::Action(_) = purpose
            && error.is_none()
        {
            self.plugin_said(id, reply.get("value"));
        }
        let calls = reply
            .get("calls")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for call in calls {
            let Some(name) = call.get(0).and_then(Value::as_str) else {
                continue;
            };
            let args = call.get(1).and_then(Value::as_array).cloned().unwrap_or_default();
            self.apply_plugin_call(id, name, &args);
        }
        if matches!(purpose, Purpose::Danmaku(_) | Purpose::Resolve(_) | Purpose::Suggest(_)) {
            self.plugin_hook_answered(id, purpose, reply.get("value").cloned(), error);
        }
    }

    /// What `pluginHandleUIAction` answered: `{ type: 'text', title, content }`.
    fn plugin_said(&mut self, id: &str, value: Option<&Value>) {
        let value = match value {
            Some(Value::String(text)) => serde_json::from_str(text).unwrap_or(Value::Null),
            Some(value) => value.clone(),
            None => Value::Null,
        };
        if !value.is_object() {
            return;
        }
        let name = self.plugins.find(id).map(|p| p.pack.name.clone()).unwrap_or_default();
        let title = text_of(&value, "title");
        self.show_plugin_message(if title.is_empty() { name } else { title }, text_of(&value, "content"));
    }

    fn show_plugin_message(&mut self, title: String, message: String) {
        let panel = &mut self.state.plugin_panel;
        panel.has_message = true;
        panel.message_title = title;
        panel.message = message;
    }

    /// Apply a call `id` queued, if it declared the permission for it.
    fn apply_plugin_call(&mut self, id: &str, name: &str, args: &[Value]) {
        let Some(pack) = self.plugins.find(id).map(|plugin| plugin.pack.clone()) else {
            return;
        };
        if let Some(call) = Call::parse(name, args) {
            if pack.has(call.permission()) {
                self.apply_playback_call(id, call);
            }
            return;
        }
        let text = |index: usize| args.get(index).map_or(String::new(), |value| match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        });
        let flag = |index: usize| {
            args.get(index)
                .is_some_and(|value| value.as_bool() == Some(true) || value.as_str() == Some("true"))
        };
        let saved = &mut self.plugins.saved;
        match name {
            "uiShowToast" if pack.has("ui.dialog") => self.notify(Notice::PluginSays {
                name: pack.name.clone(),
                message: text(0),
            }),
            "uiShowDialog" if pack.has("ui.dialog") => self.show_plugin_message(text(0), text(1)),
            "storageSet" if pack.has("storage") => {
                saved.storage.entry(id.into()).or_default().insert(text(0), text(1));
                saved.save(&self.db);
            }
            "storageRemove" if pack.has("storage") => {
                saved.storage.entry(id.into()).or_default().remove(&text(0));
                saved.save(&self.db);
            }
            "storageClear" if pack.has("storage") => {
                saved.storage.remove(id);
                saved.save(&self.db);
            }
            "pluginSetTextSetting" => {
                saved.text.entry(id.into()).or_default().insert(text(0), text(1));
                saved.save(&self.db);
                self.show_plugins();
            }
            "pluginSetSwitchSetting" => {
                saved.switches.entry(id.into()).or_default().insert(text(0), flag(1));
                saved.save(&self.db);
                self.show_plugins();
            }
            "devLog" => eprintln!("erika: plugin {id}: {}", text(0)),
            "devLogError" => eprintln!("erika: plugin {id}: error: {}", text(0)),
            "systemSetDownloaderEnabled" if pack.has("system.override") => {
                saved.downloader = flag(0);
                saved.save(&self.db);
                self.unlock_downloader(flag(0));
            }
            "processStart" if pack.has("system.override") => self.start_process(id, args),
            "processStop" if pack.has("system.override") => self.stop_process(id),
            "netFetch" if pack.has("network") => self.plugin_fetch(id, args),
            "librarySetEpisodeWatched" if pack.has("library.write") => {
                self.plugin_mark_watched(
                    args.first().and_then(Value::as_i64).unwrap_or(0),
                    args.get(1).and_then(Value::as_i64).unwrap_or(0),
                    flag(2),
                );
            }
            "extendSpeeds" if pack.has("player.control") => {
                let speeds = args
                    .first()
                    .and_then(Value::as_array)
                    .map(|list| {
                        list.iter()
                            .filter_map(Value::as_f64)
                            .filter(|rate| (0.0625..=16.0).contains(rate))
                            .collect()
                    })
                    .unwrap_or_default();
                self.plugins.extensions.entry(id.into()).or_default().speeds = speeds;
                self.apply_extensions();
            }
            "extendDanmakuColors" if pack.has("danmaku.modify") => {
                let colors = args
                    .first()
                    .and_then(Value::as_array)
                    .map(|list| {
                        list.iter()
                            .filter_map(|color| {
                                let hex = text_of(color, "color");
                                let valid = hex.len() == 7
                                    && hex.starts_with('#')
                                    && hex[1..].chars().all(|c| c.is_ascii_hexdigit());
                                valid.then(|| (text_of(color, "name"), hex.to_ascii_uppercase()))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                self.plugins.extensions.entry(id.into()).or_default().colors = colors;
                self.apply_extensions();
            }
            "extendDanmakuHexInput" if pack.has("danmaku.modify") => {
                self.plugins.extensions.entry(id.into()).or_default().hex_input = flag(0);
                self.apply_extensions();
            }
            "extendOpeningProfile" if pack.has("player.control") => {
                let seconds = args.get(1).and_then(Value::as_f64).unwrap_or(OPENING_SECONDS);
                self.plugins.extensions.entry(id.into()).or_default().opening = if flag(0) {
                    seconds.clamp(1.0, 60.0)
                } else {
                    0.0
                };
            }
            "extendDownloadQuickSelect" if pack.has("library.read") => {
                self.plugins.extensions.entry(id.into()).or_default().quick_select = flag(0);
                self.apply_extensions();
            }
            "extendWatchedButton" if pack.has("library.write") => {
                self.plugins.extensions.entry(id.into()).or_default().watched_button = flag(0);
                self.apply_extensions();
            }
            "__later" => self.plugin_later(id, args),
            _ => {}
        }
    }

    fn apply_playback_call(&mut self, id: &str, call: Call) {
        match call {
            Call::Play if self.state.paused => {
                let _ = erika::Commands::toggle_pause(self);
            }
            Call::Pause if self.state.playing && !self.state.paused => {
                let _ = erika::Commands::toggle_pause(self);
            }
            Call::Seek(seconds) if self.state.duration > 0.0 => {
                let _ = erika::Commands::seek(self, (seconds / self.state.duration).clamp(0.0, 1.0));
            }
            Call::SetRate(rate) => self.set_rate_override(Some(rate)),
            Call::ShowDanmaku => {
                let _ = erika::Commands::set_preference(self, "show_danmaku".into(), true);
            }
            Call::HideDanmaku => {
                let _ = erika::Commands::set_preference(self, "show_danmaku".into(), false);
            }
            Call::SetDanmakuOpacity(opacity) => {
                let _ = erika::Commands::set_danmaku_number(self, "opacity".into(), opacity);
            }
            Call::SetDanmakuOffset(seconds) => {
                let _ = erika::Commands::set_danmaku_number(self, "delay".into(), seconds);
            }
            Call::AddFilter(entry) => {
                if !self.plugins.filters.contains(&entry) {
                    self.plugins.filters.push(entry);
                    self.reload_plugin_danmaku();
                }
            }
            Call::RemoveFilter(entry) => {
                let before = self.plugins.filters.len();
                self.plugins.filters.retain(|filter| filter != &entry);
                if self.plugins.filters.len() != before {
                    self.reload_plugin_danmaku();
                }
            }
            Call::ReplaceDanmaku(comments) => self.replace_plugin_danmaku(id, comments),
            _ => {}
        }
    }

    /// Show what the running plugins add to the controls.
    fn apply_extensions(&mut self) {
        self.show_speeds();
        self.state.download_quick_select = self.plugins.extensions().quick_select;
        self.plugin_controls_changed();
    }

    /// The account sync brought what was kept of the plugins from another
    /// device: those turned on there run here, from the market when they
    /// are not installed yet.
    pub(crate) fn plugins_restored(&mut self) {
        if !self.plugins.started {
            return;
        }
        let saved = Saved::load(&self.db);
        if saved == self.plugins.saved {
            return;
        }
        // With plugins turned off, what came is kept for when they run.
        if !self.state.features.plugins {
            self.plugins.saved = saved;
            return;
        }
        let ids: Vec<String> = self.plugins.runtimes.keys().cloned().collect();
        for id in ids {
            self.stop_runtime(&id);
        }
        self.plugins.saved = saved;
        self.unlock_downloader(self.plugins.saved.downloader);
        for id in self.plugins.saved.enabled.clone() {
            if self.plugins.find(&id).is_some() {
                self.start_runtime(&id);
            } else if self.plugins.listings.iter().any(|listing| listing.pack.id == id) {
                let _ = self.install_plugin_command(id);
            }
        }
        self.rerun_plugin_danmaku();
        self.show_plugins();
    }

    /// Plugins turned on or off among the optional features: off stops every
    /// plugin and takes back what they changed, keeping what is installed
    /// and which are turned on; on starts those again.
    pub(crate) fn set_plugins_running(&mut self, running: bool) {
        if !self.plugins.started {
            return;
        }
        if running {
            if self.plugins.saved.downloader {
                self.unlock_downloader(true);
            }
            for id in self.plugins.saved.enabled.clone() {
                self.start_runtime(&id);
            }
        } else {
            let ids: Vec<String> = self.plugins.runtimes.keys().cloned().collect();
            for id in ids {
                self.stop_runtime(&id);
            }
            self.plugins.waiting.clear();
            if self.plugins.saved.downloader {
                self.unlock_downloader(false);
            }
        }
        self.rerun_plugin_danmaku();
        self.show_plugins();
    }

    /// Show the downloader where Erika hides it, or hide it again: the
    /// downloads module comes on with it.
    fn unlock_downloader(&mut self, unlocked: bool) {
        let was_hidden = crate::settings::downloader_hidden();
        crate::settings::set_downloader_unlocked(unlocked);
        self.state.downloader_unlocked = unlocked;
        self.state.downloads_hidden = crate::settings::downloader_hidden();
        if was_hidden != self.state.downloads_hidden && cfg!(feature = "bern-module-downloads") {
            let _ = erika::Commands::set_module(self, "downloads".into(), !self.state.downloads_hidden);
        }
    }

    /// Measure the first seconds of what opened, when a running plugin
    /// asked to: only where liberika plays files.
    pub(crate) fn profile_opening(&mut self) {
        let seconds = self.plugins.extensions().opening;
        if seconds <= 0.0 {
            return;
        }
        let Some(url) = self.source.as_ref().map(|source| source.url.clone()) else {
            return;
        };
        #[cfg(any(target_os = "macos", target_os = "windows", all(target_os = "linux", not(target_env = "ohos"))))]
        in_background(&self.plugins.inbox, move || {
            let profile = liberika::opening::opening_profile(&url, seconds, 4.0)
                .map(|frames| json!(frames));
            vec![Done::Opening(url, profile)]
        });
        #[cfg(not(any(target_os = "macos", target_os = "windows", all(target_os = "linux", not(target_env = "ohos")))))]
        let _ = (url, seconds);
    }

    /// What plays, as `videoLoaded`, `play` and `pause` describe it.
    pub(crate) fn plugin_media(&self) -> Value {
        json!({
            "title": self.state.title,
            "url": self.source.as_ref().map(|source| source.url.clone()),
            "duration": self.state.duration,
            "position": self.state.elapsed,
        })
    }

    /// Send what happened to every running plugin: `pluginOnEvent`.
    pub(crate) fn plugin_event(&mut self, name: &str, data: Value) {
        let event = json!({ "name": name, "data": data });
        let ids: Vec<String> = self.plugins.runtimes.keys().cloned().collect();
        for id in ids {
            let snapshot = self.plugin_snapshot(&id);
            if let Some(runtime) = self.plugins.runtimes.get_mut(&id) {
                runtime.invoke("onEvent", Some(&event), &snapshot, 0, Purpose::Event);
            }
        }
    }

    /// The plugins asked for something that answers later, by `net.fetch`.
    fn plugin_fetch(&mut self, id: &str, args: &[Value]) {
        let Some(request) = args.first().and_then(Value::as_u64) else {
            return;
        };
        let url = args.get(1).and_then(Value::as_str).unwrap_or_default().to_string();
        let options = args.get(2).cloned().unwrap_or(Value::Null);
        let id = id.to_string();
        in_background(&self.plugins.inbox, move || {
            vec![Done::Fetched(id, request, fetch(&url, &options))]
        });
    }

    /// A handler's promise settled: an action's text, or a hook's result.
    fn plugin_later(&mut self, id: &str, args: &[Value]) {
        let tag = args.first().and_then(Value::as_u64).unwrap_or(0);
        let value = args.get(1).cloned();
        let error = args.get(2).and_then(Value::as_str).map(String::from);
        if tag == 0 {
            if let Some(error) = error {
                eprintln!("erika: plugin {id}: {error}");
            } else {
                self.plugin_said(id, value.as_ref());
            }
            return;
        }
        if let Some(purpose) = self.plugins.later.get(&tag).cloned() {
            self.plugin_hook_answered(id, purpose, value, error);
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    fn start_process(&mut self, id: &str, args: &[Value]) {
        self.stop_process(id);
        let Some(program) = args.first().and_then(Value::as_str).filter(|p| !p.trim().is_empty()) else {
            return;
        };
        let mut command = std::process::Command::new(program);
        command.args(strings(args.get(1)).unwrap_or_default());
        if let Some(env) = args.get(2).and_then(Value::as_object) {
            for (key, value) in env {
                if let Some(value) = value.as_str() {
                    command.env(key, value);
                }
            }
        }
        match command.spawn() {
            Ok(child) => {
                self.plugins.processes.insert(id.to_string(), child);
            }
            Err(error) => eprintln!("erika: plugin {id}: {program}: {error}"),
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    fn start_process(&mut self, id: &str, _args: &[Value]) {
        eprintln!("erika: plugin {id}: programs cannot be started here");
    }

    fn stop_process(&mut self, id: &str) {
        #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
        if let Some(mut child) = self.plugins.processes.remove(id) {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = id;
    }

    fn process_running(&self, id: &str) -> bool {
        #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
        return self.plugins.processes.contains_key(id);
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            let _ = id;
            false
        }
    }

    fn installed_plugin(&mut self, pack: Pack) {
        let id = pack.id.clone();
        let name = pack.name.clone();
        let was_enabled = self.plugins.enabled(&id);
        self.stop_runtime(&id);
        let dir = store::folder().map(|folder| folder.join(&id)).unwrap_or_default();
        self.plugins.installed.retain(|plugin| plugin.pack.id != id);
        self.plugins.installed.push(Installed {
            pack,
            dir,
            entries: Vec::new(),
            block_words: Vec::new(),
            loaded: false,
            error: String::new(),
        });
        self.plugins
            .installed
            .sort_by(|a, b| a.pack.priority.cmp(&b.pack.priority).then(a.pack.id.cmp(&b.pack.id)));
        // A new plugin is turned on; an update keeps the switch it had.
        if !was_enabled && !self.plugins.saved.enabled.contains(&id) {
            self.plugins.saved.enabled.push(id.clone());
            self.plugins.saved.save(&self.db);
        }
        if self.plugins.enabled(&id) {
            self.start_runtime(&id);
        }
        self.notify(Notice::PluginAdded { name });
    }

    /// The market and the installed plugins as the page lists them.
    pub(crate) fn show_plugins(&mut self) {
        let host = self.state.version.clone();
        let mut all: Vec<Plugin> = Vec::new();
        for plugin in &self.plugins.installed {
            let listed = self.plugins.listings.iter().find(|l| l.pack.id == plugin.pack.id);
            all.push(self.plugin_row(&plugin.pack, Some(plugin), listed, &host));
        }
        for listing in &self.plugins.listings {
            if self.plugins.find(&listing.pack.id).is_none() {
                all.push(self.plugin_row(&listing.pack, None, Some(listing), &host));
            }
        }
        self.state.installed_count = self.plugins.installed.len() as i64;
        // The page open shows the plugin as it is now: installed, turned on.
        if let Some(open) = &self.state.plugin_info.plugin
            && let Some(now) = all.iter().find(|plugin| plugin.id == open.id)
        {
            self.state.plugin_info.plugin = Some(now.clone());
        }
        self.state.plugins = all
            .into_iter()
            .filter(|plugin| {
                (self.state.plugin_category == PluginCategory::All
                    || plugin.category == self.state.plugin_category)
                    && (self.state.plugin_tab == PluginTab::Market || plugin.installed)
            })
            .collect();
        self.state.plugins_loading = self.plugins.loading;
        self.state.plugins_starting = !self.plugins.waiting.is_empty();
        self.state.plugins_error = self.plugins.error.clone();
        self.show_plugin_panel();
    }

    fn plugin_row(
        &self,
        pack: &Pack,
        installed: Option<&Installed>,
        listing: Option<&store::Listing>,
        host: &str,
    ) -> Plugin {
        let image = match (installed, &pack.image) {
            (Some(plugin), Some(name)) => {
                let path = plugin.dir.join(name);
                path.is_file().then(|| MediaSource::file(path))
            }
            _ => listing.and_then(|l| l.image.clone()).map(MediaSource::new),
        };
        let mut unsupported: Vec<String> = pack.unsupported().iter().map(|s| s.to_string()).collect();
        if !pack.fits(host) {
            unsupported.push(format!(
                "需要 Erika {} 或更新版本",
                pack.min_host.as_deref().unwrap_or_default()
            ));
        }
        Plugin {
            id: pack.id.clone(),
            name: pack.name.clone(),
            author: pack.author.clone(),
            description: pack.description.clone(),
            version: pack.version.clone(),
            tags: pack
                .tags()
                .into_iter()
                .map(|permission| PluginTag {
                    id: format!("{permission:?}"),
                    permission,
                })
                .collect(),
            category: pack.category.clone(),
            icon: pack.icon.clone(),
            image,
            installed: installed.is_some(),
            enabled: installed.is_some() && self.plugins.enabled(&pack.id),
            loaded: installed.is_some_and(|plugin| plugin.loaded),
            update: installed.is_some()
                && listing.is_some_and(|listing| pack::newer(&listing.pack.version, &pack.version)),
            busy: self.plugins.busy.contains(&pack.id),
            unsupported: unsupported.join("；"),
            error: installed.map(|plugin| plugin.error.clone()).unwrap_or_default(),
            has_settings: installed.is_some_and(|plugin| !plugin.entries.is_empty()),
            nipaplay: pack.nipaplay,
        }
    }

    /// The settings of the plugin open, from its `pluginUIEntries`: a
    /// switch where an entry has `enabled`, a text field where it has
    /// `textSetting`, a button otherwise.
    fn show_plugin_panel(&mut self) {
        let Some(id) = self.plugins.open.clone() else {
            self.state.plugin_panel.open = false;
            return;
        };
        let Some(plugin) = self.plugins.find(&id) else {
            self.plugins.open = None;
            self.state.plugin_panel.open = false;
            return;
        };
        let saved_text = self.plugins.saved.text.get(&id);
        let saved_switch = self.plugins.saved.switches.get(&id);
        let entries = plugin
            .entries
            .iter()
            .filter_map(|entry| {
                let entry_id = text_of(entry, "id");
                let title = text_of(entry, "title");
                if entry_id.is_empty() || title.is_empty() {
                    return None;
                }
                let (kind, on, text, hint) = if let Some(enabled) = entry.get("enabled") {
                    let on = saved_switch
                        .and_then(|values| values.get(&entry_id).copied())
                        .unwrap_or(enabled.as_bool().unwrap_or(false));
                    (PluginEntryKind::Switch, on, String::new(), String::new())
                } else if let Some(setting) = entry.get("textSetting") {
                    let text = saved_text
                        .and_then(|values| values.get(&entry_id).cloned())
                        .unwrap_or_else(|| text_of(setting, "default"));
                    (PluginEntryKind::Text, false, text, text_of(setting, "hintText"))
                } else {
                    (PluginEntryKind::Action, false, String::new(), String::new())
                };
                Some(PluginEntry {
                    id: entry_id,
                    title,
                    description: text_of(entry, "description"),
                    kind,
                    on,
                    text,
                    hint,
                })
            })
            .collect();
        let panel = &mut self.state.plugin_panel;
        panel.open = true;
        panel.id = id;
        panel.name = plugin.pack.name.clone();
        panel.entries = entries;
    }

    fn open_plugin_id(&self) -> Result<String, CommandError> {
        self.plugins
            .open
            .clone()
            .ok_or_else(|| CommandError::new("unknown_plugin", "没有打开的插件设置"))
    }

    fn invoke_action(&mut self, id: &str, entry: &str) {
        let snapshot = self.plugin_snapshot(id);
        if let Some(runtime) = self.plugins.runtimes.get_mut(id) {
            runtime.invoke(
                "handleAction",
                Some(&json!(entry)),
                &snapshot,
                0,
                Purpose::Action(entry.to_string()),
            );
        }
    }

    // ---- Commands ----

    pub(crate) fn install_plugin_command(&mut self, id: String) -> Result<(), CommandError> {
        let listing = self
            .plugins
            .listings
            .iter()
            .find(|listing| listing.pack.id == id)
            .cloned()
            .ok_or_else(|| CommandError::new("unknown_plugin", format!("市场里没有插件 {id}")))?;
        if self.plugins.busy.contains(&id) {
            return Ok(());
        }
        self.plugins.busy.push(id.clone());
        in_background(&self.plugins.inbox, move || {
            let result = store::install(&listing).and_then(|pack| {
                fetch_requires(&pack)?;
                Ok(pack)
            });
            vec![Done::Installed(id, result)]
        });
        self.show_plugins();
        Ok(())
    }

    pub(crate) fn import_plugin_command(&mut self, source: MediaSource) -> Result<(), CommandError> {
        let path = bern::files::file_path(&source.url)
            .ok_or_else(|| CommandError::new("invalid_plugin", "请选择本地的插件包、插件文件夹或 .js 文件"))?;
        let id = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.plugins.busy.push(id.clone());
        let inbox = Arc::clone(&self.plugins.inbox);
        in_background(&inbox, move || {
            let package = path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case(store::PACKAGE_EXTENSION));
            let result = if path.is_dir() {
                store::import_folder(&path)
            } else if package {
                store::import_package(&path)
            } else {
                import_script(&path)
            }
            .and_then(|pack| {
                fetch_requires(&pack)?;
                Ok(pack)
            });
            vec![Done::Installed(id, result)]
        });
        Ok(())
    }

    pub(crate) fn open_plugin_info_command(&mut self, id: String) -> Result<(), CommandError> {
        let host = self.state.version.clone();
        let installed = self.plugins.find(&id);
        let listing = self.plugins.listings.iter().find(|listing| listing.pack.id == id);
        let pack = installed
            .map(|plugin| &plugin.pack)
            .or(listing.map(|listing| &listing.pack))
            .ok_or_else(|| CommandError::new("unknown_plugin", format!("没有插件 {id}")))?;
        let plugin = self.plugin_row(pack, installed, listing, &host);
        let readme = Readme::of(installed, listing);
        self.state.plugin_info = PluginInfo {
            open: true,
            loading: readme.is_some(),
            error: String::new(),
            blocks: if readme.is_some() { Vec::new() } else { markdown::paragraph(&plugin.description) },
            plugin: Some(plugin),
        };
        if let Some(readme) = readme {
            in_background(&self.plugins.inbox, move || vec![Done::Readme(id, readme.read())]);
        }
        Ok(())
    }

    pub(crate) fn close_plugin_info_command(&mut self) -> Result<(), CommandError> {
        self.state.plugin_info = closed_info();
        Ok(())
    }

    /// Show the README that came, on the page still open for it; without
    /// one, what the market says of the plugin.
    fn readme_read(&mut self, id: &str, readme: Result<(String, String), String>) {
        let info = &mut self.state.plugin_info;
        let Some(plugin) = info.plugin.as_ref().filter(|plugin| info.open && plugin.id == id) else {
            return;
        };
        let blocks = match readme {
            Ok((text, base)) => markdown::blocks(&text, &base),
            Err(_) => Vec::new(),
        };
        info.blocks = if blocks.is_empty() { markdown::paragraph(&plugin.description) } else { blocks };
        info.loading = false;
    }

    pub(crate) fn uninstall_plugin_command(&mut self, id: String) -> Result<(), CommandError> {
        if self.plugins.find(&id).is_none() {
            return Err(CommandError::new("unknown_plugin", format!("没有安装插件 {id}")));
        }
        self.stop_runtime(&id);
        store::remove(&id);
        self.plugins.installed.retain(|plugin| plugin.pack.id != id);
        let saved = &mut self.plugins.saved;
        saved.enabled.retain(|enabled| enabled != &id);
        saved.text.remove(&id);
        saved.switches.remove(&id);
        saved.storage.remove(&id);
        saved.save(&self.db);
        if self.plugins.open.as_deref() == Some(id.as_str()) {
            self.plugins.open = None;
        }
        self.rerun_plugin_danmaku();
        self.show_plugins();
        Ok(())
    }

    pub(crate) fn set_plugin_enabled_command(&mut self, id: String, enabled: bool) -> Result<(), CommandError> {
        if self.plugins.find(&id).is_none() {
            return Err(CommandError::new("unknown_plugin", format!("没有安装插件 {id}")));
        }
        self.plugins.saved.enabled.retain(|on| on != &id);
        if enabled {
            self.plugins.saved.enabled.push(id.clone());
            self.start_runtime(&id);
        } else {
            self.stop_runtime(&id);
            // A plugin that unlocked the downloader takes it back with it.
            if self
                .plugins
                .find(&id)
                .is_some_and(|plugin| plugin.pack.has("system.override"))
            {
                self.plugins.saved.downloader = false;
                self.unlock_downloader(false);
            }
        }
        self.plugins.saved.save(&self.db);
        self.rerun_plugin_danmaku();
        self.show_plugins();
        Ok(())
    }

    pub(crate) fn refresh_plugins_command(&mut self) -> Result<(), CommandError> {
        self.refresh_market();
        self.show_plugins();
        Ok(())
    }

    pub(crate) fn open_plugin_command(&mut self, id: String) -> Result<(), CommandError> {
        if self.plugins.find(&id).is_none() {
            return Err(CommandError::new("unknown_plugin", format!("没有安装插件 {id}")));
        }
        self.plugins.open = Some(id);
        self.state.plugin_panel.has_message = false;
        self.show_plugins();
        Ok(())
    }

    pub(crate) fn close_plugin_command(&mut self) -> Result<(), CommandError> {
        self.plugins.open = None;
        self.state.plugin_panel.open = false;
        self.state.plugin_panel.has_message = false;
        Ok(())
    }

    pub(crate) fn plugin_action_command(&mut self, entry: String) -> Result<(), CommandError> {
        let id = self.open_plugin_id()?;
        self.invoke_action(&id, &entry);
        Ok(())
    }

    pub(crate) fn set_plugin_switch_command(&mut self, entry: String, value: bool) -> Result<(), CommandError> {
        let id = self.open_plugin_id()?;
        self.plugins
            .saved
            .switches
            .entry(id.clone())
            .or_default()
            .insert(entry.clone(), value);
        self.plugins.saved.save(&self.db);
        // NipaPlay calls the action for a switch, and reads the lists anew.
        self.invoke_action(&id, &entry);
        self.show_plugins();
        Ok(())
    }

    pub(crate) fn set_plugin_text_command(&mut self, entry: String, value: String) -> Result<(), CommandError> {
        let id = self.open_plugin_id()?;
        self.plugins
            .saved
            .text
            .entry(id)
            .or_default()
            .insert(entry, value);
        self.plugins.saved.save(&self.db);
        self.show_plugins();
        Ok(())
    }

    pub(crate) fn dismiss_plugin_message_command(&mut self) -> Result<(), CommandError> {
        self.state.plugin_panel.has_message = false;
        Ok(())
    }
}

/// The scripts `--load-js` names on the command line.
fn load_js_arguments(arguments: impl Iterator<Item = String>) -> Vec<PathBuf> {
    let mut scripts = Vec::new();
    let mut arguments = arguments.skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--load-js" {
            if let Some(path) = arguments.next() {
                scripts.push(PathBuf::from(path));
            }
        } else if let Some(path) = argument.strip_prefix("--load-js=") {
            scripts.push(PathBuf::from(path));
        }
    }
    scripts
}

/// Download the scripts a plugin requires into its folder, numbered in
/// order: they run before it.
fn fetch_requires(pack: &Pack) -> Result<(), String> {
    if pack.requires.is_empty() || !pack.has("script.external") {
        return Ok(());
    }
    let folder = store::folder().ok_or("没有可以安装插件的地方")?.join(&pack.id).join("requires");
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    for (index, external) in pack.requires.iter().enumerate() {
        let script = store::get(&external.url)?;
        std::fs::write(folder.join(format!("{index}.js")), script).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Import a NipaPlay script: its `pluginManifest` is read by running it
/// in a context of its own.
fn import_script(path: &std::path::Path) -> Result<Pack, String> {
    let script = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let manifest = manifest_of(&script)?;
    let pack = Pack::from_manifest(&manifest)?;
    let readme = std::fs::read(path.with_file_name("README.md")).ok();
    store::install_script(&pack, script.as_bytes(), readme.as_deref())
}

/// The `pluginManifest` a script declares, read in a context of its own.
fn manifest_of(script: &str) -> Result<Value, String> {
    let context = bern::script::Context::new().map_err(|error| error.to_string())?;
    let stubs = "var __s = new Proxy(function(){ return __s; }, { get: function(){ return __s; } });\
                 var player = __s, danmaku = __s, ui = __s, storage = __s, settings = __s, dev = __s, system = __s, process = __s, plugin = __s, net = __s, library = __s;\n";
    let source = format!(
        "{stubs}{script}\n;(typeof pluginManifest === 'undefined' ? null : JSON.parse(JSON.stringify(pluginManifest)));"
    );
    let evaluation = context.eval(source);
    let value = block_on(evaluation).map_err(|error| error.to_string())?;
    if value.is_object() {
        Ok(value)
    } else {
        Err("脚本里没有 pluginManifest".into())
    }
}

/// Wait for an evaluation on a worker thread.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, Wake, Waker};
    struct Thread(std::thread::Thread);
    impl Wake for Thread {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Thread(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
        std::thread::park_timeout(std::time::Duration::from_millis(100));
    }
}

/// `net.fetch(url, { method, headers, body })`: the answer as
/// `{ status, ok, headers, body }`.
#[cfg(not(target_arch = "wasm32"))]
fn fetch(url: &str, options: &Value) -> Result<Value, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!("不是网址：{url}"));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .http_status_as_error(false)
        .build()
        .into();
    let method = options
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("GET")
        .to_ascii_uppercase();
    let body = match options.get("body") {
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Null) | None => None,
        Some(other) => Some(other.to_string()),
    };
    let mut request = ureq::http::Request::builder().method(method.as_str()).uri(url);
    if let Some(headers) = options.get("headers").and_then(Value::as_object) {
        for (key, value) in headers {
            if let Some(value) = value.as_str() {
                request = request.header(key.as_str(), value);
            }
        }
    }
    let request = request
        .body(body.unwrap_or_default())
        .map_err(|error| error.to_string())?;
    let mut response = agent.run(request).map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    let headers: Map<String, Value> = response
        .headers()
        .iter()
        .filter_map(|(key, value)| Some((key.as_str().to_ascii_lowercase(), json!(value.to_str().ok()?))))
        .collect();
    let body = response
        .body_mut()
        .with_config()
        .limit(16 << 20)
        .read_to_string()
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "status": status,
        "ok": (200..300).contains(&status),
        "headers": headers,
        "body": body,
    }))
}

#[cfg(target_arch = "wasm32")]
fn fetch(_url: &str, _options: &Value) -> Result<Value, String> {
    Err("网页版的插件暂时不能联网".into())
}

#[cfg(test)]
mod engine_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_js_names_scripts() {
        let arguments = ["erika", "--load-js", "/a.js", "--other", "--load-js=/b.js"].map(String::from);
        assert_eq!(
            load_js_arguments(arguments.into_iter()),
            vec![PathBuf::from("/a.js"), PathBuf::from("/b.js")]
        );
    }

    #[test]
    fn saved_state_round_trips() {
        let mut saved = Saved::default();
        saved.enabled.push("a".into());
        saved.text.entry("a".into()).or_default().insert("key".into(), "值".into());
        saved.switches.entry("a".into()).or_default().insert("on".into(), true);
        saved.storage.entry("a".into()).or_default().insert("k".into(), "\"v\"".into());
        saved.downloader = true;
        assert_eq!(Saved::from_value(&saved.to_value()), saved);
        assert_eq!(Saved::from_value(&Value::Null), Saved::default());
    }
}
