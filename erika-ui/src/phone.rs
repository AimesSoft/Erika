//! The phone's shell, measured from the prototype at 390 × 844
//! (design/next-screens.js `renderPhone`, next.css `.x-pnav`): a tab for
//! every page with search last, and above each page its header row, the
//! large title with grouped actions and settings, or the way back while
//! a show's details are open.
use crate::pure::{phone_pure, phone_pure_props};
use crate::search::{search_results, search_results_props};
use crate::settings::*;
use crate::{AppWindows, ErikaDraft, MainPage};
use bern::prelude::*;
#[cfg(feature = "bern-module-anime")]
use erika_anime_protocol::anime::{self, BackTo};
#[cfg(feature = "bern-module-anime")]
use erika_anime_ui::{details, details_props};
use erika_protocol::erika::{self, SettingsPage};

/// Where the phone's settings stand (`phStack`): closed, their list, or
/// one of their sections, pushed over the current tab.
#[bern::value]
pub enum PhoneSettings {
    #[label(tr! { "zh-Hans" => "关闭", "en" => "Closed" })]
    Closed,
    #[label(tr! { "zh-Hans" => "设置", "en" => "Settings" })]
    List,
    #[label(tr! { "zh-Hans" => "账号", "en" => "Account" })]
    Account,
    #[label(tr! { "zh-Hans" => "通用", "en" => "General" })]
    General,
    #[label(tr! { "zh-Hans" => "外观", "en" => "Appearance" })]
    Appearance,
    #[label(tr! { "zh-Hans" => "播放", "en" => "Playback" })]
    Player,
    #[label(tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" })]
    Danmaku,
    #[label(tr! { "zh-Hans" => "媒体库", "en" => "Library" })]
    Library,
    #[label(tr! { "zh-Hans" => "新番", "en" => "New Shows" })]
    Anime,
    #[label(tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" })]
    Sync,
    #[label(tr! { "zh-Hans" => "网络媒体源", "en" => "Network Sources" })]
    Sources,
    #[label(tr! { "zh-Hans" => "局域网与远程", "en" => "LAN and Remote" })]
    Remote,
    #[label(tr! { "zh-Hans" => "下载", "en" => "Downloads" })]
    Downloads,
    #[label(tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" })]
    Enhance,
    #[label(tr! { "zh-Hans" => "可选功能", "en" => "Optional Features" })]
    Optional,
    #[label(tr! { "zh-Hans" => "插件", "en" => "Plugins" })]
    Plugins,
    #[label(tr! { "zh-Hans" => "快捷键", "en" => "Keyboard Shortcuts" })]
    Keys,
    #[label(tr! { "zh-Hans" => "关于", "en" => "About" })]
    About,
}

#[component]
pub fn phone_shell(
    app: erika::Client,
    #[cfg(feature = "bern-module-anime")] anime: anime::Client,
    windows: AppWindows,
    #[session] draft: ErikaDraft,
) -> impl View {
    ui! {
        // Settings replace the whole tab shell, so the hidden pages cannot
        // be switched underneath them. Closing restores the selected tab.
        if draft.phone_settings != PhoneSettings::Closed {
            Stack(variant: phone_page, align: top_leading) {
                if draft.phone_settings == PhoneSettings::List {
                    phone_settings(app: app)
                } else {
                    phone_settings_section(app: app, #[cfg(feature = "bern-module-anime")] anime: anime)
                }
            }
        // Pure playback (design 10.2): no pages, so no tab bar.
        } else if !(build.modules.anime && app.features.anime) && !(build.modules.library && app.features.library) && !(build.modules.downloads && app.features.downloads) {
            phone_pure(app: app, #[cfg(feature = "bern-module-anime")] anime: anime)
        } else {
        Tabs(id: tabs, selection: draft.page) {
            if (build.modules.anime && app.features.anime) {
                Tab(value: MainPage::Home, label: tr! { "zh-Hans" => "首页", "en" => "Home" }, icon: "house") {
                    phone_tab_content(app: app, #[cfg(feature = "bern-module-anime")] anime: anime, windows: windows)
                }
                Tab(value: MainPage::New, label: tr! { "zh-Hans" => "新番", "en" => "New" }, icon: "calendar") {
                    phone_tab_content(app: app, #[cfg(feature = "bern-module-anime")] anime: anime, windows: windows)
                }
            }
            if (build.modules.library && app.features.library) {
                Tab(value: MainPage::Library, label: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, icon: "film.stack") {
                    phone_tab_content(app: app, #[cfg(feature = "bern-module-anime")] anime: anime, windows: windows)
                }
            }
            if (build.modules.downloads && app.features.downloads) {
                Tab(value: MainPage::Downloads, label: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, icon: "arrow.down.circle") {
                    phone_tab_content(app: app, #[cfg(feature = "bern-module-anime")] anime: anime, windows: windows)
                }
            }
            Tab(value: MainPage::Search, label: tr! { "zh-Hans" => "搜索", "en" => "Search" }, icon: "magnifyingglass") {
                phone_tab_content(app: app, #[cfg(feature = "bern-module-anime")] anime: anime, windows: windows)
            }
            // `.x-pmini`: what plays, above the tab bar on every page.
            TabAccessory(shown: app.playing && !app.expanded) {
                phone_mini_player(app: app)
            }
        }
        }
    }
}

/// The selected tab's page or details. Settings are owned by the shell.
#[component]
pub fn phone_tab_content(
    app: erika::Client,
    #[cfg(feature = "bern-module-anime")] anime: anime::Client,
    windows: AppWindows,
    #[session] draft: ErikaDraft,
) -> impl View {
    ui! {
        Stack(variant: phone_page, align: top_leading) {
            if build.modules.anime && anime.opened > 0 && ((build.modules.library && draft.page == MainPage::Library && anime.back == BackTo::Library) || (draft.page == MainPage::Search && anime.back == BackTo::Search)) {
                details(app: app, #[cfg(feature = "bern-module-anime")] anime: anime)
            } else if draft.page == MainPage::Search {
                phone_search(app: app, #[cfg(feature = "bern-module-anime")] anime: anime, windows: windows)
            } else {
                PageContent(slot: main, selected: draft.page)
            }
        }
    }
}

/// The same trailing action is contributed to module-owned headers too.
/// Signed in with a picture, the button is the account's picture; it opens
/// the settings either way.
#[component]
pub fn phone_settings_button(app: erika::Client, #[session] draft: ErikaDraft) -> impl View {
    ui! {
        if let Some(picture) = app.user_avatar {
            Button(id: phone_settings_avatar, label: tr! { "zh-Hans" => "设置", "en" => "Settings" }, variant: phone_avatar) {
                Image(source: picture, width: 30, height: 30, corner: 15, fit: cover, label: app.user_name)
            } => draft.phone_settings.set(PhoneSettings::List)
        } else {
            IconButton(id: phone_settings, icon: "gearshape", label: tr! { "zh-Hans" => "设置", "en" => "Settings" }, prominence: plain) => draft.phone_settings.set(PhoneSettings::List)
        }
    }
}

/// The phone's mini player (`.x-pmini .x-mp`): the picture 64 × 36 with
/// its progress, the title and where it is, then play or pause and close
/// on 28-point targets. Touching the picture or the words grows the player
/// back.
#[component]
pub fn phone_mini_player(app: erika::Client) -> impl View {
    ui! {
        Row(gap: 0, padding_x: 6, align: center) {
            Button(id: phone_mini, label: app.title, variant: stage) {
                Row(gap: 4, align: center) {
                    Stack(align: bottom_leading) {
                        Image(source: app.still, width: 64, height: 36, corner: 10, fit: cover, label: app.title, hero: frame, hero_key: app.title)
                        Rail {
                            Canvas(from: 0, to: 1, height: 3) { Shape(kind: rect, fill: surface.canvas, opacity: 0.35) }
                            Canvas(from: 0, to: app.progress, height: 3) { Shape(kind: rect, fill: accent) }
                        }
                    }
                    Column(gap: 0, padding_x: 4) {
                        Text(content: app.title, style: caption2, lines: 1, line_height: 19)
                        Text(content: tr! { "zh-Hans" => "{subtitle} · {elapsed, duration, mm:ss}", "en" => "{subtitle} · {elapsed, duration, mm:ss}"; subtitle: app.subtitle, elapsed: app.elapsed }, style: caption, font_size: 11, emphasis: secondary, lines: 1, line_height: 16.25)
                    }
                    Spacer
                }
            } => app.expand()
            if app.paused {
                IconButton(id: phone_mini_play, icon: "play.fill", font_size: 11, height: 28, label: tr! { "zh-Hans" => "播放", "en" => "Play" }) => app.toggle_pause()
            } else {
                IconButton(id: phone_mini_pause, icon: "pause.fill", font_size: 11, height: 28, label: tr! { "zh-Hans" => "暂停", "en" => "Pause" }) => app.toggle_pause()
            }
            IconButton(id: phone_mini_close, icon: "xmark", font_size: 11, height: 28, label: tr! { "zh-Hans" => "关闭", "en" => "Close" }) => app.close_player()
        }
    }
}

/// The search page (`phHTML('search')`): the system's search field 4
/// points under the title, and while it is in use the palette's results
/// 8 points under it, scrolling.
#[component]
pub fn phone_search(
    app: erika::Client,
    #[cfg(feature = "bern-module-anime")] anime: anime::Client,
    windows: AppWindows,
    #[session] draft: ErikaDraft,
) -> impl View {
    ui! {
        ScrollView(appear: page_in) {
            Column(gap: 0) {
            phone_header(app: app, #[cfg(feature = "bern-module-anime")] anime: anime)
            Column(gap: 0, padding_x: 16) {
                Canvas(height: 4)
                // `.c-field.x-search`: 34 points, 13.5-point text.
                Search(id: phone_search, value: draft.query, prompt: tr! { "zh-Hans" => "番剧、剧集、设置", "en" => "Shows, episodes, settings" }, height: 34, font_size: 13.5) {
                    Column(gap: 0) {
                        Canvas(height: 8)
                        search_results(app: app, #[cfg(feature = "bern-module-anime")] anime: anime, windows: windows)
                    }
                }
                Canvas(height: 24)
            }
            }
        }
    }
}

/// `.x-pnav`: 48 points tall, 16 from the edges, its top 54 points down,
/// as the prototype's status bar is tall; the iPhone's safe area is 47.
/// The title is the large title (30 / 36, bold); trailing actions share
/// the library's 44-point control group, with settings last.
#[component]
pub fn phone_header(
    app: erika::Client,
    #[cfg(feature = "bern-module-anime")] anime: anime::Client,
    #[session] draft: ErikaDraft,
) -> impl View {
    ui! {
        Column(gap: 0) {
        Canvas(height: 7)
        if build.modules.anime && anime.opened > 0 && ((draft.page == MainPage::Home && !anime.opened_from_schedule) || (draft.page == MainPage::New && anime.opened_from_schedule) || (build.modules.library && draft.page == MainPage::Library && anime.back == BackTo::Library) || (draft.page == MainPage::Search && anime.back == BackTo::Search)) {
            // The way back as iOS 26's own apps have it: the chevron on a
            // 44-point Liquid Glass circle, 16 from the edge, its label the
            // page it returns to.
            Row(gap: 0, padding_x: 16, height: 48, align: center) {
                IconButton(id: phone_back, icon: "chevron.left", font_size: 18, height: 44, label: anime.back) => anime.close()
                Spacer
                ControlGroup(label: tr! { "zh-Hans" => "页面操作", "en" => "Page actions" }, height: 44) {
                    phone_settings_button(app: app)
                }
            }
        } else if !build.modules.library || draft.page != MainPage::Library {
        // The library draws its own header row, with its buttons.
        Row(gap: 10, padding_x: 16, height: 48, align: center) {
                match draft.page {
                    #[cfg(feature = "bern-module-anime")]
                    MainPage::Home => {
                        Text(content: tr! { "zh-Hans" => "首页", "en" => "Home" }, style: large_title)
                    }
                    #[cfg(feature = "bern-module-anime")]
                    MainPage::New => {
                        Text(content: tr! { "zh-Hans" => "新番", "en" => "New" }, style: large_title)
                    }
                    #[cfg(feature = "bern-module-library")]
                    MainPage::Library => {
                        Text(content: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, style: large_title)
                    }
                    #[cfg(feature = "bern-module-downloads")]
                    MainPage::Downloads => {
                        Text(content: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, style: large_title)
                    }
                    MainPage::Search => {
                        Text(content: tr! { "zh-Hans" => "搜索", "en" => "Search" }, style: large_title)
                    }
                    _ => {}
                }
                Spacer
                ControlGroup(label: tr! { "zh-Hans" => "页面操作", "en" => "Page actions" }, height: 44) {
                    if build.modules.anime && draft.page == MainPage::Home {
                        IconButton(id: phone_open, icon: "plus", label: tr! { "zh-Hans" => "打开文件", "en" => "Open File" }, prominence: plain, pick: draft.file, files: "video") => app.open_file(draft.file)
                    }
                    phone_settings_button(app: app)
                }
            }
            // The season under the title (`.x-mut`, 13 / 18), 4 points up
            // into the header's foot.
            if build.modules.anime && draft.page == MainPage::New {
                Row(gap: 0, padding_x: 16, height: 14, align: bottom) {
                    Text(content: anime.season_label, font_size: 13, line_height: 18, emphasis: secondary, lines: 1)
                }
            }
        }
        }
    }
}

/// The phone's settings (`phHTML('settings')`): the way back, the large
/// title and the search over the account and the sections, all scrolling
/// as one grouped list on the page's background.
#[component]
pub fn phone_settings(
    app: erika::Client,
    #[session] draft: ErikaDraft,
    #[session] settings: SettingsDraft,
) -> impl View {
    ui! {
        // `.c-list`s 2 points apart, their rows (`.c-li`) at least 44 tall,
        // 6 and 14 inside.
        Form(appear: page_in, gap: 2, row_height: 44) {
            Section(style: plain) {
                Column(gap: 0) {
                    Canvas(height: 7)
                    Row(gap: 0, height: 48, align: center) {
                        IconButton(id: settings_back, icon: "chevron.left", font_size: 18, height: 44, label: tr! { "zh-Hans" => "返回", "en" => "Back" }) => draft.phone_settings.set(PhoneSettings::Closed)
                        Spacer
                    }
                    // `.x-lt` 4 points up into the row above.
                    Row(gap: 0, height: 32, align: bottom) {
                        Text(content: tr! { "zh-Hans" => "设置", "en" => "Settings" }, style: large_title, line_height: 36)
                    }
                    Canvas(height: 18)
                    Search(id: settings_search_phone, value: settings.query, prompt: tr! { "zh-Hans" => "搜索设置", "en" => "Search settings" }, height: 34, font_size: 13.5) {
                        Query(id: settings_results_phone, query: app.find_settings(settings.query), empty: tr! { "zh-Hans" => "没有匹配的设置", "en" => "No matching settings" }) |results| {
                            Column(gap: 0) {
                                Canvas(height: 8)
                                ForEach(items: results, key: id) |result| {
                                    phone_settings_result(app: app, result: result)
                                }
                            }
                        }
                    }
                    Canvas(height: 2)
                }
            }
            // `.ph-acct`: the account, its picture 40 points across; it
            // opens the account's form.
            if build.modules.account && app.features.account {
            Section(padding_x: 14, padding_y: 6) {
                Button(id: settings_account, label: tr! { "zh-Hans" => "账号", "en" => "Account" }) {
                    Row(gap: 12, align: center) {
                        phone_account_picture(app: app)
                        Column(gap: 1) {
                            Text(content: app.user_name, font_size: 13, line_height: 18, lines: 1)
                            Text(content: app.user_account, style: caption, emphasis: secondary, line_height: 15)
                        }
                        Spacer
                        Text(content: tr! { "zh-Hans" => "›", "en" => "›" }, font_size: 13, emphasis: secondary)
                    }
                } => draft.phone_settings.set(PhoneSettings::Account)
            }
            }
            Section(padding_x: 14, padding_y: 6) {
                Button(id: settings_general, label: tr! { "zh-Hans" => "通用", "en" => "General" }) {
                    Row(gap: 12, align: center) {
                        Stack(variant: setting_icon, width: 28, height: 28, align: center) {
                            Label(title: tr! { "zh-Hans" => "通用", "en" => "General" }, icon: "gearshape", display: icon, font_size: 15)
                        }
                        Text(content: tr! { "zh-Hans" => "通用", "en" => "General" }, font_size: 13, line_height: 18)
                        Spacer
                        Text(content: tr! { "zh-Hans" => "›", "en" => "›" }, font_size: 13, emphasis: secondary)
                    }
                } => draft.phone_settings.set(PhoneSettings::General)
                Button(id: settings_appearance, label: tr! { "zh-Hans" => "外观", "en" => "Appearance" }) {
                    Row(gap: 12, align: center) {
                        Stack(variant: setting_icon, width: 28, height: 28, align: center) {
                            Label(title: tr! { "zh-Hans" => "外观", "en" => "Appearance" }, icon: "paintpalette", display: icon, font_size: 15)
                        }
                        Text(content: tr! { "zh-Hans" => "外观", "en" => "Appearance" }, font_size: 13, line_height: 18)
                        Spacer
                        Text(content: tr! { "zh-Hans" => "›", "en" => "›" }, font_size: 13, emphasis: secondary)
                    }
                } => draft.phone_settings.set(PhoneSettings::Appearance)
                Button(id: settings_player, label: tr! { "zh-Hans" => "播放", "en" => "Playback" }) {
                    Row(gap: 12, align: center) {
                        Stack(variant: setting_icon, width: 28, height: 28, align: center) {
                            Label(title: tr! { "zh-Hans" => "播放", "en" => "Playback" }, icon: "play", display: icon, font_size: 15)
                        }
                        Text(content: tr! { "zh-Hans" => "播放", "en" => "Playback" }, font_size: 13, line_height: 18)
                        Spacer
                        Text(content: tr! { "zh-Hans" => "›", "en" => "›" }, font_size: 13, emphasis: secondary)
                    }
                } => draft.phone_settings.set(PhoneSettings::Player)
                if (build.modules.danmaku && app.features.danmaku) {
                    Button(id: settings_danmaku, label: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }) {
                        Row(gap: 12, align: center) {
                            Stack(variant: setting_icon, width: 28, height: 28, align: center) {
                                Label(title: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, icon: "text.bubble", display: icon, font_size: 15)
                            }
                            Text(content: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, font_size: 13, line_height: 18)
                            Spacer
                            Text(content: tr! { "zh-Hans" => "›", "en" => "›" }, font_size: 13, emphasis: secondary)
                        }
                    } => draft.phone_settings.set(PhoneSettings::Danmaku)
                }
                if (build.modules.library && app.features.library) {
                    Button(id: settings_library, label: tr! { "zh-Hans" => "媒体库", "en" => "Library" }) {
                        Row(gap: 12, align: center) {
                            Stack(variant: setting_icon, width: 28, height: 28, align: center) {
                                Label(title: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, icon: "film.stack", display: icon, font_size: 15)
                            }
                            Text(content: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, font_size: 13, line_height: 18)
                            Spacer
                            Text(content: tr! { "zh-Hans" => "›", "en" => "›" }, font_size: 13, emphasis: secondary)
                        }
                    } => draft.phone_settings.set(PhoneSettings::Library)
                }
                if (build.modules.anime && app.features.anime) {
                    Button(id: settings_anime, label: tr! { "zh-Hans" => "新番", "en" => "New Shows" }) {
                        Row(gap: 12, align: center) {
                            Stack(variant: setting_icon, width: 28, height: 28, align: center) {
                                Label(title: tr! { "zh-Hans" => "新番", "en" => "New Shows" }, icon: "calendar", display: icon, font_size: 15)
                            }
                            Text(content: tr! { "zh-Hans" => "新番", "en" => "New Shows" }, font_size: 13, line_height: 18)
                            Spacer
                            Text(content: tr! { "zh-Hans" => "›", "en" => "›" }, font_size: 13, emphasis: secondary)
                        }
                    } => draft.phone_settings.set(PhoneSettings::Anime)
                }
                if (build.modules.sync && app.features.sync) {
                    Button(id: settings_sync, label: tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" }) {
                        Row(gap: 12, align: center) {
                            Stack(variant: setting_icon, width: 28, height: 28, align: center) {
                                Label(title: tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" }, icon: "arrow.triangle.2.circlepath", display: icon, font_size: 15)
                            }
                            Text(content: tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" }, font_size: 13, line_height: 18)
                            Spacer
                            Text(content: tr! { "zh-Hans" => "›", "en" => "›" }, font_size: 13, emphasis: secondary)
                        }
                    } => draft.phone_settings.set(PhoneSettings::Sync)
                }
                if (build.modules.sources && app.features.sources) {
                    Button(id: settings_sources, label: tr! { "zh-Hans" => "网络媒体源", "en" => "Network Sources" }) {
                        Row(gap: 12, align: center) {
                            Stack(variant: setting_icon, width: 28, height: 28, align: center) {
                                Label(title: tr! { "zh-Hans" => "网络媒体源", "en" => "Network Sources" }, icon: "server.rack", display: icon, font_size: 15)
                            }
                            Text(content: tr! { "zh-Hans" => "网络媒体源", "en" => "Network Sources" }, font_size: 13, line_height: 18)
                            Spacer
                            Text(content: tr! { "zh-Hans" => "›", "en" => "›" }, font_size: 13, emphasis: secondary)
                        }
                    } => draft.phone_settings.set(PhoneSettings::Sources)
                }
                if (build.modules.remote && app.features.remote) {
                    Button(id: settings_remote, label: tr! { "zh-Hans" => "局域网与远程", "en" => "LAN and Remote" }) {
                        Row(gap: 12, align: center) {
                            Stack(variant: setting_icon, width: 28, height: 28, align: center) {
                                Label(title: tr! { "zh-Hans" => "局域网与远程", "en" => "LAN and Remote" }, icon: "wifi", display: icon, font_size: 15)
                            }
                            Text(content: tr! { "zh-Hans" => "局域网与远程", "en" => "LAN and Remote" }, font_size: 13, line_height: 18)
                            Spacer
                            Text(content: tr! { "zh-Hans" => "›", "en" => "›" }, font_size: 13, emphasis: secondary)
                        }
                    } => draft.phone_settings.set(PhoneSettings::Remote)
                }
                if (build.modules.downloads && app.features.downloads) {
                    Button(id: settings_downloads, label: tr! { "zh-Hans" => "下载", "en" => "Downloads" }) {
                        Row(gap: 12, align: center) {
                            Stack(variant: setting_icon, width: 28, height: 28, align: center) {
                                Label(title: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, icon: "arrow.down.circle", display: icon, font_size: 15)
                            }
                            Text(content: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, font_size: 13, line_height: 18)
                            Spacer
                            Text(content: tr! { "zh-Hans" => "›", "en" => "›" }, font_size: 13, emphasis: secondary)
                        }
                    } => draft.phone_settings.set(PhoneSettings::Downloads)
                }
                if (build.modules.enhance && app.features.enhance) {
                    Button(id: settings_enhance, label: tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" }) {
                        Row(gap: 12, align: center) {
                            Stack(variant: setting_icon, width: 28, height: 28, align: center) {
                                Label(title: tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" }, icon: "sparkles", display: icon, font_size: 15)
                            }
                            Text(content: tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" }, font_size: 13, line_height: 18)
                            Spacer
                            Text(content: tr! { "zh-Hans" => "›", "en" => "›" }, font_size: 13, emphasis: secondary)
                        }
                    } => draft.phone_settings.set(PhoneSettings::Enhance)
                }
                Button(id: settings_optional, label: tr! { "zh-Hans" => "可选功能", "en" => "Optional Features" }) {
                    Row(gap: 12, align: center) {
                        Stack(variant: setting_icon, width: 28, height: 28, align: center) {
                            Label(title: tr! { "zh-Hans" => "可选功能", "en" => "Optional Features" }, icon: "square.grid.2x2", display: icon, font_size: 15)
                        }
                        Text(content: tr! { "zh-Hans" => "可选功能", "en" => "Optional Features" }, font_size: 13, line_height: 18)
                        Spacer
                        Text(content: tr! { "zh-Hans" => "›", "en" => "›" }, font_size: 13, emphasis: secondary)
                    }
                } => draft.phone_settings.set(PhoneSettings::Optional)
                Button(id: settings_plugins, label: tr! { "zh-Hans" => "插件", "en" => "Plugins" }) {
                    Row(gap: 12, align: center) {
                        Stack(variant: setting_icon, width: 28, height: 28, align: center) {
                            Label(title: tr! { "zh-Hans" => "插件", "en" => "Plugins" }, icon: "puzzlepiece.extension", display: icon, font_size: 15)
                        }
                        Text(content: tr! { "zh-Hans" => "插件", "en" => "Plugins" }, font_size: 13, line_height: 18)
                        Spacer
                        Text(content: tr! { "zh-Hans" => "›", "en" => "›" }, font_size: 13, emphasis: secondary)
                    }
                } => draft.phone_settings.set(PhoneSettings::Plugins)
                Button(id: settings_keys, label: tr! { "zh-Hans" => "快捷键", "en" => "Keyboard Shortcuts" }) {
                    Row(gap: 12, align: center) {
                        Stack(variant: setting_icon, width: 28, height: 28, align: center) {
                            Label(title: tr! { "zh-Hans" => "快捷键", "en" => "Keyboard Shortcuts" }, icon: "keyboard", display: icon, font_size: 15)
                        }
                        Text(content: tr! { "zh-Hans" => "快捷键", "en" => "Keyboard Shortcuts" }, font_size: 13, line_height: 18)
                        Spacer
                        Text(content: tr! { "zh-Hans" => "›", "en" => "›" }, font_size: 13, emphasis: secondary)
                    }
                } => draft.phone_settings.set(PhoneSettings::Keys)
                Button(id: settings_about, label: tr! { "zh-Hans" => "关于", "en" => "About" }) {
                    Row(gap: 12, align: center) {
                        Stack(variant: setting_icon, width: 28, height: 28, align: center) {
                            Label(title: tr! { "zh-Hans" => "关于", "en" => "About" }, icon: "info.circle", display: icon, font_size: 15)
                        }
                        Text(content: tr! { "zh-Hans" => "关于", "en" => "About" }, font_size: 13, line_height: 18)
                        Spacer
                        Text(content: tr! { "zh-Hans" => "›", "en" => "›" }, font_size: 13, emphasis: secondary)
                    }
                } => draft.phone_settings.set(PhoneSettings::About)
            }
        }
    }
}

/// A settings search result on the phone: it opens its section.
#[component]
pub fn phone_settings_result(
    app: erika::Client,
    result: erika::SettingResult,
    #[session] draft: ErikaDraft,
) -> impl View {
    ui! {
        match result.page {
            SettingsPage::Account => {
                if build.modules.account && app.features.account {
                    Button(id: result_account, label: tr! { "zh-Hans" => "{item} · {page}", "en" => "{item} · {page}"; item: result.item, page: result.page }, prominence: plain) => draft.phone_settings.set(PhoneSettings::Account)
                }
            }
            SettingsPage::General => {
                Button(id: result_general, label: tr! { "zh-Hans" => "{item} · {page}", "en" => "{item} · {page}"; item: result.item, page: result.page }, prominence: plain) => draft.phone_settings.set(PhoneSettings::General)
            }
            SettingsPage::Appearance => {
                Button(id: result_appearance, label: tr! { "zh-Hans" => "{item} · {page}", "en" => "{item} · {page}"; item: result.item, page: result.page }, prominence: plain) => draft.phone_settings.set(PhoneSettings::Appearance)
            }
            SettingsPage::Player => {
                Button(id: result_player, label: tr! { "zh-Hans" => "{item} · {page}", "en" => "{item} · {page}"; item: result.item, page: result.page }, prominence: plain) => draft.phone_settings.set(PhoneSettings::Player)
            }
            #[cfg(feature = "bern-module-danmaku")]
            SettingsPage::Danmaku => {
                Button(id: result_danmaku, label: tr! { "zh-Hans" => "{item} · {page}", "en" => "{item} · {page}"; item: result.item, page: result.page }, prominence: plain) => draft.phone_settings.set(PhoneSettings::Danmaku)
            }
            #[cfg(feature = "bern-module-library")]
            SettingsPage::Library => {
                Button(id: result_library, label: tr! { "zh-Hans" => "{item} · {page}", "en" => "{item} · {page}"; item: result.item, page: result.page }, prominence: plain) => draft.phone_settings.set(PhoneSettings::Library)
            }
            #[cfg(feature = "bern-module-anime")]
            SettingsPage::Anime => {
                Button(id: result_anime, label: tr! { "zh-Hans" => "{item} · {page}", "en" => "{item} · {page}"; item: result.item, page: result.page }, prominence: plain) => draft.phone_settings.set(PhoneSettings::Anime)
            }
            #[cfg(feature = "bern-module-sync")]
            SettingsPage::Sync => {
                Button(id: result_sync, label: tr! { "zh-Hans" => "{item} · {page}", "en" => "{item} · {page}"; item: result.item, page: result.page }, prominence: plain) => draft.phone_settings.set(PhoneSettings::Sync)
            }
            #[cfg(feature = "bern-module-sources")]
            SettingsPage::Sources => {
                Button(id: result_sources, label: tr! { "zh-Hans" => "{item} · {page}", "en" => "{item} · {page}"; item: result.item, page: result.page }, prominence: plain) => draft.phone_settings.set(PhoneSettings::Sources)
            }
            #[cfg(feature = "bern-module-remote")]
            SettingsPage::Remote => {
                Button(id: result_remote, label: tr! { "zh-Hans" => "{item} · {page}", "en" => "{item} · {page}"; item: result.item, page: result.page }, prominence: plain) => draft.phone_settings.set(PhoneSettings::Remote)
            }
            #[cfg(feature = "bern-module-downloads")]
            SettingsPage::Downloads => {
                Button(id: result_downloads, label: tr! { "zh-Hans" => "{item} · {page}", "en" => "{item} · {page}"; item: result.item, page: result.page }, prominence: plain) => draft.phone_settings.set(PhoneSettings::Downloads)
            }
            #[cfg(feature = "bern-module-enhance")]
            SettingsPage::Enhance => {
                Button(id: result_enhance, label: tr! { "zh-Hans" => "{item} · {page}", "en" => "{item} · {page}"; item: result.item, page: result.page }, prominence: plain) => draft.phone_settings.set(PhoneSettings::Enhance)
            }
            SettingsPage::Optional => {
                Button(id: result_optional, label: tr! { "zh-Hans" => "{item} · {page}", "en" => "{item} · {page}"; item: result.item, page: result.page }, prominence: plain) => draft.phone_settings.set(PhoneSettings::Optional)
            }
            SettingsPage::Plugins => {
                Button(id: result_plugins, label: tr! { "zh-Hans" => "{item} · {page}", "en" => "{item} · {page}"; item: result.item, page: result.page }, prominence: plain) => draft.phone_settings.set(PhoneSettings::Plugins)
            }
            SettingsPage::Keys => {
                Button(id: result_keys, label: tr! { "zh-Hans" => "{item} · {page}", "en" => "{item} · {page}"; item: result.item, page: result.page }, prominence: plain) => draft.phone_settings.set(PhoneSettings::Keys)
            }
            SettingsPage::About => {
                Button(id: result_about, label: tr! { "zh-Hans" => "{item} · {page}", "en" => "{item} · {page}"; item: result.item, page: result.page }, prominence: plain) => draft.phone_settings.set(PhoneSettings::About)
            }
            #[cfg(any(not(feature = "bern-module-danmaku"), not(feature = "bern-module-enhance"), not(feature = "bern-module-anime"), not(feature = "bern-module-library"), not(feature = "bern-module-sync"), not(feature = "bern-module-sources"), not(feature = "bern-module-remote"), not(feature = "bern-module-downloads")))]
            _ => {}
        }
    }
}

/// One section of the phone's settings, pushed over their list: the
/// desktop's form of it, which holds the way back and its title at its top.
#[component]
pub fn phone_settings_section(
    app: erika::Client,
    #[cfg(feature = "bern-module-anime")] anime: anime::Client,
    #[session] draft: ErikaDraft,
) -> impl View {
    ui! {
        match draft.phone_settings {
            PhoneSettings::Account => {
                if build.modules.account && app.features.account {
                    settings_account(app: app, settings_window: false)
                } else {
                    settings_general(app: app)
                }
            }
            PhoneSettings::General => { settings_general(app: app) }
            PhoneSettings::Appearance => { settings_appearance(app: app) }
            PhoneSettings::Player => { settings_player(app: app) }
            #[cfg(feature = "bern-module-danmaku")]
            PhoneSettings::Danmaku => { settings_danmaku(app: app) }
            #[cfg(feature = "bern-module-library")]
            PhoneSettings::Library => { settings_library(app: app) }
            #[cfg(feature = "bern-module-anime")]
            PhoneSettings::Anime => { settings_anime(anime: anime) }
            #[cfg(feature = "bern-module-sync")]
            PhoneSettings::Sync => { Slot(id: settings_sync) }
            #[cfg(feature = "bern-module-sources")]
            PhoneSettings::Sources => { settings_module_placeholder(app: app) }
            #[cfg(feature = "bern-module-remote")]
            PhoneSettings::Remote => { settings_module_placeholder(app: app) }
            #[cfg(feature = "bern-module-downloads")]
            PhoneSettings::Downloads => { settings_module_placeholder(app: app) }
            #[cfg(feature = "bern-module-enhance")]
            PhoneSettings::Enhance => { settings_module_placeholder(app: app) }
            PhoneSettings::Optional => { settings_optional(app: app) }
            PhoneSettings::Plugins => { settings_plugins(app: app) }
            PhoneSettings::Keys => { settings_keys(app: app) }
            PhoneSettings::About => { settings_about(app: app) }
            _ => {}
        }
    }
}

/// The way back to the settings and the section's large title, at the top
/// of a settings form on the phone; the desktop's window shows its own.
#[component]
pub fn phone_form_header(app: erika::Client, #[session] draft: ErikaDraft) -> impl View {
    ui! {
        if window.compact {
            Section(style: plain) {
                phone_section_title(app: app)
            }
        }
    }
}

/// The same over a settings page that scrolls without a form.
#[component]
pub fn phone_scroll_header(app: erika::Client, #[session] draft: ErikaDraft) -> impl View {
    ui! {
        if window.compact {
            Column(gap: 0, padding_x: 16) {
                phone_section_title(app: app)
            }
        }
    }
}

#[component]
pub fn phone_section_title(app: erika::Client, #[session] draft: ErikaDraft) -> impl View {
    ui! {
        Column(gap: 0) {
            Canvas(height: 7)
            Row(gap: 0, height: 48, align: center) {
                IconButton(id: section_back, icon: "chevron.left", font_size: 18, height: 44, label: tr! { "zh-Hans" => "设置", "en" => "Settings" }) => draft.phone_settings.set(PhoneSettings::List)
                Spacer
            }
            // `.x-lt` 4 points up into the row above.
            Row(gap: 0, height: 32, align: bottom) {
                if draft.phone_settings == PhoneSettings::Account && !app.features.account {
                    Text(content: tr! { "zh-Hans" => "通用", "en" => "General" }, style: large_title, line_height: 36)
                } else {
                    Text(content: draft.phone_settings, style: large_title, line_height: 36)
                }
            }
        }
    }
}

/// `.ph-acct .x-av`: the account's picture at 40 points.
#[component]
pub fn phone_account_picture(app: erika::Client) -> impl View {
    ui! {
        Stack(variant: avatar, width: 40, height: 40) {
            Canvas(width: 40, height: 40, label: tr! { "zh-Hans" => "头像", "en" => "Avatar" }) {
                Shape(kind: ellipse, fill: fill.strong)
                Shape(kind: ellipse, x: 14.07, y: 10.07, width: 11.86, height: 11.86, fill: text.secondary, opacity: 0.5)
                Shape(kind: ellipse, x: -4, y: 21.6, width: 48, height: 36.8, fill: text.secondary, opacity: 0.5)
            }
            if let Some(picture) = app.user_avatar {
                Image(source: picture, width: 40, height: 40, corner: 20, fit: cover, label: app.user_name)
            }
        }
    }
}
