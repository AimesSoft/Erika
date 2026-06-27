use std::collections::BTreeSet;
use std::ffi::{CStr, CString, c_int, c_void};
use std::marker::PhantomData;
use std::mem;
use std::path::Path;
use std::ptr;
use std::slice;
use std::time::Duration;

use crate::core::{ColorPrimaries, TrackInfo, TrackKind, TransferFunction, VideoParams};
use crate::renderer::pipeline::{
    Chromaticity, ColorRange, ContentLightMetadata, HdrMetadata, MasteringDisplayMetadata,
    MatrixCoefficients,
};
use crate::source::{ByteRange, MediaSource};
use crate::subtitle::{
    DecodedSubtitleFrame, SubtitleBitmapPlane, SubtitleTextFormat, SubtitleTextSegment,
    SubtitleTrackConfig,
};
use erika_ffmpeg_sys as sys;
use libc::{EAGAIN, EINVAL, EIO, ESPIPE, SEEK_CUR, SEEK_END, SEEK_SET};
use thiserror::Error;

const AVERROR_EOF: i32 = -541_478_725;

#[derive(Debug, Error)]
pub enum FfmpegError {
    #[error("path contains interior nul byte")]
    InteriorNul,
    #[error("ffmpeg error in {operation}: {message} ({code})")]
    Api {
        operation: &'static str,
        code: i32,
        message: String,
    },
    #[error("ffmpeg returned a null pointer from {0}")]
    NullPointer(&'static str),
    #[error("unknown stream index: {0}")]
    UnknownStream(i32),
    #[error("packet stream {packet_stream} does not match decoder stream {decoder_stream}")]
    StreamMismatch {
        decoder_stream: i32,
        packet_stream: i32,
    },
    #[error("expected audio frame")]
    ExpectedAudioFrame,
    #[error("expected subtitle stream")]
    ExpectedSubtitleStream,
    #[error("unsupported D3D11VA software pixel format: {0}")]
    UnsupportedD3d11vaSwFormat(i32),
    #[error(
        "invalid subtitle bitmap: width={width} height={height} stride={stride} colors={colors}"
    )]
    InvalidSubtitleBitmap {
        width: i32,
        height: i32,
        stride: i32,
        colors: i32,
    },
    #[error("source error: {0}")]
    Source(String),
}

pub type Result<T> = std::result::Result<T, FfmpegError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeBase {
    pub num: i32,
    pub den: i32,
}

impl TimeBase {
    pub fn seconds_from_timestamp(self, timestamp: i64) -> f64 {
        timestamp as f64 * self.num as f64 / self.den as f64
    }

    fn to_av_rational(self) -> sys::AVRational {
        sys::AVRational {
            num: self.num,
            den: self.den,
        }
    }

    fn from_av(rational: sys::AVRational) -> Self {
        if rational.den == 0 {
            return Self { num: 0, den: 1 };
        }
        Self {
            num: rational.num,
            den: rational.den,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketTimestamp {
    pub raw: i64,
    pub time_base: TimeBase,
}

impl PacketTimestamp {
    pub fn seconds(self) -> f64 {
        self.time_base.seconds_from_timestamp(self.raw)
    }

    pub fn as_duration(self) -> Option<Duration> {
        let seconds = self.seconds();
        if seconds.is_sign_negative() || !seconds.is_finite() {
            return None;
        }
        Some(Duration::from_secs_f64(seconds))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketFlags {
    bits: i32,
}

impl PacketFlags {
    pub fn bits(self) -> i32 {
        self.bits
    }

    pub fn is_key(self) -> bool {
        self.bits & sys::AV_PKT_FLAG_KEY as i32 != 0
    }

    pub fn is_corrupt(self) -> bool {
        self.bits & sys::AV_PKT_FLAG_CORRUPT as i32 != 0
    }

    pub fn is_discard(self) -> bool {
        self.bits & sys::AV_PKT_FLAG_DISCARD as i32 != 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaProbe {
    pub uri: String,
    pub duration: Option<Duration>,
    pub tracks: Vec<TrackInfo>,
    pub video: Vec<VideoProbe>,
    pub audio: Vec<AudioProbe>,
    pub subtitles: Vec<SubtitleTrackConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoProbe {
    pub track_id: i64,
    pub params: VideoParams,
    pub codec: Option<String>,
    pub pixel_format: Option<String>,
    pub profile: Option<String>,
    pub level: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioProbe {
    pub track_id: i64,
    pub codec: Option<String>,
    pub sample_rate: u32,
    pub channels: u32,
    pub sample_format: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcmSampleFormat {
    F32Interleaved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcmFormat {
    pub sample_rate: u32,
    pub channels: u32,
    pub sample_format: PcmSampleFormat,
}

impl PcmFormat {
    pub fn f32_interleaved(sample_rate: u32, channels: u32) -> Self {
        Self {
            sample_rate,
            channels,
            sample_format: PcmSampleFormat::F32Interleaved,
        }
    }
}

impl Default for PcmFormat {
    fn default() -> Self {
        Self::f32_interleaved(48_000, 2)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PcmAudioFrame {
    pub format: PcmFormat,
    pub pts: Option<Duration>,
    pub frames: usize,
    pub samples: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamSelection {
    All,
    Only(BTreeSet<i32>),
}

impl StreamSelection {
    pub fn all() -> Self {
        Self::All
    }

    pub fn only<I>(stream_indices: I) -> Self
    where
        I: IntoIterator<Item = i32>,
    {
        Self::Only(stream_indices.into_iter().collect())
    }

    fn accepts(&self, stream_index: i32) -> bool {
        match self {
            Self::All => true,
            Self::Only(streams) => streams.contains(&stream_index),
        }
    }
}

pub struct Demuxer {
    context: FormatContext,
    probe: MediaProbe,
    stream_time_bases: Vec<Option<TimeBase>>,
    selection: StreamSelection,
}

unsafe impl Send for Demuxer {}

impl Demuxer {
    pub fn open_path(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_uri(&path.as_ref().to_string_lossy())
    }

    pub fn open_uri(uri: &str) -> Result<Self> {
        let mut context = open_format_context(uri)?;
        find_stream_info(&mut context)?;
        let (probe, stream_time_bases) = inspect_format_context(uri, context.as_ptr());
        Ok(Self {
            context,
            probe,
            stream_time_bases,
            selection: StreamSelection::all(),
        })
    }

    pub fn open_source(source: Box<dyn MediaSource>) -> Result<Self> {
        let uri = source.uri().to_string();
        let mut context = open_source_format_context(source)?;
        find_stream_info(&mut context)?;
        let (probe, stream_time_bases) = inspect_format_context(&uri, context.as_ptr());
        Ok(Self {
            context,
            probe,
            stream_time_bases,
            selection: StreamSelection::all(),
        })
    }

    pub fn probe(&self) -> &MediaProbe {
        &self.probe
    }

    pub fn selection(&self) -> &StreamSelection {
        &self.selection
    }

    pub fn set_stream_selection(&mut self, selection: StreamSelection) -> Result<()> {
        if let StreamSelection::Only(streams) = &selection {
            for stream_index in streams {
                if !self.has_stream(*stream_index) {
                    return Err(FfmpegError::UnknownStream(*stream_index));
                }
            }
        }
        self.selection = selection;
        Ok(())
    }

    pub fn stream_time_base(&self, stream_index: i32) -> Option<TimeBase> {
        if stream_index < 0 {
            return None;
        }
        self.stream_time_bases
            .get(stream_index as usize)
            .copied()
            .flatten()
    }

    pub fn codec_parameters(&self, stream_index: i32) -> Result<CodecParameters<'_>> {
        if stream_index < 0 {
            return Err(FfmpegError::UnknownStream(stream_index));
        }
        let raw = self.context.as_ptr();
        let stream_count = unsafe { (*raw).nb_streams as usize };
        let Some(stream_slot) = (stream_index as usize)
            .checked_sub(0)
            .filter(|index| *index < stream_count)
        else {
            return Err(FfmpegError::UnknownStream(stream_index));
        };
        let stream = unsafe { *(*raw).streams.add(stream_slot) };
        if stream.is_null() {
            return Err(FfmpegError::UnknownStream(stream_index));
        }
        let codecpar = unsafe { (*stream).codecpar };
        if codecpar.is_null() {
            return Err(FfmpegError::NullPointer("AVStream.codecpar"));
        }
        Ok(CodecParameters {
            ptr: codecpar,
            stream_index,
            time_base: TimeBase::from_av(unsafe { (*stream).time_base }),
            _owner: PhantomData,
        })
    }

    pub fn owned_codec_parameters(&self, stream_index: i32) -> Result<OwnedCodecParameters> {
        let parameters = self.codec_parameters(stream_index)?;
        OwnedCodecParameters::copy_from(parameters)
    }

    pub fn open_decoder(&self, stream_index: i32) -> Result<Decoder> {
        Decoder::open(self.codec_parameters(stream_index)?)
    }

    pub fn open_subtitle_decoder(&self, stream_index: i32) -> Result<SubtitleDecoder> {
        SubtitleDecoder::open(self.codec_parameters(stream_index)?)
    }

    pub fn read_packet(&mut self) -> Result<Option<Packet>> {
        loop {
            let mut packet = Packet::alloc()?;
            let code =
                unsafe { sys::av_read_frame(self.context.as_mut_ptr(), packet.as_mut_ptr()) };
            if code == AVERROR_EOF {
                return Ok(None);
            }
            check(code, "av_read_frame")?;

            let stream_index = packet.stream_index();
            packet.time_base = self.stream_time_base(stream_index);
            if self.selection.accepts(stream_index) {
                return Ok(Some(packet));
            }
        }
    }

    pub fn seek(&mut self, position: Duration) -> Result<()> {
        let target = position.as_micros().min(i64::MAX as u128) as i64;
        check(
            unsafe {
                sys::av_seek_frame(
                    self.context.as_mut_ptr(),
                    -1,
                    target,
                    sys::AVSEEK_FLAG_BACKWARD as i32,
                )
            },
            "av_seek_frame",
        )?;
        check(
            unsafe { sys::avformat_flush(self.context.as_mut_ptr()) },
            "avformat_flush",
        )?;
        Ok(())
    }

    fn has_stream(&self, stream_index: i32) -> bool {
        self.stream_time_base(stream_index).is_some()
            || self
                .probe
                .tracks
                .iter()
                .any(|track| track.id == stream_index as i64)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CodecParameters<'a> {
    ptr: *const sys::AVCodecParameters,
    stream_index: i32,
    time_base: TimeBase,
    _owner: PhantomData<&'a Demuxer>,
}

impl CodecParameters<'_> {
    pub fn stream_index(self) -> i32 {
        self.stream_index
    }

    pub fn time_base(self) -> TimeBase {
        self.time_base
    }

    pub fn codec_name(self) -> Option<String> {
        unsafe { codec_name((*self.ptr).codec_id) }
    }

    pub fn kind(self) -> Option<TrackKind> {
        unsafe { track_kind((*self.ptr).codec_type) }
    }
}

pub struct OwnedCodecParameters {
    ptr: *mut sys::AVCodecParameters,
    stream_index: i32,
    time_base: TimeBase,
}

unsafe impl Send for OwnedCodecParameters {}

impl OwnedCodecParameters {
    fn copy_from(parameters: CodecParameters<'_>) -> Result<Self> {
        let ptr = unsafe { sys::avcodec_parameters_alloc() };
        if ptr.is_null() {
            return Err(FfmpegError::NullPointer("avcodec_parameters_alloc"));
        }
        check(
            unsafe { sys::avcodec_parameters_copy(ptr, parameters.ptr) },
            "avcodec_parameters_copy",
        )?;
        Ok(Self {
            ptr,
            stream_index: parameters.stream_index,
            time_base: parameters.time_base,
        })
    }

    pub fn stream_index(&self) -> i32 {
        self.stream_index
    }

    pub fn time_base(&self) -> TimeBase {
        self.time_base
    }

    pub fn codec_name(&self) -> Option<String> {
        unsafe { codec_name((*self.ptr).codec_id) }
    }

    pub fn kind(&self) -> Option<TrackKind> {
        unsafe { track_kind((*self.ptr).codec_type) }
    }
}

impl Drop for OwnedCodecParameters {
    fn drop(&mut self) {
        unsafe { sys::avcodec_parameters_free(&mut self.ptr) };
    }
}

pub struct SubtitleDecoder {
    context: *mut sys::AVCodecContext,
    stream_index: i32,
    time_base: TimeBase,
    track: SubtitleTrackConfig,
}

unsafe impl Send for SubtitleDecoder {}

impl SubtitleDecoder {
    pub fn open(parameters: CodecParameters<'_>) -> Result<Self> {
        Self::open_raw(
            parameters.ptr,
            parameters.stream_index,
            parameters.time_base,
        )
    }

    pub fn open_owned(parameters: &OwnedCodecParameters) -> Result<Self> {
        Self::open_raw(
            parameters.ptr,
            parameters.stream_index,
            parameters.time_base,
        )
    }

    fn open_raw(
        parameters_ptr: *const sys::AVCodecParameters,
        stream_index: i32,
        time_base: TimeBase,
    ) -> Result<Self> {
        if unsafe { track_kind((*parameters_ptr).codec_type) } != Some(TrackKind::Subtitle) {
            return Err(FfmpegError::ExpectedSubtitleStream);
        }

        let codec_id = unsafe { (*parameters_ptr).codec_id };
        let codec = unsafe { sys::avcodec_find_decoder(codec_id) };
        if codec.is_null() {
            return Err(FfmpegError::NullPointer("avcodec_find_decoder(subtitle)"));
        }
        let context = unsafe { sys::avcodec_alloc_context3(codec) };
        if context.is_null() {
            return Err(FfmpegError::NullPointer("avcodec_alloc_context3(subtitle)"));
        }
        let decoder = Self {
            context,
            stream_index,
            time_base,
            track: SubtitleTrackConfig::embedded(i64::from(stream_index), i64::from(stream_index)),
        };
        check(
            unsafe { sys::avcodec_parameters_to_context(decoder.context, parameters_ptr) },
            "avcodec_parameters_to_context(subtitle)",
        )?;
        unsafe {
            (*decoder.context).pkt_timebase = time_base.to_av_rational();
        }
        check(
            unsafe { sys::avcodec_open2(decoder.context, codec, ptr::null_mut()) },
            "avcodec_open2(subtitle)",
        )?;
        Ok(decoder)
    }

    pub fn stream_index(&self) -> i32 {
        self.stream_index
    }

    pub fn time_base(&self) -> TimeBase {
        self.time_base
    }

    pub fn track(&self) -> &SubtitleTrackConfig {
        &self.track
    }

    pub fn decode_packet(&mut self, packet: &Packet) -> Result<Option<DecodedSubtitleFrame>> {
        if packet.stream_index() != self.stream_index {
            return Err(FfmpegError::StreamMismatch {
                decoder_stream: self.stream_index,
                packet_stream: packet.stream_index(),
            });
        }

        let mut subtitle = sys::AVSubtitle::default();
        let mut got_subtitle = 0;
        let code = unsafe {
            sys::avcodec_decode_subtitle2(
                self.context,
                &mut subtitle,
                &mut got_subtitle,
                packet.as_ptr().cast_mut(),
            )
        };
        if code < 0 {
            if got_subtitle != 0 {
                unsafe { sys::avsubtitle_free(&mut subtitle) };
            }
            check(code, "avcodec_decode_subtitle2")?;
        }
        if got_subtitle == 0 {
            return Ok(None);
        }

        let frame = unsafe { import_av_subtitle(i64::from(self.stream_index), packet, &subtitle) };
        unsafe { sys::avsubtitle_free(&mut subtitle) };
        frame.map(Some)
    }

    pub fn flush(&mut self) {
        unsafe { sys::avcodec_flush_buffers(self.context) };
    }
}

impl Drop for SubtitleDecoder {
    fn drop(&mut self) {
        unsafe { sys::avcodec_free_context(&mut self.context) };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderOutput {
    Frame,
    NeedMoreInput,
    EndOfStream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderBackend {
    Software,
    VideoToolbox,
    D3d11va,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecoderConfig {
    pub backend: DecoderBackend,
}

impl DecoderConfig {
    pub fn software() -> Self {
        Self {
            backend: DecoderBackend::Software,
        }
    }

    pub fn videotoolbox() -> Self {
        Self {
            backend: DecoderBackend::VideoToolbox,
        }
    }

    pub fn d3d11va() -> Self {
        Self {
            backend: DecoderBackend::D3d11va,
        }
    }
}

impl Default for DecoderConfig {
    fn default() -> Self {
        Self::software()
    }
}

struct HardwareDecoderState {
    device_ref: *mut sys::AVBufferRef,
    frames_ref: *mut sys::AVBufferRef,
    pixel_format: sys::AVPixelFormat,
}

impl Drop for HardwareDecoderState {
    fn drop(&mut self) {
        unsafe { sys::av_buffer_unref(&mut self.frames_ref) };
        unsafe { sys::av_buffer_unref(&mut self.device_ref) };
    }
}

pub struct Decoder {
    context: *mut sys::AVCodecContext,
    stream_index: i32,
    time_base: TimeBase,
    backend: DecoderBackend,
    hw_state: Option<Box<HardwareDecoderState>>,
}

unsafe impl Send for Decoder {}

impl Decoder {
    pub fn open(parameters: CodecParameters<'_>) -> Result<Self> {
        Self::open_with_config(parameters, DecoderConfig::default())
    }

    pub fn open_owned(parameters: &OwnedCodecParameters) -> Result<Self> {
        Self::open_owned_with_config(parameters, DecoderConfig::default())
    }

    pub fn open_owned_with_config(
        parameters: &OwnedCodecParameters,
        config: DecoderConfig,
    ) -> Result<Self> {
        Self::open_raw(
            parameters.ptr,
            parameters.stream_index,
            parameters.time_base,
            config,
        )
    }

    pub fn open_with_config(
        parameters: CodecParameters<'_>,
        config: DecoderConfig,
    ) -> Result<Self> {
        Self::open_raw(
            parameters.ptr,
            parameters.stream_index,
            parameters.time_base,
            config,
        )
    }

    fn open_raw(
        parameters_ptr: *const sys::AVCodecParameters,
        stream_index: i32,
        time_base: TimeBase,
        config: DecoderConfig,
    ) -> Result<Self> {
        configure_ffmpeg_debug_logging();
        let codec_id = unsafe { (*parameters_ptr).codec_id };
        let codec = unsafe { sys::avcodec_find_decoder(codec_id) };
        if codec.is_null() {
            return Err(FfmpegError::NullPointer("avcodec_find_decoder"));
        }
        let context = unsafe { sys::avcodec_alloc_context3(codec) };
        if context.is_null() {
            return Err(FfmpegError::NullPointer("avcodec_alloc_context3"));
        }
        let decoder = Self {
            context,
            stream_index,
            time_base,
            backend: config.backend,
            hw_state: None,
        };
        check(
            unsafe { sys::avcodec_parameters_to_context(decoder.context, parameters_ptr) },
            "avcodec_parameters_to_context",
        )?;
        let mut decoder = decoder;
        match config.backend {
            DecoderBackend::Software => {}
            DecoderBackend::VideoToolbox => decoder.configure_videotoolbox(codec)?,
            DecoderBackend::D3d11va => decoder.configure_d3d11va(codec)?,
        }
        check(
            unsafe { sys::avcodec_open2(decoder.context, codec, ptr::null_mut()) },
            "avcodec_open2",
        )?;
        Ok(decoder)
    }

    pub fn stream_index(&self) -> i32 {
        self.stream_index
    }

    pub fn time_base(&self) -> TimeBase {
        self.time_base
    }

    pub fn backend(&self) -> DecoderBackend {
        self.backend
    }

    pub fn send_packet(&mut self, packet: &Packet) -> Result<()> {
        if packet.stream_index() != self.stream_index {
            return Err(FfmpegError::StreamMismatch {
                decoder_stream: self.stream_index,
                packet_stream: packet.stream_index(),
            });
        }
        check(
            unsafe { sys::avcodec_send_packet(self.context, packet.as_ptr()) },
            "avcodec_send_packet",
        )
    }

    pub fn send_eof(&mut self) -> Result<()> {
        check(
            unsafe { sys::avcodec_send_packet(self.context, ptr::null()) },
            "avcodec_send_packet_eof",
        )
    }

    pub fn receive_frame(&mut self) -> Result<DecoderOutputFrame> {
        let frame = Frame::alloc(self.time_base)?;
        let code = unsafe { sys::avcodec_receive_frame(self.context, frame.ptr) };
        if code == av_error(EAGAIN) {
            return Ok(DecoderOutputFrame::NeedMoreInput);
        }
        if code == AVERROR_EOF {
            return Ok(DecoderOutputFrame::EndOfStream);
        }
        check(code, "avcodec_receive_frame")?;
        Ok(DecoderOutputFrame::Frame(frame))
    }

    pub fn flush(&mut self) {
        unsafe { sys::avcodec_flush_buffers(self.context) };
    }

    fn configure_videotoolbox(&mut self, codec: *const sys::AVCodec) -> Result<()> {
        self.configure_hardware(
            codec,
            sys::AVHWDeviceType_AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
            "avcodec_get_hw_config(VideoToolbox)",
            "av_hwdevice_ctx_create(VideoToolbox)",
        )
    }

    fn configure_d3d11va(&mut self, codec: *const sys::AVCodec) -> Result<()> {
        self.configure_hardware(
            codec,
            sys::AVHWDeviceType_AV_HWDEVICE_TYPE_D3D11VA,
            "avcodec_get_hw_config(D3D11VA)",
            "av_hwdevice_ctx_create(D3D11VA)",
        )?;
        Ok(())
    }

    fn configure_hardware(
        &mut self,
        codec: *const sys::AVCodec,
        device_type: sys::AVHWDeviceType,
        hw_config_operation: &'static str,
        hw_device_operation: &'static str,
    ) -> Result<()> {
        let pixel_format = hardware_pixel_format(codec, device_type)
            .ok_or_else(|| FfmpegError::NullPointer(hw_config_operation))?;
        let mut device_ref = ptr::null_mut();
        check(
            unsafe {
                sys::av_hwdevice_ctx_create(
                    &mut device_ref,
                    device_type,
                    ptr::null(),
                    ptr::null_mut(),
                    0,
                )
            },
            hw_device_operation,
        )?;
        if device_ref.is_null() {
            return Err(FfmpegError::NullPointer(hw_device_operation));
        }

        let context_device_ref = unsafe { sys::av_buffer_ref(device_ref) };
        if context_device_ref.is_null() {
            unsafe { sys::av_buffer_unref(&mut device_ref) };
            return Err(FfmpegError::NullPointer("av_buffer_ref(hw_device_ctx)"));
        }

        let mut hw_state = Box::new(HardwareDecoderState {
            device_ref,
            frames_ref: ptr::null_mut(),
            pixel_format,
        });

        unsafe {
            (*self.context).hw_device_ctx = context_device_ref;
            (*self.context).opaque = (&mut *hw_state) as *mut HardwareDecoderState as *mut _;
            (*self.context).get_format = Some(select_hw_format);
        }
        self.hw_state = Some(hw_state);
        Ok(())
    }
}

#[cfg(target_os = "windows")]
unsafe fn ensure_d3d11va_frames_for_context(
    context: *mut sys::AVCodecContext,
    state: *mut HardwareDecoderState,
) {
    if !unsafe { (*state).frames_ref }.is_null() {
        return;
    }
    const D3D11_BIND_SHADER_RESOURCE: u32 = 0x8;
    const D3D11_BIND_DECODER: u32 = 0x200;
    const D3D11_RESOURCE_MISC_SHARED: u32 = 0x2;

    let mut frames_ref = unsafe { sys::av_hwframe_ctx_alloc((*state).device_ref) };
    if frames_ref.is_null() {
        trace_ffmpeg("av_hwframe_ctx_alloc(D3D11VA) returned null");
        return;
    }

    let init_result = (|| {
        let frames_ctx = unsafe { (*frames_ref).data.cast::<sys::AVHWFramesContext>().as_mut() }?;
        let d3d11_frames = unsafe {
            frames_ctx
                .hwctx
                .cast::<sys::AVD3D11VAFramesContext>()
                .as_mut()
        }?;
        frames_ctx.format = unsafe { (*state).pixel_format };
        frames_ctx.sw_format = d3d11va_sw_format_for_context(context);
        let alignment = d3d11va_surface_alignment(unsafe { (*context).codec_id });
        frames_ctx.width = align_i32(unsafe { (*context).coded_width }, alignment);
        frames_ctx.height = align_i32(unsafe { (*context).coded_height }, alignment);
        frames_ctx.initial_pool_size = d3d11va_pool_size(unsafe { (*context).codec_id });
        d3d11_frames.BindFlags = D3D11_BIND_DECODER | D3D11_BIND_SHADER_RESOURCE;
        d3d11_frames.MiscFlags = D3D11_RESOURCE_MISC_SHARED;
        let code = unsafe { sys::av_hwframe_ctx_init(frames_ref) };
        if code < 0 {
            trace_ffmpeg("av_hwframe_ctx_init(D3D11VA) failed");
            return None;
        }
        Some(())
    })();

    if init_result.is_some() {
        unsafe {
            (*state).frames_ref = frames_ref;
        }
        frames_ref = ptr::null_mut();
    }
    if !frames_ref.is_null() {
        unsafe { sys::av_buffer_unref(&mut frames_ref) };
    }
}

#[cfg(target_os = "windows")]
fn d3d11va_sw_format_for_context(context: *const sys::AVCodecContext) -> sys::AVPixelFormat {
    match unsafe { (*context).sw_pix_fmt } {
        sys::AVPixelFormat_AV_PIX_FMT_YUV420P10LE
        | sys::AVPixelFormat_AV_PIX_FMT_YUV420P10BE
        | sys::AVPixelFormat_AV_PIX_FMT_P010LE
        | sys::AVPixelFormat_AV_PIX_FMT_P010BE => sys::AVPixelFormat_AV_PIX_FMT_P010LE,
        _ => sys::AVPixelFormat_AV_PIX_FMT_NV12,
    }
}

#[cfg(target_os = "windows")]
fn d3d11va_surface_alignment(codec_id: sys::AVCodecID) -> i32 {
    if codec_id == sys::AVCodecID_AV_CODEC_ID_MPEG2VIDEO {
        32
    } else if codec_id == sys::AVCodecID_AV_CODEC_ID_HEVC
        || codec_id == sys::AVCodecID_AV_CODEC_ID_AV1
    {
        128
    } else {
        16
    }
}

#[cfg(target_os = "windows")]
fn d3d11va_pool_size(codec_id: sys::AVCodecID) -> i32 {
    if codec_id == sys::AVCodecID_AV_CODEC_ID_H264 || codec_id == sys::AVCodecID_AV_CODEC_ID_HEVC {
        20
    } else if codec_id == sys::AVCodecID_AV_CODEC_ID_VP9
        || codec_id == sys::AVCodecID_AV_CODEC_ID_AV1
    {
        12
    } else {
        5
    }
}

#[cfg(target_os = "windows")]
fn align_i32(value: i32, alignment: i32) -> i32 {
    let value = value.max(1);
    ((value + alignment - 1) / alignment) * alignment
}

fn configure_ffmpeg_debug_logging() {
    if std::env::var_os("ERIKA_FFMPEG_DEBUG").is_some() {
        unsafe { sys::av_log_set_level(sys::AV_LOG_DEBUG as c_int) };
    }
}

fn trace_ffmpeg(message: &str) {
    if std::env::var_os("ERIKA_FFMPEG_DEBUG").is_some() {
        eprintln!("erika ffmpeg: {message}");
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe { sys::avcodec_free_context(&mut self.context) };
    }
}

pub enum DecoderOutputFrame {
    Frame(Frame),
    NeedMoreInput,
    EndOfStream,
}

pub struct Frame {
    ptr: *mut sys::AVFrame,
    time_base: TimeBase,
}

pub struct AudioResampler {
    context: *mut sys::SwrContext,
    output_format: PcmFormat,
    #[allow(dead_code)]
    output_layout: ChannelLayout,
}

unsafe impl Send for AudioResampler {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoToolboxPixelBuffer<'a> {
    raw: *mut c_void,
    width: u32,
    height: u32,
    _frame: PhantomData<&'a Frame>,
}

impl VideoToolboxPixelBuffer<'_> {
    pub fn raw(self) -> *mut c_void {
        self.raw
    }

    pub fn width(self) -> u32 {
        self.width
    }

    pub fn height(self) -> u32 {
        self.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct D3d11vaTexture<'a> {
    raw_texture: *mut c_void,
    array_index: u32,
    width: u32,
    height: u32,
    _frame: PhantomData<&'a Frame>,
}

impl D3d11vaTexture<'_> {
    pub fn raw_texture(self) -> *mut c_void {
        self.raw_texture
    }

    pub fn array_index(self) -> u32 {
        self.array_index
    }

    pub fn width(self) -> u32 {
        self.width
    }

    pub fn height(self) -> u32 {
        self.height
    }
}

unsafe impl Send for Frame {}

impl Frame {
    fn alloc(time_base: TimeBase) -> Result<Self> {
        let ptr = unsafe { sys::av_frame_alloc() };
        if ptr.is_null() {
            return Err(FfmpegError::NullPointer("av_frame_alloc"));
        }
        Ok(Self { ptr, time_base })
    }

    pub fn try_clone_ref(&self) -> Result<Self> {
        let frame = Frame::alloc(self.time_base)?;
        check(
            unsafe { sys::av_frame_ref(frame.ptr, self.ptr) },
            "av_frame_ref",
        )?;
        Ok(frame)
    }

    pub fn as_ptr(&self) -> *const sys::AVFrame {
        self.ptr
    }

    pub fn width(&self) -> u32 {
        unsafe { (*self.ptr).width.max(0) as u32 }
    }

    pub fn height(&self) -> u32 {
        unsafe { (*self.ptr).height.max(0) as u32 }
    }

    pub fn sample_rate(&self) -> u32 {
        unsafe { (*self.ptr).sample_rate.max(0) as u32 }
    }

    pub fn channel_count(&self) -> u32 {
        #[cfg(erika_ffmpeg_legacy_channel_layout)]
        unsafe {
            sys::av_frame_get_channels(self.ptr).max(0) as u32
        }
        #[cfg(not(erika_ffmpeg_legacy_channel_layout))]
        unsafe {
            (*self.ptr).ch_layout.nb_channels.max(0) as u32
        }
    }

    pub fn sample_count(&self) -> usize {
        unsafe { (*self.ptr).nb_samples.max(0) as usize }
    }

    pub fn sample_format(&self) -> Option<String> {
        unsafe { sample_format_name((*self.ptr).format) }
    }

    pub fn raw_sample_format(&self) -> sys::AVSampleFormat {
        unsafe { (*self.ptr).format }
    }

    pub fn is_audio(&self) -> bool {
        self.sample_rate() > 0 && self.sample_count() > 0 && self.channel_count() > 0
    }

    pub fn pixel_format(&self) -> Option<String> {
        unsafe { pixel_format_name((*self.ptr).format) }
    }

    pub fn raw_pixel_format(&self) -> i32 {
        unsafe { (*self.ptr).format }
    }

    pub fn is_videotoolbox(&self) -> bool {
        self.raw_pixel_format() == sys::AVPixelFormat_AV_PIX_FMT_VIDEOTOOLBOX
    }

    pub fn is_d3d11va(&self) -> bool {
        self.raw_pixel_format() == sys::AVPixelFormat_AV_PIX_FMT_D3D11
    }

    pub fn has_hw_frames_context(&self) -> bool {
        unsafe { !(*self.ptr).hw_frames_ctx.is_null() }
    }

    pub fn videotoolbox_pixel_buffer(&self) -> Option<VideoToolboxPixelBuffer<'_>> {
        if !self.is_videotoolbox() {
            return None;
        }
        let raw = unsafe { (*self.ptr).data[3] }.cast::<c_void>();
        if raw.is_null() {
            return None;
        }
        Some(VideoToolboxPixelBuffer {
            raw,
            width: self.width(),
            height: self.height(),
            _frame: PhantomData,
        })
    }

    pub fn d3d11va_texture(&self) -> Option<D3d11vaTexture<'_>> {
        if !self.is_d3d11va() {
            return None;
        }
        let raw_texture = unsafe { (*self.ptr).data[0] }.cast::<c_void>();
        if raw_texture.is_null() {
            return None;
        }
        let array_index = unsafe { (*self.ptr).data[1] as usize }.try_into().ok()?;
        Some(D3d11vaTexture {
            raw_texture,
            array_index,
            width: self.width(),
            height: self.height(),
            _frame: PhantomData,
        })
    }

    pub fn raw_pts(&self) -> Option<i64> {
        timestamp_value(unsafe { (*self.ptr).pts })
    }

    pub fn pts(&self) -> Option<PacketTimestamp> {
        Some(PacketTimestamp {
            raw: self.raw_pts()?,
            time_base: self.time_base,
        })
    }

    pub fn color_primaries(&self) -> ColorPrimaries {
        unsafe { color_primaries((*self.ptr).color_primaries) }
    }

    pub fn transfer_function(&self) -> TransferFunction {
        unsafe { transfer_function((*self.ptr).color_trc) }
    }

    pub fn color_range(&self) -> ColorRange {
        unsafe { color_range((*self.ptr).color_range) }
    }

    pub fn matrix_coefficients(&self) -> MatrixCoefficients {
        unsafe { matrix_coefficients((*self.ptr).colorspace) }
    }

    pub fn hdr_metadata(&self) -> Option<HdrMetadata> {
        unsafe { frame_hdr_metadata(self.ptr) }
    }

    pub fn transfer_to_system_memory(&self) -> Result<Frame> {
        let frame = Frame::alloc(self.time_base)?;
        check(
            unsafe { sys::av_hwframe_transfer_data(frame.ptr, self.ptr, 0) },
            "av_hwframe_transfer_data",
        )?;
        check(
            unsafe { sys::av_frame_copy_props(frame.ptr, self.ptr) },
            "av_frame_copy_props",
        )?;
        Ok(frame)
    }

    /// Repack a software-decoded 8-bit 4:2:0 frame (yuv420p or nv12) into tightly
    /// packed NV12 planes, resolving the source row stride. Returns `None` for
    /// hardware frames or unsupported pixel formats (e.g. 10-bit P010).
    pub fn to_nv12(&self) -> Option<Nv12Frame> {
        let width = self.width() as usize;
        let height = self.height() as usize;
        if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
            return None;
        }
        let format = self.raw_pixel_format();
        let chroma_width = width / 2;
        let chroma_height = height / 2;

        // SAFETY: `self.ptr` is a valid AVFrame for this Frame's lifetime. For a
        // software 4:2:0 frame `data[0]` is the luma plane and `data[1..]` the
        // chroma plane(s); each row spans `linesize[i]` bytes with at least the
        // visible width of valid samples. We read only the visible region, row by
        // row, after checking the pointers are non-null and the strides are wide
        // enough.
        unsafe {
            let frame = &*self.ptr;
            let luma_ptr = frame.data[0] as *const u8;
            if luma_ptr.is_null() {
                return None;
            }
            let luma_stride = frame.linesize[0].max(0) as usize;
            if luma_stride < width {
                return None;
            }
            let mut luma = vec![0u8; width * height];
            for row in 0..height {
                let src = std::slice::from_raw_parts(luma_ptr.add(row * luma_stride), width);
                luma[row * width..row * width + width].copy_from_slice(src);
            }

            let mut chroma = vec![0u8; chroma_width * chroma_height * 2];
            if format == sys::AVPixelFormat_AV_PIX_FMT_YUV420P {
                let u_ptr = frame.data[1] as *const u8;
                let v_ptr = frame.data[2] as *const u8;
                if u_ptr.is_null() || v_ptr.is_null() {
                    return None;
                }
                let u_stride = frame.linesize[1].max(0) as usize;
                let v_stride = frame.linesize[2].max(0) as usize;
                if u_stride < chroma_width || v_stride < chroma_width {
                    return None;
                }
                for row in 0..chroma_height {
                    let u = std::slice::from_raw_parts(u_ptr.add(row * u_stride), chroma_width);
                    let v = std::slice::from_raw_parts(v_ptr.add(row * v_stride), chroma_width);
                    for col in 0..chroma_width {
                        let idx = (row * chroma_width + col) * 2;
                        chroma[idx] = u[col];
                        chroma[idx + 1] = v[col];
                    }
                }
            } else if format == sys::AVPixelFormat_AV_PIX_FMT_NV12 {
                let uv_ptr = frame.data[1] as *const u8;
                if uv_ptr.is_null() {
                    return None;
                }
                let uv_stride = frame.linesize[1].max(0) as usize;
                let row_bytes = chroma_width * 2;
                if uv_stride < row_bytes {
                    return None;
                }
                for row in 0..chroma_height {
                    let src = std::slice::from_raw_parts(uv_ptr.add(row * uv_stride), row_bytes);
                    chroma[row * row_bytes..row * row_bytes + row_bytes].copy_from_slice(src);
                }
            } else {
                return None;
            }

            Some(Nv12Frame {
                width: width as u32,
                height: height as u32,
                luma,
                chroma,
            })
        }
    }

    /// Repack a software-decoded 4:2:0 frame into GPU-ready planes: NV12 for 8-bit
    /// (yuv420p/nv12) or P010 (16-bit, MSB-aligned) for 10-bit (yuv420p10le/p010le).
    /// Returns `None` for hardware frames or unsupported formats.
    pub fn to_planar_frame(&self) -> Option<PlanarFrame> {
        let format = self.raw_pixel_format();
        if format == sys::AVPixelFormat_AV_PIX_FMT_YUV420P
            || format == sys::AVPixelFormat_AV_PIX_FMT_NV12
        {
            let nv12 = self.to_nv12()?;
            return Some(PlanarFrame {
                format: PlanarPixelFormat::Nv12,
                width: nv12.width,
                height: nv12.height,
                luma: nv12.luma,
                chroma: nv12.chroma,
            });
        }

        let width = self.width() as usize;
        let height = self.height() as usize;
        if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
            return None;
        }
        let chroma_width = width / 2;
        let chroma_height = height / 2;

        // SAFETY: `self.ptr` is a valid AVFrame. For 10-bit planar 4:2:0 the planes
        // hold 16-bit little-endian samples spanning `linesize[i]` bytes per row; the
        // helpers read only the visible region after checking pointers and strides.
        unsafe {
            let frame = &*self.ptr;
            if format == sys::AVPixelFormat_AV_PIX_FMT_YUV420P10LE {
                let luma =
                    read_10bit_plane_as_p010(frame.data[0], frame.linesize[0], width, height)?;
                let chroma = read_10bit_chroma_as_p010(
                    frame.data[1],
                    frame.linesize[1],
                    frame.data[2],
                    frame.linesize[2],
                    chroma_width,
                    chroma_height,
                )?;
                Some(PlanarFrame {
                    format: PlanarPixelFormat::P010,
                    width: width as u32,
                    height: height as u32,
                    luma,
                    chroma,
                })
            } else if format == sys::AVPixelFormat_AV_PIX_FMT_P010LE {
                let luma = copy_16bit_rows(frame.data[0], frame.linesize[0], width, height)?;
                let chroma = copy_16bit_rows(
                    frame.data[1],
                    frame.linesize[1],
                    chroma_width * 2,
                    chroma_height,
                )?;
                Some(PlanarFrame {
                    format: PlanarPixelFormat::P010,
                    width: width as u32,
                    height: height as u32,
                    luma,
                    chroma,
                })
            } else {
                None
            }
        }
    }
}

/// Tightly packed NV12 planes produced by [`Frame::to_nv12`]: an 8-bit luma plane
/// (`width * height`) and an interleaved Cb/Cr plane at half resolution
/// (`(width / 2) * (height / 2) * 2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nv12Frame {
    pub width: u32,
    pub height: u32,
    pub luma: Vec<u8>,
    pub chroma: Vec<u8>,
}

/// GPU upload format for a repacked planar frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanarPixelFormat {
    /// 8-bit NV12: R8 luma + interleaved Rg8 chroma.
    Nv12,
    /// 10-bit P010 (values MSB-aligned in 16-bit LE): R16 luma + Rg16 chroma.
    P010,
}

/// Tightly packed planar frame produced by [`Frame::to_planar_frame`]. `luma` and
/// `chroma` hold raw bytes: 1 byte/sample for [`PlanarPixelFormat::Nv12`], 2 bytes
/// (little-endian) for [`PlanarPixelFormat::P010`]. `chroma` is interleaved Cb/Cr at
/// half resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanarFrame {
    pub format: PlanarPixelFormat,
    pub width: u32,
    pub height: u32,
    pub luma: Vec<u8>,
    pub chroma: Vec<u8>,
}

/// Reads a 10-bit little-endian plane and rewrites it as P010 (value `<< 6`, so the
/// 10 bits occupy the high bits of each 16-bit sample), tightly packed.
///
/// # Safety
/// `ptr` must point to a plane with at least `stride` bytes per row for `height`
/// rows and at least `width` 16-bit samples of valid data per row.
unsafe fn read_10bit_plane_as_p010(
    ptr: *mut u8,
    stride: i32,
    width: usize,
    height: usize,
) -> Option<Vec<u8>> {
    let ptr = ptr as *const u8;
    if ptr.is_null() {
        return None;
    }
    let stride = stride.max(0) as usize;
    if stride < width * 2 {
        return None;
    }
    let mut out = Vec::with_capacity(width * height * 2);
    for row in 0..height {
        let row_ptr = unsafe { ptr.add(row * stride) };
        for col in 0..width {
            let lo = unsafe { *row_ptr.add(col * 2) };
            let hi = unsafe { *row_ptr.add(col * 2 + 1) };
            let sample = (u16::from_le_bytes([lo, hi]) & 0x03FF) << 6;
            out.extend_from_slice(&sample.to_le_bytes());
        }
    }
    Some(out)
}

/// Interleaves two 10-bit LE chroma planes (Cb, Cr) into P010 (`value << 6`) order.
///
/// # Safety
/// `u_ptr`/`v_ptr` must each point to at least `cw` 16-bit samples per row for `ch`
/// rows, spanning the given strides.
unsafe fn read_10bit_chroma_as_p010(
    u_ptr: *mut u8,
    u_stride: i32,
    v_ptr: *mut u8,
    v_stride: i32,
    cw: usize,
    ch: usize,
) -> Option<Vec<u8>> {
    let u_ptr = u_ptr as *const u8;
    let v_ptr = v_ptr as *const u8;
    if u_ptr.is_null() || v_ptr.is_null() {
        return None;
    }
    let u_stride = u_stride.max(0) as usize;
    let v_stride = v_stride.max(0) as usize;
    if u_stride < cw * 2 || v_stride < cw * 2 {
        return None;
    }
    let mut out = Vec::with_capacity(cw * ch * 4);
    for row in 0..ch {
        let u_row = unsafe { u_ptr.add(row * u_stride) };
        let v_row = unsafe { v_ptr.add(row * v_stride) };
        for col in 0..cw {
            let u = (u16::from_le_bytes([unsafe { *u_row.add(col * 2) }, unsafe {
                *u_row.add(col * 2 + 1)
            }]) & 0x03FF)
                << 6;
            let v = (u16::from_le_bytes([unsafe { *v_row.add(col * 2) }, unsafe {
                *v_row.add(col * 2 + 1)
            }]) & 0x03FF)
                << 6;
            out.extend_from_slice(&u.to_le_bytes());
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    Some(out)
}

/// Copies `samples_per_row` 16-bit samples per row for `rows` rows, resolving stride.
///
/// # Safety
/// `ptr` must point to at least `stride` bytes per row for `rows` rows with at least
/// `samples_per_row` 16-bit samples of valid data per row.
unsafe fn copy_16bit_rows(
    ptr: *mut u8,
    stride: i32,
    samples_per_row: usize,
    rows: usize,
) -> Option<Vec<u8>> {
    let ptr = ptr as *const u8;
    if ptr.is_null() {
        return None;
    }
    let stride = stride.max(0) as usize;
    let row_bytes = samples_per_row * 2;
    if stride < row_bytes {
        return None;
    }
    let mut out = Vec::with_capacity(row_bytes * rows);
    for row in 0..rows {
        let src = unsafe { std::slice::from_raw_parts(ptr.add(row * stride), row_bytes) };
        out.extend_from_slice(src);
    }
    Some(out)
}

impl AudioResampler {
    pub fn new_from_frame(frame: &Frame, output_format: PcmFormat) -> Result<Self> {
        if !frame.is_audio() {
            return Err(FfmpegError::ExpectedAudioFrame);
        }
        let output_layout = ChannelLayout::default_for_channels(output_format.channels)?;
        let context = unsafe { allocate_swr_context(frame, output_format, &output_layout)? };
        check(unsafe { sys::swr_init(context) }, "swr_init")?;
        Ok(Self {
            context,
            output_format,
            output_layout,
        })
    }

    pub fn output_format(&self) -> PcmFormat {
        self.output_format
    }

    pub fn convert(&mut self, frame: &Frame) -> Result<PcmAudioFrame> {
        if !frame.is_audio() {
            return Err(FfmpegError::ExpectedAudioFrame);
        }
        let input_samples = frame.sample_count().min(i32::MAX as usize) as i32;
        let delay = unsafe { sys::swr_get_delay(self.context, frame.sample_rate() as i64) }.max(0);
        let output_capacity = unsafe {
            sys::av_rescale_rnd(
                delay + input_samples as i64,
                self.output_format.sample_rate as i64,
                frame.sample_rate() as i64,
                sys::AVRounding_AV_ROUND_UP,
            )
        }
        .max(1)
        .min(i32::MAX as i64) as i32;
        let channels = self.output_format.channels.max(1) as usize;
        let mut samples = vec![0.0f32; output_capacity as usize * channels];
        let mut output_planes = [samples.as_mut_ptr().cast::<u8>()];
        let input = unsafe { (*frame.ptr).extended_data as *mut *const u8 };
        if input.is_null() {
            return Err(FfmpegError::NullPointer("AVFrame.extended_data"));
        }
        let converted = unsafe {
            sys::swr_convert(
                self.context,
                output_planes.as_mut_ptr(),
                output_capacity,
                input,
                input_samples,
            )
        };
        check(converted, "swr_convert")?;
        let frames = converted.max(0) as usize;
        samples.truncate(frames * channels);
        Ok(PcmAudioFrame {
            format: self.output_format,
            pts: frame.pts().and_then(|pts| pts.as_duration()),
            frames,
            samples,
        })
    }
}

impl Drop for AudioResampler {
    fn drop(&mut self) {
        unsafe { sys::swr_free(&mut self.context) };
    }
}

#[cfg(not(erika_ffmpeg_legacy_channel_layout))]
unsafe fn allocate_swr_context(
    frame: &Frame,
    output_format: PcmFormat,
    output_layout: &ChannelLayout,
) -> Result<*mut sys::SwrContext> {
    let mut context = ptr::null_mut();
    check(
        unsafe {
            sys::swr_alloc_set_opts2(
                &mut context,
                output_layout.as_ptr(),
                sys::AVSampleFormat_AV_SAMPLE_FMT_FLT,
                output_format.sample_rate as i32,
                &(*frame.ptr).ch_layout,
                frame.raw_sample_format(),
                frame.sample_rate() as i32,
                0,
                ptr::null_mut(),
            )
        },
        "swr_alloc_set_opts2",
    )?;
    if context.is_null() {
        return Err(FfmpegError::NullPointer("swr_alloc_set_opts2"));
    }
    Ok(context)
}

#[cfg(erika_ffmpeg_legacy_channel_layout)]
unsafe fn allocate_swr_context(
    frame: &Frame,
    output_format: PcmFormat,
    output_layout: &ChannelLayout,
) -> Result<*mut sys::SwrContext> {
    let input_layout = unsafe { legacy_frame_channel_layout(frame) };
    let context = unsafe {
        sys::swr_alloc_set_opts(
            ptr::null_mut(),
            output_layout.as_raw(),
            sys::AVSampleFormat_AV_SAMPLE_FMT_FLT,
            output_format.sample_rate as i32,
            input_layout,
            frame.raw_sample_format(),
            frame.sample_rate() as i32,
            0,
            ptr::null_mut(),
        )
    };
    if context.is_null() {
        return Err(FfmpegError::NullPointer("swr_alloc_set_opts"));
    }
    Ok(context)
}

#[cfg(erika_ffmpeg_legacy_channel_layout)]
unsafe fn legacy_frame_channel_layout(frame: &Frame) -> i64 {
    let layout = unsafe { sys::av_frame_get_channel_layout(frame.ptr) };
    if layout != 0 {
        layout
    } else {
        let channels = frame.channel_count().min(i32::MAX as u32) as i32;
        unsafe { sys::av_get_default_channel_layout(channels) }
    }
}

#[cfg(not(erika_ffmpeg_legacy_channel_layout))]
struct ChannelLayout {
    raw: sys::AVChannelLayout,
}

#[cfg(not(erika_ffmpeg_legacy_channel_layout))]
impl ChannelLayout {
    fn default_for_channels(channels: u32) -> Result<Self> {
        let channels = channels.min(i32::MAX as u32) as i32;
        let mut raw = sys::AVChannelLayout::default();
        unsafe { sys::av_channel_layout_default(&mut raw, channels) };
        if unsafe { sys::av_channel_layout_check(&raw) } == 0 {
            unsafe { sys::av_channel_layout_uninit(&mut raw) };
            return Err(FfmpegError::Api {
                operation: "av_channel_layout_default",
                code: -1,
                message: "invalid channel layout".to_string(),
            });
        }
        Ok(Self { raw })
    }

    fn as_ptr(&self) -> *const sys::AVChannelLayout {
        &self.raw
    }
}

#[cfg(not(erika_ffmpeg_legacy_channel_layout))]
impl Drop for ChannelLayout {
    fn drop(&mut self) {
        unsafe { sys::av_channel_layout_uninit(&mut self.raw) };
    }
}

#[cfg(erika_ffmpeg_legacy_channel_layout)]
struct ChannelLayout {
    raw: i64,
}

#[cfg(erika_ffmpeg_legacy_channel_layout)]
impl ChannelLayout {
    fn default_for_channels(channels: u32) -> Result<Self> {
        let channels = channels.min(i32::MAX as u32) as i32;
        let raw = unsafe { sys::av_get_default_channel_layout(channels) };
        if raw == 0 {
            return Err(FfmpegError::Api {
                operation: "av_get_default_channel_layout",
                code: -1,
                message: "invalid channel layout".to_string(),
            });
        }
        Ok(Self { raw })
    }

    fn as_raw(&self) -> i64 {
        self.raw
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        unsafe { sys::av_frame_free(&mut self.ptr) };
    }
}

pub struct Packet {
    ptr: *mut sys::AVPacket,
    time_base: Option<TimeBase>,
}

unsafe impl Send for Packet {}

impl Packet {
    fn alloc() -> Result<Self> {
        let ptr = unsafe { sys::av_packet_alloc() };
        if ptr.is_null() {
            return Err(FfmpegError::NullPointer("av_packet_alloc"));
        }
        Ok(Self {
            ptr,
            time_base: None,
        })
    }

    pub fn as_ptr(&self) -> *const sys::AVPacket {
        self.ptr
    }

    pub fn as_mut_ptr(&mut self) -> *mut sys::AVPacket {
        self.ptr
    }

    pub fn stream_index(&self) -> i32 {
        unsafe { (*self.ptr).stream_index }
    }

    pub fn size(&self) -> usize {
        unsafe { (*self.ptr).size.max(0) as usize }
    }

    pub fn data(&self) -> &[u8] {
        let size = self.size();
        let data = unsafe { (*self.ptr).data };
        if data.is_null() || size == 0 {
            return &[];
        }
        unsafe { slice::from_raw_parts(data, size) }
    }

    pub fn flags(&self) -> PacketFlags {
        PacketFlags {
            bits: unsafe { (*self.ptr).flags },
        }
    }

    pub fn is_key(&self) -> bool {
        self.flags().is_key()
    }

    pub fn pos(&self) -> Option<i64> {
        let pos = unsafe { (*self.ptr).pos };
        if pos < 0 { None } else { Some(pos) }
    }

    pub fn raw_pts(&self) -> Option<i64> {
        timestamp_value(unsafe { (*self.ptr).pts })
    }

    pub fn raw_dts(&self) -> Option<i64> {
        timestamp_value(unsafe { (*self.ptr).dts })
    }

    pub fn raw_duration(&self) -> Option<i64> {
        let duration = unsafe { (*self.ptr).duration };
        if duration <= 0 { None } else { Some(duration) }
    }

    pub fn time_base(&self) -> Option<TimeBase> {
        self.time_base
    }

    pub fn pts(&self) -> Option<PacketTimestamp> {
        Some(PacketTimestamp {
            raw: self.raw_pts()?,
            time_base: self.time_base?,
        })
    }

    pub fn dts(&self) -> Option<PacketTimestamp> {
        Some(PacketTimestamp {
            raw: self.raw_dts()?,
            time_base: self.time_base?,
        })
    }

    pub fn duration_seconds(&self) -> Option<f64> {
        Some(self.time_base?.seconds_from_timestamp(self.raw_duration()?))
    }
}

impl Drop for Packet {
    fn drop(&mut self) {
        unsafe { sys::av_packet_free(&mut self.ptr) };
    }
}

pub fn version() -> String {
    unsafe {
        let ptr = sys::av_version_info();
        if ptr.is_null() {
            return "unknown".to_string();
        }
        CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

pub fn probe_path(path: impl AsRef<Path>) -> Result<MediaProbe> {
    let uri = path.as_ref().to_string_lossy().into_owned();
    probe_uri(&uri)
}

pub fn probe_uri(uri: &str) -> Result<MediaProbe> {
    Ok(Demuxer::open_uri(uri)?.probe().clone())
}

struct FormatContext {
    ptr: *mut sys::AVFormatContext,
    avio: Option<Box<CustomAvio>>,
}

impl FormatContext {
    fn new(ptr: *mut sys::AVFormatContext) -> Result<Self> {
        if ptr.is_null() {
            return Err(FfmpegError::NullPointer("avformat_open_input"));
        }
        Ok(Self { ptr, avio: None })
    }

    fn new_with_custom_io(ptr: *mut sys::AVFormatContext, avio: Box<CustomAvio>) -> Result<Self> {
        if ptr.is_null() {
            return Err(FfmpegError::NullPointer("avformat_open_input"));
        }
        Ok(Self {
            ptr,
            avio: Some(avio),
        })
    }

    fn as_ptr(&self) -> *const sys::AVFormatContext {
        self.ptr
    }

    fn as_mut_ptr(&mut self) -> *mut sys::AVFormatContext {
        self.ptr
    }
}

impl Drop for FormatContext {
    fn drop(&mut self) {
        unsafe { sys::avformat_close_input(&mut self.ptr) };
        let _ = self.avio.take();
    }
}

struct CustomAvio {
    context: *mut sys::AVIOContext,
    source: Box<dyn MediaSource>,
    offset: u64,
}

impl CustomAvio {
    const BUFFER_SIZE: usize = 64 * 1024;

    fn new(source: Box<dyn MediaSource>) -> Result<Box<Self>> {
        let buffer = unsafe { sys::av_malloc(Self::BUFFER_SIZE) }.cast::<u8>();
        if buffer.is_null() {
            return Err(FfmpegError::NullPointer("av_malloc(avio buffer)"));
        }

        let mut avio = Box::new(Self {
            context: ptr::null_mut(),
            source,
            offset: 0,
        });
        let context = unsafe {
            sys::avio_alloc_context(
                buffer,
                Self::BUFFER_SIZE as c_int,
                0,
                (&mut *avio) as *mut CustomAvio as *mut c_void,
                Some(custom_avio_read_packet),
                None,
                Some(custom_avio_seek),
            )
        };
        if context.is_null() {
            unsafe { sys::av_free(buffer.cast::<c_void>()) };
            return Err(FfmpegError::NullPointer("avio_alloc_context"));
        }
        avio.context = context;
        Ok(avio)
    }

    fn context(&self) -> *mut sys::AVIOContext {
        self.context
    }

    fn read_packet(&mut self, buffer: *mut u8, buffer_size: c_int) -> c_int {
        if buffer.is_null() || buffer_size <= 0 {
            return av_error(EINVAL);
        }
        let length = buffer_size as u64;
        match self.source.read_range(ByteRange {
            start: self.offset,
            length: Some(length),
        }) {
            Ok(bytes) if bytes.is_empty() => AVERROR_EOF,
            Ok(bytes) => {
                let copy_len = bytes.len().min(buffer_size as usize);
                unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), buffer, copy_len) };
                self.offset = self.offset.saturating_add(copy_len as u64);
                copy_len as c_int
            }
            Err(_) => av_error(EIO),
        }
    }

    fn seek(&mut self, offset: i64, whence: c_int) -> i64 {
        if whence == sys::AVSEEK_SIZE as c_int {
            return match self.source.len() {
                Ok(Some(length)) => length.min(i64::MAX as u64) as i64,
                Ok(None) => av_error(ESPIPE) as i64,
                Err(_) => av_error(EIO) as i64,
            };
        }

        let target = match whence {
            SEEK_SET => offset,
            SEEK_CUR => self.offset as i64 + offset,
            SEEK_END => match self.source.len() {
                Ok(Some(length)) => length.min(i64::MAX as u64) as i64 + offset,
                Ok(None) => return av_error(ESPIPE) as i64,
                Err(_) => return av_error(EIO) as i64,
            },
            _ => return av_error(EINVAL) as i64,
        };
        if target < 0 {
            return av_error(EINVAL) as i64;
        }
        self.offset = target as u64;
        self.offset.min(i64::MAX as u64) as i64
    }
}

impl Drop for CustomAvio {
    fn drop(&mut self) {
        if !self.context.is_null() {
            let buffer = unsafe { (*self.context).buffer };
            let mut context = self.context;
            unsafe { sys::avio_context_free(&mut context) };
            if !buffer.is_null() {
                unsafe { sys::av_free(buffer.cast::<c_void>()) };
            }
            self.context = ptr::null_mut();
        }
    }
}

unsafe extern "C" fn custom_avio_read_packet(
    opaque: *mut c_void,
    buffer: *mut u8,
    buffer_size: c_int,
) -> c_int {
    if opaque.is_null() {
        return av_error(EINVAL);
    }
    unsafe { (&mut *(opaque.cast::<CustomAvio>())).read_packet(buffer, buffer_size) }
}

unsafe extern "C" fn custom_avio_seek(opaque: *mut c_void, offset: i64, whence: c_int) -> i64 {
    if opaque.is_null() {
        return av_error(EINVAL) as i64;
    }
    unsafe { (&mut *(opaque.cast::<CustomAvio>())).seek(offset, whence) }
}

fn open_format_context(uri: &str) -> Result<FormatContext> {
    let input = CString::new(uri).map_err(|_| FfmpegError::InteriorNul)?;
    let mut format_context = ptr::null_mut();
    check(
        unsafe {
            sys::avformat_open_input(
                &mut format_context,
                input.as_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        "avformat_open_input",
    )?;
    FormatContext::new(format_context)
}

fn open_source_format_context(source: Box<dyn MediaSource>) -> Result<FormatContext> {
    let uri = CString::new(source.uri()).map_err(|_| FfmpegError::InteriorNul)?;
    let avio = CustomAvio::new(source)?;
    let format_context = unsafe { sys::avformat_alloc_context() };
    if format_context.is_null() {
        return Err(FfmpegError::NullPointer("avformat_alloc_context"));
    }
    unsafe {
        (*format_context).pb = avio.context();
        (*format_context).flags |= sys::AVFMT_FLAG_CUSTOM_IO as c_int;
    }

    let mut opened_context = format_context;
    match check(
        unsafe {
            sys::avformat_open_input(
                &mut opened_context,
                uri.as_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        "avformat_open_input(custom_io)",
    ) {
        Ok(()) => FormatContext::new_with_custom_io(opened_context, avio),
        Err(error) => {
            if !opened_context.is_null() {
                unsafe { sys::avformat_close_input(&mut opened_context) };
            }
            Err(error)
        }
    }
}

fn find_stream_info(context: &mut FormatContext) -> Result<()> {
    check(
        unsafe { sys::avformat_find_stream_info(context.as_mut_ptr(), ptr::null_mut()) },
        "avformat_find_stream_info",
    )
}

fn inspect_format_context(
    uri: &str,
    raw: *const sys::AVFormatContext,
) -> (MediaProbe, Vec<Option<TimeBase>>) {
    let duration = unsafe { duration_from_av((*raw).duration) };
    let stream_count = unsafe { (*raw).nb_streams as usize };
    let mut tracks = Vec::with_capacity(stream_count);
    let mut video = Vec::new();
    let mut audio = Vec::new();
    let mut subtitles = Vec::new();
    let mut stream_time_bases = Vec::with_capacity(stream_count);

    for index in 0..stream_count {
        let stream = unsafe { *(*raw).streams.add(index) };
        if stream.is_null() {
            stream_time_bases.push(None);
            continue;
        }

        stream_time_bases.push(Some(TimeBase::from_av(unsafe { (*stream).time_base })));

        let codecpar = unsafe { (*stream).codecpar };
        if codecpar.is_null() {
            continue;
        }

        let Some(kind) = (unsafe { track_kind((*codecpar).codec_type) }) else {
            continue;
        };

        let codec = unsafe { codec_name((*codecpar).codec_id) };
        let mut track = TrackInfo::embedded(unsafe { (*stream).index as i64 }, kind);
        track.title = metadata_value(unsafe { (*stream).metadata }, "title");
        track.language = metadata_value(unsafe { (*stream).metadata }, "language");
        track.codec = codec.clone();

        if kind == TrackKind::Video {
            video.push(unsafe { video_probe(&track, codecpar) });
        }
        if kind == TrackKind::Audio {
            audio.push(unsafe { audio_probe(&track, codecpar) });
        }
        if kind == TrackKind::Subtitle {
            subtitles.push(subtitle_probe(&track));
        }

        tracks.push(track);
    }

    (
        MediaProbe {
            uri: uri.to_string(),
            duration,
            tracks,
            video,
            audio,
            subtitles,
        },
        stream_time_bases,
    )
}

fn check(code: i32, operation: &'static str) -> Result<()> {
    if code >= 0 {
        Ok(())
    } else {
        Err(FfmpegError::Api {
            operation,
            code,
            message: error_string(code),
        })
    }
}

fn error_string(code: i32) -> String {
    let mut buffer = [0i8; 256];
    unsafe {
        if sys::av_strerror(code, buffer.as_mut_ptr(), buffer.len()) == 0 {
            CStr::from_ptr(buffer.as_ptr())
                .to_string_lossy()
                .into_owned()
        } else {
            "unknown FFmpeg error".to_string()
        }
    }
}

unsafe fn duration_from_av(duration: i64) -> Option<Duration> {
    if duration <= 0 || duration == i64::MIN {
        return None;
    }
    let micros = duration as u64;
    Some(Duration::from_micros(micros))
}

unsafe fn track_kind(kind: sys::AVMediaType) -> Option<TrackKind> {
    match kind {
        sys::AVMediaType_AVMEDIA_TYPE_VIDEO => Some(TrackKind::Video),
        sys::AVMediaType_AVMEDIA_TYPE_AUDIO => Some(TrackKind::Audio),
        sys::AVMediaType_AVMEDIA_TYPE_SUBTITLE => Some(TrackKind::Subtitle),
        _ => None,
    }
}

unsafe fn codec_name(codec_id: sys::AVCodecID) -> Option<String> {
    let descriptor = unsafe { sys::avcodec_descriptor_get(codec_id) };
    if descriptor.is_null() {
        return None;
    }
    let name = unsafe { (*descriptor).name };
    if name.is_null() {
        return None;
    }
    Some(
        unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned(),
    )
}

unsafe fn video_probe(track: &TrackInfo, codecpar: *const sys::AVCodecParameters) -> VideoProbe {
    let width = unsafe { (*codecpar).width.max(0) as u32 };
    let height = unsafe { (*codecpar).height.max(0) as u32 };
    let pixel_format = unsafe { pixel_format_name((*codecpar).format) };
    let codec = track.codec.clone();
    let profile = codec
        .as_deref()
        .and_then(|codec_name| unsafe { profile_name(codecpar, codec_name) });
    VideoProbe {
        track_id: track.id,
        params: VideoParams {
            width,
            height,
            primaries: unsafe { color_primaries((*codecpar).color_primaries) },
            transfer: unsafe { transfer_function((*codecpar).color_trc) },
        },
        codec,
        pixel_format,
        profile,
        level: Some(unsafe { (*codecpar).level }).filter(|level| *level > 0),
    }
}

unsafe fn audio_probe(track: &TrackInfo, codecpar: *const sys::AVCodecParameters) -> AudioProbe {
    AudioProbe {
        track_id: track.id,
        codec: track.codec.clone(),
        sample_rate: unsafe { (*codecpar).sample_rate.max(0) as u32 },
        #[cfg(erika_ffmpeg_legacy_channel_layout)]
        channels: unsafe { (*codecpar).channels.max(0) as u32 },
        #[cfg(not(erika_ffmpeg_legacy_channel_layout))]
        channels: unsafe { (*codecpar).ch_layout.nb_channels.max(0) as u32 },
        sample_format: unsafe { sample_format_name((*codecpar).format) },
    }
}

fn subtitle_probe(track: &TrackInfo) -> SubtitleTrackConfig {
    let mut config = SubtitleTrackConfig::embedded(track.id, track.id);
    config.language = track.language.clone();
    config.title = track.title.clone();
    config
}

unsafe fn import_av_subtitle(
    track_id: i64,
    packet: &Packet,
    subtitle: &sys::AVSubtitle,
) -> Result<DecodedSubtitleFrame> {
    let start = subtitle_start_time(packet, subtitle);
    let start_offset = Duration::from_millis(u64::from(subtitle.start_display_time));
    let start = start.map(|pts| pts.saturating_add(start_offset));
    let end = subtitle_end_time(start, subtitle);
    let mut frame = DecodedSubtitleFrame::new(track_id, start, end);

    let rect_count = subtitle.num_rects as usize;
    if rect_count == 0 || subtitle.rects.is_null() {
        return Ok(frame);
    }

    for index in 0..rect_count {
        let rect = unsafe { *subtitle.rects.add(index) };
        if rect.is_null() {
            continue;
        }
        let rect = unsafe { &*rect };
        let forced = subtitle_rect_forced(rect);
        match rect.type_ {
            sys::AVSubtitleType_SUBTITLE_TEXT => {
                if let Some(text) = unsafe { subtitle_c_string(rect.text) } {
                    frame.push_text(
                        SubtitleTextSegment::new(SubtitleTextFormat::PlainText, text)
                            .with_forced(forced),
                    );
                }
            }
            sys::AVSubtitleType_SUBTITLE_ASS => {
                if let Some(text) = unsafe { subtitle_c_string(rect.ass) } {
                    frame.push_text(
                        SubtitleTextSegment::new(SubtitleTextFormat::Ass, text).with_forced(forced),
                    );
                }
            }
            sys::AVSubtitleType_SUBTITLE_BITMAP => {
                if let Some(plane) = unsafe { subtitle_bitmap_rect_to_rgba_plane(rect) }? {
                    frame.push_bitmap_plane(plane, forced);
                }
            }
            _ => {}
        }
    }

    Ok(frame)
}

fn subtitle_start_time(packet: &Packet, subtitle: &sys::AVSubtitle) -> Option<Duration> {
    if subtitle.pts != i64::MIN {
        let seconds = subtitle.pts as f64 / f64::from(sys::AV_TIME_BASE);
        if seconds.is_finite() && seconds >= 0.0 {
            return Some(Duration::from_secs_f64(seconds));
        }
    }
    packet.pts().and_then(PacketTimestamp::as_duration)
}

fn subtitle_end_time(start: Option<Duration>, subtitle: &sys::AVSubtitle) -> Option<Duration> {
    let start = start?;
    if subtitle.end_display_time <= subtitle.start_display_time
        || subtitle.end_display_time == u32::MAX
    {
        return None;
    }
    let duration_ms = subtitle
        .end_display_time
        .saturating_sub(subtitle.start_display_time);
    Some(start.saturating_add(Duration::from_millis(u64::from(duration_ms))))
}

unsafe fn subtitle_c_string(ptr: *const libc::c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    (!text.is_empty()).then_some(text)
}

fn subtitle_rect_forced(rect: &sys::AVSubtitleRect) -> bool {
    rect.flags & sys::AV_SUBTITLE_FLAG_FORCED as i32 != 0
}

unsafe fn subtitle_bitmap_rect_to_rgba_plane(
    rect: &sys::AVSubtitleRect,
) -> Result<Option<SubtitleBitmapPlane>> {
    if rect.w <= 0 || rect.h <= 0 {
        return Ok(None);
    }
    if rect.data[0].is_null()
        || rect.data[1].is_null()
        || rect.linesize[0] < rect.w
        || rect.nb_colors <= 0
        || rect.nb_colors > sys::AVPALETTE_COUNT as i32
    {
        return Err(FfmpegError::InvalidSubtitleBitmap {
            width: rect.w,
            height: rect.h,
            stride: rect.linesize[0],
            colors: rect.nb_colors,
        });
    }

    let width = rect.w as usize;
    let height = rect.h as usize;
    let stride = rect.linesize[0] as usize;
    let mut rgba = vec![0u8; width.saturating_mul(height).saturating_mul(4)];
    let palette =
        unsafe { std::slice::from_raw_parts(rect.data[1].cast::<u32>(), rect.nb_colors as usize) };

    for y in 0..height {
        let row = unsafe { std::slice::from_raw_parts(rect.data[0].add(y * stride), width) };
        for (x, index) in row.iter().copied().enumerate() {
            let color = palette.get(index as usize).copied().unwrap_or(0);
            let dst = &mut rgba[(y * width + x) * 4..][..4];
            dst.copy_from_slice(&palette_color_to_rgba(color));
        }
    }

    Ok(Some(SubtitleBitmapPlane {
        x: rect.x,
        y: rect.y,
        width: rect.w as u32,
        height: rect.h as u32,
        rgba,
    }))
}

fn palette_color_to_rgba(color: u32) -> [u8; 4] {
    [
        ((color >> 16) & 0xff) as u8,
        ((color >> 8) & 0xff) as u8,
        (color & 0xff) as u8,
        ((color >> 24) & 0xff) as u8,
    ]
}

unsafe fn pixel_format_name(format: i32) -> Option<String> {
    if format < 0 {
        return None;
    }
    let name = unsafe { sys::av_get_pix_fmt_name(format) };
    if name.is_null() {
        return None;
    }
    Some(
        unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned(),
    )
}

unsafe fn sample_format_name(format: i32) -> Option<String> {
    if format < 0 {
        return None;
    }
    let name = unsafe { sys::av_get_sample_fmt_name(format) };
    if name.is_null() {
        return None;
    }
    Some(
        unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned(),
    )
}

unsafe fn profile_name(
    codecpar: *const sys::AVCodecParameters,
    codec_name: &str,
) -> Option<String> {
    let codec_id = unsafe { (*codecpar).codec_id };
    let profile = unsafe { (*codecpar).profile };
    if profile == sys::FF_PROFILE_UNKNOWN {
        return None;
    }
    let name = unsafe { sys::av_get_profile_name(sys::avcodec_find_decoder(codec_id), profile) };
    if name.is_null() {
        return Some(format!("{codec_name}:{profile}"));
    }
    Some(
        unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned(),
    )
}

unsafe fn color_primaries(value: sys::AVColorPrimaries) -> ColorPrimaries {
    match value {
        sys::AVColorPrimaries_AVCOL_PRI_BT709 => ColorPrimaries::Bt709,
        sys::AVColorPrimaries_AVCOL_PRI_SMPTE432 => ColorPrimaries::DisplayP3,
        sys::AVColorPrimaries_AVCOL_PRI_BT2020 => ColorPrimaries::Bt2020,
        _ => ColorPrimaries::Unknown,
    }
}

unsafe fn transfer_function(value: sys::AVColorTransferCharacteristic) -> TransferFunction {
    match value {
        sys::AVColorTransferCharacteristic_AVCOL_TRC_IEC61966_2_1 => TransferFunction::Srgb,
        sys::AVColorTransferCharacteristic_AVCOL_TRC_BT709 => TransferFunction::Bt1886,
        sys::AVColorTransferCharacteristic_AVCOL_TRC_SMPTE2084 => TransferFunction::Pq,
        sys::AVColorTransferCharacteristic_AVCOL_TRC_ARIB_STD_B67 => TransferFunction::Hlg,
        _ => TransferFunction::Unknown,
    }
}

unsafe fn color_range(value: sys::AVColorRange) -> ColorRange {
    match value {
        sys::AVColorRange_AVCOL_RANGE_MPEG => ColorRange::Limited,
        sys::AVColorRange_AVCOL_RANGE_JPEG => ColorRange::Full,
        _ => ColorRange::Unspecified,
    }
}

unsafe fn matrix_coefficients(value: sys::AVColorSpace) -> MatrixCoefficients {
    match value {
        sys::AVColorSpace_AVCOL_SPC_RGB => MatrixCoefficients::Identity,
        sys::AVColorSpace_AVCOL_SPC_BT709 => MatrixCoefficients::Bt709,
        sys::AVColorSpace_AVCOL_SPC_BT470BG | sys::AVColorSpace_AVCOL_SPC_SMPTE170M => {
            MatrixCoefficients::Bt601
        }
        sys::AVColorSpace_AVCOL_SPC_BT2020_NCL => MatrixCoefficients::Bt2020NonConstantLuminance,
        _ => MatrixCoefficients::Unspecified,
    }
}

unsafe fn frame_hdr_metadata(frame: *const sys::AVFrame) -> Option<HdrMetadata> {
    let mastering_display = unsafe { mastering_display_metadata(frame) };
    let content_light = unsafe { content_light_metadata(frame) };
    if mastering_display.is_none() && content_light.is_none() {
        return None;
    }
    Some(HdrMetadata::new(mastering_display, content_light))
}

unsafe fn mastering_display_metadata(
    frame: *const sys::AVFrame,
) -> Option<MasteringDisplayMetadata> {
    let metadata = unsafe {
        read_frame_side_data::<sys::AVMasteringDisplayMetadata>(
            frame,
            sys::AVFrameSideDataType_AV_FRAME_DATA_MASTERING_DISPLAY_METADATA,
        )
    }?;

    let has_primaries = metadata.has_primaries != 0;
    let has_luminance = metadata.has_luminance != 0;
    let display_primaries = has_primaries
        .then(|| {
            Some([
                rational_chromaticity(metadata.display_primaries[0])?,
                rational_chromaticity(metadata.display_primaries[1])?,
                rational_chromaticity(metadata.display_primaries[2])?,
            ])
        })
        .flatten();
    let white_point = has_primaries
        .then(|| rational_chromaticity(metadata.white_point))
        .flatten();
    let min_luminance_nits = has_luminance
        .then(|| rational_to_positive_f32(metadata.min_luminance))
        .flatten();
    let max_luminance_nits = has_luminance
        .then(|| rational_to_positive_f32(metadata.max_luminance))
        .flatten();

    if display_primaries.is_none()
        && white_point.is_none()
        && min_luminance_nits.is_none()
        && max_luminance_nits.is_none()
    {
        return None;
    }

    Some(MasteringDisplayMetadata {
        display_primaries,
        white_point,
        min_luminance_nits,
        max_luminance_nits,
    })
}

unsafe fn content_light_metadata(frame: *const sys::AVFrame) -> Option<ContentLightMetadata> {
    let metadata = unsafe {
        read_frame_side_data::<sys::AVContentLightMetadata>(
            frame,
            sys::AVFrameSideDataType_AV_FRAME_DATA_CONTENT_LIGHT_LEVEL,
        )
    }?;
    if metadata.MaxCLL == 0 && metadata.MaxFALL == 0 {
        return None;
    }
    Some(ContentLightMetadata {
        max_content_light_level_nits: metadata.MaxCLL,
        max_frame_average_light_level_nits: metadata.MaxFALL,
    })
}

#[allow(irrefutable_let_patterns)]
unsafe fn read_frame_side_data<T: Copy>(
    frame: *const sys::AVFrame,
    side_data_type: sys::AVFrameSideDataType,
) -> Option<T> {
    if frame.is_null() {
        return None;
    }
    let side_data = unsafe { sys::av_frame_get_side_data(frame, side_data_type) };
    if side_data.is_null() {
        return None;
    }
    let data = unsafe { (*side_data).data };
    let size = unsafe { (*side_data).size };
    let Ok(size) = usize::try_from(size) else {
        return None;
    };
    if data.is_null() || size < mem::size_of::<T>() {
        return None;
    }
    Some(unsafe { ptr::read_unaligned(data.cast::<T>()) })
}

fn rational_chromaticity(values: [sys::AVRational; 2]) -> Option<Chromaticity> {
    Some(Chromaticity::new(
        rational_to_positive_f32(values[0])?,
        rational_to_positive_f32(values[1])?,
    ))
}

fn rational_to_positive_f32(value: sys::AVRational) -> Option<f32> {
    if value.den == 0 {
        return None;
    }
    let value = value.num as f32 / value.den as f32;
    if value.is_finite() && value > 0.0 {
        Some(value)
    } else {
        None
    }
}

fn metadata_value(metadata: *mut sys::AVDictionary, key: &str) -> Option<String> {
    let key = CString::new(key).ok()?;
    unsafe {
        let entry = sys::av_dict_get(metadata, key.as_ptr(), ptr::null(), 0);
        if entry.is_null() || (*entry).value.is_null() {
            return None;
        }
        Some(
            CStr::from_ptr((*entry).value)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

fn timestamp_value(value: i64) -> Option<i64> {
    if value == i64::MIN { None } else { Some(value) }
}

fn av_error(errno: i32) -> i32 {
    -errno
}

fn hardware_pixel_format(
    codec: *const sys::AVCodec,
    device_type: sys::AVHWDeviceType,
) -> Option<sys::AVPixelFormat> {
    let mut index = 0;
    loop {
        let config = unsafe { sys::avcodec_get_hw_config(codec, index) };
        if config.is_null() {
            return None;
        }
        let supports_device_ctx = unsafe {
            (*config).device_type == device_type
                && ((*config).methods & sys::AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as i32) != 0
        };
        if supports_device_ctx {
            return Some(unsafe { (*config).pix_fmt });
        }
        index += 1;
    }
}

unsafe extern "C" fn select_hw_format(
    context: *mut sys::AVCodecContext,
    formats: *const sys::AVPixelFormat,
) -> sys::AVPixelFormat {
    let state = unsafe { (*context).opaque as *const HardwareDecoderState };
    if !state.is_null() {
        let target = unsafe { (*state).pixel_format };
        let mut index = 0usize;
        loop {
            let format = unsafe { *formats.add(index) };
            if format == sys::AVPixelFormat_AV_PIX_FMT_NONE {
                break;
            }
            if format == target {
                #[cfg(target_os = "windows")]
                if target == sys::AVPixelFormat_AV_PIX_FMT_D3D11 {
                    unsafe { ensure_d3d11va_frames_for_context(context, state.cast_mut()) };
                }
                let frames_ref = unsafe { (*state).frames_ref };
                if !frames_ref.is_null() {
                    let context_frames = unsafe { &mut (*context).hw_frames_ctx };
                    if !(*context_frames).is_null() {
                        unsafe { sys::av_buffer_unref(context_frames) };
                    }
                    let frames_ref = unsafe { sys::av_buffer_ref(frames_ref) };
                    if !frames_ref.is_null() {
                        *context_frames = frames_ref;
                    }
                }
                return format;
            }
            index += 1;
        }
    }
    unsafe { sys::avcodec_default_get_format(context, formats) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linked_ffmpeg_reports_version() {
        assert!(!version().is_empty());
    }

    #[test]
    fn time_base_converts_packet_timestamps() {
        let timestamp = PacketTimestamp {
            raw: 24,
            time_base: TimeBase { num: 1, den: 24 },
        };
        assert_eq!(timestamp.seconds(), 1.0);
        assert_eq!(timestamp.as_duration(), Some(Duration::from_secs(1)));
    }

    #[test]
    fn stream_selection_accepts_expected_streams() {
        let selection = StreamSelection::only([0, 2]);
        assert!(selection.accepts(0));
        assert!(!selection.accepts(1));
        assert!(selection.accepts(2));
    }

    #[test]
    fn maps_ffmpeg_color_ranges() {
        assert_eq!(
            unsafe { color_range(sys::AVColorRange_AVCOL_RANGE_MPEG) },
            ColorRange::Limited
        );
        assert_eq!(
            unsafe { color_range(sys::AVColorRange_AVCOL_RANGE_JPEG) },
            ColorRange::Full
        );
        assert_eq!(
            unsafe { color_range(sys::AVColorRange_AVCOL_RANGE_UNSPECIFIED) },
            ColorRange::Unspecified
        );
    }

    #[test]
    fn maps_ffmpeg_matrix_coefficients() {
        assert_eq!(
            unsafe { matrix_coefficients(sys::AVColorSpace_AVCOL_SPC_BT709) },
            MatrixCoefficients::Bt709
        );
        assert_eq!(
            unsafe { matrix_coefficients(sys::AVColorSpace_AVCOL_SPC_SMPTE170M) },
            MatrixCoefficients::Bt601
        );
        assert_eq!(
            unsafe { matrix_coefficients(sys::AVColorSpace_AVCOL_SPC_BT470BG) },
            MatrixCoefficients::Bt601
        );
        assert_eq!(
            unsafe { matrix_coefficients(sys::AVColorSpace_AVCOL_SPC_BT2020_NCL) },
            MatrixCoefficients::Bt2020NonConstantLuminance
        );
        assert_eq!(
            unsafe { matrix_coefficients(sys::AVColorSpace_AVCOL_SPC_RGB) },
            MatrixCoefficients::Identity
        );
        assert_eq!(
            unsafe { matrix_coefficients(sys::AVColorSpace_AVCOL_SPC_UNSPECIFIED) },
            MatrixCoefficients::Unspecified
        );
    }

    #[test]
    fn subtitle_probe_marks_embedded_tracks_non_removable() {
        let mut track = TrackInfo::embedded(3, TrackKind::Subtitle);
        track.title = Some("Signs".to_string());
        track.language = Some("jpn".to_string());
        track.codec = Some("hdmv_pgs_subtitle".to_string());

        let config = subtitle_probe(&track);

        assert_eq!(config.id, 3);
        assert_eq!(config.language.as_deref(), Some("jpn"));
        assert_eq!(config.title.as_deref(), Some("Signs"));
        assert!(config.source.is_embedded());
        assert!(!config.can_remove());
    }

    #[test]
    fn imports_av_subtitle_text_and_ass_rects() {
        let packet = Packet::alloc().unwrap();
        let plain = CString::new("hello").unwrap();
        let ass = CString::new("Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,hi").unwrap();
        let mut text_rect = sys::AVSubtitleRect {
            type_: sys::AVSubtitleType_SUBTITLE_TEXT,
            text: plain.as_ptr().cast_mut(),
            flags: sys::AV_SUBTITLE_FLAG_FORCED as i32,
            ..sys::AVSubtitleRect::default()
        };
        let mut ass_rect = sys::AVSubtitleRect {
            type_: sys::AVSubtitleType_SUBTITLE_ASS,
            ass: ass.as_ptr().cast_mut(),
            ..sys::AVSubtitleRect::default()
        };
        let mut rects = [&mut text_rect as *mut _, &mut ass_rect as *mut _];
        let subtitle = sys::AVSubtitle {
            start_display_time: 250,
            end_display_time: 1250,
            num_rects: rects.len() as u32,
            rects: rects.as_mut_ptr(),
            pts: 1_000_000,
            ..sys::AVSubtitle::default()
        };

        let frame = unsafe { import_av_subtitle(7, &packet, &subtitle) }.unwrap();

        assert_eq!(frame.track_id, 7);
        assert_eq!(frame.start, Some(Duration::from_millis(1250)));
        assert_eq!(frame.end, Some(Duration::from_millis(2250)));
        assert_eq!(frame.text.len(), 2);
        assert_eq!(frame.text[0].format, SubtitleTextFormat::PlainText);
        assert_eq!(frame.text[0].text, "hello");
        assert!(frame.text[0].forced);
        assert_eq!(frame.text[1].format, SubtitleTextFormat::Ass);
        assert!(frame.forced);
    }

    #[test]
    fn imports_palette_bitmap_subtitle_as_rgba_plane() {
        let packet = Packet::alloc().unwrap();
        let pixels = [0u8, 1, 2, 1];
        let palette = [0x00000000u32, 0x804020ff, 0xff00ff80];
        let mut rect = sys::AVSubtitleRect {
            x: 11,
            y: 22,
            w: 2,
            h: 2,
            nb_colors: palette.len() as i32,
            data: [
                pixels.as_ptr().cast_mut(),
                palette.as_ptr().cast::<u8>().cast_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ],
            linesize: [2, 0, 0, 0],
            type_: sys::AVSubtitleType_SUBTITLE_BITMAP,
            ..sys::AVSubtitleRect::default()
        };
        let mut rects = [&mut rect as *mut _];
        let subtitle = sys::AVSubtitle {
            num_rects: 1,
            rects: rects.as_mut_ptr(),
            pts: 2_000_000,
            start_display_time: 0,
            end_display_time: 500,
            ..sys::AVSubtitle::default()
        };

        let frame = unsafe { import_av_subtitle(9, &packet, &subtitle) }.unwrap();

        assert_eq!(frame.start, Some(Duration::from_secs(2)));
        assert_eq!(frame.end, Some(Duration::from_millis(2500)));
        assert_eq!(frame.bitmap.planes.len(), 1);
        let plane = &frame.bitmap.planes[0];
        assert_eq!(
            (plane.x, plane.y, plane.width, plane.height),
            (11, 22, 2, 2)
        );
        assert_eq!(
            plane.rgba,
            vec![
                0, 0, 0, 0, 0x40, 0x20, 0xff, 0x80, 0x00, 0xff, 0x80, 0xff, 0x40, 0x20, 0xff, 0x80,
            ]
        );
    }

    #[test]
    fn rejects_malformed_bitmap_subtitle_rect() {
        let mut rect = sys::AVSubtitleRect {
            w: 4,
            h: 2,
            linesize: [3, 0, 0, 0],
            nb_colors: 1,
            type_: sys::AVSubtitleType_SUBTITLE_BITMAP,
            ..sys::AVSubtitleRect::default()
        };
        let pixels = [0u8; 8];
        let palette = [0xffffffffu32];
        rect.data[0] = pixels.as_ptr().cast_mut();
        rect.data[1] = palette.as_ptr().cast::<u8>().cast_mut();

        let error = unsafe { subtitle_bitmap_rect_to_rgba_plane(&rect) }.unwrap_err();

        assert!(matches!(error, FfmpegError::InvalidSubtitleBitmap { .. }));
    }

    #[test]
    fn frame_reads_hdr_side_data() {
        let frame = Frame::alloc(TimeBase { num: 1, den: 1 }).unwrap();
        unsafe {
            let mastering = sys::av_mastering_display_metadata_create_side_data(frame.ptr);
            assert!(!mastering.is_null());
            (*mastering).has_primaries = 1;
            (*mastering).display_primaries[0] = [rational(708, 1000), rational(292, 1000)];
            (*mastering).display_primaries[1] = [rational(170, 1000), rational(797, 1000)];
            (*mastering).display_primaries[2] = [rational(131, 1000), rational(46, 1000)];
            (*mastering).white_point = [rational(3127, 10000), rational(3290, 10000)];
            (*mastering).has_luminance = 1;
            (*mastering).min_luminance = rational(5, 1000);
            (*mastering).max_luminance = rational(1000, 1);

            let content_light = sys::av_content_light_metadata_create_side_data(frame.ptr);
            assert!(!content_light.is_null());
            (*content_light).MaxCLL = 4000;
            (*content_light).MaxFALL = 450;
        }

        let metadata = frame.hdr_metadata().unwrap();
        let mastering = metadata.mastering_display.unwrap();
        let content_light = metadata.content_light.unwrap();

        assert_eq!(metadata.nominal_peak_nits(), Some(1000.0));
        assert_eq!(content_light.max_content_light_level_nits, 4000);
        assert_eq!(content_light.max_frame_average_light_level_nits, 450);
        assert_close(mastering.max_luminance_nits.unwrap(), 1000.0);
        assert_close(mastering.min_luminance_nits.unwrap(), 0.005);
        assert_close(mastering.display_primaries.unwrap()[0].x, 0.708);
        assert_close(mastering.white_point.unwrap().y, 0.329);
    }

    fn rational(num: i32, den: i32) -> sys::AVRational {
        sys::AVRational { num, den }
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.0001,
            "expected {expected}, got {actual}"
        );
    }
}
