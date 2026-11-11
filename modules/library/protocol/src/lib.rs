//! The media library: the shows the user has files of, and the places the
//! files are. A source is a folder the library scans as NipaPlay's scan
//! does; the library browses the sources' folders, lists the videos not
//! matched, and matches videos to episodes, one at a time or a folder at a
//! time, as design/next-screens.js lays it out.

#[bern::protocol(namespace = "library", version = 5)]
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
        /// Canonical UUID; never an encoded service ID.
        pub id: String,
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
        /// The first visible show's UUID, or empty if absent.
        pub target: String,
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

    /// A search hint, not a choice of identity or danmaku provider.
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
        #[label(tr! { "zh-Hans" => "共享设备离线", "en" => "Shared device offline" })]
        Offline,
    }

    /// A source in the list of sources.
    #[value]
    pub struct LibrarySourceRow {
        pub id: i64,
        pub shared: bool,
        /// A dandanplay PC client's remote access, which is shared too.
        pub dandanplay: bool,
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
        #[label(tr! { "zh-Hans" => "{title}", "en" => "{title}" })]
        Content {
            #[content]
            title: String,
        },
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
        pub show: String,
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
        pub shared: bool,
        pub dandanplay: bool,
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
        #[label(tr! { "zh-Hans" => "现在：{title}", "en" => "Now: {title}" })]
        Now {
            #[content]
            title: String,
        },
        #[label(tr! { "zh-Hans" => "{reason}", "en" => "{reason}" })]
        Invalid {
            #[content]
            reason: String,
        },
        #[label(tr! { "zh-Hans" => "现在：没认出", "en" => "Now: not recognized" })]
        Unmatched,
        #[label(tr! { "zh-Hans" => "与其他文件覆盖了相同分集", "en" => "Overlaps episodes covered by another file" })]
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
        /// An episode number, list or range; empty for movies or unassigned rows.
        #[content]
        pub assignment: String,
        /// Actual destination, including S00; movies have no synthetic number.
        #[content]
        pub destination: String,
        /// Whether its number is being typed.
        pub editing: bool,
        pub note: MatchNote,
        /// Skipped, in the way, or a second version: worth a look.
        pub odd: bool,
    }

    /// A TMDB result. TV and movie IDs never share a namespace.
    #[value]
    pub struct MatchWork {
        pub id: String,
        #[content]
        pub title: String,
        #[content]
        pub detail: String,
        pub chosen: bool,
    }

    #[value]
    pub struct MatchSeason {
        pub id: String,
        #[content]
        pub title: String,
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
        pub work: String,
        #[content]
        pub title: String,
        pub movie: bool,
        pub seasons: Vec<MatchSeason>,
        pub season: String,
        pub loading: bool,
        #[content]
        pub error: String,
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
        pub results: Vec<MatchWork>,
        pub searching: bool,
        pub search_failed: bool,
    }

    /// What the library says once something is done.
    #[value]
    pub enum LibraryNotice {
        #[label(tr! { "zh-Hans" => "{reason}", "en" => "{reason}" })]
        Failed {
            #[content]
            reason: String,
        },
        #[label(tr! { "zh-Hans" => "已把 {count} 个文件匹配到《{title}》", "en" => "Matched {count} files to {title}" })]
        Matched {
            count: i64,
            #[content]
            title: String,
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

    /// Typed public catalogue API; numbers never encode another provider.
    #[value]
    pub enum CatalogKind { Tv, Movie, Season, Episode, Subject, Item }
    #[value]
    pub enum CatalogProvider { Tmdb, Dandanplay, Bangumi, Misaka, Jellyfin }
    #[value]
    pub enum CatalogRelation { Exact, PartOf, Contains, Related }
    #[value]
    pub enum CatalogDecision { Auto, Manual, Blocked }
    #[value]
    pub struct CatalogReference {
        pub provider: CatalogProvider,
        pub scope: String,
        pub kind: CatalogKind,
        pub id: String,
    }
    #[value]
    pub struct CatalogNode {
        pub id: String,
        pub kind: CatalogKind,
        pub root: String,
        pub parent: Option<String>,
        pub season_number: Option<i64>,
        pub episode_number: Option<i64>,
        pub reference: CatalogReference,
        #[content]
        pub title: String,
        #[content]
        pub overview: String,
        pub date: String,
        pub revision: i64,
    }
    #[value]
    pub struct CatalogItem {
        pub media: String,
        pub start_seconds: Option<f64>,
        pub end_seconds: Option<f64>,
    }
    #[value]
    pub struct CatalogBinding {
        pub file: i64,
        pub revision: i64,
        pub decision: CatalogDecision,
        pub items: Vec<CatalogItem>,
        pub evidence: String,
    }
    #[value]
    pub struct CatalogDanmaku {
        pub file: i64,
        pub revision: i64,
        pub reference: CatalogReference,
        pub media: Option<String>,
        pub manual: bool,
        pub fingerprint_verified: bool,
        pub shift_seconds: f64,
        pub user_shift_seconds: f64,
    }
    #[value]
    pub struct CatalogMapping {
        pub media: String,
        pub reference: CatalogReference,
        pub relation: CatalogRelation,
        pub coverage: Vec<String>,
        pub evidence: String,
        pub confirmed: bool,
        pub manual: bool,
        pub revision: i64,
    }
    #[value]
    pub struct CatalogInspection {
        pub file: i64,
        pub revision: i64,
        pub danmaku_revision: i64,
        pub binding: Option<CatalogBinding>,
        pub danmaku: Option<CatalogDanmaku>,
        pub nodes: Vec<CatalogNode>,
        pub mappings: Vec<CatalogMapping>,
    }
    #[value]
    pub struct CatalogSearch {
        pub nodes: Vec<CatalogNode>,
        pub page: i64,
        pub total_pages: i64,
    }
    #[value]
    pub struct CatalogReceipt {
        pub request: String,
        pub change: String,
        pub preview: bool,
        pub bindings: Vec<CatalogBinding>,
        pub danmaku: Vec<CatalogDanmaku>,
        pub mappings: Vec<CatalogMapping>,
    }
    #[value]
    pub struct CatalogJob {
        pub id: String,
        pub file: Option<i64>,
        pub input_revision: i64,
        pub state: String,
        pub phase: String,
        pub error: String,
        pub next_attempt: i64,
        pub nodes: Vec<CatalogNode>,
        pub items: Vec<CatalogItem>,
        pub reason: String,
    }

    #[state]
    pub struct State {
        pub shared_prompt: bool,
        pub shared_connecting: bool,
        #[content]
        pub shared_error: String,
        /// Mounting a dandanplay PC client's remote access, or editing the
        /// source `remote_editing` (0 while adding one): its name, address
        /// and whether it keeps an API key, which empty fields keep.
        pub remote_prompt: bool,
        pub remote_connecting: bool,
        #[content]
        pub remote_error: String,
        pub remote_editing: i64,
        #[content]
        pub remote_name: String,
        #[content]
        pub remote_address: String,
        pub remote_has_token: bool,
        /// Counts the connections saved, which clears the fields typed.
        pub remote_saved: i64,
        pub posters: Vec<LibraryPoster>,
        /// The shows added last, newest first, for the home page, which
        /// shows them only when there are some.
        pub recent: Vec<LibraryPoster>,
        pub has_recent: bool,
        pub opened: String,
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
        pub catalog_result: Option<CatalogReceipt>,
    }

    #[events]
    pub trait Events {
        /// A poster was opened: from the home page's 「最近添加」 when
        /// `home`, from the library page otherwise.
        fn open_show(&self, id: String, home: bool);
        /// Play a readable source entry even before it has a canonical binding.
        fn play_source(&self, source: MediaSource, title: String);
        /// Videos were matched, unlinked or removed: what the user has of
        /// each anime changed.
        fn files_changed(&self);
    }

    #[commands]
    pub trait Commands {
        /// Read verified TMDB metadata before binding; `request` is a fresh UUID.
        fn catalog_fetch(&mut self, request: String, reference: CatalogReference, season: Option<i64>) -> Result<(), CommandError>;
        /// Resolve one stored file using the normal dandanplay → filename chain.
        fn catalog_resolve(&mut self, request: String, file: i64, revision: i64, dry_run: bool) -> Result<(), CommandError>;
        /// Whole-batch preview and commit use identical validation and CAS rules.
        fn catalog_bind(&mut self, request: String, bindings: Vec<CatalogBinding>, preview: bool) -> Result<(), CommandError>;
        fn catalog_unbind(&mut self, request: String, file: i64, revision: i64, preview: bool) -> Result<(), CommandError>;
        fn catalog_bind_danmaku(&mut self, request: String, bindings: Vec<CatalogDanmaku>, preview: bool) -> Result<(), CommandError>;
        fn catalog_map_provider(&mut self, request: String, mappings: Vec<CatalogMapping>, preview: bool) -> Result<(), CommandError>;
        /// Remove the manual/blocked decision without changing the danmaku choice.
        fn catalog_allow_auto(&mut self, file: i64, revision: i64) -> Result<(), CommandError>;
        fn catalog_undo(&mut self, change: String) -> Result<(), CommandError>;
        #[errors("unknown_show")]
        fn open(&mut self, id: String) -> Result<(), CommandError>;
        /// Open a poster of the home page's 「最近添加」: its details show
        /// over the home page and go back to it.
        #[errors("unknown_show")]
        fn open_recent(&mut self, id: String) -> Result<(), CommandError>;
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
        fn open_shared_mount(&mut self) -> Result<(), CommandError>;
        fn cancel_shared_mount(&mut self) -> Result<(), CommandError>;
        fn mount_shared(&mut self, name: String, address: String) -> Result<(), CommandError>;
        /// Mount a dandanplay PC client's remote access, or change the one
        /// being edited; empty fields keep what it has.
        fn open_remote_mount(&mut self) -> Result<(), CommandError>;
        #[errors("unknown_source")]
        fn edit_remote(&mut self, source: i64) -> Result<(), CommandError>;
        fn cancel_remote_mount(&mut self) -> Result<(), CommandError>;
        fn mount_remote(
            &mut self,
            name: String,
            address: String,
            token: String,
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

        /// Play the current address of a readable file, without requiring a match.
        #[errors("unknown_file")]
        fn play_file(&mut self, key: i64) -> Result<(), CommandError>;
        /// Match one video by hand, or match it anew.
        #[errors("unknown_file")]
        fn assign(&mut self, key: i64) -> Result<(), CommandError>;
        /// Say a video is no episode, which scans then leave alone.
        #[errors("unknown_file")]
        fn unlink(&mut self, key: i64) -> Result<(), CommandError>;
        /// Match the videos of the folder being browsed.
        fn match_folder(&mut self) -> Result<(), CommandError>;
        fn match_search(&mut self, text: String) -> Result<(), CommandError>;
        fn match_work(&mut self, id: String) -> Result<(), CommandError>;
        fn match_season(&mut self, id: String) -> Result<(), CommandError>;
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
        fn catalog_inspect(&self, file: i64, answer: Answer<CatalogInspection>);
        fn catalog_media(&self, id: String, answer: Answer<CatalogNode>);
        fn catalog_children(&self, id: String, answer: Answer<Vec<CatalogNode>>);
        fn catalog_search(&self, kind: CatalogKind, text: String, year: Option<i64>, page: i64, answer: Answer<CatalogSearch>);
        fn catalog_job(&self, id: String, answer: Answer<CatalogJob>);
        fn catalog_receipt(&self, request: String, answer: Answer<CatalogReceipt>);
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
