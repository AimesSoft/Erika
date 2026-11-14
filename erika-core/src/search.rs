//! The fixed action index of the desktop prototype.
use erika_protocol::erika::SearchActions;
use erika_protocol::erika::{FeatureSelection, SettingItem, SettingResult, SettingsPage};

/// Search individual controls, retaining their owning page and localized name.
/// Disabled modules hide their settings; their enable switches remain searchable
/// when that module is included in this build.
pub(crate) fn settings(text: &str, features: &FeatureSelection) -> Vec<SettingResult> {
    let query = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    use SettingItem as I;
    use SettingsPage as P;
    let entries = [
        (
            I::Account,
            P::Account,
            "弹弹play 账号登录 account sign in",
            "用户名 邮箱 密码 退出 登录 login dandanplay password sign out",
            features.account,
        ),
        (
            I::Startup,
            P::General,
            "启动时打开 open at startup",
            "启动 首页 媒体库 播放器 launch start",
            true,
        ),
        (
            I::PlaybackEnd,
            P::General,
            "播放结束后 when playback ends",
            "播放下一集 停止播放 next stop",
            true,
        ),
        (
            I::Language,
            P::General,
            "界面语言 interface language",
            "中文 英文 跟随系统 chinese english",
            true,
        ),
        (
            I::Accent,
            P::Appearance,
            "主题色 accent color",
            "绯红 群青 荧绿 缎带粉 theme color crimson ultramarine neon ribbon",
            true,
        ),
        (
            I::Appearance,
            P::Appearance,
            "明暗 appearance",
            "浅色 深色 跟随系统 黑色 白色 light dark system theme background 背景",
            true,
        ),
        (
            I::ReduceMotion,
            P::Appearance,
            "减少动态效果 reduce motion",
            "动画 animation",
            true,
        ),
        (
            I::Speed,
            P::Player,
            "默认倍速 default speed",
            "播放速度 playback rate",
            true,
        ),
        (
            I::SkipIntro,
            P::Player,
            "片头／片尾跳过提示 opening ending skip hint",
            "跳过片头 跳过片尾 skip intro outro op ed",
            true,
        ),
        (
            I::RememberPosition,
            P::Player,
            "记住播放位置 remember playback position",
            "播放进度 断点续播 history resume",
            true,
        ),
        (
            I::Autoplay,
            P::Player,
            "自动连播下一集 automatically play next episode",
            "autoplay",
            true,
        ),
        (
            I::HardwareDecode,
            P::Player,
            "硬件解码 hardware decoding",
            "硬解 decode gpu",
            true,
        ),
        (
            I::ShowDanmaku,
            P::Danmaku,
            "显示弹幕 show danmaku",
            "开关 隐藏 hide",
            features.danmaku,
        ),
        (
            I::DanmakuOpacity,
            P::Danmaku,
            "弹幕不透明度 danmaku opacity",
            "透明度 transparency",
            features.danmaku,
        ),
        (
            I::DanmakuFontSize,
            P::Danmaku,
            "弹幕字号 danmaku font size",
            "字体 大小",
            features.danmaku,
        ),
        (
            I::DanmakuArea,
            P::Danmaku,
            "弹幕显示区域 danmaku display area",
            "半屏 全屏 quarter half full",
            features.danmaku,
        ),
        (
            I::AutoMatch,
            P::Danmaku,
            "弹弹play 自动匹配 automatically match with dandanplay",
            "弹幕识别 auto match",
            features.danmaku,
        ),
        (
            I::MountDanmaku,
            P::Danmaku,
            "挂载本地弹幕 mount local danmaku",
            "加载 导入 文件 xml json import load",
            features.danmaku,
        ),
        (
            I::SyncStorage,
            P::Library,
            "同步与存储 sync and storage",
            "",
            features.library,
        ),
        (
            I::SeasonSource,
            P::Anime,
            "新番数据源 new shows source",
            "弹弹play bangumi bgm dandanplay 切换 来源",
            features.anime && cfg!(feature = "bern-module-anime"),
        ),
        (
            I::SyncStorage,
            P::Anime,
            "同步与存储 sync and storage",
            "",
            features.anime,
        ),
        (
            I::SyncStorage,
            P::Sync,
            "同步与存储 sync and storage",
            "",
            features.sync,
        ),
        (
            I::SyncStorage,
            P::Sources,
            "同步与存储 sync and storage",
            "",
            features.sources,
        ),
        (
            I::SyncStorage,
            P::Remote,
            "同步与存储 sync and storage",
            "",
            features.remote,
        ),
        (
            I::SyncStorage,
            P::Downloads,
            "同步与存储 sync and storage",
            "",
            features.downloads,
        ),
        (
            I::SyncStorage,
            P::Enhance,
            "同步与存储 sync and storage",
            "",
            features.enhance,
        ),
        (
            I::EnableAccount,
            P::Optional,
            "启用账号 enable account",
            "模块 登录 弹弹play login dandanplay",
            cfg!(feature = "bern-module-account"),
        ),
        (
            I::EnableDanmaku,
            P::Optional,
            "启用弹幕 enable danmaku",
            "模块",
            cfg!(feature = "bern-module-danmaku"),
        ),
        (
            I::EnableEnhance,
            P::Optional,
            "启用画质增强 enable upscaling",
            "模块",
            cfg!(feature = "bern-module-enhance") && !cfg!(target_arch = "wasm32"),
        ),
        (
            I::EnableLibrary,
            P::Optional,
            "启用媒体库 enable library",
            "模块",
            cfg!(feature = "bern-module-library"),
        ),
        (
            I::EnableAnime,
            P::Optional,
            "启用新番 enable new shows anime",
            "模块",
            cfg!(feature = "bern-module-anime"),
        ),
        (
            I::EnableSync,
            P::Optional,
            "启用账号同步 enable account sync",
            "模块",
            cfg!(feature = "bern-module-sync"),
        ),
        (
            I::EnableSources,
            P::Optional,
            "启用网络媒体源 enable network sources",
            "模块",
            cfg!(feature = "bern-module-sources"),
        ),
        (
            I::EnableRemote,
            P::Optional,
            "启用局域网与远程 enable lan and remote",
            "模块",
            cfg!(feature = "bern-module-remote"),
        ),
        (
            I::EnableDownloads,
            P::Optional,
            "启用下载 enable downloads",
            "模块",
            cfg!(feature = "bern-module-downloads"),
        ),
        (
            I::EnablePlugins,
            P::Optional,
            "启用插件 enable plugins",
            "扩展 extensions",
            true,
        ),
        (
            I::Plugins,
            P::Plugins,
            "插件市场与已安装插件 plugin marketplace installed plugins",
            "分类 扩展 extensions",
            features.plugins,
        ),
        (
            I::Keys,
            P::Keys,
            "快捷键 keyboard shortcuts",
            "键盘 快进 快退 暂停 全屏 音量 space escape volume",
            true,
        ),
        (
            I::SourceCode,
            P::About,
            "开源地址 source code",
            "github 仓库 repository 开源 版本 version",
            true,
        ),
    ];
    let mut results: Vec<_> = entries
        .into_iter()
        .filter_map(|(item, page, name, aliases, enabled)| {
            if !enabled {
                return None;
            }
            let category = match page {
                P::Account => "账号 account",
                P::General => "通用 general",
                P::Appearance => "外观 appearance",
                P::Player => "播放 playback player",
                P::Danmaku => "弹幕 danmaku",
                P::Library => "媒体库 library",
                P::Anime => "新番 new shows anime",
                P::Sync => "账号同步 account sync",
                P::Sources => "网络媒体源 network sources",
                P::Remote => "局域网与远程 lan remote",
                P::Downloads => "下载 downloads",
                P::Enhance => "画质增强 upscaling",
                P::Optional => "可选功能 optional features",
                P::Plugins => "插件 plugins",
                P::Keys => "快捷键 keyboard shortcuts",
                P::About => "关于 about",
            };
            let terms = format!("{name} {aliases} {category} 设置 settings");
            if !query.split_whitespace().all(|word| terms.contains(word)) {
                return None;
            }
            let rank = if name.contains(&query) {
                0
            } else if aliases.contains(&query) {
                1
            } else {
                2
            };
            Some((
                rank,
                SettingResult {
                    id: format!("{page:?}.{item:?}"),
                    page,
                    item,
                },
            ))
        })
        .collect();
    results.sort_by_key(|(rank, _)| *rank);
    results.into_iter().map(|(_, entry)| entry).collect()
}

pub(crate) fn actions(text: &str, anime_enabled: bool, danmaku_enabled: bool) -> SearchActions {
    let q = text.trim().to_lowercase();
    let matches = |name: &str| q.is_empty() || name.to_lowercase().contains(&q);
    let mut slots = if q.is_empty() { 3 } else { 4 };
    let mut setting = |name: &str| {
        let found = slots > 0 && matches(name);
        if found {
            slots -= 1;
        }
        found
    };
    let theme = setting("主题色 设置 › 外观 theme color appearance settings");
    let danmaku = danmaku_enabled && setting("弹幕不透明度 设置 › 弹幕 danmaku opacity settings");
    let skip_intro = setting("片头／片尾跳过提示 设置 › 播放 skip intro playback settings");
    let optional = setting("可选功能 设置 optional features settings");
    let plugins = setting("插件市场 设置 › 插件 plugin marketplace settings");
    let keys = setting("快捷键 设置 keyboard shortcuts settings");
    let open_file = matches("打开文件… open file");
    let schedule = anime_enabled && matches("新番表 airing schedule");
    let dark = matches("切换深色模式 toggle dark mode");
    let has_settings = theme || danmaku || skip_intro || optional || plugins || keys;
    let has_operations = open_file || schedule || dark;
    SearchActions {
        theme,
        danmaku,
        skip_intro,
        optional,
        plugins,
        keys,
        open_file,
        schedule,
        dark,
        has_settings,
        has_operations,
        empty: !has_settings && !has_operations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_search_returns_individual_items_and_accepts_aliases() {
        let features = crate::settings::feature_selection(&[]);
        let results = settings("  硬解  ", &features);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].item, SettingItem::HardwareDecode);
        assert_eq!(results[0].page, SettingsPage::Player);
        assert_eq!(
            settings("HARDWARE decoding", &features)[0].id,
            results[0].id
        );
        assert_eq!(
            settings("跳过片尾", &features)[0].item,
            SettingItem::SkipIntro
        );
        let appearance = settings("外观", &features);
        assert!(appearance.iter().any(|x| x.item == SettingItem::Accent));
        assert!(
            appearance
                .iter()
                .any(|x| x.item == SettingItem::ReduceMotion)
        );
        assert!(settings("不存在的设置", &features).is_empty());
        assert!(settings("   ", &features).is_empty());
    }

    #[test]
    fn settings_search_respects_modules_and_keeps_result_ids_unique() {
        let mut features = crate::settings::feature_selection(&[]);
        assert!(settings("不透明度", &features).is_empty());
        features.danmaku = true;
        assert_eq!(
            settings("弹幕 透明度", &features)[0].item,
            SettingItem::DanmakuOpacity
        );
        features.library = true;
        features.anime = true;
        let results = settings("设置", &features);
        let ids: std::collections::HashSet<_> = results.iter().map(|x| &x.id).collect();
        assert_eq!(ids.len(), results.len());
        assert_eq!(
            results.iter().any(|x| x.item == SettingItem::EnableDanmaku),
            cfg!(feature = "bern-module-danmaku")
        );
    }
    #[test]
    fn disabled_account_hides_its_settings_but_can_still_be_enabled() {
        let mut features = crate::settings::feature_selection(&[]);
        let results = settings("账号", &features);
        assert!(results.iter().all(|x| x.page != SettingsPage::Account));
        assert_eq!(
            results.iter().any(|x| x.item == SettingItem::EnableAccount),
            cfg!(feature = "bern-module-account")
        );
        assert!(settings("识别服务器", &features).is_empty());
        features.account = true;
        assert!(
            settings("账号", &features)
                .iter()
                .any(|x| x.item == SettingItem::Account)
        );
    }
    #[test]
    fn empty_query_uses_three_settings_and_all_operations() {
        let a = actions("", true, true);
        assert!(a.theme && a.danmaku && a.skip_intro && a.open_file && a.schedule && a.dark);
        assert!(!a.optional && !a.plugins && !a.keys);
        assert!(actions("不存在", true, true).empty);
        assert!(actions("  深色  ", true, true).dark);
        let a = actions("设置", true, true);
        assert!(a.theme && a.danmaku && a.skip_intro && a.optional);
        assert!(!a.plugins && !a.keys);
        let disabled = actions("", false, false);
        assert!(!disabled.schedule && !disabled.danmaku && disabled.optional);
    }
}
