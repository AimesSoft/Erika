//! Settings from design/next-screens.js and next.css (Apple desktop).
//! A native grouped form on every page (design 10.9: "苹果是分组列表"),
//! a 220-point sidebar, and the title bar named after the page. All
//! preference changes go through the owning engine's protocol.
use crate::AppWindows;
#[cfg(feature = "bern-module-account")]
use crate::account::{account_dialogs, account_dialogs_props, account_profile, account_profile_props};
use crate::feature_preview::{
    feature_preview_hud, feature_preview_hud_props, feature_preview_window,
    feature_preview_window_props,
};
use bern::prelude::*;
#[cfg(feature = "bern-module-anime")]
use erika_anime_protocol::anime;
use erika_protocol::erika::{self, *};
#[bern::session]
pub struct SettingsDraft {
    pub page: SettingsPage,
    pub query: String,
    pub danmaku_file: MediaSource,
}
#[component]
pub fn settings(
    app: erika::Client,
    #[cfg(feature = "bern-module-anime")] anime: anime::Client,
    windows: AppWindows,
    #[session] draft: SettingsDraft,
) -> impl View {
    ui! {
        Stack {
        if window.tv {
            tv_settings(app: app, #[cfg(feature = "bern-module-anime")] anime: anime, windows: windows)
        } else {
        Stack(variant: app_window, align: bottom, reduce_motion: app.preferences.reduce_motion) {
            SplitView {
                Sidebar(id: settings_sections, width: 220, selection: draft.page) {
                    Search(id: settings_search, value: draft.query, prompt: tr! { "zh-Hans" => "搜索设置", "en" => "Search settings" }, shortcut: app.keys.search) {
                        Query(id: settings_results, query: app.find_settings(draft.query), empty: tr! { "zh-Hans" => "没有匹配的设置", "en" => "No matching settings" }) |results| {
                            ForEach(items: results, key: id) |result| {
                                Button(id: settings_result, label: result.item, variant: row, clears: draft.query) {
                                    Column(gap: 2) {
                                        Text(content: result.item)
                                        Text(content: result.page, style: caption, emphasis: secondary)
                                    }
                                } => draft.page.set(result.page)
                            }
                        }
                    }
                    if build.modules.account && app.features.account {
                        SidebarItem(value: SettingsPage::Account, label: tr! { "zh-Hans" => "账号", "en" => "Account" }, icon: "person.crop.circle")
                    }
                    SidebarItem(value: SettingsPage::General, label: tr! { "zh-Hans" => "通用", "en" => "General" }, icon: "gearshape")
                    SidebarItem(value: SettingsPage::Appearance, label: tr! { "zh-Hans" => "外观", "en" => "Appearance" }, icon: "paintpalette")
                    SidebarItem(value: SettingsPage::Player, label: tr! { "zh-Hans" => "播放", "en" => "Playback" }, icon: "play")
                    if (build.modules.danmaku && app.features.danmaku) {
                        SidebarItem(value: SettingsPage::Danmaku, label: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, icon: "text.bubble")
                    }
                    if (build.modules.library && app.features.library) {
                        SidebarItem(value: SettingsPage::Library, label: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, icon: "film.stack")
                    }
                    if (build.modules.anime && app.features.anime) {
                        SidebarItem(value: SettingsPage::Anime, label: tr! { "zh-Hans" => "新番", "en" => "New Shows" }, icon: "calendar")
                    }
                    if (build.modules.sync && app.features.sync) {
                        SidebarItem(value: SettingsPage::Sync, label: tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" }, icon: "arrow.triangle.2.circlepath")
                    }
                    if (build.modules.sources && app.features.sources) {
                        SidebarItem(value: SettingsPage::Sources, label: tr! { "zh-Hans" => "网络媒体源", "en" => "Network Sources" }, icon: "server.rack")
                    }
                    if (build.modules.remote && app.features.remote) {
                        SidebarItem(value: SettingsPage::Remote, label: tr! { "zh-Hans" => "局域网与远程", "en" => "LAN and Remote" }, icon: "wifi")
                    }
                    if (build.modules.downloads && app.features.downloads) {
                        SidebarItem(value: SettingsPage::Downloads, label: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, icon: "arrow.down.circle")
                    }
                    if (build.modules.enhance && app.features.enhance) {
                        SidebarItem(value: SettingsPage::Enhance, label: tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" }, icon: "sparkles")
                    }
                    Section(title: tr! { "zh-Hans" => "扩展", "en" => "Extensions" }) {
                        SidebarItem(value: SettingsPage::Optional, label: tr! { "zh-Hans" => "可选功能", "en" => "Optional Features" }, icon: "square.grid.2x2")
                        SidebarItem(value: SettingsPage::Plugins, label: tr! { "zh-Hans" => "插件", "en" => "Plugins" }, icon: "puzzlepiece.extension")
                        SidebarItem(value: SettingsPage::Keys, label: tr! { "zh-Hans" => "快捷键", "en" => "Keyboard Shortcuts" }, icon: "keyboard")
                        SidebarItem(value: SettingsPage::About, label: tr! { "zh-Hans" => "关于", "en" => "About" }, icon: "info.circle")
                    }
                }
                Detail {
                    settings_page(app: app, #[cfg(feature = "bern-module-anime")] anime: anime)
                }
            }
            if let Some(notice) = app.notice {
                Toast(content: notice, key: app.notice_revision, duration: 3)
            }
        }
        }
        if build.modules.account && app.features.account {
            account_dialogs(app: app, settings_window: true)
        }
        }
    }
}

/// The page the settings show.
#[component]
pub fn settings_page(
    app: erika::Client,
    #[cfg(feature = "bern-module-anime")] anime: anime::Client,
    #[session] draft: SettingsDraft,
) -> impl View {
    ui! {
        Column(gap: 0) {
            match draft.page {
                SettingsPage::Account => {
                    if build.modules.account && app.features.account {
                        settings_account(app: app, settings_window: true)
                    } else {
                        settings_general(app: app)
                    }
                }
                SettingsPage::General => { settings_general(app: app) }
                SettingsPage::Appearance => { settings_appearance(app: app) }
                SettingsPage::Player => { settings_player(app: app) }
                #[cfg(feature = "bern-module-danmaku")]
                SettingsPage::Danmaku => { settings_danmaku(app: app) }
                #[cfg(feature = "bern-module-library")]
                SettingsPage::Library => { settings_library(app: app, #[cfg(feature = "bern-module-anime")] anime: anime) }
                #[cfg(feature = "bern-module-anime")]
                SettingsPage::Anime => { settings_module_placeholder(app: app) }
                #[cfg(feature = "bern-module-sync")]
                SettingsPage::Sync => { settings_module_placeholder(app: app) }
                #[cfg(feature = "bern-module-sources")]
                SettingsPage::Sources => { settings_module_placeholder(app: app) }
                #[cfg(feature = "bern-module-remote")]
                SettingsPage::Remote => { settings_module_placeholder(app: app) }
                #[cfg(feature = "bern-module-downloads")]
                SettingsPage::Downloads => { settings_module_placeholder(app: app) }
                #[cfg(feature = "bern-module-enhance")]
                SettingsPage::Enhance => { settings_module_placeholder(app: app) }
                SettingsPage::Optional => { settings_optional(app: app) }
                SettingsPage::Plugins => { settings_plugins(app: app) }
                SettingsPage::Keys => { settings_keys(app: app) }
                SettingsPage::About => { settings_about(app: app) }
                #[cfg(any(not(feature = "bern-module-danmaku"), not(feature = "bern-module-enhance"), not(feature = "bern-module-anime"), not(feature = "bern-module-library"), not(feature = "bern-module-sync"), not(feature = "bern-module-sources"), not(feature = "bern-module-remote"), not(feature = "bern-module-downloads")))]
                _ => {}
            }
        }
    }
}

/// The settings on a television (`openSettings` on the television): the
/// whole screen, the prototype's 1280 × 720 scene. The pages stand in a
/// 300-point panel 8 in from the edges (`.s-side`) with the search above
/// them, 46-point rows without symbols; the page's title (22 / 600) and
/// its form to the right; the close button in the top right corner. The
/// remote's Back closes them too.
#[component]
pub fn tv_settings(
    app: erika::Client,
    #[cfg(feature = "bern-module-anime")] anime: anime::Client,
    windows: AppWindows,
    #[session] draft: SettingsDraft,
) -> impl View {
    ui! {
        Stack(under_bars: true, reduce_motion: app.preferences.reduce_motion) {
            ScaleBox(width: 1280, height: 720) {
                Stack(variant: onboarding_page, width: 1280, height: 720, align: top_leading) {
                    Positioned(x: 8, y: 8, width: 300, height: 704) {
                        Stack(variant: settings_tv_side, width: 300, height: 704, align: top_leading) {
                            // The television searches its settings from the search
                            // tab, whose results list them; the panel holds the
                            // pages alone.
                            Column(gap: 0, padding_x: 10, padding_y: 14) {
                                ScrollView {
                                    Column(gap: 2) {
                                    // The remote starts on the first page (`.c-navi.on`).
                                    Button(id: tv_settings_general, label: tr! { "zh-Hans" => "通用", "en" => "General" }, variant: settings_tv_nav, default_focus: true) {
                                        Stack(width: 280, height: 46, align: leading) {
                                            if draft.page == SettingsPage::General || (draft.page == SettingsPage::Account && !app.features.account) {
                                                Canvas {
                                                    Shape(kind: capsule, fill: fill.strong)
                                                }
                                            }
                                            Row(gap: 0, padding_x: 16) {
                                                Text(content: tr! { "zh-Hans" => "通用", "en" => "General" }, lines: 1)
                                                Spacer
                                            }
                                        }
                                    } => draft.page.set(SettingsPage::General)
                                    if build.modules.account && app.features.account {
                                    Button(id: tv_settings_account, label: tr! { "zh-Hans" => "账号", "en" => "Account" }, variant: settings_tv_nav) {
                                        Stack(width: 280, height: 46, align: leading) {
                                            if draft.page == SettingsPage::Account {
                                                Canvas {
                                                    Shape(kind: capsule, fill: fill.strong)
                                                }
                                            }
                                            Row(gap: 0, padding_x: 16) {
                                                Text(content: tr! { "zh-Hans" => "账号", "en" => "Account" }, lines: 1)
                                                Spacer
                                            }
                                        }
                                    } => draft.page.set(SettingsPage::Account)
                                    }
                                    Button(id: tv_settings_appearance, label: tr! { "zh-Hans" => "外观", "en" => "Appearance" }, variant: settings_tv_nav) {
                                        Stack(width: 280, height: 46, align: leading) {
                                            if draft.page == SettingsPage::Appearance {
                                                Canvas {
                                                    Shape(kind: capsule, fill: fill.strong)
                                                }
                                            }
                                            Row(gap: 0, padding_x: 16) {
                                                Text(content: tr! { "zh-Hans" => "外观", "en" => "Appearance" }, lines: 1)
                                                Spacer
                                            }
                                        }
                                    } => draft.page.set(SettingsPage::Appearance)
                                    Button(id: tv_settings_player, label: tr! { "zh-Hans" => "播放", "en" => "Playback" }, variant: settings_tv_nav) {
                                        Stack(width: 280, height: 46, align: leading) {
                                            if draft.page == SettingsPage::Player {
                                                Canvas {
                                                    Shape(kind: capsule, fill: fill.strong)
                                                }
                                            }
                                            Row(gap: 0, padding_x: 16) {
                                                Text(content: tr! { "zh-Hans" => "播放", "en" => "Playback" }, lines: 1)
                                                Spacer
                                            }
                                        }
                                    } => draft.page.set(SettingsPage::Player)
                                    if (build.modules.danmaku && app.features.danmaku) {
                                        Button(id: tv_settings_danmaku, label: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, variant: settings_tv_nav) {
                                            Stack(width: 280, height: 46, align: leading) {
                                                if draft.page == SettingsPage::Danmaku {
                                                    Canvas {
                                                        Shape(kind: capsule, fill: fill.strong)
                                                    }
                                                }
                                                Row(gap: 0, padding_x: 16) {
                                                    Text(content: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, lines: 1)
                                                    Spacer
                                                }
                                            }
                                        } => draft.page.set(SettingsPage::Danmaku)
                                    }
                                    if (build.modules.library && app.features.library) {
                                        Button(id: tv_settings_library, label: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, variant: settings_tv_nav) {
                                            Stack(width: 280, height: 46, align: leading) {
                                                if draft.page == SettingsPage::Library {
                                                    Canvas {
                                                        Shape(kind: capsule, fill: fill.strong)
                                                    }
                                                }
                                                Row(gap: 0, padding_x: 16) {
                                                    Text(content: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, lines: 1)
                                                    Spacer
                                                }
                                            }
                                        } => draft.page.set(SettingsPage::Library)
                                    }
                                    if (build.modules.anime && app.features.anime) {
                                        Button(id: tv_settings_anime, label: tr! { "zh-Hans" => "新番", "en" => "New Shows" }, variant: settings_tv_nav) {
                                            Stack(width: 280, height: 46, align: leading) {
                                                if draft.page == SettingsPage::Anime {
                                                    Canvas {
                                                        Shape(kind: capsule, fill: fill.strong)
                                                    }
                                                }
                                                Row(gap: 0, padding_x: 16) {
                                                    Text(content: tr! { "zh-Hans" => "新番", "en" => "New Shows" }, lines: 1)
                                                    Spacer
                                                }
                                            }
                                        } => draft.page.set(SettingsPage::Anime)
                                    }
                                    if (build.modules.sync && app.features.sync) {
                                        Button(id: tv_settings_sync, label: tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" }, variant: settings_tv_nav) {
                                            Stack(width: 280, height: 46, align: leading) {
                                                if draft.page == SettingsPage::Sync {
                                                    Canvas {
                                                        Shape(kind: capsule, fill: fill.strong)
                                                    }
                                                }
                                                Row(gap: 0, padding_x: 16) {
                                                    Text(content: tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" }, lines: 1)
                                                    Spacer
                                                }
                                            }
                                        } => draft.page.set(SettingsPage::Sync)
                                    }
                                    if (build.modules.sources && app.features.sources) {
                                        Button(id: tv_settings_sources, label: tr! { "zh-Hans" => "网络媒体源", "en" => "Network Sources" }, variant: settings_tv_nav) {
                                            Stack(width: 280, height: 46, align: leading) {
                                                if draft.page == SettingsPage::Sources {
                                                    Canvas {
                                                        Shape(kind: capsule, fill: fill.strong)
                                                    }
                                                }
                                                Row(gap: 0, padding_x: 16) {
                                                    Text(content: tr! { "zh-Hans" => "网络媒体源", "en" => "Network Sources" }, lines: 1)
                                                    Spacer
                                                }
                                            }
                                        } => draft.page.set(SettingsPage::Sources)
                                    }
                                    if (build.modules.remote && app.features.remote) {
                                        Button(id: tv_settings_remote, label: tr! { "zh-Hans" => "局域网与远程", "en" => "LAN and Remote" }, variant: settings_tv_nav) {
                                            Stack(width: 280, height: 46, align: leading) {
                                                if draft.page == SettingsPage::Remote {
                                                    Canvas {
                                                        Shape(kind: capsule, fill: fill.strong)
                                                    }
                                                }
                                                Row(gap: 0, padding_x: 16) {
                                                    Text(content: tr! { "zh-Hans" => "局域网与远程", "en" => "LAN and Remote" }, lines: 1)
                                                    Spacer
                                                }
                                            }
                                        } => draft.page.set(SettingsPage::Remote)
                                    }
                                    if (build.modules.downloads && app.features.downloads) {
                                        Button(id: tv_settings_downloads, label: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, variant: settings_tv_nav) {
                                            Stack(width: 280, height: 46, align: leading) {
                                                if draft.page == SettingsPage::Downloads {
                                                    Canvas {
                                                        Shape(kind: capsule, fill: fill.strong)
                                                    }
                                                }
                                                Row(gap: 0, padding_x: 16) {
                                                    Text(content: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, lines: 1)
                                                    Spacer
                                                }
                                            }
                                        } => draft.page.set(SettingsPage::Downloads)
                                    }
                                    if (build.modules.enhance && app.features.enhance) {
                                        Button(id: tv_settings_enhance, label: tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" }, variant: settings_tv_nav) {
                                            Stack(width: 280, height: 46, align: leading) {
                                                if draft.page == SettingsPage::Enhance {
                                                    Canvas {
                                                        Shape(kind: capsule, fill: fill.strong)
                                                    }
                                                }
                                                Row(gap: 0, padding_x: 16) {
                                                    Text(content: tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" }, lines: 1)
                                                    Spacer
                                                }
                                            }
                                        } => draft.page.set(SettingsPage::Enhance)
                                    }
                                    Row(gap: 0, padding_x: 14, padding_y: 8) {
                                        Text(content: tr! { "zh-Hans" => "扩展", "en" => "Extensions" }, style: caption, emphasis: secondary)
                                        Spacer
                                    }
                                    Button(id: tv_settings_optional, label: tr! { "zh-Hans" => "可选功能", "en" => "Optional Features" }, variant: settings_tv_nav) {
                                        Stack(width: 280, height: 46, align: leading) {
                                            if draft.page == SettingsPage::Optional {
                                                Canvas {
                                                    Shape(kind: capsule, fill: fill.strong)
                                                }
                                            }
                                            Row(gap: 0, padding_x: 16) {
                                                Text(content: tr! { "zh-Hans" => "可选功能", "en" => "Optional Features" }, lines: 1)
                                                Spacer
                                            }
                                        }
                                    } => draft.page.set(SettingsPage::Optional)
                                    Button(id: tv_settings_plugins, label: tr! { "zh-Hans" => "插件", "en" => "Plugins" }, variant: settings_tv_nav) {
                                        Stack(width: 280, height: 46, align: leading) {
                                            if draft.page == SettingsPage::Plugins {
                                                Canvas {
                                                    Shape(kind: capsule, fill: fill.strong)
                                                }
                                            }
                                            Row(gap: 0, padding_x: 16) {
                                                Text(content: tr! { "zh-Hans" => "插件", "en" => "Plugins" }, lines: 1)
                                                Spacer
                                            }
                                        }
                                    } => draft.page.set(SettingsPage::Plugins)
                                    Button(id: tv_settings_keys, label: tr! { "zh-Hans" => "快捷键", "en" => "Keyboard Shortcuts" }, variant: settings_tv_nav) {
                                        Stack(width: 280, height: 46, align: leading) {
                                            if draft.page == SettingsPage::Keys {
                                                Canvas {
                                                    Shape(kind: capsule, fill: fill.strong)
                                                }
                                            }
                                            Row(gap: 0, padding_x: 16) {
                                                Text(content: tr! { "zh-Hans" => "快捷键", "en" => "Keyboard Shortcuts" }, lines: 1)
                                                Spacer
                                            }
                                        }
                                    } => draft.page.set(SettingsPage::Keys)
                                    Button(id: tv_settings_about, label: tr! { "zh-Hans" => "关于", "en" => "About" }, variant: settings_tv_nav) {
                                        Stack(width: 280, height: 46, align: leading) {
                                            if draft.page == SettingsPage::About {
                                                Canvas {
                                                    Shape(kind: capsule, fill: fill.strong)
                                                }
                                            }
                                            Row(gap: 0, padding_x: 16) {
                                                Text(content: tr! { "zh-Hans" => "关于", "en" => "About" }, lines: 1)
                                                Spacer
                                            }
                                        }
                                    } => draft.page.set(SettingsPage::About)
                                    }
                                }
                            }
                        }
                    }
                    Positioned(x: 336, y: 14, width: 860, height: 28) {
                        Row(gap: 0) {
                            if draft.page == SettingsPage::Account && !app.features.account {
                                Text(content: tr! { "zh-Hans" => "通用", "en" => "General" }, style: title3, lines: 1)
                            } else {
                                Text(content: draft.page, style: title3, lines: 1)
                            }
                            Spacer
                        }
                    }
                    Positioned(x: 1220, y: 5, width: 46, height: 46) {
                        IconButton(id: tv_settings_close, icon: "xmark", label: tr! { "zh-Hans" => "关闭", "en" => "Close" }, height: 46) => windows.settings.close()
                    }
                    Positioned(x: 322, y: 56, width: 944, height: 664) {
                        settings_page(app: app, #[cfg(feature = "bern-module-anime")] anime: anime)
                    }
                    if let Some(notice) = app.notice {
                        Positioned(x: 322, y: 600, width: 944, height: 80) {
                            Toast(content: notice, key: app.notice_revision, duration: 3)
                        }
                    }
                }
            }
        }
    }
}

/// 「账号」: both providers use the same profile and open the shared login
/// dialog. Only their data and authentication commands differ.
#[cfg(feature = "bern-module-account")]
#[component]
pub fn settings_account(app: erika::Client, settings_window: bool, #[session] draft: SettingsDraft) -> impl View {
    ui! {
        Form(variant: settings_form, appear: settings_page_in) {
            Slot(id: form_header)
            Section(title: tr! { "zh-Hans" => "弹弹play 账号", "en" => "dandanplay account" }) {
                account_profile(app: app, bangumi: false, settings_window: settings_window, signed_in: app.account_status.signed_in, name: app.account_status.name, user: app.account_status.user, picture: app.account_status.avatar, busy: app.account_status.busy, message: app.account_status.error)
            }
            Section(title: tr! { "zh-Hans" => "Bangumi 账号", "en" => "Bangumi account" }) {
                account_profile(app: app, bangumi: true, settings_window: settings_window, signed_in: app.bangumi.signed_in, name: app.bangumi.name, user: app.bangumi.email, picture: app.bangumi.avatar, busy: app.bangumi.busy, message: app.bangumi.error)
            }
        }
    }
}

#[component]
pub fn settings_general(app: erika::Client) -> impl View {
    ui! {
        Form(variant: settings_form, appear: settings_page_in) {
            Slot(id: form_header)
            Section(title: tr! { "zh-Hans" => "启动", "en" => "Startup" }) {
                Picker(id: startup, label: tr! { "zh-Hans" => "启动时打开", "en" => "Open at startup" }, selected: app.preferences.startup, style: menu) => app.set_startup(app.preferences.startup)
                Picker(id: playback_end, label: tr! { "zh-Hans" => "播放结束后", "en" => "When playback ends" }, selected: app.preferences.playback_end, style: menu) => app.set_playback_end(app.preferences.playback_end)
            }
            Section(title: tr! { "zh-Hans" => "语言", "en" => "Language" }) {
                Picker(id: language, label: tr! { "zh-Hans" => "界面语言", "en" => "Interface language" }, selected: app.preferences.language, style: menu) => app.set_language(app.preferences.language)
            }
        }
    }
}
#[component]
pub fn settings_appearance(app: erika::Client) -> impl View {
    ui! {
        Form(variant: settings_form, appear: settings_page_in) {
            Slot(id: form_header)
            Section(title: tr! { "zh-Hans" => "主题色", "en" => "Accent color" }) {
                Row(gap: 12, align: center) {
                    Column(gap: 1) {
                        Text(content: tr! { "zh-Hans" => "主题色", "en" => "Accent color" }, style: body)
                        Text(content: tr! { "zh-Hans" => "取自 Erika 图标，传给系统作为强调色", "en" => "From the Erika icon, used as the system accent" }, style: caption, emphasis: secondary)
                    }
                    Spacer
                    Row(gap: 8, align: center) {
                        Button(id: accent_crimson, label: tr! { "zh-Hans" => "绯红", "en" => "Crimson" }, variant: theme_swatch) {
                            Column(gap: 6, width: 38, align: center) {
                                Stack(variant: crimson_swatch, width: 30, height: 30) {
                                    if app.accent == Accent::Crimson {
                                        Canvas(width: 30, height: 30) {
                                            Shape(kind: ellipse, x: 1, y: 1, width: 28, height: 28, stroke: text.primary, stroke_width: 2)
                                        }
                                    }
                                }
                                Text(content: tr! { "zh-Hans" => "绯红", "en" => "Crimson" }, style: caption, emphasis: secondary)
                            }
                        }
                        => app.set_accent(Accent::Crimson)
                        Button(id: accent_ultramarine, label: tr! { "zh-Hans" => "群青", "en" => "Ultramarine" }, variant: theme_swatch) {
                            Column(gap: 6, width: 38, align: center) {
                                Stack(variant: ultramarine_swatch, width: 30, height: 30) {
                                    if app.accent == Accent::Ultramarine {
                                        Canvas(width: 30, height: 30) {
                                            Shape(kind: ellipse, x: 1, y: 1, width: 28, height: 28, stroke: text.primary, stroke_width: 2)
                                        }
                                    }
                                }
                                Text(content: tr! { "zh-Hans" => "群青", "en" => "Ultramarine" }, style: caption, emphasis: secondary)
                            }
                        }
                        => app.set_accent(Accent::Ultramarine)
                        Button(id: accent_neon, label: tr! { "zh-Hans" => "荧绿", "en" => "Neon" }, variant: theme_swatch) {
                            Column(gap: 6, width: 38, align: center) {
                                Stack(variant: neon_swatch, width: 30, height: 30) {
                                    if app.accent == Accent::Neon {
                                        Canvas(width: 30, height: 30) {
                                            Shape(kind: ellipse, x: 1, y: 1, width: 28, height: 28, stroke: text.primary, stroke_width: 2)
                                        }
                                    }
                                }
                                Text(content: tr! { "zh-Hans" => "荧绿", "en" => "Neon" }, style: caption, emphasis: secondary)
                            }
                        }
                        => app.set_accent(Accent::Neon)
                        Button(id: accent_ribbon, label: tr! { "zh-Hans" => "缎带粉", "en" => "Ribbon Pink" }, variant: theme_swatch) {
                            Column(gap: 6, width: 38, align: center) {
                                Stack(variant: ribbon_swatch, width: 30, height: 30) {
                                    if app.accent == Accent::Ribbon {
                                        Canvas(width: 30, height: 30) {
                                            Shape(kind: ellipse, x: 1, y: 1, width: 28, height: 28, stroke: text.primary, stroke_width: 2)
                                        }
                                    }
                                }
                                Text(content: tr! { "zh-Hans" => "缎带粉", "en" => "Ribbon Pink" }, style: caption, emphasis: secondary)
                            }
                        }
                        => app.set_accent(Accent::Ribbon)
                    }
                }
            }
            Section(title: tr! { "zh-Hans" => "外观", "en" => "Appearance" }) {
                Picker(id: appearance, label: tr! { "zh-Hans" => "明暗", "en" => "Appearance" }, selected: app.appearance, style: segmented) => app.set_appearance(app.appearance)
                Row(gap: 12, align: center) {
                    Column(gap: 1) {
                        Text(content: tr! { "zh-Hans" => "减少动态效果", "en" => "Reduce motion" }, style: body)
                        Text(content: tr! { "zh-Hans" => "弹簧动画换成 150ms 淡入淡出", "en" => "Replace springs with a 150ms crossfade" }, style: caption, emphasis: secondary)
                    }
                    Spacer
                    Switch(id: reduce_motion, label: tr! { "zh-Hans" => "减少动态效果", "en" => "Reduce motion" }, checked: app.preferences.reduce_motion, style: switch, label_hidden: true) => app.set_preference("reduce_motion", app.preferences.reduce_motion)
                }
            }
        }
    }
}
#[component]
pub fn settings_player(app: erika::Client) -> impl View {
    ui! {
        Form(variant: settings_form, appear: settings_page_in) {
            Slot(id: form_header)
            Section(title: tr! { "zh-Hans" => "播放", "en" => "Playback" }) {
                Picker(id: speed, label: tr! { "zh-Hans" => "默认倍速", "en" => "Default speed" }, selected: app.preferences.speed, style: menu) => app.set_speed(app.preferences.speed)
                Switch(id: skip_segments, label: tr! { "zh-Hans" => "片头／片尾跳过提示", "en" => "Opening / ending skip hint" }, checked: app.player.skip_intro, style: switch) => app.set_skip_intro(app.player.skip_intro)
                Switch(id: remember_position, label: tr! { "zh-Hans" => "记住播放位置", "en" => "Remember playback position" }, checked: app.preferences.remember_position, style: switch) => app.set_preference("remember_position", app.preferences.remember_position)
                Switch(id: autoplay, label: tr! { "zh-Hans" => "自动连播下一集", "en" => "Automatically play next episode" }, checked: app.preferences.autoplay, style: switch) => app.set_preference("autoplay", app.preferences.autoplay)
            }
            Section(title: tr! { "zh-Hans" => "解码", "en" => "Decoding" }) {
                Switch(id: hardware_decode, label: tr! { "zh-Hans" => "硬件解码", "en" => "Hardware decoding" }, checked: app.preferences.hardware_decode, style: switch) => app.set_preference("hardware_decode", app.preferences.hardware_decode)
            }
        }
    }
}
#[cfg(feature = "bern-module-danmaku")]
#[component]
pub fn settings_danmaku(app: erika::Client, #[session] draft: SettingsDraft) -> impl View {
    ui! {
        Form(variant: settings_form, appear: settings_page_in) {
            Slot(id: form_header)
            Section(title: tr! { "zh-Hans" => "显示", "en" => "Display" }) {
                Switch(id: show_danmaku, label: tr! { "zh-Hans" => "显示弹幕", "en" => "Show danmaku" }, checked: app.preferences.show_danmaku, style: switch) => app.set_preference("show_danmaku", app.preferences.show_danmaku)
                Column(gap: 4) {
                    Row(align: center) {
                        Text(content: tr! { "zh-Hans" => "不透明度", "en" => "Opacity" }) Spacer
                        Text(content: tr! { "zh-Hans" => "{value}%", "en" => "{value}%"; value: app.danmaku_settings.opacity_percent })
                    }
                    Slider(id: opacity, label: tr! { "zh-Hans" => "不透明度", "en" => "Opacity" }, label_hidden: true, current: app.danmaku_settings.opacity, continuous: true, min: 0, max: 1) => app.set_danmaku_number("opacity", app.danmaku_settings.opacity)
                }
                Column(gap: 4) {
                    Row(align: center) {
                        Text(content: tr! { "zh-Hans" => "字号", "en" => "Font size" }) Spacer
                        Text(content: tr! { "zh-Hans" => "{value} pt", "en" => "{value} pt"; value: app.danmaku_settings.font_points })
                    }
                    Slider(id: font_size, label: tr! { "zh-Hans" => "字号", "en" => "Font size" }, label_hidden: true, current: app.danmaku_settings.font_size, continuous: true, min: 0, max: 1) => app.set_danmaku_number("font_size", app.danmaku_settings.font_size)
                }
                Picker(id: area, label: tr! { "zh-Hans" => "显示区域", "en" => "Display area" }, selected: app.preferences.danmaku_area, style: segmented) => app.set_danmaku_area(app.preferences.danmaku_area)
            }
            Section(title: tr! { "zh-Hans" => "来源", "en" => "Sources" }) {
                Switch(id: auto_match, label: tr! { "zh-Hans" => "弹弹play 自动匹配", "en" => "Automatically match with Dandanplay" }, checked: app.preferences.auto_match, style: switch) => app.set_preference("auto_match", app.preferences.auto_match)
                // A television has no files to mount.
                if !window.tv {
                    Row(gap: 12, align: center) {
                        Column(gap: 1) {
                            Text(content: tr! { "zh-Hans" => "本地弹幕文件", "en" => "Local danmaku file" }, style: body)
                            Text(content: tr! { "zh-Hans" => "xml / json", "en" => "xml / json" }, style: caption, emphasis: secondary)
                        }
                        Spacer
                        Button(id: mount_danmaku, label: tr! { "zh-Hans" => "挂载…", "en" => "Mount…" }, pick: draft.danmaku_file, files: ".xml .json") => app.mount_danmaku(draft.danmaku_file)
                    }
                }
            }
        }
    }
}
#[component]
pub fn settings_library(
    app: erika::Client,
    #[cfg(feature = "bern-module-anime")] anime: anime::Client,
) -> impl View {
    ui! {
        Form(variant: settings_form, appear: settings_page_in) {
            Slot(id: form_header)
            if build.modules.anime {
                Section(title: tr! { "zh-Hans" => "新番", "en" => "New Shows" }) {
                    if anime.can_choose_season_source {
                        Picker(id: season_source, label: tr! { "zh-Hans" => "新番数据源", "en" => "New Shows source" }, selected: anime.season_source, style: menu) => anime.set_season_source(anime.season_source)
                        Text(content: tr! { "zh-Hans" => "切换后立即刷新新番，并记住你的选择。所选来源不可用时会自动回退。", "en" => "Refreshes New Shows immediately and remembers your choice. Falls back automatically if the selected source is unavailable." }, style: caption, emphasis: secondary)
                    } else {
                        Row(gap: 12, align: center) {
                            Text(content: tr! { "zh-Hans" => "新番数据源", "en" => "New Shows source" })
                            Spacer
                            if anime.signed_in {
                                Text(content: anime.season_source, emphasis: secondary)
                            } else {
                                Text(content: tr! { "zh-Hans" => "公共数据", "en" => "Public catalogue" }, emphasis: secondary)
                            }
                        }
                        Text(content: tr! { "zh-Hans" => "同时登录弹弹play和 Bangumi 后可切换；只登录一个账号时自动使用该账号的数据源。", "en" => "Sign in to both dandanplay and Bangumi to choose. With one account, its source is used automatically." }, style: caption, emphasis: secondary)
                    }
                }
            }
            Section {
                Switch(id: sync_storage, label: tr! { "zh-Hans" => "同步与存储", "en" => "Sync and storage" }, checked: app.preferences.sync_storage, style: switch) => app.set_preference("sync_storage", app.preferences.sync_storage)
            }
        }
    }
}

#[component]
pub fn settings_module_placeholder(app: erika::Client) -> impl View {
    ui! {
        Form(variant: settings_form, appear: settings_page_in) {
            Slot(id: form_header)
            Section {
                Row(gap: 12, align: center) {
                    Column(gap: 1) {
                        Text(content: tr! { "zh-Hans" => "这一节的设置", "en" => "Settings for this section" }, style: body)
                        Text(content: tr! { "zh-Hans" => "模块开启后才会出现在这里", "en" => "Shown when its module is enabled" }, style: caption, emphasis: secondary)
                    }
                    Spacer
                    Text(content: tr! { "zh-Hans" => "示意", "en" => "Preview" }, style: body, emphasis: secondary)
                }
                Switch(id: sync_storage, label: tr! { "zh-Hans" => "同步与存储", "en" => "Sync and storage" }, checked: app.preferences.sync_storage, style: switch) => app.set_preference("sync_storage", app.preferences.sync_storage)
            }
        }
    }
}
/// Each module's symbol sits on a neutral grey tile: the design keeps the
/// accent off list icons (10.9). The desktop shows the preview and the
/// presets beside the switches; the phone over them (`phHTML('settings')`),
/// 18 under the title, without the note that points to the right.
#[component]
pub fn settings_optional(app: erika::Client) -> impl View {
    ui! {
        Row(gap: 0, align: top) {
            Form(variant: settings_form, appear: settings_page_in) {
                Slot(id: form_header)
                if window.compact {
                    Section(style: plain) {
                        Column(gap: 0) {
                            Canvas(height: 8)
                            settings_preview(app: app, width: 358.0)
                            // `.o-pv` 6 above `.o-presets`.
                            Canvas(height: 6)
                            settings_presets(app: app)
                        }
                    }
                    Section(title: tr! { "zh-Hans" => "观看", "en" => "Watching" }) {
                        if build.modules.danmaku { settings_danmaku_module(app: app) }
                        if build.modules.enhance { settings_enhance_module(app: app) }
                    }
                } else {
                    Section(title: tr! { "zh-Hans" => "观看", "en" => "Watching" }, footer: tr! { "zh-Hans" => "决定 Erika 要做多少事。关掉的功能不显示，也不占资源。右边的小窗口会跟着变。", "en" => "Choose what Erika does. Disabled features disappear and use no resources. The preview updates as you go." }) {
                        if build.modules.danmaku { settings_danmaku_module(app: app) }
                        if build.modules.enhance { settings_enhance_module(app: app) }
                    }
                }
                Section(title: tr! { "zh-Hans" => "内容", "en" => "Content" }) {
                    if build.modules.account {
                    Row(gap: 12, align: center) {
                        Stack(width: 28, height: 28) {
                            Canvas {
                                Shape(kind: rounded, corner: 7, fill: fill.strong)
                            }
                            Label(title: tr! { "zh-Hans" => "账号", "en" => "Account" }, icon: "person.crop.circle", style: body, display: icon)
                        }
                        Column(gap: 1) {
                            Text(content: tr! { "zh-Hans" => "账号", "en" => "Account" }, style: body)
                            Text(content: tr! { "zh-Hans" => "弹弹play 登录与识别服务器", "en" => "Dandanplay sign-in and recognition server" }, style: caption, emphasis: secondary)
                        }
                        Spacer
                        Switch(id: feature_account, label: tr! { "zh-Hans" => "账号", "en" => "Account" }, checked: app.features.account, style: switch, label_hidden: true) => app.set_module("account", app.features.account)
                    }
                    }
                    if build.modules.library {
                    Row(gap: 12, align: center) {
                        Stack(width: 28, height: 28) {
                            Canvas {
                                Shape(kind: rounded, corner: 7, fill: fill.strong)
                            }
                            Label(title: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, icon: "film.stack", style: body, display: icon)
                        }
                        Column(gap: 1) {
                            Text(content: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, style: body)
                            Text(content: tr! { "zh-Hans" => "扫描文件夹，识别番剧与封面", "en" => "Scan folders, recognize shows and posters" }, style: caption, emphasis: secondary)
                        }
                        Spacer
                        Switch(id: feature_library, label: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, checked: app.features.library, style: switch, label_hidden: true) => app.set_module("library", app.features.library)
                    }
                    }
                    if build.modules.anime {
                    Row(gap: 12, align: center) {
                        Stack(width: 28, height: 28) {
                            Canvas {
                                Shape(kind: rounded, corner: 7, fill: fill.strong)
                            }
                            Label(title: tr! { "zh-Hans" => "新番", "en" => "New Shows" }, icon: "calendar", style: body, display: icon)
                        }
                        Column(gap: 1) {
                            Text(content: tr! { "zh-Hans" => "新番", "en" => "New Shows" }, style: body)
                            Text(content: tr! { "zh-Hans" => "首页推荐、新番表", "en" => "Home and airing calendar" }, style: caption, emphasis: secondary)
                        }
                        Spacer
                        Switch(id: feature_anime, label: tr! { "zh-Hans" => "新番", "en" => "New Shows" }, checked: app.features.anime, style: switch, label_hidden: true) => app.set_module("anime", app.features.anime)
                    }
                    }
                    if build.modules.sync {
                    Row(gap: 12, align: center) {
                        Stack(width: 28, height: 28) {
                            Canvas {
                                Shape(kind: rounded, corner: 7, fill: fill.strong)
                            }
                            Label(title: tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" }, icon: "arrow.triangle.2.circlepath", style: body, display: icon)
                        }
                        Column(gap: 1) {
                            Text(content: tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" }, style: body)
                            Text(content: tr! { "zh-Hans" => "弹弹play / Bangumi 同步 · 需要账号及媒体库或新番", "en" => "Dandanplay / Bangumi · requires Account and Library or New Shows" }, style: caption, emphasis: secondary)
                        }
                        Spacer
                        Switch(id: feature_sync, label: tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" }, checked: app.features.sync, style: switch, label_hidden: true) => app.set_module("sync", app.features.sync)
                    }
                    }
                }
                Section(title: tr! { "zh-Hans" => "连接", "en" => "Connections" }) {
                    if build.modules.sources {
                    Row(gap: 12, align: center) {
                        Stack(width: 28, height: 28) {
                            Canvas {
                                Shape(kind: rounded, corner: 7, fill: fill.strong)
                            }
                            Label(title: tr! { "zh-Hans" => "网络媒体源", "en" => "Network Sources" }, icon: "server.rack", style: body, display: icon)
                        }
                        Column(gap: 1) {
                            Text(content: tr! { "zh-Hans" => "网络媒体源", "en" => "Network Sources" }, style: body)
                            Text(content: tr! { "zh-Hans" => "Jellyfin、Emby、WebDAV、SMB · 需要媒体库", "en" => "Jellyfin, Emby, WebDAV, SMB · requires Library" }, style: caption, emphasis: secondary)
                        }
                        Spacer
                        Switch(id: feature_sources, label: tr! { "zh-Hans" => "网络媒体源", "en" => "Network Sources" }, checked: app.features.sources, style: switch, label_hidden: true) => app.set_module("sources", app.features.sources)
                    }
                    }
                    if build.modules.remote {
                    Row(gap: 12, align: center) {
                        Stack(width: 28, height: 28) {
                            Canvas {
                                Shape(kind: rounded, corner: 7, fill: fill.strong)
                            }
                            Label(title: tr! { "zh-Hans" => "局域网与远程", "en" => "LAN and Remote" }, icon: "wifi", style: body, display: icon)
                        }
                        Column(gap: 1) {
                            Text(content: tr! { "zh-Hans" => "局域网与远程", "en" => "LAN and Remote" }, style: body)
                            Text(content: tr! { "zh-Hans" => "局域网共享、远程访问 · 需要媒体库", "en" => "Sharing and remote access · requires Library" }, style: caption, emphasis: secondary)
                        }
                        Spacer
                        Switch(id: feature_remote, label: tr! { "zh-Hans" => "局域网与远程", "en" => "LAN and Remote" }, checked: app.features.remote, style: switch, label_hidden: true) => app.set_module("remote", app.features.remote)
                    }
                    }
                    if build.modules.downloads {
                    Row(gap: 12, align: center) {
                        Stack(width: 28, height: 28) {
                            Canvas {
                                Shape(kind: rounded, corner: 7, fill: fill.strong)
                            }
                            Label(title: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, icon: "arrow.down.circle", style: body, display: icon)
                        }
                        Column(gap: 1) {
                            Text(content: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, style: body)
                            Text(content: tr! { "zh-Hans" => "种子与磁力链接", "en" => "Torrents and magnet links" }, style: caption, emphasis: secondary)
                        }
                        Spacer
                        Switch(id: feature_downloads, label: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, checked: app.features.downloads, style: switch, label_hidden: true) => app.set_module("downloads", app.features.downloads)
                    }
                    }
                }
            }
            // A scroll view of its own, so the title bar's scroll edge spans
            // this column as it does the form beside it.
            if !window.compact {
                ScrollView(appear: settings_page_in) {
                    Row(gap: 0, align: top) {
                    Column(gap: 0, width: 300, padding_y: 20) {
                        Text(content: tr! { "zh-Hans" => "预览", "en" => "Preview" }, style: caption2, emphasis: secondary)
                        Canvas(height: 8)
                        settings_preview(app: app, width: 300.0)
                        Canvas(height: 10)
                        settings_presets(app: app)
                    }
                    Canvas(width: 20)
                    }
                }
            }
        }
    }
}
/// The danmaku module's row, in both layouts of the optional features.
#[cfg(feature = "bern-module-danmaku")]
#[component]
pub fn settings_danmaku_module(app: erika::Client) -> impl View {
    ui! {
        Row(gap: 12, align: center) {
            Stack(width: 28, height: 28) {
                Canvas {
                    Shape(kind: rounded, corner: 7, fill: fill.strong)
                }
                Label(title: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, icon: "text.bubble", style: body, display: icon)
            }
            Column(gap: 1) {
                Text(content: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, style: body)
                Text(content: tr! { "zh-Hans" => "弹弹play 匹配与渲染、发送弹幕", "en" => "Match, render and send danmaku" }, style: caption, emphasis: secondary)
            }
            Spacer
            Switch(id: feature_danmaku, label: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, checked: app.features.danmaku, style: switch, label_hidden: true) => app.set_module("danmaku", app.features.danmaku)
        }
    }
}
/// The upscaling module's row, in both layouts of the optional features.
#[cfg(feature = "bern-module-enhance")]
#[component]
pub fn settings_enhance_module(app: erika::Client) -> impl View {
    ui! {
        Row(gap: 12, align: center) {
            Stack(width: 28, height: 28) {
                Canvas {
                    Shape(kind: rounded, corner: 7, fill: fill.strong)
                }
                Label(title: tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" }, icon: "sparkles", style: body, display: icon)
            }
            Column(gap: 1) {
                Text(content: tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" }, style: body)
                Text(content: tr! { "zh-Hans" => "ArtCNN 超分辨率", "en" => "ArtCNN upscaling" }, style: caption, emphasis: secondary)
            }
            Spacer
            Switch(id: feature_enhance, label: tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" }, checked: app.features.enhance, style: switch, label_hidden: true) => app.set_module("enhance", app.features.enhance)
        }
    }
}
/// `.o-presets`: the three presets, 6 apart.
#[component]
pub fn settings_presets(app: erika::Client) -> impl View {
    ui! {
        Row(gap: 6) {
            Button(id: preset_pure, label: tr! { "zh-Hans" => "纯粹播放", "en" => "Pure playback" }) => app.apply_preset(FeaturePreset::Pure)
            Button(id: preset_anime, label: tr! { "zh-Hans" => "看番", "en" => "Anime" }) => app.apply_preset(FeaturePreset::Anime)
            Button(id: preset_all, label: tr! { "zh-Hans" => "全都要", "en" => "Everything" }) => app.apply_preset(FeaturePreset::All)
        }
    }
}
/// `.o-pv`: the window, 10 above its playback bar, and 14 under that how
/// many modules are on (`.o-sum`, 13 / 18), naming the preset they are
/// exactly.
#[component]
pub fn settings_preview(app: erika::Client, width: f64) -> impl View {
    ui! {
        Column(gap: 0, width: width) {
            feature_preview_window(features: app.features, revision: app.feature_preview_revision, width: width)
            Canvas(height: 10)
            feature_preview_hud(features: app.features)
            Canvas(height: 14)
            if app.preset == OnboardingPreset::Custom {
                Text(content: tr! { "zh-Hans" => "已开启 {count} 个模块", "en" => "{count, plural, one {# module} other {# modules}} on"; count: app.module_count }, font_size: 13, line_height: 18, emphasis: secondary, lines: 2)
            } else {
                Text(content: tr! { "zh-Hans" => "已开启 {count} 个模块 · 正好是「{preset}」", "en" => "{count, plural, one {# module} other {# modules}} on · just like “{preset}”"; count: app.module_count, preset: app.preset }, font_size: 13, line_height: 18, emphasis: secondary, lines: 2)
            }
        }
    }
}
/// 「设置 › 快捷键」: every shortcut, which a press of its button and then
/// of the new keys changes. Keys another action has are refused with a
/// notice; a changed shortcut offers its default back. The menu bar's own
/// actions show only where the menu bar does.
#[component]
pub fn settings_keys(app: erika::Client) -> impl View {
    ui! {
        Form(variant: settings_form, appear: settings_page_in) {
            Slot(id: form_header)
            Section(title: tr! { "zh-Hans" => "播放", "en" => "Playback" }) {
                Row(gap: 8, align: center) {
                    Text(content: tr! { "zh-Hans" => "播放 / 暂停", "en" => "Play / Pause" }, style: body)
                    Spacer
                    if app.keys.play_pause != app.default_keys.play_pause {
                        IconButton(id: key_play_pause_default, icon: "arrow.triangle.2.circlepath", label: tr! { "zh-Hans" => "恢复默认", "en" => "Restore default" }, prominence: plain) => app.reset_shortcut(KeyAction::PlayPause)
                    }
                    ShortcutInput(id: key_play_pause, label: tr! { "zh-Hans" => "播放 / 暂停", "en" => "Play / Pause" }, label_hidden: true, current: app.keys.play_pause) => app.set_shortcut(KeyAction::PlayPause, app.keys.play_pause)
                }
                Row(gap: 8, align: center) {
                    Text(content: tr! { "zh-Hans" => "快退 10 秒", "en" => "Back 10 seconds" }, style: body)
                    Spacer
                    if app.keys.seek_back != app.default_keys.seek_back {
                        IconButton(id: key_seek_back_default, icon: "arrow.triangle.2.circlepath", label: tr! { "zh-Hans" => "恢复默认", "en" => "Restore default" }, prominence: plain) => app.reset_shortcut(KeyAction::SeekBack)
                    }
                    ShortcutInput(id: key_seek_back, label: tr! { "zh-Hans" => "快退 10 秒", "en" => "Back 10 seconds" }, label_hidden: true, current: app.keys.seek_back) => app.set_shortcut(KeyAction::SeekBack, app.keys.seek_back)
                }
                Row(gap: 8, align: center) {
                    Text(content: tr! { "zh-Hans" => "快进 10 秒", "en" => "Forward 10 seconds" }, style: body)
                    Spacer
                    if app.keys.seek_forward != app.default_keys.seek_forward {
                        IconButton(id: key_seek_forward_default, icon: "arrow.triangle.2.circlepath", label: tr! { "zh-Hans" => "恢复默认", "en" => "Restore default" }, prominence: plain) => app.reset_shortcut(KeyAction::SeekForward)
                    }
                    ShortcutInput(id: key_seek_forward, label: tr! { "zh-Hans" => "快进 10 秒", "en" => "Forward 10 seconds" }, label_hidden: true, current: app.keys.seek_forward) => app.set_shortcut(KeyAction::SeekForward, app.keys.seek_forward)
                }
                Row(gap: 8, align: center) {
                    Text(content: tr! { "zh-Hans" => "全屏", "en" => "Full screen" }, style: body)
                    Spacer
                    if app.keys.fullscreen != app.default_keys.fullscreen {
                        IconButton(id: key_fullscreen_default, icon: "arrow.triangle.2.circlepath", label: tr! { "zh-Hans" => "恢复默认", "en" => "Restore default" }, prominence: plain) => app.reset_shortcut(KeyAction::Fullscreen)
                    }
                    ShortcutInput(id: key_fullscreen, label: tr! { "zh-Hans" => "全屏", "en" => "Full screen" }, label_hidden: true, current: app.keys.fullscreen) => app.set_shortcut(KeyAction::Fullscreen, app.keys.fullscreen)
                }
                if build.modules.danmaku && app.features.danmaku {
                    Row(gap: 8, align: center) {
                        Text(content: tr! { "zh-Hans" => "开关弹幕", "en" => "Show or hide danmaku" }, style: body)
                        Spacer
                        if app.keys.toggle_danmaku != app.default_keys.toggle_danmaku {
                            IconButton(id: key_toggle_danmaku_default, icon: "arrow.triangle.2.circlepath", label: tr! { "zh-Hans" => "恢复默认", "en" => "Restore default" }, prominence: plain) => app.reset_shortcut(KeyAction::ToggleDanmaku)
                        }
                        ShortcutInput(id: key_toggle_danmaku, label: tr! { "zh-Hans" => "开关弹幕", "en" => "Show or hide danmaku" }, label_hidden: true, current: app.keys.toggle_danmaku) => app.set_shortcut(KeyAction::ToggleDanmaku, app.keys.toggle_danmaku)
                    }
                }
                Row(gap: 8, align: center) {
                    Text(content: tr! { "zh-Hans" => "截图", "en" => "Screenshot" }, style: body)
                    Spacer
                    if app.keys.screenshot != app.default_keys.screenshot {
                        IconButton(id: key_screenshot_default, icon: "arrow.triangle.2.circlepath", label: tr! { "zh-Hans" => "恢复默认", "en" => "Restore default" }, prominence: plain) => app.reset_shortcut(KeyAction::Screenshot)
                    }
                    ShortcutInput(id: key_screenshot, label: tr! { "zh-Hans" => "截图", "en" => "Screenshot" }, label_hidden: true, current: app.keys.screenshot) => app.set_shortcut(KeyAction::Screenshot, app.keys.screenshot)
                }
            }
            Section(title: tr! { "zh-Hans" => "通用", "en" => "General" }) {
                Row(gap: 8, align: center) {
                    Text(content: tr! { "zh-Hans" => "打开文件", "en" => "Open file" }, style: body)
                    Spacer
                    if app.keys.open_file != app.default_keys.open_file {
                        IconButton(id: key_open_file_default, icon: "arrow.triangle.2.circlepath", label: tr! { "zh-Hans" => "恢复默认", "en" => "Restore default" }, prominence: plain) => app.reset_shortcut(KeyAction::OpenFile)
                    }
                    ShortcutInput(id: key_open_file, label: tr! { "zh-Hans" => "打开文件", "en" => "Open file" }, label_hidden: true, current: app.keys.open_file) => app.set_shortcut(KeyAction::OpenFile, app.keys.open_file)
                }
                Row(gap: 8, align: center) {
                    Text(content: tr! { "zh-Hans" => "搜索", "en" => "Search" }, style: body)
                    Spacer
                    if app.keys.search != app.default_keys.search {
                        IconButton(id: key_search_default, icon: "arrow.triangle.2.circlepath", label: tr! { "zh-Hans" => "恢复默认", "en" => "Restore default" }, prominence: plain) => app.reset_shortcut(KeyAction::Search)
                    }
                    ShortcutInput(id: key_search, label: tr! { "zh-Hans" => "搜索", "en" => "Search" }, label_hidden: true, current: app.keys.search) => app.set_shortcut(KeyAction::Search, app.keys.search)
                }
                Row(gap: 8, align: center) {
                    Text(content: tr! { "zh-Hans" => "设置", "en" => "Settings" }, style: body)
                    Spacer
                    if app.keys.settings != app.default_keys.settings {
                        IconButton(id: key_settings_default, icon: "arrow.triangle.2.circlepath", label: tr! { "zh-Hans" => "恢复默认", "en" => "Restore default" }, prominence: plain) => app.reset_shortcut(KeyAction::Settings)
                    }
                    ShortcutInput(id: key_settings, label: tr! { "zh-Hans" => "设置", "en" => "Settings" }, label_hidden: true, current: app.keys.settings) => app.set_shortcut(KeyAction::Settings, app.keys.settings)
                }
                Row(gap: 8, align: center) {
                    Text(content: tr! { "zh-Hans" => "返回 / 关闭", "en" => "Back / close" }, style: body)
                    Spacer
                    if app.keys.back != app.default_keys.back {
                        IconButton(id: key_back_default, icon: "arrow.triangle.2.circlepath", label: tr! { "zh-Hans" => "恢复默认", "en" => "Restore default" }, prominence: plain) => app.reset_shortcut(KeyAction::Back)
                    }
                    ShortcutInput(id: key_back, label: tr! { "zh-Hans" => "返回 / 关闭", "en" => "Back / close" }, label_hidden: true, current: app.keys.back) => app.set_shortcut(KeyAction::Back, app.keys.back)
                }
                Row(gap: 8, align: center) {
                    Text(content: tr! { "zh-Hans" => "确认", "en" => "Confirm" }, style: body)
                    Spacer
                    if app.keys.confirm != app.default_keys.confirm {
                        IconButton(id: key_confirm_default, icon: "arrow.triangle.2.circlepath", label: tr! { "zh-Hans" => "恢复默认", "en" => "Restore default" }, prominence: plain) => app.reset_shortcut(KeyAction::Confirm)
                    }
                    ShortcutInput(id: key_confirm, label: tr! { "zh-Hans" => "确认", "en" => "Confirm" }, label_hidden: true, current: app.keys.confirm) => app.set_shortcut(KeyAction::Confirm, app.keys.confirm)
                }
            }
            if build.modules.library && app.features.library {
                Section(title: tr! { "zh-Hans" => "媒体库", "en" => "Library" }) {
                    Row(gap: 8, align: center) {
                        Text(content: tr! { "zh-Hans" => "缩小海报", "en" => "Smaller posters" }, style: body)
                        Spacer
                        if app.keys.smaller_posters != app.default_keys.smaller_posters {
                            IconButton(id: key_smaller_posters_default, icon: "arrow.triangle.2.circlepath", label: tr! { "zh-Hans" => "恢复默认", "en" => "Restore default" }, prominence: plain) => app.reset_shortcut(KeyAction::SmallerPosters)
                        }
                        ShortcutInput(id: key_smaller_posters, label: tr! { "zh-Hans" => "缩小海报", "en" => "Smaller posters" }, label_hidden: true, current: app.keys.smaller_posters) => app.set_shortcut(KeyAction::SmallerPosters, app.keys.smaller_posters)
                    }
                    Row(gap: 8, align: center) {
                        Text(content: tr! { "zh-Hans" => "放大海报", "en" => "Larger posters" }, style: body)
                        Spacer
                        if app.keys.larger_posters != app.default_keys.larger_posters {
                            IconButton(id: key_larger_posters_default, icon: "arrow.triangle.2.circlepath", label: tr! { "zh-Hans" => "恢复默认", "en" => "Restore default" }, prominence: plain) => app.reset_shortcut(KeyAction::LargerPosters)
                        }
                        ShortcutInput(id: key_larger_posters, label: tr! { "zh-Hans" => "放大海报", "en" => "Larger posters" }, label_hidden: true, current: app.keys.larger_posters) => app.set_shortcut(KeyAction::LargerPosters, app.keys.larger_posters)
                    }
                }
            }
            // Only the menu bar has these, and only the Mac shows it.
            if window.menu_bar {
                Section(title: tr! { "zh-Hans" => "菜单栏", "en" => "Menu Bar" }, footer: tr! { "zh-Hans" => "这些操作只在菜单栏中，没有界面上的按钮。", "en" => "These are in the menu bar only, with no button in the window." }) {
                    Row(gap: 8, align: center) {
                        Text(content: tr! { "zh-Hans" => "打开文件夹", "en" => "Open folder" }, style: body)
                        Spacer
                        if app.keys.open_folder != app.default_keys.open_folder {
                            IconButton(id: key_open_folder_default, icon: "arrow.triangle.2.circlepath", label: tr! { "zh-Hans" => "恢复默认", "en" => "Restore default" }, prominence: plain) => app.reset_shortcut(KeyAction::OpenFolder)
                        }
                        ShortcutInput(id: key_open_folder, label: tr! { "zh-Hans" => "打开文件夹", "en" => "Open folder" }, label_hidden: true, current: app.keys.open_folder) => app.set_shortcut(KeyAction::OpenFolder, app.keys.open_folder)
                    }
                    Row(gap: 8, align: center) {
                        Text(content: tr! { "zh-Hans" => "打开链接", "en" => "Open link" }, style: body)
                        Spacer
                        if app.keys.open_link != app.default_keys.open_link {
                            IconButton(id: key_open_link_default, icon: "arrow.triangle.2.circlepath", label: tr! { "zh-Hans" => "恢复默认", "en" => "Restore default" }, prominence: plain) => app.reset_shortcut(KeyAction::OpenLink)
                        }
                        ShortcutInput(id: key_open_link, label: tr! { "zh-Hans" => "打开链接", "en" => "Open link" }, label_hidden: true, current: app.keys.open_link) => app.set_shortcut(KeyAction::OpenLink, app.keys.open_link)
                    }
                    Row(gap: 8, align: center) {
                        Text(content: tr! { "zh-Hans" => "上一集", "en" => "Previous episode" }, style: body)
                        Spacer
                        if app.keys.previous_episode != app.default_keys.previous_episode {
                            IconButton(id: key_previous_episode_default, icon: "arrow.triangle.2.circlepath", label: tr! { "zh-Hans" => "恢复默认", "en" => "Restore default" }, prominence: plain) => app.reset_shortcut(KeyAction::PreviousEpisode)
                        }
                        ShortcutInput(id: key_previous_episode, label: tr! { "zh-Hans" => "上一集", "en" => "Previous episode" }, label_hidden: true, current: app.keys.previous_episode) => app.set_shortcut(KeyAction::PreviousEpisode, app.keys.previous_episode)
                    }
                    Row(gap: 8, align: center) {
                        Text(content: tr! { "zh-Hans" => "下一集", "en" => "Next episode" }, style: body)
                        Spacer
                        if app.keys.next_episode != app.default_keys.next_episode {
                            IconButton(id: key_next_episode_default, icon: "arrow.triangle.2.circlepath", label: tr! { "zh-Hans" => "恢复默认", "en" => "Restore default" }, prominence: plain) => app.reset_shortcut(KeyAction::NextEpisode)
                        }
                        ShortcutInput(id: key_next_episode, label: tr! { "zh-Hans" => "下一集", "en" => "Next episode" }, label_hidden: true, current: app.keys.next_episode) => app.set_shortcut(KeyAction::NextEpisode, app.keys.next_episode)
                    }
                }
            }
            Section {
                Row(gap: 12, align: center) {
                    Text(content: tr! { "zh-Hans" => "点按快捷键，再按下新的组合键；Esc 取消，⌫ 清除。已被其他操作使用的组合不会生效。", "en" => "Click a shortcut, then press the new keys; Esc cancels and ⌫ clears it. Keys another action uses are not taken." }, style: caption, emphasis: secondary)
                    Spacer
                    Button(id: keys_defaults, label: tr! { "zh-Hans" => "全部恢复默认", "en" => "Restore All Defaults" }, enabled: app.keys_changed) => app.reset_shortcuts()
                }
            }
        }
    }
}
#[component]
pub fn settings_about(app: erika::Client) -> impl View {
    ui! {
        Form(variant: settings_form, appear: settings_page_in) {
            Slot(id: form_header)
            Section {
                Column(gap: 6, padding_y: 12, align: center) {
                    Row {
                        Spacer
                        Svg(source: "design/brand/erika-icon-animated.svg", static_source: "design/brand/erika-icon-export/logo/erika-logo-icon.svg", width: 104, height: 104, label: tr! { "zh-Hans" => "Erika", "en" => "Erika" })
                        Spacer
                    }
                    Stack(variant: settings_wordmark) {
                        Image(source: "erika-ui/assets/erika-wordmark.png", height: 72, rendering: template, label: tr! { "zh-Hans" => "ERIKA エリカ", "en" => "ERIKA エリカ" })
                    }
                    Text(content: tr! { "zh-Hans" => "Erika 0.1 · 界面预览", "en" => "Erika 0.1 · Interface preview" }, emphasis: secondary)
                }
            }
            Section {
                Row(gap: 12, align: center) {
                    Text(content: tr! { "zh-Hans" => "检查更新", "en" => "Check for updates" }, style: body)
                    Spacer
                    Button(id: check_updates, label: tr! { "zh-Hans" => "检查", "en" => "Check" }) => app.show_notice(Notice::PreviewVersion)
                }
                Row(gap: 12, align: center) {
                    Text(content: tr! { "zh-Hans" => "开源许可", "en" => "Open-source licenses" }, style: body)
                    Spacer
                    Button(id: licenses, label: tr! { "zh-Hans" => "›", "en" => "›" }, prominence: plain) => app.show_notice(Notice::Licenses)
                }
                Row(gap: 12, align: center) {
                    Text(content: tr! { "zh-Hans" => "致谢", "en" => "Acknowledgements" }, style: body)
                    Spacer
                    Button(id: credits, label: tr! { "zh-Hans" => "›", "en" => "›" }, prominence: plain) => app.show_notice(Notice::Credits)
                }
            }
        }
    }
}
#[component]
pub fn settings_plugins(app: erika::Client) -> impl View {
    ui! {
        ScrollView(appear: settings_page_in) {
        Column(gap: 0) {
        Slot(id: scroll_header)
        // The phone's page keeps 16 from its sides (`phHTML`), as the
        // market's tool strip needs to fit the prototype's chips per line.
        Column(gap: 0, padding_x: if window.compact { 16.0 } else { 28.0 }, padding_y: 22) {
            Stack(variant: settings_banner, height: 128, align: leading, appearance: dark) {
                Canvas {
                    Pattern(name: academy)
                }
                Column(gap: 6, padding_x: 26) {
                    Text(content: tr! { "zh-Hans" => "插件市场", "en" => "Plugin Market" }, style: title2)
                    Text(content: tr! { "zh-Hans" => "小功能按需安装。插件用脚本编写，只能使用它声明过的权限。", "en" => "Install small extras as needed. Plugins can only use their declared permissions." }, style: body)
                }
            }
            Canvas(height: 18)
            // `.m-tools`: the segmented control, then the category chips at
            // the end of the line; where the line is too narrow, the chips
            // that do not fit wrap onto the next one.
            Flow(gap: 10) {
                Picker(id: plugin_tab, label: tr! { "zh-Hans" => "插件", "en" => "Plugins" }, selected: app.plugin_tab, style: segmented, label_hidden: true) {
                    PickerOption(value: PluginTab::Market, label: tr! { "zh-Hans" => "市场", "en" => "Market" })
                    PickerOption(value: PluginTab::Installed, label: tr! { "zh-Hans" => "已安装 {count}", "en" => "Installed {count}"; count: app.installed_count })
                } => app.set_plugin_tab(app.plugin_tab)
                Spacer
                Picker(id: plugin_category, label: tr! { "zh-Hans" => "分类", "en" => "Category" }, selected: app.plugin_category, style: chips, label_hidden: true) => app.set_plugin_category(app.plugin_category)
            }
            Canvas(height: 14)
            Grid(min_width: 300, gap: 12) {
                ForEach(items: app.plugins, key: id) |plugin| {
                    Stack(variant: settings_card) {
                        Canvas {
                            Shape(kind: rounded, corner: 16, stroke: control.border, stroke_width: 0.5)
                        }
                        Column(gap: 8, padding: 16, appear: page_in) {
                            Row(gap: 12) {
                                Stack(width: 42, height: 42) {
                                    Canvas {
                                        Shape(kind: rounded, corner: 11, fill: fill.strong)
                                    }
                                    if plugin.id == "spoiler" {
                                        Label(title: plugin.name, icon: "eye.slash", style: title3, display: icon)
                                    }
                                    if plugin.id == "regex" {
                                        Label(title: plugin.name, icon: "line.3.horizontal.decrease", style: title3, display: icon)
                                    }
                                    if plugin.id == "words" {
                                        Label(title: plugin.name, icon: "shield", style: title3, display: icon)
                                    }
                                    if plugin.id == "crt" {
                                        Label(title: plugin.name, icon: "tv", style: title3, display: icon)
                                    }
                                    if plugin.id == "clip" {
                                        Label(title: plugin.name, icon: "scissors", style: title3, display: icon)
                                    }
                                    if plugin.id == "ext" {
                                        Label(title: plugin.name, icon: "arrow.up.right.square", style: title3, display: icon)
                                    }
                                    if plugin.id == "sfx" {
                                        Label(title: plugin.name, icon: "music.note", style: title3, display: icon)
                                    }
                                    if plugin.id == "theme" {
                                        Label(title: plugin.name, icon: "paintpalette", style: title3, display: icon)
                                    }
                                    if plugin.id == "titan" {
                                        Label(title: plugin.name, icon: "text.bubble", style: title3, display: icon)
                                    }
                                }
                                Column(gap: 3) {
                                    Text(content: plugin.name, style: headline)
                                    Text(content: plugin.author, style: caption, emphasis: secondary)
                                }
                                Spacer
                                if plugin.installed {
                                    Button(id: open_plugin, label: tr! { "zh-Hans" => "打开", "en" => "Open" }) => app.use_plugin(plugin.id)
                                }
                                else {
                                    Button(id: get_plugin, label: tr! { "zh-Hans" => "获取", "en" => "Get" }, prominence: prominent) => app.use_plugin(plugin.id)
                                }
                            }
                            Text(content: plugin.description, emphasis: secondary)
                            Row(gap: 6) {
                                ForEach(items: plugin.tags, key: id) |tag| {
                                    Stack(variant: settings_tag) {
                                        Row(padding_x: 8, padding_y: 2) {
                                            Text(content: tag.permission, style: caption, emphasis: secondary)
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        }
        }
    }
}
