//! The interface of Erika's shell: the window, the sidebar of the modules'
//! pages with the mini player and the account at its foot, the toolbar, and
//! the player, a layer above every page. The picture is one frame: it fills
//! the window while playing, steps back when the pointer moves, and shrinks
//! into the mini player on the way back.

mod player;
use player::{player, player_props};
mod search;
use search::{search_results, search_results_props};
mod phone;
use phone::{phone_shell, phone_shell_props, PhoneSettings};

use bern::prelude::*;
use erika_anime_protocol::anime::{self, BackTo};
use erika_anime_ui::{details, details_props};
use erika_protocol::erika;
mod onboarding;
use onboarding::{onboarding, onboarding_props};
mod feature_preview;
mod settings;
use settings::{settings, settings_props};
mod pure;
use pure::{pure_window, pure_window_props};
mod tv;
use tv::{tv_shell, tv_shell_props};

style! {
    accents {
        crimson: "#FF2E55",
        ultramarine: "#2A51D4",
        neon: color(light: "#00B86B", dark: "#07FF94"),
    }

    // The type of the design (section 8.2 and the desktop prototype):
    // the system font at the prototype's sizes and weights.
    // `tv`: the television's sizes (next.css `.nx[data-dev="tv"]`), read
    // from across the room: `--t-lt` 34, `--t-h1` 32, `--t-h2` 22,
    // `--t-title` 17, `--t-body` 16, `--t-cap` 13.5, `--t-time` 14.
    text {
        large_title: font(size: 30, tv: 34, weight: bold),
        title: font(size: 34, weight: bold),
        title2: font(size: 28, tv: 32, weight: bold),
        title3: font(size: 19, tv: 22, weight: bold),
        headline: font(size: 14, tv: 17, weight: semibold),
        // `--t-body`: the words of the pages, 13 points on every device
        // but a television.
        body: font(size: 13, tv: 16, weight: regular),
        callout: font(size: 16, weight: semibold),
        caption: font(size: 11.5, tv: 13.5, weight: medium),
        caption2: font(size: 12.5, tv: 14, weight: semibold),
        subheadline: font(size: 8.5, weight: semibold),
    }

    Stack.player_black { background: color(light: "#000000", dark: "#000000"), foreground: color(light: "#FFFFFF", dark: "#FFFFFF") }
    Stack.player_background { background: color(light: "#1C1C1E", dark: "#1C1C1E"), foreground: color(light: "#FFFFFF", dark: "#FFFFFF") }
    // The web design system's player stands on its dark page
    // (`.x-player.shown` takes `--bg`, #09090B).
    #[target(browser)]
    Stack.player_background { background: color(light: "#09090B", dark: "#09090B"), foreground: color(light: "#FFFFFF", dark: "#FFFFFF") }
    Stack.player_panel { background: color(light: "#2C2C2F", dark: "#2C2C2F"), corner: 26 }
    // The web design system's panel (`.p-sheet`): the page's black, a
    // 1-point outline, 12-point corners.
    #[target(browser)]
    Stack.player_panel { background: color(light: "#09090B", dark: "#09090B"), border: border(color(light: "#27272A", dark: "#27272A"), width: 1), corner: 12 }
    // Fluent's panel (`.nx[data-ds="fluent"] .p-sheet`): 8-point corners.
    #[target(windows)]
    Stack.player_panel { corner: 8 }
    // libadwaita's and Material's panels: 12- and 28-point corners.
    #[target(linux)]
    Stack.player_panel { corner: 12 }
    #[target(android)]
    Stack.player_panel { corner: 28 }
    Stack.player_gold { foreground: color(light: "#FFD24A", dark: "#FFD24A") }
    Stack.player_cyan { foreground: color(light: "#8EE3FF", dark: "#8EE3FF") }
    Stack.player_pink { foreground: color(light: "#FFB3C7", dark: "#FFB3C7") }
    Button.player_picture { hover: none, pressed: none }
    // The television's picture while the controls are hidden: it holds
    // the remote's focus without showing it.
    Button.remote_surface { hover: none, pressed: none, focus: none }
    IconButton.player_primary { size: extra_large, hover: scale(1.08, snappy), pressed: scale(0.84, press), settle: jelly }
    #[target(browser)]
    IconButton.player_primary { size: regular }
    Stack.onboarding_art { background: color(light: "#232A52", dark: "#232A52") }
    // Cards and groups take the system's neutral fill (P2); only the
    // picture, the player and the brand art keep their own colors.
    Stack.onboarding_card { background: fill.subtle, corner: large }
    Stack.onboarding_check { foreground: on_accent }
    Stack.onboarding_swatch_label { foreground: text.secondary }
    Button.onboarding_crimson { foreground: color(light: "#FF2E55", dark: "#FF2E55"), hover: scale(1.08, snappy), pressed: scale(0.94, press) }
    Button.onboarding_blue { foreground: color(light: "#2A51D4", dark: "#2A51D4"), hover: scale(1.08, snappy), pressed: scale(0.94, press) }
    Button.onboarding_green { foreground: color(light: "#07FF94", dark: "#07FF94"), hover: scale(1.08, snappy), pressed: scale(0.94, press) }
    Button.onboarding_card { corner: large, hover: none, pressed: scale(0.98, press), settle: jelly }
    Stack.row_group { background: fill.subtle, corner: list }
    Stack.settings_card { background: fill.subtle, corner: large }
    Stack.settings_hud { background: color(light: "#18181B", dark: "#18181B"), corner: 17 }
    Stack.settings_wordmark { foreground: color(light: "#213A6B", dark: "#FFFFFF") }
    Stack.settings_tag { background: fill.subtle, corner: 999 }
    Stack.settings_banner { background: color(light: "#232A52", dark: "#232A52"), corner: large }
    pattern { academy: check(gap(22), band(color(light: "#000000", dark: "#000000"), 34, 0.3), gap(6), band(accent, 3, 0.92), gap(6), band(color(light: "#000000", dark: "#000000"), 34, 0.3), gap(15), band(color(light: "#FFFFFF", dark: "#FFFFFF"), 1.5, 0.5), gap(16.5), twill: true) }
    Button.theme_swatch { hover: scale(1.08, snappy), pressed: scale(0.94, press) }
    Stack.crimson_swatch { background: color(light: "#FF2E55", dark: "#FF2E55"), corner: 15 }
    Stack.ultramarine_swatch { background: color(light: "#2A51D4", dark: "#2A51D4"), corner: 15 }
    Stack.neon_swatch { background: color(light: "#07FF94", dark: "#07FF94"), corner: 15 }
    // The shapes of the design (section 6.4, next.css `--r-*`), named so
    // that each design language takes its own; these are Apple's.
    corner {
        // `--r-card`: the frame wherever it stands (the stage, the mini
        // player, the player, the empty frame) and the calendar's columns.
        card: 22,
        // The empty frame's 2-point line and its dashed line inside
        // (`.i-frame::before`, 10 in: `--r-card` less 6), each where the
        // middle of its stroke runs.
        frame_edge: 21,
        frame_dash: 15.25,
        // The player's picture, drawn at 1280 × 720 and scaled to 896
        // beside the settings or to 1074 stepped back: `--r-card` before
        // the scale (22 × 1280 / 896 and 22 × 1280 / 1074).
        frame_beside: 31.43,
        frame_back: 26.22,
        // `--r-poster`.
        poster: 10,
        // A row's thumbnail (`.h-row .x-thumb`, `.i-rc .x-thumb`):
        // `--r-poster` less 2.
        thumb: 8,
        // `--r-l`: the film's cells (`.d-ep .x-thumb`) and the raised
        // surfaces (`.o-card`, `.o-mw`, `.m-card`, `.m-banner`).
        large: 16,
        // `--r-m`: rows of a list (`.n-row`, `.i-rc`).
        medium: 12,
        // The home page's rows (`.h-row`) and the sidebar's items
        // (`.c-navi`).
        row: 9,
        nav: 9,
        // A grouped list (`.c-list`); Fluent's rows are cards of their own
        // (`.c-li`).
        list: 14,
    }
    // Fluent 2 (`.nx[data-ds="fluent"]`): `--r-card` 8, `--r-poster` 6,
    // `--r-l` 8, `--r-m` 4, `.c-navi` 4.
    #[target(windows)]
    corner {
        card: 8,
        frame_edge: 7,
        frame_dash: 1.25,
        frame_beside: 11.43,
        frame_back: 9.53,
        poster: 6,
        thumb: 4,
        large: 8,
        medium: 4,
        row: 4,
        nav: 4,
        list: 4,
    }
    // libadwaita (`.nx[data-ds="adwaita"]`): `--r-card` 12, `--r-poster` 8,
    // `--r-l` 12, `--r-m` 9, `.c-navi` 6, `.c-list` 12.
    #[target(linux)]
    corner {
        card: 12,
        frame_edge: 11,
        frame_dash: 5.25,
        frame_beside: 17.14,
        frame_back: 14.3,
        poster: 8,
        thumb: 6,
        large: 12,
        medium: 9,
        row: 9,
        nav: 6,
        list: 12,
    }
    // Material 3 (`.nx[data-ds="material"]`): `--r-card` 28, `--r-poster`
    // 12, `--r-l` 16, `--r-m` 12, `.c-navi` 26, `.c-list` 20.
    #[target(android)]
    corner {
        card: 28,
        frame_edge: 27,
        frame_dash: 21.25,
        frame_beside: 40,
        frame_back: 33.37,
        poster: 12,
        thumb: 10,
        large: 16,
        medium: 12,
        row: 12,
        nav: 26,
        list: 20,
    }
    // The web design system (`.nx[data-ds="web"]`): `--r-card` 12,
    // `--r-poster` 10, `--r-l` 12, `--r-m` 8, `.c-navi` 8, `.c-list` 12.
    #[target(browser)]
    corner {
        card: 12,
        frame_edge: 11,
        frame_dash: 5.25,
        frame_beside: 17.14,
        frame_back: 14.3,
        poster: 10,
        thumb: 8,
        large: 12,
        medium: 8,
        row: 8,
        nav: 8,
        list: 12,
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
        // The playback settings panel slides in from the right (T4).
        player_panel_in: slide(trailing, 24, hero),
        player_feedback_in: pop(from: 0.6, jelly),
        // The controls around the frame (`.x-player .fx`, T10): in, they
        // fade and come 8 points from their edge on a spring that overshoots
        // a little; out, they leave the same way, quicker. The top bar comes
        // from above, the rest from below.
        controls_in: slide(bottom, 8, bouncy),
        controls_out: slide(bottom, 8, snappy),
        top_bar_in: slide(top, 8, bouncy),
        top_bar_out: slide(top, 8, snappy),
        // Pure playback (`renderIdle`): the empty frame grows from 0.96 on a
        // jelly spring; the recent videos rise 10 points one after another.
        pure_frame_in: pop(from: 0.96, jelly),
        recent_in: slide(edge: bottom, distance: 10, jelly),
        // A new page fades in and rises 12 points (T3).
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
        corner: nav,
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

    // The web prototype's sidebar (`.c-navi` on the web design system):
    // pointing and selection both lay the lightest neutral fill.
    #[target(browser)]
    SidebarItem {
        hover: background(fill.subtle) + scale(1.03, snappy),
        selected: fill(fill.subtle),
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
        corner: card,
        pressed: scale(0.98, press),
        settle: jelly,
    }

    // A thumbnail clips its progress bar to the picture's corners, as the
    // stage's button does.
    Stack.thumbnail {
        corner: thumb,
    }

    // The television's settings: the panel of pages (`.s-side`, 16-point
    // corners on the raised surface) and its rows, which the focus turns
    // white.
    Stack.settings_tv_side { background: surface.raised, corner: 16 }
    Button.settings_tv_nav { corner: 23, hover: background(fill.subtle), pressed: scale(0.97, press), settle: jelly }
    // A key of the television's search keyboard (`.tv-kb .c-btn`): the
    // neutral fill, 12-point corners; the focus turns it white.
    Button.tv_key { background: fill.subtle, corner: 12, hover: none, pressed: scale(0.94, press), settle: jelly }
    // The grids beside the television's zoom slider.
    Stack.zoom_symbol { foreground: text.secondary }

    // Pure playback (design 10.2): the film in the empty frame in the
    // tertiary label color (`--on3`), a recent video's row (`.i-rc`), and
    // the key on 「继续上次」 (`.i-key kbd`).
    Stack.pure_symbol { foreground: text.placeholder }
    Button.recent_row {
        corner: medium,
        padding: 6,
        hover: background(fill.subtle),
        pressed: scale(0.98, press),
        settle: jelly,
    }
    Stack.key_chip { background: fill.strong, foreground: text.secondary, corner: 5 }
    // What 「打开链接…」 says of a link it cannot play.
    Stack.link_error { foreground: status.error }
    // The phone's recent video (`.c-li .x-thumb`): 6-point corners.
    Stack.phone_recent_thumb { corner: 6 }

    // The phone's pages: the design's background (`--bg`), the system's
    // grouped secondary background in both appearances.
    Stack.phone_page {}
    #[target(ios)]
    Stack.phone_page { background: color(light: "#FFFFFF", dark: "#1C1C1E") }

    // The phone's grouped lists (`.c-list`): the raised card on the page's
    // own background, as grouped lists stand in iOS's elevated content.
    #[target(ios)]
    Form { background: surface.raised }

    // The first run's page (`.o-win`): the design's background on the
    // phone and the television, which it covers.
    Stack.onboarding_page {}
    #[target(ios)]
    Stack.onboarding_page { background: color(light: "#FFFFFF", dark: "#1C1C1E") }
    // On the television the first run fills the screen, over the pages.
    #[target(tvos)]
    Stack.onboarding_page { background: color(light: "#FFFFFF", dark: "#1C1C1E") }

    // The symbol of a settings row (`.c-li .lic`): 28 points on the neutral
    // fill, 7-point corners.
    Stack.setting_icon { background: fill.strong, corner: 7 }

    // A show's tags (`.d-tags span`): pills on the neutral fill.
    Stack.tag_pill { background: fill.subtle, corner: 999 }
    // An account's picture is round: the disc cuts the shoulders.
    Stack.avatar { corner: 999 }

    // The phone's way back (`.x-pback`): the accent, as iOS's.
    Stack.phone_back { foreground: accent }

    // The phone's header row (`.x-av`): the account, a round button of the
    // bar's 44 points.
    Button.phone_avatar {
        corner: 22,
        hover: none,
        pressed: scale(0.92, press),
        settle: jelly,
    }

    // The phone's cards (`.ph-cards`): the thumbnail clips its progress to
    // its 12-point corners.
    // The phone's main action (`.c-btn.p.L`), across the width.
    Button.phone_main {
        pressed: scale(x: 0.97, y: 0.9, press),
        settle: jelly,
    }

    Button.phone_card {
        hover: none,
        pressed: scale(0.97, press),
        settle: jelly,
    }

    Stack.phone_thumb {
        corner: 12,
    }

    Button.poster {
        corner: poster,
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
        corner: large,
        pressed: scale(0.97, press),
        settle: jelly,
    }

    // A row of a list that is a button as a whole.
    Button.row {
        corner: row,
        padding: 6,
        hover: background(fill.subtle),
        pressed: scale(0.98, press),
        settle: jelly,
    }

    // Playback shortcuts over stills: only the white symbol is visible.
    Button.preview_play {
        corner: thumb,
        hover: none,
        pressed: scale(0.94, press),
        settle: jelly,
    }

    // The weekly calendar is a group of content cards. Only the rows
    // highlight on hover; the column itself keeps its exact geometry.
    Button.calendar_day {
        corner: card,
        background: fill.subtle,
        hover: none,
        pressed: none,
        settle: bouncy,
    }

    Button.calendar_selected {
        corner: card,
        border: control.border,
        hover: none,
        pressed: none,
        settle: bouncy,
    }
    // The phone's open day (`.n-vrow.on`): the raised card with its hairline.
    #[target(ios)]
    Button.calendar_selected {
        corner: card,
        background: surface.raised,
        border: border(fill.subtle, width: 0.5),
        hover: none,
        pressed: none,
        settle: bouncy,
    }

    Button.calendar_row {
        corner: medium,
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
    /// The phone's search tab; the desktop searches from the toolbar.
    #[label(tr! { "zh-Hans" => "搜索", "en" => "Search" })]
    Search,
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
    /// The phone's settings, pushed over the home page.
    #[initial(PhoneSettings::Closed)]
    pub phone_settings: PhoneSettings,
}

#[component]
pub fn app(app: erika::Client, anime: anime::Client, windows: AppWindows) -> impl View {
    ui! {
        WindowSet(handle: AppWindows, primary: main, accent: app.accent, appearance: app.appearance) {
            Window(id: main, title: tr! { "zh-Hans" => "Erika", "en" => "Erika" }, initial: open, width: 1280, height: 800, min_width: 720, min_height: 480, chrome: unified, idle_after: 3, idle_enabled: app.playing && app.expanded && !app.paused && !app.player.panel, immersive: app.playing && app.expanded, landscape: app.playing && app.expanded) {
                main_window(app: app, anime: anime, windows: windows)
            }
            Window(id: settings, title: tr! { "zh-Hans" => "设置", "en" => "Settings" }, width: 1000, height: 660, min_width: 1000, min_height: 660, chrome: standard) {
                settings(app: app, windows: windows)
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
            Adaptive(id: shell) {
            // A phone (design 3.1): the pages are tabs, search the last.
            Layout(size: compact) {
                phone_shell(app: app, anime: anime, windows: windows)
            }
            // A television (design 3.1, 6.2): the tabs along the top,
            // search the first, driven by the remote's focus.
            Layout(size: tv) {
                tv_shell(app: app, anime: anime, windows: windows)
            }
            Layout(size: expanded) {
            // Pure playback (design 10.2): no module adds pages, so no
            // sidebar; the window is the empty frame.
            if !app.features.anime && !app.features.library && !app.features.downloads {
                pure_window(app: app, windows: windows)
            } else {
            SplitView {
                Sidebar(id: pages, selection: draft.page) {
                    // The logo and name; Windows shows both in the title bar
                    // instead.
                    SidebarHeader(role: title) {
                        // Match the navigation icon centers and title column.
                        Row(gap: 5, padding_x: 12, align: center) {
                            // `.x-brand img`: the logo in its own colors.
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
                        // Keep the destination inside the native button so
                        // its toolbar capsule measures and pads the whole label.
                        if anime.opened > 0 && (anime.back == BackTo::Search || (draft.page == MainPage::Home && !anime.opened_from_schedule && anime.back == BackTo::Home) || (draft.page == MainPage::New && anime.opened_from_schedule) || (draft.page == MainPage::Library && anime.back == BackTo::Library)) {
                            Button(id: back, icon: "chevron.left", label: anime.back, shortcut: "escape") => anime.close()
                        }
                        Spacer
                        Search(id: search, value: draft.query, style: palette, width: 640, height: 480, panel_prompt: tr! { "zh-Hans" => "搜索番剧、剧集、设置和操作", "en" => "Search shows, episodes, settings and actions" }, prompt: tr! { "zh-Hans" => "搜索番剧、文件、设置", "en" => "Search shows, files and settings" }, shortcut: "cmd+k") {
                            search_results(app: app, anime: anime, windows: windows)
                        }
                        Button(id: open_file, label: tr! { "zh-Hans" => "打开", "en" => "Open" }, icon: "plus", pick: draft.file, files: "video", shortcut: "cmd+o") => app.open_file(draft.file)
                    }
                    if anime.opened > 0 && (anime.back == BackTo::Search || (draft.page == MainPage::Library && anime.back == BackTo::Library)) {
                        details(anime: anime)
                    } else {
                        PageContent(slot: main, selected: draft.page)
                    }
                }
            }
            }
            }
            }
            }
            if app.playing && app.expanded {
                // In pure playback the frame is where the player's frame
                // steps back to: the picture fades in over it, then the
                // controls (`plOpen` with `fromIdle`), and fades out on the
                // way back.
                if !app.features.anime && !app.features.library && !app.features.downloads {
                    Stack(appear: fade, disappear: fade) {
                        player(app: app)
                    }
                } else {
                    player(app: app)
                }
            }
        }
    }
}

/// The account's picture; a person on a grey disc until the account has
/// one of its own (`.x-av`): the head 40 % down, the shoulders an ellipse
/// centred on the bottom edge, both in the tertiary label color (`--on3`),
/// and the disc cuts the shoulders.
#[component]
pub fn avatar() -> impl View {
    ui! {
        Stack(variant: avatar, width: 30, height: 30) {
            Canvas(width: 30, height: 30, label: tr! { "zh-Hans" => "头像", "en" => "Avatar" }) {
                Shape(kind: ellipse, fill: fill.strong)
                Shape(kind: ellipse, x: 10.55, y: 7.55, width: 8.9, height: 8.9, fill: text.placeholder)
                Shape(kind: ellipse, x: -3, y: 16.2, width: 36, height: 27.6, fill: text.placeholder)
            }
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
                    Image(source: app.still, width: 208, height: 117, corner: card, fit: cover, label: app.title, hero: frame, hero_key: app.title)
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
                    Text(content: tr! { "zh-Hans" => "{subtitle} · {elapsed, duration, mm:ss}", "en" => "{subtitle} · {elapsed, duration, mm:ss}"; subtitle: app.subtitle, elapsed: app.elapsed }, style: caption, emphasis: secondary)
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
