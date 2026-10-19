//! The design's media library: browsing stays in the window's session;
//! the engine supplies stable poster identities and the filtered index.

#[bern::protocol(namespace = "library", version = 1)]
pub mod library {
    use bern::prelude::*;

    #[value]
    pub enum LibrarySource {
        #[label(tr! { "zh-Hans" => "本地", "en" => "Local" })]
        Local,
        #[label(tr! { "zh-Hans" => "服务器", "en" => "Servers" })]
        Server,
        #[label(tr! { "zh-Hans" => "共享", "en" => "Shared" })]
        Shared,
    }

    #[value]
    pub enum LibraryFilter {
        #[label(tr! { "zh-Hans" => "全部", "en" => "All" })]
        All,
        #[label(tr! { "zh-Hans" => "在看", "en" => "Watching" })]
        Watching,
        #[label(tr! { "zh-Hans" => "未看", "en" => "Unwatched" })]
        Unwatched,
        #[label(tr! { "zh-Hans" => "看完", "en" => "Finished" })]
        Finished,
    }

    #[value]
    pub enum LibrarySort {
        #[label(tr! { "zh-Hans" => "最近观看", "en" => "Recently watched" })]
        Recent,
        #[label(tr! { "zh-Hans" => "名称", "en" => "Name" })]
        Name,
        #[label(tr! { "zh-Hans" => "评分", "en" => "Rating" })]
        Rating,
    }

    #[value]
    pub struct LibraryPoster {
        pub id: i64,
        #[content]
        pub title: String,
        pub poster: MediaSource,
        /// The letter the title files under in the index.
        #[content]
        pub initial: String,
        pub episodes: i64,
        pub watched: i64,
        pub rating: f64,
        pub progress: f64,
        pub watching: bool,
        pub movie: bool,
    }

    #[value]
    pub struct LibraryLetter {
        #[content]
        pub letter: String,
        /// The first visible show with this initial, or zero if absent.
        pub target: i64,
    }

    #[value]
    pub struct LibraryFilterChip {
        pub id: String,
        pub value: LibraryFilter,
        pub count: i64,
    }

    #[value]
    pub struct LibraryListing {
        pub filters: Vec<LibraryFilterChip>,
        pub posters: Vec<LibraryPoster>,
        pub letters: Vec<LibraryLetter>,
        pub total: i64,
        pub watching: i64,
        pub unwatched: i64,
        pub finished: i64,
        pub empty: bool,
    }

    #[state]
    pub struct State {
        pub posters: Vec<LibraryPoster>,
        /// The shows added last, newest first, for the home page.
        pub recent: Vec<LibraryPoster>,
        pub opened: i64,
        pub show: LibraryPoster,
        pub notice: i64,
    }

    #[events]
    pub trait Events {
        fn open_show(&self, id: i64);
    }

    #[commands]
    pub trait Commands {
        #[errors("unknown_show")]
        fn open(&mut self, id: i64) -> Result<(), CommandError>;
        fn close(&mut self) -> Result<(), CommandError>;
        /// First-phase management feedback, matching the prototype.
        fn manage(&mut self) -> Result<(), CommandError>;
    }

    #[queries]
    pub trait Queries {
        #[reads(posters)]
        fn browse(
            &self,
            source: LibrarySource,
            filter: LibraryFilter,
            sort: LibrarySort,
            answer: Answer<LibraryListing>,
        );
    }
}
