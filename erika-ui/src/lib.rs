//! The interface of Erika's shell: the window, the sidebar of the modules'
//! pages with the mini player and the account at its foot, the toolbar, and
//! the player, a layer above every page. The picture is one frame: it fills
//! the window while playing, steps back when the pointer moves, and shrinks
//! into the mini player on the way back.

mod player;
use player::player;
mod search;
use search::search_results;

use bern::prelude::*;
use erika_anime_protocol::anime;
use erika_protocol::erika;
mod onboarding;
use onboarding::onboarding;
mod feature_preview;
mod settings;
use settings::settings;

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
        caption2: font(size: 12.5, weight: semibold),
        subheadline: font(size: 8.5, weight: semibold),
    }

    Stack.player_black { background: color(light: "#000000", dark: "#000000"), foreground: color(light: "#FFFFFF", dark: "#FFFFFF") }
    Stack.player_background { background: color(light: "#1C1C1E", dark: "#1C1C1E"), foreground: color(light: "#FFFFFF", dark: "#FFFFFF") }
    Stack.player_panel { background: color(light: "#2C2C2F", dark: "#2C2C2F"), corner: 26 }
    Stack.player_gold { foreground: color(light: "#FFD24A", dark: "#FFD24A") }
    Button.player_picture { hover: none, pressed: none }
    IconButton.player_primary { size: extra_large, hover: scale(1.08, snappy), pressed: scale(0.84, press), settle: jelly }
    #[target(browser)]
    IconButton.player_primary { size: regular }
    Stack.onboarding_art { background: color(light: "#232A52", dark: "#232A52") }
    // Cards and groups take the system's neutral fill (P2); only the
    // picture, the player and the brand art keep their own colors.
    Stack.onboarding_card { background: fill.subtle, corner: 16 }
    Stack.onboarding_check { foreground: on_accent }
    Stack.onboarding_swatch_label { foreground: text.secondary }
    Button.onboarding_crimson { foreground: color(light: "#FF2E55", dark: "#FF2E55"), hover: scale(1.08, snappy), pressed: scale(0.94, press) }
    Button.onboarding_blue { foreground: color(light: "#2A51D4", dark: "#2A51D4"), hover: scale(1.08, snappy), pressed: scale(0.94, press) }
    Button.onboarding_green { foreground: color(light: "#07FF94", dark: "#07FF94"), hover: scale(1.08, snappy), pressed: scale(0.94, press) }
    Button.onboarding_card { corner: 16, hover: none, pressed: scale(0.98, press), settle: jelly }
    Stack.row_group { background: fill.subtle, corner: 14 }
    Stack.settings_card { background: fill.subtle, corner: 16 }
    Stack.settings_hud { background: color(light: "#18181B", dark: "#18181B"), corner: 17 }
    Stack.settings_wordmark { foreground: color(light: "#213A6B", dark: "#FFFFFF") }
    Stack.settings_tag { background: fill.subtle, corner: 999 }
    Stack.settings_banner { background: color(light: "#232A52", dark: "#232A52"), corner: 16 }
    pattern { academy: check(gap(22), band(color(light: "#000000", dark: "#000000"), 34, 0.3), gap(6), band(accent, 3, 0.92), gap(6), band(color(light: "#000000", dark: "#000000"), 34, 0.3), gap(15), band(color(light: "#FFFFFF", dark: "#FFFFFF"), 1.5, 0.5), gap(16.5), twill: true) }
    Button.theme_swatch { hover: scale(1.08, snappy), pressed: scale(0.94, press) }
    Stack.crimson_swatch { background: color(light: "#FF2E55", dark: "#FF2E55"), corner: 15 }
    Stack.ultramarine_swatch { background: color(light: "#2A51D4", dark: "#2A51D4"), corner: 15 }
    Stack.neon_swatch { background: color(light: "#07FF94", dark: "#07FF94"), corner: 15 }
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
        player_panel_in: slide(right, 24, hero),
        player_feedback_in: pop(from: 0.6, jelly),
        page_in: slide(bottom, 12, smooth),
        settings_page_in: slide(bottom, 8, smooth),
        drop_in: slide(top, 24, jelly),
        pop_in: pop(from: 0.4, bouncy),
        feature_preview_item_in: pop(from: 0.4, jelly),
        feature_preview_in: pop(from: 0.95, snappy),
        // Home timeline: draw the ruler, drop the cards, then raise "now".
        timeline_rule_in: grow(edge: leading, smooth),
        timeline_card_in: slide(edge: top, distance: 18, jelly),
        timeline_now_in: grow(edge: bottom, bouncy),
        library_card_in: slide(edge: bottom, distance: 22, smooth),
        library_card_out: pop(from: 0.86, snappy),
        calendar_column_in: slide(edge: bottom, distance: 18, jelly),
        calendar_row_in: slide(edge: bottom, distance: 10, jelly),
    }

    // Share navigation motion; each target keeps its selection palette.
    SidebarItem {
        corner: 9,
        hover: scale(1.03, snappy),
        pressed: scale(0.94, press),
        selected: system,
        settle: jelly,
    }

    // The macOS prototype uses neutral selection with accent-colored icons.
    #[target(macos)]
    SidebarItem {
        hover: background(fill.subtle) + scale(1.03, snappy),
        selected: fill(fill.strong),
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

    Button.library_chip {
        size: large,
    }

    Button.library_index {
        hover: scale(1.12, snappy),
        pressed: scale(0.9, press),
    }

    // A cell of the film.
    Button.film {
        corner: 16,
        pressed: scale(0.97, press),
        settle: jelly,
    }

    // A row of a list that is a button as a whole.
    Button.row {
        corner: 9,
        padding: 6,
        hover: background(fill.subtle),
        pressed: scale(0.98, press),
        settle: jelly,
    }

    // Playback shortcuts over stills: only the white symbol is visible.
    Button.preview_play {
        corner: 8,
        hover: none,
        pressed: scale(0.94, press),
        settle: jelly,
    }

    // The weekly calendar is a group of content cards. Only the rows
    // highlight on hover; the column itself keeps its exact geometry.
    Button.calendar_day {
        corner: 22,
        background: fill.subtle,
        hover: none,
        pressed: none,
        settle: bouncy,
    }

    Button.calendar_selected {
        corner: 22,
        border: control.border,
        hover: none,
        pressed: none,
        settle: bouncy,
    }

    Button.calendar_row {
        corner: 12,
        hover: background(fill.subtle),
        pressed: background(fill.strong),
        settle: snappy,
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
            Window(id: main, title: tr! { "zh-Hans" => "Erika", "en" => "Erika" }, initial: open, width: 1280, height: 800, min_width: 720, min_height: 480, chrome: unified, idle_after: 3, idle_enabled: app.playing && app.expanded && !app.paused && !app.player.panel, immersive: app.playing && app.expanded) {
                main_window(app: app, anime: anime, windows: windows)
            }
            Window(id: settings, title: tr! { "zh-Hans" => "设置", "en" => "Settings" }, width: 1000, height: 660, min_width: 1000, min_height: 660, chrome: standard) {
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
        Stack(reduce_motion: app.preferences.reduce_motion) {
            Timer(interval: 0.25, enabled: app.playing) => app.player_tick()
            Slot(id: background)
            if app.onboarding.visible {
                Stack(enabled: false) {
                    main_shell(app: app, anime: anime, windows: windows)
                }
                onboarding(app: app)
            } else {
                Stack(hero: onboarding_window) {
                    main_shell(app: app, anime: anime, windows: windows)
                }
            }
        }
    }
}

#[component]
pub fn main_shell(app: erika::Client, anime: anime::Client, windows: AppWindows, #[session] draft: ErikaDraft) -> impl View {
    ui! {
        Stack {
            Stack(enabled: !app.playing || !app.expanded) {
            SplitView {
                Sidebar(id: pages, selection: draft.page) {
                    SidebarHeader {
                        // Match the navigation icon centers and title column.
                        Row(gap: 5, padding_x: 12, align: center) {
                            Image(source: "design/brand/erika-icon-export/png/mono-black/erika-mono-black-128.png", width: 22, height: 22, rendering: template, label: tr! { "zh-Hans" => "Erika", "en" => "Erika" })
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
                        // Keep the destination inside the native button so
                        // its toolbar capsule measures and pads the whole label.
                        if anime.opened > 0 && (anime.back_label == "搜索" || (draft.page == MainPage::Home && !anime.opened_from_schedule && anime.back_label == "首页") || (draft.page == MainPage::New && anime.opened_from_schedule) || (draft.page == MainPage::Library && anime.back_label == "媒体库")) {
                            Button(id: back, icon: "chevron.left", label: anime.back_label, shortcut: "escape") => anime.close()
                        }
                        Spacer
                        Search(id: search, value: draft.query, style: palette, width: 640, height: 480, panel_prompt: tr! { "zh-Hans" => "搜索番剧、剧集、设置和操作", "en" => "Search shows, episodes, settings and actions" }, prompt: tr! { "zh-Hans" => "搜索番剧、文件、设置", "en" => "Search shows, files and settings" }, shortcut: "cmd+k") {
                            search_results(app: app, anime: anime, windows: windows)
                        }
                        Button(id: open_file, label: tr! { "zh-Hans" => "打开", "en" => "Open" }, icon: "plus", pick: draft.file, files: "video", shortcut: "cmd+o") => app.open_file(draft.file)
                    }
                    if !app.features.anime && !app.features.library && !app.features.downloads {
                        empty_frame(app: app)
                    } else if anime.opened > 0 && (anime.back_label == "搜索" || (draft.page == MainPage::Library && anime.back_label == "媒体库")) {
                        details(anime: anime)
                    } else {
                    match draft.page {
                        MainPage::Idle => { empty_frame(app: app) }
                        _ => { PageContent(slot: main, selected: draft.page) }
                    }
                    }
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
                    Image(source: app.still, width: 208, height: 117, corner: 22, fit: cover, label: app.title, hero: frame, hero_key: app.title)
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
                Column(gap: 16, align: center) {
                    if app.playing {
                        Text(content: app.title, style: title3)
                        Button(id: resume, label: tr! { "zh-Hans" => "继续播放", "en" => "Continue watching" }, icon: "play.fill", prominence: prominent) => app.expand()
                    }
                Button(id: open_here, label: tr! { "zh-Hans" => "打开文件", "en" => "Open File" }, icon: "folder", prominence: prominent, variant: primary, pick: draft.file, files: "video") => app.open_file(draft.file)
                }
            }
        }
    }
}
