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

    #[state]
    pub struct State {
        pub accent: Accent,
        pub appearance: Appearance,
        /// Whether something is playing. The player layer or the mini
        /// player shows it.
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

        /// Shrink the player into the mini player and keep playing.
        fn minimize(&mut self) -> Result<(), CommandError>;

        /// Grow the mini player back into the player.
        fn expand(&mut self) -> Result<(), CommandError>;

        /// Stop playing.
        fn close_player(&mut self) -> Result<(), CommandError>;

        /// Pause, or play on.
        fn toggle_pause(&mut self) -> Result<(), CommandError>;

        fn set_accent(&mut self, accent: Accent) -> Result<(), CommandError>;

        fn set_appearance(&mut self, appearance: Appearance) -> Result<(), CommandError>;
    }
}
