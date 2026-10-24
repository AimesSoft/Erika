//! The protocol of Erika's shell: the look, and the player that sits above
//! every page. The interface, the command line and the MCP server all go
//! through these commands. Pages come from the modules.

#[bern::protocol(namespace = "erika", version = 1)]
pub mod erika {
    use bern::media::MediaSource;
    use bern::prelude::*;

    /// The four accent colors of the design.
    #[value]
    pub enum Accent {
        #[label(tr! { "zh-Hans" => "绯红", "en" => "Crimson" })]
        Crimson,
        #[label(tr! { "zh-Hans" => "群青", "en" => "Ultramarine" })]
        Ultramarine,
        #[label(tr! { "zh-Hans" => "荧绿", "en" => "Neon" })]
        Neon,
        /// The icon's hat, from the satin ribbon of Erika's headdress.
        #[label(tr! { "zh-Hans" => "缎带粉", "en" => "Ribbon Pink" })]
        Ribbon,
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
        #[label(tr! { "zh-Hans" => "账号", "en" => "Account" })]
        Account,
        #[label(tr! { "zh-Hans" => "外观", "en" => "Appearance" })]
        Appearance,
        #[label(tr! { "zh-Hans" => "播放", "en" => "Playback" })]
        Player,
        #[label(tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" })]
        Danmaku,
        #[label(tr! { "zh-Hans" => "媒体库", "en" => "Library" })]
        Library,
        #[label(tr! { "zh-Hans" => "新番", "en" => "New Shows" })]
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
        pub hardware_decode: bool,
        pub show_danmaku: bool,
        pub auto_match: bool,
        pub sync_storage: bool,
        pub startup: StartupPage,
        pub playback_end: PlaybackEnd,
        pub language: InterfaceLanguage,
        pub update_channel: UpdateChannel,
        pub speed: PlaybackSpeed,
        pub danmaku_area: DanmakuArea,
    }
    #[value]
    pub struct FeatureSelection {
        pub account: bool,
        pub danmaku: bool,
        pub enhance: bool,
        pub library: bool,
        /// The New Shows feature; runtime `modules.anime` also serves library details.
        pub anime: bool,
        pub sync: bool,
        pub sources: bool,
        pub remote: bool,
        pub downloads: bool,
    }
    /// The individual setting shown in search, localized by the interface.
    #[value]
    pub enum SettingItem {
        #[label(tr! { "zh-Hans" => "弹弹play 账号登录", "en" => "Dandanplay Account" })]
        Account,
        #[label(tr! { "zh-Hans" => "启动时打开", "en" => "Open at Startup" })]
        Startup,
        #[label(tr! { "zh-Hans" => "播放结束后", "en" => "When Playback Ends" })]
        PlaybackEnd,
        #[label(tr! { "zh-Hans" => "界面语言", "en" => "Interface Language" })]
        Language,
        #[label(tr! { "zh-Hans" => "自动检查更新", "en" => "Automatically Check for Updates" })]
        AutoUpdates,
        #[label(tr! { "zh-Hans" => "更新通道", "en" => "Update Channel" })]
        UpdateChannel,
        #[label(tr! { "zh-Hans" => "主题色", "en" => "Accent Color" })]
        Accent,
        #[label(tr! { "zh-Hans" => "明暗", "en" => "Appearance" })]
        Appearance,
        #[label(tr! { "zh-Hans" => "减少动态效果", "en" => "Reduce Motion" })]
        ReduceMotion,
        #[label(tr! { "zh-Hans" => "默认倍速", "en" => "Default Speed" })]
        Speed,
        #[label(tr! { "zh-Hans" => "片头／片尾跳过提示", "en" => "Opening / Ending Skip Hint" })]
        SkipIntro,
        #[label(tr! { "zh-Hans" => "记住播放位置", "en" => "Remember Playback Position" })]
        RememberPosition,
        #[label(tr! { "zh-Hans" => "自动连播下一集", "en" => "Automatically Play Next Episode" })]
        Autoplay,
        #[label(tr! { "zh-Hans" => "画框圆角", "en" => "Frame Corners" })]
        FrameCorners,
        #[label(tr! { "zh-Hans" => "硬件解码", "en" => "Hardware Decoding" })]
        HardwareDecode,
        #[label(tr! { "zh-Hans" => "显示弹幕", "en" => "Show Danmaku" })]
        ShowDanmaku,
        #[label(tr! { "zh-Hans" => "弹幕不透明度", "en" => "Danmaku Opacity" })]
        DanmakuOpacity,
        #[label(tr! { "zh-Hans" => "弹幕字号", "en" => "Danmaku Font Size" })]
        DanmakuFontSize,
        #[label(tr! { "zh-Hans" => "弹幕显示区域", "en" => "Danmaku Display Area" })]
        DanmakuArea,
        #[label(tr! { "zh-Hans" => "弹弹play 自动匹配", "en" => "Automatically Match with Dandanplay" })]
        AutoMatch,
        #[label(tr! { "zh-Hans" => "挂载本地弹幕", "en" => "Mount Local Danmaku" })]
        MountDanmaku,
        #[label(tr! { "zh-Hans" => "同步与存储", "en" => "Sync and Storage" })]
        SyncStorage,
        #[label(tr! { "zh-Hans" => "新番数据源", "en" => "New Shows Source" })]
        SeasonSource,
        #[label(tr! { "zh-Hans" => "可选功能预设", "en" => "Optional Feature Presets" })]
        Presets,
        #[label(tr! { "zh-Hans" => "启用账号", "en" => "Enable Account" })]
        EnableAccount,
        #[label(tr! { "zh-Hans" => "启用弹幕", "en" => "Enable Danmaku" })]
        EnableDanmaku,
        #[label(tr! { "zh-Hans" => "启用画质增强", "en" => "Enable Upscaling" })]
        EnableEnhance,
        #[label(tr! { "zh-Hans" => "启用媒体库", "en" => "Enable Library" })]
        EnableLibrary,
        #[label(tr! { "zh-Hans" => "启用新番", "en" => "Enable New Shows" })]
        EnableAnime,
        #[label(tr! { "zh-Hans" => "启用账号同步", "en" => "Enable Account Sync" })]
        EnableSync,
        #[label(tr! { "zh-Hans" => "启用网络媒体源", "en" => "Enable Network Sources" })]
        EnableSources,
        #[label(tr! { "zh-Hans" => "启用局域网与远程", "en" => "Enable LAN and Remote" })]
        EnableRemote,
        #[label(tr! { "zh-Hans" => "启用下载", "en" => "Enable Downloads" })]
        EnableDownloads,
        #[label(tr! { "zh-Hans" => "插件市场与已安装插件", "en" => "Plugin Marketplace and Installed Plugins" })]
        Plugins,
        #[label(tr! { "zh-Hans" => "快捷键", "en" => "Keyboard Shortcuts" })]
        Keys,
        #[label(tr! { "zh-Hans" => "检查版本更新", "en" => "Check for Updates" })]
        CheckUpdates,
        #[label(tr! { "zh-Hans" => "开源许可", "en" => "Open Source Licenses" })]
        Licenses,
        #[label(tr! { "zh-Hans" => "致谢", "en" => "Credits" })]
        Credits,
    }

    /// A concrete setting and the section containing it.
    #[value]
    pub struct SettingResult {
        pub id: String,
        pub page: SettingsPage,
        pub item: SettingItem,
    }
    /// What a plugin declares it may do.
    #[value]
    pub enum PluginPermission {
        #[label(tr! { "zh-Hans" => "网络", "en" => "Network" })]
        Network,
        #[label(tr! { "zh-Hans" => "修改弹幕", "en" => "Modify danmaku" })]
        ModifyDanmaku,
        #[label(tr! { "zh-Hans" => "需要弹幕模块", "en" => "Needs danmaku" })]
        NeedsDanmaku,
        #[label(tr! { "zh-Hans" => "着色器", "en" => "Shaders" })]
        Shaders,
        #[label(tr! { "zh-Hans" => "媒体导出", "en" => "Media export" })]
        MediaExport,
        #[label(tr! { "zh-Hans" => "写入文件", "en" => "Write files" })]
        WriteFiles,
        #[label(tr! { "zh-Hans" => "启动程序", "en" => "Launch programs" })]
        LaunchPrograms,
        #[label(tr! { "zh-Hans" => "外观", "en" => "Appearance" })]
        Appearance,
        #[label(tr! { "zh-Hans" => "WebView 叠加", "en" => "WebView overlay" })]
        WebViewOverlay,
    }
    #[value]
    pub struct PluginTag {
        pub id: String,
        pub permission: PluginPermission,
    }

    /// A plugin as its market lists it, in the words of its author.
    #[value]
    pub struct Plugin {
        pub id: String,
        #[content]
        pub name: String,
        #[content]
        pub author: String,
        #[content]
        pub description: String,
        /// The permissions it declares.
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
    /// The optional modules, by their names.
    #[value]
    pub enum FeatureModule {
        #[label(tr! { "zh-Hans" => "账号", "en" => "Account" })]
        Account,
        #[label(tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" })]
        Danmaku,
        #[label(tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" })]
        Enhance,
        #[label(tr! { "zh-Hans" => "媒体库", "en" => "Library" })]
        Library,
        #[label(tr! { "zh-Hans" => "新番", "en" => "New Shows" })]
        Anime,
        #[label(tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" })]
        Sync,
        #[label(tr! { "zh-Hans" => "网络媒体源", "en" => "Network Sources" })]
        Sources,
        #[label(tr! { "zh-Hans" => "局域网与远程", "en" => "LAN and Remote" })]
        Remote,
        #[label(tr! { "zh-Hans" => "下载", "en" => "Downloads" })]
        Downloads,
    }
    #[value]
    pub struct OnboardingModule {
        pub id: String,
        pub module: FeatureModule,
        pub enabled: bool,
    }
    #[value]
    pub struct Onboarding {
        pub visible: bool,
        pub step: i64,
        pub preview_revision: i64,
        /// The preset the selection is exactly, or `Custom`.
        pub preset: OnboardingPreset,
        pub features: FeatureSelection,
        pub modules: Vec<OnboardingModule>,
        pub count: i64,
    }

    /// What the toast says. The interface's own notices come from its
    /// buttons; the others carry what they name.
    #[value]
    pub enum Notice {
        #[label(tr! { "zh-Hans" => "弹幕已发送", "en" => "Danmaku sent" })]
        DanmakuSent,
        #[label(tr! { "zh-Hans" => "操作失败：{reason}", "en" => "Failed: {reason}" })]
        OperationFailed {
            #[content]
            reason: String,
        },
        #[label(tr! { "zh-Hans" => "界面预览：弹幕匹配尚未接入", "en" => "Preview: danmaku matching isn't connected yet" })]
        DanmakuMatchPending,
        #[label(tr! { "zh-Hans" => "界面预览：可使用窗口绿色按钮进入全屏", "en" => "Preview: use the window's green button for full screen" })]
        FullScreenHint,
        #[label(tr! { "zh-Hans" => "界面预览：示例剧集没有画面可截", "en" => "Preview: sample episodes have no picture to save" })]
        ScreenshotPending,
        #[label(tr! { "zh-Hans" => "已登录弹弹play：{name}", "en" => "Signed in to dandanplay as {name}" })]
        SignedIn {
            #[content]
            name: String,
        },
        #[label(tr! { "zh-Hans" => "已退出弹弹play账号", "en" => "Signed out of dandanplay" })]
        SignedOut,
        #[label(tr! { "zh-Hans" => "这一集还没有本地文件，打开它的视频后就能从这里播放", "en" => "No file of this episode yet; open its video once and it plays from here" })]
        NoEpisodeFile,
        #[label(tr! { "zh-Hans" => "弹弹play 认出了：{title}", "en" => "dandanplay recognized it: {title}" })]
        Matched {
            #[content]
            title: String,
        },
        #[label(tr! { "zh-Hans" => "已保存截图：图片 › Erika › {file}", "en" => "Screenshot saved: Pictures › Erika › {file}" })]
        ScreenshotSaved {
            #[content]
            file: String,
        },
        #[label(tr! { "zh-Hans" => "已挂载弹幕：{file}，{count} 条", "en" => "Danmaku mounted: {file}, {count}" })]
        DanmakuMounted {
            #[content]
            file: String,
            count: i64,
        },
        #[label(tr! { "zh-Hans" => "无法读取弹幕：{file}", "en" => "Can't read danmaku: {file}" })]
        DanmakuUnreadable {
            #[content]
            file: String,
        },
        #[label(tr! { "zh-Hans" => "先打开一个视频，再挂载弹幕", "en" => "Open a video before mounting danmaku" })]
        DanmakuNeedsVideo,
        #[label(tr! { "zh-Hans" => "当前为界面预览版本", "en" => "This is a preview version" })]
        PreviewVersion,
        #[label(tr! { "zh-Hans" => "Erika · Rust / Bern / SwiftUI", "en" => "Erika · Rust / Bern / SwiftUI" })]
        Licenses,
        #[label(tr! { "zh-Hans" => "感谢所有开源贡献者与社区支持者", "en" => "Thanks to every open-source contributor and supporter" })]
        Credits,
        #[label(tr! { "zh-Hans" => "已同时开启：{module}", "en" => "Also turned on: {module}" })]
        AlsoOn { module: FeatureModule },
        #[label(tr! { "zh-Hans" => "已同时开启：{first}、{second}", "en" => "Also turned on: {first} and {second}" })]
        AlsoOnBoth {
            first: FeatureModule,
            second: FeatureModule,
        },
        #[label(tr! { "zh-Hans" => "已同时关闭：{module}", "en" => "Also turned off: {module}" })]
        AlsoOff { module: FeatureModule },
        #[label(tr! { "zh-Hans" => "已同时关闭：{first}、{second}", "en" => "Also turned off: {first} and {second}" })]
        AlsoOffBoth {
            first: FeatureModule,
            second: FeatureModule,
        },
        #[label(tr! { "zh-Hans" => "已添加 {name}", "en" => "Added {name}" })]
        PluginAdded {
            #[content]
            name: String,
        },
        #[label(tr! { "zh-Hans" => "{name} · 插件设置", "en" => "{name} · Plugin settings" })]
        PluginSettings {
            #[content]
            name: String,
        },
        #[label(tr! { "zh-Hans" => "已添加字幕：{file}", "en" => "Subtitles added: {file}" })]
        SubtitleChosen {
            #[content]
            file: String,
        },
        #[label(tr! { "zh-Hans" => "无法保存设置：{reason}", "en" => "Couldn't save settings: {reason}" })]
        SaveFailed {
            #[content]
            reason: String,
        },
        #[label(tr! { "zh-Hans" => "已开始下载截图：{file}", "en" => "Screenshot download started: {file}" })]
        ScreenshotDownload {
            #[content]
            file: String,
        },
        #[label(tr! { "zh-Hans" => "{keys} 已用于「{action}」，没有更改", "en" => "{keys} already does “{action}”; nothing changed" })]
        ShortcutTaken {
            #[content]
            keys: String,
            action: KeyAction,
        },
        #[label(tr! { "zh-Hans" => "快捷键已全部恢复默认", "en" => "Every shortcut is back to its default" })]
        ShortcutsReset,
    }

    /// The line under the title of what plays.
    #[value]
    pub enum PlayingNote {
        #[label(tr! { "zh-Hans" => "第 {episode} 集", "en" => "Episode {episode}" })]
        Episode { episode: i64 },
        #[label(tr! { "zh-Hans" => "本地文件", "en" => "Local file" })]
        LocalFile,
        #[label(tr! { "zh-Hans" => "已下载 · 本地文件", "en" => "Downloaded · Local file" })]
        Downloaded,
        #[label(tr! { "zh-Hans" => "下载中 · 边下边播", "en" => "Downloading · Streaming" })]
        Downloading,
        #[label(tr! { "zh-Hans" => "网络链接", "en" => "Link" })]
        Link,
    }

    /// A video played before (design 10.2, 「最近播放」): its file name,
    /// how far it got, what is left in seconds, its picture, and the media
    /// itself when it was a file or link the kernel played.
    #[value]
    pub struct RecentPlay {
        #[content]
        pub name: String,
        pub progress: f64,
        pub remaining: f64,
        pub still: MediaSource,
        pub source: Option<MediaSource>,
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

    /// A subtitle or audio track of the media, as the file names it.
    #[value]
    pub struct MediaTrack {
        pub id: i64,
        #[content]
        pub name: String,
    }

    /// liberika's luma upscalers (ArtCNN).
    #[value]
    pub enum Enhancement {
        #[label(tr! { "zh-Hans" => "关闭", "en" => "Off" })]
        Off,
        #[label(tr! { "zh-Hans" => "ArtCNN 2×", "en" => "ArtCNN 2×" })]
        ArtCnn,
        #[label(tr! { "zh-Hans" => "ArtCNN 2× 高质量", "en" => "ArtCNN 2× high quality" })]
        ArtCnnQuality,
        #[label(tr! { "zh-Hans" => "ArtCNN 2× 降噪", "en" => "ArtCNN 2× denoise" })]
        ArtCnnDenoise,
    }

    /// How liberika draws danmaku: sliders from 0 to 1 (opacity, size,
    /// speed), the delay in seconds, and what it hides.
    #[value]
    pub struct DanmakuSettings {
        pub opacity: f64,
        pub opacity_percent: i64,
        pub supersampling: bool,
        pub font_size: f64,
        /// Display values in the same units as the playback kernel, rounded
        /// to one decimal place rather than exposing normalized slider values.
        #[content]
        pub font_points: String,
        pub speed: f64,
        #[content]
        pub scroll_seconds: String,
        pub delay: f64,
        /// Absolute seconds; the UI localizes the advance/delay direction.
        #[content]
        pub delay_seconds: String,
        pub merge: bool,
        pub block_top: bool,
        pub block_bottom: bool,
        pub block_scroll: bool,
    }

    #[value]
    pub enum SkipKind {
        None,
        Opening,
        Ending,
    }

    #[value]
    pub struct MediaChapter {
        pub id: i64,
        #[content]
        pub title: String,
        /// Seconds on the video's timeline; zero end means not yet known.
        pub start: f64,
        pub end: f64,
        pub at: f64,
        /// Space before the next chapter label on the desktop's 1074 pt rail.
        pub label_width: f64,
        pub kind: SkipKind,
    }

    /// A matching search result with enough context to identify the right show.
    #[value]
    pub struct MatchingShow {
        pub id: i64,
        #[content]
        pub name: String,
        pub poster: MediaSource,
        #[content]
        pub summary: String,
        #[content]
        pub kind: String,
        pub episode_count: i64,
        pub loading: bool,
    }

    #[value]
    pub struct PlayerMatching {
        pub open: bool,
        pub loading: bool,
        #[content]
        pub error: String,
        pub selected: Option<MatchingShow>,
        pub shows: Vec<MatchingShow>,
        pub episodes: Vec<MediaTrack>,
    }

    #[value]
    pub struct PlayerState {
        #[content]
        pub danmaku_text: String,
        pub sending_danmaku: bool,
        pub danmaku_sent_revision: i64,
        pub queue_index: i64,
        pub episode_list: Vec<MediaTrack>,
        pub has_previous: bool,
        pub has_next: bool,
        pub panel: bool,
        pub tab: PlayerTab,
        pub episode: i64,
        pub episodes: bool,
        /// The media's tracks and the selected one's id; no subtitle track
        /// is selected when subtitles are off (`subtitle_track` < 0).
        pub subtitle_tracks: Vec<MediaTrack>,
        pub subtitle_track: i64,
        pub audio_tracks: Vec<MediaTrack>,
        pub audio_track: i64,
        /// The selected tracks' names, and the video's codec and height.
        #[content]
        pub subtitle_name: String,
        #[content]
        pub audio_name: String,
        #[content]
        pub video_info: String,
        pub enhancement: Enhancement,
        pub volume: f64,
        pub volume_percent: i64,
        pub subtitle_size: f64,
        pub subtitle_scale_percent: i64,
        /// Render text subtitles at twice the resolution when the budget allows.
        pub subtitle_supersampling: bool,
        pub skip_intro: bool,
        pub chapters: Vec<MediaChapter>,
        pub has_chapters: bool,
        pub current_chapter: i64,
        pub skip_kind: SkipKind,
        pub feedback: bool,
        /// The subtitle file the user attached last.
        #[content]
        pub subtitle_file: String,
        /// The danmaku files liberika shows, and how many comments they hold.
        #[content]
        pub danmaku_source: String,
        pub danmaku_count: i64,
    }

    /// An action with a keyboard shortcut, as 「设置 › 快捷键」 lists it.
    /// The last four are the menu bar's own, which only the Mac shows.
    #[value]
    pub enum KeyAction {
        #[label(tr! { "zh-Hans" => "播放 / 暂停", "en" => "Play / Pause" })]
        PlayPause,
        #[label(tr! { "zh-Hans" => "快退 10 秒", "en" => "Back 10 Seconds" })]
        SeekBack,
        #[label(tr! { "zh-Hans" => "快进 10 秒", "en" => "Forward 10 Seconds" })]
        SeekForward,
        #[label(tr! { "zh-Hans" => "全屏", "en" => "Full Screen" })]
        Fullscreen,
        #[label(tr! { "zh-Hans" => "开关弹幕", "en" => "Show or Hide Danmaku" })]
        ToggleDanmaku,
        #[label(tr! { "zh-Hans" => "截图", "en" => "Screenshot" })]
        Screenshot,
        #[label(tr! { "zh-Hans" => "返回 / 关闭", "en" => "Back / Close" })]
        Back,
        #[label(tr! { "zh-Hans" => "确认", "en" => "Confirm" })]
        Confirm,
        #[label(tr! { "zh-Hans" => "打开文件", "en" => "Open File" })]
        OpenFile,
        #[label(tr! { "zh-Hans" => "搜索", "en" => "Search" })]
        Search,
        #[label(tr! { "zh-Hans" => "设置", "en" => "Settings" })]
        Settings,
        #[label(tr! { "zh-Hans" => "缩小海报", "en" => "Smaller Posters" })]
        SmallerPosters,
        #[label(tr! { "zh-Hans" => "放大海报", "en" => "Larger Posters" })]
        LargerPosters,
        #[label(tr! { "zh-Hans" => "打开文件夹", "en" => "Open Folder" })]
        OpenFolder,
        #[label(tr! { "zh-Hans" => "打开链接", "en" => "Open Link" })]
        OpenLink,
        #[label(tr! { "zh-Hans" => "上一集", "en" => "Previous Episode" })]
        PreviousEpisode,
        #[label(tr! { "zh-Hans" => "下一集", "en" => "Next Episode" })]
        NextEpisode,
    }

    /// Every action's keyboard shortcut, as Bern writes shortcuts
    /// (`cmd+o`, `space`); empty for none.
    #[value]
    pub struct KeyBindings {
        pub play_pause: String,
        pub seek_back: String,
        pub seek_forward: String,
        pub fullscreen: String,
        pub toggle_danmaku: String,
        pub screenshot: String,
        pub back: String,
        pub confirm: String,
        pub open_file: String,
        pub search: String,
        pub settings: String,
        pub smaller_posters: String,
        pub larger_posters: String,
        pub open_folder: String,
        pub open_link: String,
        pub previous_episode: String,
        pub next_episode: String,
    }

    #[state]
    pub struct State {
        pub onboarding: Onboarding,
        pub preferences: Preferences,
        pub features: FeatureSelection,
        pub module_count: i64,
        pub feature_preview_revision: i64,
        /// The preset the enabled modules are exactly, or `Custom`.
        pub preset: OnboardingPreset,
        pub notice: Option<Notice>,
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
        pub matching: PlayerMatching,
        pub danmaku_settings: DanmakuSettings,
        pub playing: bool,
        /// Whether the player fills the window; otherwise it has shrunk into
        /// the mini player at the foot of the sidebar.
        pub expanded: bool,
        pub paused: bool,
        /// What plays: its title, the episode or "local file", and the
        /// picture the frame shows until the kernel draws the video.
        #[content]
        pub title: String,
        pub subtitle: PlayingNote,
        pub still: MediaSource,
        /// liberika plays the media: the frame shows its video (the
        /// `video` player) over the still. Otherwise the player simulates
        /// time over the still, as for the library's sample episodes.
        pub video: bool,
        /// How far it has played, from 0 to 1, and as seconds.
        pub progress: f64,
        pub elapsed: f64,
        pub duration: f64,
        /// What played before, the latest first, at most three: the empty
        /// frame lists them under it (design 10.2).
        pub recents: Vec<RecentPlay>,
        /// Whether anything played before, which 「继续上次」 needs.
        pub has_recents: bool,
        /// Whether 「打开链接…」 asks for a link, and whether the link given
        /// last was not one it can play.
        pub link_prompt: bool,
        pub link_invalid: bool,
        /// The keys that open a file, as this computer writes them: ⌘O on
        /// a Mac and Ctrl+O elsewhere by default, or those the user chose
        /// in 「设置 › 快捷键」.
        #[content]
        pub open_key: String,
        /// The account at the foot of the sidebar: its name, or 「本地用户」
        /// until one is signed in.
        #[content]
        pub user_name: String,
        /// The signed-in provider selected for the outer account identity.
        pub user_account: ProfileAccount,
        pub user_avatar: Option<MediaSource>,
        pub account_status: Account,
        pub bangumi: BangumiAccount,
        /// Every action's keyboard shortcut, and the one it has by
        /// default; whether any is not its default.
        pub keys: KeyBindings,
        pub default_keys: KeyBindings,
        pub keys_changed: bool,
    }

    #[value]
    pub enum ProfileAccount {
        #[label(tr! { "zh-Hans" => "未登录", "en" => "Not signed in" })]
        Local,
        #[label(tr! { "zh-Hans" => "弹弹play 已登录", "en" => "Signed in to dandanplay" })]
        Dandanplay,
        #[label(tr! { "zh-Hans" => "Bangumi 已登录", "en" => "Signed in to Bangumi" })]
        Bangumi,
    }

    /// The dandanplay account, which the season's anime, the details of a
    /// show and matching danmaku need.
    #[value]
    pub struct Account {
        pub signed_in: bool,
        pub prompt: bool,
        pub settings_window: bool,
        /// The name others see, and the one signed in with.
        #[content]
        pub name: String,
        #[content]
        pub user: String,
        /// The QQ avatar when signed in with a numeric QQ email address.
        pub avatar: Option<MediaSource>,
        /// Signing in is under way.
        pub busy: bool,
        /// Why signing in failed last; empty when it did not.
        #[content]
        pub error: String,
    }

    /// Website session. Passwords and cookies never enter observable state.
    #[value]
    pub struct BangumiAccount {
        pub signed_in: bool,
        #[content]
        pub name: String,
        /// Stable website ID, used internally to associate the session.
        #[content]
        pub user: String,
        /// The email used to sign in, shown in the shared account profile.
        #[content]
        pub email: String,
        pub avatar: Option<MediaSource>,
        pub busy: bool,
        #[content]
        pub error: String,
        pub prompt: bool,
        pub settings_window: bool,
        pub expired: bool,
        pub captcha: Option<MediaSource>,
    }

    #[commands]
    pub trait Commands {
        /// Browser executor metadata and observations; business state stays in the host.
        fn browser_file(
            &mut self,
            client: String,
            url: String,
            name: String,
        ) -> Result<(), CommandError>;
        fn browser_exchange(
            &mut self,
            client: String,
            packet: String,
        ) -> Result<String, CommandError>;
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
        fn set_danmaku_area(&mut self, value: DanmakuArea) -> Result<(), CommandError>;
        fn set_preference(&mut self, key: String, value: bool) -> Result<(), CommandError>;
        fn set_module(&mut self, id: String, enabled: bool) -> Result<(), CommandError>;
        fn apply_preset(&mut self, preset: FeaturePreset) -> Result<(), CommandError>;
        fn set_plugin_category(&mut self, category: PluginCategory) -> Result<(), CommandError>;
        fn set_plugin_tab(&mut self, tab: PluginTab) -> Result<(), CommandError>;
        fn use_plugin(&mut self, id: String) -> Result<(), CommandError>;
        fn show_notice(&mut self, notice: Notice) -> Result<(), CommandError>;
        /// Play a file the user opened, picked or dropped.
        fn open_file(&mut self, source: MediaSource) -> Result<(), CommandError>;

        /// Play the videos of a folder the user picked, the first first.
        fn open_folder(&mut self, source: MediaSource) -> Result<(), CommandError>;

        /// Ask for a link to play, or stop asking.
        fn show_link_prompt(&mut self, visible: bool) -> Result<(), CommandError>;

        /// Play an http(s) or magnet link; any other keeps asking.
        fn open_link(&mut self, link: String) -> Result<(), CommandError>;

        /// Play one of the recent videos again, from where it stopped.
        #[errors("no_such_recent")]
        fn play_recent(&mut self, name: String) -> Result<(), CommandError>;

        /// Play the latest recent video on (「继续上次」).
        #[errors("no_such_recent")]
        fn continue_last(&mut self) -> Result<(), CommandError>;

        /// The way back with no sidebar to shrink into (design 10.2): the
        /// picture goes back into the empty frame and the video heads the
        /// recent ones.
        fn return_to_frame(&mut self) -> Result<(), CommandError>;

        /// Play what a module asks for, such as the next episode.
        #[cfg_attr(feature = "bern-module-anime", on(anime::play))]
        fn play(
            &mut self,
            title: String,
            episode: i64,
            still: MediaSource,
            progress: f64,
            source: MediaSource,
        ) -> Result<(), CommandError>;

        /// Play a download: its file once done, a stream of it while it
        /// downloads.
        #[cfg(feature = "bern-module-downloads")]
        #[on(downloads::play)]
        fn play_download(
            &mut self,
            title: String,
            source: MediaSource,
            downloading: bool,
        ) -> Result<(), CommandError>;

        /// A download finished: its folder joins the library and is
        /// scanned.
        #[cfg(feature = "bern-module-downloads")]
        #[on(downloads::finished)]
        fn download_finished(&mut self, folder: MediaSource) -> Result<(), CommandError>;

        /// Open a library poster using the optional anime details page.
        #[cfg(feature = "bern-module-library")]
        #[on(library::open_show)]
        fn open_library_show(&mut self, id: i64) -> Result<(), CommandError>;

        /// The library matched or removed videos: the anime pages show the
        /// files anew.
        #[cfg(feature = "bern-module-library")]
        #[on(library::files_changed)]
        fn library_files_changed(&mut self) -> Result<(), CommandError>;

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
        #[errors("no_such_chapter")]
        fn seek_chapter(&mut self, id: i64) -> Result<(), CommandError>;
        /// Skip only the recognized OP/ED containing the current position.
        fn skip_segment(&mut self) -> Result<(), CommandError>;
        fn select_episode(&mut self, episode: i64) -> Result<(), CommandError>;
        fn change_episode(&mut self, delta: i64) -> Result<(), CommandError>;
        fn player_panel(&mut self, visible: bool) -> Result<(), CommandError>;
        fn player_tab(&mut self, tab: PlayerTab) -> Result<(), CommandError>;
        /// Show subtitle track `id`, or none when it is negative.
        #[errors("no_such_track")]
        fn select_subtitle_track(&mut self, id: i64) -> Result<(), CommandError>;
        #[errors("no_such_track")]
        fn select_audio_track(&mut self, id: i64) -> Result<(), CommandError>;
        fn set_enhancement(&mut self, value: Enhancement) -> Result<(), CommandError>;
        fn set_subtitle_supersampling(&mut self, enabled: bool) -> Result<(), CommandError>;
        fn set_player_number(&mut self, key: String, value: f64) -> Result<(), CommandError>;
        fn set_skip_intro(&mut self, value: bool) -> Result<(), CommandError>;
        /// Save the picture on screen, with its subtitles, at the video's
        /// size in Pictures › Erika.
        fn screenshot(&mut self) -> Result<(), CommandError>;
        fn show_matching(&mut self, open: bool) -> Result<(), CommandError>;
        fn search_matching(&mut self, query: String) -> Result<(), CommandError>;
        fn choose_match_show(&mut self, id: i64) -> Result<(), CommandError>;
        fn back_to_match_results(&mut self) -> Result<(), CommandError>;
        fn confirm_match_episode(&mut self, id: i64) -> Result<(), CommandError>;
        fn send_danmaku(&mut self, text: String) -> Result<(), CommandError>;
        /// Set a danmaku slider: `opacity`, `font_size`, `speed` (0 to 1)
        /// or `delay` (seconds).
        #[errors("unknown_danmaku_option")]
        fn set_danmaku_number(&mut self, key: String, value: f64) -> Result<(), CommandError>;
        /// Set `supersampling`, `merge`, `block_top`, `block_bottom` or `block_scroll`.
        #[errors("unknown_danmaku_option")]
        fn set_danmaku_switch(&mut self, key: String, value: bool) -> Result<(), CommandError>;
        /// Show the comments of a Bilibili xml or Dandanplay json file over
        /// what plays.
        fn mount_danmaku(&mut self, source: MediaSource) -> Result<(), CommandError>;
        fn attach_subtitle(&mut self, source: MediaSource) -> Result<(), CommandError>;

        fn set_accent(&mut self, accent: Accent) -> Result<(), CommandError>;

        /// Sign in to dandanplay; the account shows when the service
        /// answers.
        #[cfg(feature = "bern-module-account")]
        fn sign_in(&mut self, user: String, password: String) -> Result<(), CommandError>;
        #[cfg(feature = "bern-module-account")]
        fn sign_out(&mut self) -> Result<(), CommandError>;
        #[cfg(feature = "bern-module-account")]
        fn open_account_login(&mut self, settings_window: bool) -> Result<(), CommandError>;
        #[cfg(feature = "bern-module-account")]
        fn close_account_login(&mut self) -> Result<(), CommandError>;
        #[cfg(feature = "bern-module-account")]
        fn cancel_account_login(&mut self) -> Result<(), CommandError>;
        #[cfg(feature = "bern-module-account")]
        fn submit_account_login(&mut self, bangumi: bool, user: String, password: String, captcha: String) -> Result<(), CommandError>;


        #[cfg(feature = "bern-module-account")]
        fn open_bangumi_login(&mut self, settings_window: bool) -> Result<(), CommandError>;
        #[cfg(feature = "bern-module-account")]
        fn refresh_bangumi_captcha(&mut self) -> Result<(), CommandError>;
        #[cfg(feature = "bern-module-account")]
        fn close_bangumi_login(&mut self) -> Result<(), CommandError>;
        #[cfg(feature = "bern-module-account")]
        fn bangumi_sign_in(&mut self, email: String, password: String, captcha: String) -> Result<(), CommandError>;
        #[cfg(feature = "bern-module-account")]
        fn bangumi_sign_out(&mut self) -> Result<(), CommandError>;
        #[cfg(feature = "bern-module-account")]
        fn check_bangumi_session(&mut self) -> Result<(), CommandError>;
        #[cfg(feature = "bern-module-account")]
        fn dismiss_bangumi_expired(&mut self) -> Result<(), CommandError>;

        fn set_appearance(&mut self, appearance: Appearance) -> Result<(), CommandError>;

        /// Give `action` the shortcut `keys`, as Bern writes them
        /// (`cmd+o`, `space`); empty for none. Keys another action has stay
        /// that action's: a notice names it.
        #[errors("invalid_shortcut")]
        fn set_shortcut(&mut self, action: KeyAction, keys: String) -> Result<(), CommandError>;
        /// Give `action` its default shortcut again, unless another action
        /// has those keys now.
        fn reset_shortcut(&mut self, action: KeyAction) -> Result<(), CommandError>;
        /// Every shortcut as it is by default.
        fn reset_shortcuts(&mut self) -> Result<(), CommandError>;
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

impl Default for erika::Account {
    fn default() -> Self {
        Self {
            signed_in: false,
            prompt: false,
            settings_window: false,
            name: String::new(),
            user: String::new(),
            avatar: None,
            busy: false,
            error: String::new(),
        }
    }
}

impl Default for erika::BangumiAccount {
    fn default() -> Self {
        Self {
            signed_in: false,
            name: String::new(),
            user: String::new(),
            email: String::new(),
            avatar: None,
            busy: false,
            error: String::new(),
            prompt: false,
            settings_window: false,
            expired: false,
            captcha: None,
        }
    }
}
