//! Downloads: BitTorrent tasks from magnet links and .torrent files, as
//! NipaPlay's downloader runs them (librqbit). A torrent is read first to
//! list its files; tasks go to a folder the user picks, each in a folder of
//! its own if they like; a video can play while it downloads; a finished
//! download can join the library.
#[bern::protocol(namespace = "downloads", version = 2)]
pub mod downloads {
    use bern::media::MediaSource;
    use bern::prelude::*;

    #[value]
    pub enum DownloadTab {
        #[label(tr! { "zh-Hans" => "进行中", "en" => "In progress" })]
        Active,
        #[label(tr! { "zh-Hans" => "已完成", "en" => "Completed" })]
        Completed,
    }

    /// How large a download is, once known.
    #[value]
    pub enum DataSize {
        #[label(tr! { "zh-Hans" => "—", "en" => "—" })]
        Unknown,
        #[label(tr! { "zh-Hans" => "{kilobytes} KB", "en" => "{kilobytes} KB" })]
        Kilobytes { kilobytes: f64 },
        #[label(tr! { "zh-Hans" => "{megabytes} MB", "en" => "{megabytes} MB" })]
        Megabytes { megabytes: f64 },
        #[label(tr! { "zh-Hans" => "{gigabytes} GB", "en" => "{gigabytes} GB" })]
        Gigabytes { gigabytes: f64 },
    }

    /// A file of a download the player plays.
    #[value]
    pub struct DownloadVideo {
        pub index: i64,
        #[content]
        pub name: String,
    }

    #[value]
    pub struct DownloadTask {
        pub id: i64,
        /// Its name; empty while its torrent is being read.
        #[content]
        pub title: String,
        pub fetching: bool,
        pub size: DataSize,
        pub progress: f64,
        pub percent: i64,
        /// Megabytes a second, down and up.
        pub speed: f64,
        pub upload: f64,
        pub running: bool,
        pub completed: bool,
        pub failed: bool,
        #[content]
        pub error: String,
        pub first: bool,
        /// Its videos; with one, playing needs no choice.
        pub videos: Vec<DownloadVideo>,
        pub playable: bool,
        pub single: bool,
        pub first_video: i64,
        /// Where it is written.
        #[content]
        pub folder: String,
    }

    /// A file of the torrent read before adding it.
    #[value]
    pub struct PreviewFile {
        pub index: i64,
        #[content]
        pub name: String,
        pub size: DataSize,
    }

    /// A folder downloaded into before, to pick again.
    #[value]
    pub struct SavePlace {
        pub id: i64,
        /// The folder's own name, and its whole path.
        #[content]
        pub name: String,
        #[content]
        pub path: String,
        pub chosen: bool,
    }

    /// Why a task cannot be added.
    #[value]
    pub enum AddProblem {
        #[label(tr! { "zh-Hans" => "请输入 magnet 链接，或选择种子文件", "en" => "Enter a magnet link, or choose a torrent file" })]
        Empty,
        #[label(tr! { "zh-Hans" => "链接格式不是有效的 magnet 地址", "en" => "That isn't a magnet link" })]
        NotMagnet,
        #[label(tr! { "zh-Hans" => "解析失败：{detail}", "en" => "Couldn't read it: {detail}" })]
        Unreadable {
            #[content]
            detail: String,
        },
        #[label(tr! { "zh-Hans" => "添加失败：{detail}", "en" => "Couldn't add it: {detail}" })]
        Failed {
            #[content]
            detail: String,
        },
        #[label(tr! { "zh-Hans" => "这里还不能下载", "en" => "Downloads aren't available here" })]
        Unavailable,
    }

    /// Adding a task: the link or the torrent file read, and where it goes.
    #[value]
    pub struct AddSheet {
        pub open: bool,
        /// The torrent is being read from the swarm or the file.
        pub reading: bool,
        pub read: bool,
        #[content]
        pub name: String,
        pub size: DataSize,
        pub file_count: i64,
        /// The first files, for a look.
        pub files: Vec<PreviewFile>,
        pub more_files: i64,
        #[content]
        pub folder_name: String,
        /// The folder downloaded into, and those used before.
        #[content]
        pub place: String,
        pub places: Vec<SavePlace>,
        pub has_places: bool,
        pub own_folder: bool,
        pub problem: Option<AddProblem>,
    }

    /// What the downloads say once something is done.
    #[value]
    pub enum DownloadNotice {
        #[label(tr! { "zh-Hans" => "下载完成：{title}", "en" => "Download finished: {title}" })]
        Finished {
            #[content]
            title: String,
        },
        #[label(tr! { "zh-Hans" => "已添加下载任务", "en" => "Download added" })]
        Added,
        #[label(tr! { "zh-Hans" => "已移除下载任务，文件留着", "en" => "Download removed; its files stay" })]
        Removed,
        #[label(tr! { "zh-Hans" => "已删除任务和文件", "en" => "Download and files deleted" })]
        Deleted,
        #[label(tr! { "zh-Hans" => "已暂停下载", "en" => "Download paused" })]
        Paused,
        #[label(tr! { "zh-Hans" => "已继续下载", "en" => "Download resumed" })]
        Resumed,
        #[label(tr! { "zh-Hans" => "尚未获取到可播放的视频文件，请稍后再试", "en" => "No playable video yet; try again later" })]
        NothingToPlay,
        #[label(tr! { "zh-Hans" => "操作失败：{detail}", "en" => "That didn't work: {detail}" })]
        Failed {
            #[content]
            detail: String,
        },
    }

    #[state]
    pub struct State {
        pub tab: DownloadTab,
        pub active: Vec<DownloadTask>,
        pub completed: Vec<DownloadTask>,
        pub active_count: i64,
        pub completed_count: i64,
        /// Something downloads or is being read: the clock ticks.
        pub running: bool,
        /// Whether downloads run here at all.
        pub available: bool,
        pub adding: AddSheet,
        /// The task to delete with its files once confirmed; 0 when none.
        pub deleting: i64,
        #[content]
        pub deleting_title: String,
        /// Whether finished downloads join the library.
        pub to_library: bool,
        pub notice: Option<DownloadNotice>,
        pub notice_revision: i64,
    }

    #[events]
    pub trait Events {
        /// Play a video of a download: its file once done, or a stream of
        /// it while it downloads.
        fn play(&self, title: String, source: MediaSource, downloading: bool);
        /// A download finished in `folder`, which the library may scan.
        fn finished(&self, folder: MediaSource);
    }

    #[commands]
    pub trait Commands {
        fn set_tab(&mut self, tab: DownloadTab) -> Result<(), CommandError>;
        fn open_add(&mut self) -> Result<(), CommandError>;
        fn close_add(&mut self) -> Result<(), CommandError>;
        /// Read a magnet link's torrent to list its files.
        fn read_link(&mut self, link: String) -> Result<(), CommandError>;
        /// Read a .torrent file.
        fn read_file(&mut self, file: MediaSource) -> Result<(), CommandError>;
        /// Download into another folder.
        fn choose_place(&mut self, folder: MediaSource) -> Result<(), CommandError>;
        fn use_place(&mut self, id: i64) -> Result<(), CommandError>;
        fn forget_place(&mut self, id: i64) -> Result<(), CommandError>;
        fn set_own_folder(&mut self, on: bool) -> Result<(), CommandError>;
        /// Add the torrent read, or read `link` and add it.
        fn add(&mut self, link: String) -> Result<(), CommandError>;
        #[errors("unknown_task")]
        fn toggle(&mut self, id: i64) -> Result<(), CommandError>;
        #[errors("unknown_task")]
        fn play(&mut self, id: i64, video: i64) -> Result<(), CommandError>;
        /// Remove a task and keep its files.
        #[errors("unknown_task")]
        fn remove(&mut self, id: i64) -> Result<(), CommandError>;
        /// Ask before deleting a task with its files.
        #[errors("unknown_task")]
        fn ask_delete(&mut self, id: i64) -> Result<(), CommandError>;
        fn cancel_delete(&mut self) -> Result<(), CommandError>;
        fn delete(&mut self) -> Result<(), CommandError>;
        fn set_to_library(&mut self, on: bool) -> Result<(), CommandError>;
        /// Read the tasks' progress anew; the page's clock calls it.
        fn tick(&mut self) -> Result<(), CommandError>;
    }
}
