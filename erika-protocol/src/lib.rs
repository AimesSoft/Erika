//! The protocol of Erika's shell: the look, and the player that sits above
//! every page. The interface, the command line and the MCP server all go
//! through these commands. Pages come from the modules.

#[bern::protocol(namespace = "erika", version = 1)]
pub mod erika {
    use bern::media::MediaSource;
    use bern::prelude::*;

    /// The three accent colors of the design.
    #[value]
    pub enum Accent {
        #[label(tr! { "zh-Hans" => "绯红", "en" => "Crimson" })]
        Crimson,
        #[label(tr! { "zh-Hans" => "群青", "en" => "Ultramarine" })]
        Ultramarine,
        #[label(tr! { "zh-Hans" => "荧绿", "en" => "Neon" })]
        Neon,
    }

    #[value]
    pub enum Appearance {
        #[label(tr! { "zh-Hans" => "跟随系统", "en" => "System" })]
        System,
        #[label(tr! { "zh-Hans" => "浅色", "en" => "Light" })]
        Light,
        #[label(tr! { "zh-Hans" => "深色", "en" => "Dark" })]
        Dark,
    }

    #[value]
    pub enum SettingsPage {
        #[label(tr! { "zh-Hans" => "通用", "en" => "General" })]
        General,
        #[label(tr! { "zh-Hans" => "外观", "en" => "Appearance" })]
        Appearance,
        #[label(tr! { "zh-Hans" => "播放", "en" => "Playback" })]
        Player,
        #[label(tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" })]
        Danmaku,
        #[label(tr! { "zh-Hans" => "媒体库", "en" => "Library" })]
        Library,
        #[label(tr! { "zh-Hans" => "追番", "en" => "Anime" })]
        Anime,
        #[label(tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" })]
        Sync,
        #[label(tr! { "zh-Hans" => "网络媒体源", "en" => "Network Sources" })]
        Sources,
        #[label(tr! { "zh-Hans" => "局域网与远程", "en" => "LAN and Remote" })]
        Remote,
        #[label(tr! { "zh-Hans" => "下载", "en" => "Downloads" })]
        Downloads,
        #[label(tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" })]
        Enhance,
        #[label(tr! { "zh-Hans" => "可选功能", "en" => "Optional Features" })]
        Optional,
        #[label(tr! { "zh-Hans" => "插件", "en" => "Plugins" })]
        Plugins,
        #[label(tr! { "zh-Hans" => "快捷键", "en" => "Keyboard Shortcuts" })]
        Keys,
        #[label(tr! { "zh-Hans" => "关于", "en" => "About" })]
        About,
    }

    #[value]
    pub enum StartupPage {
        #[label(tr! { "zh-Hans" => "首页", "en" => "Home" })]
        Home,
        #[label(tr! { "zh-Hans" => "媒体库", "en" => "Library" })]
        Library,
        #[label(tr! { "zh-Hans" => "播放器", "en" => "Player" })]
        Player,
    }

    #[value]
    pub enum PlaybackEnd {
        #[label(tr! { "zh-Hans" => "播放下一集", "en" => "Play next episode" })]
        Next,
        #[label(tr! { "zh-Hans" => "停止播放", "en" => "Stop playback" })]
        Stop,
    }

    #[value]
    pub enum InterfaceLanguage {
        #[label(tr! { "zh-Hans" => "跟随系统", "en" => "System" })]
        System,
        #[label(tr! { "zh-Hans" => "简体中文", "en" => "Simplified Chinese" })]
        Chinese,
        #[label(tr! { "zh-Hans" => "English", "en" => "English" })]
        English,
    }

    #[value]
    pub enum UpdateChannel {
        #[label(tr! { "zh-Hans" => "稳定版", "en" => "Stable" })]
        Stable,
        #[label(tr! { "zh-Hans" => "预览版", "en" => "Preview" })]
        Preview,
    }

    #[value]
    pub enum PlaybackSpeed {
        #[label(tr! { "zh-Hans" => "1.0×", "en" => "1.0×" })]
        Normal,
        #[label(tr! { "zh-Hans" => "0.5×", "en" => "0.5×" })]
        Half,
        #[label(tr! { "zh-Hans" => "0.75×", "en" => "0.75×" })]
        Slow,
        #[label(tr! { "zh-Hans" => "1.25×", "en" => "1.25×" })]
        Fast,
        #[label(tr! { "zh-Hans" => "1.5×", "en" => "1.5×" })]
        Faster,
        #[label(tr! { "zh-Hans" => "2.0×", "en" => "2.0×" })]
        Double,
    }

    #[value]
    pub enum PlaybackKernel {
        #[label(tr! { "zh-Hans" => "liberika（推荐）", "en" => "liberika (recommended)" })]
        Liberika,
    }

    #[value]
    pub enum DanmakuArea {
        #[label(tr! { "zh-Hans" => "半屏", "en" => "Half" })]
        Half,
        #[label(tr! { "zh-Hans" => "1/4", "en" => "1/4" })]
        Quarter,
        #[label(tr! { "zh-Hans" => "全屏", "en" => "Full" })]
        Full,
    }

    #[value]
    pub enum FeaturePreset {
        #[label(tr! { "zh-Hans" => "纯粹播放", "en" => "Pure playback" })]
        Pure,
        #[label(tr! { "zh-Hans" => "看番", "en" => "Anime" })]
        Anime,
        #[label(tr! { "zh-Hans" => "全都要", "en" => "Everything" })]
        All,
    }

    #[value]
    pub enum PluginCategory {
        #[label(tr! { "zh-Hans" => "全部", "en" => "All" })]
        All,
        #[label(tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" })]
        Danmaku,
        #[label(tr! { "zh-Hans" => "画面", "en" => "Picture" })]
        Picture,
        #[label(tr! { "zh-Hans" => "工具", "en" => "Tools" })]
        Tools,
        #[label(tr! { "zh-Hans" => "外观", "en" => "Appearance" })]
        Appearance,
    }

    #[value]
    pub enum PluginTab {
        #[label(tr! { "zh-Hans" => "市场", "en" => "Market" })]
        Market,
        #[label(tr! { "zh-Hans" => "已安装", "en" => "Installed" })]
        Installed,
    }
    #[value]
    pub struct Preferences {
        pub auto_updates: bool,
        pub reduce_motion: bool,
        pub remember_position: bool,
        pub autoplay: bool,
        pub step_back: bool,
        pub hardware_decode: bool,
        pub show_danmaku: bool,
        pub auto_match: bool,
        pub sync_storage: bool,
        pub startup: StartupPage,
        pub playback_end: PlaybackEnd,
        pub language: InterfaceLanguage,
        pub update_channel: UpdateChannel,
        pub speed: PlaybackSpeed,
        pub kernel: PlaybackKernel,
        pub danmaku_area: DanmakuArea,
    }
    #[value]
    pub struct FeatureSelection {
        pub danmaku: bool,
        pub enhance: bool,
        pub library: bool,
        pub anime: bool,
        pub sync: bool,
        pub sources: bool,
        pub remote: bool,
        pub downloads: bool,
    }
    #[value]
    pub struct SettingResult {
        pub id: String,
        pub title: String,
        pub page: SettingsPage,
    }
    #[value]
    pub struct PluginTag {
        pub id: String,
        pub label: String,
    }

    #[value]
    pub struct Plugin {
        pub id: String,
        pub name: String,
        pub author: String,
        pub description: String,
        pub permissions: String,
        pub tags: Vec<PluginTag>,
        pub category: PluginCategory,
        pub installed: bool,
    }

    #[value]
    pub enum OnboardingPreset {
        #[label(tr! { "zh-Hans" => "纯粹播放", "en" => "Pure playback" })]
        Pure,
        #[label(tr! { "zh-Hans" => "看番", "en" => "Anime" })]
        Anime,
        #[label(tr! { "zh-Hans" => "全都要", "en" => "Everything" })]
        All,
        #[label(tr! { "zh-Hans" => "自己挑", "en" => "Custom" })]
        Custom,
    }
    #[value]
    pub struct OnboardingModule {
        pub id: String,
        pub name: String,
        pub enabled: bool,
    }
    #[value]
    pub struct Onboarding {
        pub visible: bool,
        pub step: i64,
        pub preview_revision: i64,
        pub preset: OnboardingPreset,
        pub features: FeatureSelection,
        pub modules: Vec<OnboardingModule>,
        pub count: i64,
        pub summary: String,
    }

    #[value]
    pub enum PlayerTab {
        #[label(tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" })]
        Danmaku,
        #[label(tr! { "zh-Hans" => "字幕", "en" => "Subtitles" })]
        Subtitles,
        #[label(tr! { "zh-Hans" => "画面", "en" => "Picture" })]
        Picture,
        #[label(tr! { "zh-Hans" => "音频", "en" => "Audio" })]
        Audio,
        #[label(tr! { "zh-Hans" => "播放", "en" => "Playback" })]
        Playback,
    }

    #[value]
    pub enum SubtitleTrack {
        #[label(tr! { "zh-Hans" => "简中", "en" => "Chinese" })]
        Chinese,
        #[label(tr! { "zh-Hans" => "日本語", "en" => "Japanese" })]
        Japanese,
        #[label(tr! { "zh-Hans" => "关闭", "en" => "Off" })]
        Off,
    }

    #[value]
    pub enum AudioTrack {
        #[label(tr! { "zh-Hans" => "日本語 · FLAC 2.0", "en" => "Japanese · FLAC 2.0" })]
        Japanese,
        #[label(tr! { "zh-Hans" => "中文配音 · AAC 2.0", "en" => "Chinese · AAC 2.0" })]
        Chinese,
    }

    #[value]
    pub enum Enhancement {
        #[label(tr! { "zh-Hans" => "关闭", "en" => "Off" })]
        Off,
        #[label(tr! { "zh-Hans" => "Anime4K", "en" => "Anime4K" })]
        Anime4K,
        #[label(tr! { "zh-Hans" => "ArtCNN 2×", "en" => "ArtCNN 2×" })]
        ArtCNN,
    }

    #[value]
    pub enum PictureRatio {
        #[label(tr! { "zh-Hans" => "原始", "en" => "Original" })]
        Original,
        #[label(tr! { "zh-Hans" => "16:9", "en" => "16:9" })]
        Wide,
        #[label(tr! { "zh-Hans" => "4:3", "en" => "4:3" })]
        Classic,
    }

    #[value]
    pub enum HdrMode {
        #[label(tr! { "zh-Hans" => "自动", "en" => "Auto" })]
        Auto,
        #[label(tr! { "zh-Hans" => "关闭", "en" => "Off" })]
        Off,
    }

    #[value]
    pub struct PlayerState {
        pub panel: bool,
        pub tab: PlayerTab,
        pub episode: i64,
        pub episodes: bool,
        pub subtitle_track: SubtitleTrack,
        pub subtitle_label: String,
        pub audio_track: AudioTrack,
        pub enhancement: Enhancement,
        pub ratio: PictureRatio,
        pub hdr: HdrMode,
        pub volume: f64,
        pub brightness: f64,
        pub subtitle_size: f64,
        pub subtitle_delay: f64,
        pub audio_delay: f64,
        pub skip_intro: bool,
        pub feedback: bool,
        pub subtitle_line: String,
        pub sent_danmaku: String,
        pub subtitle_file: String,
    }

    #[state]
    pub struct State {
        pub onboarding: Onboarding,
        pub preferences: Preferences,
        pub features: FeatureSelection,
        pub module_count: i64,
        pub feature_preview_revision: i64,
        pub preset_name: String,
        pub notice: String,
        pub notice_revision: i64,
        pub plugins: Vec<Plugin>,
        pub plugin_category: PluginCategory,
        pub plugin_tab: PluginTab,
        pub installed_count: i64,
        pub accent: Accent,
        pub appearance: Appearance,
        /// Whether something is playing. The player layer or the mini
        /// player shows it.
        pub player: PlayerState,
        pub playing: bool,
        /// Whether the player fills the window; otherwise it has shrunk into
        /// the mini player at the foot of the sidebar.
        pub expanded: bool,
        pub paused: bool,
        /// What plays: its title, the episode or "local file", and the
        /// picture the frame shows until the kernel draws the video.
        pub title: String,
        pub subtitle: String,
        pub still: MediaSource,
        /// How far it has played, from 0 to 1, and as times.
        pub progress: f64,
        pub elapsed: String,
        pub duration: String,
        /// The account at the foot of the sidebar.
        pub user_name: String,
    }

    #[commands]
    pub trait Commands {
        fn open_onboarding(&mut self) -> Result<(), CommandError>;
        fn onboarding_next(&mut self) -> Result<(), CommandError>;
        fn onboarding_back(&mut self) -> Result<(), CommandError>;
        fn onboarding_skip(&mut self) -> Result<(), CommandError>;
        fn onboarding_preset(&mut self, preset: OnboardingPreset) -> Result<(), CommandError>;
        fn onboarding_toggle(&mut self, id: String) -> Result<(), CommandError>;

        fn set_startup(&mut self, value: StartupPage) -> Result<(), CommandError>;
        fn set_playback_end(&mut self, value: PlaybackEnd) -> Result<(), CommandError>;
        fn set_language(&mut self, value: InterfaceLanguage) -> Result<(), CommandError>;
        fn set_update_channel(&mut self, value: UpdateChannel) -> Result<(), CommandError>;
        fn set_speed(&mut self, value: PlaybackSpeed) -> Result<(), CommandError>;
        fn set_kernel(&mut self, value: PlaybackKernel) -> Result<(), CommandError>;
        fn set_danmaku_area(&mut self, value: DanmakuArea) -> Result<(), CommandError>;
        fn set_preference(&mut self, key: String, value: bool) -> Result<(), CommandError>;
        fn set_module(&mut self, id: String, enabled: bool) -> Result<(), CommandError>;
        fn apply_preset(&mut self, preset: FeaturePreset) -> Result<(), CommandError>;
        fn set_plugin_category(&mut self, category: PluginCategory) -> Result<(), CommandError>;
        fn set_plugin_tab(&mut self, tab: PluginTab) -> Result<(), CommandError>;
        fn use_plugin(&mut self, id: String) -> Result<(), CommandError>;
        fn show_notice(&mut self, message: String) -> Result<(), CommandError>;
        /// Play a file the user opened, picked or dropped.
        fn open_file(&mut self, source: MediaSource) -> Result<(), CommandError>;

        /// Play what a module asks for, such as the next episode.
        #[on(anime::play)]
        fn play(
            &mut self,
            title: String,
            subtitle: String,
            still: MediaSource,
            progress: f64,
        ) -> Result<(), CommandError>;

        #[on(downloads::play)]
        fn play_download(&mut self, title: String, subtitle: String, still: MediaSource, progress: f64) -> Result<(), CommandError>;

        /// Open a library poster using the optional anime details page.
        #[on(library::open_show)]
        fn open_library_show(&mut self, id: i64) -> Result<(), CommandError>;

        /// Shrink the player into the mini player and keep playing.
        fn minimize(&mut self) -> Result<(), CommandError>;

        /// Grow the mini player back into the player.
        fn expand(&mut self) -> Result<(), CommandError>;

        /// Stop playing.
        fn close_player(&mut self) -> Result<(), CommandError>;

        /// Pause, or play on.
        fn toggle_pause(&mut self) -> Result<(), CommandError>;
        fn player_tick(&mut self) -> Result<(), CommandError>;
        fn seek(&mut self, progress: f64) -> Result<(), CommandError>;
        fn skip(&mut self, seconds: f64) -> Result<(), CommandError>;
        fn select_episode(&mut self, episode: i64) -> Result<(), CommandError>;
        fn change_episode(&mut self, delta: i64) -> Result<(), CommandError>;
        fn player_panel(&mut self, visible: bool) -> Result<(), CommandError>;
        fn player_tab(&mut self, tab: PlayerTab) -> Result<(), CommandError>;
        fn set_subtitle_track(&mut self, value: SubtitleTrack) -> Result<(), CommandError>;
        fn set_audio_track(&mut self, value: AudioTrack) -> Result<(), CommandError>;
        fn set_enhancement(&mut self, value: Enhancement) -> Result<(), CommandError>;
        fn set_picture_ratio(&mut self, value: PictureRatio) -> Result<(), CommandError>;
        fn set_hdr(&mut self, value: HdrMode) -> Result<(), CommandError>;
        fn set_player_number(&mut self, key: String, value: f64) -> Result<(), CommandError>;
        fn set_skip_intro(&mut self, value: bool) -> Result<(), CommandError>;
        fn send_danmaku(&mut self, text: String) -> Result<(), CommandError>;
        fn attach_subtitle(&mut self, source: MediaSource) -> Result<(), CommandError>;


        fn set_accent(&mut self, accent: Accent) -> Result<(), CommandError>;

        fn set_appearance(&mut self, appearance: Appearance) -> Result<(), CommandError>;
    }
    #[value]
    pub struct SearchActions {
        pub theme: bool,
        pub danmaku: bool,
        pub skip_intro: bool,
        pub optional: bool,
        pub plugins: bool,
        pub keys: bool,
        pub open_file: bool,
        pub schedule: bool,
        pub dark: bool,
        pub has_settings: bool,
        pub has_operations: bool,
        pub empty: bool,
    }

    #[queries]
    pub trait Queries {
        #[reads(features)]
        fn search_actions(&self, text: String, answer: Answer<SearchActions>);
        #[reads(features)]
        fn find_settings(&self, text: String, answer: Answer<Vec<SettingResult>>);
    }
}
