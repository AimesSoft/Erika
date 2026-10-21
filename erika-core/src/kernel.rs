//! The playback kernel. Where Bern hosts liberika (macOS so far), the
//! engine plays real media through it; elsewhere, and in tests, the player
//! keeps simulating time over the design's stills.

#[cfg(target_os = "macos")]
mod liberika;

use erika_protocol::erika::Enhancement;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The application player Erika plays in: `Players::video` of the
/// interface, the name every entry addresses it by.
pub const PLAYER: &str = "video";

/// What the engine asks of the kernel. Calls return at once; the kernel
/// works on its own thread and the engine reads the outcome from
/// [`Kernel::playback`] on its next tick.
pub trait Kernel: Send + Sync {
    /// The frontend hosts the kernel: video views draw through it.
    fn ready(&self) -> bool;
    /// Load `url` (a `file:` or `http(s):` address), seek to `start`
    /// seconds once it is open, and play.
    fn open(&self, url: &str, start: f64);
    fn play(&self);
    fn pause(&self);
    fn seek(&self, seconds: f64);
    fn set_volume(&self, volume: f64);
    fn set_rate(&self, rate: f64);
    fn select_audio(&self, id: i64);
    /// `None` turns subtitles off.
    fn select_subtitle(&self, id: Option<i64>);
    fn add_subtitle(&self, url: &str);
    /// 1 is the subtitles' own size.
    fn set_subtitle_scale(&self, scale: f64);
    fn set_enhancement(&self, enhancement: Enhancement);
    /// Save the picture on screen, with its subtitles, `width` pixels wide
    /// (the video's own width when `None`) as a PNG at `path`;
    /// [`Playback::still`] names it once it is written.
    fn capture(&self, path: &Path, width: Option<u32>);
    /// Stop playing and let go of the media.
    fn close(&self);
    fn playback(&self) -> Playback;
}

/// What the kernel last saw of the player.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Playback {
    /// A source is open and has not failed.
    pub loaded: bool,
    pub playing: bool,
    /// Playback stopped at the end of the media.
    pub ended: bool,
    /// A frame of the video is on screen.
    pub picture: bool,
    /// Seconds.
    pub position: f64,
    pub duration: Option<f64>,
    pub audio_tracks: Vec<Track>,
    pub subtitle_tracks: Vec<Track>,
    pub audio_track: Option<i64>,
    pub subtitle_track: Option<i64>,
    /// Codec and height of the video, such as `HEVC · 1080p`.
    pub video: String,
    /// The picture captured last.
    pub still: Option<PathBuf>,
    pub error: Option<String>,
}

/// A track of the media, named as the file names it.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub id: i64,
    pub name: String,
}

/// Start the platform's kernel and make it Bern's media kernel, so the
/// frontend's video views draw through it. `None` where there is none.
pub fn start() -> Option<Arc<dyn Kernel>> {
    #[cfg(target_os = "macos")]
    {
        Some(liberika::Liberika::install())
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
