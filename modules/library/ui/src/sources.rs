//! The library's sources (`libSrcHTML`, `dirHTML` and `matchHTML` in
//! design/next-screens.js): the folders the library scans with the videos
//! not recognized under them; a folder opened level by level, each video
//! saying what it was matched to; and matching, one video or a folder's.
use bern::prelude::*;
use crate::{library_tabs, library_tabs_props, LibraryDraft};
use erika_library_protocol::library;

#[component]
pub fn library_sources_page(library: library::Client) -> impl View {
    ui! {
        ScrollView(appear: page_in) {
            Column(gap: 0, padding_x: if window.compact { 16.0 } else if window.tv { 56.0 } else if window.medium { 24.0 } else { 32.0 }, padding_y: 6) {
                if library.matching.open {
                    library_match(library: library)
                } else if library.browsing {
                    library_folder(library: library)
                } else {
                    library_source_list(library: library)
                }
                Canvas(height: 48)
            }
        }
    }
}

/// A hairline between the rows of a group.
#[component]
pub fn library_hairline() -> impl View {
    ui! {
        Canvas(height: 0.5) {
            Shape(kind: rect, fill: control.border, opacity: 0.55)
        }
    }
}

/// The sources (`.c-list` of `.c-li`), the videos not recognized, and a
/// note on how sources add up.
#[component]
pub fn library_source_list(library: library::Client, #[session] draft: LibraryDraft) -> impl View {
    ui! {
        Column(gap: 0) {
            if window.compact {
                Canvas(height: 7)
                Row(gap: 8, height: 48, align: center) {
                    Text(content: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, style: large_title)
                    Spacer
                    IconButton(id: add_folder_phone, icon: "plus", label: tr! { "zh-Hans" => "添加文件夹", "en" => "Add folder" }, pick: draft.folder, files: "folder") => library.add_folder(draft.folder)
                }
                Canvas(height: 2)
                Row(gap: 10, align: center) {
                    library_tabs()
                    Spacer
                }
            } else {
                Row(gap: 12, align: center) {
                    Text(content: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, style: large_title)
                    library_tabs()
                    Spacer
                    // A television picks no files; its sources come from the
                    // other devices' library.
                    if !window.tv {
                        Button(id: add_folder, label: tr! { "zh-Hans" => "添加文件夹", "en" => "Add folder" }, icon: "plus", shape: capsule, height: 32, prominence: prominent, pick: draft.folder, files: "folder") => library.add_folder(draft.folder)
                    }
                }
            }
            Canvas(height: 18)
            if library.has_sources {
                Stack(variant: row_group) {
                    Column(gap: 0, padding: 4) {
                        ForEach(items: library.sources, key: id) |row| {
                            Button(id: open_source, label: row.name, variant: row) {
                                Row(gap: 12, align: center) {
                                    Stack(variant: setting_icon, width: 30, height: 30, align: center) {
                                        Label(title: row.name, icon: "folder", display: icon, style: body)
                                    }
                                    Column(gap: 2) {
                                        Text(content: row.name, style: headline, lines: 1)
                                        Text(content: tr! { "zh-Hans" => "本地文件夹 · {at}", "en" => "Folder · {at}"; at: row.location }, style: caption, emphasis: secondary, lines: 1)
                                    }
                                    Spacer
                                    Text(content: row.status, style: caption, emphasis: secondary, lines: 1)
                                    Label(title: tr! { "zh-Hans" => "打开", "en" => "Open" }, icon: "chevron.right", display: icon, style: caption)
                                }
                            } => library.open_source(row.id)
                        }
                    }
                }
            } else {
                Column(gap: 4, padding_y: 8) {
                    Text(content: tr! { "zh-Hans" => "还没有来源。", "en" => "No sources yet." }, style: title3)
                    Text(content: tr! { "zh-Hans" => "添加放视频的文件夹，Erika 会扫描里面的视频，弹弹play 认出的番列在「番剧」里；以后有新文件，重新扫描只读变了的。", "en" => "Add a folder of videos: Erika scans it, and the shows dandanplay recognizes appear under Shows. Later scans only read what changed." }, emphasis: secondary, lines: 3)
                }
            }
            if library.has_sources {
                Canvas(height: 26)
                Text(content: tr! { "zh-Hans" => "没认出的文件 · {count}", "en" => "Not recognized · {count}"; count: library.unmatched_count }, style: title3)
                Canvas(height: 12)
                if library.unmatched_count > 0 {
                    Stack(variant: row_group) {
                        Column(gap: 0, padding: 4) {
                            ForEach(items: library.unmatched, key: key) |file| {
                                library_file_row(library: library, file: file, place: true)
                            }
                        }
                    }
                } else {
                    Text(content: tr! { "zh-Hans" => "都认出来了。", "en" => "Everything is recognized." }, emphasis: secondary)
                }
                Canvas(height: 14)
                Text(content: tr! { "zh-Hans" => "同一个文件在几个来源里都有，只算一份。番剧墙不分来源，要只看某个来源，在「番剧」里筛选。", "en" => "A file in several sources counts once. The wall shows every source; filter it under Shows to see one." }, style: caption, emphasis: secondary, lines: 3)
            }
        }
    }
}

/// A video: what it is, and matching it (`dirHTML`'s `file`).
#[component]
pub fn library_file_row(library: library::Client, file: library::LibraryFile, place: bool) -> impl View {
    ui! {
        Row(gap: 12, align: center, padding: 6) {
            Stack(width: 30, height: 30, align: center) {
                Label(title: file.name, icon: "film", display: icon, style: body)
            }
            Column(gap: 2) {
                Text(content: file.name, lines: 1)
                if place {
                    Text(content: file.place, style: caption, emphasis: secondary, lines: 1)
                } else {
                    Text(content: file.note, style: caption, emphasis: secondary, lines: 1)
                }
            }
            Spacer
            if file.matched {
                Menu(id: file_more, label: tr! { "zh-Hans" => "更多", "en" => "More" }, icon: "ellipsis", style: icon) {
                    MenuItem(id: rematch, label: tr! { "zh-Hans" => "重新匹配…", "en" => "Match again…" }, icon: "link") => library.assign(file.key)
                    MenuItem(id: unlink_file, label: tr! { "zh-Hans" => "解除匹配", "en" => "Unlink" }, icon: "xmark") => library.unlink(file.key)
                }
            } else if file.readable {
                Button(id: assign, label: tr! { "zh-Hans" => "指定剧集", "en" => "Choose episode" }, shape: capsule, height: if window.tv { 40 } else { 28 }) => library.assign(file.key)
            }
        }
    }
}

/// A source's folder opened (`dirHTML`): back, its name, matching it at
/// once and more; where it is; then its folders and its videos.
#[component]
pub fn library_folder(library: library::Client) -> impl View {
    ui! {
        Column(gap: 0) {
            Canvas(height: if window.compact { 7.0 } else { 0.0 })
            Row(gap: 10, align: center, height: if window.compact { 48.0 } else { 44.0 }) {
                IconButton(id: folder_up, icon: "chevron.left", label: tr! { "zh-Hans" => "返回", "en" => "Back" }) => library.go_up()
                if window.compact {
                    Text(content: library.folder.title, style: title2, lines: 1)
                } else {
                    Text(content: library.folder.title, style: large_title, lines: 1)
                }
                Spacer
                if library.folder.can_batch && !window.compact {
                    Button(id: batch_match, label: tr! { "zh-Hans" => "批量匹配", "en" => "Match all" }, icon: "link", shape: capsule, height: if window.tv { 46 } else { 32 }) => library.match_folder()
                }
                Menu(id: folder_more, label: tr! { "zh-Hans" => "更多", "en" => "More" }, icon: "ellipsis", style: icon) {
                    MenuItem(id: rescan, label: tr! { "zh-Hans" => "重新扫描", "en" => "Scan again" }, icon: "arrow.triangle.2.circlepath") => library.rescan(library.folder.source)
                    MenuItem(id: remove_source, label: tr! { "zh-Hans" => "移除这个来源", "en" => "Remove this source" }, icon: "trash") => library.remove_source(library.folder.source)
                }
            }
            // `.m-crumb`s: 来源 › the source › the folders.
            ScrollView(axis: horizontal, indicators: hidden) {
                Row(gap: 4, align: center) {
                    Button(id: crumb_sources, label: tr! { "zh-Hans" => "来源", "en" => "Sources" }, prominence: plain, variant: more_link) => library.close_source()
                    ForEach(items: library.folder.crumbs, key: depth) |crumb| {
                        Text(content: tr! { "zh-Hans" => "›", "en" => "›" }, style: caption, emphasis: tertiary)
                        Button(id: crumb, label: crumb.name, prominence: plain, variant: more_link) => library.go_to(crumb.depth)
                    }
                }
            }
            Text(content: tr! { "zh-Hans" => "本地文件夹 · {at}", "en" => "Folder · {at}"; at: library.folder.location }, style: caption, emphasis: secondary, lines: 1)
            Canvas(height: 16)
            if library.folder.can_batch && window.compact {
                Row(gap: 0) {
                    Button(id: batch_match_phone, label: tr! { "zh-Hans" => "批量匹配", "en" => "Match all" }, icon: "link", shape: capsule, height: 32) => library.match_folder()
                    Spacer
                }
                Canvas(height: 14)
            }
            if library.folder.missing {
                Text(content: tr! { "zh-Hans" => "找不到这个文件夹：它被移动了，或者所在的磁盘没有接上。接上以后在「更多」里重新扫描。", "en" => "This folder can't be found: it moved, or its disk isn't connected. Connect it and scan again from More." }, emphasis: secondary, lines: 3)
            } else if library.folder.empty {
                Text(content: tr! { "zh-Hans" => "这里没有视频。", "en" => "No videos here." }, emphasis: secondary)
            } else {
                Stack(variant: row_group) {
                    Column(gap: 0, padding: 4) {
                        ForEach(items: library.folder.dirs, key: name) |dir| {
                            Button(id: open_dir, label: dir.name, variant: row) {
                                Row(gap: 12, align: center) {
                                    Stack(width: 30, height: 30, align: center) {
                                        Label(title: dir.name, icon: "folder", display: icon, style: body)
                                    }
                                    Column(gap: 2) {
                                        Text(content: dir.name, lines: 1)
                                        if dir.unmatched > 0 {
                                            Text(content: tr! { "zh-Hans" => "{files} 个文件 · {unmatched} 个没认出", "en" => "{files} files · {unmatched} not recognized"; files: dir.files, unmatched: dir.unmatched }, style: caption, emphasis: secondary)
                                        } else {
                                            Text(content: tr! { "zh-Hans" => "{files} 个文件", "en" => "{files} files"; files: dir.files }, style: caption, emphasis: secondary)
                                        }
                                    }
                                    Spacer
                                    Label(title: tr! { "zh-Hans" => "打开", "en" => "Open" }, icon: "chevron.right", display: icon, style: caption)
                                }
                            } => library.open_dir(dir.name)
                        }
                        ForEach(items: library.folder.files, key: key) |file| {
                            library_file_row(library: library, file: file, place: false)
                        }
                    }
                }
            }
        }
    }
}

/// Matching (`matchHTML`): the anime and how the videos go to its
/// episodes, the videos with the episode each goes to, and confirming.
#[component]
pub fn library_match(library: library::Client, #[session] draft: LibraryDraft) -> impl View {
    ui! {
        Column(gap: 0) {
            Canvas(height: if window.compact { 7.0 } else { 0.0 })
            Row(gap: 10, align: center, height: if window.compact { 48.0 } else { 44.0 }) {
                IconButton(id: match_back, icon: "chevron.left", label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }) => library.match_cancel()
                if library.matching.single {
                    Text(content: tr! { "zh-Hans" => "重新匹配", "en" => "Match again" }, style: large_title)
                } else {
                    Text(content: tr! { "zh-Hans" => "批量匹配", "en" => "Match all" }, style: large_title)
                }
                Spacer
            }
            if library.matching.single {
                Text(content: library.matching.place, style: caption, emphasis: secondary, lines: 2)
            } else {
                Text(content: tr! { "zh-Hans" => "{place} · {count} 个文件", "en" => "{place} · {count} files"; place: library.matching.place, count: library.matching.all_count }, style: caption, emphasis: secondary, lines: 2)
            }
            Canvas(height: 18)
            // `.m-ctl`: the anime, then the episode or how the videos go.
            Row(gap: 18, align: center) {
                Row(gap: 8, align: center) {
                    Text(content: tr! { "zh-Hans" => "番剧", "en" => "Show" }, emphasis: secondary)
                    if library.matching.anime > 0 {
                        PopoverButton(id: match_anime_chosen, label: library.matching.anime_title, presented: draft.anime_open) {
                            library_match_search(library: library)
                        }
                    } else {
                        PopoverButton(id: match_anime_search, label: tr! { "zh-Hans" => "搜索番剧", "en" => "Search shows" }, presented: draft.anime_open) {
                            library_match_search(library: library)
                        }
                    }
                }
                if library.matching.single {
                    ForEach(items: library.matching.rows, key: key) |row| {
                        Row(gap: 6, align: center) {
                            Text(content: tr! { "zh-Hans" => "集数 第", "en" => "Episode" }, emphasis: secondary)
                            Row(width: 72) {
                                TextInput(id: single_number, label: tr! { "zh-Hans" => "集数", "en" => "Episode" }, label_hidden: true, value: draft.number, prompt: tr! { "zh-Hans" => "{number}", "en" => "{number}"; number: row.number }, submit: clear) => library.match_number(row.key, draft.number)
                            }
                            Text(content: tr! { "zh-Hans" => "话", "en" => "" }, emphasis: secondary)
                        }
                    }
                } else {
                    Row(gap: 6, align: center) {
                        Text(content: tr! { "zh-Hans" => "配法", "en" => "By" }, emphasis: secondary)
                        if library.matching.by_order {
                            Button(id: by_number, label: tr! { "zh-Hans" => "按文件名里的集数", "en" => "Numbers in names" }, shape: chip, height: 30, variant: library_chip) => library.match_by_order(false)
                            Button(id: by_order_on, label: tr! { "zh-Hans" => "按顺序", "en" => "Order" }, icon: "checkmark", prominence: prominent, shape: chip, height: 30, variant: library_chip) => library.match_by_order(true)
                        } else {
                            Button(id: by_number_on, label: tr! { "zh-Hans" => "按文件名里的集数", "en" => "Numbers in names" }, icon: "checkmark", prominence: prominent, shape: chip, height: 30, variant: library_chip) => library.match_by_order(false)
                            Button(id: by_order, label: tr! { "zh-Hans" => "按顺序", "en" => "Order" }, shape: chip, height: 30, variant: library_chip) => library.match_by_order(true)
                        }
                    }
                }
            }
            if !library.matching.single {
                Canvas(height: 12)
                if library.matching.by_order {
                    Row(gap: 6, align: center) {
                        Text(content: tr! { "zh-Hans" => "从第", "en" => "From episode" }, emphasis: secondary)
                        Row(width: 72) {
                            TextInput(id: match_start, label: tr! { "zh-Hans" => "从第几话开始", "en" => "First episode" }, label_hidden: true, value: draft.start, prompt: tr! { "zh-Hans" => "{start}", "en" => "{start}"; start: library.matching.start }, submit: clear) => library.match_start(draft.start)
                        }
                        Text(content: tr! { "zh-Hans" => "话开始", "en" => "on" }, emphasis: secondary)
                    }
                } else if library.matching.lowest > 0 {
                    Row(gap: 6, align: center) {
                        Text(content: tr! { "zh-Hans" => "对齐：文件名里的 {lowest} 是第", "en" => "Align: {lowest} in the names is episode"; lowest: library.matching.lowest }, emphasis: secondary)
                        Row(width: 72) {
                            TextInput(id: match_align, label: tr! { "zh-Hans" => "对齐到第几话", "en" => "Align to episode" }, label_hidden: true, value: draft.align, prompt: tr! { "zh-Hans" => "{episode}", "en" => "{episode}"; episode: library.matching.align_to }, submit: clear) => library.match_align(draft.align)
                        }
                        Text(content: tr! { "zh-Hans" => "话", "en" => "" }, emphasis: secondary)
                    }
                }
                Canvas(height: 14)
                // `.m-chips`: all or the ones worth a look, and the tally.
                Row(gap: 8, align: center) {
                    if library.matching.odd_only {
                        Button(id: match_all, label: tr! { "zh-Hans" => "全部 {count}", "en" => "All {count}"; count: library.matching.all_count }, shape: chip, height: 30, variant: library_chip) => library.match_odd(false)
                        Button(id: match_odd_on, label: tr! { "zh-Hans" => "要看一下 {count}", "en" => "Worth a look {count}"; count: library.matching.odd_count }, icon: "checkmark", prominence: prominent, shape: chip, height: 30, variant: library_chip) => library.match_odd(true)
                    } else {
                        Button(id: match_all_on, label: tr! { "zh-Hans" => "全部 {count}", "en" => "All {count}"; count: library.matching.all_count }, icon: "checkmark", prominence: prominent, shape: chip, height: 30, variant: library_chip) => library.match_odd(false)
                        Button(id: match_odd, label: tr! { "zh-Hans" => "要看一下 {count}", "en" => "Worth a look {count}"; count: library.matching.odd_count }, shape: chip, height: 30, variant: library_chip) => library.match_odd(true)
                    }
                    Text(content: tr! { "zh-Hans" => "{on} 个会匹配 · {skipped} 个跳过 · {bad} 个要先处理", "en" => "{on} to match · {skipped} skipped · {bad} to fix"; on: library.matching.on_count, skipped: library.matching.skipped_count, bad: library.matching.bad_count }, style: caption, emphasis: secondary, lines: 1)
                    Spacer
                }
            }
            Canvas(height: 14)
            Stack(variant: row_group) {
                Column(gap: 0, padding: 4) {
                    ForEach(items: library.matching.shown, key: key) |row| {
                        library_match_row(library: library, row: row, single: library.matching.single)
                    }
                }
            }
            Canvas(height: 12)
            if library.matching.single {
                Text(content: tr! { "zh-Hans" => "弹幕跟着换成新那一集的；观看进度和弹幕偏移记在文件上，原样留着。这个文件在别的来源里的副本也一起改。", "en" => "Danmaku follow the new episode; progress and the danmaku offset stay with the file. Copies in other sources change too." }, style: caption, emphasis: secondary, lines: 3)
            } else if library.matching.by_order {
                Text(content: tr! { "zh-Hans" => "按文件名排好顺序，从起始那一话开始依次往后配。个别不对的点集数直接改，左边的勾可以跳过一个文件。确定后一次写入，出错就全部不改。", "en" => "In the order of their names, from the first episode on. Click an episode to change it, or the tick to skip a file. Everything is saved at once, or nothing." }, style: caption, emphasis: secondary, lines: 3)
            } else {
                Text(content: tr! { "zh-Hans" => "每个文件按文件名里的集数配到那一话；第二季接着第一季编号时改「对齐」。同一话的几个文件算几个版本。个别不对的点集数直接改，左边的勾可以跳过一个文件。这些文件在别的来源里的副本也一起改，确定后一次写入，出错就全部不改。", "en" => "Each file goes to the episode its name gives; when a second season goes on from the first, change Align. Files of one episode are its versions. Click an episode to change it, or the tick to skip a file. Copies in other sources change too; everything is saved at once, or nothing." }, style: caption, emphasis: secondary, lines: 4)
            }
            Canvas(height: 16)
            Row(gap: 10, align: center) {
                if library.matching.single {
                    Button(id: match_confirm_one, label: tr! { "zh-Hans" => "确定", "en" => "Done" }, shape: capsule, height: 32, prominence: prominent, enabled: library.matching.ready) => library.match_confirm()
                } else {
                    Button(id: match_confirm_many, label: tr! { "zh-Hans" => "匹配 {count} 个文件", "en" => "Match {count} files"; count: library.matching.on_count }, shape: capsule, height: 32, prominence: prominent, enabled: library.matching.ready) => library.match_confirm()
                }
                if library.matching.linked {
                    Button(id: match_unlink, label: tr! { "zh-Hans" => "解除匹配", "en" => "Unlink" }, shape: capsule, height: 32) => library.match_unlink()
                }
                Button(id: match_cancel, label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }, shape: capsule, height: 32) => library.match_cancel()
                Spacer
            }
        }
    }
}

/// The anime to match to, searched for on dandanplay (`manime`).
#[component]
pub fn library_match_search(library: library::Client, #[session] draft: LibraryDraft) -> impl View {
    ui! {
        Column(gap: 8, width: 340, padding: 8) {
            TextInput(id: match_search, label: tr! { "zh-Hans" => "搜索番剧", "en" => "Search shows" }, label_hidden: true, value: draft.search, prompt: tr! { "zh-Hans" => "搜索弹弹play 的番剧，回车搜索", "en" => "Search dandanplay, then press Return" }) => library.match_search(draft.search)
            if library.matching.searching {
                Text(content: tr! { "zh-Hans" => "正在搜索…", "en" => "Searching…" }, style: caption, emphasis: secondary)
            } else if library.matching.search_failed {
                Text(content: tr! { "zh-Hans" => "搜索不了：先在 设置 › 账号 里登录弹弹play。", "en" => "Can't search: sign in to dandanplay under Settings › Account." }, style: caption, emphasis: secondary, lines: 2)
            }
            Column(gap: 0) {
                ForEach(items: library.matching.results, key: id) |anime| {
                    Button(id: match_result, label: anime.title, variant: row) {
                        Row(gap: 8, align: center) {
                            if anime.chosen {
                                Label(title: tr! { "zh-Hans" => "已选", "en" => "Chosen" }, icon: "checkmark", display: icon, style: caption)
                            } else {
                                Canvas(width: 12, height: 12)
                            }
                            Text(content: anime.title, lines: 1)
                            Spacer
                            if anime.episodes > 0 {
                                Text(content: tr! { "zh-Hans" => "{count} 话", "en" => "{count} eps"; count: anime.episodes }, style: caption, emphasis: secondary)
                            }
                        }
                    } => library.match_anime(anime.id)
                }
            }
        }
    }
}

/// A video being matched: matched or skipped, its name and what to know
/// of it, and the episode it goes to, typed over in place.
#[component]
pub fn library_match_row(library: library::Client, row: library::MatchRow, single: bool, #[session] draft: LibraryDraft) -> impl View {
    ui! {
        Row(gap: 12, align: center, padding: 6) {
            if single {
                Stack(width: 30, height: 30, align: center) {
                    Label(title: row.name, icon: "film", display: icon, style: body)
                }
            } else if row.on {
                IconButton(id: skip_row, icon: "checkmark", label: tr! { "zh-Hans" => "跳过这个文件", "en" => "Skip this file" }) => library.match_toggle(row.key)
            } else {
                IconButton(id: take_row, icon: "circle", label: tr! { "zh-Hans" => "匹配这个文件", "en" => "Match this file" }) => library.match_toggle(row.key)
            }
            Column(gap: 2) {
                Text(content: row.name, lines: 1)
                Text(content: row.note, style: caption, emphasis: secondary, lines: 1)
            }
            Spacer
            if !single {
                if !row.on {
                    Text(content: tr! { "zh-Hans" => "跳过", "en" => "Skipped" }, style: caption, emphasis: tertiary)
                } else if row.editing {
                    Row(width: 96) {
                        TextInput(id: row_number, label: tr! { "zh-Hans" => "集数", "en" => "Episode" }, label_hidden: true, value: draft.number, prompt: tr! { "zh-Hans" => "第 {number} 话", "en" => "Episode {number}"; number: row.number }, submit: clear) => library.match_number(row.key, draft.number)
                    }
                } else if row.number > 0 {
                    Button(id: edit_row, label: tr! { "zh-Hans" => "→ 第 {number} 话", "en" => "→ Episode {number}"; number: row.number }, prominence: plain) => library.match_edit(row.key)
                } else {
                    Button(id: fill_row, label: tr! { "zh-Hans" => "→ 填集数", "en" => "→ Enter episode" }, prominence: plain) => library.match_edit(row.key)
                }
            }
        }
    }
}
