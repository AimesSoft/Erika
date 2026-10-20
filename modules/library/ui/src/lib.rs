//! Media library, matching design/next-screens.js and next.css.
use bern::prelude::*;
use erika_library_protocol::library::{self, LibraryFilter, LibrarySort, LibrarySource};
use erika_protocol::erika;

#[bern::value]
pub enum ServerSource {
    #[label(tr! { "zh-Hans" => "本地", "en" => "Local" })]
    Local,
    #[label(tr! { "zh-Hans" => "服务器", "en" => "Servers" })]
    Server,
}
#[bern::value]
pub enum SharedSource {
    #[label(tr! { "zh-Hans" => "本地", "en" => "Local" })]
    Local,
    #[label(tr! { "zh-Hans" => "共享", "en" => "Shared" })]
    Shared,
}
#[bern::session]
pub struct LibraryDraft {
    pub source: LibrarySource,
    pub server_source: ServerSource,
    pub shared_source: SharedSource,
    pub filter: LibraryFilter,
    pub sort: LibrarySort,
    /// 84 + 0.36 × (200 − 84), as in the desktop prototype.
    #[initial(125.76)]
    pub poster_size: f64,
    /// The phone's wall runs from 64 to 150: 64 + 0.36 × 86.
    #[initial(94.96)]
    pub phone_poster_size: f64,
    /// The television's wall runs from 112 to 230: 112 + 0.36 × 118
    /// (`libLayout` on the television).
    #[initial(154.48)]
    pub tv_poster_size: f64,
    pub position: i64,
}

#[component]
pub fn library_page(
    library: library::Client,
    app: erika::Client,
    #[session] draft: LibraryDraft,
) -> impl View {
    ui! {
        if !app.modules.anime && library.opened > 0 {
            library_preview(library: library)
        } else {
            Query(id: catalog, query: library.browse(
                if app.modules.sources && app.modules.remote { draft.source }
                else if app.modules.sources && draft.server_source == ServerSource::Server { LibrarySource::Server }
                else if app.modules.remote && draft.shared_source == SharedSource::Shared { LibrarySource::Shared }
                else { LibrarySource::Local }, draft.filter, draft.sort)) |listing| {
                if window.compact {
                    library_phone(library: library, app: app, listing: listing)
                } else {
                Column(gap: 0, padding_x: if window.tv { 56.0 } else if window.medium { 24.0 } else { 32.0 }, padding_y: 6, appear: page_in) {
                    Adaptive {
                        Layout(size: expanded) {
                            Row(gap: 12, align: center) {
                                Text(content: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, style: large_title)
                                library_sources(app: app)
                                Spacer
                                library_zoom()
                                library_sort()
                                Button(id: manage, label: tr! { "zh-Hans" => "管理", "en" => "Manage" }, icon: "folder", shape: capsule, height: if window.tv { 46 } else { 32 }) => library.manage()
                            }
                        }
                        Layout(size: compact) {
                            Column(gap: 12) {
                                Row(gap: 12, align: center) {
                                    Text(content: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, style: large_title)
                                    Spacer
                                    library_sort()
                                    Button(id: manage_small, label: tr! { "zh-Hans" => "管理", "en" => "Manage" }, icon: "folder", shape: capsule, height: if window.tv { 46 } else { 32 }) => library.manage()
                                }
                                Row(gap: 12, align: center) {
                                    library_sources(app: app)
                                    Spacer
                                    library_zoom()
                                }
                            }
                        }
                    }
                    Canvas(height: 14)
                    Row(gap: 8, align: center) {
                        ForEach(items: listing.filters, key: id) |chip| {
                            library_filter(value: chip.value, count: chip.count)
                        }
                        Spacer
                    }
                    Canvas(height: 18)
                    library_wall(library: library, listing: listing)
                }
                }
            }
            if library.notice > 0 {
                Toast(content: tr! { "zh-Hans" => "管理媒体库：文件夹、扫描、服务器", "en" => "Manage library: folders, scanning and servers" }, key: library.notice, icon: "folder")
            }
        }
    }
}

// Keep continuous zoom and scroll updates inside the wall. The catalog,
// toolbar and filters do not depend on either value.
#[component]
pub fn library_wall(
    library: library::Client,
    listing: library::LibraryListing,
    #[session] draft: LibraryDraft,
) -> impl View {
    ui! {
        if window.tv {
            ScrollView(position: draft.position, arrival: nudge) {
                library_grid(library: library, listing: listing, size: draft.tv_poster_size, gap: 20.0, titles: true)
                Canvas(height: 48)
            }
        } else {
            Pinch(id: zoom_wall, label: tr! { "zh-Hans" => "缩放海报墙", "en" => "Zoom poster wall" }, value: draft.poster_size, min: 84, max: 200) {
                ScrollView(position: draft.position, arrival: nudge) {
                    library_grid(library: library, listing: listing, size: draft.poster_size, gap: if draft.poster_size < 110.0 { 10.0 } else { 20.0 }, titles: draft.poster_size >= 110.0)
                    Canvas(height: 48)
                }
            }
        }
    }
}

/// The phone's library (`libHTML(true)`): one page scrolling as a whole,
/// its header row (`.x-pnav`: the large title and, 8 apart, managing and
/// sorting as 34-point round buttons), the sources and the zoom 2 points
/// under it, the filters 12 under those and the wall 18 under them.
#[component]
pub fn library_phone(
    library: library::Client,
    app: erika::Client,
    listing: library::LibraryListing,
    #[session] draft: LibraryDraft,
) -> impl View {
    ui! {
        Pinch(id: zoom_wall_phone, label: tr! { "zh-Hans" => "缩放海报墙", "en" => "Zoom poster wall" }, value: draft.phone_poster_size, min: 64, max: 150) {
            ScrollView(position: draft.position, arrival: nudge, appear: page_in) {
                Column(gap: 0, padding_x: 16) {
                    Canvas(height: 7)
                    // Managing and sorting share one glass capsule, as the
                    // system bar groups neighbouring buttons, 44 points tall.
                    Row(gap: 8, height: 48, align: center) {
                        Text(content: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, style: large_title)
                        Spacer
                        ControlGroup(label: tr! { "zh-Hans" => "媒体库操作", "en" => "Library actions" }, height: 44) {
                            IconButton(id: manage_phone, icon: "folder", label: tr! { "zh-Hans" => "管理", "en" => "Manage" }) => library.manage()
                            library_sort_round()
                        }
                    }
                    Canvas(height: 2)
                    Row(gap: 10, align: center) {
                        library_sources(app: app)
                        Spacer
                        library_zoom()
                    }
                    Canvas(height: 12)
                    // `.l-chips` on the phone stay one line.
                    ScrollView(axis: horizontal, indicators: hidden) {
                        Row(gap: 8, align: center) {
                            ForEach(items: listing.filters, key: id) |chip| {
                                library_filter(value: chip.value, count: chip.count)
                            }
                        }
                    }
                    Canvas(height: 18)
                    // `libLayout` on the phone: 10 between posters below 110
                    // points, else 12; the titles while posters are 96 wide.
                    library_grid(library: library, listing: listing, size: draft.phone_poster_size, gap: if draft.phone_poster_size < 110.0 { 10.0 } else { 12.0 }, titles: draft.phone_poster_size >= 88.0)
                    Canvas(height: 48)
                }
            }
        }
    }
}

/// The wall and the letters beside it (`.l-wrap`, 28 points for the
/// letters, 22 on the phone).
#[component]
pub fn library_grid(
    library: library::Client,
    listing: library::LibraryListing,
    size: f64,
    gap: f64,
    titles: bool,
    #[session] draft: LibraryDraft,
) -> impl View {
    ui! {
        Row(gap: if window.compact { 2.0 } else { 8.0 }, align: top) {
            // `libLayout`: every row is 2 : 3 posters and 54 points for
            // the titles, whatever the titles take.
            Grid(min_width: size, gap: gap, row_gap: if titles { 16.0 } else { gap }, cell_ratio: 0.6666666667, cell_extra: if titles { 54.0 } else { 0.0 }, stagger: 0.016, animate: smooth) {
                ForEach(items: listing.posters, key: id, reflow: smooth) |poster| {
                    library_poster(library: library, poster: poster, show_details: titles)
                }
            }
            // A television's letters are 26 points wide (`.l-idx span`).
            Column(gap: 1, width: if window.tv { 26 } else { 20 }) {
                ForEach(items: listing.letters, key: letter) |letter| {
                    Button(id: index, label: letter.letter, enabled: letter.target > 0, variant: library_index) {
                        Column(gap: 0, width: if window.tv { 26 } else { 20 }, height: 18, align: center) {
                            Text(content: letter.letter, style: caption2, font_size: 10.5, emphasis: secondary)
                        }
                    } => draft.position.set(letter.target)
                }
            }
        }
    }
}

#[component]
pub fn library_sources(app: erika::Client, #[session] draft: LibraryDraft) -> impl View {
    ui! {
        if app.modules.sources && app.modules.remote {
            Row(width: 204) {
                Picker(id: source, label: tr! { "zh-Hans" => "媒体来源", "en" => "Media source" }, value: draft.source, style: segmented, label_hidden: true)
            }
        } else if app.modules.sources {
            Row(width: 140) {
                Picker(id: server_source, label: tr! { "zh-Hans" => "媒体来源", "en" => "Media source" }, value: draft.server_source, style: segmented, label_hidden: true)
            }
        } else if app.modules.remote {
            Row(width: 140) {
                Picker(id: shared_source, label: tr! { "zh-Hans" => "媒体来源", "en" => "Media source" }, value: draft.shared_source, style: segmented, label_hidden: true)
            }
        }
    }
}

#[component]
pub fn library_zoom(#[session] draft: LibraryDraft) -> impl View {
    ui! {
        // `.l-zoom`: 8 between the parts and 8 more on each side of the
        // slider.
        if window.tv {
            // The television's zoom: the slider between the two grids, which
            // the remote moves by a step (`.l-zs`, 132 points).
            Row(gap: 8, align: center) {
                Stack(variant: zoom_symbol) {
                    Label(title: tr! { "zh-Hans" => "缩小海报", "en" => "Smaller posters" }, icon: "square.grid.3x3", display: icon, style: caption)
                }
                Row(width: 132) {
                    Slider(id: tv_poster_size, label: tr! { "zh-Hans" => "海报大小", "en" => "Poster size" }, value: draft.tv_poster_size, min: 112, max: 230, step: 11.8, label_hidden: true)
                }
                Stack(variant: zoom_symbol) {
                    Label(title: tr! { "zh-Hans" => "放大海报", "en" => "Larger posters" }, icon: "square.grid.2x2", display: icon, style: caption)
                }
            }
        } else if window.compact {
            // The small grid in 14 points, the large in 20, as the design draws
            // them; the slider is its 28 points tall.
            Row(gap: 16, height: 28, align: center) {
                Stack(width: 14, height: 14, align: center) {
                    IconButton(id: smaller_phone, icon: "square.grid.3x3", label: tr! { "zh-Hans" => "缩小海报", "en" => "Smaller posters" }, prominence: plain, font_size: 11) => draft.phone_poster_size.set(if draft.phone_poster_size > 140.0 { 140.0 } else if draft.phone_poster_size > 130.0 { 130.0 } else if draft.phone_poster_size > 120.0 { 120.0 } else if draft.phone_poster_size > 110.0 { 110.0 } else if draft.phone_poster_size > 100.0 { 100.0 } else if draft.phone_poster_size > 90.0 { 90.0 } else if draft.phone_poster_size > 80.0 { 80.0 } else if draft.phone_poster_size > 72.0 { 72.0 } else { 64.0 })
                }
                Row(width: 120) {
                    Slider(id: phone_poster_size, label: tr! { "zh-Hans" => "海报大小", "en" => "Poster size" }, value: draft.phone_poster_size, min: 64, max: 150, label_hidden: true)
                }
                Stack(width: 20, height: 20, align: center) {
                    IconButton(id: larger_phone, icon: "square.grid.2x2", label: tr! { "zh-Hans" => "放大海报", "en" => "Larger posters" }, prominence: plain, font_size: 16) => draft.phone_poster_size.set(if draft.phone_poster_size < 72.0 { 72.0 } else if draft.phone_poster_size < 80.0 { 80.0 } else if draft.phone_poster_size < 90.0 { 90.0 } else if draft.phone_poster_size < 100.0 { 100.0 } else if draft.phone_poster_size < 110.0 { 110.0 } else if draft.phone_poster_size < 120.0 { 120.0 } else if draft.phone_poster_size < 130.0 { 130.0 } else if draft.phone_poster_size < 140.0 { 140.0 } else { 150.0 })
                }
            }
        } else {
            Row(gap: 16, width: 188, align: center) {
                IconButton(id: smaller, icon: "square.grid.3x3", label: tr! { "zh-Hans" => "缩小海报", "en" => "Smaller posters" }, prominence: plain, shortcut: "cmd+-") => draft.poster_size.set(if draft.poster_size > 188.0 { 188.0 } else if draft.poster_size > 176.0 { 176.0 } else if draft.poster_size > 164.0 { 164.0 } else if draft.poster_size > 152.0 { 152.0 } else if draft.poster_size > 140.0 { 140.0 } else if draft.poster_size > 128.0 { 128.0 } else if draft.poster_size > 116.0 { 116.0 } else if draft.poster_size > 104.0 { 104.0 } else if draft.poster_size > 92.0 { 92.0 } else { 84.0 })
                Row(width: 120) {
                    Slider(id: poster_size, label: tr! { "zh-Hans" => "海报大小", "en" => "Poster size" }, value: draft.poster_size, min: 84, max: 200, label_hidden: true)
                }
                IconButton(id: larger, icon: "square.grid.2x2", label: tr! { "zh-Hans" => "放大海报", "en" => "Larger posters" }, prominence: plain, shortcut: "cmd+=") => draft.poster_size.set(if draft.poster_size < 96.0 { 96.0 } else if draft.poster_size < 108.0 { 108.0 } else if draft.poster_size < 120.0 { 120.0 } else if draft.poster_size < 132.0 { 132.0 } else if draft.poster_size < 144.0 { 144.0 } else if draft.poster_size < 156.0 { 156.0 } else if draft.poster_size < 168.0 { 168.0 } else if draft.poster_size < 180.0 { 180.0 } else if draft.poster_size < 192.0 { 192.0 } else { 200.0 })
            }
        }
    }
}

#[component]
pub fn library_sort(#[session] draft: LibraryDraft) -> impl View {
    ui! {
        if draft.sort == LibrarySort::Recent {
            Menu(id: sort_recent, label: draft.sort, icon: "arrow.up.arrow.down", shape: capsule, height: if window.tv { 46 } else { 32 }) {
                MenuItem(id: recent_recent, label: tr! { "zh-Hans" => "最近观看", "en" => "Recently watched" }, icon: "checkmark") => draft.sort.set(LibrarySort::Recent)
                MenuItem(id: name_recent, label: tr! { "zh-Hans" => "名称（拼音）", "en" => "Name (pinyin)" }) => draft.sort.set(LibrarySort::Name)
                MenuItem(id: rating_recent, label: tr! { "zh-Hans" => "评分", "en" => "Rating" }) => draft.sort.set(LibrarySort::Rating)
            }
        } else if draft.sort == LibrarySort::Name {
            Menu(id: sort_name, label: draft.sort, icon: "arrow.up.arrow.down", shape: capsule, height: if window.tv { 46 } else { 32 }) {
                MenuItem(id: recent_name, label: tr! { "zh-Hans" => "最近观看", "en" => "Recently watched" }) => draft.sort.set(LibrarySort::Recent)
                MenuItem(id: name_name, label: tr! { "zh-Hans" => "名称（拼音）", "en" => "Name (pinyin)" }, icon: "checkmark") => draft.sort.set(LibrarySort::Name)
                MenuItem(id: rating_name, label: tr! { "zh-Hans" => "评分", "en" => "Rating" }) => draft.sort.set(LibrarySort::Rating)
            }
        } else if draft.sort == LibrarySort::Rating {
            Menu(id: sort_rating, label: draft.sort, icon: "arrow.up.arrow.down", shape: capsule, height: if window.tv { 46 } else { 32 }) {
                MenuItem(id: recent_rating, label: tr! { "zh-Hans" => "最近观看", "en" => "Recently watched" }) => draft.sort.set(LibrarySort::Recent)
                MenuItem(id: name_rating, label: tr! { "zh-Hans" => "名称（拼音）", "en" => "Name (pinyin)" }) => draft.sort.set(LibrarySort::Name)
                MenuItem(id: rating_rating, label: tr! { "zh-Hans" => "评分", "en" => "Rating" }, icon: "checkmark") => draft.sort.set(LibrarySort::Rating)
            }
        }
    }
}

/// The phone's sort in the header's group: the same menu as an icon.
#[component]
pub fn library_sort_round(#[session] draft: LibraryDraft) -> impl View {
    ui! {
        if draft.sort == LibrarySort::Recent {
            Menu(id: sort_phone_recent, label: tr! { "zh-Hans" => "排序", "en" => "Sort" }, icon: "arrow.up.arrow.down", style: icon) {
                MenuItem(id: recent_phone_recent, label: tr! { "zh-Hans" => "最近观看", "en" => "Recently watched" }, icon: "checkmark") => draft.sort.set(LibrarySort::Recent)
                MenuItem(id: name_phone_recent, label: tr! { "zh-Hans" => "名称（拼音）", "en" => "Name (pinyin)" }) => draft.sort.set(LibrarySort::Name)
                MenuItem(id: rating_phone_recent, label: tr! { "zh-Hans" => "评分", "en" => "Rating" }) => draft.sort.set(LibrarySort::Rating)
            }
        } else if draft.sort == LibrarySort::Name {
            Menu(id: sort_phone_name, label: tr! { "zh-Hans" => "排序", "en" => "Sort" }, icon: "arrow.up.arrow.down", style: icon) {
                MenuItem(id: recent_phone_name, label: tr! { "zh-Hans" => "最近观看", "en" => "Recently watched" }) => draft.sort.set(LibrarySort::Recent)
                MenuItem(id: name_phone_name, label: tr! { "zh-Hans" => "名称（拼音）", "en" => "Name (pinyin)" }, icon: "checkmark") => draft.sort.set(LibrarySort::Name)
                MenuItem(id: rating_phone_name, label: tr! { "zh-Hans" => "评分", "en" => "Rating" }) => draft.sort.set(LibrarySort::Rating)
            }
        } else {
            Menu(id: sort_phone_rating, label: tr! { "zh-Hans" => "排序", "en" => "Sort" }, icon: "arrow.up.arrow.down", style: icon) {
                MenuItem(id: recent_phone_rating, label: tr! { "zh-Hans" => "最近观看", "en" => "Recently watched" }) => draft.sort.set(LibrarySort::Recent)
                MenuItem(id: name_phone_rating, label: tr! { "zh-Hans" => "名称（拼音）", "en" => "Name (pinyin)" }) => draft.sort.set(LibrarySort::Name)
                MenuItem(id: rating_phone_rating, label: tr! { "zh-Hans" => "评分", "en" => "Rating" }, icon: "checkmark") => draft.sort.set(LibrarySort::Rating)
            }
        }
    }
}

#[component]
pub fn library_filter(
    value: LibraryFilter,
    count: i64,
    #[session] draft: LibraryDraft,
) -> impl View {
    ui! {
        if draft.filter == value {
            Button(id: selected, label: tr! { "zh-Hans" => "{name} {count}", "en" => "{name} {count}"; name: value, count: count }, icon: "checkmark", prominence: prominent, shape: capsule, height: if window.tv { 40 } else { 30 }, variant: library_chip) => draft.filter.set(value)
        } else {
            Button(id: select, label: tr! { "zh-Hans" => "{name} {count}", "en" => "{name} {count}"; name: value, count: count }, shape: capsule, height: if window.tv { 40 } else { 30 }, variant: library_chip) => draft.filter.set(value)
        }
    }
}

#[component]
pub fn library_poster(
    library: library::Client,
    poster: library::LibraryPoster,
    show_details: bool,
) -> impl View {
    ui! {
        Column(gap: 0, appear: library_card_in, disappear: library_card_out) {
            Button(id: open, label: poster.title, variant: poster) {
                Stack(align: bottom_leading) {
                    Image(source: poster.poster, aspect: 0.6666666667, fit: cover, label: poster.title)
                    if poster.watching {
                        Rail {
                            Canvas(from: 0, to: 1, height: 4) {
                                Shape(kind: rect, fill: text.primary, opacity: 0.18)
                            }
                            Canvas(from: 0, to: poster.progress, height: 4) {
                                Shape(kind: rect, fill: accent)
                            }
                        }
                    }
                }
            } => library.open(poster.id)
            if show_details {
                Column(gap: 0) {
                    Canvas(height: 8)
                    Text(content: poster.title, style: headline, lines: 2, line_height: 19)
                    Canvas(height: 2)
                    if poster.watching {
                        Text(content: tr! { "zh-Hans" => "看到 {watched} / {episodes}", "en" => "Watched {watched} / {episodes}"; watched: poster.watched, episodes: poster.episodes }, style: caption, emphasis: secondary, line_height: 15)
                    } else if poster.watched == 0 {
                        Text(content: tr! { "zh-Hans" => "未看", "en" => "Unwatched" }, style: caption, emphasis: secondary, line_height: 15)
                    } else {
                        Text(content: tr! { "zh-Hans" => "看完", "en" => "Finished" }, style: caption, emphasis: secondary, line_height: 15)
                    }
                }
            }
        }
    }
}

/// The home page's "recently added" (`.x-sechd` over `.h-shelf` on the
/// desktop, 124-point cards 18 apart with their state; on the phone an
/// `.x-hrow` of 108-point cards 12 apart, the titles alone).
#[component]
pub fn library_recent(library: library::Client) -> impl View {
    ui! {
        if window.compact {
            Column(gap: 0) {
                Column(gap: 0, padding_x: 16) {
                    Canvas(height: 30)
                    Row(gap: 10, align: baseline) {
                        Text(content: tr! { "zh-Hans" => "最近添加", "en" => "Recently added" }, style: title3)
                        Spacer
                        Button(id: recent_all, label: tr! { "zh-Hans" => "全部", "en" => "All" }) {
                            Text(content: tr! { "zh-Hans" => "全部 ›", "en" => "All ›" }, font_size: 13, emphasis: secondary)
                        } => pages.library.show()
                    }
                    Canvas(height: 12)
                }
                Row(gap: 0, align: top) {
                    Canvas(width: 16)
                    ScrollView(axis: horizontal, indicators: hidden) {
                        Row(gap: 0, align: top) {
                            Row(gap: 12, align: top) {
                                ForEach(items: library.recent, key: id) |poster| {
                                    Column(gap: 0, width: 108) {
                                        library_card(library: library, poster: poster, status: false)
                                    }
                                }
                            }
                            Canvas(width: 16)
                        }
                    }
                }
            }
        } else {
            Column(gap: 0) {
                Canvas(height: 30)
                Row(gap: 10, align: baseline) {
                    Text(content: tr! { "zh-Hans" => "媒体库 · 最近添加", "en" => "Library · Recently added" }, style: title3)
                    Spacer
                    Button(id: recent_all_desktop, label: tr! { "zh-Hans" => "全部 ›", "en" => "All ›" }, prominence: plain) => pages.library.show()
                }
                Canvas(height: 12)
                ScrollView(axis: horizontal, indicators: hidden) {
                    Row(gap: 18, align: top, padding_y: 2) {
                        ForEach(items: library.recent, key: id) |poster| {
                            Column(gap: 0, width: 124) {
                                library_card(library: library, poster: poster, status: true)
                            }
                        }
                    }
                }
            }
        }
    }
}

/// `pcard`: the poster, with its progress while it is being watched, the
/// title in two lines at most (14 / 19) and, when asked, its state
/// (11.5 / 15).
#[component]
pub fn library_card(library: library::Client, poster: library::LibraryPoster, status: bool) -> impl View {
    ui! {
        Column(gap: 0) {
            Button(id: open_recent, label: poster.title, variant: poster) {
                Stack(align: bottom_leading) {
                    Image(source: poster.poster, aspect: 0.6666666667, fit: cover, corner: poster, label: poster.title)
                    if poster.watching {
                        Rail {
                            Canvas(from: 0, to: 1, height: 4) {
                                Shape(kind: rect, fill: text.primary, opacity: 0.18)
                            }
                            Canvas(from: 0, to: poster.progress, height: 4) {
                                Shape(kind: rect, fill: accent)
                            }
                        }
                    }
                }
            } => library.open(poster.id)
            Canvas(height: 8)
            Text(content: poster.title, style: headline, lines: 2, line_height: 19)
            if status {
                Canvas(height: 2)
                if poster.watching {
                    Text(content: tr! { "zh-Hans" => "看到 {watched} / {episodes}", "en" => "Watched {watched} / {episodes}"; watched: poster.watched, episodes: poster.episodes }, style: caption, emphasis: secondary, line_height: 15)
                } else if poster.watched == 0 {
                    Text(content: tr! { "zh-Hans" => "未看", "en" => "Unwatched" }, style: caption, emphasis: secondary, line_height: 15)
                } else {
                    Text(content: tr! { "zh-Hans" => "看完", "en" => "Finished" }, style: caption, emphasis: secondary, line_height: 15)
                }
            }
        }
    }
}

/// Library remains usable when the optional anime module is disabled.
#[component]
pub fn library_preview(library: library::Client) -> impl View {
    ui! {
        ScrollView {
            Column(gap: 24, padding: 32) {
                Button(id: back, label: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, icon: "chevron.left", prominence: plain) => library.close()
                Row(gap: 36, align: top) {
                    Image(source: library.show.poster, width: 220, height: 330, fit: cover, corner: poster, label: library.show.title)
                    Column(gap: 14) {
                        Text(content: library.show.title, style: title)
                        Text(content: tr! { "zh-Hans" => "共 {episodes} 话 · 看到第 {watched} 话", "en" => "{episodes} episodes · {watched} watched"; episodes: library.show.episodes, watched: library.show.watched }, emphasis: secondary)
                    }
                }
            }
        }
    }
}
