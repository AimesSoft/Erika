//! Protocol of the anime module: the shows the home page puts on its stage
//! and timeline, and the show whose details are open. Asking to play goes
//! out as the `play` event, which the shell's player answers.

#[bern::protocol(namespace = "anime", version = 4)]
pub mod anime {
    use bern::media::MediaSource;
    use bern::prelude::*;

    /// One tag of a show, such as "战斗", as the show's source names it.
    #[value]
    pub struct Tag {
        #[content]
        pub name: String,
    }

    /// Local explicit status takes precedence; confirmed provider links determine
    /// which enabled accounts can receive the user's current intent.
    #[value]
    pub enum WatchStatus {
        #[label(tr! { "zh-Hans" => "想看", "en" => "Want to watch" })]
        Want,
        #[label(tr! { "zh-Hans" => "在看", "en" => "Watching" })]
        Watching,
        #[label(tr! { "zh-Hans" => "看过", "en" => "Watched" })]
        Watched,
        #[label(tr! { "zh-Hans" => "抛弃", "en" => "Dropped" })]
        Dropped,
        /// Nothing marked anywhere yet.
        #[label(tr! { "zh-Hans" => "未标记", "en" => "Not marked" })]
        Unmarked,
    }

    /// The user's own score of a show, as Bangumi names the ten steps
    /// (and NipaPlay after it). Without a Bangumi account only the even
    /// ones are offered.
    #[value]
    pub enum Score {
        #[label(tr! { "zh-Hans" => "评分", "en" => "Rate" })]
        Unrated,
        #[label(tr! { "zh-Hans" => "1 分 · 不忍直视", "en" => "1 · Unwatchable" })]
        One,
        #[label(tr! { "zh-Hans" => "2 分 · 很差", "en" => "2 · Very bad" })]
        Two,
        #[label(tr! { "zh-Hans" => "3 分 · 差", "en" => "3 · Bad" })]
        Three,
        #[label(tr! { "zh-Hans" => "4 分 · 较差", "en" => "4 · Poor" })]
        Four,
        #[label(tr! { "zh-Hans" => "5 分 · 不过不失", "en" => "5 · Average" })]
        Five,
        #[label(tr! { "zh-Hans" => "6 分 · 还行", "en" => "6 · Fair" })]
        Six,
        #[label(tr! { "zh-Hans" => "7 分 · 推荐", "en" => "7 · Good" })]
        Seven,
        #[label(tr! { "zh-Hans" => "8 分 · 力荐", "en" => "8 · Great" })]
        Eight,
        #[label(tr! { "zh-Hans" => "9 分 · 神作", "en" => "9 · Masterpiece" })]
        Nine,
        #[label(tr! { "zh-Hans" => "10 分 · 超神作", "en" => "10 · All-time best" })]
        Ten,
    }

    /// Where what the user marks on the details goes besides this device.
    #[value]
    pub enum SyncTargets {
        #[label(tr! { "zh-Hans" => "只保存在本机", "en" => "Kept on this device" })]
        Local,
        #[label(tr! { "zh-Hans" => "同步到弹弹play", "en" => "Synced to dandanplay" })]
        Dandanplay,
        #[label(tr! { "zh-Hans" => "同步到 Bangumi", "en" => "Synced to Bangumi" })]
        Bangumi,
        #[label(tr! { "zh-Hans" => "同步到弹弹play 和 Bangumi", "en" => "Synced to dandanplay and Bangumi" })]
        Both,
    }

    /// One show.
    #[value]
    pub struct Show {
        /// Stable identity, used as the list key.
        pub id: String,
        #[content]
        pub title: String,
        pub poster: MediaSource,
        pub backdrop: MediaSource,
        pub logo: MediaSource,
        /// Episodes there are, and how many have been watched.
        pub episodes: i64,
        pub watched: i64,
        /// The score out of ten; 0 when the show has no rating.
        pub rating: f64,
        /// A film rather than a TV series, and when it first aired: the
        /// month counts for a series, the year for a film.
        pub movie: bool,
        pub premiere: CalendarDate,
        pub has_premiere: bool,
        pub tags: Vec<Tag>,
        #[content]
        pub summary: String,
        pub status: WatchStatus,
    }

    /// A show being watched: the home page's stage and the rows beside it.
    #[value]
    pub struct Watching {
        pub id: String,
        #[content]
        pub title: String,
        /// The picture on the stage or the thumbnail.
        pub still: MediaSource,
        pub episode: i64,
        /// Season-qualified label, empty for a movie or an unknown collection chapter.
        #[content]
        pub chapter: String,
        /// Minutes left of the episode.
        pub left: i64,
        /// How far the episode has played, from 0 to 1.
        pub progress: f64,
    }

    /// One show airing today, in its group on the timeline.
    #[value]
    pub struct Airing {
        pub id: String,
        #[content]
        pub title: String,
        pub poster: MediaSource,
        pub aired: bool,
    }

    /// The shows airing today at the same time, side by side on the
    /// timeline under that time.
    #[value]
    pub struct AiringGroup {
        /// When they air, in seconds after midnight, and whether they have.
        pub time: i64,
        pub aired: bool,
        pub shows: Vec<Airing>,
        /// The posters' width side by side, in points, and where their
        /// leading edge falls on the timeline, from 0 to 1.
        pub width: f64,
        pub at: f64,
        /// How far it is from the middle of the first poster to the middle
        /// of the last, where a bracket joins them; 0 for one poster.
        pub span: f64,
    }

    /// A long empty stretch the timeline shrank, marked with a break.
    #[value]
    pub struct TimelineBreak {
        /// The time of the group before it, which keys it.
        pub after: i64,
        pub at: f64,
    }

    /// A broadcast in the weekly calendar. Midnight belongs to the
    /// preceding broadcast day, just as it does on the timeline.
    #[value]
    pub struct Broadcast {
        pub id: String,
        #[content]
        pub title: String,
        pub poster: MediaSource,
        /// When it airs, in seconds after midnight, when its time is known
        /// (`timed`); otherwise only its day is.
        pub time: i64,
        pub timed: bool,
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
        /// Up to four covers shown in the compact calendar's horizontal header.
        pub preview: Vec<Broadcast>,
        pub remaining: i64,
    }

    /// The name of an episode on the film: its number, and whether it is
    /// the one to start or to continue.
    #[value]
    pub enum EpisodeTitle {
        #[label(tr! { "zh-Hans" => "{label}", "en" => "{label}" })]
        Content {
            #[content]
            label: String,
        },
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
        #[label(tr! { "zh-Hans" => "剩 {minutes} 分钟", "en" => "{minutes} min left" })]
        Left { minutes: i64 },
        #[label(tr! { "zh-Hans" => "{minutes} 分钟", "en" => "{minutes} min" })]
        Length { minutes: i64 },
        /// The user has a file of it, not played yet.
        #[label(tr! { "zh-Hans" => "未看", "en" => "Not watched" })]
        Unwatched,
        /// The user has no file of it yet.
        #[label(tr! { "zh-Hans" => "没有文件", "en" => "No file" })]
        Missing,
        /// A collection without per-item timestamps resumes the file, not a guessed episode.
        #[label(tr! { "zh-Hans" => "合辑 · 文件续播", "en" => "Collection · resume file" })]
        Collection,
        /// The user has a file of it, of a length not known, and watched it.
        #[label(tr! { "zh-Hans" => "本地文件", "en" => "Local file" })]
        File,
    }

    /// One episode on the film of the details page.
    #[value]
    pub struct Episode {
        /// Canonical episode or movie identity; display numbers are not keys.
        pub id: String,
        pub number: i64,
        pub season: i64,
        pub title: EpisodeTitle,
        pub note: EpisodeNote,
        /// The still for the episodes reached; the others show `numeral`.
        pub pictured: bool,
        pub still: MediaSource,
        /// The number written with two digits, "02", alike in every
        /// language.
        #[content]
        pub numeral: String,
        /// Watched: played to the end here, or marked watched here, on
        /// dandanplay or on Bangumi. The film writes 「· 已看」 after the
        /// note.
        pub watched: bool,
        /// Whether it is the one to watch next, and how far it has played.
        pub next: bool,
        pub progress: f64,
    }

    #[value]
    pub struct SearchEpisode {
        pub id: String,
        #[content]
        pub title: String,
        pub episode: i64,
        #[content]
        pub chapter: String,
    }

    /// What the details' main button does.
    #[value]
    pub enum NextAction {
        #[label(tr! { "zh-Hans" => "开始观看", "en" => "Start watching" })]
        Start,
        #[label(tr! { "zh-Hans" => "继续观看", "en" => "Continue watching" })]
        Resume,
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

    /// Preferred catalogue for the New Shows calendar when both accounts exist.
    #[value]
    pub enum SeasonSource {
        #[label(tr! { "zh-Hans" => "弹弹play", "en" => "dandanplay" })]
        Dandanplay,
        #[label(tr! { "zh-Hans" => "Bangumi", "en" => "Bangumi" })]
        Bangumi,
    }

    #[value]
    pub struct SeasonChoice {
        pub id: String,
        #[content]
        pub title: String,
        pub number: i64,
    }

    #[value]
    pub enum MappingProvider {
        #[label(tr! { "zh-Hans" => "Bangumi", "en" => "Bangumi" })]
        Bangumi,
        #[label(tr! { "zh-Hans" => "弹弹play", "en" => "dandanplay" })]
        Dandanplay,
    }

    #[value]
    pub enum MappingKind {
        #[label(tr! { "zh-Hans" => "作品条目", "en" => "Subject" })]
        Subject,
        #[label(tr! { "zh-Hans" => "分集条目", "en" => "Episode" })]
        Episode,
    }

    #[value]
    pub enum MappingRelation {
        #[label(tr! { "zh-Hans" => "相关作品", "en" => "Related" })]
        Related,
        #[label(tr! { "zh-Hans" => "精确对应", "en" => "Exact match" })]
        Exact,
        #[label(tr! { "zh-Hans" => "属于对方的一部分", "en" => "Part of the target" })]
        PartOf,
        #[label(tr! { "zh-Hans" => "包含对方", "en" => "Contains the target" })]
        Contains,
    }

    #[value]
    pub struct MappingTarget {
        pub id: String,
        #[content]
        pub title: String,
    }

    #[value]
    pub struct MappingRow {
        /// Opaque key in the editor's snapshot, not an external ID.
        pub id: String,
        pub provider: MappingProvider,
        pub kind: MappingKind,
        #[content]
        pub reference: String,
        pub relation: MappingRelation,
    }

    #[state]
    pub struct State {
        /// Whether either catalogue account is signed in. Public browsing
        /// remains available without an account.
        pub signed_in: bool,
        /// Effective first choice; a single signed-in account takes precedence.
        pub season_source: SeasonSource,
        pub can_choose_season_source: bool,
        pub loading: bool,
        #[content]
        pub error: String,
        /// The season, as the schedule's heading names it.
        #[content]
        pub season_label: String,
        /// Whether anything is being watched, for the stage, and whether
        /// anything airs today at a known time, for the timeline.
        pub has_stage: bool,
        pub has_airing: bool,
        pub today: CalendarDate,
        /// The show on the stage, and the others being watched.
        pub stage: Watching,
        pub also: Vec<Watching>,
        /// Shows airing today in groups by time, the breaks between groups
        /// far apart, and where "now" falls on the timeline. The timeline is
        /// `track_width` points long, `track_wide` on a large window, and
        /// scrolls when that is more than there is room for.
        pub airing: Vec<AiringGroup>,
        pub breaks: Vec<TimelineBreak>,
        pub now_at: f64,
        pub track_width: f64,
        pub track_wide: f64,
        /// "Now", in seconds after midnight.
        pub now: i64,
        /// Shows aired today so far, and those still to come tonight.
        pub aired_count: i64,
        pub later_count: i64,
        pub week: Vec<BroadcastDay>,
        pub selected_day: i64,
        pub opened_from_schedule: bool,
        /// The open work's canonical UUID, empty when closed.
        pub opened: String,
        /// Opaque lifetime of this details visit; changes even when reopening the same media.
        pub view_generation: String,
        pub show: Show,
        pub details_loading: bool,
        pub details_failed: bool,
        #[content]
        pub details_error: String,
        pub seasons: Vec<SeasonChoice>,
        pub selected_season: String,
        #[content]
        pub selected_season_title: String,
        pub episodes: Vec<Episode>,
        /// Stable scroll targets within the selected season, not episode numbers.
        pub first_episode: String,
        pub last_episode: String,
        /// The episode the main button plays, and what the button says.
        pub next_episode: String,
        pub next: NextAction,
        pub back: BackTo,
        pub comments: Vec<Review>,
        pub comments_loading: bool,
        pub has_comments: bool,
        #[content]
        pub comments_source: String,
        #[content]
        pub comments_error: String,
        /// The user's own score of the open show; whether there is an
        /// account to send it to, and Bangumi's, which takes all ten
        /// steps and the review.
        pub my_score: Score,
        pub can_rate: bool,
        pub fine_scores: bool,
        pub can_review: bool,
        pub sync_targets: SyncTargets,
        /// Whether the open show's marks are being synced, and what the
        /// last sync left to say: a status kept because an account had
        /// one further along, or what failed.
        pub syncing: bool,
        #[content]
        pub sync_note: String,
        /// The dialogs of the score and of the review.
        pub rating_open: bool,
        pub review_open: bool,
        pub review_loading: bool,
        pub review_saving: bool,
        /// The review the editor starts from: the one on Bangumi, or what
        /// the user last sent.
        #[content]
        pub review_text: String,
        #[content]
        pub review_error: String,
        /// Whether each episode of the film has its own button to mark it
        /// watched or not. Off until a plugin turns it on through
        /// `set_episode_marking`.
        pub episode_marking: bool,
        pub can_map: bool,
        pub mapping_open: bool,
        pub mapping_targets: Vec<MappingTarget>,
        pub mapping_target: String,
        pub mapping_movie: bool,
        pub mapping_episode: bool,
        pub mapping_rows: Vec<MappingRow>,
        #[content]
        pub mapping_error: String,
    }

    #[value]
    pub struct Review {
        pub id: String,
        #[content]
        pub author: String,
        pub avatar: MediaSource,
        #[content]
        pub text: String,
        pub rating: i64,
        #[content]
        pub updated: String,
    }

    #[events]
    pub trait Events {
        /// Play an exact file position in seconds. Empty media means file-only
        /// resume (a collection whose active item cannot be established).
        fn play_media(&self, media: String, source: MediaSource, position: f64, from_stage: bool);

        /// The user asked to play episode `episode` of a show, from the
        /// file at `source` (empty when the user has none of it); the
        /// shell's player answers. `from_stage`: asked on the home page's
        /// stage, whose picture flies into the player's frame.
        fn play(
            &self,
            title: String,
            episode: i64,
            still: MediaSource,
            progress: f64,
            source: MediaSource,
            from_stage: bool,
        );
    }

    #[commands]
    pub trait Commands {
        /// Open the details of show `id`.
        #[errors("unknown_show")]
        fn open(&mut self, id: String) -> Result<(), CommandError>;

        /// Expand one day of the calendar, preserving the other columns.
        #[errors("unknown_day")]
        fn select_day(&mut self, day: i64) -> Result<(), CommandError>;

        /// Open a calendar row and return to that same day afterwards.
        #[errors("unknown_show")]
        fn open_scheduled(&mut self, id: String) -> Result<(), CommandError>;

        /// Open a poster from the media library and keep its return label.
        #[errors("unknown_show")]
        fn open_from_library(&mut self, id: String) -> Result<(), CommandError>;

        /// Close the details and return to the page.
        fn close(&mut self) -> Result<(), CommandError>;

        /// A result opened above whichever page the user searched from.
        fn open_from_search(&mut self, id: String) -> Result<(), CommandError>;

        /// Continue any matching show, including shows outside the home stage.
        fn continue_from_search(&mut self, id: String) -> Result<(), CommandError>;

        /// Put show `id` on the stage, in place of the one there.
        #[errors("unknown_show")]
        fn stage(&mut self, id: String) -> Result<(), CommandError>;

        /// Play show `id` from where it was left.
        #[errors("unknown_show")]
        fn resume(&mut self, id: String) -> Result<(), CommandError>;

        fn select_season(&mut self, id: String) -> Result<(), CommandError>;

        /// Play the movie or episode identified by its canonical UUID.
        #[errors("unknown_episode")]
        fn play_episode(&mut self, id: String) -> Result<(), CommandError>;

        /// Mark where the user is with the open show. With an account it
        /// is synced, never over a status further along there.
        fn set_status(&mut self, status: WatchStatus) -> Result<(), CommandError>;

        /// Rate the selected work or season from 1 to 10 on Bangumi; 0 clears it.
        fn rate(&mut self, score: i64) -> Result<(), CommandError>;

        /// Rate the open show with a step of the score dialog, and close
        /// it.
        fn pick_score(&mut self, score: Score) -> Result<(), CommandError>;

        /// Show or hide the score dialog.
        fn show_rating(&mut self, open: bool) -> Result<(), CommandError>;

        /// Show the review dialog, reading the review on Bangumi first,
        /// or hide it.
        fn show_review(&mut self, open: bool) -> Result<(), CommandError>;

        /// Send `text` as the user's review of the open show to Bangumi;
        /// empty takes the review away.
        #[errors("bangumi_required")]
        fn submit_review(&mut self, text: String) -> Result<(), CommandError>;

        /// Mark episode `number` of the open show watched or not, here and
        /// on the accounts. dandanplay cannot take a watched mark back.
        #[errors("unknown_episode")]
        fn mark_episode(&mut self, id: String, watched: bool) -> Result<(), CommandError>;

        /// Turn the film's per-episode mark buttons on or off; the plugin
        /// that offers them turns them on.
        fn set_episode_marking(&mut self, enabled: bool) -> Result<(), CommandError>;

        /// Open the current work's provider-link editor; no legacy identity merge.
        fn show_mappings(&mut self, open: bool) -> Result<(), CommandError>;
        fn select_mapping_target(&mut self, id: String) -> Result<(), CommandError>;
        /// Uses the revisions captured when this target was displayed.
        fn save_provider_mapping(&mut self, provider: MappingProvider, kind: MappingKind, relation: MappingRelation, reference: String) -> Result<(), CommandError>;
        fn remove_provider_mapping(&mut self, row: String) -> Result<(), CommandError>;

        /// Sync the open show's marks with the accounts now.
        fn sync_marks(&mut self) -> Result<(), CommandError>;

        /// Update dandanplay availability and refresh the preferred catalogue.
        /// Public data remains visible when either account signs out.
        fn account_changed(&mut self, signed_in: bool) -> Result<(), CommandError>;

        fn bangumi_account_changed(&mut self, signed_in: bool) -> Result<(), CommandError>;
        /// Save the New Shows preference and refresh it immediately.
        #[errors("accounts_required")]
        fn set_season_source(&mut self, source: SeasonSource) -> Result<(), CommandError>;
        fn refresh_details(&mut self) -> Result<(), CommandError>;
        fn refresh_comments(&mut self) -> Result<(), CommandError>;

        /// The shell played or matched a video, or the account sync wrote
        /// marks: what is being watched, and what is kept of the shows, may
        /// have changed.
        fn history_changed(&mut self) -> Result<(), CommandError>;

        /// Move "now" along today's timeline, and on to the next broadcast
        /// day when it comes; the module's clock calls it.
        fn tick(&mut self) -> Result<(), CommandError>;
    }
}
