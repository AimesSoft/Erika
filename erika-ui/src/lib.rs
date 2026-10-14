//! The interface of Erika's shell: the window, the sidebar of the modules'
//! pages with the mini player and the account at its foot, the toolbar, and
//! the player, a layer above every page. The picture is one frame: it fills
//! the window while playing, steps back when the pointer moves, and shrinks
//! into the mini player on the way back.

use bern::prelude::*;
use erika_anime_protocol::anime;
use erika_protocol::erika::{self, Accent, Appearance};

style! {
    accents {
        crimson: "#FF2E55",
        ultramarine: "#2A51D4",
        neon: color(light: "#00B86B", dark: "#07FF94"),
    }

    // The type of the design (section 8.2 and the desktop prototype):
    // the system font at the prototype's sizes and weights.
    text {
        large_title: font(size: 30, weight: bold),
        title: font(size: 34, weight: bold),
        title2: font(size: 28, weight: bold),
        title3: font(size: 19, weight: bold),
        headline: font(size: 14, weight: semibold),
        callout: font(size: 16, weight: semibold),
        caption: font(size: 11.5, weight: medium),
        caption2: font(size: 12, weight: semibold),
    }

    // The springs of the design, section 9.2.
    motion {
        press: spring(period: 0.12, damping: 1.0),
        jelly: spring(period: 0.42, damping: 0.42),
        bouncy: spring(period: 0.50, damping: 0.62),
        hero: spring(period: 0.48, damping: 0.86),
        smooth: spring(period: 0.45, damping: 1.0),
        snappy: spring(period: 0.22, damping: 0.85),
    }

    transition {
        // A new page fades in and rises 12 points (T3).
        page_in: slide(bottom, 12, smooth),
        drop_in: slide(top, 24, jelly),
        pop_in: pop(from: 0.4, bouncy),
    }

    // Controls keep the system's look (P2); only the design's springs are
    // added (section 9.4). The one main button of a screen also has
    // `prominence: prominent`.
    Button.primary {
        size: large,
        hover: scale(1.03, snappy),
        pressed: scale(x: 0.97, y: 0.9, press),
        settle: jelly,
    }

    Button {
        hover: scale(1.03, snappy),
        pressed: scale(0.94, press),
        settle: jelly,
    }

    // What the design draws itself: the frame, posters and the film.
    // The stage of the home page and the mini player: the frame.
    Button.stage {
        corner: 22,
        pressed: scale(0.98, press),
        settle: jelly,
    }

    Button.poster {
        corner: 10,
        hover: scale(1.03, snappy) + shadow(14, y: 6, opacity: 0.18),
        pressed: scale(0.97, press),
        settle: jelly,
    }

    // A cell of the film.
    Button.film {
        corner: 16,
        pressed: scale(0.97, press),
        settle: jelly,
    }

    // A row of a list that is a button as a whole.
    Button.row {
        pressed: scale(0.98, press),
        settle: jelly,
    }

    IconButton {
        hover: scale(1.08, snappy),
        pressed: scale(0.84, press),
        settle: jelly,
    }
}

/// Pages of the main window. The modules add theirs; with none, the window
/// is the empty frame.
#[bern::value(pages = "main")]
pub enum MainPage {
    #[label(tr! { "zh-Hans" => "播放器", "en" => "Player" })]
    Idle,
}

/// Drafts of the main window.
#[bern::session]
pub struct ErikaDraft {
    /// The home page first; the empty frame when no module adds pages.
    #[initial(MainPage::Home)]
    pub page: MainPage,
    pub query: String,
    /// The video picked or dropped last.
    pub file: MediaSource,
}

#[component]
pub fn app(app: erika::Client, anime: anime::Client, windows: AppWindows) -> impl View {
    ui! {
        WindowSet(handle: AppWindows, primary: main, accent: app.accent, appearance: app.appearance) {
            Window(id: main, title: tr! { "zh-Hans" => "Erika", "en" => "Erika" }, initial: open, width: 1280, height: 800, min_width: 720, min_height: 480, chrome: unified, idle_after: 3, immersive: app.playing && app.expanded) {
                main_window(app: app, anime: anime, windows: windows)
            }
            Window(id: settings, title: tr! { "zh-Hans" => "设置", "en" => "Settings" }, width: 640, height: 480) {
                settings(app: app)
            }
        }
    }
}

/// The sidebar and the page, with the player above both while it plays.
#[component]
pub fn main_window(
    app: erika::Client,
    anime: anime::Client,
    windows: AppWindows,
    #[session] draft: ErikaDraft,
) -> impl View {
    ui! {
        Stack {
            SplitView {
                Sidebar(id: pages, selection: draft.page) {
                    SidebarHeader {
                        Row(gap: 10, align: center) {
                            Image(source: "design/assets/icon.png", width: 22, height: 22, label: tr! { "zh-Hans" => "Erika", "en" => "Erika" })
                            Text(content: tr! { "zh-Hans" => "Erika", "en" => "Erika" }, style: headline)
                        }
                    }
                    Pages(slot: main)
                    SidebarFooter {
                        if app.playing && !app.expanded {
                            mini_player(app: app)
                        }
                        Row(gap: 10, align: center) {
                            avatar()
                            Column(gap: 1) {
                                Text(content: app.user_name, style: headline)
                                Text(content: tr! { "zh-Hans" => "本地用户", "en" => "Local user" }, style: caption, emphasis: secondary)
                            }
                            Spacer
                            IconButton(id: settings, icon: "gearshape", label: tr! { "zh-Hans" => "设置", "en" => "Settings" }, prominence: plain, shortcut: "cmd+,") => windows.settings.open()
                        }
                    }
                }
                Detail(titled: false) {
                    Toolbar {
                        // The way back from a show's details, at the leading
                        // end with the page it leads to.
                        if anime.opened > 0 {
                            IconButton(id: back, icon: "chevron.left", label: tr! { "zh-Hans" => "返回", "en" => "Back" }, prominence: plain, shortcut: "escape") => anime.close()
                            Text(content: anime.back_label, emphasis: secondary)
                        }
                        Spacer
                        Search(id: search, value: draft.query, prompt: tr! { "zh-Hans" => "搜索番剧、文件、设置", "en" => "Search shows, files and settings" }, shortcut: "cmd+k") {
                            Text(content: tr! { "zh-Hans" => "搜索在下一步做", "en" => "Search comes next" }, emphasis: secondary)
                        }
                        Button(id: open_file, label: tr! { "zh-Hans" => "打开", "en" => "Open" }, icon: "plus", pick: draft.file, files: "video", shortcut: "cmd+o") => app.open_file(draft.file)
                    }
                    match draft.page {
                        MainPage::Idle => { empty_frame(app: app) }
                        _ => { PageContent(slot: main, selected: draft.page) }
                    }
                }
            }
            if app.playing && app.expanded {
                player(app: app)
            }
        }
    }
}

/// The account's picture; a person on a grey disc until the account has
/// one of its own.
#[component]
pub fn avatar() -> impl View {
    ui! {
        Canvas(width: 30, height: 30, label: tr! { "zh-Hans" => "头像", "en" => "Avatar" }) {
            Shape(kind: ellipse, fill: fill.strong)
            Shape(kind: ellipse, x: 9.5, y: 6.5, width: 11, height: 11, fill: text.secondary, opacity: 0.6)
            Shape(kind: ellipse, x: 6, y: 20, width: 18, height: 14, fill: text.secondary, opacity: 0.6)
        }
    }
}

/// The frame shrunk into the foot of the sidebar: its bottom edge is the
/// progress. Clicking it grows the frame back.
#[component]
pub fn mini_player(app: erika::Client) -> impl View {
    ui! {
        Column(gap: 8, appear: fade, disappear: fade) {
            Button(id: mini, label: app.title, variant: stage) {
                Stack(align: bottom_leading) {
                    Image(source: app.still, width: 208, height: 117, fit: cover, label: app.title, hero: frame)
                    Rail {
                        Canvas(from: 0, to: app.progress, height: 3) {
                            Shape(kind: rect, fill: accent)
                        }
                    }
                }
            } => app.expand()
            Row(gap: 4, align: center) {
                Column(gap: 2) {
                    Text(content: app.title, style: footnote)
                    Text(content: tr! { "zh-Hans" => "{subtitle} · {elapsed}", "en" => "{subtitle} · {elapsed}"; subtitle: app.subtitle, elapsed: app.elapsed }, style: caption, emphasis: secondary)
                }
                Spacer
                if app.paused {
                    IconButton(id: mini_play, icon: "play.fill", label: tr! { "zh-Hans" => "播放", "en" => "Play" }) => app.toggle_pause()
                } else {
                    IconButton(id: mini_pause, icon: "pause.fill", label: tr! { "zh-Hans" => "暂停", "en" => "Pause" }) => app.toggle_pause()
                }
                IconButton(id: mini_close, icon: "xmark", label: tr! { "zh-Hans" => "关闭", "en" => "Close" }) => app.close_player()
            }
        }
    }
}

/// The player: always dark. Playing with the pointer at rest, the frame
/// fills the window; once the pointer moves it steps back and the controls
/// come in around it.
#[component]
pub fn player(app: erika::Client) -> impl View {
    ui! {
        Stack(appearance: dark, align: top) {
            Canvas(label: tr! { "zh-Hans" => "播放器", "en" => "Player" }) {
                Shape(kind: rect, fill: surface.canvas)
            }
            Column(gap: 12, padding: if window.idle { 0 } else { 24 }, animate: hero, align: center) {
                if !window.idle {
                    Row(gap: 8, align: center, appear: fade, disappear: fade) {
                        IconButton(id: back, icon: "chevron.left", label: tr! { "zh-Hans" => "返回", "en" => "Back" }, shortcut: "escape") => app.minimize()
                        Column(gap: 2) {
                            Text(content: app.title, style: headline)
                            Text(content: app.subtitle, style: caption, emphasis: secondary)
                        }
                        Spacer
                        IconButton(id: pip, icon: "pip.enter", label: tr! { "zh-Hans" => "画中画", "en" => "Picture in Picture" }) => app.minimize()
                        IconButton(id: player_settings, icon: "slider.horizontal.3", label: tr! { "zh-Hans" => "播放设置", "en" => "Playback Settings" }) => app.minimize()
                    }
                }
                Image(source: app.still, width: if window.idle { 1280 } else { 1074 }, height: if window.idle { 720 } else { 604 }, fit: cover, label: app.title, hero: frame, animate: hero)
                if !window.idle {
                    Column(gap: 12, appear: fade, disappear: fade) {
                        Row(gap: 12, align: center) {
                            Text(content: app.elapsed, style: caption)
                            Progress(value: app.progress)
                            Text(content: app.duration, style: caption)
                        }
                        Row(gap: 12, align: center) {
                            Spacer
                            IconButton(id: back10, icon: "gobackward.10", label: tr! { "zh-Hans" => "快退 10 秒", "en" => "Back 10 Seconds" }) => app.toggle_pause()
                            if app.paused {
                                IconButton(id: play, icon: "play.fill", label: tr! { "zh-Hans" => "播放", "en" => "Play" }, shortcut: "space") => app.toggle_pause()
                            } else {
                                IconButton(id: pause, icon: "pause.fill", label: tr! { "zh-Hans" => "暂停", "en" => "Pause" }, shortcut: "space") => app.toggle_pause()
                            }
                            IconButton(id: forward10, icon: "goforward.10", label: tr! { "zh-Hans" => "快进 10 秒", "en" => "Forward 10 Seconds" }) => app.toggle_pause()
                            Spacer
                        }
                    }
                }
            }
        }
    }
}

/// The window with no pages: the frame before a picture is in it, where
/// the player's frame steps back to, with "open" at its middle.
#[component]
pub fn empty_frame(app: erika::Client, #[session] draft: ErikaDraft) -> impl View {
    ui! {
        Column(gap: 24, padding: 48, align: center, appear: page_in) {
            Stack(align: center) {
                Canvas(height: 420, label: tr! { "zh-Hans" => "空画框", "en" => "Empty frame" }) {
                    Shape(kind: rounded, corner: 22, stroke: control.border, stroke_width: 2)
                }
                Button(id: open_here, label: tr! { "zh-Hans" => "打开文件", "en" => "Open File" }, icon: "folder", prominence: prominent, variant: primary, pick: draft.file, files: "video") => app.open_file(draft.file)
            }
        }
    }
}

/// Settings. The sections come with their modules in a later step.
#[component]
pub fn settings(app: erika::Client) -> impl View {
    ui! {
        Form {
            Section(title: tr! { "zh-Hans" => "外观", "en" => "Appearance" }) {
                Menu(id: accent_menu, label: tr! { "zh-Hans" => "主题色", "en" => "Accent" }) {
                    MenuItem(id: crimson, label: tr! { "zh-Hans" => "绯红", "en" => "Crimson" }) => app.set_accent(Accent::Crimson)
                    MenuItem(id: ultramarine, label: tr! { "zh-Hans" => "群青", "en" => "Ultramarine" }) => app.set_accent(Accent::Ultramarine)
                    MenuItem(id: neon, label: tr! { "zh-Hans" => "荧绿", "en" => "Neon" }) => app.set_accent(Accent::Neon)
                }
                Menu(id: appearance_menu, label: tr! { "zh-Hans" => "明暗", "en" => "Appearance" }) {
                    MenuItem(id: follow_system, label: tr! { "zh-Hans" => "跟随系统", "en" => "System" }) => app.set_appearance(Appearance::System)
                    MenuItem(id: light, label: tr! { "zh-Hans" => "浅色", "en" => "Light" }) => app.set_appearance(Appearance::Light)
                    MenuItem(id: dark, label: tr! { "zh-Hans" => "深色", "en" => "Dark" }) => app.set_appearance(Appearance::Dark)
                }
            }
        }
    }
}
