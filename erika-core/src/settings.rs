//! View state for the first-phase settings prototype. Playback, updates,
//! accounts and the plugin catalogue do not perform external operations.
use super::*;

pub const ALL_MODULES: &[&str] = &[
    #[cfg(feature = "bern-module-account")]
    "account",
    #[cfg(feature = "bern-module-danmaku")]
    "danmaku",
    #[cfg(feature = "bern-module-enhance")]
    "enhance",
    #[cfg(feature = "bern-module-library")]
    "library",
    #[cfg(feature = "bern-module-anime")]
    "anime",
    #[cfg(feature = "bern-module-sync")]
    "sync",
    #[cfg(feature = "bern-module-sources")]
    "sources",
    #[cfg(feature = "bern-module-remote")]
    "remote",
    #[cfg(feature = "bern-module-downloads")]
    "downloads",
];

/// The Anime preset includes only modules present in this build.
pub const ANIME_MODULES: &[&str] = &[
    #[cfg(feature = "bern-module-account")]
    "account",
    #[cfg(feature = "bern-module-danmaku")]
    "danmaku",
    #[cfg(feature = "bern-module-library")]
    "library",
    #[cfg(feature = "bern-module-anime")]
    "anime",
];

/// The module `id` names, by its label.
pub fn feature_module(id: &str) -> FeatureModule {
    match id {
        "account" => FeatureModule::Account,
        "danmaku" => FeatureModule::Danmaku,
        "enhance" => FeatureModule::Enhance,
        "library" => FeatureModule::Library,
        "anime" => FeatureModule::Anime,
        "sync" => FeatureModule::Sync,
        "sources" => FeatureModule::Sources,
        "remote" => FeatureModule::Remote,
        _ => FeatureModule::Downloads,
    }
}

/// The preset the enabled modules are exactly, or `Custom`.
pub fn preset_of(enabled: &[String]) -> OnboardingPreset {
    if enabled.is_empty() {
        OnboardingPreset::Pure
    } else if enabled.len() == ALL_MODULES.len() {
        OnboardingPreset::All
    } else if enabled.len() == ANIME_MODULES.len()
        && ANIME_MODULES
            .iter()
            .all(|id| enabled.iter().any(|m| m == id))
    {
        OnboardingPreset::Anime
    } else {
        OnboardingPreset::Custom
    }
}

pub fn default_preferences() -> Preferences {
    Preferences {
        auto_updates: true,
        reduce_motion: false,
        remember_position: true,
        autoplay: true,
        hardware_decode: true,
        show_danmaku: true,
        auto_match: true,
        sync_storage: true,
        startup: StartupPage::Home,
        playback_end: PlaybackEnd::Next,
        language: InterfaceLanguage::System,
        update_channel: UpdateChannel::Stable,
        speed: PlaybackSpeed::Normal,
        danmaku_area: DanmakuArea::Half,
    }
}
pub fn feature_selection(enabled: &[String]) -> FeatureSelection {
    FeatureSelection {
        account: cfg!(feature = "bern-module-account") && enabled.iter().any(|id| id == "account"),
        danmaku: cfg!(feature = "bern-module-danmaku") && enabled.iter().any(|id| id == "danmaku"),
        enhance: cfg!(feature = "bern-module-enhance")
            && !cfg!(target_arch = "wasm32")
            && enabled.iter().any(|id| id == "enhance"),
        library: cfg!(feature = "bern-module-library") && enabled.iter().any(|id| id == "library"),
        anime: cfg!(feature = "bern-module-anime") && enabled.iter().any(|id| id == "anime"),
        sync: cfg!(feature = "bern-module-sync") && enabled.iter().any(|id| id == "sync"),
        sources: cfg!(feature = "bern-module-sources") && enabled.iter().any(|id| id == "sources"),
        remote: cfg!(feature = "bern-module-remote") && enabled.iter().any(|id| id == "remote"),
        downloads: cfg!(feature = "bern-module-downloads")
            && enabled.iter().any(|id| id == "downloads"),
    }
}

/// `anime` in saved feature choices means New Shows. The runtime anime
/// module also supplies library details, even when New Shows is off.
pub(crate) fn runtime_modules(selected: &[String]) -> Vec<String> {
    let mut enabled = selected.to_vec();
    if cfg!(feature = "bern-module-anime")
        && selected.iter().any(|id| id == "library")
        && !enabled.iter().any(|id| id == "anime")
    {
        enabled.push("anime".into());
    }
    enabled
}

pub(crate) fn change_feature(selected: &mut Vec<String>, id: &str, enabled: bool) {
    let set = |selected: &mut Vec<String>, id: &str, enabled: bool| {
        selected.retain(|other| other != id);
        if enabled && ALL_MODULES.contains(&id) {
            selected.push(id.into());
        }
    };
    set(selected, id, enabled);
    match (id, enabled) {
        ("sources" | "remote", true) => set(selected, "library", true),
        ("library", false) => {
            set(selected, "sources", false);
            set(selected, "remote", false);
        }
        ("account", false) => set(selected, "sync", false),
        ("sync", true) => {
            set(selected, "account", true);
            if !selected.iter().any(|id| id == "library" || id == "anime") {
                set(selected, "anime", true);
            }
        }
        _ => {}
    }
    if !selected.iter().any(|id| id == "library" || id == "anime") {
        set(selected, "sync", false);
    }
    // Keep presets and persistence in a stable feature order.
    selected.sort_by_key(|id| ALL_MODULES.iter().position(|other| other == id));
}

impl ErikaEngine {
    pub(crate) fn apply_feature_selection(
        &mut self,
        selected: Vec<String>,
    ) -> Result<(), CommandError> {
        let previous = runtime_modules(
            self.saved_modules.as_deref().unwrap_or(
                &ALL_MODULES
                    .iter()
                    .map(|id| (*id).into())
                    .collect::<Vec<_>>(),
            ),
        );
        let wanted = runtime_modules(&selected);
        if self.onboarding_completed {
            self.write_welcome_preferences(&selected)?;
        }
        self.publish_feature_selection(&selected);
        self.pending_modules = (previous.len() != wanted.len()
            || previous.iter().any(|id| !wanted.contains(id)))
        .then(|| wanted.clone());
        // Stop dependents first. Start anime before the library so library
        // events can immediately reach the shared detail implementation.
        for id in ALL_MODULES.iter().rev() {
            if previous.iter().any(|old| old == id) && !wanted.iter().any(|new| new == id) {
                bern::modules::set_enabled(id, false);
            }
        }
        if wanted.iter().any(|id| id == "anime") && !previous.iter().any(|id| id == "anime") {
            bern::modules::set_enabled("anime", true);
        }
        for id in ALL_MODULES.iter().filter(|id| **id != "anime") {
            if wanted.iter().any(|new| new == id) && !previous.iter().any(|old| old == id) {
                bern::modules::set_enabled(id, true);
            }
        }
        Ok(())
    }

    pub(crate) fn publish_feature_selection(&mut self, selected: &[String]) {
        self.saved_modules = Some(selected.to_vec());
        #[cfg(feature = "bern-module-account")]
        let account_was_enabled = self.state.features.account;
        self.state.features = feature_selection(selected);
        #[cfg(feature = "bern-module-account")]
        if account_was_enabled != self.state.features.account {
            self.set_account_enabled(self.state.features.account);
        }
        self.state.module_count = selected.len() as i64;
        self.state.preset = preset_of(selected);
        self.apply_enhancement();
        self.apply_danmaku();
    }
}
fn plugin_tags(permissions: &[PluginPermission]) -> Vec<PluginTag> {
    permissions
        .iter()
        .map(|permission| PluginTag {
            id: format!("{permission:?}"),
            permission: permission.clone(),
        })
        .collect()
}

pub fn sample_plugins() -> Vec<Plugin> {
    vec![
        Plugin {
            id: "spoiler".into(),
            name: "防剧透".into(),
            author: "Erika 官方".into(),
            description: "用你配置的 AI 接口识别并遮挡剧透弹幕".into(),
            tags: plugin_tags(&[
                PluginPermission::Network,
                PluginPermission::ModifyDanmaku,
                PluginPermission::NeedsDanmaku,
            ]),
            category: PluginCategory::Danmaku,
            installed: false,
        },
        Plugin {
            id: "regex".into(),
            name: "正则弹幕过滤".into(),
            author: "Erika 官方".into(),
            description: "用正则表达式屏蔽弹幕".into(),
            tags: plugin_tags(&[PluginPermission::ModifyDanmaku]),
            category: PluginCategory::Danmaku,
            installed: true,
        },
        Plugin {
            id: "words".into(),
            name: "敏感词屏蔽".into(),
            author: "Erika 官方".into(),
            description: "内置中文敏感词词库，一键屏蔽".into(),
            tags: plugin_tags(&[PluginPermission::ModifyDanmaku]),
            category: PluginCategory::Danmaku,
            installed: true,
        },
        Plugin {
            id: "crt".into(),
            name: "复古 CRT".into(),
            author: "社区 · shaderlab".into(),
            description: "扫描线与荧光屏质感的着色器".into(),
            tags: plugin_tags(&[PluginPermission::Shaders]),
            category: PluginCategory::Picture,
            installed: false,
        },
        Plugin {
            id: "clip".into(),
            name: "片段导出".into(),
            author: "Erika 官方".into(),
            description: "把精彩片段导出为 GIF 或 MP4".into(),
            tags: plugin_tags(&[PluginPermission::MediaExport, PluginPermission::WriteFiles]),
            category: PluginCategory::Tools,
            installed: false,
        },
        Plugin {
            id: "ext".into(),
            name: "外部播放器".into(),
            author: "社区 · ext-player".into(),
            description: "用 mpv、PotPlayer 或 IINA 打开当前视频".into(),
            tags: plugin_tags(&[PluginPermission::LaunchPrograms]),
            category: PluginCategory::Tools,
            installed: false,
        },
        Plugin {
            id: "sfx".into(),
            name: "界面音效包".into(),
            author: "Erika 官方".into(),
            description: "给按钮和转场配上轻快的音效".into(),
            tags: plugin_tags(&[]),
            category: PluginCategory::Appearance,
            installed: true,
        },
        Plugin {
            id: "theme".into(),
            name: "主题与壁纸".into(),
            author: "社区 · mochi".into(),
            description: "自定义背景图与配色方案".into(),
            tags: plugin_tags(&[PluginPermission::Appearance]),
            category: PluginCategory::Appearance,
            installed: false,
        },
        Plugin {
            id: "titan".into(),
            name: "Titan 弹幕".into(),
            author: "社区 · titan".into(),
            description: "用 WebView 渲染的高级弹幕特效".into(),
            tags: plugin_tags(&[PluginPermission::WebViewOverlay]),
            category: PluginCategory::Danmaku,
            installed: false,
        },
    ]
}
impl ErikaEngine {
    pub(crate) fn notify(&mut self, notice: Notice) {
        self.state.notice = Some(notice);
        self.state.notice_revision += 1;
    }
    pub(crate) fn filter_plugins(&mut self) {
        self.state.installed_count = self.all_plugins.iter().filter(|p| p.installed).count() as i64;
        self.state.plugins = self
            .all_plugins
            .iter()
            .filter(|p| {
                (self.state.plugin_category == PluginCategory::All
                    || p.category == self.state.plugin_category)
                    && (self.state.plugin_tab == PluginTab::Market || p.installed)
            })
            .cloned()
            .collect();
    }
}
impl erika::Queries for ErikaEngine {
    fn search_actions(&self, text: String, answer: bern::prelude::Answer<erika::SearchActions>) {
        answer.ok(crate::search::actions(
            &text,
            self.state.features.anime,
            self.state.features.danmaku,
        ));
    }

    fn find_settings(&self, text: String, answer: Answer<Vec<SettingResult>>) {
        answer.ok(crate::search::settings(&text, &self.state.features));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use erika::{Commands, Engine};

    #[test]
    fn module_callbacks_keep_the_preview_in_sync() {
        let mut engine = ErikaEngine::default();
        engine.modules_changed(&[]);
        assert_eq!(engine.state.module_count, 0);
        assert_eq!(engine.state.preset, OnboardingPreset::Pure);
        assert!(!engine.state.features.account);
        assert!(!engine.state.features.library);
        engine.modules_changed(
            &ANIME_MODULES
                .iter()
                .map(|id| (*id).into())
                .collect::<Vec<_>>(),
        );
        assert_eq!(engine.state.preset, OnboardingPreset::Anime);
        assert_eq!(
            engine.state.features.account,
            cfg!(feature = "bern-module-account")
        );
        assert!(engine.state.features.anime && engine.state.features.library);
        assert!(!engine.state.features.sources);
        assert!(engine.set_module("missing".into(), true).is_err());
    }

    #[test]
    fn new_shows_and_library_keep_details_until_both_are_off() {
        for library in [false, true] {
            for new_shows in [false, true] {
                let mut engine = ErikaEngine::default();
                engine.modules_changed(&[]);
                engine.set_module("library".into(), library).unwrap();
                if engine.pending_modules.is_some() {
                    engine.modules_changed(&engine.initial_modules().unwrap());
                }
                engine.set_module("anime".into(), new_shows).unwrap();
                let runtime = engine.initial_modules().unwrap();
                assert_eq!(runtime.iter().any(|id| id == "anime"), library || new_shows);
                if engine.pending_modules.is_some() {
                    engine.modules_changed(&runtime);
                }
                assert_eq!(engine.state.features.library, library);
                assert_eq!(engine.state.features.anime, new_shows);
                assert_eq!(
                    engine.state.module_count,
                    i64::from(library) + i64::from(new_shows)
                );
            }
        }
    }

    #[test]
    fn toggling_new_shows_with_library_running_does_not_restart_details() {
        let mut engine = ErikaEngine::default();
        engine.modules_changed(&["library".into(), "anime".into()]);
        engine.set_module("anime".into(), false).unwrap();
        assert!(!engine.state.features.anime);
        assert!(engine.pending_modules.is_none());
        assert!(
            engine
                .initial_modules()
                .unwrap()
                .iter()
                .any(|id| id == "anime")
        );
        engine.set_module("anime".into(), true).unwrap();
        assert!(engine.state.features.anime);
        assert!(engine.pending_modules.is_none());
        engine.set_module("anime".into(), false).unwrap();
        engine.set_module("library".into(), false).unwrap();
        assert!(engine.initial_modules().unwrap().is_empty());
    }

    #[test]
    fn intermediate_runtime_callbacks_do_not_overwrite_feature_choices() {
        let mut engine = ErikaEngine::default();
        engine.modules_changed(&[]);
        engine.set_module("library".into(), true).unwrap();
        engine.modules_changed(&["anime".into()]);
        assert!(engine.state.features.library);
        assert!(!engine.state.features.anime);
        engine.modules_changed(&["anime".into(), "library".into()]);
        assert!(engine.pending_modules.is_none());
        assert_eq!(engine.saved_modules.as_deref().unwrap(), ["library"]);
    }

    #[test]
    fn library_alone_satisfies_sync_and_disabling_the_last_content_feature_stops_it() {
        let mut selected = vec!["library".into()];
        change_feature(&mut selected, "sync", true);
        assert!(selected.iter().any(|id| id == "account"));
        assert!(selected.iter().any(|id| id == "sync"));
        assert!(!selected.iter().any(|id| id == "anime"));
        change_feature(&mut selected, "anime", true);
        change_feature(&mut selected, "anime", false);
        assert!(selected.iter().any(|id| id == "sync"));
        change_feature(&mut selected, "library", false);
        assert!(!selected.iter().any(|id| id == "sync"));
    }

    #[test]
    fn installing_a_preview_plugin_updates_filters_without_duplicate_counts() {
        let mut engine = ErikaEngine::default();
        engine.set_plugin_tab(PluginTab::Installed).unwrap();
        assert_eq!(engine.state.plugins.len(), 3);
        engine.use_plugin("spoiler".into()).unwrap();
        assert_eq!(engine.state.installed_count, 4);
        assert_eq!(engine.state.plugins.len(), 4);
        engine.use_plugin("spoiler".into()).unwrap();
        assert_eq!(engine.state.installed_count, 4);
        engine.set_plugin_category(PluginCategory::Tools).unwrap();
        assert!(engine.state.plugins.is_empty());
        engine.set_plugin_tab(PluginTab::Market).unwrap();
        assert_eq!(engine.state.plugins.len(), 2);
    }
}
