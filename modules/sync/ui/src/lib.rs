//! Interface of the sync module: the parts of 「设置 › 账号同步」, which the
//! application places in its settings — the state at the top of the page
//! (`settings_sync_status`) and the pages it lists after it — and the clock
//! of the automatic sync, mounted in the background of the main window.

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

/// The page's state: what the last sync did, or what runs. It heads
/// 「设置 › 账号同步」, above the pages the application lists after it.
#[component]
pub fn account_sync_status(sync: sync::Client) -> impl View {
    ui! {
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
    }
}

/// 「账号同步 › 同步口令」: the passphrase private records are sealed with.
#[component]
pub fn account_sync_passphrase(sync: sync::Client, #[session] draft: SyncDraft) -> impl View {
    ui! {
        Form(title: tr! { "zh-Hans" => "同步口令", "en" => "Sync Passphrase" }, variant: settings_form, appear: settings_page_in) {
            Section(footer: tr! { "zh-Hans" => "同步口令保护账号、来源密钥、WebDAV 配置和扩展数据。换设备需使用同一口令；没有口令时跳过这些数据，保留本机内容。", "en" => "The sync passphrase protects accounts, source credentials, WebDAV configuration and extension data. Use the same passphrase on another device. Without it, these records are skipped and local data is kept." }) {
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
        }
    }
}

/// 「账号同步 › WebDAV 同步」: the folder the devices sync through.
#[component]
pub fn account_sync_webdav(sync: sync::Client, #[session] draft: SyncDraft) -> impl View {
    ui! {
        Form(title: tr! { "zh-Hans" => "WebDAV 同步", "en" => "WebDAV Sync" }, variant: settings_form, appear: settings_page_in) {
            if sync.webdav_available {
                Section(footer: tr! { "zh-Hans" => "与 NipaPlay 的增量同步一样：云端保存一份快照和各设备的补丁，每台设备只上传自己的改动，两边都改过的记录以较新的为准。本机的文件夹来源、文件路径与下载位置不会上传。", "en" => "Like NipaPlay's incremental sync: the cloud keeps a snapshot and each device's patches; every device uploads only its own changes, and where two changed one record the newer wins. Local folders, file paths and download locations stay on this device." }) {
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
            } else {
                Section {
                    Text(content: tr! { "zh-Hans" => "网页版用不了 WebDAV 同步", "en" => "WebDAV sync is not available on the web" }, emphasis: secondary)
                }
            }
        }
    }
}

/// 「账号同步 › 备份文件」: everything in one file, out and in again.
#[component]
pub fn account_sync_backup(sync: sync::Client, #[session] draft: SyncDraft) -> impl View {
    ui! {
        Form(title: tr! { "zh-Hans" => "备份文件", "en" => "Backup File" }, variant: settings_form, appear: settings_page_in) {
            // A television has no files to pick.
            if !window.tv {
                Section(footer: tr! { "zh-Hans" => ".ebp 保存设置、来源、媒体关联、收藏、已看状态和播放进度；账号、来源密钥、WebDAV 配置和扩展数据需要同步口令。缓存不备份。导入不会删除本机记录。", "en" => "An .ebp file holds settings, sources, media links, collections, watched states and playback progress. Accounts, source credentials, WebDAV configuration and extension data require a sync passphrase. Caches are excluded. Importing does not delete local records." }) {
                    Row(gap: 12, align: center) {
                        Column(gap: 1) {
                            Text(content: tr! { "zh-Hans" => "导出", "en" => "Export" }, style: body)
                            Text(content: tr! { "zh-Hans" => "选一个文件夹，写入 erika-backup-日期-时间.ebp", "en" => "Choose a folder for erika-backup-date-time.ebp" }, style: caption, emphasis: secondary)
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
                        Button(id: sync_import, label: tr! { "zh-Hans" => "导入文件…", "en" => "Import File…" }, enabled: !sync.busy, pick: draft.import_file, files: ".ebp") => sync.import_from(draft.import_file)
                    }
                }
            }
        }
    }
}

/// 「账号同步 › 同步的内容」: how much of each kind a sync carries.
#[component]
pub fn account_sync_contents(sync: sync::Client) -> impl View {
    ui! {
        Form(title: tr! { "zh-Hans" => "同步的内容", "en" => "What Is Synced" }, variant: settings_form, appear: settings_page_in) {
            Section {
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
