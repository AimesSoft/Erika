//! The engine of Erika's shell. liberika plays the files and links the
//! user opens; the modules' sample media still play on a simulated clock
//! over their stills.

mod settings;
mod onboarding;

mod account;

mod danmaku;
mod history;
mod kernel;
mod matching;
mod search;
mod player;

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
    pub(crate) onboarding_completed: bool,
    /// liberika, where it runs, and the media it plays.
    kernel: Option<Arc<dyn Kernel>>,
    source: Option<MediaSource>,
    /// A pause or a seek the user asked for, with the ticks it has waited
    /// for the kernel to show it.
    pause_wanted: Option<(bool, u8)>,
    seek_wanted: Option<(f64, u8)>,
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
    account_inbox: account::Inbox,
    /// What matching the video playing came back with, and the dandanplay
    /// episode it was matched to.
    found: Arc<std::sync::Mutex<Vec<matching::Found>>>,
    episode_playing: Option<i64>,
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
        inbox.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).extend(results);
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

impl Default for ErikaEngine {
    fn default() -> Self {
        let db = erika_store::Db::open();
        let recents = history::recents(&db);
        let (account, token) = account::restore(&db);
        erika_dandanplay::set_token(token);
        // The browser has no process id.
        let process = if cfg!(target_arch = "wasm32") { 0 } else { std::process::id() };
        let run = format!("{}-{process}-", erika_store::now());
        let mut engine = Self {
            db,
            file: None,
            still_path: None,
            saved_at: 0.0,
            run,
            account_inbox: Arc::default(),
            found: Arc::default(),
            episode_playing: None,
            feedback_remaining: 0.0,
            saved_modules: None,
            onboarding_completed: false,
            kernel: if cfg!(test) { None } else { kernel::start() },
            source: None,
            pause_wanted: None,
            seek_wanted: None,
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
                module_count: 8,
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
                open_key: if cfg!(any(target_os = "macos", target_os = "ios")) { "⌘O" } else { "Ctrl+O" }.into(),
                user_name: account::display_name(&account),
                account,
            },
        };
        engine.restore_welcome_preferences();
        if let Some(enabled) = &engine.saved_modules {
            engine.state.features = settings::feature_selection(enabled);
            engine.state.module_count = enabled.len() as i64;
            engine.state.preset = settings::preset_of(enabled);
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
        let progress = if progress.is_finite() { progress.clamp(0.0, 1.0) } else { 0.0 };
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
    fn onboarding_next(&mut self) -> Result<(), CommandError> { self.advance_onboarding() }
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
    fn onboarding_toggle(&mut self, id: String) -> Result<(), CommandError> { self.toggle_onboarding_module(&id) }

    fn set_startup(&mut self, value: StartupPage) -> Result<(), CommandError> {
        self.state.preferences.startup = value;
        Ok(())
    }
    fn set_playback_end(&mut self, value: PlaybackEnd) -> Result<(), CommandError> {
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
        self.state.preferences.speed = value;
        self.apply_speed();
        Ok(())
    }
    fn set_danmaku_area(&mut self, value: DanmakuArea) -> Result<(), CommandError> {
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
            "step_back" => self.state.preferences.step_back = value,
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
        Ok(())
    }
    fn set_module(&mut self, id: String, enabled: bool) -> Result<(), CommandError> {
        if !settings::ALL_MODULES.contains(&id.as_str()) {
            return Err(CommandError::new("unknown_module", "unknown module"));
        }
        let f = &self.state.features;
        let mut affected = Vec::new();
        match (id.as_str(), enabled) {
            ("library", false) => {
                if f.sources {
                    affected.push(FeatureModule::Sources);
                }
                if f.remote {
                    affected.push(FeatureModule::Remote);
                }
            }
            ("anime", false) if f.sync => affected.push(FeatureModule::Sync),
            ("sync", true) if !f.anime => affected.push(FeatureModule::Anime),
            ("sources" | "remote", true) if !f.library => affected.push(FeatureModule::Library),
            _ => {}
        }
        let mut affected = affected.into_iter();
        match (enabled, affected.next(), affected.next()) {
            (_, None, _) => {}
            (true, Some(module), _) => self.notify(Notice::AlsoOn { module }),
            (false, Some(module), None) => self.notify(Notice::AlsoOff { module }),
            (false, Some(first), Some(second)) => self.notify(Notice::AlsoOffBoth { first, second }),
        }
        bern::modules::set_enabled(&id, enabled);
        Ok(())
    }
    fn apply_preset(&mut self, preset: FeaturePreset) -> Result<(), CommandError> {
        self.state.feature_preview_revision += 1;
        let wanted: &[&str] = match preset {
            FeaturePreset::Pure => &[],
            FeaturePreset::Anime => &["danmaku", "library", "anime"],
            FeaturePreset::All => &settings::ALL_MODULES,
        };
        // Stop dependents first; enable prerequisites first. Composite still
        // owns the dependency rules and publishes the resulting module state.
        for id in settings::ALL_MODULES.iter().rev() {
            if !wanted.contains(id) {
                bern::modules::set_enabled(id, false);
            }
        }
        for id in settings::ALL_MODULES {
            if wanted.contains(&id) {
                bern::modules::set_enabled(id, true);
            }
        }
        Ok(())
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
        let title = source.file_name().unwrap_or_else(|| source.url.clone());
        let (file, fingerprint) = self.identify(&source);
        let (start, progress) = self.saved_position(file);
        self.start(title, PlayingNote::LocalFile, no_picture(), progress);
        self.file = file;
        self.load(Some(source.clone()), start);
        self.recognize(&source, fingerprint);
        if self.state.video {
            self.mount_danmaku_beside(&source);
        }
        Ok(())
    }

    fn open_folder(&mut self, source: MediaSource) -> Result<(), CommandError> {
        // Until the kernel reads folders, the folder plays under its name.
        let name = source.file_name().unwrap_or_else(|| {
            source.url.trim_end_matches('/').rsplit('/').next().unwrap_or_default().to_string()
        });
        self.start(name, PlayingNote::LocalFile, no_picture(), 0.0);
        self.load(None, 0.0);
        Ok(())
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
        // The last part of the address names it, or the whole link.
        let path = link.split(['?', '#']).next().unwrap_or(link);
        let name = path.trim_end_matches('/').rsplit('/').next().filter(|name| web && !name.is_empty() && !name.contains(':'));
        let title = name.map(without_extension).unwrap_or_else(|| link.to_string());
        self.state.link_prompt = false;
        self.state.link_invalid = false;
        self.start(title, PlayingNote::Link, no_picture(), 0.0);
        self.load(web.then(|| MediaSource::new(link)), 0.0);
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
        let (file, fingerprint) = recent.source.as_ref().map(|source| self.identify(source)).unwrap_or((None, None));
        let (start, progress) = if file.is_some() {
            self.saved_position(file)
        } else if recent.progress < 1.0 {
            (recent.remaining / (1.0 - recent.progress) * recent.progress, recent.progress)
        } else {
            (0.0, 0.0)
        };
        self.start(without_extension(&recent.name), PlayingNote::LocalFile, recent.still, progress);
        self.file = file;
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
        self.start(title, PlayingNote::Episode { episode }, still, if file.is_some() { saved } else { progress });
        self.file = file;
        self.load(Some(source.clone()), start);
        self.recognize(&source, fingerprint);
        Ok(())
    }

    fn play_download(&mut self, title: String, source: MediaSource, downloading: bool) -> Result<(), CommandError> {
        // A finished file is read like any other, so it is recognized and
        // its danmaku come; a stream is played as it arrives.
        let (file, fingerprint) = if downloading { (None, None) } else { self.identify(&source) };
        let (start, progress) = self.saved_position(file);
        self.start(title, if downloading { PlayingNote::Downloading } else { PlayingNote::Downloaded }, no_picture(), progress);
        self.file = file;
        self.load(Some(source.clone()), start);
        if !downloading {
            self.recognize(&source, fingerprint);
        }
        Ok(())
    }

    fn download_finished(&mut self, folder: MediaSource) -> Result<(), CommandError> {
        erika_library_protocol::library::calls::add_folder(folder);
        Ok(())
    }

    fn open_library_show(&mut self, id: i64) -> Result<(), CommandError> {
        erika_anime_protocol::anime::calls::open_from_library(id);
        Ok(())
    }

    fn library_files_changed(&mut self) -> Result<(), CommandError> {
        erika_anime_protocol::anime::calls::history_changed();
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

    fn player_tick(&mut self) -> Result<(), CommandError> { self.tick_player(); Ok(()) }
    fn seek(&mut self, progress: f64) -> Result<(), CommandError> {
        self.require_playing()?;
        if !progress.is_finite() { return Err(CommandError::new("invalid_progress", "progress must be finite")); }
        let length = if self.state.video { self.state.duration } else { LENGTH };
        self.update_position(progress.clamp(0.0, 1.0) * length); Ok(())
    }
    fn skip(&mut self, seconds: f64) -> Result<(), CommandError> {
        self.require_playing()?;
        if !seconds.is_finite() { return Err(CommandError::new("invalid_time", "time must be finite")); }
        self.update_position(self.state.elapsed + seconds); Ok(())
    }
    fn select_episode(&mut self, episode: i64) -> Result<(), CommandError> { self.player_episode(episode) }
    fn change_episode(&mut self, delta: i64) -> Result<(), CommandError> {
        self.player_episode(self.state.player.episode.saturating_add(delta))
    }
    fn player_panel(&mut self, visible: bool) -> Result<(), CommandError> {
        self.require_playing()?; self.state.player.panel = visible;
        if !self.state.features.danmaku && self.state.player.tab == PlayerTab::Danmaku { self.state.player.tab = PlayerTab::Subtitles; }
        if !self.state.features.enhance && self.state.player.tab == PlayerTab::Picture { self.state.player.tab = PlayerTab::Subtitles; }
        Ok(())
    }
    fn player_tab(&mut self, tab: PlayerTab) -> Result<(), CommandError> {
        self.require_playing()?; self.state.player.tab = tab; self.state.player.panel = true; Ok(())
    }
    fn select_subtitle_track(&mut self, id: i64) -> Result<(), CommandError> { self.select_subtitle(id) }
    fn select_audio_track(&mut self, id: i64) -> Result<(), CommandError> { self.select_audio(id) }
    fn set_enhancement(&mut self, value: Enhancement) -> Result<(), CommandError> { self.state.player.enhancement = value; self.apply_enhancement(); Ok(()) }
    fn set_player_number(&mut self, key: String, value: f64) -> Result<(), CommandError> { self.player_number(&key, value) }
    fn set_skip_intro(&mut self, value: bool) -> Result<(), CommandError> { self.state.player.skip_intro = value; Ok(()) }
    fn screenshot(&mut self) -> Result<(), CommandError> { self.require_playing()?; self.save_screenshot() }
    fn send_danmaku(&mut self, text: String) -> Result<(), CommandError> {
        self.require_playing()?;
        if !self.state.features.danmaku || !self.state.features.sync { return Err(CommandError::new("feature_disabled", "danmaku and sync must be enabled")); }
        self.send_own_danmaku(&text); Ok(())
    }
    fn set_danmaku_number(&mut self, key: String, value: f64) -> Result<(), CommandError> { self.danmaku_number(&key, value) }
    fn set_danmaku_switch(&mut self, key: String, value: bool) -> Result<(), CommandError> { self.danmaku_switch(&key, value) }
    fn mount_danmaku(&mut self, source: MediaSource) -> Result<(), CommandError> { self.mount_danmaku_file(&source); Ok(()) }
    fn attach_subtitle(&mut self, source: MediaSource) -> Result<(), CommandError> {
        self.add_subtitle(&source);
        self.state.player.subtitle_file = source.file_name().unwrap_or_default();
        self.notify(Notice::SubtitleChosen { file: self.state.player.subtitle_file.clone() }); Ok(())
    }

    fn set_accent(&mut self, accent: Accent) -> Result<(), CommandError> {
        self.state.accent = accent;
        self.save_welcome_preferences()?;
        Ok(())
    }

    fn sign_in(&mut self, user: String, password: String) -> Result<(), CommandError> {
        self.sign_in_started(user, password);
        Ok(())
    }

    fn sign_out(&mut self) -> Result<(), CommandError> {
        self.sign_out_now();
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
        self.take_account_results();
        self.take_found();
        self.renew_when_old();
    }

    fn initial_modules(&self) -> Option<Vec<String>> { self.saved_modules.clone() }
    fn modules_changed(&mut self, enabled: &[String]) {
        self.saved_modules = Some(enabled.to_vec());
        if let Err(error) = self.save_welcome_preferences() { self.notify(Notice::SaveFailed { reason: error.message }); }
        self.state.features = settings::feature_selection(enabled);
        self.state.module_count = enabled.len() as i64;
        self.state.preset = settings::preset_of(enabled);
        self.apply_enhancement();
        self.apply_danmaku();
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
            .play("葬送的芙莉莲".into(), 4, still(), 0.35, MediaSource::new("file:///Videos/frieren-04.mkv"))
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
    fn links_and_folders_play_under_their_names() {
        let mut engine = ErikaEngine::default();
        engine.show_link_prompt(true).unwrap();
        engine.open_link("ftp://example.com/a.mkv".into()).unwrap();
        assert!(engine.state.link_prompt && engine.state.link_invalid && !engine.state.playing);
        engine.open_link(" https://example.com/videos/Kyoto.mp4?t=3 ".into()).unwrap();
        assert_eq!(engine.state.title, "Kyoto");
        assert_eq!(engine.state.subtitle, PlayingNote::Link);
        assert!(!engine.state.link_prompt && !engine.state.link_invalid);
        engine.open_link("magnet:?xt=urn:btih:abc".into()).unwrap();
        assert_eq!(engine.state.title, "magnet:?xt=urn:btih:abc");
        engine.open_folder(MediaSource::new("file:///Videos/Trip/")).unwrap();
        assert_eq!(engine.state.title, "Trip");
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
