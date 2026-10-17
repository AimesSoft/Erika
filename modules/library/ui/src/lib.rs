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
                Column(gap: 0, padding_x: if window.compact { 16.0 } else if window.medium { 24.0 } else { 32.0 }, padding_y: 6, appear: page_in) {
                    Adaptive {
                        Layout(size: expanded) {
                            Row(gap: 12, align: center) {
                                Text(content: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, style: large_title)
                                library_sources(app: app)
                                Spacer
                                library_zoom()
                                library_sort()
                                Button(id: manage, label: tr! { "zh-Hans" => "管理", "en" => "Manage" }, icon: "folder", shape: capsule, height: 32) => library.manage()
                            }
                        }
                        Layout(size: compact) {
                            if window.compact {
                                Column(gap: 12) {
                                    Row(gap: 12, align: center) {
                                        Text(content: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, style: large_title)
                                        Spacer
                                        Button(id: manage_compact, label: tr! { "zh-Hans" => "管理", "en" => "Manage" }, icon: "folder", shape: capsule, height: 32) => library.manage()
                                    }
                                    library_sources(app: app)
                                    Row(gap: 12, align: center) {
                                        library_sort()
                                        Spacer
                                        library_zoom()
                                    }
                                }
                            } else {
                                Column(gap: 12) {
                                    Row(gap: 12, align: center) {
                                        Text(content: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, style: large_title)
                                        Spacer
                                        library_sort()
                                        Button(id: manage_small, label: tr! { "zh-Hans" => "管理", "en" => "Manage" }, icon: "folder", shape: capsule, height: 32) => library.manage()
                                    }
                                    Row(gap: 12, align: center) {
                                        library_sources(app: app)
                                        Spacer
                                        library_zoom()
                                    }
                                }
                            }
                        }
                    }
                    Canvas(height: 14)
                    if window.compact {
                        Flow(gap: 8) {
                            ForEach(items: listing.filters, key: id) |chip| {
                                library_filter(value: chip.value, count: chip.count)
                            }
                        }
                    } else {
                        Row(gap: 8, align: center) {
                            ForEach(items: listing.filters, key: id) |chip| {
                                library_filter(value: chip.value, count: chip.count)
                            }
                            Spacer
                        }
                    }
                    Canvas(height: 18)
                    library_wall(library: library, listing: listing)
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
        Pinch(id: zoom_wall, label: tr! { "zh-Hans" => "缩放海报墙", "en" => "Zoom poster wall" }, value: draft.poster_size, min: 84, max: 200) {
            ScrollView(position: draft.position, arrival: nudge) {
                Row(gap: 8, align: top) {
                    Grid(min_width: draft.poster_size, gap: if draft.poster_size < 110.0 { 10.0 } else { 20.0 }, row_gap: if draft.poster_size < 110.0 { 10.0 } else { 16.0 }, stagger: 0.016, animate: smooth) {
                        ForEach(items: listing.posters, key: id, reflow: smooth) |poster| {
                            library_poster(library: library, poster: poster, show_details: draft.poster_size >= 110.0)
                        }
                    }
                    Column(gap: 1, width: 20) {
                        ForEach(items: listing.letters, key: letter) |letter| {
                            Button(id: index, label: letter.letter, enabled: letter.target > 0, variant: library_index) {
                                Column(gap: 0, width: 20, height: 18, align: center) {
                                    Text(content: letter.letter, style: caption2, font_size: 10.5, emphasis: secondary)
                                }
                            } => draft.position.set(letter.target)
                        }
                    }
                }
                Canvas(height: 48)
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
        Row(gap: 8, width: if window.compact { 156 } else { 188 }, align: center) {
            IconButton(id: smaller, icon: "square.grid.3x3", label: tr! { "zh-Hans" => "缩小海报", "en" => "Smaller posters" }, prominence: plain, shortcut: "cmd+-") => draft.poster_size.set(if draft.poster_size > 188.0 { 188.0 } else if draft.poster_size > 176.0 { 176.0 } else if draft.poster_size > 164.0 { 164.0 } else if draft.poster_size > 152.0 { 152.0 } else if draft.poster_size > 140.0 { 140.0 } else if draft.poster_size > 128.0 { 128.0 } else if draft.poster_size > 116.0 { 116.0 } else if draft.poster_size > 104.0 { 104.0 } else if draft.poster_size > 92.0 { 92.0 } else { 84.0 })
            Row(width: if window.compact { 88 } else { 120 }) {
                Slider(id: poster_size, label: tr! { "zh-Hans" => "海报大小", "en" => "Poster size" }, value: draft.poster_size, min: 84, max: 200, label_hidden: true)
            }
            IconButton(id: larger, icon: "square.grid.2x2", label: tr! { "zh-Hans" => "放大海报", "en" => "Larger posters" }, prominence: plain, shortcut: "cmd+=") => draft.poster_size.set(if draft.poster_size < 96.0 { 96.0 } else if draft.poster_size < 108.0 { 108.0 } else if draft.poster_size < 120.0 { 120.0 } else if draft.poster_size < 132.0 { 132.0 } else if draft.poster_size < 144.0 { 144.0 } else if draft.poster_size < 156.0 { 156.0 } else if draft.poster_size < 168.0 { 168.0 } else if draft.poster_size < 180.0 { 180.0 } else if draft.poster_size < 192.0 { 192.0 } else { 200.0 })
        }
    }
}

#[component]
pub fn library_sort(#[session] draft: LibraryDraft) -> impl View {
    ui! {
        if draft.sort == LibrarySort::Recent {
            Menu(id: sort_recent, label: draft.sort, icon: "arrow.up.arrow.down", shape: capsule, height: 32) {
                MenuItem(id: recent_recent, label: tr! { "zh-Hans" => "最近观看", "en" => "Recently watched" }, icon: "checkmark") => draft.sort.set(LibrarySort::Recent)
                MenuItem(id: name_recent, label: tr! { "zh-Hans" => "名称（拼音）", "en" => "Name (pinyin)" }) => draft.sort.set(LibrarySort::Name)
                MenuItem(id: rating_recent, label: tr! { "zh-Hans" => "评分", "en" => "Rating" }) => draft.sort.set(LibrarySort::Rating)
            }
        } else if draft.sort == LibrarySort::Name {
            Menu(id: sort_name, label: draft.sort, icon: "arrow.up.arrow.down", shape: capsule, height: 32) {
                MenuItem(id: recent_name, label: tr! { "zh-Hans" => "最近观看", "en" => "Recently watched" }) => draft.sort.set(LibrarySort::Recent)
                MenuItem(id: name_name, label: tr! { "zh-Hans" => "名称（拼音）", "en" => "Name (pinyin)" }, icon: "checkmark") => draft.sort.set(LibrarySort::Name)
                MenuItem(id: rating_name, label: tr! { "zh-Hans" => "评分", "en" => "Rating" }) => draft.sort.set(LibrarySort::Rating)
            }
        } else if draft.sort == LibrarySort::Rating {
            Menu(id: sort_rating, label: draft.sort, icon: "arrow.up.arrow.down", shape: capsule, height: 32) {
                MenuItem(id: recent_rating, label: tr! { "zh-Hans" => "最近观看", "en" => "Recently watched" }) => draft.sort.set(LibrarySort::Recent)
                MenuItem(id: name_rating, label: tr! { "zh-Hans" => "名称（拼音）", "en" => "Name (pinyin)" }) => draft.sort.set(LibrarySort::Name)
                MenuItem(id: rating_rating, label: tr! { "zh-Hans" => "评分", "en" => "Rating" }, icon: "checkmark") => draft.sort.set(LibrarySort::Rating)
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
            Button(id: selected, label: tr! { "zh-Hans" => "{name} {count}", "en" => "{name} {count}"; name: value, count: count }, icon: "checkmark", prominence: prominent, variant: library_chip) => draft.filter.set(value)
        } else {
            Button(id: select, label: tr! { "zh-Hans" => "{name} {count}", "en" => "{name} {count}"; name: value, count: count }, variant: library_chip) => draft.filter.set(value)
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
                    Text(content: poster.title, style: headline, lines: 2)
                    Canvas(height: 2)
                    if poster.watching {
                        Text(content: tr! { "zh-Hans" => "看到 {watched} / {episodes}", "en" => "Watched {watched} / {episodes}"; watched: poster.watched, episodes: poster.episodes }, style: caption, emphasis: secondary)
                    } else if poster.watched == 0 {
                        Text(content: tr! { "zh-Hans" => "未看", "en" => "Unwatched" }, style: caption, emphasis: secondary)
                    } else {
                        Text(content: tr! { "zh-Hans" => "看完", "en" => "Finished" }, style: caption, emphasis: secondary)
                    }
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
                    Image(source: library.show.poster, width: 220, height: 330, fit: cover, corner: 10, label: library.show.title)
                    Column(gap: 14) {
                        Text(content: library.show.title, style: title)
                        Text(content: tr! { "zh-Hans" => "共 {episodes} 话 · 看到第 {watched} 话", "en" => "{episodes} episodes · {watched} watched"; episodes: library.show.episodes, watched: library.show.watched }, emphasis: secondary)
                    }
                }
            }
        }
    }
}
