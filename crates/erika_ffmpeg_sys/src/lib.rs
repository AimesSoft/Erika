pub const FFMPEG_VERSION: &str = "7.1.1";
pub const LIBASS_VERSION: &str = "0.17.3";
pub const HARFBUZZ_VERSION: &str = "10.4.0";
pub const FREETYPE_VERSION: &str = "2.13.3";

#[allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    unnecessary_transmutes
)]
mod bindings {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

pub use bindings::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeDependencyProfile {
    Lgpl,
    GplFull,
}

impl NativeDependencyProfile {
    pub fn ffmpeg_configure_flags(self) -> &'static [&'static str] {
        match self {
            Self::Lgpl => &[
                "--disable-gpl",
                "--enable-version3",
                "--enable-static",
                "--disable-shared",
                "--disable-programs",
                "--disable-doc",
                "--disable-network",
                "--disable-autodetect",
                "--enable-zlib",
                "--enable-protocol=file",
                "--enable-demuxer=mov,matroska,mpegts,mp3,aac,flac,wav,ogg,ass,srt,webvtt",
                "--enable-parser=hevc,h264,aac,opus,vorbis,flac,mpegaudio",
                "--enable-decoder=hevc,h264,aac,opus,vorbis,flac,mp3,pcm_s16le,pcm_s24le,pcm_s32le,ass,srt,webvtt",
                "--enable-videotoolbox",
            ],
            Self::GplFull => &[
                "--enable-gpl",
                "--enable-version3",
                "--enable-static",
                "--disable-shared",
                "--disable-programs",
                "--disable-doc",
                "--disable-network",
                "--disable-autodetect",
                "--enable-zlib",
                "--enable-protocol=file",
                "--enable-demuxer=mov,matroska,mpegts,mp3,aac,flac,wav,ogg,ass,srt,webvtt",
                "--enable-parser=hevc,h264,aac,opus,vorbis,flac,mpegaudio",
                "--enable-decoder=hevc,h264,aac,opus,vorbis,flac,mp3,pcm_s16le,pcm_s24le,pcm_s32le,ass,srt,webvtt",
                "--enable-videotoolbox",
            ],
        }
    }
}
