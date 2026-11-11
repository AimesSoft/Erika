//! The library's sources (`libSrcHTML`, `dirHTML` and `matchHTML` in
//! design/next-screens.js): the folders the library scans with the videos
//! not recognized under them; a folder opened level by level, each video
//! saying what it was matched to; and matching, one video or a folder's.
use crate::{LibraryDraft, library_tabs, library_tabs_props};
use bern::prelude::*;
use erika_library_protocol::library::{self, SourceContent};
use erika_protocol::erika;

#[component]
pub fn library_sources_page(app: erika::Client, library: library::Client) -> impl View {
    ui! {
        Stack {
            ScrollView(appear: page_in) {
                Column(gap: 0, padding_x: if window.compact { 16.0 } else if window.tv { 56.0 } else if window.medium { 24.0 } else { 32.0 }, padding_y: 6) {
                    if library.matching.open {
                        library_match(app: app, library: library)
                    } else if library.browsing {
                        library_folder(library: library)
                    } else {
                        library_source_list(library: library)
                    }
                    Canvas(height: 48)
                }
            }
            if library.shared_prompt {
                library_shared_mount(app: app, library: library)
            }
            if library.remote_prompt {
                library_remote_mount(app: app, library: library)
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
                    ControlGroup(label: tr! { "zh-Hans" => "媒体库操作", "en" => "Library actions" }, height: 44) {
                        Menu(id: mount_phone, label: tr! { "zh-Hans" => "挂载", "en" => "Mount" }, icon: "plus", style: icon) {
                            MenuItem(id: mount_local_phone, label: tr! { "zh-Hans" => "本地文件夹", "en" => "Local folder" }, icon: "folder", pick: draft.folder, files: "folder") => library.add_folder(draft.folder, SourceContent::Anime)
                            MenuItem(id: mount_shared_phone, label: tr! { "zh-Hans" => "NipaPlay共享媒体", "en" => "NipaPlay shared media" }, icon: "server.rack") => library.open_shared_mount()
                            MenuItem(id: mount_remote_phone, label: tr! { "zh-Hans" => "弹弹play共享媒体", "en" => "dandanplay shared media" }, icon: "server.rack") => library.open_remote_mount()
                        }
                        Slot(id: phone_header_settings)
                    }
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
                        Menu(id: mount, label: tr! { "zh-Hans" => "挂载", "en" => "Mount" }, icon: "plus", shape: capsule, height: 32) {
                            MenuItem(id: mount_local, label: tr! { "zh-Hans" => "本地文件夹", "en" => "Local folder" }, icon: "folder", pick: draft.folder, files: "folder") => library.add_folder(draft.folder, SourceContent::Anime)
                            MenuItem(id: mount_shared, label: tr! { "zh-Hans" => "NipaPlay共享媒体", "en" => "NipaPlay shared media" }, icon: "server.rack") => library.open_shared_mount()
                            MenuItem(id: mount_remote, label: tr! { "zh-Hans" => "弹弹play共享媒体", "en" => "dandanplay shared media" }, icon: "server.rack") => library.open_remote_mount()
                        }
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
                                        if row.shared {
                                            Label(title: row.name, icon: "server.rack", display: icon, style: body)
                                        } else {
                                            Label(title: row.name, icon: "folder", display: icon, style: body)
                                        }
                                    }
                                    Column(gap: 2) {
                                        Text(content: row.name, style: headline, lines: 1)
                                        if row.dandanplay {
                                            Text(content: tr! { "zh-Hans" => "弹弹play共享媒体 · {at}", "en" => "dandanplay shared media · {at}"; at: row.location }, style: caption, emphasis: secondary, lines: 1)
                                        } else if row.shared {
                                            Text(content: tr! { "zh-Hans" => "NipaPlay共享媒体 · {at}", "en" => "NipaPlay shared media · {at}"; at: row.location }, style: caption, emphasis: secondary, lines: 1)
                                        } else {
                                            Text(content: tr! { "zh-Hans" => "{content} · {at}", "en" => "{content} · {at}"; content: row.content, at: row.location }, style: caption, emphasis: secondary, lines: 1)
                                        }
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
                    Text(content: tr! { "zh-Hans" => "通过「挂载」添加本地文件夹、NipaPlay共享媒体或弹弹play共享媒体，视频会汇总到「番剧」里。", "en" => "Mount a local folder, NipaPlay shared media or dandanplay shared media to add its videos to Shows." }, emphasis: secondary, lines: 3)
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

/// The same connection fields as NipaPlay's shared-host dialog.
#[component]
pub fn library_shared_mount(app: erika::Client, library: library::Client, #[session] draft: LibraryDraft) -> impl View {
    ui! {
        // A card as tall as its fields, also on a phone: they scroll above
        // the buttons where the window is too short for them.
        Modal(width: 440, appear: pop_in, disappear: fade) {
            Column(gap: 16, padding: 24) {
                ScrollView {
                    Column(gap: 16) {
                        Text(content: tr! { "zh-Hans" => "挂载 NipaPlay共享媒体", "en" => "Mount NipaPlay shared media" }, style: title3)
                        Text(content: tr! { "zh-Hans" => "连接已开启远程访问的 NipaPlay 设备。", "en" => "Connect to a NipaPlay device with remote access enabled." }, emphasis: secondary, lines: 2)
                        TextInput(id: shared_name, label: tr! { "zh-Hans" => "备注名称（可选）", "en" => "Name (optional)" }, value: draft.shared_name, prompt: tr! { "zh-Hans" => "例如：家里的电脑", "en" => "e.g. Home computer" })
                        TextInput(id: shared_address, label: tr! { "zh-Hans" => "访问地址", "en" => "Address" }, value: draft.shared_address, prompt: tr! { "zh-Hans" => "192.168.1.100:1180", "en" => "192.168.1.100:1180" })
                        Text(content: tr! { "zh-Hans" => "局域网地址默认使用端口 1180，也可填写完整 HTTP(S) 地址。", "en" => "Local addresses use port 1180 by default. A full HTTP(S) address also works." }, style: caption, emphasis: secondary, lines: 2)
                        if library.shared_connecting {
                            Text(content: tr! { "zh-Hans" => "正在连接并读取共享媒体…", "en" => "Connecting and loading shared media…" }, style: caption, emphasis: secondary)
                        } else if library.shared_error != "" {
                            Text(content: library.shared_error, style: caption, lines: 3)
                        }
                    }
                }
                Row(gap: 12, align: center) {
                    Spacer
                    Button(id: shared_cancel, label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }, role: cancel, shortcut: app.keys.back, prominence: plain) => library.cancel_shared_mount()
                    Button(id: shared_mount, label: tr! { "zh-Hans" => "挂载", "en" => "Mount" }, prominence: prominent, enabled: !library.shared_connecting) => library.mount_shared(draft.shared_name, draft.shared_address)
                }
            }
        }
    }
}

/// Connecting to a dandanplay PC client's remote access, with the fields
/// of NipaPlay's 弹弹play远程访问 dialog; editing it, empty fields keep what
/// the source has.
#[component]
pub fn library_remote_mount(app: erika::Client, library: library::Client, #[session] draft: LibraryDraft) -> impl View {
    ui! {
        // A card as tall as its fields, also on a phone: they scroll above
        // the buttons where the window is too short for them.
        Modal(width: 460, appear: pop_in, disappear: fade) {
            Column(gap: 14, padding: 24) {
                ScrollView {
                    Column(gap: 14) {
                        if library.remote_editing > 0 {
                            Text(content: tr! { "zh-Hans" => "编辑弹弹play共享媒体", "en" => "Edit dandanplay shared media" }, style: title3)
                            Text(content: tr! { "zh-Hans" => "只改要改的项，留空的保持不变。", "en" => "Change only what you need; empty fields stay as they are." }, emphasis: secondary, lines: 2)
                            TextInput(id: remote_name_edit, label: tr! { "zh-Hans" => "备注名称", "en" => "Name" }, value: draft.remote_edit_name, prompt: library.remote_name, clear_key: library.remote_saved)
                            TextInput(id: remote_address_edit, label: tr! { "zh-Hans" => "访问地址", "en" => "Address" }, value: draft.remote_edit_address, prompt: library.remote_address, clear_key: library.remote_saved)
                            if library.remote_has_token {
                                TextInput(id: remote_token_edit, label: tr! { "zh-Hans" => "API 密钥", "en" => "API key" }, value: draft.remote_edit_token, kind: secure, prompt: tr! { "zh-Hans" => "已保存，留空不变", "en" => "Saved; leave empty to keep it" }, clear_key: library.remote_saved)
                            } else {
                                TextInput(id: remote_token_new, label: tr! { "zh-Hans" => "API 密钥（可选）", "en" => "API key (optional)" }, value: draft.remote_edit_token, kind: secure, prompt: tr! { "zh-Hans" => "可留空，按需填写", "en" => "Optional" }, clear_key: library.remote_saved)
                            }
                        } else {
                            Text(content: tr! { "zh-Hans" => "挂载弹弹play共享媒体", "en" => "Mount dandanplay shared media" }, style: title3)
                            Text(content: tr! { "zh-Hans" => "连接已开启「远程访问」的弹弹play 桌面端，它媒体库里的番剧会汇总到「番剧」里，可以直接播放。", "en" => "Connect to the dandanplay desktop app with remote access on. Its shows join Shows and play from here." }, emphasis: secondary, lines: 3)
                            TextInput(id: remote_name, label: tr! { "zh-Hans" => "备注名称（可选）", "en" => "Name (optional)" }, value: draft.remote_name, prompt: tr! { "zh-Hans" => "例如：书房的电脑", "en" => "e.g. Study computer" }, clear_key: library.remote_saved)
                            TextInput(id: remote_address, label: tr! { "zh-Hans" => "访问地址", "en" => "Address" }, value: draft.remote_address, prompt: tr! { "zh-Hans" => "例如：http://192.168.1.2:23333", "en" => "e.g. http://192.168.1.2:23333" }, clear_key: library.remote_saved)
                            TextInput(id: remote_token, label: tr! { "zh-Hans" => "API 密钥（可选）", "en" => "API key (optional)" }, value: draft.remote_token, kind: secure, prompt: tr! { "zh-Hans" => "可留空，按需填写", "en" => "Optional" }, clear_key: library.remote_saved)
                        }
                        Text(content: tr! { "zh-Hans" => "地址和端口在弹弹play 桌面端的「远程访问」设置里；开启了 API 密钥验证时，填写那里设置的密钥。", "en" => "Find the address and port under Remote access in the dandanplay desktop app. If it asks for an API key, enter the key set there." }, style: caption, emphasis: secondary, lines: 3)
                        if library.remote_connecting {
                            Text(content: tr! { "zh-Hans" => "正在连接并读取弹弹play媒体库…", "en" => "Connecting and loading the dandanplay library…" }, style: caption, emphasis: secondary)
                        } else if library.remote_error != "" {
                            Text(content: library.remote_error, style: caption, lines: 3)
                        }
                    }
                }
                Row(gap: 12, align: center) {
                    Spacer
                    // The key is hidden as it is typed: it does not wait,
                    // unseen, for the next connection.
                    if library.remote_editing > 0 {
                        Button(id: remote_edit_cancel, label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }, role: cancel, shortcut: app.keys.back, prominence: plain, clears: draft.remote_edit_token) => library.cancel_remote_mount()
                        Button(id: remote_save, label: tr! { "zh-Hans" => "保存", "en" => "Save" }, prominence: prominent, enabled: !library.remote_connecting, clears: draft.remote_edit_token) => library.mount_remote(draft.remote_edit_name, draft.remote_edit_address, draft.remote_edit_token)
                    } else {
                        Button(id: remote_cancel, label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }, role: cancel, shortcut: app.keys.back, prominence: plain, clears: draft.remote_token) => library.cancel_remote_mount()
                        Button(id: remote_mount, label: tr! { "zh-Hans" => "挂载", "en" => "Mount" }, prominence: prominent, enabled: !library.remote_connecting, clears: draft.remote_token) => library.mount_remote(draft.remote_name, draft.remote_address, draft.remote_token)
                    }
                }
            }
        }
    }
}

/// A video: what it is, and matching it (`dirHTML`'s `file`).
#[component]
pub fn library_file_row(
    library: library::Client,
    file: library::LibraryFile,
    place: bool,
) -> impl View {
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
            if file.readable {
                Button(id: play_file, label: tr! { "zh-Hans" => "播放", "en" => "Play" }, icon: "play.fill", shape: capsule, height: if window.tv { 40 } else { 28 }) => library.play_file(file.key)
            }
            if library.folder.shared {
                if file.show != "" {
                    Button(id: shared_show, label: tr! { "zh-Hans" => "查看剧集", "en" => "View episodes" }, shape: capsule, height: 28) => library.open(file.show)
                } else if library.folder.dandanplay && file.readable {
                    Button(id: remote_assign, label: tr! { "zh-Hans" => "指定剧集", "en" => "Choose episode" }, shape: capsule, height: if window.tv { 40 } else { 28 }) => library.assign(file.key)
                }
            } else if file.matched {
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
                if window.compact {
                    ControlGroup(label: tr! { "zh-Hans" => "媒体库操作", "en" => "Library actions" }, height: 44) {
                        library_folder_menu(library: library)
                        Slot(id: phone_header_settings)
                    }
                } else {
                    library_folder_menu(library: library)
                }
            }
            // `.m-crumb`s: 来源 › the source › the folders.
            ScrollView(axis: horizontal, indicators: mouse) {
                Row(gap: 4, align: center) {
                    Button(id: crumb_sources, label: tr! { "zh-Hans" => "来源", "en" => "Sources" }, prominence: plain, variant: more_link) => library.close_source()
                    ForEach(items: library.folder.crumbs, key: depth) |crumb| {
                        Text(content: tr! { "zh-Hans" => "›", "en" => "›" }, style: caption, emphasis: tertiary)
                        Button(id: crumb, label: crumb.name, prominence: plain, variant: more_link) => library.go_to(crumb.depth)
                    }
                }
            }
            Text(content: tr! { "zh-Hans" => "{content} · {at}", "en" => "{content} · {at}"; content: library.folder.content, at: library.folder.location }, style: caption, emphasis: secondary, lines: 1)
            Canvas(height: 16)
            if library.folder.can_batch && window.compact {
                Row(gap: 0) {
                    Button(id: batch_match_phone, label: tr! { "zh-Hans" => "批量匹配", "en" => "Match all" }, icon: "link", shape: capsule, height: 32) => library.match_folder()
                    Spacer
                }
                Canvas(height: 14)
            }
            if library.folder.missing {
                if library.folder.shared {
                    Text(content: tr! { "zh-Hans" => "暂时连不上共享设备。请确认设备已开启远程访问，然后在「更多」里刷新。", "en" => "The shared device is unavailable. Enable remote access on that device, then refresh from More." }, emphasis: secondary, lines: 3)
                } else {
                    Text(content: tr! { "zh-Hans" => "找不到这个文件夹：它被移动了，或者所在的磁盘没有接上。接上以后在「更多」里重新扫描。", "en" => "This folder can't be found: it moved, or its disk isn't connected. Connect it and scan again from More." }, emphasis: secondary, lines: 3)
                }
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

/// Source actions, shared by the desktop menu and phone control group.
#[component]
pub fn library_folder_menu(library: library::Client) -> impl View {
    ui! {
        if library.folder.dandanplay {
            Menu(id: remote_more, label: tr! { "zh-Hans" => "更多", "en" => "More" }, icon: "ellipsis", style: icon) {
                MenuItem(id: refresh_remote, label: tr! { "zh-Hans" => "刷新弹弹play媒体库", "en" => "Refresh dandanplay library" }, icon: "arrow.triangle.2.circlepath") => library.rescan(library.folder.source)
                MenuItem(id: edit_remote, label: tr! { "zh-Hans" => "编辑连接…", "en" => "Edit connection…" }, icon: "slider.horizontal.3") => library.edit_remote(library.folder.source)
                Divider
                MenuItem(id: remove_remote, label: tr! { "zh-Hans" => "移除这个来源", "en" => "Remove this source" }, icon: "trash") => library.remove_source(library.folder.source)
            }
        } else if library.folder.shared {
            Menu(id: shared_more, label: tr! { "zh-Hans" => "更多", "en" => "More" }, icon: "ellipsis", style: icon) {
                MenuItem(id: refresh_shared, label: tr! { "zh-Hans" => "刷新共享媒体", "en" => "Refresh shared media" }, icon: "arrow.triangle.2.circlepath") => library.rescan(library.folder.source)
                Divider
                MenuItem(id: remove_shared, label: tr! { "zh-Hans" => "移除这个来源", "en" => "Remove this source" }, icon: "trash") => library.remove_source(library.folder.source)
            }
        } else {
            Menu(id: folder_more, label: tr! { "zh-Hans" => "更多", "en" => "More" }, icon: "ellipsis", style: icon) {
                MenuItem(id: rescan, label: tr! { "zh-Hans" => "重新扫描", "en" => "Scan again" }, icon: "arrow.triangle.2.circlepath") => library.rescan(library.folder.source)
                Divider
                MenuItem(id: as_anime, label: tr! { "zh-Hans" => "改为番剧", "en" => "Holds anime" }) => library.set_content(library.folder.source, SourceContent::Anime)
                MenuItem(id: as_series, label: tr! { "zh-Hans" => "改为电视剧", "en" => "Holds TV series" }) => library.set_content(library.folder.source, SourceContent::Series)
                MenuItem(id: as_movie, label: tr! { "zh-Hans" => "改为电影", "en" => "Holds movies" }) => library.set_content(library.folder.source, SourceContent::Movie)
                Divider
                MenuItem(id: remove_source, label: tr! { "zh-Hans" => "移除这个来源", "en" => "Remove this source" }, icon: "trash") => library.remove_source(library.folder.source)
            }
        }
    }
}

/// Content matching uses the same TMDB work/season identities on every device.
#[component]
pub fn library_match(app: erika::Client, library: library::Client, #[session] draft: LibraryDraft) -> impl View {
    ui! {
        Column(gap: 14) {
            Row(gap: 10, align: center) {
                IconButton(id: match_back, icon: "chevron.left", label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }) => library.match_cancel()
                Text(content: tr! { "zh-Hans" => "匹配媒体", "en" => "Match media" }, style: large_title)
                Spacer
                if window.compact {
                    ControlGroup(label: tr! { "zh-Hans" => "媒体库操作", "en" => "Library actions" }, height: 44) { Slot(id: phone_header_settings) }
                }
            }
            Text(content: library.matching.place, style: caption, emphasis: secondary, lines: 2)
            Row(gap: 8, align: center) {
                Text(content: tr! { "zh-Hans" => "作品", "en" => "Title" }, emphasis: secondary)
                if library.matching.work != "" {
                    PopoverButton(id: match_work_chosen, label: library.matching.title, presented: draft.anime_open) {
                        library_match_search(app: app, library: library)
                    }
                } else {
                    PopoverButton(id: match_work_search, label: tr! { "zh-Hans" => "搜索作品", "en" => "Search titles" }, presented: draft.anime_open) {
                        library_match_search(app: app, library: library)
                    }
                }
                Spacer
            }
            if !library.matching.movie {
                ScrollView(axis: horizontal) {
                    Row(gap: 8) {
                        ForEach(items: library.matching.seasons, key: id) |season| {
                            if library.matching.season == season.id {
                                Button(id: match_season_chosen, label: season.title, prominence: prominent, shape: chip) => library.match_season(season.id)
                            } else {
                                Button(id: match_season, label: season.title, shape: chip) => library.match_season(season.id)
                            }
                        }
                    }
                }
                if !library.matching.single {
                    ScrollView(axis: horizontal) {
                        Row(gap: 8, align: center) {
                            if library.matching.by_order {
                                Button(id: by_number, label: tr! { "zh-Hans" => "文件名集数", "en" => "Filename numbers" }, shape: chip) => library.match_by_order(false)
                                Button(id: by_order_on, label: tr! { "zh-Hans" => "按顺序", "en" => "In order" }, icon: "checkmark", prominence: prominent, shape: chip) => library.match_by_order(true)
                            } else {
                                Button(id: by_number_on, label: tr! { "zh-Hans" => "文件名集数", "en" => "Filename numbers" }, icon: "checkmark", prominence: prominent, shape: chip) => library.match_by_order(false)
                                Button(id: by_order, label: tr! { "zh-Hans" => "按顺序", "en" => "In order" }, shape: chip) => library.match_by_order(true)
                            }
                        }
                    }
                    if library.matching.by_order {
                        Row(gap: 8, align: center) {
                            Text(content: tr! { "zh-Hans" => "起始集数", "en" => "First episode" }, emphasis: secondary)
                            Row(width: 80) {
                                TextInput(id: match_start, label: tr! { "zh-Hans" => "起始集数", "en" => "First episode" }, label_hidden: true, value: draft.start, prompt: tr! { "zh-Hans" => "{start}", "en" => "{start}"; start: library.matching.start }, submit: clear) => library.match_start(draft.start)
                            }
                        }
                    } else if library.matching.lowest > 0 {
                        Row(gap: 8, align: center) {
                            Text(content: tr! { "zh-Hans" => "文件名 {lowest} 对齐至", "en" => "Map filename {lowest} to"; lowest: library.matching.lowest }, emphasis: secondary)
                            Row(width: 80) {
                                TextInput(id: match_align, label: tr! { "zh-Hans" => "对齐集数", "en" => "Aligned episode" }, label_hidden: true, value: draft.align, prompt: tr! { "zh-Hans" => "{number}", "en" => "{number}"; number: library.matching.align_to }, submit: clear) => library.match_align(draft.align)
                            }
                        }
                    }
                }
            }
            if library.matching.loading {
                Text(content: tr! { "zh-Hans" => "正在载入资料…", "en" => "Loading details…" }, style: caption, emphasis: secondary)
            }
            if library.matching.error != "" {
                Text(content: library.matching.error, style: caption, lines: 4)
            }
            if !library.matching.single {
                Row(gap: 8, align: center) {
                    if library.matching.odd_only {
                        Button(id: match_all, label: tr! { "zh-Hans" => "全部 {count}", "en" => "All {count}"; count: library.matching.all_count }, shape: chip) => library.match_odd(false)
                        Button(id: match_odd_on, label: tr! { "zh-Hans" => "需检查 {count}", "en" => "Review {count}"; count: library.matching.odd_count }, prominence: prominent, shape: chip) => library.match_odd(true)
                    } else {
                        Button(id: match_all_on, label: tr! { "zh-Hans" => "全部 {count}", "en" => "All {count}"; count: library.matching.all_count }, prominence: prominent, shape: chip) => library.match_odd(false)
                        Button(id: match_odd, label: tr! { "zh-Hans" => "需检查 {count}", "en" => "Review {count}"; count: library.matching.odd_count }, shape: chip) => library.match_odd(true)
                    }
                    Spacer
                }
            }
            Stack(variant: row_group) {
                Column(gap: 0, padding: 4) {
                    ForEach(items: library.matching.shown, key: key) |row| {
                        library_match_row(library: library, row: row, single: library.matching.single)
                    }
                }
            }
            Text(content: tr! { "zh-Hans" => "同一文件在所有来源中的内容关联都会更新；弹幕关联不变。", "en" => "Content links change in every source containing the same file; danmaku links stay unchanged." }, style: caption, emphasis: secondary, lines: 3)
            Row(gap: 10, align: center) {
                Button(id: match_confirm, label: tr! { "zh-Hans" => "保存匹配", "en" => "Save matches" }, shape: capsule, prominence: prominent, enabled: library.matching.ready) => library.match_confirm()
                if library.matching.linked {
                    Button(id: match_unlink, label: tr! { "zh-Hans" => "解除匹配", "en" => "Unlink" }, shape: capsule) => library.match_unlink()
                }
                Button(id: match_cancel, label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }, shape: capsule) => library.match_cancel()
                Spacer
            }
        }
    }
}

#[component]
pub fn library_match_search(app: erika::Client, library: library::Client, #[session] draft: LibraryDraft) -> impl View {
    ui! {
        Column(gap: 8, width: 300, padding: 8) {
            TextInput(id: match_search, label: tr! { "zh-Hans" => "搜索作品", "en" => "Search titles" }, label_hidden: true, value: draft.search, prompt: tr! { "zh-Hans" => "作品名称", "en" => "Title" }) => library.match_search(draft.search)
            if library.matching.searching {
                Text(content: tr! { "zh-Hans" => "正在搜索…", "en" => "Searching…" }, style: caption, emphasis: secondary)
            } else if library.matching.search_failed {
                Text(content: library.matching.error, style: caption, lines: 3)
            }
            Column(gap: 0) {
                ForEach(items: library.matching.results, key: id) |work| {
                    Button(id: match_result, label: work.title, variant: row) {
                        Row(gap: 8, align: center) {
                            if work.chosen {
                                Label(title: tr! { "zh-Hans" => "已选", "en" => "Chosen" }, icon: "checkmark", display: icon, style: caption)
                            }
                            Column(gap: 2) {
                                Text(content: work.title, lines: 2)
                                Text(content: work.detail, style: caption, emphasis: secondary)
                            }
                            Spacer
                        }
                    } => library.match_work(work.id)
                }
            }
        }
    }
}

#[component]
pub fn library_match_row(library: library::Client, row: library::MatchRow, single: bool, #[session] draft: LibraryDraft) -> impl View {
    ui! {
        Row(gap: 12, align: center, padding: 6) {
            if single {
                Label(title: row.name, icon: "film", display: icon, style: body)
            } else if row.on {
                IconButton(id: skip_row, icon: "checkmark", label: tr! { "zh-Hans" => "跳过这个文件", "en" => "Skip this file" }) => library.match_toggle(row.key)
            } else {
                IconButton(id: take_row, icon: "circle", label: tr! { "zh-Hans" => "匹配这个文件", "en" => "Match this file" }) => library.match_toggle(row.key)
            }
            Column(gap: 4) {
                Text(content: row.name, lines: 1)
                Text(content: row.note, style: caption, emphasis: secondary, lines: 2)
                if !row.on {
                    Text(content: tr! { "zh-Hans" => "跳过", "en" => "Skipped" }, style: caption, emphasis: tertiary)
                } else if library.matching.movie {
                    Text(content: row.destination, style: caption, lines: 2)
                } else if row.editing {
                    TextInput(id: row_number, label: tr! { "zh-Hans" => "集数", "en" => "Episodes" }, value: draft.number, prompt: tr! { "zh-Hans" => "{number}；如 1、1-3 或 1,3", "en" => "{number}; e.g. 1, 1-3 or 1,3"; number: row.assignment }, submit: clear) => library.match_number(row.key, draft.number)
                } else if row.destination != "" {
                    Button(id: edit_row, label: row.destination, prominence: plain) => library.match_edit(row.key)
                } else {
                    Button(id: fill_row, label: tr! { "zh-Hans" => "指定集数", "en" => "Set episodes" }, prominence: plain) => library.match_edit(row.key)
                }
            }
            Spacer
        }
    }
}
