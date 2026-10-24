//! Protocol of the sync module: everything the user keeps in Erika, in
//! one binary file to export and import, or synced through a WebDAV folder
//! as NipaPlay's incremental sync does. Accounts travel sealed with a sync
//! passphrase the user chooses.

#[bern::protocol(namespace = "sync", version = 1)]
pub mod sync {
    use bern::media::MediaSource;
    use bern::prelude::*;

    /// How often the automatic sync runs.
    #[value]
    pub enum SyncInterval {
        #[label(tr! { "zh-Hans" => "每 15 分钟", "en" => "Every 15 minutes" })]
        Quarter,
        #[label(tr! { "zh-Hans" => "每 30 分钟", "en" => "Every 30 minutes" })]
        Half,
        #[label(tr! { "zh-Hans" => "每小时", "en" => "Hourly" })]
        Hour,
        #[label(tr! { "zh-Hans" => "每 6 小时", "en" => "Every 6 hours" })]
        SixHours,
    }

    /// What a sync is doing now.
    #[value]
    pub enum SyncPhase {
        #[label(tr! { "zh-Hans" => "空闲", "en" => "Idle" })]
        Idle,
        #[label(tr! { "zh-Hans" => "正在同步…", "en" => "Syncing…" })]
        Syncing,
        #[label(tr! { "zh-Hans" => "正在测试连接…", "en" => "Testing the connection…" })]
        Testing,
        #[label(tr! { "zh-Hans" => "正在导出…", "en" => "Exporting…" })]
        Exporting,
        #[label(tr! { "zh-Hans" => "正在导入…", "en" => "Importing…" })]
        Importing,
        #[label(tr! { "zh-Hans" => "正在生成口令密钥…", "en" => "Deriving the passphrase key…" })]
        Deriving,
    }

    /// How the last sync, test, export or import went.
    #[value]
    pub enum SyncResult {
        #[label(tr! { "zh-Hans" => "还没有同步过", "en" => "Not synced yet" })]
        Never,
        #[label(tr! { "zh-Hans" => "已在云端建立同步库，并上传了本机数据", "en" => "Created the sync repository and uploaded this device's data" })]
        Created,
        #[label(tr! { "zh-Hans" => "同步完成：下载 {downloaded} 个补丁，上传 {uploaded} 项，本机更新 {restored} 项", "en" => "Synced: {downloaded} patches downloaded, {uploaded} changes uploaded, {restored} applied here" })]
        Synced {
            downloaded: i64,
            uploaded: i64,
            restored: i64,
        },
        #[label(tr! { "zh-Hans" => "连接正常", "en" => "The connection works" })]
        Connected,
        #[label(tr! { "zh-Hans" => "已导出到 {path}", "en" => "Exported to {path}" })]
        Exported {
            #[content]
            path: String,
        },
        #[label(tr! { "zh-Hans" => "已导入 {changes} 项", "en" => "Imported {changes} changes" })]
        Imported { changes: i64 },
        #[label(tr! { "zh-Hans" => "失败：{detail}", "en" => "Failed: {detail}" })]
        Failed {
            #[content]
            detail: String,
        },
    }

    /// What a sync left out and why, said under the result.
    #[value]
    pub enum SyncWarning {
        #[label(tr! { "zh-Hans" => "未设置同步口令：账号没有一起同步", "en" => "No sync passphrase: accounts were left out" })]
        NoPassphrase,
        #[label(tr! { "zh-Hans" => "同步口令与加密账号时用的不一致：账号没有同步", "en" => "The sync passphrase differs from the one the accounts were sealed with: accounts were left out" })]
        WrongPassphrase,
        #[label(tr! { "zh-Hans" => "下载位置、新番数据源等模块设置在重启 Erika 后生效", "en" => "Module settings such as the download folder and the New Shows source take effect after restarting Erika" })]
        Restart,
    }

    /// A kind of record the sync carries.
    #[value]
    pub enum SyncKind {
        #[label(tr! { "zh-Hans" => "设置", "en" => "Settings" })]
        Settings,
        #[label(tr! { "zh-Hans" => "账号", "en" => "Accounts" })]
        Accounts,
        #[label(tr! { "zh-Hans" => "媒体库来源", "en" => "Library sources" })]
        Sources,
        #[label(tr! { "zh-Hans" => "文件与剧集对应", "en" => "Files matched to episodes" })]
        Links,
        #[label(tr! { "zh-Hans" => "播放进度", "en" => "Playback progress" })]
        Progress,
        #[label(tr! { "zh-Hans" => "最近播放", "en" => "Recently played" })]
        Recents,
        #[label(tr! { "zh-Hans" => "看过的剧集", "en" => "Watched episodes" })]
        Watched,
        #[label(tr! { "zh-Hans" => "番剧与剧集资料", "en" => "Shows and episodes" })]
        Anime,
        #[label(tr! { "zh-Hans" => "番剧状态与评分", "en" => "Show status and ratings" })]
        Marks,
        #[label(tr! { "zh-Hans" => "扩展数据", "en" => "Extension data" })]
        Extensions,
    }

    /// How many of one kind of record an export holds.
    #[value]
    pub struct SyncPart {
        pub id: i64,
        pub kind: SyncKind,
        pub count: i64,
    }

    #[value]
    pub struct SyncNote {
        pub id: i64,
        pub warning: SyncWarning,
    }

    #[state]
    pub struct State {
        /// WebDAV works here (not in the browser).
        pub webdav_available: bool,
        /// The WebDAV folder synced with.
        #[content]
        pub url: String,
        #[content]
        pub user: String,
        #[content]
        pub path: String,
        pub configured: bool,
        /// The WebDAV form is open over the saved folder.
        pub editing: bool,
        /// The sync passphrase is set on this device.
        pub has_passphrase: bool,
        pub auto: bool,
        pub interval: SyncInterval,
        pub phase: SyncPhase,
        pub busy: bool,
        pub result: SyncResult,
        pub notes: Vec<SyncNote>,
        /// When the last sync finished, as the user reads it; empty before.
        #[content]
        pub last_sync: String,
        /// What an export would hold now.
        pub parts: Vec<SyncPart>,
        pub notice: Option<SyncResult>,
        pub notice_revision: i64,
    }

    #[events]
    pub trait Events {
        /// Data came in from another device or a file and was written to
        /// this device's storage: the shell and modules read it anew.
        /// `sources` are library sources that are new here, to scan.
        fn restored(
            &self,
            settings: bool,
            accounts: bool,
            library: bool,
            history: bool,
            sources: Vec<i64>,
        );
    }

    #[commands]
    pub trait Commands {
        /// Keep the WebDAV folder to sync with; what is left empty keeps
        /// what was saved.
        fn set_webdav(
            &mut self,
            url: String,
            user: String,
            password: String,
            path: String,
        ) -> Result<(), CommandError>;
        /// Open the WebDAV form over the saved folder, or close it.
        fn edit_webdav(&mut self, editing: bool) -> Result<(), CommandError>;
        /// Forget the WebDAV folder and what was last agreed with it.
        fn clear_webdav(&mut self) -> Result<(), CommandError>;
        /// Check that the folder can be reached and written.
        fn test_webdav(&mut self) -> Result<(), CommandError>;
        /// Sync with the WebDAV folder now.
        fn sync_now(&mut self) -> Result<(), CommandError>;
        fn set_auto(&mut self, on: bool) -> Result<(), CommandError>;
        fn set_interval(&mut self, interval: SyncInterval) -> Result<(), CommandError>;
        /// Set the passphrase accounts are sealed with; empty forgets it.
        fn set_passphrase(&mut self, passphrase: String) -> Result<(), CommandError>;
        /// Write everything into one `.erika` file in `folder`.
        fn export_to(&mut self, folder: MediaSource) -> Result<(), CommandError>;
        /// Take in an exported `.erika` file.
        fn import_from(&mut self, file: MediaSource) -> Result<(), CommandError>;
        /// Count what an export would hold.
        fn refresh_parts(&mut self) -> Result<(), CommandError>;
        /// The clock: syncs when the automatic sync is due.
        fn tick(&mut self) -> Result<(), CommandError>;
    }
}
