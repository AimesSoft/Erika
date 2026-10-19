//! Protocol of the anime module: the shows the home page puts on its stage
//! and timeline, and the show whose details are open. Asking to play goes
//! out as the `play` event, which the shell's player answers.

#[bern::protocol(namespace = "anime", version = 1)]
pub mod anime {
    use bern::media::MediaSource;
    use bern::prelude::*;

    /// One tag of a show, such as "战斗", as the show's source names it.
    #[value]
    pub struct Tag {
        #[content]
        pub name: String,
    }

    /// Where the user is with a show.
    #[value]
    pub enum WatchStatus {
        #[label(tr! { "zh-Hans" => "想看", "en" => "Want to watch" })]
        Want,
        #[label(tr! { "zh-Hans" => "在看", "en" => "Watching" })]
        Watching,
        #[label(tr! { "zh-Hans" => "看过", "en" => "Watched" })]
        Watched,
    }

    /// One show.
    #[value]
    pub struct Show {
        /// Stable identity, used as the list key.
        pub id: i64,
        #[content]
        pub title: String,
        pub poster: MediaSource,
        /// Episodes there are, and how many have been watched.
        pub episodes: i64,
        pub watched: i64,
        /// The score out of ten; 0 when the show has no rating.
        pub rating: f64,
        /// A film rather than a TV series, and when it first aired: the
        /// month counts for a series, the year for a film.
        pub movie: bool,
        pub premiere: CalendarDate,
        pub tags: Vec<Tag>,
        #[content]
        pub summary: String,
        pub status: WatchStatus,
    }

    /// A show being watched: the home page's stage and the rows beside it.
    #[value]
    pub struct Watching {
        pub id: i64,
        #[content]
        pub title: String,
        /// The picture on the stage or the thumbnail.
        pub still: MediaSource,
        pub episode: i64,
        /// Minutes left of the episode.
        pub left: i64,
        /// How far the episode has played, from 0 to 1.
        pub progress: f64,
    }

    /// One show airing today, hung on the timeline at its time.
    #[value]
    pub struct Airing {
        pub id: i64,
        #[content]
        pub title: String,
        pub poster: MediaSource,
        /// When it airs, in seconds after midnight, and whether it has.
        pub time: i64,
        pub aired: bool,
        /// Where the time falls on the timeline, from 0 to 1.
        pub at: f64,
    }

    /// One hour marked under the timeline.
    #[value]
    pub struct Hour {
        /// The hour, in seconds after midnight.
        pub time: i64,
        pub at: f64,
    }

    /// A broadcast in the prototype's weekly calendar. Midnight belongs
    /// to the preceding broadcast day, just as it does on the timeline.
    #[value]
    pub struct Broadcast {
        pub id: i64,
        #[content]
        pub title: String,
        pub poster: MediaSource,
        /// When it airs, in seconds after midnight.
        pub time: i64,
        pub episode: i64,
        pub aired: bool,
        pub tonight: bool,
    }

    #[value]
    pub struct BroadcastDay {
        /// Monday is 0, Sunday is 6.
        pub id: i64,
        pub date: CalendarDate,
        pub count: i64,
        pub today: bool,
        pub tomorrow: bool,
        pub day_after: bool,
        pub shows: Vec<Broadcast>,
        /// The four non-overlapping covers shown in a collapsed column.
        pub preview: Vec<Broadcast>,
        pub remaining: i64,
    }

    /// The name of an episode on the film: its number, and whether it is
    /// the one to start or to continue.
    #[value]
    pub enum EpisodeTitle {
        #[label(tr! { "zh-Hans" => "第 {number} 话", "en" => "Episode {number}" })]
        Plain { number: i64 },
        #[label(tr! { "zh-Hans" => "第 {number} 话 · 开始", "en" => "Episode {number} · Start" })]
        Start { number: i64 },
        #[label(tr! { "zh-Hans" => "第 {number} 话 · 继续", "en" => "Episode {number} · Continue" })]
        Continue { number: i64 },
    }

    /// What the film says under an episode.
    #[value]
    pub enum EpisodeNote {
        #[label(tr! { "zh-Hans" => "已看", "en" => "Watched" })]
        Watched,
        #[label(tr! { "zh-Hans" => "剩 {minutes} 分钟", "en" => "{minutes} min left" })]
        Left { minutes: i64 },
        #[label(tr! { "zh-Hans" => "{minutes} 分钟", "en" => "{minutes} min" })]
        Length { minutes: i64 },
    }

    /// One episode on the film of the details page.
    #[value]
    pub struct Episode {
        /// The episode's number, used as the list key and scroll position.
        pub number: i64,
        pub title: EpisodeTitle,
        pub note: EpisodeNote,
        /// The still for the episodes reached; the others show `numeral`.
        pub pictured: bool,
        pub still: MediaSource,
        /// The number written with two digits, "02", alike in every
        /// language.
        #[content]
        pub numeral: String,
        pub watched: bool,
        /// Whether it is the one to watch next, and how far it has played.
        pub next: bool,
        pub progress: f64,
    }

    #[value]
    pub struct SearchEpisode {
        pub id: i64,
        #[content]
        pub title: String,
        pub episode: i64,
    }

    /// What the details' main button does.
    #[value]
    pub enum NextAction {
        #[label(tr! { "zh-Hans" => "开始观看", "en" => "Start watching" })]
        Start,
        #[label(tr! { "zh-Hans" => "继续 第 {episode} 话", "en" => "Continue episode {episode}" })]
        Continue { episode: i64 },
    }

    /// The page the details were opened from, which their back button
    /// returns to.
    #[value]
    pub enum BackTo {
        #[label(tr! { "zh-Hans" => "首页", "en" => "Home" })]
        Home,
        #[label(tr! { "zh-Hans" => "新番", "en" => "New" })]
        Schedule,
        #[label(tr! { "zh-Hans" => "媒体库", "en" => "Library" })]
        Library,
        #[label(tr! { "zh-Hans" => "搜索", "en" => "Search" })]
        Search,
    }

    #[value]
    pub struct SearchResults {
        pub shows: Vec<Show>,
        pub continuing: Vec<SearchEpisode>,
        pub has_shows: bool,
        pub has_continuing: bool,
    }

    #[queries]
    pub trait Queries {
        fn search(&self, text: String, answer: Answer<SearchResults>);
    }

    #[state]
    pub struct State {
        pub today: CalendarDate,
        /// The show on the stage, and the others being watched.
        pub stage: Watching,
        pub also: Vec<Watching>,
        /// Shows airing today, the hours marked under them, and where "now"
        /// falls on the timeline.
        pub airing: Vec<Airing>,
        pub hours: Vec<Hour>,
        pub now_at: f64,
        /// "Now", in seconds after midnight.
        pub now: i64,
        /// Shows aired today so far, and those still to come tonight.
        pub aired_count: i64,
        pub later_count: i64,
        pub week: Vec<BroadcastDay>,
        pub selected_day: i64,
        pub opened_from_schedule: bool,
        /// The show whose details are open; `opened` is 0 when none is.
        pub opened: i64,
        pub show: Show,
        pub episodes: Vec<Episode>,
        /// The episode the main button plays, and what the button says.
        pub next_episode: i64,
        pub next: NextAction,
        pub back: BackTo,
    }

    #[events]
    pub trait Events {
        /// The user asked to play episode `episode` of a show; the shell's
        /// player answers.
        fn play(&self, title: String, episode: i64, still: MediaSource, progress: f64);
    }

    #[commands]
    pub trait Commands {
        /// Open the details of show `id`.
        #[errors("unknown_show")]
        fn open(&mut self, id: i64) -> Result<(), CommandError>;

        /// Expand one day of the calendar, preserving the other columns.
        #[errors("unknown_day")]
        fn select_day(&mut self, day: i64) -> Result<(), CommandError>;

        /// Open a calendar row and return to that same day afterwards.
        #[errors("unknown_show")]
        fn open_scheduled(&mut self, id: i64) -> Result<(), CommandError>;

        /// Open a poster from the media library and keep its return label.
        #[errors("unknown_show")]
        fn open_from_library(&mut self, id: i64) -> Result<(), CommandError>;

        /// Close the details and return to the page.
        fn close(&mut self) -> Result<(), CommandError>;

        /// A result opened above whichever page the user searched from.
        fn open_from_search(&mut self, id: i64) -> Result<(), CommandError>;

        /// Continue any matching show, including shows outside the home stage.
        fn continue_from_search(&mut self, id: i64) -> Result<(), CommandError>;

        /// Put show `id` on the stage, in place of the one there.
        #[errors("unknown_show")]
        fn stage(&mut self, id: i64) -> Result<(), CommandError>;

        /// Play show `id` from where it was left.
        #[errors("unknown_show")]
        fn resume(&mut self, id: i64) -> Result<(), CommandError>;

        /// Play episode `number` of the open show.
        #[errors("unknown_episode")]
        fn play_episode(&mut self, number: i64) -> Result<(), CommandError>;

        /// Mark where the user is with the open show.
        fn set_status(&mut self, status: WatchStatus) -> Result<(), CommandError>;

        /// Rate the open show from 1 to 10.
        fn rate(&mut self, score: i64) -> Result<(), CommandError>;
    }
}
