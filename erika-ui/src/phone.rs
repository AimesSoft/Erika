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
use erika_protocol::erika::{self, SettingResult, SettingsPage};

/// The phone's settings (`phStack`): closed or open over the current tab,
/// and the sections their navigation stack pushes over the list.
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
                phone_settings(app: app, #[cfg(feature = "bern-module-anime")] anime: anime)
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

/// The phone's settings (`phHTML('settings')`): the account and the
/// sections in a navigation stack, each section pushed over the list; the
/// way out and the search in the platform's bars, the search at the bottom
/// of the screen on iOS 26.
#[component]
pub fn phone_settings(
    app: erika::Client,
    #[cfg(feature = "bern-module-anime")] anime: anime::Client,
    #[session] draft: ErikaDraft,
    #[session] settings: SettingsDraft,
) -> impl View {
    ui! {
        Navigation(path: draft.settings_path) {
            // Each page stands on the phone's page: the navigation stack
            // would show its own white behind the rows.
            Stack(variant: phone_page, align: top_leading) {
                // The system's own settings: groups the platform's distance
                // apart, rows 52 tall with 30-point icons and 17-point words.
                // Each section's icon is white on a tile of its own color on
                // iOS, and bare on the other systems, as Android shows them.
                Form(title: tr! { "zh-Hans" => "设置", "en" => "Settings" }, appear: page_in, row_height: 52) {
                    // The account, its picture 60 points across; it opens the
                    // account's form.
                    if build.modules.account && app.features.account {
                    Section(padding_x: 14, padding_y: 11) {
                        NavLink(id: settings_account, route: PhoneSettings::Account) {
                            Row(gap: 14, align: center) {
                                phone_account_picture(app: app)
                                Column(gap: 2) {
                                    Text(content: app.user_name, style: title3, lines: 1)
                                    Text(content: app.user_account, font_size: 15, line_height: 20, emphasis: secondary, lines: 1)
                                }
                            }
                        }
                    }
                    }
                    Section(padding_x: 14, padding_y: 11) {
                        NavLink(id: settings_general, route: PhoneSettings::General) {
                            Row(gap: 14, align: center) {
                                Stack(variant: settings_general, width: 30, height: 30, align: center) {
                                    Label(title: tr! { "zh-Hans" => "通用", "en" => "General" }, icon: "gearshape", display: icon, font_size: 20)
                                }
                                Text(content: tr! { "zh-Hans" => "通用", "en" => "General" }, font_size: 17, line_height: 22)
                            }
                        }
                        NavLink(id: settings_appearance, route: PhoneSettings::Appearance) {
                            Row(gap: 14, align: center) {
                                Stack(variant: settings_appearance, width: 30, height: 30, align: center) {
                                    Label(title: tr! { "zh-Hans" => "外观", "en" => "Appearance" }, icon: "paintpalette", display: icon, font_size: 20)
                                }
                                Text(content: tr! { "zh-Hans" => "外观", "en" => "Appearance" }, font_size: 17, line_height: 22)
                            }
                        }
                        NavLink(id: settings_player, route: PhoneSettings::Player) {
                            Row(gap: 14, align: center) {
                                Stack(variant: settings_player, width: 30, height: 30, align: center) {
                                    Label(title: tr! { "zh-Hans" => "播放", "en" => "Playback" }, icon: "play", display: icon, font_size: 20)
                                }
                                Text(content: tr! { "zh-Hans" => "播放", "en" => "Playback" }, font_size: 17, line_height: 22)
                            }
                        }
                        if (build.modules.danmaku && app.features.danmaku) {
                            NavLink(id: settings_danmaku, route: PhoneSettings::Danmaku) {
                                Row(gap: 14, align: center) {
                                    Stack(variant: settings_danmaku, width: 30, height: 30, align: center) {
                                        Label(title: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, icon: "text.bubble", display: icon, font_size: 20)
                                    }
                                    Text(content: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, font_size: 17, line_height: 22)
                                }
                            }
                        }
                        if (build.modules.library && app.features.library) {
                            NavLink(id: settings_library, route: PhoneSettings::Library) {
                                Row(gap: 14, align: center) {
                                    Stack(variant: settings_library, width: 30, height: 30, align: center) {
                                        Label(title: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, icon: "film.stack", display: icon, font_size: 20)
                                    }
                                    Text(content: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, font_size: 17, line_height: 22)
                                }
                            }
                        }
                        if (build.modules.anime && app.features.anime) {
                            NavLink(id: settings_anime, route: PhoneSettings::Anime) {
                                Row(gap: 14, align: center) {
                                    Stack(variant: settings_anime, width: 30, height: 30, align: center) {
                                        Label(title: tr! { "zh-Hans" => "新番", "en" => "New Shows" }, icon: "calendar", display: icon, font_size: 20)
                                    }
                                    Text(content: tr! { "zh-Hans" => "新番", "en" => "New Shows" }, font_size: 17, line_height: 22)
                                }
                            }
                        }
                        if (build.modules.sync && app.features.sync) {
                            NavLink(id: settings_sync, route: PhoneSettings::Sync) {
                                Row(gap: 14, align: center) {
                                    Stack(variant: settings_sync, width: 30, height: 30, align: center) {
                                        Label(title: tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" }, icon: "arrow.triangle.2.circlepath", display: icon, font_size: 20)
                                    }
                                    Text(content: tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" }, font_size: 17, line_height: 22)
                                }
                            }
                        }
                        if (build.modules.sources && app.features.sources) {
                            NavLink(id: settings_sources, route: PhoneSettings::Sources) {
                                Row(gap: 14, align: center) {
                                    Stack(variant: settings_sources, width: 30, height: 30, align: center) {
                                        Label(title: tr! { "zh-Hans" => "网络媒体源", "en" => "Network Sources" }, icon: "server.rack", display: icon, font_size: 20)
                                    }
                                    Text(content: tr! { "zh-Hans" => "网络媒体源", "en" => "Network Sources" }, font_size: 17, line_height: 22)
                                }
                            }
                        }
                        if (build.modules.remote && app.features.remote) {
                            NavLink(id: settings_remote, route: PhoneSettings::Remote) {
                                Row(gap: 14, align: center) {
                                    Stack(variant: settings_remote, width: 30, height: 30, align: center) {
                                        Label(title: tr! { "zh-Hans" => "局域网与远程", "en" => "LAN and Remote" }, icon: "wifi", display: icon, font_size: 20)
                                    }
                                    Text(content: tr! { "zh-Hans" => "局域网与远程", "en" => "LAN and Remote" }, font_size: 17, line_height: 22)
                                }
                            }
                        }
                        if (build.modules.downloads && app.features.downloads) {
                            NavLink(id: settings_downloads, route: PhoneSettings::Downloads) {
                                Row(gap: 14, align: center) {
                                    Stack(variant: settings_downloads, width: 30, height: 30, align: center) {
                                        Label(title: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, icon: "arrow.down.circle", display: icon, font_size: 20)
                                    }
                                    Text(content: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, font_size: 17, line_height: 22)
                                }
                            }
                        }
                        if (build.modules.enhance && app.features.enhance) {
                            NavLink(id: settings_enhance, route: PhoneSettings::Enhance) {
                                Row(gap: 14, align: center) {
                                    Stack(variant: settings_enhance, width: 30, height: 30, align: center) {
                                        Label(title: tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" }, icon: "sparkles", display: icon, font_size: 20)
                                    }
                                    Text(content: tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" }, font_size: 17, line_height: 22)
                                }
                            }
                        }
                        NavLink(id: settings_optional, route: PhoneSettings::Optional) {
                            Row(gap: 14, align: center) {
                                Stack(variant: settings_optional, width: 30, height: 30, align: center) {
                                    Label(title: tr! { "zh-Hans" => "可选功能", "en" => "Optional Features" }, icon: "square.grid.2x2", display: icon, font_size: 20)
                                }
                                Text(content: tr! { "zh-Hans" => "可选功能", "en" => "Optional Features" }, font_size: 17, line_height: 22)
                            }
                        }
                        NavLink(id: settings_plugins, route: PhoneSettings::Plugins) {
                            Row(gap: 14, align: center) {
                                Stack(variant: settings_plugins, width: 30, height: 30, align: center) {
                                    Label(title: tr! { "zh-Hans" => "插件", "en" => "Plugins" }, icon: "puzzlepiece.extension", display: icon, font_size: 20)
                                }
                                Text(content: tr! { "zh-Hans" => "插件", "en" => "Plugins" }, font_size: 17, line_height: 22)
                            }
                        }
                        NavLink(id: settings_keys, route: PhoneSettings::Keys) {
                            Row(gap: 14, align: center) {
                                Stack(variant: settings_keys, width: 30, height: 30, align: center) {
                                    Label(title: tr! { "zh-Hans" => "快捷键", "en" => "Keyboard Shortcuts" }, icon: "keyboard", display: icon, font_size: 20)
                                }
                                Text(content: tr! { "zh-Hans" => "快捷键", "en" => "Keyboard Shortcuts" }, font_size: 17, line_height: 22)
                            }
                        }
                        NavLink(id: settings_about, route: PhoneSettings::About) {
                            Row(gap: 14, align: center) {
                                Stack(variant: settings_about, width: 30, height: 30, align: center) {
                                    Label(title: tr! { "zh-Hans" => "关于", "en" => "About" }, icon: "info.circle", display: icon, font_size: 20)
                                }
                                Text(content: tr! { "zh-Hans" => "关于", "en" => "About" }, font_size: 17, line_height: 22)
                            }
                        }
                    }
                }
            }
            // The way out of the settings and the search, in the platform's
            // bars: iOS 26 puts the search at the bottom of the screen.
            Toolbar {
                IconButton(id: settings_back, icon: "chevron.left", label: tr! { "zh-Hans" => "返回", "en" => "Back" }) => draft.phone_settings.set(PhoneSettings::Closed)
                Spacer
                Search(id: settings_search_phone, value: settings.query, prompt: tr! { "zh-Hans" => "搜索设置", "en" => "Search settings" }) {
                    // Nothing is searched before something is typed.
                    if "" != settings.query {
                        Query(id: settings_results_phone, query: app.find_settings(settings.query), empty: tr! { "zh-Hans" => "没有匹配的设置", "en" => "No matching settings" }) |results| {
                            // A result opens its section, pushed as a row of the list is.
                            ForEach(items: results, key: id) |result| {
                                match result.page {
                                    SettingsPage::Account => {
                                        if build.modules.account && app.features.account {
                                            NavLink(id: result_account, route: PhoneSettings::Account) { phone_settings_hit(result: result) }
                                        }
                                    }
                                    SettingsPage::General => {
                                        NavLink(id: result_general, route: PhoneSettings::General) { phone_settings_hit(result: result) }
                                    }
                                    SettingsPage::Appearance => {
                                        NavLink(id: result_appearance, route: PhoneSettings::Appearance) { phone_settings_hit(result: result) }
                                    }
                                    SettingsPage::Player => {
                                        NavLink(id: result_player, route: PhoneSettings::Player) { phone_settings_hit(result: result) }
                                    }
                                    #[cfg(feature = "bern-module-danmaku")]
                                    SettingsPage::Danmaku => {
                                        NavLink(id: result_danmaku, route: PhoneSettings::Danmaku) { phone_settings_hit(result: result) }
                                    }
                                    #[cfg(feature = "bern-module-library")]
                                    SettingsPage::Library => {
                                        NavLink(id: result_library, route: PhoneSettings::Library) { phone_settings_hit(result: result) }
                                    }
                                    #[cfg(feature = "bern-module-anime")]
                                    SettingsPage::Anime => {
                                        NavLink(id: result_anime, route: PhoneSettings::Anime) { phone_settings_hit(result: result) }
                                    }
                                    #[cfg(feature = "bern-module-sync")]
                                    SettingsPage::Sync => {
                                        NavLink(id: result_sync, route: PhoneSettings::Sync) { phone_settings_hit(result: result) }
                                    }
                                    #[cfg(feature = "bern-module-sources")]
                                    SettingsPage::Sources => {
                                        NavLink(id: result_sources, route: PhoneSettings::Sources) { phone_settings_hit(result: result) }
                                    }
                                    #[cfg(feature = "bern-module-remote")]
                                    SettingsPage::Remote => {
                                        NavLink(id: result_remote, route: PhoneSettings::Remote) { phone_settings_hit(result: result) }
                                    }
                                    #[cfg(feature = "bern-module-downloads")]
                                    SettingsPage::Downloads => {
                                        NavLink(id: result_downloads, route: PhoneSettings::Downloads) { phone_settings_hit(result: result) }
                                    }
                                    #[cfg(feature = "bern-module-enhance")]
                                    SettingsPage::Enhance => {
                                        NavLink(id: result_enhance, route: PhoneSettings::Enhance) { phone_settings_hit(result: result) }
                                    }
                                    SettingsPage::Optional => {
                                        NavLink(id: result_optional, route: PhoneSettings::Optional) { phone_settings_hit(result: result) }
                                    }
                                    SettingsPage::Plugins => {
                                        NavLink(id: result_plugins, route: PhoneSettings::Plugins) { phone_settings_hit(result: result) }
                                    }
                                    SettingsPage::Keys => {
                                        NavLink(id: result_keys, route: PhoneSettings::Keys) { phone_settings_hit(result: result) }
                                    }
                                    SettingsPage::About => {
                                        NavLink(id: result_about, route: PhoneSettings::About) { phone_settings_hit(result: result) }
                                    }
                                    #[cfg(any(not(feature = "bern-module-danmaku"), not(feature = "bern-module-enhance"), not(feature = "bern-module-anime"), not(feature = "bern-module-library"), not(feature = "bern-module-sync"), not(feature = "bern-module-sources"), not(feature = "bern-module-remote"), not(feature = "bern-module-downloads")))]
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
            Destination |section| {
                Stack(variant: phone_page, align: top_leading) {
                    phone_settings_section(app: app, #[cfg(feature = "bern-module-anime")] anime: anime, section: section)
                }
            }
        }
    }
}

/// One section of the phone's settings, pushed over their list: the
/// desktop's form of it, its title in the platform's bar.
#[component]
pub fn phone_settings_section(
    app: erika::Client,
    #[cfg(feature = "bern-module-anime")] anime: anime::Client,
    section: PhoneSettings,
) -> impl View {
    ui! {
        match section {
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

/// The large title over the plugins page, which scrolls without a form and
/// so has no title for the platform's bar.
#[component]
pub fn phone_scroll_header() -> impl View {
    ui! {
        if window.compact {
            Row(gap: 0, padding_x: 16, height: 44, align: bottom) {
                Text(content: tr! { "zh-Hans" => "插件", "en" => "Plugins" }, style: large_title, line_height: 36)
            }
        }
    }
}

/// A setting the search found: its name over the page it stands on, as
/// the system's settings show what they find.
#[component]
pub fn phone_settings_hit(result: SettingResult) -> impl View {
    ui! {
        Column(gap: 2) {
            Text(content: tr! { "zh-Hans" => "{item}", "en" => "{item}"; item: result.item }, font_size: 17, line_height: 22, lines: 1)
            Text(content: tr! { "zh-Hans" => "{page}", "en" => "{page}"; page: result.page }, font_size: 13, line_height: 18, emphasis: secondary, lines: 1)
        }
    }
}

/// The account's picture at 60 points, as the system's settings show theirs.
#[component]
pub fn phone_account_picture(app: erika::Client) -> impl View {
    ui! {
        Stack(variant: avatar, width: 60, height: 60) {
            Canvas(width: 60, height: 60, label: tr! { "zh-Hans" => "头像", "en" => "Avatar" }) {
                Shape(kind: ellipse, fill: fill.strong)
                Shape(kind: ellipse, x: 21.1, y: 15.1, width: 17.8, height: 17.8, fill: text.secondary, opacity: 0.5)
                Shape(kind: ellipse, x: -6, y: 32.4, width: 72, height: 55.2, fill: text.secondary, opacity: 0.5)
            }
            if let Some(picture) = app.user_avatar {
                Image(source: picture, width: 60, height: 60, corner: 30, fit: cover, label: app.user_name)
            }
        }
    }
}
