//! Interface of the sync module: 「设置 › 账号同步」, which the application
//! places in its settings (`settings_sync`), and the clock of the
//! automatic sync, mounted in the background of the main window.

use bern::prelude::*;
use erika_sync_protocol::sync;

#[bern::session]
pub struct SyncDraft {
    pub passphrase: String,
    pub url: String,
    pub user: String,
    pub password: String,
    pub path: String,
    pub export_folder: MediaSource,
    pub import_file: MediaSource,
}

/// Mounted everywhere: syncs when the automatic sync is due, and says how
/// a sync the user started went.
#[component]
pub fn sync_clock(sync: sync::Client) -> impl View {
    ui! {
        Stack {
            Timer(interval: 60.0, enabled: sync.auto && sync.configured) => sync.tick()
            if let Some(notice) = sync.notice {
                Toast(content: notice, key: sync.notice_revision, duration: 4)
            }
        }
    }
}

/// The page: the passphrase accounts are sealed with, the WebDAV folder,
/// and the file to export or import. A native grouped form, as every
/// settings page.
#[component]
pub fn sync_settings(sync: sync::Client, #[session] draft: SyncDraft) -> impl View {
    ui! {
        Form(variant: settings_form, appear: settings_page_in) {
            Slot(id: form_header)
            Section(title: tr! { "zh-Hans" => "状态", "en" => "Status" }) {
                Row(gap: 12, align: center) {
                    Column(gap: 1) {
                        if sync.busy {
                            Text(content: sync.phase, style: body)
                        } else {
                            Text(content: sync.result, style: body, lines: 3)
                        }
                        if "" != sync.last_sync {
                            Text(content: tr! { "zh-Hans" => "上次同步：{time}", "en" => "Last synced: {time}"; time: sync.last_sync }, style: caption, emphasis: secondary)
                        }
                    }
                    Spacer
                    if sync.busy {
                        Progress(label: tr! { "zh-Hans" => "进行中", "en" => "Working" })
                    }
                }
                ForEach(items: sync.notes, key: id) |note| {
                    Text(content: note.warning, style: caption, emphasis: secondary, lines: 3)
                }
            }
            Section(title: tr! { "zh-Hans" => "同步口令", "en" => "Sync passphrase" }, footer: tr! { "zh-Hans" => "弹弹play 令牌、Bangumi 登录和 WebDAV 密码用这个口令加密后才写进同步文件与云端，换设备后输入同一个口令，账号就能直接使用。口令只存在本机的系统钥匙串里；忘了口令只会丢掉账号，其他数据不受影响。", "en" => "Your dandanplay token, Bangumi session and WebDAV password are encrypted with this passphrase before they go into a sync file or the cloud. Enter the same passphrase on a new device and your accounts work at once. It stays in this device's keychain; forgetting it loses only the accounts." }) {
                Row(gap: 12, align: center) {
                    Text(content: tr! { "zh-Hans" => "同步口令", "en" => "Passphrase" }, style: body)
                    Spacer
                    if sync.has_passphrase {
                        Text(content: tr! { "zh-Hans" => "已设置", "en" => "Set" }, emphasis: secondary)
                    } else {
                        Text(content: tr! { "zh-Hans" => "未设置", "en" => "Not set" }, emphasis: secondary)
                    }
                }
                TextInput(id: sync_passphrase, label: tr! { "zh-Hans" => "新口令", "en" => "New passphrase" }, value: draft.passphrase, kind: secure, prompt: tr! { "zh-Hans" => "至少 6 个字符", "en" => "At least 6 characters" })
                Row(gap: 8, align: center) {
                    Spacer
                    if sync.has_passphrase {
                        Button(id: sync_clear_passphrase, label: tr! { "zh-Hans" => "清除口令", "en" => "Clear" }, role: destructive, enabled: !sync.busy) => sync.set_passphrase("")
                    }
                    Button(id: sync_save_passphrase, label: tr! { "zh-Hans" => "保存口令", "en" => "Save Passphrase" }, prominence: prominent, enabled: !sync.busy, clears: draft.passphrase) => sync.set_passphrase(draft.passphrase)
                }
            }
            if sync.webdav_available {
                Section(title: tr! { "zh-Hans" => "WebDAV 同步", "en" => "WebDAV sync" }, footer: tr! { "zh-Hans" => "与 NipaPlay 的增量同步一样：云端保存一份快照和各设备的补丁，每台设备只上传自己的改动，两边都改过的记录以较新的为准。本机的文件夹来源、文件路径与下载位置不会上传。", "en" => "Like NipaPlay's incremental sync: the cloud keeps a snapshot and each device's patches; every device uploads only its own changes, and where two changed one record the newer wins. Local folders, file paths and download locations stay on this device." }) {
                    if sync.configured && !sync.editing {
                        Row(gap: 12, align: center) {
                            Column(gap: 1) {
                                Text(content: sync.url, style: body, lines: 1)
                                Text(content: tr! { "zh-Hans" => "{user} · {path}", "en" => "{user} · {path}"; user: sync.user, path: sync.path }, style: caption, emphasis: secondary, lines: 1)
                            }
                            Spacer
                            Button(id: sync_edit, label: tr! { "zh-Hans" => "修改", "en" => "Edit" }, enabled: !sync.busy) => sync.edit_webdav(true)
                        }
                        Switch(id: sync_auto, label: tr! { "zh-Hans" => "自动同步", "en" => "Sync automatically" }, checked: sync.auto, style: switch) => sync.set_auto(sync.auto)
                        if sync.auto {
                            Picker(id: sync_interval, label: tr! { "zh-Hans" => "同步间隔", "en" => "Interval" }, selected: sync.interval, style: menu) => sync.set_interval(sync.interval)
                        }
                        Row(gap: 8, align: center) {
                            Button(id: sync_disconnect, label: tr! { "zh-Hans" => "断开", "en" => "Disconnect" }, role: destructive, enabled: !sync.busy) => sync.clear_webdav()
                            Spacer
                            Button(id: sync_test, label: tr! { "zh-Hans" => "测试连接", "en" => "Test" }, enabled: !sync.busy) => sync.test_webdav()
                            Button(id: sync_now, label: tr! { "zh-Hans" => "立即同步", "en" => "Sync Now" }, prominence: prominent, enabled: !sync.busy) => sync.sync_now()
                        }
                    } else {
                        TextInput(id: sync_url, label: tr! { "zh-Hans" => "服务器地址", "en" => "Server" }, value: draft.url, prompt: tr! { "zh-Hans" => "https://dav.example.com/dav", "en" => "https://dav.example.com/dav" })
                        TextInput(id: sync_user, label: tr! { "zh-Hans" => "用户名", "en" => "User name" }, value: draft.user, prompt: tr! { "zh-Hans" => "用户名", "en" => "User name" })
                        TextInput(id: sync_password, label: tr! { "zh-Hans" => "密码", "en" => "Password" }, value: draft.password, kind: secure, prompt: tr! { "zh-Hans" => "密码或应用专用密码", "en" => "Password or app password" })
                        TextInput(id: sync_path, label: tr! { "zh-Hans" => "远端目录", "en" => "Remote folder" }, value: draft.path, prompt: tr! { "zh-Hans" => "/Erika/sync", "en" => "/Erika/sync" })
                        if sync.configured {
                            Text(content: tr! { "zh-Hans" => "留空的项保持原来的设置", "en" => "Fields left empty keep their saved values" }, style: caption, emphasis: secondary)
                        }
                        Row(gap: 8, align: center) {
                            Spacer
                            if sync.configured {
                                Button(id: sync_cancel_edit, label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }, role: cancel, clears: draft.password) => sync.edit_webdav(false)
                            }
                            Button(id: sync_save_webdav, label: tr! { "zh-Hans" => "保存", "en" => "Save" }, prominence: prominent, enabled: !sync.busy, clears: draft.password) => sync.set_webdav(draft.url, draft.user, draft.password, draft.path)
                        }
                    }
                }
            }
            // A television has no files to pick.
            if !window.tv {
                Section(title: tr! { "zh-Hans" => "备份文件", "en" => "Backup file" }, footer: tr! { "zh-Hans" => "单个 .erika 文件，装着全部设置、媒体库来源、观看记录与进度、文件与剧集的对应、番剧状态与评分、扩展数据，设置了同步口令时还有加密的账号。海报、截图等缓存不在其中，可以重新获取。导入只补上本机没有或较旧的记录，不会删除本机数据。", "en" => "One .erika file with every setting, the library's sources, what you watched and where it stopped, files matched to episodes, show status and ratings, and extension data — plus your accounts, encrypted, when a sync passphrase is set. Caches such as posters and screenshots stay out; they can be fetched again. Importing adds what this device lacks or has older, and deletes nothing." }) {
                    Row(gap: 12, align: center) {
                        Column(gap: 1) {
                            Text(content: tr! { "zh-Hans" => "导出", "en" => "Export" }, style: body)
                            Text(content: tr! { "zh-Hans" => "选一个文件夹，写入 erika-backup-日期-时间.erika", "en" => "Choose a folder for erika-backup-date-time.erika" }, style: caption, emphasis: secondary)
                        }
                        Spacer
                        Button(id: sync_export, label: tr! { "zh-Hans" => "导出到…", "en" => "Export to…" }, enabled: !sync.busy, pick: draft.export_folder, files: "folder") => sync.export_to(draft.export_folder)
                    }
                    Row(gap: 12, align: center) {
                        Column(gap: 1) {
                            Text(content: tr! { "zh-Hans" => "导入", "en" => "Import" }, style: body)
                            Text(content: tr! { "zh-Hans" => "换了设备？先填同步口令，再导入", "en" => "New device? Enter the sync passphrase first, then import" }, style: caption, emphasis: secondary)
                        }
                        Spacer
                        Button(id: sync_import, label: tr! { "zh-Hans" => "导入文件…", "en" => "Import File…" }, enabled: !sync.busy, pick: draft.import_file, files: ".erika") => sync.import_from(draft.import_file)
                    }
                }
            }
            Section(title: tr! { "zh-Hans" => "同步的内容", "en" => "What is synced" }) {
                ForEach(items: sync.parts, key: id) |part| {
                    Row(gap: 12, align: center) {
                        Text(content: part.kind, style: body)
                        Spacer
                        Text(content: tr! { "zh-Hans" => "{count} 项", "en" => "{count}"; count: part.count }, emphasis: secondary)
                    }
                }
                Button(id: sync_count, label: tr! { "zh-Hans" => "重新统计", "en" => "Count Again" }, prominence: plain) => sync.refresh_parts()
            }
        }
    }
}
