//! The engine of Erika's shell. liberika plays the files and links the
//! user opens; the modules' sample media still play on a simulated clock
//! over their stills.

mod onboarding;
mod settings;

#[cfg(feature = "bern-module-account")]
mod account;
#[cfg(feature = "bern-module-account")]
mod bangumi;

mod chapters;
mod danmaku;
mod history;
mod kernel;
mod matching;
mod playback_preferences;
mod player;
mod queue;
mod search;
#[cfg(feature = "bern-module-sync")]
mod sync_restore;

use bern::CommandError;
use bern::media::MediaSource;
use bern::query::Answer;
use bern::resources::Resources;
use erika_protocol::erika::{self, *};
use kernel::Kernel;
use std::sync::Arc;

/// A simulated episode's length in seconds.
const LENGTH: f64 = 1455.0;

/// The still the frame shows under the video, and for simulated media.
const STILL: &str = "design/assets/still1.jpg";

pub struct ErikaEngine {
    pub(crate) state: State,
    feedback_remaining: f64,
    pub(crate) all_plugins: Vec<Plugin>,
    pub(crate) saved_modules: Option<Vec<String>>,
    /// Runtime target while a feature change starts/stops its modules.
    pub(crate) pending_modules: Option<Vec<String>>,
    pub(crate) onboarding_completed: bool,
    /// liberika, where it runs, and the media it plays.
    kernel: Option<Arc<dyn Kernel>>,
    source: Option<MediaSource>,
    /// A pause or a seek the user asked for, with the ticks it has waited
    /// for the kernel to show it.
    pause_wanted: Option<(bool, u8)>,
    seek_wanted: Option<(f64, u8)>,
    timeline: chapters::Timeline,
    queue: Vec<queue::Entry>,
    /// Pictures of the media asked of the kernel: the one awaited, and how
    /// many were asked in this run, which names each file anew.
    still_wanted: Option<std::path::PathBuf>,
    stills: u32,
    /// Where the media's first still is taken, past a fade-in.
    first_still_at: f64,
    /// What the app keeps: recent videos, and where each file stopped.
    db: erika_store::Db,
    /// The file playing, once its head was read, the picture taken of it,
    /// and where it was when its progress was last saved.
    file: Option<erika_store::FileId>,
    still_path: Option<std::path::PathBuf>,
    saved_at: f64,
    /// Names this run's pictures apart from earlier runs'.
    run: String,
    /// What signing in and renewing came back with on other threads.
    #[cfg(feature = "bern-module-account")]
    account_inbox: account::Inbox,
    #[cfg(feature = "bern-module-account")]
    bangumi: bangumi::Runtime,
    /// What matching the video playing came back with, and the dandanplay
    /// episode it was matched to.
    found: Arc<std::sync::Mutex<Vec<matching::Found>>>,
    episode_playing: Option<i64>,
    match_revision: u64,
    search_revision: u64,
    danmaku_sources: std::collections::BTreeMap<String, usize>,
    match_details: Option<erika_dandanplay::Details>,
    match_previews: std::collections::BTreeMap<i64, erika_dandanplay::Details>,
    match_preview_queue: std::collections::VecDeque<i64>,
    match_previews_active: usize,
    online_danmaku_count: usize,
}

/// Run `work` on another thread; what it returns waits in `inbox`, and the
/// engine is woken to take it in. The browser has no threads: there the
/// work runs at once and the engine takes it in right after.
pub(crate) fn in_background<T: Send + 'static>(
    inbox: &Arc<std::sync::Mutex<Vec<T>>>,
    work: impl FnOnce() -> Vec<T> + Send + 'static,
) {
    let inbox = Arc::clone(inbox);
    let waker = bern::wake::waker();
    let run = move || {
        let results = work();
        if results.is_empty() {
            return;
        }
        inbox
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extend(results);
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

/// This device's id with the recognition server, which keeps its
/// confirmations apart from other devices': made once, then kept.
fn device_id(db: &erika_store::Db) -> String {
    const DEVICE: &str = "device.id";
    if let Some(id) = db.setting(DEVICE).filter(|id| !id.is_empty()) {
        return id;
    }
    use std::hash::{BuildHasher, Hasher};
    let id: String = (0..2)
        .map(|_| {
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_i64(erika_store::now());
            format!("{:016x}", hasher.finish())
        })
        .collect();
    db.set_setting(DEVICE, Some(&id));
    id
}

impl Default for ErikaEngine {
    fn default() -> Self {
        let db = erika_store::Db::open();
        let recents = history::recents(&db);
        // Saved module choices are restored before touching credentials.
        erika_dandanplay::set_token(None);
        // The recognition server is built in; the engine's tests ask none.
        erika_dandanplay::custom::set_device(&device_id(&db));
        erika_dandanplay::custom::set_server(
            (!cfg!(test)).then(|| erika_dandanplay::custom::BUILT_IN.to_string()),
        );
        // The browser has no process id.
        let process = if cfg!(target_arch = "wasm32") {
            0
        } else {
            std::process::id()
        };
        let run = format!("{}-{process}-", erika_store::now());
        let mut engine = Self {
            db,
            file: None,
            still_path: None,
            saved_at: 0.0,
            run,
            #[cfg(feature = "bern-module-account")]
            account_inbox: Arc::default(),
            #[cfg(feature = "bern-module-account")]
            bangumi: bangumi::Runtime::default(),
            found: Arc::default(),
            episode_playing: None,
            match_revision: 0,
            search_revision: 0,
            danmaku_sources: Default::default(),
            match_details: None,
            match_previews: Default::default(),
            match_preview_queue: Default::default(),
            match_previews_active: 0,
            online_danmaku_count: 0,
            feedback_remaining: 0.0,
            saved_modules: None,
            pending_modules: None,
            onboarding_completed: false,
            kernel: if cfg!(test) { None } else { kernel::start() },
            source: None,
            pause_wanted: None,
            seek_wanted: None,
            timeline: chapters::Timeline::default(),
            queue: Vec::new(),
            still_wanted: None,
            stills: 0,
            first_still_at: 0.0,
            all_plugins: settings::sample_plugins(),
            state: State {
                onboarding: onboarding::initial(),
                preferences: settings::default_preferences(),
                features: settings::feature_selection(
                    &settings::ALL_MODULES
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>(),
                ),
                module_count: settings::ALL_MODULES.len() as i64,
                feature_preview_revision: 0,
                preset: OnboardingPreset::All,
                notice: None,
                notice_revision: 0,
                plugins: settings::sample_plugins(),
                plugin_category: PluginCategory::All,
                plugin_tab: PluginTab::Market,
                installed_count: 3,

                accent: Accent::Crimson,
                appearance: Appearance::System,
                player: player::initial(),
                matching: matching::initial(),
                danmaku_settings: danmaku::initial(),
                playing: false,
                expanded: false,
                paused: false,
                title: String::new(),
                subtitle: PlayingNote::LocalFile,
                still: still(),
                video: false,
                progress: 0.0,
                elapsed: 0.0,
                duration: LENGTH,
                has_recents: !recents.is_empty(),
                recents,
                link_prompt: false,
                link_invalid: false,
                open_key: if cfg!(any(target_os = "macos", target_os = "ios")) {
                    "⌘O"
                } else {
                    "Ctrl+O"
                }
                .into(),
                user_name: "本地用户".into(),
                user_account: ProfileAccount::Local,
                user_avatar: None,
                account_status: Account::default(),
                bangumi: BangumiAccount::default(),
            },
        };
        engine.restore_welcome_preferences();
        engine.restore_playback_preferences();
        engine.state.player.subtitle_supersampling =
            engine.db.setting("subtitle_supersampling").as_deref() != Some("false");
        engine.state.player.skip_intro =
            engine.db.setting("skip_segments").as_deref() != Some("false");
        engine.state.danmaku_settings.supersampling =
            engine.db.setting("danmaku_supersampling").as_deref() != Some("false");
        if let Some(enabled) = &engine.saved_modules {
            engine.state.features = settings::feature_selection(enabled);
            engine.state.module_count = enabled.len() as i64;
            engine.state.preset = settings::preset_of(enabled);
        }
        #[cfg(feature = "bern-module-account")]
        if engine.state.features.account {
            engine.restore_account();
            engine.restore_bangumi();
        }
        engine
    }
}

/// The picture of the downloads' samples, which play on a simulated clock.
fn still() -> MediaSource {
    picture(STILL)
}

/// No picture: what a video shows until the kernel took one of it.
fn no_picture() -> MediaSource {
    MediaSource::new("")
}

/// A picture of `[resources]`, as the frame shows it.
fn picture(name: &str) -> MediaSource {
    MediaSource::resource(&Resources::host(), name).unwrap_or_else(|_| MediaSource::new(""))
}

/// The most videos 「最近播放」 lists.
const RECENT_COUNT: usize = 3;

/// A file's name without its extension, as the player titles it.
fn without_extension(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && !extension.contains(' ') => stem.into(),
        _ => name.into(),
    }
}

impl ErikaEngine {
    fn start(&mut self, title: String, subtitle: PlayingNote, still: MediaSource, progress: f64) {
        // What played until now keeps where it was.
        if self.state.playing {
            self.save_history();
        }
        self.file = None;
        self.still_path = None;
        self.saved_at = 0.0;
        self.episode_playing = None;
        self.match_revision += 1;
        self.search_revision += 1;
        self.danmaku_sources.clear();
        self.match_details = None;
        self.match_previews.clear();
        self.match_preview_queue.clear();
        self.match_previews_active = 0;
        self.online_danmaku_count = 0;
        self.state.matching = matching::initial();
        let progress = if progress.is_finite() {
            progress.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.reset_player(match subtitle {
            PlayingNote::Episode { episode } => Some(episode),
            _ => None,
        });
        self.state.playing = true;
        self.state.expanded = true;
        self.state.paused = false;
        self.state.title = title;
        self.state.subtitle = subtitle;
        self.state.still = still;
        self.state.progress = progress;
        self.state.elapsed = progress * LENGTH;
        self.state.duration = LENGTH;
    }

    fn require_playing(&self) -> Result<(), CommandError> {
        if self.state.playing {
            Ok(())
        } else {
            Err(CommandError::new("nothing_playing", "nothing is playing"))
        }
    }
}

impl erika::Commands for ErikaEngine {
    fn open_onboarding(&mut self) -> Result<(), CommandError> {
        self.state.onboarding = onboarding::initial();
        Ok(())
    }
    fn onboarding_next(&mut self) -> Result<(), CommandError> {
        self.advance_onboarding()
    }
    fn onboarding_back(&mut self) -> Result<(), CommandError> {
        self.state.onboarding.step = (self.state.onboarding.step - 1).max(0);
        Ok(())
    }
    fn onboarding_skip(&mut self) -> Result<(), CommandError> {
        self.select_onboarding_preset(OnboardingPreset::Pure);
        self.state.onboarding.step = 3;
        Ok(())
    }
    fn onboarding_preset(&mut self, preset: OnboardingPreset) -> Result<(), CommandError> {
        self.select_onboarding_preset(preset);
        self.state.onboarding.preview_revision += 1;
        Ok(())
    }
    fn onboarding_toggle(&mut self, id: String) -> Result<(), CommandError> {
        self.toggle_onboarding_module(&id)
    }

    fn set_startup(&mut self, value: StartupPage) -> Result<(), CommandError> {
        self.state.preferences.startup = value;
        Ok(())
    }
    fn set_playback_end(&mut self, value: PlaybackEnd) -> Result<(), CommandError> {
        self.db
            .set_setting("playback.end", Some(&format!("{value:?}")));
        self.state.preferences.playback_end = value;
        Ok(())
    }
    fn set_language(&mut self, value: InterfaceLanguage) -> Result<(), CommandError> {
        self.state.preferences.language = value;
        Ok(())
    }
    fn set_update_channel(&mut self, value: UpdateChannel) -> Result<(), CommandError> {
        self.state.preferences.update_channel = value;
        Ok(())
    }
    fn set_speed(&mut self, value: PlaybackSpeed) -> Result<(), CommandError> {
        self.db
            .set_setting("playback.speed", Some(&format!("{value:?}")));
        self.state.preferences.speed = value;
        self.apply_speed();
        Ok(())
    }
    fn set_danmaku_area(&mut self, value: DanmakuArea) -> Result<(), CommandError> {
        self.db
            .set_setting("danmaku.area", Some(&format!("{value:?}")));
        self.state.preferences.danmaku_area = value;
        self.apply_danmaku();
        Ok(())
    }
    fn set_preference(&mut self, key: String, value: bool) -> Result<(), CommandError> {
        match key.as_str() {
            "auto_updates" => self.state.preferences.auto_updates = value,
            "reduce_motion" => self.state.preferences.reduce_motion = value,
            "remember_position" => self.state.preferences.remember_position = value,
            "autoplay" => self.state.preferences.autoplay = value,
            "hardware_decode" => self.state.preferences.hardware_decode = value,
            "show_danmaku" => {
                self.state.preferences.show_danmaku = value;
                self.apply_danmaku();
            }
            "auto_match" => self.state.preferences.auto_match = value,
            "sync_storage" => self.state.preferences.sync_storage = value,
            _ => {
                return Err(CommandError::new(
                    "unknown_preference",
                    "unknown preference",
                ));
            }
        }
        self.db
            .set_setting(&key, Some(if value { "true" } else { "false" }));
        Ok(())
    }
    fn set_module(&mut self, id: String, enabled: bool) -> Result<(), CommandError> {
        if !settings::ALL_MODULES.contains(&id.as_str()) {
            return Err(CommandError::new("unknown_module", "unknown module"));
        }
        let previous = self.saved_modules.clone().unwrap_or_else(|| {
            settings::ALL_MODULES
                .iter()
                .map(|id| (*id).into())
                .collect()
        });
        let mut selected = previous.clone();
        settings::change_feature(&mut selected, &id, enabled);
        let mut affected = settings::ALL_MODULES
            .iter()
            .filter(|other| **other != id)
            .filter(|other| {
                previous.iter().any(|old| old == *other)
                    != selected.iter().any(|new| new == *other)
            })
            .map(|id| settings::feature_module(id));
        match (enabled, affected.next(), affected.next()) {
            (_, None, _) => {}
            (true, Some(module), None) => self.notify(Notice::AlsoOn { module }),
            (true, Some(first), Some(second)) => self.notify(Notice::AlsoOnBoth { first, second }),
            (false, Some(module), None) => self.notify(Notice::AlsoOff { module }),
            (false, Some(first), Some(second)) => {
                self.notify(Notice::AlsoOffBoth { first, second })
            }
        }
        self.apply_feature_selection(selected)
    }
    fn apply_preset(&mut self, preset: FeaturePreset) -> Result<(), CommandError> {
        self.state.feature_preview_revision += 1;
        let wanted: &[&str] = match preset {
            FeaturePreset::Pure => &[],
            FeaturePreset::Anime => settings::ANIME_MODULES,
            FeaturePreset::All => settings::ALL_MODULES,
        };
        self.apply_feature_selection(wanted.iter().map(|id| (*id).into()).collect())
    }
    fn set_plugin_category(&mut self, category: PluginCategory) -> Result<(), CommandError> {
        self.state.plugin_category = category;
        self.filter_plugins();
        Ok(())
    }
    fn set_plugin_tab(&mut self, tab: PluginTab) -> Result<(), CommandError> {
        self.state.plugin_tab = tab;
        self.filter_plugins();
        Ok(())
    }
    fn use_plugin(&mut self, id: String) -> Result<(), CommandError> {
        let plugin = self
            .all_plugins
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| CommandError::new("unknown_plugin", "unknown plugin"))?;
        let installed = plugin.installed;
        let name = plugin.name.clone();
        plugin.installed = true;
        self.filter_plugins();
        self.notify(if installed {
            Notice::PluginSettings { name }
        } else {
            Notice::PluginAdded { name }
        });
        Ok(())
    }
    fn show_notice(&mut self, notice: Notice) -> Result<(), CommandError> {
        self.notify(notice);
        Ok(())
    }

    fn open_file(&mut self, source: MediaSource) -> Result<(), CommandError> {
        let title = self.source_name(&source);
        let (file, fingerprint) = self.identify(&source);
        let (start, progress) = self.saved_position(file);
        self.start(title, PlayingNote::LocalFile, no_picture(), progress);
        self.file = file;
        self.prepare_queue(&source);
        self.load(Some(source.clone()), start);
        self.recognize(&source, fingerprint);
        Ok(())
    }

    fn browser_file(
        &mut self,
        client: String,
        url: String,
        name: String,
    ) -> Result<(), CommandError> {
        self.kernel
            .as_ref()
            .ok_or_else(|| CommandError::new("no_kernel", "Playback is unavailable"))?
            .browser_file(&client, &url, &name)
            .map_err(|e| CommandError::new("browser_file", e))
    }

    fn browser_exchange(&mut self, client: String, packet: String) -> Result<String, CommandError> {
        self.kernel
            .as_ref()
            .ok_or_else(|| CommandError::new("no_kernel", "Playback is unavailable"))?
            .browser_exchange(&client, &packet)
            .map_err(|e| CommandError::new("browser_exchange", e))
    }

    fn open_folder(&mut self, source: MediaSource) -> Result<(), CommandError> {
        let path = bern::files::file_path(&source.url)
            .ok_or_else(|| CommandError::new("invalid_folder", "请选择本地文件夹"))?;
        let entries = queue::folder(&path)?;
        if entries.is_empty() {
            return Err(CommandError::new(
                "empty_folder",
                "文件夹中没有可播放的视频",
            ));
        }
        self.queue = entries;
        self.play_queue(1, true)
    }

    fn show_link_prompt(&mut self, visible: bool) -> Result<(), CommandError> {
        self.state.link_prompt = visible;
        self.state.link_invalid = false;
        Ok(())
    }

    fn open_link(&mut self, link: String) -> Result<(), CommandError> {
        let link = link.trim();
        let web = link.starts_with("http://") || link.starts_with("https://");
        if !web && !link.starts_with("magnet:") {
            // The prompt stays and says what a link looks like.
            self.state.link_invalid = true;
            return Ok(());
        }
        if link.starts_with("magnet:") {
            #[cfg(not(feature = "bern-module-downloads"))]
            return Err(CommandError::new(
                "module_unavailable",
                "此版本未包含下载模块",
            ));
            #[cfg(feature = "bern-module-downloads")]
            {
                if !self.state.features.downloads {
                    return Err(CommandError::new(
                        "downloads_disabled",
                        "请先在可选功能中启用下载，再打开磁力链接",
                    ));
                }
                self.state.link_prompt = false;
                self.state.link_invalid = false;
                erika_downloads_protocol::downloads::calls::open_add();
                erika_downloads_protocol::downloads::calls::read_link(link.to_string());
                return Ok(());
            }
        }
        // The last part of the address names it, or the whole link.
        let path = link.split(['?', '#']).next().unwrap_or(link);
        let name = path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .filter(|name| web && !name.is_empty() && !name.contains(':'));
        let title = name
            .map(without_extension)
            .unwrap_or_else(|| link.to_string());
        self.state.link_prompt = false;
        self.state.link_invalid = false;
        self.start(title, PlayingNote::Link, no_picture(), 0.0);
        let source = MediaSource::new(link);
        self.prepare_queue(&source);
        self.load(Some(source), 0.0);
        Ok(())
    }

    fn play_recent(&mut self, name: String) -> Result<(), CommandError> {
        let recent = self
            .state
            .recents
            .iter()
            .find(|recent| recent.name == name)
            .cloned()
            .ok_or_else(|| CommandError::new("no_such_recent", "no recent video has this name"))?;
        // Where its file stopped, or what the list says of a web address.
        let (file, fingerprint) = recent
            .source
            .as_ref()
            .map(|source| self.identify(source))
            .unwrap_or((None, None));
        let (start, progress) = if file.is_some() {
            self.saved_position(file)
        } else if self.state.preferences.remember_position && recent.progress < 1.0 {
            (
                recent.remaining / (1.0 - recent.progress) * recent.progress,
                recent.progress,
            )
        } else {
            (0.0, 0.0)
        };
        self.start(
            without_extension(&recent.name),
            PlayingNote::LocalFile,
            recent.still,
            progress,
        );
        self.file = file;
        if let Some(source) = &recent.source {
            self.prepare_queue(source);
        }
        self.load(recent.source.clone(), start);
        if let Some(source) = &recent.source {
            self.recognize(source, fingerprint);
        }
        Ok(())
    }

    fn continue_last(&mut self) -> Result<(), CommandError> {
        let name = self
            .state
            .recents
            .first()
            .map(|recent| recent.name.clone())
            .ok_or_else(|| CommandError::new("no_such_recent", "nothing played yet"))?;
        self.play_recent(name)
    }

    fn return_to_frame(&mut self) -> Result<(), CommandError> {
        self.require_playing()?;
        self.save_history();
        self.show_recents();
        self.state.playing = false;
        self.state.expanded = false;
        self.state.paused = false;
        self.state.player.panel = false;
        self.unload();
        Ok(())
    }

    fn play(
        &mut self,
        title: String,
        episode: i64,
        still: MediaSource,
        progress: f64,
        source: MediaSource,
    ) -> Result<(), CommandError> {
        if source.url.is_empty() {
            self.notify(Notice::NoEpisodeFile);
            return Ok(());
        }
        let (file, fingerprint) = self.identify(&source);
        let (start, saved) = self.saved_position(file);
        self.start(
            title,
            PlayingNote::Episode { episode },
            still,
            if file.is_some() { saved } else { progress },
        );
        self.file = file;
        self.prepare_queue(&source);
        self.load(Some(source.clone()), start);
        self.recognize(&source, fingerprint);
        Ok(())
    }

    #[cfg(feature = "bern-module-downloads")]
    fn play_download(
        &mut self,
        title: String,
        source: MediaSource,
        downloading: bool,
    ) -> Result<(), CommandError> {
        // A finished file is read like any other, so it is recognized and
        // its danmaku come; a stream is played as it arrives.
        let (file, fingerprint) = if downloading {
            (None, None)
        } else {
            self.identify(&source)
        };
        let (start, progress) = self.saved_position(file);
        self.start(
            title,
            if downloading {
                PlayingNote::Downloading
            } else {
                PlayingNote::Downloaded
            },
            no_picture(),
            progress,
        );
        self.file = file;
        self.prepare_queue(&source);
        self.load(Some(source.clone()), start);
        if !downloading {
            self.recognize(&source, fingerprint);
        }
        Ok(())
    }

    #[cfg(feature = "bern-module-downloads")]
    fn download_finished(&mut self, folder: MediaSource) -> Result<(), CommandError> {
        // Downloads are anime until the user says otherwise in the sources.
        #[cfg(feature = "bern-module-library")]
        erika_library_protocol::library::calls::add_folder(
            folder,
            erika_library_protocol::library::SourceContent::Anime,
        );
        Ok(())
    }

    #[cfg(feature = "bern-module-library")]
    fn open_library_show(&mut self, id: i64) -> Result<(), CommandError> {
        #[cfg(feature = "bern-module-anime")]
        erika_anime_protocol::anime::calls::open_from_library(id);
        Ok(())
    }

    #[cfg(feature = "bern-module-library")]
    fn library_files_changed(&mut self) -> Result<(), CommandError> {
        #[cfg(feature = "bern-module-anime")]
        erika_anime_protocol::anime::calls::history_changed();
        Ok(())
    }

    #[cfg(feature = "bern-module-sync")]
    fn sync_restored(
        &mut self,
        settings: bool,
        accounts: bool,
        library: bool,
        history: bool,
        sources: Vec<i64>,
    ) -> Result<(), CommandError> {
        self.take_synced(settings, accounts, library, history, sources);
        Ok(())
    }

    fn minimize(&mut self) -> Result<(), CommandError> {
        self.require_playing()?;
        self.capture_still();
        self.state.expanded = false;
        self.state.player.panel = false;
        Ok(())
    }

    fn expand(&mut self) -> Result<(), CommandError> {
        self.require_playing()?;
        self.state.expanded = true;
        Ok(())
    }

    fn close_player(&mut self) -> Result<(), CommandError> {
        if self.state.playing {
            self.save_history();
            self.show_recents();
        }
        self.state.playing = false;
        self.state.expanded = false;
        self.unload();
        Ok(())
    }

    fn toggle_pause(&mut self) -> Result<(), CommandError> {
        self.require_playing()?;
        self.toggle();
        self.state.player.feedback = true;
        self.feedback_remaining = 0.5;
        Ok(())
    }

    fn player_tick(&mut self) -> Result<(), CommandError> {
        self.tick_player();
        Ok(())
    }
    fn seek(&mut self, progress: f64) -> Result<(), CommandError> {
        self.require_playing()?;
        if !progress.is_finite() {
            return Err(CommandError::new(
                "invalid_progress",
                "progress must be finite",
            ));
        }
        let length = if self.state.video {
            self.state.duration
        } else {
            LENGTH
        };
        self.update_position(progress.clamp(0.0, 1.0) * length);
        Ok(())
    }
    fn skip(&mut self, seconds: f64) -> Result<(), CommandError> {
        self.require_playing()?;
        if !seconds.is_finite() {
            return Err(CommandError::new("invalid_time", "time must be finite"));
        }
        self.update_position(self.state.elapsed + seconds);
        Ok(())
    }
    fn seek_chapter(&mut self, id: i64) -> Result<(), CommandError> {
        self.jump_to_chapter(id)
    }
    fn skip_segment(&mut self) -> Result<(), CommandError> {
        self.skip_current_segment()
    }
    fn select_episode(&mut self, episode: i64) -> Result<(), CommandError> {
        self.player_episode(episode)
    }
    fn change_episode(&mut self, delta: i64) -> Result<(), CommandError> {
        self.player_episode(self.state.player.queue_index.saturating_add(delta))
    }
    fn player_panel(&mut self, visible: bool) -> Result<(), CommandError> {
        self.require_playing()?;
        self.state.player.panel = visible;
        if !self.state.features.danmaku && self.state.player.tab == PlayerTab::Danmaku {
            self.state.player.tab = PlayerTab::Subtitles;
        }
        if !self.state.features.enhance && self.state.player.tab == PlayerTab::Picture {
            self.state.player.tab = PlayerTab::Subtitles;
        }
        Ok(())
    }
    fn player_tab(&mut self, tab: PlayerTab) -> Result<(), CommandError> {
        self.require_playing()?;
        self.state.player.tab = tab;
        self.state.player.panel = true;
        Ok(())
    }
    fn select_subtitle_track(&mut self, id: i64) -> Result<(), CommandError> {
        self.select_subtitle(id)
    }
    fn select_audio_track(&mut self, id: i64) -> Result<(), CommandError> {
        self.select_audio(id)
    }
    fn set_enhancement(&mut self, value: Enhancement) -> Result<(), CommandError> {
        self.db
            .set_setting("player.enhancement", Some(&format!("{value:?}")));
        self.state.player.enhancement = value;
        self.apply_enhancement();
        Ok(())
    }
    fn set_subtitle_supersampling(&mut self, enabled: bool) -> Result<(), CommandError> {
        self.state.player.subtitle_supersampling = enabled;
        self.db.set_setting(
            "subtitle_supersampling",
            Some(if enabled { "true" } else { "false" }),
        );
        if let Some(kernel) = self.kernel() {
            kernel.set_subtitle_supersampling(enabled);
        }
        Ok(())
    }
    fn set_player_number(&mut self, key: String, value: f64) -> Result<(), CommandError> {
        self.player_number(&key, value)
    }
    fn set_skip_intro(&mut self, value: bool) -> Result<(), CommandError> {
        self.state.player.skip_intro = value;
        self.db
            .set_setting("skip_segments", Some(if value { "true" } else { "false" }));
        self.update_chapter_position();
        Ok(())
    }
    fn screenshot(&mut self) -> Result<(), CommandError> {
        self.require_playing()?;
        self.save_screenshot()
    }
    fn show_matching(&mut self, open: bool) -> Result<(), CommandError> {
        self.require_playing()?;
        self.state.matching.open = open;
        Ok(())
    }
    fn search_matching(&mut self, query: String) -> Result<(), CommandError> {
        self.search_match(query)
    }
    fn choose_match_show(&mut self, id: i64) -> Result<(), CommandError> {
        self.choose_show(id)
    }
    fn back_to_match_results(&mut self) -> Result<(), CommandError> {
        self.state.matching.selected = None;
        self.state.matching.episodes.clear();
        self.state.matching.error.clear();
        self.state.matching.loading = false;
        self.match_details = None;
        Ok(())
    }
    fn confirm_match_episode(&mut self, id: i64) -> Result<(), CommandError> {
        self.confirm_episode(id)
    }
    fn send_danmaku(&mut self, text: String) -> Result<(), CommandError> {
        self.require_playing()?;
        if !self.state.features.danmaku {
            return Err(CommandError::new(
                "feature_disabled",
                "danmaku must be enabled",
            ));
        }
        self.send_online_danmaku(text)
    }
    fn set_danmaku_number(&mut self, key: String, value: f64) -> Result<(), CommandError> {
        self.danmaku_number(&key, value)
    }
    fn set_danmaku_switch(&mut self, key: String, value: bool) -> Result<(), CommandError> {
        self.danmaku_switch(&key, value)
    }
    fn mount_danmaku(&mut self, source: MediaSource) -> Result<(), CommandError> {
        self.mount_danmaku_file(&source);
        Ok(())
    }
    fn attach_subtitle(&mut self, source: MediaSource) -> Result<(), CommandError> {
        self.require_playing()?;
        if !self.state.video {
            return Err(CommandError::new("no_video", "请先打开视频"));
        }
        self.add_subtitle(&source);
        Ok(())
    }

    fn set_accent(&mut self, accent: Accent) -> Result<(), CommandError> {
        self.state.accent = accent;
        self.save_welcome_preferences()?;
        Ok(())
    }

    #[cfg(feature = "bern-module-account")]
    fn sign_in(&mut self, user: String, password: String) -> Result<(), CommandError> {
        self.require_account()?;
        self.sign_in_started(user, password);
        Ok(())
    }

    #[cfg(feature = "bern-module-account")]
    fn sign_out(&mut self) -> Result<(), CommandError> {
        self.require_account()?;
        self.sign_out_now();
        Ok(())
    }

    #[cfg(feature = "bern-module-account")]
    fn open_account_login(&mut self, settings_window: bool) -> Result<(), CommandError> {
        self.require_account()?;
        self.open_account_prompt(settings_window);
        Ok(())
    }
    #[cfg(feature = "bern-module-account")]
    fn close_account_login(&mut self) -> Result<(), CommandError> {
        self.require_account()?;
        self.close_account_prompt();
        Ok(())
    }

    #[cfg(feature = "bern-module-account")]
    fn cancel_account_login(&mut self) -> Result<(), CommandError> {
        self.require_account()?;
        self.close_account_prompt();
        self.close_bangumi();
        Ok(())
    }
    #[cfg(feature = "bern-module-account")]
    fn submit_account_login(&mut self, bangumi: bool, user: String, password: String, captcha: String) -> Result<(), CommandError> {
        self.require_account()?;
        if bangumi && self.state.bangumi.prompt {
            self.submit_bangumi(user, password, captcha);
        } else if !bangumi && self.state.account_status.prompt {
            self.sign_in_started(user, password);
        }
        Ok(())
    }

    #[cfg(feature = "bern-module-account")]
    fn open_bangumi_login(&mut self, settings_window: bool) -> Result<(), CommandError> {
        self.require_account()?;
        self.close_account_prompt();
        self.open_bangumi(settings_window);
        Ok(())
    }
    #[cfg(feature = "bern-module-account")]
    fn refresh_bangumi_captcha(&mut self) -> Result<(), CommandError> {
        self.require_account()?;
        self.prepare_bangumi();
        Ok(())
    }
    #[cfg(feature = "bern-module-account")]
    fn close_bangumi_login(&mut self) -> Result<(), CommandError> {
        self.require_account()?;
        self.close_bangumi();
        Ok(())
    }
    #[cfg(feature = "bern-module-account")]
    fn bangumi_sign_in(&mut self, email: String, password: String, captcha: String) -> Result<(), CommandError> {
        self.require_account()?;
        self.submit_bangumi(email, password, captcha);
        Ok(())
    }
    #[cfg(feature = "bern-module-account")]
    fn bangumi_sign_out(&mut self) -> Result<(), CommandError> {
        self.require_account()?;
        self.forget_bangumi()
    }
    #[cfg(feature = "bern-module-account")]
    fn check_bangumi_session(&mut self) -> Result<(), CommandError> {
        self.require_account()?;
        self.check_bangumi();
        Ok(())
    }
    #[cfg(feature = "bern-module-account")]
    fn dismiss_bangumi_expired(&mut self) -> Result<(), CommandError> {
        self.require_account()?;
        self.state.bangumi.expired = false;
        Ok(())
    }

    fn set_appearance(&mut self, appearance: Appearance) -> Result<(), CommandError> {
        self.state.appearance = appearance;
        self.save_welcome_preferences()?;
        Ok(())
    }
}

impl erika::Engine for ErikaEngine {
    /// Work done on other threads comes back; once as the app starts, the
    /// account's token is renewed when it is old.
    fn wake(&mut self) {
        self.take_kernel_results();
        #[cfg(feature = "bern-module-account")]
        {
            self.take_account_results();
            self.take_bangumi_results();
            self.check_bangumi();
        }
        self.take_found();
        #[cfg(feature = "bern-module-account")]
        self.renew_when_old();
    }

    fn initial_modules(&self) -> Option<Vec<String>> {
        self.saved_modules.as_deref().map(settings::runtime_modules)
    }
    fn modules_changed(&mut self, enabled: &[String]) {
        if let Some(wanted) = &self.pending_modules {
            if wanted.len() == enabled.len() && wanted.iter().all(|id| enabled.contains(id)) {
                self.pending_modules = None;
            }
            // Runtime anime can be on solely for library details; callbacks
            // must not turn the user's New Shows switch back on.
            return;
        }
        self.publish_feature_selection(enabled);
        if let Err(error) = self.save_welcome_preferences() {
            self.notify(Notice::SaveFailed {
                reason: error.message,
            });
        }
    }

    fn state(&self) -> &State {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use erika::Commands;

    #[test]
    fn the_player_shrinks_into_the_mini_player_and_grows_back() {
        let mut engine = ErikaEngine::default();
        assert_eq!(engine.minimize().unwrap_err().code, "nothing_playing");
        engine
            .play(
                "葬送的芙莉莲".into(),
                4,
                still(),
                0.35,
                MediaSource::new("file:///Videos/frieren-04.mkv"),
            )
            .unwrap();
        assert!(engine.state.playing && engine.state.expanded);
        assert_eq!(engine.state.elapsed.floor(), 8.0 * 60.0 + 29.0);
        assert_eq!(engine.state.subtitle, PlayingNote::Episode { episode: 4 });
        engine.minimize().unwrap();
        assert!(engine.state.playing && !engine.state.expanded);
        engine.expand().unwrap();
        assert!(engine.state.expanded);
        engine.close_player().unwrap();
        assert!(!engine.state.playing);
    }

    #[test]
    fn links_play_and_invalid_folders_do_not_replace_playback() {
        let mut engine = ErikaEngine::default();
        engine.show_link_prompt(true).unwrap();
        engine.open_link("ftp://example.com/a.mkv".into()).unwrap();
        assert!(engine.state.link_prompt && engine.state.link_invalid && !engine.state.playing);
        engine
            .open_link(" https://example.com/videos/Kyoto.mp4?t=3 ".into())
            .unwrap();
        assert_eq!(engine.state.title, "Kyoto");
        assert_eq!(engine.state.subtitle, PlayingNote::Link);
        assert!(!engine.state.link_prompt && !engine.state.link_invalid);
        engine.open_link("magnet:?xt=urn:btih:abc".into()).unwrap();
        assert_eq!(engine.state.title, "Kyoto");
        assert!(
            engine
                .open_folder(MediaSource::new("file:///definitely-missing-erika-folder/"))
                .is_err()
        );
        assert_eq!(engine.state.title, "Kyoto");
    }

    #[test]
    fn files_play_under_their_names() {
        let mut engine = ErikaEngine::default();
        engine
            .open_file(MediaSource::new("file:///Videos/%E5%BD%B1.mkv"))
            .unwrap();
        assert_eq!(engine.state.title, "影.mkv");
        assert_eq!(engine.state.subtitle, PlayingNote::LocalFile);
    }
}
