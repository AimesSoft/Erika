#[cfg(target_os = "android")]
pub mod android;
pub mod apple;
pub mod audio;
pub mod core;
pub mod danmaku;
pub mod ffmpeg;
pub mod overlay;
pub mod playback;
pub mod presenter;
pub mod renderer;
pub mod source;
pub mod subtitle;
pub mod text;
#[cfg(target_os = "windows")]
pub mod windows;

mod trace;

pub(crate) const NIPAPLAY_FALLBACK_FONT: &[u8] = include_bytes!("../assets/subfont.ttf");

pub use core::*;
