//! The media library: the shows the user has files of, and the places the
//! files are. A source is a folder the library scans as NipaPlay's scan
//! does; the library browses the sources' folders, lists the videos not
//! matched, and matches videos to episodes, one at a time or a folder at a
//! time, as design/next-screens.js lays it out.

#[bern::protocol(namespace = "library", version = 2)]
pub mod library {
    use bern::prelude::*;

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
        /// The source the wall shows, or 全部来源.
        #[content]
        pub source_name: String,
    }

    /// What a source holds, which says where its videos are recognized:
    /// anime by dandanplay, TV series and movies by the user's own server.
    #[value]
    pub enum SourceContent {
        #[label(tr! { "zh-Hans" => "番剧", "en" => "Anime" })]
        Anime,
        #[label(tr! { "zh-Hans" => "电视剧", "en" => "TV series" })]
        Series,
        #[label(tr! { "zh-Hans" => "电影", "en" => "Movies" })]
        Movie,
    }

    /// What a source is doing, as its row says it.
    #[value]
    pub enum SourceStatus {
        #[label(tr! { "zh-Hans" => "{files} 个文件", "en" => "{files} files" })]
        Ready { files: i64 },
        #[label(tr! { "zh-Hans" => "正在扫描 {done}/{total}", "en" => "Scanning {done}/{total}" })]
        Scanning { done: i64, total: i64 },
        #[label(tr! { "zh-Hans" => "正在识别 {done}/{total}", "en" => "Matching {done}/{total}" })]
        Matching { done: i64, total: i64 },
        #[label(tr! { "zh-Hans" => "找不到这个文件夹", "en" => "Folder not found" })]
        Missing,
    }

    /// A source in the list of sources.
    #[value]
    pub struct LibrarySourceRow {
        pub id: i64,
        #[content]
        pub name: String,
        pub content: SourceContent,
        /// Where it is, as a path.
        #[content]
        pub location: String,
        pub status: SourceStatus,
        pub unmatched: i64,
    }

    /// What a video in a source is.
    #[value]
    pub enum FileNote {
        #[label(tr! { "zh-Hans" => "{anime} · 第 {episode} 话", "en" => "{anime} · Episode {episode}" })]
        Matched {
            #[content]
            anime: String,
            episode: i64,
        },
        #[label(tr! { "zh-Hans" => "{anime} · 第 {episode} 话 · 手动指定", "en" => "{anime} · Episode {episode} · Set by hand" })]
        Manual {
            #[content]
            anime: String,
            episode: i64,
        },
        #[label(tr! { "zh-Hans" => "没认出", "en" => "Not recognized" })]
        Unmatched,
        #[label(tr! { "zh-Hans" => "读不了这个文件", "en" => "Unreadable file" })]
        Unreadable,
    }

    /// A video in a folder, or among those not matched with where it is.
    #[value]
    pub struct LibraryFile {
        /// Which address it is, for the commands.
        pub key: i64,
        #[content]
        pub name: String,
        pub note: FileNote,
        pub matched: bool,
        /// Whether it can be matched: its file could be read.
        pub readable: bool,
        /// The source and folder it is in, as 番剧 › 未整理.
        #[content]
        pub place: String,
    }

    /// A folder inside the folder being browsed.
    #[value]
    pub struct LibraryDir {
        #[content]
        pub name: String,
        pub files: i64,
        pub unmatched: i64,
    }

    /// A step back up the path being browsed: the source is depth 0.
    #[value]
    pub struct LibraryCrumb {
        pub depth: i64,
        #[content]
        pub name: String,
    }

    /// The folder of a source being browsed.
    #[value]
    pub struct LibraryFolder {
        pub source: i64,
        pub content: SourceContent,
        #[content]
        pub title: String,
        /// Where it is, as a path.
        #[content]
        pub location: String,
        /// What leads to it, the source first; it is the last of them.
        pub crumbs: Vec<LibraryCrumb>,
        pub dirs: Vec<LibraryDir>,
        pub files: Vec<LibraryFile>,
        /// It holds two or more videos, which can be matched at once.
        pub can_batch: bool,
        pub empty: bool,
        pub missing: bool,
    }

    /// What a video being matched says under its name.
    #[value]
    pub enum MatchNote {
        #[label(tr! { "zh-Hans" => "现在：{anime} · 第 {episode} 话", "en" => "Now: {anime} · Episode {episode}" })]
        Now {
            #[content]
            anime: String,
            episode: i64,
        },
        #[label(tr! { "zh-Hans" => "现在：没认出", "en" => "Now: not recognized" })]
        Unmatched,
        #[label(tr! { "zh-Hans" => "文件名里没有集数，填一个", "en" => "No episode number in the name; enter one" })]
        NoNumber,
        #[label(tr! { "zh-Hans" => "超出了，《{anime}》共 {total} 话", "en" => "Too high: {anime} has {total} episodes" })]
        Beyond {
            #[content]
            anime: String,
            total: i64,
        },
        #[label(tr! { "zh-Hans" => "和上面那个文件是同一话，算两个版本", "en" => "Same episode as a file above: two versions" })]
        Twin,
        #[label(tr! { "zh-Hans" => "读不了这个文件，没法匹配", "en" => "This file can't be read, so it can't be matched" })]
        Unreadable,
    }

    /// One video being matched.
    #[value]
    pub struct MatchRow {
        pub key: i64,
        #[content]
        pub name: String,
        /// Whether it is matched or skipped.
        pub on: bool,
        /// The episode it goes to; 0 while it has none.
        pub number: i64,
        /// Whether its number is being typed.
        pub editing: bool,
        pub note: MatchNote,
        /// Skipped, in the way, or a second version: worth a look.
        pub odd: bool,
    }

    /// An anime to match videos to, as dandanplay's search finds it.
    #[value]
    pub struct MatchAnime {
        pub id: i64,
        #[content]
        pub title: String,
        pub episodes: i64,
        pub chosen: bool,
    }

    /// Matching videos to episodes: one video (re-matching it) or a
    /// folder's. By number, each video goes to the episode its name gives,
    /// all shifted so the lowest number is `align_to` (a second season
    /// numbered after the first); by order, the videos go to consecutive
    /// episodes from `start`. Single numbers are typed over.
    #[value]
    pub struct Matching {
        pub open: bool,
        pub single: bool,
        /// What is matched: the path of the video, or the folder and its
        /// count.
        #[content]
        pub place: String,
        pub anime: i64,
        #[content]
        pub anime_title: String,
        /// The anime's episodes; 0 while unknown.
        pub episodes: i64,
        pub by_order: bool,
        pub start: i64,
        /// The lowest episode number in the names, 0 when they have none,
        /// and the episode it goes to.
        pub lowest: i64,
        pub align_to: i64,
        pub rows: Vec<MatchRow>,
        /// The rows shown: all of them or, filtered, the odd ones.
        pub shown: Vec<MatchRow>,
        pub odd_only: bool,
        pub all_count: i64,
        pub odd_count: i64,
        pub on_count: i64,
        pub skipped_count: i64,
        pub bad_count: i64,
        /// Whether confirming can go ahead, and whether the one video is
        /// matched now, which can be undone.
        pub ready: bool,
        pub linked: bool,
        pub results: Vec<MatchAnime>,
        pub searching: bool,
        pub search_failed: bool,
    }

    /// What the library says once something is done.
    #[value]
    pub enum LibraryNotice {
        #[label(tr! { "zh-Hans" => "已匹配到《{anime}》第 {episode} 话", "en" => "Matched to {anime} episode {episode}" })]
        MatchedOne {
            #[content]
            anime: String,
            episode: i64,
        },
        #[label(tr! { "zh-Hans" => "已把 {count} 个文件匹配到《{anime}》第 {first}–{last} 话", "en" => "Matched {count} files to {anime} episodes {first}–{last}" })]
        MatchedMany {
            count: i64,
            #[content]
            anime: String,
            first: i64,
            last: i64,
        },
        #[label(tr! { "zh-Hans" => "已解除匹配，文件回到「没认出的文件」", "en" => "Unlinked; the file is back among those not recognized" })]
        Unlinked,
        #[label(tr! { "zh-Hans" => "没能写入，什么都没改", "en" => "Couldn't save; nothing changed" })]
        MatchFailed,
        #[label(tr! { "zh-Hans" => "{name} 扫描完了：{files} 个文件，{unmatched} 个没认出", "en" => "Scanned {name}: {files} files, {unmatched} not recognized" })]
        Scanned {
            #[content]
            name: String,
            files: i64,
            unmatched: i64,
        },
        #[label(tr! { "zh-Hans" => "登录弹弹play 后才能自动认出番剧", "en" => "Sign in to dandanplay to recognize shows" })]
        SignInToMatch,
        #[label(tr! { "zh-Hans" => "电视剧和电影要先在 设置 › 账号 里设置识别服务器", "en" => "Set a recognition server under Settings › Account for TV series and movies" })]
        NoServer,
        #[label(tr! { "zh-Hans" => "{name} 改为{content}，重新识别没认出的文件", "en" => "{name} now holds {content}; recognizing its files again" })]
        Retyped {
            #[content]
            name: String,
            content: SourceContent,
        },
        #[label(tr! { "zh-Hans" => "读不了这个文件夹", "en" => "Couldn't read this folder" })]
        FolderUnreadable,
        #[label(tr! { "zh-Hans" => "这里还不能扫描文件夹", "en" => "Folders can't be scanned here yet" })]
        CannotScan,
        #[label(tr! { "zh-Hans" => "已移除 {name}", "en" => "Removed {name}" })]
        Removed {
            #[content]
            name: String,
        },
    }

    #[state]
    pub struct State {
        pub posters: Vec<LibraryPoster>,
        /// The shows added last, newest first, for the home page, which
        /// shows them only when there are some.
        pub recent: Vec<LibraryPoster>,
        pub has_recent: bool,
        pub opened: i64,
        pub show: LibraryPoster,
        /// The folders the library scans, and whether there are any, or
        /// several (the wall can then show one of them).
        pub sources: Vec<LibrarySourceRow>,
        pub has_sources: bool,
        pub several_sources: bool,
        /// The videos in the sources not matched to an episode.
        pub unmatched: Vec<LibraryFile>,
        pub unmatched_count: i64,
        pub browsing: bool,
        pub folder: LibraryFolder,
        pub matching: Matching,
        pub notice: Option<LibraryNotice>,
        pub notice_revision: i64,
    }

    #[events]
    pub trait Events {
        fn open_show(&self, id: i64);
        /// Videos were matched, unlinked or removed: what the user has of
        /// each anime changed.
        fn files_changed(&self);
    }

    #[commands]
    pub trait Commands {
        #[errors("unknown_show")]
        fn open(&mut self, id: i64) -> Result<(), CommandError>;
        fn close(&mut self) -> Result<(), CommandError>;
        /// The shell played or matched a video: the shows the user has
        /// files of may have changed.
        fn history_changed(&mut self) -> Result<(), CommandError>;
        /// The dandanplay account signed in or out; signing in matches the
        /// videos not matched yet.
        fn account_changed(&mut self, signed_in: bool) -> Result<(), CommandError>;

        /// Add a folder holding `content` and scan it.
        fn add_folder(
            &mut self,
            folder: MediaSource,
            content: SourceContent,
        ) -> Result<(), CommandError>;
        /// Say what a source holds; its videos not matched are matched
        /// again where that content is recognized.
        #[errors("unknown_source")]
        fn set_content(&mut self, source: i64, content: SourceContent) -> Result<(), CommandError>;
        #[errors("unknown_source")]
        fn rescan(&mut self, source: i64) -> Result<(), CommandError>;
        #[errors("unknown_source")]
        fn remove_source(&mut self, source: i64) -> Result<(), CommandError>;

        /// Browse a source's folders.
        #[errors("unknown_source")]
        fn open_source(&mut self, source: i64) -> Result<(), CommandError>;
        fn open_dir(&mut self, name: String) -> Result<(), CommandError>;
        fn go_up(&mut self) -> Result<(), CommandError>;
        /// Back to crumb `depth`: 0 is the source.
        fn go_to(&mut self, depth: i64) -> Result<(), CommandError>;
        fn close_source(&mut self) -> Result<(), CommandError>;

        /// Match one video by hand, or match it anew.
        #[errors("unknown_file")]
        fn assign(&mut self, key: i64) -> Result<(), CommandError>;
        /// Say a video is no episode, which scans then leave alone.
        #[errors("unknown_file")]
        fn unlink(&mut self, key: i64) -> Result<(), CommandError>;
        /// Match the videos of the folder being browsed.
        fn match_folder(&mut self) -> Result<(), CommandError>;
        fn match_search(&mut self, text: String) -> Result<(), CommandError>;
        fn match_anime(&mut self, id: i64) -> Result<(), CommandError>;
        fn match_by_order(&mut self, by_order: bool) -> Result<(), CommandError>;
        fn match_start(&mut self, text: String) -> Result<(), CommandError>;
        fn match_align(&mut self, text: String) -> Result<(), CommandError>;
        fn match_edit(&mut self, key: i64) -> Result<(), CommandError>;
        fn match_number(&mut self, key: i64, text: String) -> Result<(), CommandError>;
        fn match_toggle(&mut self, key: i64) -> Result<(), CommandError>;
        fn match_odd(&mut self, only: bool) -> Result<(), CommandError>;
        fn match_confirm(&mut self) -> Result<(), CommandError>;
        fn match_unlink(&mut self) -> Result<(), CommandError>;
        fn match_cancel(&mut self) -> Result<(), CommandError>;
    }

    #[queries]
    pub trait Queries {
        /// The wall: the shows of source `source`, or of all when it is 0.
        #[reads(posters)]
        fn browse(
            &self,
            source: i64,
            filter: LibraryFilter,
            sort: LibrarySort,
            answer: Answer<LibraryListing>,
        );
    }
}
