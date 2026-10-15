//! The engine of Erika's shell. In this first phase nothing plays: the
//! player keeps what the interface needs to show, and the picture in the
//! frame is a still until the kernel draws video.

mod settings;
mod onboarding;

mod search;

use bern::CommandError;
use bern::media::MediaSource;
use bern::query::Answer;
use bern::resources::Resources;
use erika_protocol::erika::{self, *};

/// An episode's length in seconds, until the kernel reports real ones.
const LENGTH: f64 = 1455.0;

/// The still the frame shows for a file, until the kernel draws video.
const STILL: &str = "design/assets/still1.jpg";

pub struct ErikaEngine {
    pub(crate) state: State,
    pub(crate) all_plugins: Vec<Plugin>,
    pub(crate) saved_modules: Option<Vec<String>>,
    pub(crate) onboarding_completed: bool,
}

impl Default for ErikaEngine {
    fn default() -> Self {
        let mut engine = Self {
            saved_modules: None,
            onboarding_completed: false,
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
                preset_name: "全都要".into(),
                notice: String::new(),
                notice_revision: 0,
                plugins: settings::sample_plugins(),
                plugin_category: PluginCategory::All,
                plugin_tab: PluginTab::Market,
                installed_count: 3,

                accent: Accent::Crimson,
                appearance: Appearance::System,
                playing: false,
                expanded: false,
                paused: false,
                title: String::new(),
                subtitle: String::new(),
                still: still(),
                progress: 0.0,
                elapsed: clock(0.0),
                duration: clock(LENGTH),
                user_name: "Nipa".into(),
            },
        };
        engine.restore_welcome_preferences();
        engine
    }
}

fn still() -> MediaSource {
    MediaSource::resource(&Resources::host(), STILL).unwrap_or_else(|_| MediaSource::new(""))
}

/// `m:ss` of a number of seconds.
fn clock(seconds: f64) -> String {
    let seconds = seconds.max(0.0).round() as i64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

impl ErikaEngine {
    fn start(&mut self, title: String, subtitle: String, still: MediaSource, progress: f64) {
        let progress = progress.clamp(0.0, 1.0);
        self.state.playing = true;
        self.state.expanded = true;
        self.state.paused = false;
        self.state.title = title;
        self.state.subtitle = subtitle;
        self.state.still = still;
        self.state.progress = progress;
        self.state.elapsed = clock(progress * LENGTH);
        self.state.duration = clock(LENGTH);
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
        Ok(())
    }
    fn set_kernel(&mut self, value: PlaybackKernel) -> Result<(), CommandError> {
        self.state.preferences.kernel = value;
        Ok(())
    }
    fn set_danmaku_area(&mut self, value: DanmakuArea) -> Result<(), CommandError> {
        self.state.preferences.danmaku_area = value;
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
            "show_danmaku" => self.state.preferences.show_danmaku = value,
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
                    affected.push("网络媒体源");
                }
                if f.remote {
                    affected.push("局域网与远程");
                }
            }
            ("anime", false) if f.sync => affected.push("账号同步"),
            ("sync", true) if !f.anime => affected.push("追番"),
            ("sources" | "remote", true) if !f.library => affected.push("媒体库"),
            _ => {}
        }
        if !affected.is_empty() {
            self.notify(format!(
                "已同时{}：{}",
                if enabled { "开启" } else { "关闭" },
                affected.join("、")
            ));
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
            format!("{} · 插件设置", name)
        } else {
            format!("已添加 {}", name)
        });
        Ok(())
    }
    fn show_notice(&mut self, message: String) -> Result<(), CommandError> {
        self.notify(message);
        Ok(())
    }

    fn open_file(&mut self, source: MediaSource) -> Result<(), CommandError> {
        let title = source.file_name().unwrap_or_else(|| source.url.clone());
        self.start(title, "本地文件".into(), still(), 0.0);
        Ok(())
    }

    fn play(
        &mut self,
        title: String,
        subtitle: String,
        still: MediaSource,
        progress: f64,
    ) -> Result<(), CommandError> {
        self.start(title, subtitle, still, progress);
        Ok(())
    }

    fn play_download(&mut self, title: String, subtitle: String, still: MediaSource, progress: f64) -> Result<(), CommandError> {
        self.start(title, subtitle, still, progress);
        Ok(())
    }

    fn open_library_show(&mut self, id: i64) -> Result<(), CommandError> {
        erika_anime_protocol::anime::calls::open_from_library(id);
        Ok(())
    }

    fn minimize(&mut self) -> Result<(), CommandError> {
        self.require_playing()?;
        self.state.expanded = false;
        Ok(())
    }

    fn expand(&mut self) -> Result<(), CommandError> {
        self.require_playing()?;
        self.state.expanded = true;
        Ok(())
    }

    fn close_player(&mut self) -> Result<(), CommandError> {
        self.state.playing = false;
        self.state.expanded = false;
        Ok(())
    }

    fn toggle_pause(&mut self) -> Result<(), CommandError> {
        self.require_playing()?;
        self.state.paused = !self.state.paused;
        Ok(())
    }

    fn set_accent(&mut self, accent: Accent) -> Result<(), CommandError> {
        self.state.accent = accent;
        self.save_welcome_preferences()?;
        Ok(())
    }

    fn set_appearance(&mut self, appearance: Appearance) -> Result<(), CommandError> {
        self.state.appearance = appearance;
        self.save_welcome_preferences()?;
        Ok(())
    }
}

impl erika::Engine for ErikaEngine {
    fn initial_modules(&self) -> Option<Vec<String>> { self.saved_modules.clone() }
    fn modules_changed(&mut self, enabled: &[String]) {
        self.saved_modules = Some(enabled.to_vec());
        if let Err(error) = self.save_welcome_preferences() { self.notify(format!("无法保存设置：{}", error.message)); }
        self.state.features = settings::feature_selection(enabled);
        self.state.module_count = enabled.len() as i64;
        self.state.preset_name = if enabled.is_empty() {
            "纯粹播放"
        } else if enabled.len() == 8 {
            "全都要"
        } else if enabled.len() == 3
            && ["danmaku", "library", "anime"]
                .iter()
                .all(|id| enabled.iter().any(|s| s == id))
        {
            "看番"
        } else {
            "自己挑"
        }
        .into();
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
            .play("葬送的芙莉莲".into(), "第 4 集".into(), still(), 0.35)
            .unwrap();
        assert!(engine.state.playing && engine.state.expanded);
        assert_eq!(engine.state.elapsed, "8:29");
        engine.minimize().unwrap();
        assert!(engine.state.playing && !engine.state.expanded);
        engine.expand().unwrap();
        assert!(engine.state.expanded);
        engine.close_player().unwrap();
        assert!(!engine.state.playing);
    }

    #[test]
    fn files_play_under_their_names() {
        let mut engine = ErikaEngine::default();
        engine
            .open_file(MediaSource::new("file:///Videos/%E5%BD%B1.mkv"))
            .unwrap();
        assert_eq!(engine.state.title, "影.mkv");
        assert_eq!(engine.state.subtitle, "本地文件");
    }
}
