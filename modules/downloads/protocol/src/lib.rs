//! Download presentation state. The first phase uses deterministic sample tasks.
#[bern::protocol(namespace = "downloads", version = 1)]
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
        #[label(tr! { "zh-Hans" => "{megabytes} MB", "en" => "{megabytes} MB" })]
        Megabytes { megabytes: f64 },
        #[label(tr! { "zh-Hans" => "{gigabytes} GB", "en" => "{gigabytes} GB" })]
        Gigabytes { gigabytes: f64 },
    }
    #[value]
    pub struct DownloadTask {
        pub id: i64,
        /// The file's name; empty while its metadata is being fetched.
        #[content]
        pub title: String,
        pub fetching: bool,
        pub size: DataSize,
        pub progress: f64,
        pub speed: f64,
        pub running: bool,
        pub completed: bool,
        pub percent: i64,
        pub first: bool,
    }
    #[state]
    pub struct State {
        pub tab: DownloadTab,
        pub active: Vec<DownloadTask>,
        pub completed: Vec<DownloadTask>,
        pub active_count: i64,
        pub completed_count: i64,
        pub running: bool,
        /// The download finished last, and a number that changes with each.
        #[content]
        pub finished: String,
        pub notice_revision: i64,
    }
    #[events]
    pub trait Events {
        fn play(&self, title: String, still: MediaSource, progress: f64);
    }
    #[commands]
    pub trait Commands {
        fn set_tab(&mut self, tab: DownloadTab) -> Result<(), CommandError>;
        fn add_task(&mut self) -> Result<(), CommandError>;
        fn toggle(&mut self, id: i64) -> Result<(), CommandError>;
        fn play(&mut self, id: i64) -> Result<(), CommandError>;
        /// UI prototype clock, not a transfer engine. One tick is 500 ms.
        fn tick(&mut self) -> Result<(), CommandError>;
    }
}
