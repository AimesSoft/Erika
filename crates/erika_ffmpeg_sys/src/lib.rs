pub const FFMPEG_VERSION: &str = "8.1.2";
pub const LIBASS_VERSION: &str = "0.17.5";
pub const HARFBUZZ_VERSION: &str = "14.2.1";
pub const FREETYPE_VERSION: &str = "2.14.3";

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

pub const ERIKA_SWS_BILINEAR: std::os::raw::c_int = SwsFlags_SWS_BILINEAR as std::os::raw::c_int;
pub const ERIKA_PROFILE_UNKNOWN: i32 = AV_PROFILE_UNKNOWN;

#[cfg(test)]
mod tests {
    use super::{ERIKA_PROFILE_UNKNOWN, ERIKA_SWS_BILINEAR};

    #[test]
    fn ffmpeg_812_compatibility_constants() {
        assert_eq!(ERIKA_SWS_BILINEAR, 2);
        assert_eq!(ERIKA_PROFILE_UNKNOWN, -99);
    }
}

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

    pub fn ffmpeg_configure_flags_for_target_os(self, target_os: &str) -> Vec<&'static str> {
        let mut flags = self.ffmpeg_configure_flags().to_vec();
        if target_os == "windows" {
            flags.retain(|flag| *flag != "--enable-videotoolbox");
            flags.extend(["--enable-d3d11va", "--enable-dxva2"]);
        }
        flags
    }
}
