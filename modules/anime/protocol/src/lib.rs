//! Protocol of the anime module: the shows the home page puts on its stage
//! and timeline, and the show whose details are open. Asking to play goes
//! out as the `play` event, which the shell's player answers.

#[bern::protocol(namespace = "anime", version = 1)]
pub mod anime {
    use bern::media::MediaSource;
    use bern::prelude::*;

    /// One tag of a show, such as "战斗".
    #[value]
    pub struct Tag {
        pub name: String,
    }

    /// One show.
    #[value]
    pub struct Show {
        /// Stable identity, used as the list key.
        pub id: i64,
        pub title: String,
        pub poster: MediaSource,
        /// Episodes there are, and how many have been watched.
        pub episodes: i64,
        pub watched: i64,
        /// "★ 6.1"; empty when the show has no rating.
        pub rating: String,
        /// "TV 动画 · 2026 年 1 月 · 共 12 话".
        pub kind: String,
        pub tags: Vec<Tag>,
        pub summary: String,
        /// Where the user is with the show: "想看", "在看" or "看过".
        pub status: String,
    }

    /// A show being watched: the home page's stage and the rows beside it.
    #[value]
    pub struct Watching {
        pub id: i64,
        pub title: String,
        /// The picture on the stage or the thumbnail.
        pub still: MediaSource,
        pub episode: i64,
        /// "第 1 集", and how much is left: "剩 15 分钟".
        pub episode_label: String,
        pub left: String,
        /// How far the episode has played, from 0 to 1.
        pub progress: f64,
    }

    /// One show airing today, hung on the timeline at its time.
    #[value]
    pub struct Airing {
        pub id: i64,
        pub title: String,
        pub poster: MediaSource,
        /// "18:30 · 已更新".
        pub time: String,
        /// Where the time falls on the timeline, from 0 to 1.
        pub at: f64,
    }

    /// One hour marked under the timeline.
    #[value]
    pub struct Hour {
        /// "18:00".
        pub label: String,
        pub at: f64,
    }

    /// One episode on the film of the details page.
    #[value]
    pub struct Episode {
        /// The episode's number, used as the list key and scroll position.
        pub number: i64,
        /// "第 1 话 · 开始", "第 2 话".
        pub title: String,
        /// "已看", "剩 16 分钟" or "24 分钟".
        pub note: String,
        /// The still for the episodes reached; the others show `numeral`.
        pub pictured: bool,
        pub still: MediaSource,
        /// "02".
        pub numeral: String,
        pub watched: bool,
        /// Whether it is the one to watch next, and how far it has played.
        pub next: bool,
        pub progress: f64,
    }

    #[state]
    pub struct State {
        /// "周一 · 10 月 12 日".
        pub today: String,
        /// The show on the stage, and the others being watched.
        pub stage: Watching,
        pub also: Vec<Watching>,
        /// Shows airing today, the hours marked under them, and where "now"
        /// falls on the timeline.
        pub airing: Vec<Airing>,
        pub hours: Vec<Hour>,
        pub now_at: f64,
        pub now_label: String,
        /// "已更新 4 部 · 今晚还有 3 部".
        pub airing_summary: String,
        /// The show whose details are open; `opened` is 0 when none is.
        pub opened: i64,
        pub show: Show,
        pub episodes: Vec<Episode>,
        /// "共 12 话 · 看到第 0 话".
        pub progress_label: String,
        /// The episode the main button plays, and its label: "开始观看" or
        /// "继续 第 4 话".
        pub next_episode: i64,
        pub next_label: String,
        /// The page the details were opened from: "首页".
        pub back_label: String,
    }

    #[events]
    pub trait Events {
        /// The user asked to play an episode; the shell's player answers.
        fn play(&self, title: String, subtitle: String, still: MediaSource, progress: f64);
    }

    #[commands]
    pub trait Commands {
        /// Open the details of show `id`.
        #[errors("unknown_show")]
        fn open(&mut self, id: i64) -> Result<(), CommandError>;

        /// Close the details and return to the page.
        fn close(&mut self) -> Result<(), CommandError>;

        /// Put show `id` on the stage, in place of the one there.
        #[errors("unknown_show")]
        fn stage(&mut self, id: i64) -> Result<(), CommandError>;

        /// Play show `id` from where it was left.
        #[errors("unknown_show")]
        fn resume(&mut self, id: i64) -> Result<(), CommandError>;

        /// Play episode `number` of the open show.
        #[errors("unknown_episode")]
        fn play_episode(&mut self, number: i64) -> Result<(), CommandError>;

        /// Mark the open show "想看", "在看" or "看过".
        fn set_status(&mut self, status: String) -> Result<(), CommandError>;

        /// Rate the open show from 1 to 10.
        fn rate(&mut self, score: i64) -> Result<(), CommandError>;
    }
}
