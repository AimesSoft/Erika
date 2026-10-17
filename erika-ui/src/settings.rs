//! Settings from design/next-screens.js and next.css (Apple desktop).
//! A native grouped form on every page (design 10.9: "苹果是分组列表"),
//! a 220-point sidebar, and the title bar named after the page. All
//! preference changes go through the shell protocol.
use bern::prelude::*;
use crate::feature_preview::{feature_preview_hud, feature_preview_window};
use erika_protocol::erika::{self, *};
#[bern::session]
pub struct SettingsDraft {
    pub page: SettingsPage,
    pub query: String,
    #[initial(0.8)]
    pub opacity: f64,
    #[initial(0.45)]
    pub font_size: f64,
    pub danmaku_file: MediaSource,
}
#[component]
pub fn settings(app: erika::Client, #[session] draft: SettingsDraft) -> impl View {
    ui! {
        Stack(align: bottom, reduce_motion: app.preferences.reduce_motion) {
            SplitView {
                Sidebar(id: settings_sections, width: 220, selection: draft.page) {
                    Search(id: settings_search, value: draft.query, prompt: tr! { "zh-Hans" => "搜索设置", "en" => "Search settings" }, shortcut: "cmd+k") {
                        Query(id: settings_results, query: app.find_settings(draft.query), empty: tr! { "zh-Hans" => "没有匹配的设置", "en" => "No matching settings" }) |results| {
                            ForEach(items: results, key: id) |result| {
                                Button(id: settings_result, label: result.title, prominence: plain) => draft.page.set(result.page)
                            }
                        }
                    }
                    SidebarItem(value: SettingsPage::General, label: tr! { "zh-Hans" => "通用", "en" => "General" }, icon: "gearshape")
                    SidebarItem(value: SettingsPage::Appearance, label: tr! { "zh-Hans" => "外观", "en" => "Appearance" }, icon: "paintpalette")
                    SidebarItem(value: SettingsPage::Player, label: tr! { "zh-Hans" => "播放", "en" => "Playback" }, icon: "play")
                    if app.features.danmaku {
                        SidebarItem(value: SettingsPage::Danmaku, label: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, icon: "text.bubble")
                    }
                    if app.features.library {
                        SidebarItem(value: SettingsPage::Library, label: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, icon: "film.stack")
                    }
                    if app.features.anime {
                        SidebarItem(value: SettingsPage::Anime, label: tr! { "zh-Hans" => "追番", "en" => "Anime" }, icon: "calendar")
                    }
                    if app.features.sync {
                        SidebarItem(value: SettingsPage::Sync, label: tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" }, icon: "arrow.triangle.2.circlepath")
                    }
                    if app.features.sources {
                        SidebarItem(value: SettingsPage::Sources, label: tr! { "zh-Hans" => "网络媒体源", "en" => "Network Sources" }, icon: "server.rack")
                    }
                    if app.features.remote {
                        SidebarItem(value: SettingsPage::Remote, label: tr! { "zh-Hans" => "局域网与远程", "en" => "LAN and Remote" }, icon: "wifi")
                    }
                    if app.features.downloads {
                        SidebarItem(value: SettingsPage::Downloads, label: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, icon: "arrow.down.circle")
                    }
                    if app.features.enhance {
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
                    Column(gap: 0) {
                        match draft.page {
                            SettingsPage::General => { settings_general(app: app) }
                            SettingsPage::Appearance => { settings_appearance(app: app) }
                            SettingsPage::Player => { settings_player(app: app) }
                            SettingsPage::Danmaku => { settings_danmaku(app: app) }
                            SettingsPage::Library => { settings_module_placeholder(app: app) }
                            SettingsPage::Anime => { settings_module_placeholder(app: app) }
                            SettingsPage::Sync => { settings_module_placeholder(app: app) }
                            SettingsPage::Sources => { settings_module_placeholder(app: app) }
                            SettingsPage::Remote => { settings_module_placeholder(app: app) }
                            SettingsPage::Downloads => { settings_module_placeholder(app: app) }
                            SettingsPage::Enhance => { settings_module_placeholder(app: app) }
                            SettingsPage::Optional => { settings_optional(app: app) }
                            SettingsPage::Plugins => { settings_plugins(app: app) }
                            SettingsPage::Keys => { settings_keys(app: app) }
                            SettingsPage::About => { settings_about(app: app) }
                        }
                    }
                }
            }
            if app.notice != "" {
                Toast(content: app.notice, key: app.notice_revision, duration: 3)
            }
        }
    }
}
#[component]
pub fn settings_general(app: erika::Client) -> impl View {
    ui! {
        Form(appear: settings_page_in) {
            Section(title: tr! { "zh-Hans" => "启动", "en" => "Startup" }) {
                Picker(id: startup, label: tr! { "zh-Hans" => "启动时打开", "en" => "Open at startup" }, selected: app.preferences.startup, style: menu) => app.set_startup(app.preferences.startup)
                Picker(id: playback_end, label: tr! { "zh-Hans" => "播放结束后", "en" => "When playback ends" }, selected: app.preferences.playback_end, style: menu) => app.set_playback_end(app.preferences.playback_end)
            }
            Section(title: tr! { "zh-Hans" => "语言", "en" => "Language" }) {
                Picker(id: language, label: tr! { "zh-Hans" => "界面语言", "en" => "Interface language" }, selected: app.preferences.language, style: menu) => app.set_language(app.preferences.language)
            }
            Section(title: tr! { "zh-Hans" => "更新", "en" => "Updates" }) {
                Switch(id: auto_updates, label: tr! { "zh-Hans" => "自动检查更新", "en" => "Automatically check for updates" }, checked: app.preferences.auto_updates, style: switch) => app.set_preference("auto_updates", app.preferences.auto_updates)
                Picker(id: update_channel, label: tr! { "zh-Hans" => "更新通道", "en" => "Update channel" }, selected: app.preferences.update_channel, style: menu) => app.set_update_channel(app.preferences.update_channel)
            }
        }
    }
}
#[component]
pub fn settings_appearance(app: erika::Client) -> impl View {
    ui! {
        Form(appear: settings_page_in) {
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
        Form(appear: settings_page_in) {
            Section(title: tr! { "zh-Hans" => "播放", "en" => "Playback" }) {
                Picker(id: speed, label: tr! { "zh-Hans" => "默认倍速", "en" => "Default speed" }, selected: app.preferences.speed, style: menu) => app.set_speed(app.preferences.speed)
                Row(gap: 12, align: center) {
                    Text(content: tr! { "zh-Hans" => "跳过片头", "en" => "Skip intro" }, style: body)
                    Spacer
                    Text(content: tr! { "zh-Hans" => "85 秒", "en" => "85 seconds" }, style: body, emphasis: secondary)
                }
                Switch(id: remember_position, label: tr! { "zh-Hans" => "记住播放位置", "en" => "Remember playback position" }, checked: app.preferences.remember_position, style: switch) => app.set_preference("remember_position", app.preferences.remember_position)
                Switch(id: autoplay, label: tr! { "zh-Hans" => "自动连播下一集", "en" => "Automatically play next episode" }, checked: app.preferences.autoplay, style: switch) => app.set_preference("autoplay", app.preferences.autoplay)
            }
            Section(title: tr! { "zh-Hans" => "画框", "en" => "Frame" }) {
                Row(gap: 12, align: center) {
                    Column(gap: 1) {
                        Text(content: tr! { "zh-Hans" => "控制层出现时画面退后", "en" => "Step back when controls appear" }, style: body)
                        Text(content: tr! { "zh-Hans" => "关掉后控制层叠在画面底部，用纯色底", "en" => "When off, controls overlay the bottom of the picture" }, style: caption, emphasis: secondary)
                    }
                    Spacer
                    Switch(id: step_back, label: tr! { "zh-Hans" => "控制层出现时画面退后", "en" => "Step back when controls appear" }, checked: app.preferences.step_back, style: switch, label_hidden: true) => app.set_preference("step_back", app.preferences.step_back)
                }
                Row(gap: 12, align: center) {
                    Text(content: tr! { "zh-Hans" => "画框圆角", "en" => "Frame corners" }, style: body)
                    Spacer
                    Text(content: tr! { "zh-Hans" => "跟随系统", "en" => "System" }, style: body, emphasis: secondary)
                }
            }
            Section(title: tr! { "zh-Hans" => "解码", "en" => "Decoding" }) {
                Picker(id: kernel, label: tr! { "zh-Hans" => "播放内核", "en" => "Playback engine" }, selected: app.preferences.kernel, style: menu) => app.set_kernel(app.preferences.kernel)
                Switch(id: hardware_decode, label: tr! { "zh-Hans" => "硬件解码", "en" => "Hardware decoding" }, checked: app.preferences.hardware_decode, style: switch) => app.set_preference("hardware_decode", app.preferences.hardware_decode)
            }
        }
    }
}
#[component]
pub fn settings_danmaku(app: erika::Client, #[session] draft: SettingsDraft) -> impl View {
    ui! {
        Form(appear: settings_page_in) {
            Section(title: tr! { "zh-Hans" => "显示", "en" => "Display" }) {
                Switch(id: show_danmaku, label: tr! { "zh-Hans" => "显示弹幕", "en" => "Show danmaku" }, checked: app.preferences.show_danmaku, style: switch) => app.set_preference("show_danmaku", app.preferences.show_danmaku)
                Slider(id: opacity, label: tr! { "zh-Hans" => "不透明度", "en" => "Opacity" }, value: draft.opacity, min: 0, max: 1)
                Slider(id: font_size, label: tr! { "zh-Hans" => "字号", "en" => "Font size" }, value: draft.font_size, min: 0, max: 1)
                Picker(id: area, label: tr! { "zh-Hans" => "显示区域", "en" => "Display area" }, selected: app.preferences.danmaku_area, style: segmented) => app.set_danmaku_area(app.preferences.danmaku_area)
            }
            Section(title: tr! { "zh-Hans" => "来源", "en" => "Sources" }) {
                Switch(id: auto_match, label: tr! { "zh-Hans" => "弹弹play 自动匹配", "en" => "Automatically match with Dandanplay" }, checked: app.preferences.auto_match, style: switch) => app.set_preference("auto_match", app.preferences.auto_match)
                Row(gap: 12, align: center) {
                    Column(gap: 1) {
                        Text(content: tr! { "zh-Hans" => "本地弹幕文件", "en" => "Local danmaku file" }, style: body)
                        Text(content: tr! { "zh-Hans" => "xml / json", "en" => "xml / json" }, style: caption, emphasis: secondary)
                    }
                    Spacer
                    Button(id: mount_danmaku, label: tr! { "zh-Hans" => "挂载…", "en" => "Mount…" }, pick: draft.danmaku_file, files: ".xml .json") => app.show_notice("已挂载本地弹幕文件")
                }
            }
        }
    }
}
#[component]
pub fn settings_module_placeholder(app: erika::Client) -> impl View {
    ui! {
        Form(appear: settings_page_in) {
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
/// accent off list icons (10.9).
#[component]
pub fn settings_optional(app: erika::Client) -> impl View {
    ui! {
        Row(gap: 0, align: top) {
            Form(appear: settings_page_in) {
                Section(title: tr! { "zh-Hans" => "观看", "en" => "Watching" }, footer: tr! { "zh-Hans" => "决定 Erika 要做多少事。关掉的功能不显示，也不占资源。右边的小窗口会跟着变。", "en" => "Choose what Erika does. Disabled features disappear and use no resources. The preview updates as you go." }) {
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
                    Row(gap: 12, align: center) {
                        Stack(width: 28, height: 28) {
                            Canvas {
                                Shape(kind: rounded, corner: 7, fill: fill.strong)
                            }
                            Label(title: tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" }, icon: "sparkles", style: body, display: icon)
                        }
                        Column(gap: 1) {
                            Text(content: tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" }, style: body)
                            Text(content: tr! { "zh-Hans" => "Anime4K / ArtCNN 超分辨率", "en" => "Anime4K / ArtCNN upscaling" }, style: caption, emphasis: secondary)
                        }
                        Spacer
                        Switch(id: feature_enhance, label: tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" }, checked: app.features.enhance, style: switch, label_hidden: true) => app.set_module("enhance", app.features.enhance)
                    }
                }
                Section(title: tr! { "zh-Hans" => "内容", "en" => "Content" }) {
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
                    Row(gap: 12, align: center) {
                        Stack(width: 28, height: 28) {
                            Canvas {
                                Shape(kind: rounded, corner: 7, fill: fill.strong)
                            }
                            Label(title: tr! { "zh-Hans" => "追番", "en" => "Anime" }, icon: "calendar", style: body, display: icon)
                        }
                        Column(gap: 1) {
                            Text(content: tr! { "zh-Hans" => "追番", "en" => "Anime" }, style: body)
                            Text(content: tr! { "zh-Hans" => "首页推荐、新番表、番剧详情", "en" => "Home, airing calendar and show details" }, style: caption, emphasis: secondary)
                        }
                        Spacer
                        Switch(id: feature_anime, label: tr! { "zh-Hans" => "追番", "en" => "Anime" }, checked: app.features.anime, style: switch, label_hidden: true) => app.set_module("anime", app.features.anime)
                    }
                    Row(gap: 12, align: center) {
                        Stack(width: 28, height: 28) {
                            Canvas {
                                Shape(kind: rounded, corner: 7, fill: fill.strong)
                            }
                            Label(title: tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" }, icon: "arrow.triangle.2.circlepath", style: body, display: icon)
                        }
                        Column(gap: 1) {
                            Text(content: tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" }, style: body)
                            Text(content: tr! { "zh-Hans" => "弹弹play / Bangumi 同步 · 需要追番", "en" => "Dandanplay / Bangumi · requires Anime" }, style: caption, emphasis: secondary)
                        }
                        Spacer
                        Switch(id: feature_sync, label: tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" }, checked: app.features.sync, style: switch, label_hidden: true) => app.set_module("sync", app.features.sync)
                    }
                }
                Section(title: tr! { "zh-Hans" => "连接", "en" => "Connections" }) {
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
            // A scroll view of its own, so the title bar's scroll edge spans
            // this column as it does the form beside it.
            ScrollView(appear: settings_page_in) {
                Row(gap: 0, align: top) {
                Column(gap: 0, width: 300, padding_y: 20) {
                    Text(content: tr! { "zh-Hans" => "预览", "en" => "Preview" }, style: caption2, emphasis: secondary)
                    Canvas(height: 8)
                    settings_preview(app: app)
                    Canvas(height: 10)
                    Row(gap: 6) {
                        Button(id: preset_pure, label: tr! { "zh-Hans" => "纯粹播放", "en" => "Pure playback" }) => app.apply_preset(FeaturePreset::Pure)
                        Button(id: preset_anime, label: tr! { "zh-Hans" => "看番", "en" => "Anime" }) => app.apply_preset(FeaturePreset::Anime)
                        Button(id: preset_all, label: tr! { "zh-Hans" => "全都要", "en" => "Everything" }) => app.apply_preset(FeaturePreset::All)
                    }
                }
                Canvas(width: 20)
                }
            }
        }
    }
}
#[component]
pub fn settings_preview(app: erika::Client) -> impl View {
    ui! {
        Column(gap: 0, width: 300) {
            feature_preview_window(features: app.features, revision: app.feature_preview_revision)
            Canvas(height: 10)
            feature_preview_hud(features: app.features)
            Canvas(height: 14)
            Text(content: tr! { "zh-Hans" => "已开启 {count} 个模块 · {preset}", "en" => "{count} modules enabled · {preset}"; count: app.module_count, preset: app.preset_name }, emphasis: secondary)
        }
    }
}
#[component]
pub fn settings_keys(app: erika::Client) -> impl View {
    ui! {
        Form(appear: settings_page_in) {
            Section(title: tr! { "zh-Hans" => "播放", "en" => "Playback" }) {
                Row(gap: 12, align: center) {
                    Text(content: tr! { "zh-Hans" => "播放 / 暂停", "en" => "Play / pause" }, style: body)
                    Spacer
                    Text(content: tr! { "zh-Hans" => "空格", "en" => "Space" }, style: body, emphasis: secondary)
                }
                Row(gap: 12, align: center) {
                    Text(content: tr! { "zh-Hans" => "快退 / 快进 5 秒", "en" => "Seek backward / forward 5 seconds" }, style: body)
                    Spacer
                    Text(content: tr! { "zh-Hans" => "← / →", "en" => "← / →" }, style: body, emphasis: secondary)
                }
                if app.features.danmaku {
                    Row(gap: 12, align: center) {
                        Text(content: tr! { "zh-Hans" => "开关弹幕", "en" => "Toggle danmaku" }, style: body)
                        Spacer
                        Text(content: tr! { "zh-Hans" => "D", "en" => "D" }, style: body, emphasis: secondary)
                    }
                }
                Row(gap: 12, align: center) {
                    Text(content: tr! { "zh-Hans" => "全屏", "en" => "Full screen" }, style: body)
                    Spacer
                    Text(content: tr! { "zh-Hans" => "F", "en" => "F" }, style: body, emphasis: secondary)
                }
            }
            Section(title: tr! { "zh-Hans" => "全局", "en" => "Global" }) {
                Row(gap: 12, align: center) {
                    Text(content: tr! { "zh-Hans" => "打开文件", "en" => "Open file" }, style: body)
                    Spacer
                    Text(content: tr! { "zh-Hans" => "⌘O", "en" => "⌘O" }, style: body, emphasis: secondary)
                }
                Row(gap: 12, align: center) {
                    Text(content: tr! { "zh-Hans" => "搜索", "en" => "Search" }, style: body)
                    Spacer
                    Text(content: tr! { "zh-Hans" => "⌘K", "en" => "⌘K" }, style: body, emphasis: secondary)
                }
                Row(gap: 12, align: center) {
                    Text(content: tr! { "zh-Hans" => "设置", "en" => "Settings" }, style: body)
                    Spacer
                    Text(content: tr! { "zh-Hans" => "⌘,", "en" => "⌘," }, style: body, emphasis: secondary)
                }
            }
        }
    }
}
#[component]
pub fn settings_about(app: erika::Client) -> impl View {
    ui! {
        Form(appear: settings_page_in) {
            Section {
                Column(gap: 6, padding_y: 12, align: center) {
                    Row {
                        Spacer
                        Svg(source: "design/brand/erika-icon-animated.svg", width: 104, height: 104, label: tr! { "zh-Hans" => "Erika", "en" => "Erika" })
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
                    Button(id: check_updates, label: tr! { "zh-Hans" => "检查", "en" => "Check" }) => app.show_notice("当前为界面预览版本")
                }
                Row(gap: 12, align: center) {
                    Text(content: tr! { "zh-Hans" => "开源许可", "en" => "Open-source licenses" }, style: body)
                    Spacer
                    Button(id: licenses, label: tr! { "zh-Hans" => "›", "en" => "›" }, prominence: plain) => app.show_notice("Erika · Rust / Bern / SwiftUI")
                }
                Row(gap: 12, align: center) {
                    Text(content: tr! { "zh-Hans" => "致谢", "en" => "Acknowledgements" }, style: body)
                    Spacer
                    Button(id: credits, label: tr! { "zh-Hans" => "›", "en" => "›" }, prominence: plain) => app.show_notice("感谢所有开源贡献者与社区支持者")
                }
            }
        }
    }
}
#[component]
pub fn settings_plugins(app: erika::Client) -> impl View {
    ui! {
        ScrollView(appear: settings_page_in) {
        Column(gap: 0, padding_x: 28, padding_y: 22) {
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
            Row(gap: 10) {
                Row(width: 160) {
                    Picker(id: plugin_tab, label: tr! { "zh-Hans" => "插件", "en" => "Plugins" }, selected: app.plugin_tab, style: segmented, label_hidden: true) => app.set_plugin_tab(app.plugin_tab)
                }
                Spacer
                Text(content: tr! { "zh-Hans" => "已安装 {count}", "en" => "{count} installed"; count: app.installed_count }, style: caption, emphasis: secondary)
                Picker(id: plugin_category, label: tr! { "zh-Hans" => "分类", "en" => "Category" }, selected: app.plugin_category, style: segmented, label_hidden: true) => app.set_plugin_category(app.plugin_category)
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
                                            Text(content: tag.label, style: caption, emphasis: secondary)
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
