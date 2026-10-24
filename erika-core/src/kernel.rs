//! The playback kernel: liberika on macOS and its WebCodecs/Canvas adapter
//! in browsers. Other targets and tests keep the simulated preview clock.

#[cfg(target_os = "macos")]
mod liberika;

mod web;

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
    fn is_browser(&self) -> bool {
        false
    }
    fn file_name(&self, _url: &str) -> Option<String> {
        None
    }
    fn prepare_source(&self, _url: &str) -> Result<(), String> {
        Ok(())
    }
    fn browser_file(&self, _client: &str, _url: &str, _name: &str) -> Result<(), String> {
        Err("This engine does not host browser playback".into())
    }
    fn browser_exchange(&self, _client: &str, _packet: &str) -> Result<String, String> {
        Err("This engine does not host browser playback".into())
    }
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
    /// Applied when opening the next media source.
    fn set_hardware_decode(&self, _enabled: bool) {}
    fn select_audio(&self, id: i64);
    /// `None` turns subtitles off.
    fn select_subtitle(&self, id: Option<i64>);
    fn add_subtitle(&self, url: &str);
    /// 1 is the subtitles' own size.
    fn set_subtitle_scale(&self, scale: f64);
    fn set_subtitle_supersampling(&self, enabled: bool);
    fn set_enhancement(&self, enhancement: Enhancement);
    /// How the comments over the video look and which show.
    fn set_danmaku(&self, config: &DanmakuConfig);
    /// Show the comments of a Bilibili xml or Dandanplay json file over
    /// what plays, as one more track; answers how many it holds.
    fn add_danmaku(&self, name: &str, contents: &str) -> Result<usize, String>;
    /// Show `text` now as the user's own comment.
    fn send_danmaku(&self, text: &str);
    /// Browser file I/O completes asynchronously on the player worker.
    fn mount_danmaku_file(&self, _url: &str, _name: &str) -> bool {
        false
    }
    fn take_danmaku_result(&self) -> Option<(String, Result<usize, String>)> {
        None
    }
    /// Browser captures are blob URLs, rather than filesystem paths.
    fn captured_source(&self) -> Option<String> {
        None
    }
    /// Save the picture on screen, with its subtitles, `width` pixels wide
    /// (the video's own width when `None`) as a PNG at `path`;
    /// [`Playback::still`] names it once it is written.
    fn capture(&self, path: &Path, width: Option<u32>);
    /// Stop playing and let go of the media.
    fn close(&self);
    fn playback(&self) -> Playback;
    fn take_results(&self) -> Vec<OperationResult> {
        Vec::new()
    }
}

/// What the kernel last saw of the player.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Playback {
    /// Identifies the loaded media, so a queued report cannot restore old chapters.
    pub source: Option<String>,
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
    pub chapters: Vec<Chapter>,
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

/// How liberika draws danmaku, in its own terms.
#[derive(Clone, Debug, PartialEq)]
pub struct DanmakuConfig {
    pub enabled: bool,
    pub supersampling: bool,
    /// 0 to 1.
    pub opacity: f64,
    /// Points; liberika's own default is 30.
    pub font_size: f64,
    /// The share of the picture's height comments use, from the top.
    pub area: f64,
    /// How long a scrolling comment takes to cross.
    pub scroll_seconds: f64,
    /// Seconds later than their own time; negative is earlier.
    pub delay: f64,
    pub merge: bool,
    pub block_top: bool,
    pub block_bottom: bool,
    pub block_scroll: bool,
}

/// A track of the media, named as the file names it.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    pub title: String,
    pub start: f64,
    pub end: Option<f64>,
}

/// Start the platform's kernel and make it Bern's media kernel, so the
/// frontend's video views draw through it. `None` where there is none.
/// HTTP headers a link plugin asked to fetch an address with, by address:
/// the kernel opens the address with them.
static LINK_HEADERS: std::sync::Mutex<Vec<(String, Vec<(String, String)>)>> =
    std::sync::Mutex::new(Vec::new());

pub fn set_link_headers(url: &str, headers: Vec<(String, String)>) {
    let mut all = LINK_HEADERS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    all.retain(|(known, _)| known != url);
    if !headers.is_empty() {
        all.push((url.to_string(), headers));
        // A few links' worth, the latest kept.
        let excess = all.len().saturating_sub(16);
        all.drain(..excess);
    }
}

pub fn link_headers(url: &str) -> Vec<(String, String)> {
    LINK_HEADERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .find(|(known, _)| known == url)
        .map(|(_, headers)| headers.clone())
        .unwrap_or_default()
}

pub fn start() -> Option<Arc<dyn Kernel>> {
    #[cfg(not(target_arch = "wasm32"))]
    if bern::resources::Resources::host().is_served() {
        return Some(web::Web::install());
    }
    #[cfg(target_os = "macos")]
    {
        Some(liberika::Liberika::install())
    }
    #[cfg(target_arch = "wasm32")]
    {
        Some(web::Web::install())
    }
    #[cfg(not(any(target_os = "macos", target_arch = "wasm32")))]
    {
        None
    }
}

#[derive(Debug)]
pub enum Operation {
    Screenshot(PathBuf),
    Subtitle(String),
    Setting,
}
#[derive(Debug)]
pub struct OperationResult {
    pub source: Option<String>,
    pub operation: Operation,
    pub result: Result<(), String>,
}
