//! View state for the first-phase settings prototype. Playback, updates,
//! accounts and the plugin catalogue do not perform external operations.
use super::*;

pub const ALL_MODULES: [&str; 8] = [
    "danmaku",
    "enhance",
    "library",
    "anime",
    "sync",
    "sources",
    "remote",
    "downloads",
];

/// The module `id` names, by its label.
pub fn feature_module(id: &str) -> FeatureModule {
    match id {
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
    const ANIME: [&str; 3] = ["danmaku", "library", "anime"];
    if enabled.is_empty() {
        OnboardingPreset::Pure
    } else if enabled.len() == ALL_MODULES.len() {
        OnboardingPreset::All
    } else if enabled.len() == ANIME.len()
        && ANIME.iter().all(|id| enabled.iter().any(|m| m == id))
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
        step_back: true,
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
        danmaku: enabled.iter().any(|id| id == "danmaku"),
        enhance: enabled.iter().any(|id| id == "enhance"),
        library: enabled.iter().any(|id| id == "library"),
        anime: enabled.iter().any(|id| id == "anime"),
        sync: enabled.iter().any(|id| id == "sync"),
        sources: enabled.iter().any(|id| id == "sources"),
        remote: enabled.iter().any(|id| id == "remote"),
        downloads: enabled.iter().any(|id| id == "downloads"),
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
            tags: plugin_tags(&[PluginPermission::Network, PluginPermission::ModifyDanmaku, PluginPermission::NeedsDanmaku]),
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
        let text = text.trim().to_lowercase();
        if text.is_empty() {
            answer.ok(Vec::new());
            return;
        }
        let f = &self.state.features;
        let entries = [
            (
                SettingsPage::Account,
                "账号",
                "account 弹弹play 登录 退出 用户名 密码 dandanplay sign in login",
                true,
            ),
            (
                SettingsPage::General,
                "通用",
                "general 启动 语言 更新 language startup update",
                true,
            ),
            (
                SettingsPage::Appearance,
                "外观",
                "appearance 主题色 明暗 减少动态效果 theme accent motion",
                true,
            ),
            (
                SettingsPage::Player,
                "播放",
                "playback 倍速 跳过片头 位置 自动连播 画框 硬件解码 speed skip decode",
                true,
            ),
            (
                SettingsPage::Danmaku,
                "弹幕",
                "danmaku 不透明度 字号 显示区域 弹弹play 本地弹幕 opacity font",
                f.danmaku,
            ),
            (SettingsPage::Library, "媒体库", "library ", f.library),
            (SettingsPage::Anime, "追番", "anime ", f.anime),
            (SettingsPage::Sync, "账号同步", "account sync ", f.sync),
            (
                SettingsPage::Sources,
                "网络媒体源",
                "network sources ",
                f.sources,
            ),
            (
                SettingsPage::Remote,
                "局域网与远程",
                "lan and remote ",
                f.remote,
            ),
            (SettingsPage::Downloads, "下载", "downloads ", f.downloads),
            (SettingsPage::Enhance, "画质增强", "upscaling ", f.enhance),
            (
                SettingsPage::Optional,
                "可选功能",
                "optional features ",
                true,
            ),
            (SettingsPage::Plugins, "插件", "plugins ", true),
            (
                SettingsPage::Keys,
                "快捷键",
                "keyboard shortcuts 键盘 快捷键 keyboard shortcuts",
                true,
            ),
            (SettingsPage::About, "关于", "about ", true),
        ];
        answer.ok(entries
            .into_iter()
            .filter(|(_, name, keywords, enabled)| {
                *enabled && (name.contains(&text) || keywords.contains(&text))
            })
            .map(|(page, _, _, _)| SettingResult {
                id: format!("{page:?}"),
                page,
            })
            .collect());
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
        assert!(!engine.state.features.library);
        engine.modules_changed(&["danmaku".into(), "library".into(), "anime".into()]);
        assert_eq!(engine.state.preset, OnboardingPreset::Anime);
        assert!(engine.state.features.anime && engine.state.features.library);
        assert!(!engine.state.features.sources);
        assert!(engine.set_module("missing".into(), true).is_err());
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
