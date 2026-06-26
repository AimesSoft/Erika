use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TrySendError, bounded, unbounded};
use thiserror::Error;

use crate::audio::AudioClockSnapshot;
use crate::danmaku::DanmakuRenderPlan;
use crate::ffmpeg::{Frame, PcmAudioFrame};
use crate::overlay::OverlayFrame;
use crate::playback::{PlaybackRunState, PlaybackSessionConfig, VideoPlaybackEngine};
use crate::subtitle::{DecodedSubtitleFrame, SubtitleTrackConfig};
use crate::trace;

static NEXT_PLAYER_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_EXTERNAL_SUBTITLE_TRACK_ID: AtomicU64 = AtomicU64::new(1);
const EXTERNAL_SUBTITLE_TRACK_ID_BASE: i64 = 1_000_000;
const AUDIO_PREFILL_BURST_LIMIT: usize = 64;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PlayerError {
    #[error("player is closed")]
    Closed,
    #[error("invalid state transition from {from:?} to {to:?}")]
    InvalidStateTransition { from: PlayerState, to: PlayerState },
    #[error("renderer error: {0}")]
    Renderer(String),
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

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    StateChanged(PlayerState),
    DurationChanged(Option<Duration>),
    PositionChanged(Duration),
    TracksChanged(Vec<TrackInfo>),
    TrackSelectionChanged(TrackSelection),
    BufferingChanged(bool),
    VideoParamsChanged(VideoParams),
    SurfaceAttached(PlatformSurface),
    SurfaceDetached,
    Error(PlayerError),
}

pub struct PlayerVideoFrame {
    pub frame: Frame,
    pub pts: Option<Duration>,
    pub media_time: Duration,
    pub late_by: Option<Duration>,
    pub generation: u64,
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

    /// Render the current frame (optionally compositing `overlay`) to the attached
    /// surface. Returns `false` if there is no current frame to draw, letting the
    /// caller fall back to a test frame.
    fn render_current_frame(&mut self, context: RenderFrameContext<'_>) -> Result<bool>;

    fn runtime_stats(&self) -> RendererRuntimeStats {
        RendererRuntimeStats::default()
    }

    /// Switches the neural luma upscaler at runtime. Backends without an
    /// upscaler implementation ignore the request.
    fn set_luma_upscaler(&mut self, _mode: crate::renderer::pipeline::LumaUpscalerMode) {}
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
    pub cpu_video_frame_fallbacks: u64,
}

struct PlayerInner {
    state: PlayerState,
    media: Option<MediaRequest>,
    playback: Option<PlaybackRuntime>,
    duration: Option<Duration>,
    current_media_time: Duration,
    playback_generation: u64,
    surface: Option<PlatformSurface>,
    tracks: Vec<TrackInfo>,
    track_selection: TrackSelection,
    subscribers: Vec<Sender<PlayerEvent>>,
    video_frame_sender: Option<Sender<PlayerVideoFrame>>,
    audio_frame_sender: Option<Sender<PlayerAudioFrame>>,
    subtitle_frame_sender: Option<Sender<PlayerSubtitleFrame>>,
}

enum PlaybackCommand {
    Play,
    Pause,
    Seek(Duration),
    SetPlaybackRate(f64),
    Stop,
    AudioClock(AudioClockSnapshot),
    AddExternalSubtitle(SubtitleTrackConfig),
    RemoveSubtitleTrack(i64),
    SelectAudioTrack(Option<i64>),
    SelectSubtitleTrack(Option<i64>),
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
    ) -> Self {
        let (commands, receiver) = bounded(capacity.max(1));
        let worker = thread::Builder::new()
            .name("erika-playback".to_string())
            .spawn(move || run_playback_worker(&mut engine, inner, receiver))
            .expect("spawn playback worker");
        Self {
            commands,
            worker: Some(worker),
        }
    }

    fn shutdown(&mut self) {
        let _ = self.commands.send(PlaybackCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
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
}

impl Player {
    pub fn new(config: PlayerConfig) -> Self {
        Self {
            id: PlayerId::default(),
            config,
            inner: Arc::new(Mutex::new(PlayerInner {
                state: PlayerState::Idle,
                media: None,
                playback: None,
                duration: None,
                current_media_time: Duration::ZERO,
                playback_generation: 1,
                surface: None,
                tracks: Vec::new(),
                track_selection: TrackSelection::default(),
                subscribers: Vec::new(),
                video_frame_sender: None,
                audio_frame_sender: None,
                subtitle_frame_sender: None,
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
        let capacity = self.config.subtitle_frame_queue_capacity.max(1);
        let (sender, receiver) = bounded(capacity);
        self.inner
            .lock()
            .expect("player mutex poisoned")
            .subtitle_frame_sender = Some(sender);
        receiver
    }

    pub fn open(&self, media: MediaRequest) -> Result<()> {
        self.ensure_not_closed()?;
        self.transition(PlayerState::Opening)?;
        self.replace_playback(None);
        let engine = match VideoPlaybackEngine::open(&media, self.config.playback) {
            Ok(engine) => engine,
            Err(error) => {
                let error = PlayerError::Playback(error.to_string());
                self.transition(PlayerState::Error)?;
                self.emit(PlayerEvent::Error(error.clone()));
                return Err(error);
            }
        };
        let info = engine.info().clone();
        let runtime = PlaybackRuntime::spawn(
            engine,
            Arc::clone(&self.inner),
            self.config.event_channel_capacity,
        );
        {
            let mut inner = self.inner.lock().expect("player mutex poisoned");
            inner.media = Some(media);
            inner.playback = Some(runtime);
            inner.duration = info.duration;
            inner.current_media_time = Duration::ZERO;
            inner.playback_generation = 1;
            inner.tracks = info.tracks.clone();
            inner.track_selection = info.track_selection();
        }
        self.emit(PlayerEvent::DurationChanged(info.duration));
        self.emit(PlayerEvent::TracksChanged(info.tracks.clone()));
        self.emit(PlayerEvent::TrackSelectionChanged(info.track_selection()));
        if let Some(params) = info.video_params {
            self.emit(PlayerEvent::VideoParamsChanged(params));
        }
        self.transition(PlayerState::Ready)
    }

    pub fn play(&self) -> Result<()> {
        self.ensure_not_closed()?;
        match self.state() {
            PlayerState::Ready | PlayerState::Paused | PlayerState::Stopped => {
                self.send_playback_command(PlaybackCommand::Play)?;
                self.transition(PlayerState::Playing)
            }
            from => Err(PlayerError::InvalidStateTransition {
                from,
                to: PlayerState::Playing,
            }),
        }
    }

    pub fn pause(&self) -> Result<()> {
        self.ensure_not_closed()?;
        match self.state() {
            PlayerState::Playing => {
                self.send_playback_command(PlaybackCommand::Pause)?;
                self.transition(PlayerState::Paused)
            }
            from => Err(PlayerError::InvalidStateTransition {
                from,
                to: PlayerState::Paused,
            }),
        }
    }

    pub fn seek(&self, position: Duration) -> Result<()> {
        self.ensure_not_closed()?;
        let (previous_media_time, previous_generation, next_generation) = {
            let mut inner = self.inner.lock().expect("player mutex poisoned");
            let previous_media_time = inner.current_media_time;
            let previous_generation = inner.playback_generation;
            inner.current_media_time = position;
            inner.playback_generation = inner.playback_generation.saturating_add(1).max(1);
            (
                previous_media_time,
                previous_generation,
                inner.playback_generation,
            )
        };
        let result = self.send_playback_command(PlaybackCommand::Seek(position));
        if result.is_err() {
            let mut inner = self.inner.lock().expect("player mutex poisoned");
            inner.current_media_time = previous_media_time;
            inner.playback_generation = previous_generation;
        }
        result?;
        {
            let mut inner = self.inner.lock().expect("player mutex poisoned");
            inner.current_media_time = position;
            inner.playback_generation = inner.playback_generation.max(next_generation);
        }
        self.emit(PlayerEvent::PositionChanged(position));
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
        self.send_playback_command(PlaybackCommand::AddExternalSubtitle(config.clone()))?;
        Ok(config)
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
        self.send_playback_command(PlaybackCommand::Stop)?;
        {
            let mut inner = self.inner.lock().expect("player mutex poisoned");
            inner.current_media_time = Duration::ZERO;
            inner.playback_generation = inner.playback_generation.saturating_add(1).max(1);
        }
        self.emit(PlayerEvent::PositionChanged(Duration::ZERO));
        self.transition(PlayerState::Stopped)
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

    pub fn close(&self) -> Result<()> {
        self.replace_playback(None);
        {
            let mut inner = self.inner.lock().expect("player mutex poisoned");
            inner.media = None;
            inner.tracks.clear();
            inner.track_selection = TrackSelection::default();
            inner.duration = None;
            inner.current_media_time = Duration::ZERO;
            inner.playback_generation = inner.playback_generation.saturating_add(1).max(1);
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
        let inner = self.inner.lock().expect("player mutex poisoned");
        Ok(inner
            .playback
            .as_ref()
            .ok_or_else(|| PlayerError::Playback("no media is open".to_string()))?
            .commands
            .clone())
    }

    fn replace_playback(&self, playback: Option<PlaybackRuntime>) {
        let old = {
            let mut inner = self.inner.lock().expect("player mutex poisoned");
            std::mem::replace(&mut inner.playback, playback)
        };
        drop(old);
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
) {
    let mut last_position_event = None;
    let mut playback_generation = 1u64;
    let mut last_worker_clock = None;
    loop {
        match commands.recv_timeout(playback_poll_interval(engine.state())) {
            Ok(command) => {
                if !handle_playback_command(engine, &inner, command, &mut playback_generation) {
                    return;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }

        while let Ok(command) = commands.try_recv() {
            if !handle_playback_command(engine, &inner, command, &mut playback_generation) {
                return;
            }
        }

        if engine.should_prefill_audio() {
            pump_audio_from_worker(engine, &inner, playback_generation, "before_video_tick");
        }

        sync_playback_clock_from_worker(
            engine,
            &inner,
            playback_generation,
            "before_video_tick",
            &mut last_worker_clock,
        );

        match engine.tick() {
            Ok(Some(frame)) => {
                let position = frame.pts.unwrap_or(frame.media_time);
                last_position_event = Some(position);
                emit_video_frame_from_worker(
                    &inner,
                    PlayerVideoFrame {
                        frame: frame.frame,
                        pts: frame.pts,
                        media_time: frame.media_time,
                        late_by: frame.late_by,
                        generation: playback_generation,
                    },
                );
            }
            Ok(None) => {}
            Err(error) => {
                emit_from_worker(
                    &inner,
                    PlayerEvent::Error(PlayerError::Playback(error.to_string())),
                );
                set_state_from_worker(&inner, PlayerState::Error);
            }
        }

        sync_playback_clock_from_worker(
            engine,
            &inner,
            playback_generation,
            "after_video_tick",
            &mut last_worker_clock,
        );

        if engine.state() == PlaybackRunState::Playing {
            pump_audio_from_worker(engine, &inner, playback_generation, "after_video_tick");

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
                    emit_from_worker(
                        &inner,
                        PlayerEvent::Error(PlayerError::Playback(error.to_string())),
                    );
                    set_state_from_worker(&inner, PlayerState::Error);
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

        if engine.state() == PlaybackRunState::Ended {
            set_state_from_worker(&inner, PlayerState::Stopped);
        }

        if let Some(position) = last_position_event.take() {
            emit_from_worker(&inner, PlayerEvent::PositionChanged(position));
        }
    }
}

fn pump_audio_from_worker(
    engine: &mut VideoPlaybackEngine,
    inner: &Arc<Mutex<PlayerInner>>,
    playback_generation: u64,
    stage: &'static str,
) {
    let started = Instant::now();
    let mut emitted = 0usize;
    for _ in 0..AUDIO_PREFILL_BURST_LIMIT {
        if audio_frame_sender_is_full(inner) {
            break;
        }
        match engine.tick_audio() {
            Ok(Some(frame)) => {
                emitted += 1;
                if !emit_audio_frame_from_worker(
                    inner,
                    PlayerAudioFrame {
                        frame: frame.frame,
                        generation: playback_generation,
                    },
                ) {
                    break;
                }
            }
            Ok(None) => break,
            Err(error) => {
                emit_from_worker(
                    inner,
                    PlayerEvent::Error(PlayerError::Playback(error.to_string())),
                );
                set_state_from_worker(inner, PlayerState::Error);
                break;
            }
        }
    }
    if emitted > 0 {
        trace::log(format!(
            "[erika-clock-trace] stage=worker_audio_prefill:{stage} emitted={} elapsed_ms={:.3}",
            emitted,
            started.elapsed().as_secs_f64() * 1000.0,
        ));
    }
}

fn handle_playback_command(
    engine: &mut VideoPlaybackEngine,
    inner: &Arc<Mutex<PlayerInner>>,
    command: PlaybackCommand,
    playback_generation: &mut u64,
) -> bool {
    match command {
        PlaybackCommand::Play => engine.play(),
        PlaybackCommand::Pause => engine.pause(),
        PlaybackCommand::Seek(position) => {
            trace::log(format!(
                "[erika-clock-trace] stage=worker_command_seek target={} gen_before={}",
                trace::duration_label(Some(position)),
                *playback_generation,
            ));
            if let Err(error) = engine.seek(position) {
                emit_from_worker(
                    inner,
                    PlayerEvent::Error(PlayerError::Playback(error.to_string())),
                );
                set_state_from_worker(inner, PlayerState::Error);
            } else {
                *playback_generation = playback_generation.saturating_add(1).max(1);
                trace::log(format!(
                    "[erika-clock-trace] stage=worker_command_seek_done target={} gen_after={}",
                    trace::duration_label(Some(position)),
                    *playback_generation,
                ));
            }
        }
        PlaybackCommand::SetPlaybackRate(rate) => {
            engine.set_playback_rate(rate);
        }
        PlaybackCommand::Stop => {
            engine.stop();
            *playback_generation = playback_generation.saturating_add(1).max(1);
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
        PlaybackCommand::AddExternalSubtitle(config) => {
            match engine.add_external_subtitle(config) {
                Ok((_, clear_frame)) => {
                    *playback_generation = playback_generation.saturating_add(1).max(1);
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
                }
                Err(error) => emit_from_worker(
                    inner,
                    PlayerEvent::Error(PlayerError::Playback(error.to_string())),
                ),
            }
        }
        PlaybackCommand::RemoveSubtitleTrack(track_id) => {
            match engine.remove_subtitle_track(track_id) {
                Ok(Some(frame)) => {
                    *playback_generation = playback_generation.saturating_add(1).max(1);
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
                    sync_track_state_from_worker(inner, engine)
                }
                Err(error) => emit_from_worker(
                    inner,
                    PlayerEvent::Error(PlayerError::Playback(error.to_string())),
                ),
            }
        }
        PlaybackCommand::Shutdown => return false,
    }
    true
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

fn playback_poll_interval(state: PlaybackRunState) -> Duration {
    match state {
        PlaybackRunState::Playing => Duration::from_millis(2),
        PlaybackRunState::Paused | PlaybackRunState::Stopped | PlaybackRunState::Ended => {
            Duration::from_millis(50)
        }
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

fn audio_frame_sender_is_full(inner: &Arc<Mutex<PlayerInner>>) -> bool {
    let inner = inner.lock().expect("player mutex poisoned");
    inner
        .audio_frame_sender
        .as_ref()
        .is_none_or(|sender| sender.is_full())
}

fn emit_audio_frame_from_worker(inner: &Arc<Mutex<PlayerInner>>, frame: PlayerAudioFrame) -> bool {
    let sender = {
        let inner = inner.lock().expect("player mutex poisoned");
        let Some(sender) = inner.audio_frame_sender.as_ref() else {
            return false;
        };
        sender.clone()
    };
    if sender.send(frame).is_err() {
        let mut inner = inner.lock().expect("player mutex poisoned");
        inner.audio_frame_sender = None;
        return false;
    }
    true
}

fn emit_subtitle_frame_from_worker(inner: &Arc<Mutex<PlayerInner>>, frame: PlayerSubtitleFrame) {
    let mut inner = inner.lock().expect("player mutex poisoned");
    let Some(sender) = inner.subtitle_frame_sender.as_ref() else {
        return;
    };
    match sender.try_send(frame) {
        Ok(()) | Err(crossbeam_channel::TrySendError::Full(_)) => {}
        Err(crossbeam_channel::TrySendError::Disconnected(_)) => inner.subtitle_frame_sender = None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn player_track_cache_defaults_to_empty_selection() {
        let player = Player::new(PlayerConfig::default());

        assert!(player.tracks().is_empty());
        assert_eq!(player.track_selection(), TrackSelection::default());
    }

    #[test]
    fn player_seek_updates_shared_media_time_and_generation_immediately() {
        let player = Player::new(PlayerConfig::default());
        let receiver = {
            let (commands, receiver) = bounded(1);
            let mut inner = player.inner.lock().expect("player mutex poisoned");
            inner.state = PlayerState::Ready;
            inner.playback = Some(PlaybackRuntime {
                commands,
                worker: None,
            });
            receiver
        };
        let before_generation = player.playback_generation();

        player.seek(Duration::from_secs(12)).unwrap();

        assert!(matches!(
            receiver.try_recv(),
            Ok(PlaybackCommand::Seek(position)) if position == Duration::from_secs(12)
        ));
        assert_eq!(player.current_media_time(), Duration::from_secs(12));
        assert!(player.playback_generation() > before_generation);
    }

    #[test]
    fn failed_seek_does_not_leave_clock_generation_half_updated() {
        let player = Player::new(PlayerConfig::default());
        {
            let mut inner = player.inner.lock().expect("player mutex poisoned");
            inner.state = PlayerState::Ready;
            let (commands, _receiver) = bounded(1);
            inner.playback = Some(PlaybackRuntime {
                commands,
                worker: None,
            });
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
        assert!(
            matches!(events.recv().unwrap(), PlayerEvent::VideoParamsChanged(params) if params.width > 0 && params.height > 0)
        );
        assert_eq!(
            events.recv().unwrap(),
            PlayerEvent::StateChanged(PlayerState::Ready)
        );
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
}
