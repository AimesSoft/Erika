use crate::core::{ColorPrimaries, TransferFunction};
use crate::ffmpeg::{D3d11vaTexture, Frame, PlanarFrame, Result as FfmpegResult};
use crate::renderer::pipeline::{ColorRange, HdrMetadata, MatrixCoefficients};

#[cfg(target_os = "android")]
use std::sync::Arc;

#[cfg(target_os = "android")]
use crate::android::mediacodec::AndroidHardwareBufferImage;

/// Renderer-facing metadata copied out of a decoded frame before the decoder
/// is allowed to retire. Hardware payloads can therefore cross threads without
/// retaining decoder-owned FFmpeg buffer callbacks merely to query color or
/// geometry information.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoFrameDescriptor {
    pub width: u32,
    pub height: u32,
    pub raw_pixel_format: i32,
    pub pixel_format: Option<String>,
    pub line_sizes: [i32; 4],
    pub primaries: ColorPrimaries,
    pub transfer: TransferFunction,
    pub range: ColorRange,
    pub matrix: MatrixCoefficients,
    pub hdr_metadata: Option<HdrMetadata>,
}

impl VideoFrameDescriptor {
    fn from_frame(frame: &Frame) -> Self {
        Self {
            width: frame.width(),
            height: frame.height(),
            raw_pixel_format: frame.raw_pixel_format(),
            pixel_format: frame.pixel_format(),
            line_sizes: frame.line_sizes(),
            primaries: frame.color_primaries(),
            transfer: frame.transfer_function(),
            range: frame.color_range(),
            matrix: frame.matrix_coefficients(),
            hdr_metadata: frame.hdr_metadata(),
        }
    }
}

#[cfg(target_os = "android")]
#[derive(Clone)]
pub struct PreparedAndroidHardwareBufferFrame {
    descriptor: VideoFrameDescriptor,
    image: Arc<AndroidHardwareBufferImage>,
}

/// A decoded video payload ready for a renderer. Android MediaCodec Surface
/// frames are converted to an independently owned AHardwareBuffer payload on
/// the playback worker; the original AVFrame is dropped there while its
/// decoder and MediaCodec callback context are still alive.
pub enum VideoFramePayload {
    Decoded(Frame),
    #[cfg(target_os = "android")]
    AndroidHardwareBuffer(PreparedAndroidHardwareBufferFrame),
}

impl VideoFramePayload {
    pub fn from_decoded(frame: Frame) -> FfmpegResult<Self> {
        #[cfg(target_os = "android")]
        if frame.is_mediacodec() {
            let descriptor = VideoFrameDescriptor::from_frame(&frame);
            let image = frame.prepared_mediacodec_image()?;
            return Ok(Self::AndroidHardwareBuffer(
                PreparedAndroidHardwareBufferFrame { descriptor, image },
            ));
        }

        Ok(Self::Decoded(frame))
    }

    pub fn try_clone_ref(&self) -> FfmpegResult<Self> {
        match self {
            Self::Decoded(frame) => frame.try_clone_ref().map(Self::Decoded),
            #[cfg(target_os = "android")]
            Self::AndroidHardwareBuffer(frame) => Ok(Self::AndroidHardwareBuffer(frame.clone())),
        }
    }

    pub fn decoded_frame(&self) -> Option<&Frame> {
        match self {
            Self::Decoded(frame) => Some(frame),
            #[cfg(target_os = "android")]
            Self::AndroidHardwareBuffer(_) => None,
        }
    }

    pub fn width(&self) -> u32 {
        match self {
            Self::Decoded(frame) => frame.width(),
            #[cfg(target_os = "android")]
            Self::AndroidHardwareBuffer(frame) => frame.descriptor.width,
        }
    }

    pub fn height(&self) -> u32 {
        match self {
            Self::Decoded(frame) => frame.height(),
            #[cfg(target_os = "android")]
            Self::AndroidHardwareBuffer(frame) => frame.descriptor.height,
        }
    }

    pub fn pixel_format(&self) -> Option<String> {
        match self {
            Self::Decoded(frame) => frame.pixel_format(),
            #[cfg(target_os = "android")]
            Self::AndroidHardwareBuffer(frame) => frame.descriptor.pixel_format.clone(),
        }
    }

    pub fn raw_pixel_format(&self) -> i32 {
        match self {
            Self::Decoded(frame) => frame.raw_pixel_format(),
            #[cfg(target_os = "android")]
            Self::AndroidHardwareBuffer(frame) => frame.descriptor.raw_pixel_format,
        }
    }

    pub fn line_sizes(&self) -> [i32; 4] {
        match self {
            Self::Decoded(frame) => frame.line_sizes(),
            #[cfg(target_os = "android")]
            Self::AndroidHardwareBuffer(frame) => frame.descriptor.line_sizes,
        }
    }

    pub fn color_primaries(&self) -> ColorPrimaries {
        match self {
            Self::Decoded(frame) => frame.color_primaries(),
            #[cfg(target_os = "android")]
            Self::AndroidHardwareBuffer(frame) => frame.descriptor.primaries,
        }
    }

    pub fn transfer_function(&self) -> TransferFunction {
        match self {
            Self::Decoded(frame) => frame.transfer_function(),
            #[cfg(target_os = "android")]
            Self::AndroidHardwareBuffer(frame) => frame.descriptor.transfer,
        }
    }

    pub fn color_range(&self) -> ColorRange {
        match self {
            Self::Decoded(frame) => frame.color_range(),
            #[cfg(target_os = "android")]
            Self::AndroidHardwareBuffer(frame) => frame.descriptor.range,
        }
    }

    pub fn matrix_coefficients(&self) -> MatrixCoefficients {
        match self {
            Self::Decoded(frame) => frame.matrix_coefficients(),
            #[cfg(target_os = "android")]
            Self::AndroidHardwareBuffer(frame) => frame.descriptor.matrix,
        }
    }

    pub fn hdr_metadata(&self) -> Option<HdrMetadata> {
        match self {
            Self::Decoded(frame) => frame.hdr_metadata(),
            #[cfg(target_os = "android")]
            Self::AndroidHardwareBuffer(frame) => frame.descriptor.hdr_metadata,
        }
    }

    pub fn has_hw_frames_context(&self) -> bool {
        match self {
            Self::Decoded(frame) => frame.has_hw_frames_context(),
            #[cfg(target_os = "android")]
            Self::AndroidHardwareBuffer(_) => true,
        }
    }

    pub fn to_planar_frame(&self) -> Option<PlanarFrame> {
        self.decoded_frame()?.to_planar_frame()
    }

    pub fn d3d11va_texture(&self) -> Option<D3d11vaTexture<'_>> {
        self.decoded_frame()?.d3d11va_texture()
    }

    pub fn is_videotoolbox(&self) -> bool {
        self.decoded_frame().is_some_and(Frame::is_videotoolbox)
    }

    pub fn is_d3d11va(&self) -> bool {
        self.decoded_frame().is_some_and(Frame::is_d3d11va)
    }

    #[cfg(target_os = "android")]
    pub fn is_mediacodec(&self) -> bool {
        matches!(self, Self::AndroidHardwareBuffer(_))
            || self
                .decoded_frame()
                .is_some_and(|frame| frame.is_mediacodec())
    }

    #[cfg(target_os = "android")]
    pub(crate) fn prepared_mediacodec_image(
        &self,
    ) -> FfmpegResult<Arc<AndroidHardwareBufferImage>> {
        match self {
            Self::AndroidHardwareBuffer(frame) => Ok(Arc::clone(&frame.image)),
            Self::Decoded(frame) => frame.prepared_mediacodec_image(),
        }
    }
}

impl std::fmt::Debug for VideoFramePayload {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VideoFramePayload")
            .field("width", &self.width())
            .field("height", &self.height())
            .field("pixel_format", &self.pixel_format())
            .finish_non_exhaustive()
    }
}
