//! Downloads (`dlHTML` in design/next-screens.js): progress is the row's
//! background, with a two-point accent edge. Adding a task reads its
//! torrent first, as NipaPlay's downloader does, and asks where it goes.
use bern::prelude::*;
use erika_downloads_protocol::downloads::{self, DownloadTab};

#[bern::session]
pub struct DownloadsDraft {
    /// The magnet link typed, the .torrent file picked, and the folder
    /// picked to download into.
    pub link: String,
    pub torrent: MediaSource,
    pub place: MediaSource,
}

/// Mounted everywhere: the clock that reads progress while something
/// downloads, and what the downloads say.
#[component]
pub fn downloads_clock(downloads: downloads::Client) -> impl View {
    ui! {
        Stack {
            if downloads.adding.open {
                download_add(downloads: downloads)
            }
            Timer(interval: 1.0, enabled: downloads.running) => downloads.tick()
            if let Some(notice) = downloads.notice {
                Toast(content: notice, key: downloads.notice_revision, duration: 3)
            }
        }
    }
}

#[component]
pub fn downloads_page(downloads: downloads::Client) -> impl View {
    ui! {
        Column(gap: 0, padding_x: if window.compact { 16.0 } else if window.medium { 24.0 } else { 32.0 }, padding_y: 6, appear: page_in) {
            Adaptive {
                Layout(size: expanded) {
                    Row(gap: 12, align: center) {
                        Text(content: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, style: large_title)
                        download_tabs(downloads: downloads)
                        Spacer
                        if downloads.available {
                            Button(id: add, label: tr! { "zh-Hans" => "添加任务", "en" => "Add task" }, icon: "plus", shape: capsule, height: 32, prominence: prominent) => downloads.open_add()
                        }
                    }
                }
                Layout(size: compact) {
                    Column(gap: 12) {
                        Row(gap: 12, align: center) {
                            Text(content: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, style: large_title)
                            Spacer
                            if downloads.available {
                                IconButton(id: add_small, label: tr! { "zh-Hans" => "添加任务", "en" => "Add task" }, icon: "plus", prominence: plain) => downloads.open_add()
                            }
                        }
                        download_tabs(downloads: downloads)
                    }
                }
            }
            Canvas(height: 14)
            ScrollView {
                Column(gap: 14) {
                    if !downloads.available {
                        Text(content: tr! { "zh-Hans" => "这里还不能下载：浏览器和电视上没有下载器，在电脑或手机上下载。", "en" => "Downloads don't run here: the browser and the TV have no downloader. Download on a computer or phone." }, emphasis: secondary, lines: 3)
                    }
                    Stack(variant: row_group) {
                        Column(gap: 0, animate: smooth) {
                            if downloads.tab == DownloadTab::Active {
                                if downloads.active_count == 0 {
                                    Row(padding_x: 14, height: 44) {
                                        Text(content: tr! { "zh-Hans" => "没有正在下载的任务", "en" => "No downloads in progress" })
                                        Spacer
                                    }
                                }
                                ForEach(items: downloads.active, key: id, reflow: smooth) |task| {
                                    download_row(downloads: downloads, task: task)
                                }
                            } else {
                                if downloads.completed_count == 0 {
                                    Row(padding_x: 14, height: 44) {
                                        Text(content: tr! { "zh-Hans" => "还没有下载完成的文件", "en" => "No completed downloads yet" })
                                        Spacer
                                    }
                                }
                                ForEach(items: downloads.completed, key: id, reflow: smooth) |task| {
                                    download_row(downloads: downloads, task: task)
                                }
                            }
                        }
                    }
                    Row(gap: 12, align: center) {
                        Text(content: tr! { "zh-Hans" => "下载完成后加入媒体库", "en" => "Add finished downloads to the library" })
                        Spacer
                        Switch(id: to_library, label: tr! { "zh-Hans" => "下载完成后加入媒体库", "en" => "Add finished downloads to the library" }, label_hidden: true, style: switch, checked: downloads.to_library) => downloads.set_to_library(downloads.to_library)
                    }
                    Text(content: tr! { "zh-Hans" => "行的底色就是进度。下载完成后，下载到的文件夹会作为来源加入媒体库并扫描，认出的番出现在「番剧」里。", "en" => "Each row's background shows its progress. When a download finishes, its folder joins the library as a source and is scanned." }, style: caption, font_size: 11.5, emphasis: secondary, lines: 3)
                }
            }
            if downloads.deleting != 0 {
                Modal(width: 440, height: 200, cover: compact, appear: pop_in, disappear: fade) {
                    Column(gap: 12, padding: 24) {
                        Text(content: tr! { "zh-Hans" => "删除任务和文件？", "en" => "Delete the download and its files?" }, style: title3)
                        Text(content: tr! { "zh-Hans" => "将从列表移除“{title}”，并删除已下载文件。此操作不可撤销。", "en" => "“{title}” leaves the list and its downloaded files are deleted. This can't be undone."; title: downloads.deleting_title }, emphasis: secondary, lines: 3)
                        Spacer
                        Row(gap: 10, align: center) {
                            Spacer
                            Button(id: cancel_delete, label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }, shape: capsule, height: 32) => downloads.cancel_delete()
                            Button(id: confirm_delete, label: tr! { "zh-Hans" => "删除", "en" => "Delete" }, shape: capsule, height: 32, prominence: prominent) => downloads.delete()
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn download_tabs(downloads: downloads::Client) -> impl View {
    ui! {
        Row(width: 218) {
            Picker(id: tab, label: tr! { "zh-Hans" => "下载状态", "en" => "Download status" }, selected: downloads.tab, style: segmented, label_hidden: true) {
                PickerOption(value: DownloadTab::Active, label: tr! { "zh-Hans" => "进行中 {n}", "en" => "In progress {n}"; n: downloads.active_count })
                PickerOption(value: DownloadTab::Completed, label: tr! { "zh-Hans" => "已完成 {n}", "en" => "Completed {n}"; n: downloads.completed_count })
            } => downloads.set_tab(downloads.tab)
        }
    }
}

#[component]
pub fn download_row(downloads: downloads::Client, task: downloads::DownloadTask) -> impl View {
    ui! {
        Stack(align: bottom_leading, appear: page_in, disappear: fade) {
            Rail {
                Canvas(from: 0, to: task.progress, height: 52, animate: smooth) {
                    Shape(kind: rect, fill: fill.subtle)
                }
            }
            Column(gap: 0) {
                if !task.first { Divider }
                Row(gap: 12, padding_x: 14, height: 52, align: center) {
                    Stack(width: 30, height: 30) {
                        Canvas {
                            Shape(kind: rounded, corner: 8, fill: fill.strong)
                        }
                        if task.completed {
                            Label(title: tr! { "zh-Hans" => "已完成", "en" => "Completed" }, icon: "checkmark", display: icon)
                        } else if task.failed {
                            Label(title: tr! { "zh-Hans" => "出错了", "en" => "Failed" }, icon: "xmark", display: icon)
                        } else {
                            Label(title: tr! { "zh-Hans" => "下载", "en" => "Download" }, icon: "arrow.down", display: icon)
                        }
                    }
                    Column(gap: 1) {
                        if task.fetching {
                            if task.title == "" {
                                Text(content: tr! { "zh-Hans" => "新任务 · 正在获取元数据…", "en" => "New download · Fetching metadata…" }, font_size: 13, lines: 1)
                            } else {
                                Text(content: tr! { "zh-Hans" => "{title} · 正在获取元数据…", "en" => "{title} · Fetching metadata…"; title: task.title }, font_size: 13, lines: 1)
                            }
                        } else {
                            Text(content: task.title, font_size: 13, lines: 1)
                        }
                        if task.completed {
                            Text(content: tr! { "zh-Hans" => "{size} · 已完成", "en" => "{size} · Completed"; size: task.size }, style: caption, font_size: 11.5, emphasis: secondary, lines: 1)
                        } else if task.failed {
                            Text(content: tr! { "zh-Hans" => "{size} · {percent}% · 出错了：{error}", "en" => "{size} · {percent}% · Failed: {error}"; size: task.size, percent: task.percent, error: task.error }, style: caption, font_size: 11.5, emphasis: secondary, lines: 1)
                        } else if task.fetching {
                            Text(content: tr! { "zh-Hans" => "从网络上读取种子，可能要一会儿", "en" => "Reading the torrent from the network; this can take a while" }, style: caption, font_size: 11.5, emphasis: secondary, lines: 1)
                        } else if task.running {
                            Text(content: tr! { "zh-Hans" => "{size} · {percent}% · ↓ {speed} MB/s · ↑ {upload} MB/s", "en" => "{size} · {percent}% · ↓ {speed} MB/s · ↑ {upload} MB/s"; size: task.size, percent: task.percent, speed: task.speed, upload: task.upload }, style: caption, font_size: 11.5, emphasis: secondary, lines: 1)
                        } else {
                            Text(content: tr! { "zh-Hans" => "{size} · {percent}% · 已暂停", "en" => "{size} · {percent}% · Paused"; size: task.size, percent: task.percent }, style: caption, font_size: 11.5, emphasis: secondary, lines: 1)
                        }
                    }
                    Spacer
                    // A row still being read is no task yet.
                    if task.id > 0 {
                        download_actions(downloads: downloads, task: task)
                    }
                }
            }
            Rail {
                Canvas(from: 0, to: task.progress, height: 2, animate: smooth) {
                    Shape(kind: rect, fill: accent)
                }
            }
        }
    }
}

/// A task's actions: playing (a choice of its videos when it has several,
/// streaming while it downloads), pausing or resuming, and removing.
#[component]
pub fn download_actions(downloads: downloads::Client, task: downloads::DownloadTask) -> impl View {
    ui! {
        Row(gap: 6, align: center) {
            if task.playable {
                if task.single {
                    if task.completed {
                        Button(id: play, label: tr! { "zh-Hans" => "播放", "en" => "Play" }, icon: "play", shape: capsule, height: 32) => downloads.play(task.id, task.first_video)
                    } else {
                        IconButton(id: stream, label: tr! { "zh-Hans" => "边下边播", "en" => "Play while downloading" }, icon: "play", prominence: plain) => downloads.play(task.id, task.first_video)
                    }
                } else {
                    PopoverButton(id: choose_video, label: tr! { "zh-Hans" => "选择要播放的文件", "en" => "Choose a file to play" }, icon: "play") {
                        Column(gap: 2, width: 360, padding: 8) {
                            Text(content: tr! { "zh-Hans" => "选择要播放的文件", "en" => "Choose a file to play" }, style: caption, emphasis: secondary)
                            ScrollView {
                                Column(gap: 0) {
                                    ForEach(items: task.videos, key: index) |video| {
                                        Button(id: play_video, label: video.name, variant: row) {
                                            Row(gap: 8, align: center) {
                                                Text(content: video.name, lines: 1)
                                                Spacer
                                            }
                                        } => downloads.play(task.id, video.index)
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if !task.completed {
                if task.running {
                    IconButton(id: pause, label: tr! { "zh-Hans" => "暂停下载", "en" => "Pause download" }, icon: "pause", prominence: plain) => downloads.toggle(task.id)
                } else {
                    IconButton(id: resume, label: tr! { "zh-Hans" => "继续下载", "en" => "Resume download" }, icon: "arrow.down.circle", prominence: plain) => downloads.toggle(task.id)
                }
            }
            Menu(id: task_more, label: tr! { "zh-Hans" => "更多", "en" => "More" }, icon: "ellipsis", style: icon) {
                MenuItem(id: remove_task, label: tr! { "zh-Hans" => "移除任务（保留文件）", "en" => "Remove (keep files)" }, icon: "xmark") => downloads.remove(task.id)
                MenuItem(id: delete_task, label: tr! { "zh-Hans" => "删除任务和文件…", "en" => "Delete with files…" }, icon: "trash") => downloads.ask_delete(task.id)
            }
        }
    }
}

/// Adding a task (`dladd`, NipaPlay's 添加磁力链接): a magnet link or a
/// .torrent file, read to show what it holds, and the folder it goes to.
#[component]
pub fn download_add(downloads: downloads::Client, #[session] draft: DownloadsDraft) -> impl View {
    ui! {
        Modal(width: 600, height: 600, cover: compact, appear: pop_in, disappear: fade) {
            ScrollView {
                Column(gap: 12, padding: 24) {
                    Row(gap: 8, align: center) {
                        Text(content: tr! { "zh-Hans" => "添加任务", "en" => "Add task" }, style: title2)
                        Spacer
                        IconButton(id: close_add, label: tr! { "zh-Hans" => "关闭", "en" => "Close" }, icon: "xmark", prominence: plain) => downloads.close_add()
                    }
                    TextInput(id: magnet, label: tr! { "zh-Hans" => "磁力链接", "en" => "Magnet link" }, label_hidden: true, value: draft.link, prompt: tr! { "zh-Hans" => "magnet:?xt=urn:btih:…", "en" => "magnet:?xt=urn:btih:…" }) => downloads.read_link(draft.link)
                    Row(gap: 8, align: center) {
                        Button(id: read_link, label: tr! { "zh-Hans" => "预览", "en" => "Preview" }, icon: "magnifyingglass", shape: capsule, height: 30) => downloads.read_link(draft.link)
                        if !window.tv {
                            Button(id: read_torrent, label: tr! { "zh-Hans" => "选择种子文件", "en" => "Choose a torrent file" }, icon: "folder", shape: capsule, height: 30, pick: draft.torrent, files: ".torrent") => downloads.read_file(draft.torrent)
                        }
                        Spacer
                    }
                    if downloads.adding.reading {
                        Text(content: tr! { "zh-Hans" => "正在读取种子…磁力链接要从网络上找，可能要一会儿。", "en" => "Reading the torrent… a magnet link is looked up on the network, which can take a while." }, style: caption, emphasis: secondary, lines: 2)
                    }
                    if let Some(problem) = downloads.adding.problem {
                        Text(content: problem, style: caption, emphasis: secondary, lines: 2)
                    }
                    if downloads.adding.read {
                        Stack(variant: row_group) {
                            Column(gap: 4, padding: 12) {
                                Text(content: downloads.adding.name, style: headline, lines: 2)
                                Text(content: tr! { "zh-Hans" => "{count} 个文件，{size}", "en" => "{count} files, {size}"; count: downloads.adding.file_count, size: downloads.adding.size }, style: caption, emphasis: secondary)
                                if downloads.adding.own_folder {
                                    Text(content: tr! { "zh-Hans" => "文件夹：{name}", "en" => "Folder: {name}"; name: downloads.adding.folder_name }, style: caption, emphasis: secondary, lines: 1)
                                }
                                Canvas(height: 4)
                                ForEach(items: downloads.adding.files, key: index) |file| {
                                    Row(gap: 8, align: center) {
                                        Text(content: file.name, style: caption, lines: 1)
                                        Spacer
                                        Text(content: file.size, style: caption, emphasis: secondary)
                                    }
                                }
                                if downloads.adding.more_files > 0 {
                                    Text(content: tr! { "zh-Hans" => "还有 {count} 个文件", "en" => "{count} more files"; count: downloads.adding.more_files }, style: caption, emphasis: tertiary)
                                }
                            }
                        }
                    }
                    Canvas(height: 4)
                    Text(content: tr! { "zh-Hans" => "保存到", "en" => "Save to" }, style: caption, emphasis: secondary)
                    Row(gap: 8, align: center) {
                        Text(content: downloads.adding.place, lines: 1)
                        Spacer
                        if !window.tv {
                            Button(id: choose_place, label: tr! { "zh-Hans" => "更改…", "en" => "Change…" }, shape: capsule, height: 28, pick: draft.place, files: "folder") => downloads.choose_place(draft.place)
                        }
                    }
                    if downloads.adding.has_places {
                        ScrollView(axis: horizontal, indicators: hidden) {
                            Row(gap: 8, align: center) {
                                ForEach(items: downloads.adding.places, key: id) |place| {
                                    if place.chosen {
                                        Button(id: place_chosen, label: place.name, icon: "checkmark", prominence: prominent, shape: chip, height: 28, variant: library_chip) => downloads.use_place(place.id)
                                    } else {
                                        Button(id: place, label: place.name, shape: chip, height: 28, variant: library_chip) => downloads.use_place(place.id)
                                    }
                                }
                            }
                        }
                    }
                    Row(gap: 12, align: center) {
                        Text(content: tr! { "zh-Hans" => "为任务创建独立文件夹", "en" => "A folder for each task" })
                        Spacer
                        Switch(id: own_folder, label: tr! { "zh-Hans" => "为任务创建独立文件夹", "en" => "A folder for each task" }, label_hidden: true, style: switch, checked: downloads.adding.own_folder) => downloads.set_own_folder(downloads.adding.own_folder)
                    }
                    Canvas(height: 8)
                    Row(gap: 10, align: center) {
                        Spacer
                        Button(id: cancel_add, label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }, shape: capsule, height: 32) => downloads.close_add()
                        Button(id: confirm_add, label: tr! { "zh-Hans" => "添加", "en" => "Add" }, icon: "arrow.down", shape: capsule, height: 32, prominence: prominent, clears: draft.link) => downloads.add(draft.link)
                    }
                }
            }
        }
    }
}
