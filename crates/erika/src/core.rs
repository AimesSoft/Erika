use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TrySendError, bounded, unbounded};
use thiserror::Error;

use crate::audio::{AudioClockSnapshot, AudioOutputRuntimeStats};
use crate::danmaku::DanmakuRenderPlan;
use crate::ffmpeg::{DecoderBackend, Frame, PcmAudioFrame};
use crate::overlay::OverlayFrame;
use crate::playback::{PlaybackRunState, PlaybackSessionConfig, VideoPlaybackEngine};
use crate::renderer::VideoFramePayload;
use crate::subtitle::{DecodedSubtitleFrame, SubtitleTrackConfig};
use crate::trace;

static NEXT_PLAYER_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_EXTERNAL_SUBTITLE_TRACK_ID: AtomicU64 = AtomicU64::new(1);
const EXTERNAL_SUBTITLE_TRACK_ID_BASE: i64 = 1_000_000;
const AUDIO_PREFILL_AFTER_VIDEO_LIMIT: usize = 8;
const AUDIO_PREFILL_PACKET_BUDGET: usize = 6;
const AUDIO_PREFILL_TIME_BUDGET: Duration = Duration::from_millis(5);
const AUDIO_PREFILL_LOW_WATER: Duration = Duration::from_millis(350);
const AUDIO_CLOCK_SNAPSHOT_STALE_AFTER: Duration = Duration::from_millis(500);
const AUDIO_OUTPUT_BACKPRESSURE_TIMEOUT: Duration = Duration::from_secs(10);
const POSITION_EVENT_INTERVAL: Duration = Duration::from_millis(100);
const FRAME_OUTPUT_BARRIER_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PlayerError {
    #[error("player is closed")]
    Closed,
    #[error("invalid state transition from {from:?} to {to:?}")]
    InvalidStateTransition { from: PlayerState, to: PlayerState },
    #[error("renderer error: {0}")]
    Renderer(String),
    #[error("renderer backpressure: {0}")]
    RendererBackpressure(String),
    #[error("source error: {0}")]
    Source(String),
    #[error("playback error: {0}")]
    Playback(String),
}

pub type Result<T> = std::result::Result<T, PlayerError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerConfig {
    pub name: String,
    pub event_channel_capacity: usize,
    pub playback: PlaybackSessionConfig,
    pub renderer: RendererBackendPreference,
    pub video_frame_queue_capacity: usize,
    pub audio_frame_queue_capacity: usize,
    pub subtitle_frame_queue_capacity: usize,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            name: "Erika".to_string(),
            event_channel_capacity: 1024,
            playback: PlaybackSessionConfig::default(),
            renderer: RendererBackendPreference::default(),
            video_frame_queue_capacity: 3,
            audio_frame_queue_capacity: 192,
            subtitle_frame_queue_capacity: 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererBackendPreference {
    PlatformNative,
    FlutterTexture,
    WgpuFallback,
    Auto,
}

impl Default for RendererBackendPreference {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerId(NonZeroU64);

impl PlayerId {
    pub fn get(self) -> u64 {
        self.0.get()
    }
}

impl Default for PlayerId {
    fn default() -> Self {
        let id = NEXT_PLAYER_ID.fetch_add(1, Ordering::Relaxed).max(1);
        Self(NonZeroU64::new(id).expect("player id is non-zero"))
    }
}

fn next_external_subtitle_track_id() -> i64 {
    let offset = NEXT_EXTERNAL_SUBTITLE_TRACK_ID.fetch_add(1, Ordering::Relaxed);
    EXTERNAL_SUBTITLE_TRACK_ID_BASE.saturating_add(offset.min(i64::MAX as u64) as i64)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerState {
    Idle,
    Opening,
    Ready,
    Playing,
    Paused,
    Stopped,
    Closed,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaSourceHint {
    Auto,
    LocalFile,
    Http,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaRequest {
    pub uri: String,
    pub source_hint: MediaSourceHint,
}

impl MediaRequest {
    pub fn new(uri: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            source_hint: MediaSourceHint::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Video,
    Audio,
    Subtitle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackSource {
    Embedded,
    External,
}

impl Default for TrackSource {
    fn default() -> Self {
        Self::Embedded
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackInfo {
    pub id: i64,
    pub kind: TrackKind,
    pub title: Option<String>,
    pub language: Option<String>,
    pub codec: Option<String>,
    pub width: u32,
    pub height: u32,
    pub sample_rate: u32,
    pub channels: u32,
    pub pixel_format: Option<String>,
    pub sample_format: Option<String>,
    pub profile: Option<String>,
    pub level: Option<i32>,
    pub selected: bool,
    pub source: TrackSource,
    pub can_remove: bool,
}

impl TrackInfo {
    pub fn embedded(id: i64, kind: TrackKind) -> Self {
        Self {
            id,
            kind,
            title: None,
            language: None,
            codec: None,
            width: 0,
            height: 0,
            sample_rate: 0,
            channels: 0,
            pixel_format: None,
            sample_format: None,
            profile: None,
            level: None,
            selected: false,
            source: TrackSource::Embedded,
            can_remove: false,
        }
    }

    pub fn external(id: i64, kind: TrackKind) -> Self {
        Self {
            source: TrackSource::External,
            can_remove: true,
            ..Self::embedded(id, kind)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrackSelection {
    pub video: Option<i64>,
    pub audio: Option<i64>,
    pub subtitle: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorPrimaries {
    Unknown,
    Bt709,
    DisplayP3,
    Bt2020,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferFunction {
    Unknown,
    Srgb,
    Bt1886,
    Pq,
    Hlg,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoParams {
    pub width: u32,
    pub height: u32,
    pub primaries: ColorPrimaries,
    pub transfer: TransferFunction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFrameImportFailure {
    pub decode_backend: DecoderBackend,
    pub mediacodec_surface: bool,
    pub codec: Option<String>,
    pub pixel_format: Option<String>,
    pub line_sizes: [i32; 4],
    pub width: u32,
    pub height: u32,
    pub generation: u64,
    pub reason: String,
}

impl VideoFrameImportFailure {
    pub fn structured_message(&self) -> String {
        serde_json::json!({
            "event": "video_frame_import_failure",
            "backend": self.decode_backend.as_str(),
            "mediaCodecSurface": self.mediacodec_surface,
            "codec": self.codec.as_deref(),
            "pixelFormat": self.pixel_format.as_deref(),
            "lineSizes": self.line_sizes,
            "width": self.width,
            "height": self.height,
            "generation": self.generation,
            "reason": self.reason.as_str(),
        })
        .to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoDecoderEvent {
    pub stage: String,
    pub requested_backend: DecoderBackend,
    pub previous_backend: Option<DecoderBackend>,
    pub active_backend: DecoderBackend,
    pub fallback_count: u64,
    pub codec: Option<String>,
    pub pixel_format: Option<String>,
    pub line_sizes: Option<[i32; 4]>,
    pub reason: Option<String>,
}

impl VideoDecoderEvent {
    pub fn structured_message(&self) -> String {
        serde_json::json!({
            "event": "video_decoder_changed",
            "stage": self.stage.as_str(),
            "requestedBackend": self.requested_backend.as_str(),
            "previousBackend": self.previous_backend.map(DecoderBackend::as_str),
            "activeBackend": self.active_backend.as_str(),
            "fallbackCount": self.fallback_count,
            "codec": self.codec.as_deref(),
            "pixelFormat": self.pixel_format.as_deref(),
            "lineSizes": self.line_sizes,
            "reason": self.reason.as_deref(),
        })
        .to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioOutputEvent {
    pub stats: AudioOutputRuntimeStats,
}

impl AudioOutputEvent {
    pub fn structured_message(&self) -> String {
        serde_json::json!({
            "event": "audio_output_changed",
            "recoveryState": self.stats.recovery_state.as_str(),
            "lastErrorCode": self.stats.last_error_code,
            "recoveryAttempts": self.stats.recovery_attempts,
            "recoveryCount": self.stats.recovery_count,
            "recoveryFailures": self.stats.recovery_failures,
            "transitionSequence": self.stats.transition_sequence,
        })
        .to_string()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    StateChanged(PlayerState),
    DurationChanged(Option<Duration>),
    PositionChanged(Duration),
    TracksChanged(Vec<TrackInfo>),
    TrackSelectionChanged(TrackSelection),
    BufferingChanged(bool),
    VideoParamsChanged(VideoParams),
    VideoDecoderChanged(VideoDecoderEvent),
    AudioOutputChanged(AudioOutputEvent),
    SurfaceAttached(PlatformSurface),
    SurfaceDetached,
    Error(PlayerError),
}

pub struct PlayerVideoFrame {
    pub frame: VideoFramePayload,
    pub decode_backend: DecoderBackend,
    pub pts: Option<Duration>,
    pub media_time: Duration,
    pub late_by: Option<Duration>,
    pub generation: u64,
}

impl PlayerVideoFrame {
    fn from_decoded(
        frame: Frame,
        decode_backend: DecoderBackend,
        pts: Option<Duration>,
        media_time: Duration,
        late_by: Option<Duration>,
        generation: u64,
    ) -> crate::ffmpeg::Result<Self> {
        Ok(Self {
            frame: VideoFramePayload::from_decoded(frame)?,
            decode_backend,
            pts,
            media_time,
            late_by,
            generation,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerAudioFrame {
    pub frame: PcmAudioFrame,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerSubtitleFrame {
    pub frame: DecodedSubtitleFrame,
    pub pts: Option<Duration>,
    pub media_time: Duration,
    pub late_by: Option<Duration>,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct RenderFrameContext<'a> {
    pub media_time: Duration,
    pub generation: u64,
    pub overlay: Option<&'a OverlayFrame>,
    pub danmaku: Option<&'a DanmakuRenderPlan>,
    pub output_width: u32,
    pub output_height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererFrameCapture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl<'a> RenderFrameContext<'a> {
    pub fn new(media_time: Duration, generation: u64) -> Self {
        Self {
            media_time,
            generation,
            overlay: None,
            danmaku: None,
            output_width: 0,
            output_height: 0,
        }
    }

    pub fn overlay(mut self, overlay: Option<&'a OverlayFrame>) -> Self {
        self.overlay = overlay;
        self
    }

    pub fn danmaku(mut self, danmaku: Option<&'a DanmakuRenderPlan>) -> Self {
        self.danmaku = danmaku;
        self
    }

    pub fn output_size(mut self, width: u32, height: u32) -> Self {
        self.output_width = width;
        self.output_height = height;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlatformSurface {
    Metal(MetalSurfaceHandle),
    Wgpu(WgpuSurfaceHandle),
    FlutterTexture(FlutterTextureHandle),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetalSurfaceHandle {
    pub raw_layer: u64,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

impl MetalSurfaceHandle {
    pub fn new(raw_layer: u64, width: u32, height: u32, scale: f64) -> Self {
        Self {
            raw_layer,
            width,
            height,
            scale,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WgpuSurfaceKind {
    Unknown,
    MacOsNsView,
    MacOsCaMetalLayer,
    IosUiView,
    WindowsHwnd,
    XlibWindow,
    WaylandSurface,
    AndroidNativeWindow,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WgpuSurfaceHandle {
    pub kind: WgpuSurfaceKind,
    pub raw_window: u64,
    pub raw_display: u64,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub output_capabilities: SurfaceOutputCapabilities,
}

impl WgpuSurfaceHandle {
    pub fn new(
        kind: WgpuSurfaceKind,
        raw_window: u64,
        raw_display: u64,
        width: u32,
        height: u32,
        scale: f64,
    ) -> Self {
        Self {
            kind,
            raw_window,
            raw_display,
            width,
            height,
            scale,
            output_capabilities: SurfaceOutputCapabilities::default(),
        }
    }

    pub fn with_output_capabilities(
        mut self,
        output_capabilities: SurfaceOutputCapabilities,
    ) -> Self {
        self.output_capabilities = output_capabilities;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceOutputCapabilities {
    /// The display/window host is eligible for an extended-linear signal.
    pub extended_linear: bool,
    /// The native surface bypasses Flutter texture-layer composition (for
    /// Android this means a SurfaceView hosted with Hybrid Composition).
    pub direct_composition: bool,
    /// Requested display headroom ratio relative to SDR reference white.
    pub desired_headroom: f32,
    /// Host-side reason why extended-linear eligibility is unavailable. This
    /// keeps display/API failures queryable after the renderer falls back.
    pub fallback_reason: crate::renderer::output::OutputFallbackReason,
}

impl Default for SurfaceOutputCapabilities {
    fn default() -> Self {
        Self {
            extended_linear: false,
            direct_composition: false,
            desired_headroom: 0.0,
            fallback_reason: crate::renderer::output::OutputFallbackReason::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlutterTextureKind {
    Unknown,
    MacOsTextureRegistrar,
    IosTextureRegistrar,
    AndroidSurfaceTexture,
    WindowsTextureRegistrar,
    LinuxTextureRegistrar,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlutterTextureHandle {
    pub kind: FlutterTextureKind,
    pub texture_id: i64,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub hdr_capable: bool,
}

impl FlutterTextureHandle {
    pub fn new(
        kind: FlutterTextureKind,
        texture_id: i64,
        width: u32,
        height: u32,
        scale: f64,
    ) -> Self {
        Self {
            kind,
            texture_id,
            width,
            height,
            scale,
            hdr_capable: false,
        }
    }
}

pub trait RendererBackend {
    fn attach_surface(&mut self, surface: PlatformSurface) -> Result<()>;
    fn detach_surface(&mut self) -> Result<()>;
    fn resize_surface(&mut self, width: u32, height: u32, scale: f64) -> Result<()>;
    fn render_test_frame(&mut self, time_seconds: f64) -> Result<()>;

    /// Import/upload a freshly decoded video frame and retain it as the current
    /// frame to display. The backend owns the imported representation (Metal
    /// textures, wgpu textures, ...) so the presenter stays backend-agnostic.
    fn upload_player_frame(&mut self, frame: &PlayerVideoFrame) -> Result<()>;

    /// Drop any retained video frame and clear the attached output surface when
    /// the backend can do so.
    fn clear_current_frame(&mut self) -> Result<()> {
        Ok(())
    }

    /// Release any decoder-owned recovery payload before a decoder transition
    /// while preserving a renderer-owned GPU snapshot when the backend can do
    /// so safely. Backends without a detached snapshot fall back to a full
    /// clear.
    fn preserve_current_frame_for_transition(&mut self) -> Result<()> {
        self.clear_current_frame()
    }

    /// Render the current frame (optionally compositing `overlay`) to the attached
    /// surface. Returns `false` if there is no current frame to draw, letting the
    /// caller fall back to a test frame.
    fn render_current_frame(&mut self, context: RenderFrameContext<'_>) -> Result<bool>;

    /// Render the retained current frame into an offscreen RGBA buffer.
    fn capture_current_frame(
        &mut self,
        _context: RenderFrameContext<'_>,
        _width: u32,
        _height: u32,
    ) -> Result<Option<RendererFrameCapture>> {
        Ok(None)
    }

    fn runtime_stats(&self) -> RendererRuntimeStats {
        RendererRuntimeStats::default()
    }

    fn output_status(&self) -> crate::renderer::output::OutputRuntimeStatus {
        crate::renderer::output::OutputRuntimeStatus::default()
    }

    /// Whether this renderer can consume MediaCodec Surface/AHardwareBuffer
    /// frames without a CPU readback. Android uses this before opening the
    /// decoder so unsupported Vulkan implementations start in ByteBuffer mode.
    fn supports_mediacodec_surface_frames(&self) -> bool {
        false
    }

    /// Switches the neural luma upscaler at runtime. Backends without an
    /// upscaler implementation ignore the request.
    fn set_luma_upscaler(&mut self, _mode: crate::renderer::pipeline::LumaUpscalerMode) {}

    /// Publishes the display's current HDR/SDR headroom ratio. Android uses
    /// this for queryable runtime status; renderers that do not expose dynamic
    /// display headroom may ignore the update.
    fn set_output_headroom(&mut self, _headroom: f32, _known: bool) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LumaUpscalerBackendStatus {
    #[default]
    Off,
    Inactive,
    Building,
    Scalar,
    SimdgroupMatrix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RendererRuntimeStats {
    pub surface_width: u32,
    pub surface_height: u32,
    pub rendered_frames: u64,
    pub offscreen_frames: u64,
    pub prepared_overlay_frames: u64,
    pub prepared_overlay_subtitle_planes: u64,
    pub danmaku_passes: u64,
    pub danmaku_draw_items: u64,
    pub overlay_alpha_atlas_uploads: u64,
    pub overlay_alpha_atlas_reuses: u64,
    pub last_danmaku_atlas_duration: Duration,
    pub last_danmaku_vertex_build_duration: Duration,
    pub last_danmaku_vertex_copy_duration: Duration,
    pub last_danmaku_encode_duration: Duration,
    pub last_danmaku_vertex_bytes: usize,
    pub last_danmaku_vertex_count: usize,
    pub upscaler_mode: crate::renderer::pipeline::LumaUpscalerMode,
    pub upscaler_backend: LumaUpscalerBackendStatus,
    pub upscaler_fallbacks: u64,
    pub upscaled_frames: u64,
    pub last_upscaler_encode_duration: Duration,
    pub last_gpu_duration: Duration,
    pub attached: bool,
    pub software_video_frames: u64,
    pub hardware_video_frames: u64,
    pub zero_copy_video_frames: u64,
    pub direct_zero_copy_video_frames: u64,
    pub shared_handle_video_frames: u64,
    pub cpu_video_frame_fallbacks: u64,
    pub hdr_source_frames: u64,
    pub hdr10_output_frames: u64,
    pub sdr_tonemap_frames: u64,
    pub hdr10_metadata_updates: u64,
    pub hdr10_metadata_failures: u64,
    pub hdr10_output_failures: u64,
    pub hdr10_output_active: bool,
}

struct PlayerInner {
    state: PlayerState,
    ended: bool,
    media: Option<MediaRequest>,
    duration: Option<Duration>,
    current_media_time: Duration,
    playback_generation: u64,
    playback_command_sequence: u64,
    surface: Option<PlatformSurface>,
    tracks: Vec<TrackInfo>,
    track_selection: TrackSelection,
    subscribers: Vec<Sender<PlayerEvent>>,
    video_frame_sender: Option<Sender<PlayerVideoFrame>>,
    audio_frame_sender: Option<Sender<PlayerAudioFrame>>,
    subtitle_frame_sender: Option<Sender<PlayerSubtitleFrame>>,
}

struct PlayerLifecycle {
    epoch: u64,
    playback: Option<PlaybackRuntime>,
}

enum PlaybackCommand {
    Play {
        sequence: u64,
        generation: u64,
        reply: Sender<std::result::Result<(), String>>,
    },
    Pause {
        sequence: u64,
    },
    Seek {
        position: Duration,
        sequence: u64,
        generation: u64,
        resume_after_seek: bool,
    },
    SetPlaybackRate(f64),
    Stop {
        sequence: u64,
        generation: u64,
    },
    AudioClock(AudioClockSnapshot),
    VideoFrameImportFailed(VideoFrameImportFailure),
    AddExternalSubtitle {
        config: SubtitleTrackConfig,
        reply: Sender<std::result::Result<SubtitleTrackConfig, String>>,
    },
    RemoveSubtitleTrack(i64),
    SelectAudioTrack(Option<i64>),
    SelectSubtitleTrack(Option<i64>),
    SetFrameOutputQuiesced {
        quiesced: bool,
        reply: Sender<()>,
    },
    Shutdown,
}

struct PlaybackRuntime {
    commands: Sender<PlaybackCommand>,
    worker: Option<JoinHandle<()>>,
}

impl PlaybackRuntime {
    fn spawn(
        mut engine: VideoPlaybackEngine,
        inner: Arc<Mutex<PlayerInner>>,
        capacity: usize,
        initial_generation: u64,
    ) -> Self {
        let (commands, receiver) = bounded(capacity.max(1));
        let worker = thread::Builder::new()
            .name("erika-playback".to_string())
            .spawn(move || run_playback_worker(&mut engine, inner, receiver, initial_generation))
            .expect("spawn playback worker");
        Self {
            commands,
            worker: Some(worker),
        }
    }

    fn shutdown(&mut self) {
        if self.commands.send(PlaybackCommand::Shutdown).is_err() {
            trace::diagnostic(
                serde_json::json!({
                    "event": "playback_worker_shutdown",
                    "stage": "command_disconnected",
                    "reason": "the playback worker stopped before accepting Shutdown",
                })
                .to_string(),
            );
        }
        if let Some(worker) = self.worker.take() {
            if let Err(payload) = worker.join() {
                let reason = payload
                    .downcast_ref::<&str>()
                    .map(|message| (*message).to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "unknown panic payload".to_string());
                trace::diagnostic(
                    serde_json::json!({
                        "event": "playback_worker_shutdown",
                        "stage": "join_panicked",
                        "reason": reason,
                    })
                    .to_string(),
                );
            }
        }
    }
}

impl Drop for PlaybackRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Clone)]
pub struct Player {
    id: PlayerId,
    config: PlayerConfig,
    inner: Arc<Mutex<PlayerInner>>,
    lifecycle: Arc<Mutex<PlayerLifecycle>>,
}

impl Player {
    pub fn new(config: PlayerConfig) -> Self {
        Self {
            id: PlayerId::default(),
            config,
            inner: Arc::new(Mutex::new(PlayerInner {
                state: PlayerState::Idle,
                ended: false,
                media: None,
                duration: None,
                current_media_time: Duration::ZERO,
                playback_generation: 1,
                playback_command_sequence: 0,
                surface: None,
                tracks: Vec::new(),
                track_selection: TrackSelection::default(),
                subscribers: Vec::new(),
                video_frame_sender: None,
                audio_frame_sender: None,
                subtitle_frame_sender: None,
            })),
            lifecycle: Arc::new(Mutex::new(PlayerLifecycle {
                epoch: 1,
                playback: None,
            })),
        }
    }

    pub fn id(&self) -> PlayerId {
        self.id
    }

    pub fn config(&self) -> &PlayerConfig {
        &self.config
    }

    pub fn state(&self) -> PlayerState {
        self.inner.lock().expect("player mutex poisoned").state
    }

    pub(crate) fn is_stopped_at_end(&self) -> bool {
        let inner = self.inner.lock().expect("player mutex poisoned");
        inner.state == PlayerState::Stopped && inner.ended
    }

    #[cfg(test)]
    fn is_ended(&self) -> bool {
        self.inner.lock().expect("player mutex poisoned").ended
    }

    pub fn current_media_time(&self) -> Duration {
        self.inner
            .lock()
            .expect("player mutex poisoned")
            .current_media_time
    }

    pub fn duration(&self) -> Option<Duration> {
        self.inner.lock().expect("player mutex poisoned").duration
    }

    pub fn playback_generation(&self) -> u64 {
        self.inner
            .lock()
            .expect("player mutex poisoned")
            .playback_generation
            .max(1)
    }

    pub fn subscribe(&self) -> Receiver<PlayerEvent> {
        let (sender, receiver) = unbounded();
        self.inner
            .lock()
            .expect("player mutex poisoned")
            .subscribers
            .push(sender);
        receiver
    }

    pub fn subscribe_video_frames(&self) -> Receiver<PlayerVideoFrame> {
        let capacity = self.config.video_frame_queue_capacity.max(1);
        let (sender, receiver) = bounded(capacity);
        self.inner
            .lock()
            .expect("player mutex poisoned")
            .video_frame_sender = Some(sender);
        receiver
    }

    pub fn subscribe_audio_frames(&self) -> Receiver<PlayerAudioFrame> {
        let capacity = self.config.audio_frame_queue_capacity.max(1);
        let (sender, receiver) = bounded(capacity);
        self.inner
            .lock()
            .expect("player mutex poisoned")
            .audio_frame_sender = Some(sender);
        receiver
    }

    pub fn subscribe_subtitle_frames(&self) -> Receiver<PlayerSubtitleFrame> {
        // Subtitle frames are event data: dropping one can permanently remove an
        // ASS chunk (style animation, sign, or overlapping line). Playback itself
        // is paced and bounded upstream, so keep this handoff lossless.
        let (sender, receiver) = unbounded();
        self.inner
            .lock()
            .expect("player mutex poisoned")
            .subtitle_frame_sender = Some(sender);
        receiver
    }

    pub fn open(&self, media: MediaRequest) -> Result<()> {
        let epoch = {
            let mut lifecycle = self
                .lifecycle
                .lock()
                .expect("player lifecycle mutex poisoned");
            self.ensure_not_closed()?;
            lifecycle.epoch = lifecycle.epoch.saturating_add(1).max(1);
            let epoch = lifecycle.epoch;
            let previous = lifecycle.playback.take();
            // Join the old producer before publishing Opening. The lifecycle
            // lock serializes open/close while the worker is retired, and the
            // worker never needs this lock to finish.
            drop(previous);
            self.transition(PlayerState::Opening)?;
            epoch
        };
        let mut engine = match VideoPlaybackEngine::open(&media, self.config.playback) {
            Ok(engine) => engine,
            Err(error) => {
                let error = PlayerError::Playback(error.to_string());
                let lifecycle = self
                    .lifecycle
                    .lock()
                    .expect("player lifecycle mutex poisoned");
                if lifecycle.epoch != epoch {
                    return Err(self.superseded_open_error());
                }
                self.transition(PlayerState::Error)?;
                self.emit(PlayerEvent::Error(error.clone()));
                return Err(error);
            }
        };
        let info = engine.info().clone();
        let decoder_events = engine.take_video_decoder_events();
        let mut lifecycle = self
            .lifecycle
            .lock()
            .expect("player lifecycle mutex poisoned");
        if lifecycle.epoch != epoch {
            return Err(self.superseded_open_error());
        }
        let generation = {
            let mut inner = self.inner.lock().expect("player mutex poisoned");
            if inner.state == PlayerState::Closed {
                return Err(PlayerError::Closed);
            }
            let generation = inner.playback_generation.saturating_add(1).max(1);
            inner.media = Some(media);
            inner.duration = info.duration;
            inner.current_media_time = Duration::ZERO;
            inner.playback_generation = generation;
            inner.playback_command_sequence = 0;
            inner.ended = false;
            inner.tracks = info.tracks.clone();
            inner.track_selection = info.track_selection();
            generation
        };
        lifecycle.playback = Some(PlaybackRuntime::spawn(
            engine,
            Arc::clone(&self.inner),
            self.config.event_channel_capacity,
            generation,
        ));
        self.emit(PlayerEvent::DurationChanged(info.duration));
        self.emit(PlayerEvent::TracksChanged(info.tracks.clone()));
        self.emit(PlayerEvent::TrackSelectionChanged(info.track_selection()));
        if let Some(params) = info.video_params {
            self.emit(PlayerEvent::VideoParamsChanged(params));
        }
        for event in decoder_events {
            self.emit(PlayerEvent::VideoDecoderChanged(event));
        }
        self.transition(PlayerState::Ready)?;
        Ok(())
    }

    pub fn play(&self) -> Result<()> {
        self.ensure_not_closed()?;
        let commands = self.playback_commands()?;
        let (sequence, generation) = {
            let mut inner = self.inner.lock().expect("player mutex poisoned");
            match inner.state {
                PlayerState::Ready | PlayerState::Paused | PlayerState::Stopped => {}
                from => {
                    return Err(PlayerError::InvalidStateTransition {
                        from,
                        to: PlayerState::Playing,
                    });
                }
            }
            inner.playback_command_sequence =
                inner.playback_command_sequence.saturating_add(1).max(1);
            (
                inner.playback_command_sequence,
                inner.playback_generation.max(1),
            )
        };
        let (reply, response) = bounded(1);
        if commands
            .send(PlaybackCommand::Play {
                sequence,
                generation,
                reply,
            })
            .is_err()
        {
            return Err(PlayerError::Playback(
                "playback worker is not running".to_string(),
            ));
        }
        response
            .recv()
            .map_err(|_| {
                PlayerError::Playback(
                    "playback worker stopped before acknowledging play".to_string(),
                )
            })?
            .map_err(PlayerError::Playback)
    }

    pub fn pause(&self) -> Result<()> {
        self.ensure_not_closed()?;
        let commands = self.playback_commands()?;
        let sequence = {
            let mut inner = self.inner.lock().expect("player mutex poisoned");
            if inner.state != PlayerState::Playing {
                return Err(PlayerError::InvalidStateTransition {
                    from: inner.state,
                    to: PlayerState::Paused,
                });
            }
            inner.playback_command_sequence =
                inner.playback_command_sequence.saturating_add(1).max(1);
            inner.playback_command_sequence
        };
        commands
            .send(PlaybackCommand::Pause { sequence })
            .map_err(|_| PlayerError::Playback("playback worker is not running".to_string()))?;
        let _ = commit_playback_command_intent(
            &self.inner,
            sequence,
            None,
            None,
            Some(PlayerState::Paused),
        );
        Ok(())
    }

    pub fn seek(&self, position: Duration) -> Result<()> {
        self.ensure_not_closed()?;
        let commands = self.playback_commands()?;
        let (
            previous_media_time,
            previous_generation,
            previous_ended,
            sequence,
            generation,
            resume_after_seek,
        ) = {
            let mut inner = self.inner.lock().expect("player mutex poisoned");
            let previous_media_time = inner.current_media_time;
            let previous_generation = inner.playback_generation;
            let previous_ended = inner.ended;
            let resume_after_seek = inner.state == PlayerState::Playing;
            inner.playback_command_sequence =
                inner.playback_command_sequence.saturating_add(1).max(1);
            inner.current_media_time = position;
            inner.playback_generation = inner.playback_generation.saturating_add(1).max(1);
            inner.ended = false;
            (
                previous_media_time,
                previous_generation,
                previous_ended,
                inner.playback_command_sequence,
                inner.playback_generation,
                resume_after_seek,
            )
        };
        if commands
            .send(PlaybackCommand::Seek {
                position,
                sequence,
                generation,
                resume_after_seek,
            })
            .is_err()
        {
            let mut inner = self.inner.lock().expect("player mutex poisoned");
            if inner.playback_command_sequence == sequence {
                inner.current_media_time = previous_media_time;
                inner.playback_generation = previous_generation;
                inner.ended = previous_ended;
            }
            return Err(PlayerError::Playback(
                "playback worker is not running".to_string(),
            ));
        }
        let _ = commit_playback_command_intent(&self.inner, sequence, None, Some(position), None);
        Ok(())
    }

    pub fn set_playback_rate(&self, rate: f64) -> Result<()> {
        self.ensure_not_closed()?;
        self.send_playback_command(PlaybackCommand::SetPlaybackRate(rate))
    }

    pub fn add_external_subtitle(&self, uri: impl Into<String>) -> Result<SubtitleTrackConfig> {
        self.ensure_not_closed()?;
        let uri = uri.into();
        let id = next_external_subtitle_track_id();
        let config = SubtitleTrackConfig::external(id, uri);
        let (reply, response) = bounded(1);
        self.send_playback_command(PlaybackCommand::AddExternalSubtitle { config, reply })?;
        response
            .recv()
            .map_err(|_| {
                PlayerError::Playback(
                    "playback worker stopped before opening the external subtitle".to_string(),
                )
            })?
            .map_err(PlayerError::Playback)
    }

    pub fn remove_subtitle_track(&self, track_id: i64) -> Result<()> {
        self.ensure_not_closed()?;
        self.send_playback_command(PlaybackCommand::RemoveSubtitleTrack(track_id))
    }

    pub fn select_audio_track(&self, track_id: Option<i64>) -> Result<()> {
        self.ensure_not_closed()?;
        self.send_playback_command(PlaybackCommand::SelectAudioTrack(track_id))
    }

    pub fn select_subtitle_track(&self, track_id: Option<i64>) -> Result<()> {
        self.ensure_not_closed()?;
        self.send_playback_command(PlaybackCommand::SelectSubtitleTrack(track_id))
    }

    pub fn tracks(&self) -> Vec<TrackInfo> {
        self.inner
            .lock()
            .expect("player mutex poisoned")
            .tracks
            .clone()
    }

    pub fn track_selection(&self) -> TrackSelection {
        self.inner
            .lock()
            .expect("player mutex poisoned")
            .track_selection
    }

    pub fn stop(&self) -> Result<()> {
        self.ensure_not_closed()?;
        let commands = self.playback_commands()?;
        let (previous_media_time, previous_generation, previous_ended, sequence, generation) = {
            let mut inner = self.inner.lock().expect("player mutex poisoned");
            let previous_media_time = inner.current_media_time;
            let previous_generation = inner.playback_generation;
            let previous_ended = inner.ended;
            inner.playback_command_sequence =
                inner.playback_command_sequence.saturating_add(1).max(1);
            inner.current_media_time = Duration::ZERO;
            inner.playback_generation = inner.playback_generation.saturating_add(1).max(1);
            inner.ended = false;
            (
                previous_media_time,
                previous_generation,
                previous_ended,
                inner.playback_command_sequence,
                inner.playback_generation,
            )
        };
        if commands
            .send(PlaybackCommand::Stop {
                sequence,
                generation,
            })
            .is_err()
        {
            let mut inner = self.inner.lock().expect("player mutex poisoned");
            if inner.playback_command_sequence == sequence {
                inner.current_media_time = previous_media_time;
                inner.playback_generation = previous_generation;
                inner.ended = previous_ended;
            }
            return Err(PlayerError::Playback(
                "playback worker is not running".to_string(),
            ));
        }
        let _ = commit_playback_command_intent(
            &self.inner,
            sequence,
            None,
            Some(Duration::ZERO),
            Some(PlayerState::Stopped),
        );
        Ok(())
    }

    pub fn update_audio_clock(&self, snapshot: AudioClockSnapshot) -> Result<()> {
        self.ensure_not_closed()?;
        let commands = self.playback_commands()?;
        match commands.try_send(PlaybackCommand::AudioClock(snapshot)) {
            Ok(()) | Err(TrySendError::Full(_)) => Ok(()),
            Err(TrySendError::Disconnected(_)) => Err(PlayerError::Playback(
                "playback worker is not running".to_string(),
            )),
        }
    }

    pub fn report_video_frame_import_failure(
        &self,
        failure: VideoFrameImportFailure,
    ) -> Result<()> {
        self.ensure_not_closed()?;
        self.send_playback_command(PlaybackCommand::VideoFrameImportFailed(failure))
    }

    pub(crate) fn set_frame_output_quiesced(&self, quiesced: bool) -> Result<bool> {
        self.set_frame_output_quiesced_with_timeout(quiesced, FRAME_OUTPUT_BARRIER_TIMEOUT)
    }

    fn set_frame_output_quiesced_with_timeout(
        &self,
        quiesced: bool,
        timeout: Duration,
    ) -> Result<bool> {
        let Some(commands) = self.optional_playback_commands() else {
            return Ok(false);
        };
        let (reply, response) = bounded(1);
        let requested_at = Instant::now();
        let action = if quiesced { "quiesce" } else { "resume" };
        trace::diagnostic(
            serde_json::json!({
                "event": "player_frame_output",
                "stage": "request",
                "action": action,
                "timeoutMs": timeout.as_millis(),
                "generation": self.playback_generation(),
            })
            .to_string(),
        );
        match commands.send_timeout(
            PlaybackCommand::SetFrameOutputQuiesced { quiesced, reply },
            timeout,
        ) {
            Ok(()) => {}
            Err(crossbeam_channel::SendTimeoutError::Timeout(_)) => {
                let message = format!(
                    "timed out after {} ms while sending frame output {action} to the playback worker",
                    timeout.as_millis(),
                );
                trace::diagnostic(
                    serde_json::json!({
                        "event": "player_frame_output",
                        "stage": "command_timeout",
                        "action": action,
                        "timeoutMs": timeout.as_millis(),
                        "elapsedMs": requested_at.elapsed().as_millis(),
                        "reason": message.as_str(),
                    })
                    .to_string(),
                );
                return Err(PlayerError::Playback(message));
            }
            Err(crossbeam_channel::SendTimeoutError::Disconnected(_)) => {
                return Err(PlayerError::Playback(
                    "playback worker is not running".to_string(),
                ));
            }
        }
        let remaining = timeout.saturating_sub(requested_at.elapsed());
        match response.recv_timeout(remaining) {
            Ok(()) => {
                trace::diagnostic(
                    serde_json::json!({
                        "event": "player_frame_output",
                        "stage": "acknowledged",
                        "action": action,
                        "elapsedMs": requested_at.elapsed().as_millis(),
                        "generation": self.playback_generation(),
                    })
                    .to_string(),
                );
            }
            Err(RecvTimeoutError::Timeout) => {
                let message = format!(
                    "playback worker did not acknowledge frame output {action} within {} ms",
                    timeout.as_millis(),
                );
                trace::diagnostic(
                    serde_json::json!({
                        "event": "player_frame_output",
                        "stage": "ack_timeout",
                        "action": action,
                        "timeoutMs": timeout.as_millis(),
                        "elapsedMs": requested_at.elapsed().as_millis(),
                        "reason": message.as_str(),
                    })
                    .to_string(),
                );
                if quiesced {
                    enqueue_frame_output_resume_after_timeout(&commands);
                }
                return Err(PlayerError::Playback(message));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(PlayerError::Playback(format!(
                    "playback worker stopped before acknowledging frame output {action}",
                )));
            }
        }
        Ok(true)
    }

    pub(crate) fn report_audio_output_event(&self, event: AudioOutputEvent) {
        self.emit(PlayerEvent::AudioOutputChanged(event));
    }

    pub fn close(&self) -> Result<()> {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .expect("player lifecycle mutex poisoned");
        lifecycle.epoch = lifecycle.epoch.saturating_add(1).max(1);
        let previous = lifecycle.playback.take();
        // Stop and join before publishing Closed so no worker event can appear
        // after the terminal lifecycle event.
        drop(previous);
        {
            let mut inner = self.inner.lock().expect("player mutex poisoned");
            inner.media = None;
            inner.tracks.clear();
            inner.track_selection = TrackSelection::default();
            inner.duration = None;
            inner.current_media_time = Duration::ZERO;
            inner.playback_generation = inner.playback_generation.saturating_add(1).max(1);
            inner.ended = false;
        }
        self.transition(PlayerState::Closed)
    }

    pub fn attach_surface(&self, surface: PlatformSurface) -> Result<()> {
        self.ensure_not_closed()?;
        {
            let mut inner = self.inner.lock().expect("player mutex poisoned");
            inner.surface = Some(surface);
        }
        self.emit(PlayerEvent::SurfaceAttached(surface));
        Ok(())
    }

    pub fn detach_surface(&self) -> Result<()> {
        self.ensure_not_closed()?;
        {
            let mut inner = self.inner.lock().expect("player mutex poisoned");
            inner.surface = None;
        }
        self.emit(PlayerEvent::SurfaceDetached);
        Ok(())
    }

    fn ensure_not_closed(&self) -> Result<()> {
        if self.state() == PlayerState::Closed {
            Err(PlayerError::Closed)
        } else {
            Ok(())
        }
    }

    fn send_playback_command(&self, command: PlaybackCommand) -> Result<()> {
        let commands = self.playback_commands()?;
        commands
            .send(command)
            .map_err(|_| PlayerError::Playback("playback worker is not running".to_string()))
    }

    fn playback_commands(&self) -> Result<Sender<PlaybackCommand>> {
        let lifecycle = self
            .lifecycle
            .lock()
            .expect("player lifecycle mutex poisoned");
        Ok(lifecycle
            .playback
            .as_ref()
            .ok_or_else(|| PlayerError::Playback("no media is open".to_string()))?
            .commands
            .clone())
    }

    fn optional_playback_commands(&self) -> Option<Sender<PlaybackCommand>> {
        self.lifecycle
            .lock()
            .expect("player lifecycle mutex poisoned")
            .playback
            .as_ref()
            .map(|runtime| runtime.commands.clone())
    }

    fn superseded_open_error(&self) -> PlayerError {
        if self.state() == PlayerState::Closed {
            PlayerError::Closed
        } else {
            PlayerError::Playback("media open was superseded by a newer lifecycle operation".into())
        }
    }

    fn transition(&self, next: PlayerState) -> Result<()> {
        let previous = {
            let mut inner = self.inner.lock().expect("player mutex poisoned");
            let previous = inner.state;
            inner.state = next;
            previous
        };
        if previous != next {
            self.emit(PlayerEvent::StateChanged(next));
        }
        Ok(())
    }

    fn emit(&self, event: PlayerEvent) {
        let mut inner = self.inner.lock().expect("player mutex poisoned");
        inner
            .subscribers
            .retain(|sender| sender.send(event.clone()).is_ok());
    }
}

fn run_playback_worker(
    engine: &mut VideoPlaybackEngine,
    inner: Arc<Mutex<PlayerInner>>,
    commands: Receiver<PlaybackCommand>,
    initial_generation: u64,
) {
    let worker_started = std::time::Instant::now();
    let mut last_position_event = None;
    let mut last_position_emit = Instant::now()
        .checked_sub(POSITION_EVENT_INTERVAL)
        .unwrap_or_else(Instant::now);
    let mut playback_generation = initial_generation.max(1);
    let mut frame_output_quiesced = false;
    let mut last_executed_playback_command_sequence = 0u64;
    let mut last_worker_clock = None;
    let mut last_audio_snapshot = None;
    let mut last_audio_snapshot_at = None;
    let mut audio_output_backpressure = AudioOutputBackpressureState::default();
    let mut eof_published = false;
    let mut loop_count = 0u64;
    trace::log(format!(
        "[erika-playback-trace] stage=worker_start state={:?} generation={} poll_ms={} uptime_ms={:.3}",
        engine.state(),
        playback_generation,
        playback_poll_interval(engine.state()).as_millis(),
        worker_started.elapsed().as_secs_f64() * 1000.0,
    ));
    loop {
        loop_count = loop_count.saturating_add(1);
        let loop_started = std::time::Instant::now();
        let mut command_count = 0usize;
        match commands.recv_timeout(playback_poll_interval(engine.state())) {
            Ok(command) => {
                command_count += 1;
                observe_audio_pump_command(
                    engine.state(),
                    &mut last_audio_snapshot,
                    &mut last_audio_snapshot_at,
                    &command,
                );
                if !handle_playback_command(
                    engine,
                    &inner,
                    command,
                    &mut playback_generation,
                    &mut last_executed_playback_command_sequence,
                    &mut frame_output_quiesced,
                ) {
                    return;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }

        while let Ok(command) = commands.try_recv() {
            command_count += 1;
            observe_audio_pump_command(
                engine.state(),
                &mut last_audio_snapshot,
                &mut last_audio_snapshot_at,
                &command,
            );
            if !handle_playback_command(
                engine,
                &inner,
                command,
                &mut playback_generation,
                &mut last_executed_playback_command_sequence,
                &mut frame_output_quiesced,
            ) {
                return;
            }
        }
        if engine.state() != PlaybackRunState::Ended {
            eof_published = false;
        }
        let after_commands = std::time::Instant::now();

        engine.set_audio_output_active(audio_frame_output_is_active(&inner));
        if engine.state() != PlaybackRunState::Playing || frame_output_quiesced {
            audio_output_backpressure.reset();
        }

        sync_playback_clock_from_worker(
            engine,
            &inner,
            playback_generation,
            "before_video_tick",
            &mut last_worker_clock,
        );
        let after_clock_sync = std::time::Instant::now();

        if !frame_output_quiesced {
            match engine.tick() {
                Ok(Some(frame)) => {
                    let position = frame.pts.unwrap_or(frame.media_time);
                    last_position_event = Some((playback_generation, position));
                    trace::log(format!(
                        "[erika-playback-trace] stage=video_frame gen={} pts={} media={} late={} loop={} commands={} state={:?}",
                        playback_generation,
                        trace::duration_label(frame.pts),
                        trace::duration_label(Some(frame.media_time)),
                        frame
                            .late_by
                            .map(|duration| format!("{:.3}", duration.as_secs_f64()))
                            .unwrap_or_else(|| "-".to_string()),
                        loop_count,
                        command_count,
                        engine.state(),
                    ));
                    match PlayerVideoFrame::from_decoded(
                        frame.frame,
                        frame.decode_backend,
                        frame.pts,
                        frame.media_time,
                        frame.late_by,
                        playback_generation,
                    ) {
                        Ok(frame) => emit_video_frame_from_worker(&inner, frame),
                        Err(error) => fail_playback_from_worker(
                            engine,
                            &inner,
                            "video_frame_handoff",
                            error.to_string(),
                        ),
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    trace::log(format!(
                        "[erika-playback-trace] stage=video_tick_error gen={} loop={} commands={} error={}",
                        playback_generation, loop_count, command_count, error,
                    ));
                    fail_playback_from_worker(engine, &inner, "video_tick", error.to_string());
                }
            }
        }

        sync_playback_clock_from_worker(
            engine,
            &inner,
            playback_generation,
            "after_video_tick",
            &mut last_worker_clock,
        );
        let after_video = std::time::Instant::now();

        if !frame_output_quiesced && engine.state() == PlaybackRunState::Playing {
            if should_prefill_audio_from_worker(engine, last_audio_snapshot, last_audio_snapshot_at)
            {
                pump_audio_from_worker(
                    engine,
                    &inner,
                    playback_generation,
                    "after_video_tick",
                    AUDIO_PREFILL_AFTER_VIDEO_LIMIT,
                    &mut audio_output_backpressure,
                );
            }

            match engine.tick_subtitle() {
                Ok(Some(frame)) => emit_subtitle_frame_from_worker(
                    &inner,
                    PlayerSubtitleFrame {
                        frame: frame.frame,
                        pts: frame.pts,
                        media_time: frame.media_time,
                        late_by: frame.late_by,
                        generation: playback_generation,
                    },
                ),
                Ok(None) => {}
                Err(error) => {
                    fail_playback_from_worker(engine, &inner, "subtitle_tick", error.to_string());
                }
            }
        }

        sync_playback_clock_from_worker(
            engine,
            &inner,
            playback_generation,
            "after_av_tick",
            &mut last_worker_clock,
        );
        let after_av = std::time::Instant::now();

        emit_video_decoder_events_from_worker(engine, &inner);

        if last_position_event.is_some_and(|(generation, _)| generation != playback_generation) {
            last_position_event = None;
        }
        if publish_natural_eof_from_worker(engine, &inner, playback_generation, &mut eof_published)
        {
            last_position_event = None;
        } else if last_position_event.is_some()
            && (last_position_emit.elapsed() >= POSITION_EVENT_INTERVAL
                || engine.state() != PlaybackRunState::Playing)
        {
            let (generation, position) =
                last_position_event.take().expect("position event present");
            if !emit_from_worker_for_generation(
                &inner,
                generation,
                PlayerEvent::PositionChanged(position),
            ) {
                trace::log(format!(
                    "[erika-clock-trace] stage=worker_position_skip_stale position={} gen={} shared_gen={}",
                    trace::duration_label(Some(position)),
                    generation,
                    shared_playback_generation(&inner),
                ));
            }
            last_position_emit = Instant::now();
        }

        let loop_duration = loop_started.elapsed();
        if trace::enabled() && (loop_duration > Duration::from_millis(4) || command_count > 0) {
            trace::log(format!(
                "[erika-playback-trace] stage=worker_loop gen={} loop={} commands={} total_ms={:.3} commands_ms={:.3} clock_ms={:.3} video_ms={:.3} av_ms={:.3} state={:?}",
                playback_generation,
                loop_count,
                command_count,
                loop_duration.as_secs_f64() * 1000.0,
                after_commands.duration_since(loop_started).as_secs_f64() * 1000.0,
                after_clock_sync
                    .duration_since(after_commands)
                    .as_secs_f64()
                    * 1000.0,
                after_video.duration_since(after_clock_sync).as_secs_f64() * 1000.0,
                after_av.duration_since(after_video).as_secs_f64() * 1000.0,
                engine.state(),
            ));
        }
    }
}

fn emit_video_decoder_events_from_worker(
    engine: &mut VideoPlaybackEngine,
    inner: &Arc<Mutex<PlayerInner>>,
) {
    for event in engine.take_video_decoder_events() {
        emit_from_worker(inner, PlayerEvent::VideoDecoderChanged(event));
    }
}

#[derive(Default)]
struct AudioOutputBackpressureState {
    started_at: Option<Instant>,
    polls: u64,
    pending_logged: bool,
}

impl AudioOutputBackpressureState {
    fn observe(&mut self, inner: &Arc<Mutex<PlayerInner>>) -> Option<String> {
        let now = Instant::now();
        let started_at = *self.started_at.get_or_insert(now);
        self.polls = self.polls.saturating_add(1);
        let (queued_frames, capacity) = audio_frame_channel_metrics(inner);
        if !self.pending_logged {
            self.pending_logged = true;
            trace::diagnostic(
                serde_json::json!({
                    "event": "playback_audio_output",
                    "stage": "backpressure_pending",
                    "polls": self.polls,
                    "queuedFrames": queued_frames,
                    "capacityFrames": capacity,
                    "timeoutSeconds": AUDIO_OUTPUT_BACKPRESSURE_TIMEOUT.as_secs_f64(),
                    "retryOwner": "playback_audio_pump",
                })
                .to_string(),
            );
        }
        let stalled_for = now.saturating_duration_since(started_at);
        if stalled_for < AUDIO_OUTPUT_BACKPRESSURE_TIMEOUT {
            return None;
        }
        let reason = format!(
            "audio frame consumer remained full for {:.3}s after {} polls (queued_frames={}, capacity={})",
            stalled_for.as_secs_f64(),
            self.polls,
            queued_frames,
            capacity,
        );
        trace::diagnostic(
            serde_json::json!({
                "event": "playback_audio_output",
                "stage": "backpressure_timeout",
                "polls": self.polls,
                "stalledSeconds": stalled_for.as_secs_f64(),
                "queuedFrames": queued_frames,
                "capacityFrames": capacity,
                "reason": reason.as_str(),
            })
            .to_string(),
        );
        Some(reason)
    }

    fn reset(&mut self) {
        self.started_at = None;
        self.polls = 0;
        self.pending_logged = false;
    }
}

fn pump_audio_from_worker(
    engine: &mut VideoPlaybackEngine,
    inner: &Arc<Mutex<PlayerInner>>,
    playback_generation: u64,
    stage: &'static str,
    burst_limit: usize,
    backpressure: &mut AudioOutputBackpressureState,
) {
    let started = Instant::now();
    let mut emitted = 0usize;
    let mut budget_exhausted = false;
    for _ in 0..burst_limit {
        let elapsed = started.elapsed();
        if elapsed >= AUDIO_PREFILL_TIME_BUDGET {
            budget_exhausted = true;
            break;
        }
        match engine.tick_audio_bounded(
            AUDIO_PREFILL_PACKET_BUDGET,
            AUDIO_PREFILL_TIME_BUDGET.saturating_sub(elapsed),
        ) {
            Ok(Some(frame)) => {
                match try_emit_audio_frame_from_worker(
                    inner,
                    PlayerAudioFrame {
                        frame: frame.frame,
                        generation: playback_generation,
                    },
                ) {
                    AudioFrameEmitResult::Sent => {
                        backpressure.reset();
                        emitted += 1;
                    }
                    AudioFrameEmitResult::Full(frame) => {
                        engine.restore_pending_audio_frame(frame);
                        if let Some(reason) = backpressure.observe(inner) {
                            fail_playback_from_worker(
                                engine,
                                inner,
                                "audio_output_backpressure",
                                reason,
                            );
                        }
                        break;
                    }
                    AudioFrameEmitResult::Disconnected(_frame) => {
                        backpressure.reset();
                        engine.set_audio_output_active(false);
                        break;
                    }
                }
            }
            Ok(None) => break,
            Err(error) => {
                fail_playback_from_worker(engine, inner, "audio_tick", error.to_string());
                break;
            }
        }
    }
    if emitted > 0 || budget_exhausted {
        trace::log(format!(
            "[erika-clock-trace] stage=worker_audio_prefill:{stage} emitted={} frame_limit={} packet_budget={} budget_exhausted={} elapsed_ms={:.3}",
            emitted,
            burst_limit,
            AUDIO_PREFILL_PACKET_BUDGET,
            budget_exhausted,
            started.elapsed().as_secs_f64() * 1000.0,
        ));
    }
}

fn should_prefill_audio_from_worker(
    engine: &VideoPlaybackEngine,
    snapshot: Option<AudioClockSnapshot>,
    snapshot_at: Option<Instant>,
) -> bool {
    if !engine.should_prefill_audio() {
        return false;
    }
    if snapshot_at.is_none_or(|observed| observed.elapsed() >= AUDIO_CLOCK_SNAPSHOT_STALE_AFTER) {
        return true;
    }
    snapshot
        .and_then(|snapshot| snapshot.queued_duration)
        .is_none_or(|queued| queued < AUDIO_PREFILL_LOW_WATER)
}

fn observe_audio_pump_command(
    engine_state: PlaybackRunState,
    last_audio_snapshot: &mut Option<AudioClockSnapshot>,
    last_audio_snapshot_at: &mut Option<Instant>,
    command: &PlaybackCommand,
) {
    match command {
        PlaybackCommand::AudioClock(snapshot) => {
            *last_audio_snapshot = Some(*snapshot);
            *last_audio_snapshot_at = Some(Instant::now());
        }
        PlaybackCommand::Play { .. }
            if matches!(
                engine_state,
                PlaybackRunState::Stopped | PlaybackRunState::Ended
            ) =>
        {
            *last_audio_snapshot = None;
            *last_audio_snapshot_at = None;
        }
        PlaybackCommand::Seek { .. }
        | PlaybackCommand::Stop { .. }
        | PlaybackCommand::SelectAudioTrack(_) => {
            *last_audio_snapshot = None;
            *last_audio_snapshot_at = None;
        }
        _ => {}
    }
}

fn handle_playback_command(
    engine: &mut VideoPlaybackEngine,
    inner: &Arc<Mutex<PlayerInner>>,
    command: PlaybackCommand,
    playback_generation: &mut u64,
    last_executed_playback_command_sequence: &mut u64,
    frame_output_quiesced: &mut bool,
) -> bool {
    match command {
        PlaybackCommand::Play {
            sequence,
            generation,
            reply,
        } => {
            if !begin_playback_command_execution(sequence, last_executed_playback_command_sequence)
            {
                let _ = reply.send(Err(format!(
                    "play command sequence {sequence} arrived after sequence {} was already executed",
                    *last_executed_playback_command_sequence,
                )));
                return true;
            }
            let state_before = engine.state();
            let restarting = matches!(
                state_before,
                PlaybackRunState::Stopped | PlaybackRunState::Ended
            );
            match engine.play_checked() {
                Ok(restarted) => {
                    *playback_generation = (*playback_generation).max(generation).max(1);
                    if restarted {
                        *playback_generation = playback_generation.saturating_add(1).max(1);
                    }
                    if !playback_command_sequence_is_current(inner, sequence) {
                        let _ = reply.send(Err(format!(
                            "play command sequence {sequence} was superseded during execution"
                        )));
                        return true;
                    }
                    let committed = commit_playback_command_intent(
                        inner,
                        sequence,
                        Some(*playback_generation),
                        restarted.then_some(Duration::ZERO),
                        Some(PlayerState::Playing),
                    );
                    let _ = reply.send(if committed {
                        Ok(())
                    } else {
                        Err(format!(
                            "play command sequence {sequence} was superseded before commit"
                        ))
                    });
                }
                Err(error) => {
                    if !playback_command_sequence_is_current(inner, sequence) {
                        let _ = reply.send(Err(format!(
                            "play command sequence {sequence} was superseded while failing: {error}"
                        )));
                        return true;
                    }
                    let message = if restarting {
                        format!("failed to restart playback from {state_before:?} at zero: {error}")
                    } else {
                        format!("failed to start playback from {state_before:?}: {error}")
                    };
                    fail_playback_from_worker(
                        engine,
                        inner,
                        if restarting { "play_restart" } else { "play" },
                        message.clone(),
                    );
                    let _ = reply.send(Err(message));
                }
            }
        }
        PlaybackCommand::Pause { sequence } => {
            if !begin_playback_command_execution(sequence, last_executed_playback_command_sequence)
            {
                return true;
            }
            engine.pause();
            let _ = commit_playback_command_intent(
                inner,
                sequence,
                None,
                None,
                Some(PlayerState::Paused),
            );
        }
        PlaybackCommand::Seek {
            position,
            sequence,
            generation,
            resume_after_seek,
        } => {
            if !begin_playback_command_execution(sequence, last_executed_playback_command_sequence)
            {
                trace::log(format!(
                    "[erika-clock-trace] stage=worker_command_seek_skip_out_of_order target={} sequence={} last_executed_sequence={}",
                    trace::duration_label(Some(position)),
                    sequence,
                    *last_executed_playback_command_sequence,
                ));
                return true;
            }
            trace::log(format!(
                "[erika-clock-trace] stage=worker_command_seek target={} gen_before={} sequence={} resume_after_seek={}",
                trace::duration_label(Some(position)),
                *playback_generation,
                sequence,
                resume_after_seek,
            ));
            let result = engine.seek_with_playback_intent(position, resume_after_seek);
            if result.is_ok() {
                *playback_generation = (*playback_generation).max(generation).max(1);
            }
            if !playback_command_sequence_is_current(inner, sequence) {
                trace::log(format!(
                    "[erika-clock-trace] stage=worker_command_seek_superseded_after_execute target={} sequence={}",
                    trace::duration_label(Some(position)),
                    sequence,
                ));
                return true;
            }
            match result {
                Err(error) => fail_playback_from_worker(engine, inner, "seek", error.to_string()),
                Ok(()) => {
                    let _ = commit_playback_command_intent(
                        inner,
                        sequence,
                        Some(*playback_generation),
                        None,
                        resume_after_seek.then_some(PlayerState::Playing),
                    );
                    trace::log(format!(
                        "[erika-clock-trace] stage=worker_command_seek_done target={} gen_after={} sequence={} resume_after_seek={}",
                        trace::duration_label(Some(position)),
                        *playback_generation,
                        sequence,
                        resume_after_seek,
                    ));
                }
            }
        }
        PlaybackCommand::SetPlaybackRate(rate) => {
            engine.set_playback_rate(rate);
        }
        PlaybackCommand::Stop {
            sequence,
            generation,
        } => {
            if !begin_playback_command_execution(sequence, last_executed_playback_command_sequence)
            {
                return true;
            }
            let result = engine.stop_checked();
            if !playback_command_sequence_is_current(inner, sequence) {
                trace::log(format!(
                    "[erika-clock-trace] stage=worker_command_stop_superseded_after_execute sequence={sequence}",
                ));
                return true;
            }
            match result {
                Ok(()) => {
                    *playback_generation = (*playback_generation).max(generation).max(1);
                    let _ = commit_playback_command_intent(
                        inner,
                        sequence,
                        Some(*playback_generation),
                        Some(Duration::ZERO),
                        Some(PlayerState::Stopped),
                    );
                }
                Err(error) => {
                    fail_playback_from_worker(engine, inner, "stop", error.to_string());
                }
            }
        }
        PlaybackCommand::AudioClock(snapshot) => {
            trace::log(format!(
                "[erika-clock-trace] stage=worker_command_audio_clock media={} queued={} queued_frames={} read={} written={} underflow={} gen={}",
                trace::duration_label(snapshot.media_time),
                trace::duration_label(snapshot.queued_duration),
                snapshot.queued_frames,
                snapshot.read_frames,
                snapshot.written_frames,
                snapshot.underflow_frames,
                *playback_generation,
            ));
            let _ = engine.sync_to_audio_clock(snapshot);
        }
        PlaybackCommand::VideoFrameImportFailed(failure) => {
            trace::diagnostic(failure.structured_message());
            let shared_generation = shared_playback_generation(inner);
            if !video_import_feedback_is_current(
                failure.generation,
                *playback_generation,
                shared_generation,
            ) {
                trace::diagnostic(
                    serde_json::json!({
                        "event": "video_frame_import_failure",
                        "stage": "stale_feedback_ignored",
                        "failureGeneration": failure.generation,
                        "workerGeneration": *playback_generation,
                        "sharedGeneration": shared_generation,
                        "backend": failure.decode_backend.as_str(),
                        "mediaCodecSurface": failure.mediacodec_surface,
                        "reason": failure.reason.as_str(),
                    })
                    .to_string(),
                );
                return true;
            }
            match failure.decode_backend {
                DecoderBackend::MediaCodec => {
                    if let Err(error) = engine.handle_video_frame_import_failure(&failure) {
                        fail_video_import_from_worker(
                            engine,
                            inner,
                            format!(
                                "{}; software decoder recovery failed: {error}",
                                failure.structured_message()
                            ),
                        );
                    }
                }
                DecoderBackend::Software => {
                    fail_video_import_from_worker(engine, inner, failure.structured_message());
                }
                DecoderBackend::VideoToolbox | DecoderBackend::D3d11va => {}
            }
        }
        PlaybackCommand::AddExternalSubtitle { config, reply } => {
            match engine.add_external_subtitle(config) {
                Ok((track, clear_frame)) => {
                    *playback_generation = playback_generation.saturating_add(1).max(1);
                    publish_playback_generation_from_worker(inner, *playback_generation);
                    if let Some(frame) = clear_frame {
                        emit_subtitle_frame_from_worker(
                            inner,
                            PlayerSubtitleFrame {
                                frame: frame.frame,
                                pts: frame.pts,
                                media_time: frame.media_time,
                                late_by: frame.late_by,
                                generation: *playback_generation,
                            },
                        );
                    }
                    sync_track_state_from_worker(inner, engine);
                    let _ = reply.send(Ok(track));
                }
                Err(error) => {
                    let message = error.to_string();
                    emit_from_worker(
                        inner,
                        PlayerEvent::Error(PlayerError::Playback(message.clone())),
                    );
                    let _ = reply.send(Err(message));
                }
            }
        }
        PlaybackCommand::RemoveSubtitleTrack(track_id) => {
            match engine.remove_subtitle_track(track_id) {
                Ok(Some(frame)) => {
                    *playback_generation = playback_generation.saturating_add(1).max(1);
                    publish_playback_generation_from_worker(inner, *playback_generation);
                    emit_subtitle_frame_from_worker(
                        inner,
                        PlayerSubtitleFrame {
                            frame: frame.frame,
                            pts: frame.pts,
                            media_time: frame.media_time,
                            late_by: frame.late_by,
                            generation: *playback_generation,
                        },
                    );
                    sync_track_state_from_worker(inner, engine);
                }
                Ok(None) => {}
                Err(error) => emit_from_worker(
                    inner,
                    PlayerEvent::Error(PlayerError::Playback(error.to_string())),
                ),
            }
        }
        PlaybackCommand::SelectAudioTrack(track_id) => match engine.select_audio_track(track_id) {
            Ok(()) => {
                *playback_generation = playback_generation.saturating_add(1).max(1);
                publish_playback_generation_from_worker(inner, *playback_generation);
                sync_track_state_from_worker(inner, engine)
            }
            Err(error) => emit_from_worker(
                inner,
                PlayerEvent::Error(PlayerError::Playback(error.to_string())),
            ),
        },
        PlaybackCommand::SelectSubtitleTrack(track_id) => {
            match engine.select_subtitle_track(track_id) {
                Ok(Some(frame)) => {
                    *playback_generation = playback_generation.saturating_add(1).max(1);
                    publish_playback_generation_from_worker(inner, *playback_generation);
                    emit_subtitle_frame_from_worker(
                        inner,
                        PlayerSubtitleFrame {
                            frame: frame.frame,
                            pts: frame.pts,
                            media_time: frame.media_time,
                            late_by: frame.late_by,
                            generation: *playback_generation,
                        },
                    );
                    sync_track_state_from_worker(inner, engine);
                }
                Ok(None) => {
                    *playback_generation = playback_generation.saturating_add(1).max(1);
                    publish_playback_generation_from_worker(inner, *playback_generation);
                    sync_track_state_from_worker(inner, engine)
                }
                Err(error) => emit_from_worker(
                    inner,
                    PlayerEvent::Error(PlayerError::Playback(error.to_string())),
                ),
            }
        }
        PlaybackCommand::SetFrameOutputQuiesced { quiesced, reply } => {
            *frame_output_quiesced = quiesced;
            if !quiesced {
                engine.rebase_progress_watchdogs();
            }
            trace::diagnostic(
                serde_json::json!({
                    "event": "player_frame_output",
                    "stage": if quiesced { "quiesced" } else { "resumed" },
                    "generation": *playback_generation,
                })
                .to_string(),
            );
            let _ = reply.send(());
        }
        PlaybackCommand::Shutdown => return false,
    }
    true
}

fn fail_playback_from_worker(
    engine: &mut VideoPlaybackEngine,
    inner: &Arc<Mutex<PlayerInner>>,
    stage: &'static str,
    message: String,
) {
    // A decoder/demux/audio failure is terminal for the active session. Pause
    // the engine before publishing the error so the worker cannot retry the
    // same failing packet every poll interval and flood both native and Dart
    // event queues. The Player remains alive for explicit close/dispose.
    engine.pause();
    trace::diagnostic(
        serde_json::json!({
            "event": "playback_fatal",
            "stage": stage,
            "reason": message.as_str(),
        })
        .to_string(),
    );
    emit_from_worker(inner, PlayerEvent::Error(PlayerError::Playback(message)));
    set_state_from_worker(inner, PlayerState::Error);
}

fn fail_video_import_from_worker(
    engine: &mut VideoPlaybackEngine,
    inner: &Arc<Mutex<PlayerInner>>,
    message: String,
) {
    engine.pause();
    trace::diagnostic(
        serde_json::json!({
            "event": "video_frame_import_fatal",
            "reason": message.as_str(),
        })
        .to_string(),
    );
    emit_from_worker(inner, PlayerEvent::Error(PlayerError::Renderer(message)));
    set_state_from_worker(inner, PlayerState::Error);
}

fn sync_track_state_from_worker(inner: &Arc<Mutex<PlayerInner>>, engine: &VideoPlaybackEngine) {
    let tracks = engine.info().tracks.clone();
    let selection = engine.track_selection();
    let mut events = Vec::new();
    {
        let mut inner = inner.lock().expect("player mutex poisoned");
        if inner.tracks != tracks {
            inner.tracks = tracks.clone();
            events.push(PlayerEvent::TracksChanged(tracks));
        }
        if inner.track_selection != selection {
            inner.track_selection = selection;
            events.push(PlayerEvent::TrackSelectionChanged(selection));
        }
    }
    for event in events {
        emit_from_worker(inner, event);
    }
}

fn publish_natural_eof_from_worker(
    engine: &VideoPlaybackEngine,
    inner: &Arc<Mutex<PlayerInner>>,
    playback_generation: u64,
    eof_published: &mut bool,
) -> bool {
    if engine.state() != PlaybackRunState::Ended {
        *eof_published = false;
        return false;
    }
    let final_position = engine
        .info()
        .duration
        .unwrap_or_else(|| engine.media_time());
    publish_natural_eof_events_from_worker(
        inner,
        playback_generation,
        final_position,
        eof_published,
    );
    true
}

fn publish_natural_eof_events_from_worker(
    inner: &Arc<Mutex<PlayerInner>>,
    playback_generation: u64,
    final_position: Duration,
    eof_published: &mut bool,
) {
    if *eof_published {
        return;
    }
    let mut inner = inner.lock().expect("player mutex poisoned");
    let generation = playback_generation.max(1);
    if worker_generation_is_stale(generation, inner.playback_generation) {
        trace::log(format!(
            "[erika-clock-trace] stage=worker_eof_skip_stale media={} worker_gen={} shared_gen={}",
            trace::duration_label(Some(final_position)),
            generation,
            inner.playback_generation,
        ));
        return;
    }
    inner.current_media_time = final_position;
    inner.ended = true;
    inner.subscribers.retain(|sender| {
        sender
            .send(PlayerEvent::PositionChanged(final_position))
            .is_ok()
    });
    let previous = inner.state;
    inner.state = PlayerState::Stopped;
    if previous != PlayerState::Stopped {
        inner.subscribers.retain(|sender| {
            sender
                .send(PlayerEvent::StateChanged(PlayerState::Stopped))
                .is_ok()
        });
    }
    *eof_published = true;
}

fn sync_playback_clock_from_worker(
    engine: &VideoPlaybackEngine,
    inner: &Arc<Mutex<PlayerInner>>,
    playback_generation: u64,
    stage: &'static str,
    last_worker_clock: &mut Option<(Duration, u64)>,
) {
    let media_time = engine.media_time();
    let mut inner = inner.lock().expect("player mutex poisoned");
    let shared_before = inner.current_media_time;
    let generation = playback_generation.max(1);
    let shared_generation = inner.playback_generation.max(1);
    if worker_clock_generation_is_stale(shared_generation, generation) {
        trace::log(format!(
            "[erika-clock-trace] stage=worker_sync:{stage} action=skip_stale_worker media={} shared_before={} worker_gen={} shared_gen={}",
            trace::duration_label(Some(media_time)),
            trace::duration_label(Some(shared_before)),
            generation,
            shared_generation,
        ));
        return;
    }
    let worker_back = last_worker_clock.is_some_and(|(last_time, last_generation)| {
        last_generation == generation && trace::duration_regressed(media_time, last_time)
    });
    let shared_back = trace::duration_regressed(media_time, shared_before);
    let generation_changed =
        last_worker_clock.is_some_and(|(_, last_generation)| last_generation != generation);
    if worker_back || shared_back || generation_changed {
        trace::log(format!(
            "[erika-clock-trace] stage=worker_sync:{stage} media={} shared_before={} gen={} last_worker={} last_worker_gen={} flags=worker_back:{} shared_back:{} gen_change:{}",
            trace::duration_label(Some(media_time)),
            trace::duration_label(Some(shared_before)),
            generation,
            trace::duration_label(last_worker_clock.map(|(time, _)| time)),
            last_worker_clock
                .map(|(_, generation)| generation)
                .unwrap_or(0),
            worker_back,
            shared_back,
            generation_changed,
        ));
    }
    inner.current_media_time = media_time;
    inner.playback_generation = generation;
    *last_worker_clock = Some((media_time, generation));
}

fn worker_clock_generation_is_stale(shared_generation: u64, worker_generation: u64) -> bool {
    shared_generation.max(1) > worker_generation.max(1)
}

fn playback_poll_interval(state: PlaybackRunState) -> Duration {
    match state {
        PlaybackRunState::Playing => Duration::from_millis(2),
        PlaybackRunState::Paused | PlaybackRunState::Stopped | PlaybackRunState::Ended => {
            Duration::from_millis(50)
        }
    }
}

fn playback_command_sequence_is_current(inner: &Arc<Mutex<PlayerInner>>, sequence: u64) -> bool {
    inner
        .lock()
        .expect("player mutex poisoned")
        .playback_command_sequence
        == sequence
}

fn begin_playback_command_execution(sequence: u64, last_executed_sequence: &mut u64) -> bool {
    if sequence <= *last_executed_sequence {
        return false;
    }
    *last_executed_sequence = sequence;
    true
}

fn commit_playback_command_intent(
    inner: &Arc<Mutex<PlayerInner>>,
    sequence: u64,
    playback_generation: Option<u64>,
    position: Option<Duration>,
    state: Option<PlayerState>,
) -> bool {
    let mut inner = inner.lock().expect("player mutex poisoned");
    if inner.playback_command_sequence != sequence {
        return false;
    }
    if let Some(playback_generation) = playback_generation {
        inner.playback_generation = inner.playback_generation.max(playback_generation.max(1));
    }
    inner.ended = false;
    if let Some(position) = position {
        inner.current_media_time = position;
        inner
            .subscribers
            .retain(|sender| sender.send(PlayerEvent::PositionChanged(position)).is_ok());
    }
    if let Some(state) = state {
        let previous = inner.state;
        inner.state = state;
        if previous != state {
            inner
                .subscribers
                .retain(|sender| sender.send(PlayerEvent::StateChanged(state)).is_ok());
        }
    }
    true
}

fn worker_generation_is_stale(worker_generation: u64, shared_generation: u64) -> bool {
    worker_generation.max(1) < shared_generation.max(1)
}

fn video_import_feedback_is_current(
    failure_generation: u64,
    worker_generation: u64,
    shared_generation: u64,
) -> bool {
    failure_generation == worker_generation && failure_generation == shared_generation
}

fn shared_playback_generation(inner: &Arc<Mutex<PlayerInner>>) -> u64 {
    inner
        .lock()
        .expect("player mutex poisoned")
        .playback_generation
        .max(1)
}

fn publish_playback_generation_from_worker(
    inner: &Arc<Mutex<PlayerInner>>,
    playback_generation: u64,
) {
    let mut inner = inner.lock().expect("player mutex poisoned");
    inner.playback_generation = inner.playback_generation.max(playback_generation.max(1));
}

#[cfg(test)]
fn set_state_from_worker_for_generation(
    inner: &Arc<Mutex<PlayerInner>>,
    playback_generation: u64,
    next: PlayerState,
) -> bool {
    let mut inner = inner.lock().expect("player mutex poisoned");
    if worker_generation_is_stale(playback_generation, inner.playback_generation) {
        return false;
    }
    let previous = inner.state;
    inner.state = next;
    if previous != next {
        inner
            .subscribers
            .retain(|sender| sender.send(PlayerEvent::StateChanged(next)).is_ok());
    }
    true
}

fn enqueue_frame_output_resume_after_timeout(commands: &Sender<PlaybackCommand>) {
    let (reply, response) = bounded(1);
    drop(response);
    match commands.try_send(PlaybackCommand::SetFrameOutputQuiesced {
        quiesced: false,
        reply,
    }) {
        Ok(()) => trace::diagnostic(
            serde_json::json!({
                "event": "player_frame_output",
                "stage": "timeout_resume_enqueued",
                "action": "resume",
                "reason": "a timed-out quiesce may still be pending in the playback command FIFO",
            })
            .to_string(),
        ),
        Err(TrySendError::Full(_)) => trace::diagnostic(
            serde_json::json!({
                "event": "player_frame_output",
                "stage": "timeout_resume_enqueue_failed",
                "action": "resume",
                "reason": "playback command queue is full",
            })
            .to_string(),
        ),
        Err(TrySendError::Disconnected(_)) => trace::diagnostic(
            serde_json::json!({
                "event": "player_frame_output",
                "stage": "timeout_resume_enqueue_failed",
                "action": "resume",
                "reason": "playback command queue is disconnected",
            })
            .to_string(),
        ),
    }
}

fn set_state_from_worker(inner: &Arc<Mutex<PlayerInner>>, next: PlayerState) {
    let previous = {
        let mut inner = inner.lock().expect("player mutex poisoned");
        let previous = inner.state;
        inner.state = next;
        previous
    };
    if previous != next {
        emit_from_worker(inner, PlayerEvent::StateChanged(next));
    }
}

fn emit_from_worker(inner: &Arc<Mutex<PlayerInner>>, event: PlayerEvent) {
    let mut inner = inner.lock().expect("player mutex poisoned");
    inner
        .subscribers
        .retain(|sender| sender.send(event.clone()).is_ok());
}

fn emit_from_worker_for_generation(
    inner: &Arc<Mutex<PlayerInner>>,
    playback_generation: u64,
    event: PlayerEvent,
) -> bool {
    let mut inner = inner.lock().expect("player mutex poisoned");
    if worker_generation_is_stale(playback_generation, inner.playback_generation) {
        return false;
    }
    inner
        .subscribers
        .retain(|sender| sender.send(event.clone()).is_ok());
    true
}

fn emit_video_frame_from_worker(inner: &Arc<Mutex<PlayerInner>>, frame: PlayerVideoFrame) {
    let mut inner = inner.lock().expect("player mutex poisoned");
    let Some(sender) = inner.video_frame_sender.as_ref() else {
        return;
    };
    match sender.try_send(frame) {
        Ok(()) | Err(crossbeam_channel::TrySendError::Full(_)) => {}
        Err(crossbeam_channel::TrySendError::Disconnected(_)) => inner.video_frame_sender = None,
    }
}

fn audio_frame_output_is_active(inner: &Arc<Mutex<PlayerInner>>) -> bool {
    inner
        .lock()
        .expect("player mutex poisoned")
        .audio_frame_sender
        .is_some()
}

fn audio_frame_channel_metrics(inner: &Arc<Mutex<PlayerInner>>) -> (usize, usize) {
    let inner = inner.lock().expect("player mutex poisoned");
    let Some(sender) = inner.audio_frame_sender.as_ref() else {
        return (0, 0);
    };
    (sender.len(), sender.capacity().unwrap_or(0))
}

enum AudioFrameEmitResult {
    Sent,
    Full(PcmAudioFrame),
    Disconnected(PcmAudioFrame),
}

fn try_emit_audio_frame_from_worker(
    inner: &Arc<Mutex<PlayerInner>>,
    frame: PlayerAudioFrame,
) -> AudioFrameEmitResult {
    let mut inner = inner.lock().expect("player mutex poisoned");
    let result = {
        let Some(sender) = inner.audio_frame_sender.as_ref() else {
            return AudioFrameEmitResult::Disconnected(frame.frame);
        };
        sender.try_send(frame)
    };
    match result {
        Ok(()) => AudioFrameEmitResult::Sent,
        Err(TrySendError::Full(frame)) => AudioFrameEmitResult::Full(frame.frame),
        Err(TrySendError::Disconnected(frame)) => {
            inner.audio_frame_sender = None;
            AudioFrameEmitResult::Disconnected(frame.frame)
        }
    }
}

#[cfg(test)]
fn emit_audio_frame_from_worker(inner: &Arc<Mutex<PlayerInner>>, frame: PlayerAudioFrame) -> bool {
    matches!(
        try_emit_audio_frame_from_worker(inner, frame),
        AudioFrameEmitResult::Sent
    )
}

fn emit_subtitle_frame_from_worker(inner: &Arc<Mutex<PlayerInner>>, frame: PlayerSubtitleFrame) {
    let mut inner = inner.lock().expect("player mutex poisoned");
    let result = {
        let Some(sender) = inner.subtitle_frame_sender.as_ref() else {
            return;
        };
        sender.try_send(frame)
    };
    match result {
        Ok(()) | Err(TrySendError::Full(_)) => {}
        Err(TrySendError::Disconnected(_)) => inner.subtitle_frame_sender = None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install_test_runtime(player: &Player, capacity: usize) -> Receiver<PlaybackCommand> {
        let (commands, receiver) = bounded(capacity.max(1));
        player
            .lifecycle
            .lock()
            .expect("player lifecycle mutex poisoned")
            .playback = Some(PlaybackRuntime {
            commands,
            worker: None,
        });
        receiver
    }

    fn play_with_test_ack(
        player: &Player,
        commands: &Receiver<PlaybackCommand>,
        restarted_from_eof: bool,
    ) -> (u64, u64) {
        let caller = player.clone();
        let pending_play = std::thread::spawn(move || caller.play());
        let (sequence, generation, reply) = match commands
            .recv_timeout(Duration::from_secs(1))
            .expect("play command")
        {
            PlaybackCommand::Play {
                sequence,
                generation,
                reply,
            } => (sequence, generation, reply),
            _ => panic!("expected play command"),
        };
        let worker_generation = if restarted_from_eof {
            generation.saturating_add(1).max(1)
        } else {
            generation.max(1)
        };
        assert!(commit_playback_command_intent(
            &player.inner,
            sequence,
            Some(worker_generation),
            restarted_from_eof.then_some(Duration::ZERO),
            Some(PlayerState::Playing),
        ));
        reply.send(Ok(())).expect("acknowledge play command");
        pending_play
            .join()
            .expect("play caller thread")
            .expect("play acknowledgement");
        (sequence, generation)
    }

    fn install_fake_playback(
        player: &Player,
        state: PlayerState,
        media_time: Duration,
        generation: u64,
        ended: bool,
    ) -> Receiver<PlaybackCommand> {
        let receiver = install_test_runtime(player, 16);
        {
            let mut inner = player.inner.lock().expect("player mutex poisoned");
            inner.state = state;
            inner.current_media_time = media_time;
            inner.playback_generation = generation;
            inner.ended = ended;
        }
        receiver
    }

    fn install_disconnected_playback(
        player: &Player,
        state: PlayerState,
        media_time: Duration,
        generation: u64,
        ended: bool,
    ) {
        let receiver = install_test_runtime(player, 1);
        drop(receiver);
        let mut inner = player.inner.lock().expect("player mutex poisoned");
        inner.state = state;
        inner.current_media_time = media_time;
        inner.playback_generation = generation;
        inner.ended = ended;
    }

    fn test_audio_frame(generation: u64) -> PlayerAudioFrame {
        PlayerAudioFrame {
            frame: PcmAudioFrame {
                format: Default::default(),
                pts: Some(Duration::from_millis(generation)),
                frames: 1,
                samples: vec![0.0, 0.0],
            },
            generation,
        }
    }

    #[test]
    fn frame_output_barrier_and_runtime_owner_do_not_form_an_inner_cycle() {
        let player = Player::new(PlayerConfig::default());
        let inner_weak = Arc::downgrade(&player.inner);
        let lifecycle_weak = Arc::downgrade(&player.lifecycle);
        let worker_inner = Arc::clone(&player.inner);
        let (commands, receiver) = bounded(4);
        let worker = std::thread::spawn(move || {
            while let Ok(command) = receiver.recv() {
                match command {
                    PlaybackCommand::SetFrameOutputQuiesced { reply, .. } => {
                        let _ = reply.send(());
                    }
                    PlaybackCommand::Shutdown => break,
                    _ => {}
                }
            }
            drop(worker_inner);
        });
        player
            .lifecycle
            .lock()
            .expect("player lifecycle mutex poisoned")
            .playback = Some(PlaybackRuntime {
            commands,
            worker: Some(worker),
        });

        assert!(player.set_frame_output_quiesced(true).unwrap());
        assert!(player.set_frame_output_quiesced(false).unwrap());

        drop(player);
        assert!(lifecycle_weak.upgrade().is_none());
        assert!(inner_weak.upgrade().is_none());
    }

    #[test]
    fn frame_output_barrier_is_a_noop_without_open_media() {
        let player = Player::new(PlayerConfig::default());
        assert!(!player.set_frame_output_quiesced(true).unwrap());
    }

    #[test]
    fn frame_output_barrier_reports_an_unresponsive_worker() {
        let player = Player::new(PlayerConfig::default());
        let receiver = install_test_runtime(&player, 2);
        let started = Instant::now();

        let error = player
            .set_frame_output_quiesced_with_timeout(true, Duration::from_millis(25))
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("did not acknowledge frame output quiesce")
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
            PlaybackCommand::SetFrameOutputQuiesced { quiesced: true, .. }
        ));
        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
            PlaybackCommand::SetFrameOutputQuiesced {
                quiesced: false,
                ..
            }
        ));
        drop(receiver);
    }

    #[test]
    fn stale_worker_generation_cannot_publish_position_or_stopped_state() {
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();
        {
            let mut inner = player.inner.lock().expect("player mutex poisoned");
            inner.state = PlayerState::Playing;
            inner.current_media_time = Duration::from_secs(5);
            inner.playback_generation = 4;
        }

        assert!(worker_generation_is_stale(3, 4));
        assert!(!emit_from_worker_for_generation(
            &player.inner,
            3,
            PlayerEvent::PositionChanged(Duration::from_millis(3_400))
        ));
        assert!(!set_state_from_worker_for_generation(
            &player.inner,
            3,
            PlayerState::Stopped
        ));

        assert_eq!(player.state(), PlayerState::Playing);
        assert_eq!(player.current_media_time(), Duration::from_secs(5));
        assert!(events.try_recv().is_err());
        assert!(!worker_generation_is_stale(4, 4));
    }

    #[test]
    fn stale_video_import_feedback_cannot_downgrade_a_new_decoder_generation() {
        assert!(video_import_feedback_is_current(7, 7, 7));
        assert!(!video_import_feedback_is_current(6, 7, 7));
        assert!(!video_import_feedback_is_current(7, 7, 8));
        assert!(!video_import_feedback_is_current(8, 7, 7));
    }

    #[test]
    fn newer_command_sequence_rejects_older_state_and_position_commit() {
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();
        {
            let mut inner = player.inner.lock().expect("player mutex poisoned");
            inner.state = PlayerState::Stopped;
            inner.current_media_time = Duration::from_secs(9);
            inner.playback_command_sequence = 7;
        }

        assert!(!commit_playback_command_intent(
            &player.inner,
            6,
            None,
            Some(Duration::ZERO),
            Some(PlayerState::Playing),
        ));
        assert_eq!(player.state(), PlayerState::Stopped);
        assert_eq!(player.current_media_time(), Duration::from_secs(9));
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn fifo_commands_execute_even_when_a_newer_sequence_is_already_reserved() {
        let player = Player::new(PlayerConfig::default());
        {
            let mut inner = player.inner.lock().expect("player mutex poisoned");
            inner.playback_command_sequence = 2;
        }
        let mut last_executed = 0;

        assert!(begin_playback_command_execution(1, &mut last_executed));
        assert_eq!(last_executed, 1);
        assert!(!commit_playback_command_intent(
            &player.inner,
            1,
            None,
            Some(Duration::from_secs(3)),
            Some(PlayerState::Playing),
        ));
        assert!(begin_playback_command_execution(2, &mut last_executed));
        assert_eq!(last_executed, 2);
    }

    #[test]
    fn lower_sequence_arriving_after_newer_execution_is_skipped() {
        let mut last_executed = 0;

        assert!(begin_playback_command_execution(2, &mut last_executed));
        assert!(!begin_playback_command_execution(1, &mut last_executed));
        assert_eq!(last_executed, 2);
    }

    #[test]
    fn seek_while_playing_carries_resume_intent_past_stale_eof() {
        let player = Player::new(PlayerConfig::default());
        let receiver = install_test_runtime(&player, 2);
        {
            let mut inner = player.inner.lock().expect("player mutex poisoned");
            inner.state = PlayerState::Playing;
        }
        let stale_generation = player.playback_generation();

        player.seek(Duration::from_secs(4)).unwrap();

        let (sequence, generation, resume_after_seek) = match receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("seek command")
        {
            PlaybackCommand::Seek {
                position,
                sequence,
                generation,
                resume_after_seek,
            } => {
                assert_eq!(position, Duration::from_secs(4));
                (sequence, generation, resume_after_seek)
            }
            _ => panic!("expected seek command"),
        };
        assert!(resume_after_seek);
        assert!(generation > stale_generation);
        assert!(playback_command_sequence_is_current(
            &player.inner,
            sequence
        ));
        assert!(!set_state_from_worker_for_generation(
            &player.inner,
            stale_generation,
            PlayerState::Stopped,
        ));
        assert_eq!(player.state(), PlayerState::Playing);
    }

    #[test]
    fn stop_sequence_supersedes_a_pending_restart_commit() {
        let player = Player::new(PlayerConfig::default());
        let receiver = install_test_runtime(&player, 3);
        {
            let mut inner = player.inner.lock().expect("player mutex poisoned");
            inner.state = PlayerState::Stopped;
            inner.current_media_time = Duration::from_secs(11);
        }
        let caller = player.clone();
        let pending_play = std::thread::spawn(move || caller.play());
        let (play_sequence, reply) = match receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("play command")
        {
            PlaybackCommand::Play {
                sequence, reply, ..
            } => (sequence, reply),
            _ => panic!("expected play command"),
        };

        player.stop().unwrap();
        let stop_sequence = match receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("stop command")
        {
            PlaybackCommand::Stop { sequence, .. } => sequence,
            _ => panic!("expected stop command"),
        };
        assert!(stop_sequence > play_sequence);
        assert!(!commit_playback_command_intent(
            &player.inner,
            play_sequence,
            None,
            Some(Duration::ZERO),
            Some(PlayerState::Playing),
        ));
        reply
            .send(Err("play command superseded by stop".to_string()))
            .unwrap();

        assert!(pending_play.join().unwrap().is_err());
        assert_eq!(player.state(), PlayerState::Stopped);
        assert_eq!(player.current_media_time(), Duration::ZERO);
    }

    #[test]
    fn player_play_waits_for_worker_ack_before_committing_playing() {
        let player = Player::new(PlayerConfig::default());
        let receiver = install_test_runtime(&player, 2);
        {
            let mut inner = player.inner.lock().expect("player mutex poisoned");
            inner.state = PlayerState::Stopped;
        }
        let caller = player.clone();
        let play = std::thread::spawn(move || caller.play());

        let reply = match receiver.recv_timeout(Duration::from_secs(1)).unwrap() {
            PlaybackCommand::Play { reply, .. } => reply,
            _ => panic!("expected play command"),
        };
        assert_eq!(player.state(), PlayerState::Stopped);

        set_state_from_worker(&player.inner, PlayerState::Playing);
        reply.send(Ok(())).unwrap();

        play.join().unwrap().unwrap();
        assert_eq!(player.state(), PlayerState::Playing);
    }

    #[test]
    fn player_stop_after_error_then_failed_restart_does_not_false_publish_playing() {
        let player = Player::new(PlayerConfig::default());
        let receiver = install_test_runtime(&player, 2);
        {
            let mut inner = player.inner.lock().expect("player mutex poisoned");
            inner.state = PlayerState::Error;
            inner.current_media_time = Duration::from_secs(7);
        }

        player.stop().unwrap();
        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
            PlaybackCommand::Stop { .. }
        ));
        assert_eq!(player.state(), PlayerState::Stopped);
        assert_eq!(player.current_media_time(), Duration::ZERO);

        let caller = player.clone();
        let play = std::thread::spawn(move || caller.play());

        let reply = match receiver.recv_timeout(Duration::from_secs(1)).unwrap() {
            PlaybackCommand::Play { reply, .. } => reply,
            _ => panic!("expected play command"),
        };
        let message = "video decoder unavailable: all fallback routes failed".to_string();
        set_state_from_worker(&player.inner, PlayerState::Error);
        reply.send(Err(message.clone())).unwrap();

        assert_eq!(
            play.join().unwrap().unwrap_err(),
            PlayerError::Playback(message)
        );
        assert_eq!(player.state(), PlayerState::Error);
        assert_ne!(player.state(), PlayerState::Playing);
        assert_eq!(player.current_media_time(), Duration::ZERO);
    }

    #[test]
    fn player_play_pause_emits_state_events_after_ready() {
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();

        player.transition(PlayerState::Ready).unwrap();
        assert!(matches!(
            player.play().unwrap_err(),
            PlayerError::Playback(_)
        ));

        assert_eq!(
            events.recv().unwrap(),
            PlayerEvent::StateChanged(PlayerState::Ready)
        );
    }

    #[test]
    fn closed_player_rejects_commands() {
        let player = Player::new(PlayerConfig::default());
        player.close().unwrap();
        assert_eq!(player.play().unwrap_err(), PlayerError::Closed);
    }

    #[test]
    fn attach_surface_emits_event() {
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();
        let surface = PlatformSurface::Metal(MetalSurfaceHandle::new(42, 1920, 1080, 2.0));

        player.attach_surface(surface).unwrap();

        assert_eq!(
            events.recv().unwrap(),
            PlayerEvent::SurfaceAttached(surface)
        );
    }

    #[test]
    fn attach_wgpu_surface_emits_event() {
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();
        let surface = PlatformSurface::Wgpu(WgpuSurfaceHandle::new(
            WgpuSurfaceKind::MacOsCaMetalLayer,
            42,
            0,
            1920,
            1080,
            2.0,
        ));

        player.attach_surface(surface).unwrap();

        assert_eq!(
            events.recv().unwrap(),
            PlayerEvent::SurfaceAttached(surface)
        );
    }

    #[test]
    fn attach_flutter_texture_surface_emits_event() {
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();
        let surface = PlatformSurface::FlutterTexture(FlutterTextureHandle::new(
            FlutterTextureKind::MacOsTextureRegistrar,
            7,
            1280,
            720,
            2.0,
        ));

        player.attach_surface(surface).unwrap();

        assert_eq!(
            events.recv().unwrap(),
            PlayerEvent::SurfaceAttached(surface)
        );
    }

    #[test]
    fn subscribe_subtitle_frames_replaces_previous_sender() {
        let player = Player::new(PlayerConfig::default());
        let first = player.subscribe_subtitle_frames();
        let second = player.subscribe_subtitle_frames();
        let frame = PlayerSubtitleFrame {
            frame: crate::subtitle::DecodedSubtitleFrame::new(2, Some(Duration::ZERO), None),
            pts: Some(Duration::ZERO),
            media_time: Duration::ZERO,
            late_by: None,
            generation: 1,
        };

        emit_subtitle_frame_from_worker(&player.inner, frame);

        assert!(first.try_recv().is_err());
        assert!(second.try_recv().is_ok());
    }

    #[test]
    fn full_audio_frame_queue_cannot_block_quiesce_command_ack() {
        let player = Player::new(PlayerConfig {
            audio_frame_queue_capacity: 1,
            ..PlayerConfig::default()
        });
        let frames = player.subscribe_audio_frames();
        assert!(emit_audio_frame_from_worker(
            &player.inner,
            test_audio_frame(1),
        ));

        let worker_inner = Arc::clone(&player.inner);
        let (commands, receiver) = bounded(1);
        let (emit_result_sender, emit_result_receiver) = bounded(1);
        let worker = std::thread::spawn(move || {
            let emitted = emit_audio_frame_from_worker(&worker_inner, test_audio_frame(2));
            emit_result_sender.send(emitted).unwrap();
            match receiver.recv().unwrap() {
                PlaybackCommand::SetFrameOutputQuiesced { reply, .. } => reply.send(()).unwrap(),
                _ => panic!("expected frame-output quiesce command"),
            }
        });

        let (reply, response) = bounded(1);
        commands
            .send(PlaybackCommand::SetFrameOutputQuiesced {
                quiesced: true,
                reply,
            })
            .unwrap();
        let acknowledged = response.recv_timeout(Duration::from_secs(1));
        if acknowledged.is_err() {
            // Let a blocking implementation unwind so the regression test itself
            // never strands its worker thread after reporting the timeout.
            let _ = frames.recv_timeout(Duration::from_secs(1));
            let _ = response.recv_timeout(Duration::from_secs(1));
        }
        let emitted = emit_result_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        worker.join().unwrap();

        assert!(acknowledged.is_ok(), "full audio queue blocked quiesce ACK");
        assert!(!emitted, "a full audio queue must apply backpressure");
    }

    #[test]
    fn disconnected_audio_frame_queue_is_removed_without_blocking() {
        let player = Player::new(PlayerConfig::default());
        let frames = player.subscribe_audio_frames();
        drop(frames);

        assert!(!emit_audio_frame_from_worker(
            &player.inner,
            test_audio_frame(1),
        ));
        assert!(
            player
                .inner
                .lock()
                .expect("player mutex poisoned")
                .audio_frame_sender
                .is_none()
        );
    }

    #[test]
    fn disconnected_full_audio_queue_is_detected_without_a_preflight_full_check() {
        let player = Player::new(PlayerConfig {
            audio_frame_queue_capacity: 1,
            ..PlayerConfig::default()
        });
        let frames = player.subscribe_audio_frames();
        assert!(emit_audio_frame_from_worker(
            &player.inner,
            test_audio_frame(1),
        ));
        drop(frames);

        assert!(matches!(
            try_emit_audio_frame_from_worker(&player.inner, test_audio_frame(2)),
            AudioFrameEmitResult::Disconnected(_)
        ));
        assert!(
            player
                .inner
                .lock()
                .expect("player mutex poisoned")
                .audio_frame_sender
                .is_none()
        );
    }

    #[test]
    fn subtitle_event_handoff_does_not_drop_when_configured_capacity_is_exceeded() {
        let player = Player::new(PlayerConfig {
            subtitle_frame_queue_capacity: 1,
            ..PlayerConfig::default()
        });
        let frames = player.subscribe_subtitle_frames();

        for index in 0..64 {
            emit_subtitle_frame_from_worker(
                &player.inner,
                PlayerSubtitleFrame {
                    frame: crate::subtitle::DecodedSubtitleFrame::new(
                        2,
                        Some(Duration::from_millis(index)),
                        Some(Duration::from_millis(index + 1)),
                    ),
                    pts: Some(Duration::from_millis(index)),
                    media_time: Duration::from_millis(index),
                    late_by: None,
                    generation: 1,
                },
            );
        }

        assert_eq!(frames.try_iter().count(), 64);
    }

    #[test]
    fn player_track_cache_defaults_to_empty_selection() {
        let player = Player::new(PlayerConfig::default());

        assert!(player.tracks().is_empty());
        assert_eq!(player.track_selection(), TrackSelection::default());
    }

    #[test]
    fn stopped_play_from_eof_publishes_rewind_before_playing() {
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();
        let commands = install_fake_playback(
            &player,
            PlayerState::Stopped,
            Duration::from_secs(8),
            7,
            true,
        );

        let (_, command_generation) = play_with_test_ack(&player, &commands, true);

        assert_eq!(command_generation, 7);
        assert_eq!(player.current_media_time(), Duration::ZERO);
        assert_eq!(player.playback_generation(), 8);
        assert_eq!(player.state(), PlayerState::Playing);
        assert!(!player.is_ended());
        assert_eq!(
            events.try_iter().collect::<Vec<_>>(),
            vec![
                PlayerEvent::PositionChanged(Duration::ZERO),
                PlayerEvent::StateChanged(PlayerState::Playing),
            ]
        );
    }

    #[test]
    fn stopped_play_at_zero_does_not_publish_another_generation() {
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();
        let commands =
            install_fake_playback(&player, PlayerState::Stopped, Duration::ZERO, 7, false);

        let (_, command_generation) = play_with_test_ack(&player, &commands, false);

        assert_eq!(command_generation, 7);
        assert_eq!(player.current_media_time(), Duration::ZERO);
        assert_eq!(player.playback_generation(), 7);
        assert!(!player.is_ended());
        assert_eq!(
            events.try_iter().collect::<Vec<_>>(),
            vec![PlayerEvent::StateChanged(PlayerState::Playing)]
        );
    }

    #[test]
    fn failed_eof_play_rolls_back_prepublished_clock_and_generation() {
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();
        install_disconnected_playback(
            &player,
            PlayerState::Stopped,
            Duration::from_secs(8),
            7,
            true,
        );

        assert!(matches!(player.play(), Err(PlayerError::Playback(_))));

        assert_eq!(player.current_media_time(), Duration::from_secs(8));
        assert_eq!(player.playback_generation(), 7);
        assert_eq!(player.state(), PlayerState::Stopped);
        assert!(player.is_ended());
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn seek_from_eof_then_play_preserves_nonzero_target_and_single_generation() {
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();
        let commands = install_fake_playback(
            &player,
            PlayerState::Stopped,
            Duration::from_secs(8),
            7,
            true,
        );
        let target = Duration::from_millis(3_250);

        player.seek(target).unwrap();

        assert!(matches!(
            commands.try_recv(),
            Ok(PlaybackCommand::Seek { position, .. }) if position == target
        ));
        let (_, command_generation) = play_with_test_ack(&player, &commands, false);
        assert_eq!(command_generation, 8);
        assert_eq!(player.current_media_time(), target);
        assert_eq!(player.playback_generation(), 8);
        assert_eq!(player.state(), PlayerState::Playing);
        assert!(!player.is_ended());
        assert_eq!(
            events.try_iter().collect::<Vec<_>>(),
            vec![
                PlayerEvent::PositionChanged(target),
                PlayerEvent::StateChanged(PlayerState::Playing),
            ]
        );
    }

    #[test]
    fn failed_seek_from_eof_restores_ended_marker() {
        let player = Player::new(PlayerConfig::default());
        install_disconnected_playback(
            &player,
            PlayerState::Stopped,
            Duration::from_secs(8),
            7,
            true,
        );

        assert!(matches!(
            player.seek(Duration::from_millis(3_250)),
            Err(PlayerError::Playback(_))
        ));

        assert_eq!(player.current_media_time(), Duration::from_secs(8));
        assert_eq!(player.playback_generation(), 7);
        assert_eq!(player.state(), PlayerState::Stopped);
        assert!(player.is_ended());
    }

    #[test]
    fn stop_publishes_zero_before_stopped() {
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();
        let commands = install_fake_playback(
            &player,
            PlayerState::Playing,
            Duration::from_secs(3),
            11,
            false,
        );

        player.stop().unwrap();

        assert!(matches!(
            commands.try_recv(),
            Ok(PlaybackCommand::Stop { .. })
        ));
        assert_eq!(player.current_media_time(), Duration::ZERO);
        assert_eq!(player.playback_generation(), 12);
        assert_eq!(player.state(), PlayerState::Stopped);
        assert!(!player.is_ended());
        assert_eq!(
            events.try_iter().collect::<Vec<_>>(),
            vec![
                PlayerEvent::PositionChanged(Duration::ZERO),
                PlayerEvent::StateChanged(PlayerState::Stopped),
            ]
        );
    }

    #[test]
    fn failed_stop_rolls_back_prepublished_clock_and_generation() {
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();
        install_disconnected_playback(
            &player,
            PlayerState::Playing,
            Duration::from_secs(3),
            11,
            false,
        );

        assert!(matches!(player.stop(), Err(PlayerError::Playback(_))));

        assert_eq!(player.current_media_time(), Duration::from_secs(3));
        assert_eq!(player.playback_generation(), 11);
        assert_eq!(player.state(), PlayerState::Playing);
        assert!(!player.is_ended());
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn natural_eof_worker_events_are_ordered_once_without_generation_change() {
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();
        {
            let mut inner = player.inner.lock().expect("player mutex poisoned");
            inner.state = PlayerState::Playing;
            inner.current_media_time = Duration::from_millis(7_967);
            inner.playback_generation = 13;
        }
        let mut eof_published = false;

        publish_natural_eof_events_from_worker(
            &player.inner,
            13,
            Duration::from_secs(8),
            &mut eof_published,
        );
        publish_natural_eof_events_from_worker(
            &player.inner,
            13,
            Duration::from_secs(8),
            &mut eof_published,
        );

        assert!(eof_published);
        assert_eq!(player.current_media_time(), Duration::from_secs(8));
        assert_eq!(player.playback_generation(), 13);
        assert_eq!(player.state(), PlayerState::Stopped);
        assert!(player.is_ended());
        assert_eq!(
            events.try_iter().collect::<Vec<_>>(),
            vec![
                PlayerEvent::PositionChanged(Duration::from_secs(8)),
                PlayerEvent::StateChanged(PlayerState::Stopped),
            ]
        );
    }

    #[test]
    fn ended_play_clears_stale_audio_snapshot_before_restart() {
        let mut snapshot = Some(AudioClockSnapshot {
            media_time: Some(Duration::from_secs(8)),
            queued_duration: Some(Duration::from_millis(100)),
            queued_frames: 4_800,
            read_frames: 48_000,
            written_frames: 52_800,
            underflow_frames: 0,
        });
        let mut snapshot_at = Some(Instant::now());
        let (reply, _response) = bounded(1);
        let command = PlaybackCommand::Play {
            sequence: 1,
            generation: 1,
            reply,
        };

        observe_audio_pump_command(
            PlaybackRunState::Ended,
            &mut snapshot,
            &mut snapshot_at,
            &command,
        );

        assert!(snapshot.is_none());
        assert!(snapshot_at.is_none());
    }

    #[test]
    fn worker_clock_waits_for_prepublished_generation() {
        assert!(worker_clock_generation_is_stale(8, 7));
        assert!(!worker_clock_generation_is_stale(8, 8));
        assert!(!worker_clock_generation_is_stale(7, 8));
    }

    #[test]
    fn player_rapid_seeks_enqueue_fifo_and_publish_latest_target() {
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();
        let receiver = install_test_runtime(&player, 3);
        {
            let mut inner = player.inner.lock().expect("player mutex poisoned");
            inner.state = PlayerState::Playing;
        }
        let before_generation = player.playback_generation();
        let targets = [
            Duration::from_secs(1),
            Duration::from_secs(4),
            Duration::from_secs(2),
        ];

        for target in targets {
            player.seek(target).unwrap();
        }

        let queued_targets = receiver
            .try_iter()
            .map(|command| match command {
                PlaybackCommand::Seek { position, .. } => position,
                _ => panic!("unexpected playback command"),
            })
            .collect::<Vec<_>>();
        let published_targets = events
            .try_iter()
            .map(|event| match event {
                PlayerEvent::PositionChanged(position) => position,
                _ => panic!("unexpected player event"),
            })
            .collect::<Vec<_>>();

        assert_eq!(queued_targets, targets);
        assert_eq!(published_targets, targets);
        assert_eq!(player.current_media_time(), targets[2]);
        assert_eq!(
            player.playback_generation(),
            before_generation + targets.len() as u64
        );
        assert_eq!(player.state(), PlayerState::Playing);
    }

    #[test]
    fn player_seek_updates_shared_media_time_and_generation_immediately_when_ready() {
        let player = Player::new(PlayerConfig::default());
        let receiver = install_test_runtime(&player, 1);
        {
            let mut inner = player.inner.lock().expect("player mutex poisoned");
            inner.state = PlayerState::Ready;
        }
        let before_generation = player.playback_generation();

        player.seek(Duration::from_secs(12)).unwrap();

        assert!(matches!(
            receiver.try_recv(),
            Ok(PlaybackCommand::Seek {
                position,
                resume_after_seek: false,
                ..
            }) if position == Duration::from_secs(12)
        ));
        assert_eq!(player.current_media_time(), Duration::from_secs(12));
        assert_eq!(player.playback_generation(), before_generation + 1);
        assert_eq!(player.state(), PlayerState::Ready);
    }

    #[test]
    fn player_seek_while_paused_preserves_paused_state() {
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();
        let receiver = install_test_runtime(&player, 1);
        {
            let mut inner = player.inner.lock().expect("player mutex poisoned");
            inner.state = PlayerState::Paused;
            inner.current_media_time = Duration::from_secs(8);
        }
        let target = Duration::from_secs(23);

        player.seek(target).unwrap();

        assert!(matches!(
            receiver.try_recv(),
            Ok(PlaybackCommand::Seek {
                position,
                resume_after_seek: false,
                ..
            }) if position == target
        ));
        assert_eq!(player.current_media_time(), target);
        assert_eq!(player.state(), PlayerState::Paused);
        assert_eq!(events.try_recv(), Ok(PlayerEvent::PositionChanged(target)));
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn failed_seek_does_not_leave_clock_generation_half_updated() {
        let player = Player::new(PlayerConfig::default());
        let receiver = install_test_runtime(&player, 1);
        drop(receiver);
        {
            let mut inner = player.inner.lock().expect("player mutex poisoned");
            inner.state = PlayerState::Ready;
        }
        let before_generation = player.playback_generation();

        assert!(player.seek(Duration::from_secs(12)).is_err());

        assert_eq!(player.current_media_time(), Duration::ZERO);
        assert_eq!(player.playback_generation(), before_generation);
    }

    #[test]
    fn player_open_missing_media_reports_playback_error() {
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();

        let error = player
            .open(MediaRequest::new("/tmp/erika-definitely-missing.mp4"))
            .unwrap_err();

        assert!(matches!(error, PlayerError::Playback(_)));
        assert_eq!(
            events.recv().unwrap(),
            PlayerEvent::StateChanged(PlayerState::Opening)
        );
        assert_eq!(
            events.recv().unwrap(),
            PlayerEvent::StateChanged(PlayerState::Error)
        );
        assert!(matches!(
            events.recv().unwrap(),
            PlayerEvent::Error(PlayerError::Playback(_))
        ));
    }

    #[test]
    fn player_open_sample_emits_probe_events_when_env_is_set() {
        let Ok(sample) = std::env::var("ERIKA_TEST_SAMPLE") else {
            return;
        };
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();

        player.open(MediaRequest::new(sample)).unwrap();

        assert_eq!(
            events.recv().unwrap(),
            PlayerEvent::StateChanged(PlayerState::Opening)
        );
        assert!(matches!(
            events.recv().unwrap(),
            PlayerEvent::DurationChanged(Some(_))
        ));
        assert!(
            matches!(events.recv().unwrap(), PlayerEvent::TracksChanged(tracks) if !tracks.is_empty())
        );
        assert!(matches!(
            events.recv().unwrap(),
            PlayerEvent::TrackSelectionChanged(selection) if selection.video.is_some()
        ));
        assert!(
            matches!(events.recv().unwrap(), PlayerEvent::VideoParamsChanged(params) if params.width > 0 && params.height > 0)
        );
        assert!(matches!(
            events.recv().unwrap(),
            PlayerEvent::VideoDecoderChanged(event)
                if event.stage.starts_with("open") && event.codec.is_some()
        ));
        assert_eq!(
            events.recv().unwrap(),
            PlayerEvent::StateChanged(PlayerState::Ready)
        );
    }

    #[test]
    fn playback_generation_remains_monotonic_across_reopen_when_env_is_set() {
        let Ok(sample) = std::env::var("ERIKA_TEST_SAMPLE") else {
            return;
        };
        let player = Player::new(PlayerConfig::default());

        player.open(MediaRequest::new(sample.clone())).unwrap();
        let first_generation = player.playback_generation();
        player.open(MediaRequest::new(sample)).unwrap();

        assert!(player.playback_generation() > first_generation);
    }

    #[test]
    fn player_play_sample_emits_position_events_when_env_is_set() {
        let Ok(sample) = std::env::var("ERIKA_TEST_SAMPLE") else {
            return;
        };
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();

        player.open(MediaRequest::new(sample)).unwrap();
        while events.recv().unwrap() != PlayerEvent::StateChanged(PlayerState::Ready) {}
        player.play().unwrap();

        assert_eq!(
            events.recv().unwrap(),
            PlayerEvent::StateChanged(PlayerState::Playing)
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            match events.recv_timeout(Duration::from_millis(100)) {
                Ok(PlayerEvent::PositionChanged(position)) if position > Duration::ZERO => break,
                Ok(_) => {}
                Err(_) if std::time::Instant::now() < deadline => {}
                Err(error) => panic!("expected playback position event: {error}"),
            }
        }
        player.close().unwrap();
    }

    #[test]
    fn player_restarts_from_zero_after_sample_eof_when_env_is_set() {
        let Ok(sample) = std::env::var("ERIKA_TEST_SAMPLE") else {
            return;
        };
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();

        player.open(MediaRequest::new(sample)).unwrap();
        while events.recv().unwrap() != PlayerEvent::StateChanged(PlayerState::Ready) {}
        let duration = player.duration().expect("sample duration");
        player
            .seek(duration.saturating_add(Duration::from_secs(1)))
            .unwrap();
        player.play().unwrap();

        let eof_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match events.recv_timeout(Duration::from_millis(100)) {
                Ok(PlayerEvent::StateChanged(PlayerState::Stopped)) => break,
                Ok(PlayerEvent::Error(error)) => panic!("playback failed before EOF: {error}"),
                Ok(_) => {}
                Err(_) if Instant::now() < eof_deadline => {}
                Err(error) => panic!("expected EOF stopped state: {error}"),
            }
        }
        while events.try_recv().is_ok() {}
        let generation_at_eof = player.playback_generation();

        player.play().unwrap();

        assert_eq!(player.state(), PlayerState::Playing);
        assert!(player.playback_generation() > generation_at_eof);
        assert!(player.current_media_time() < Duration::from_secs(1));
        assert_eq!(
            events.recv_timeout(Duration::from_secs(1)).unwrap(),
            PlayerEvent::PositionChanged(Duration::ZERO)
        );
        assert_eq!(
            events.recv_timeout(Duration::from_secs(1)).unwrap(),
            PlayerEvent::StateChanged(PlayerState::Playing)
        );

        let frame_deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match events.recv_timeout(Duration::from_millis(100)) {
                Ok(PlayerEvent::PositionChanged(position)) if position > Duration::ZERO => break,
                Ok(PlayerEvent::Error(error)) => panic!("restart failed: {error}"),
                Ok(_) => {}
                Err(_) if Instant::now() < frame_deadline => {}
                Err(error) => panic!("expected position after EOF restart: {error}"),
            }
        }
        player.close().unwrap();
    }
}
