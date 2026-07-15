use std::{
    env,
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender};

#[cfg(target_os = "android")]
use crate::android::aaudio::{AAudioOutput, AAudioOutputConfig};
#[cfg(target_os = "macos")]
use crate::apple::coreaudio::{CoreAudioOutput, CoreAudioOutputConfig};
#[cfg(target_os = "ios")]
use crate::apple::iosaudio::{IosAudioQueueOutput, IosAudioQueueOutputConfig};
#[cfg(not(any(
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "windows"
)))]
use crate::audio::BufferedAudioOutput;
use crate::audio::{
    AudioClockSnapshot, AudioOutputBackend, AudioOutputRuntimeStats, AudioRingBufferConfig,
};
use crate::core::{
    AudioOutputEvent, MediaRequest, PlatformSurface, Player, PlayerAudioFrame, PlayerConfig,
    PlayerSubtitleFrame, PlayerVideoFrame, RenderFrameContext, RendererBackend,
    RendererBackendPreference, RendererRuntimeStats, TrackInfo, TrackSelection,
    VideoFrameImportFailure,
};
use crate::danmaku::{
    DANMAKU_DEBUG_BUCKETS, DanmakuDebugBucket, DanmakuLayoutConfig, DanmakuPreparedStats,
    DanmakuRenderPlan, DanmakuSession, DanmakuTimeline, DanmakuTrackInfo, DanmakuTrackSource,
    DanmakuViewport, DfmLayoutEngine, DfmPreparedLayout,
};
use crate::ffmpeg::DecoderBackend;
use crate::overlay::{OverlayFrame, OverlayTimeline, OverlayViewport};
#[cfg(any(target_os = "windows", target_os = "android"))]
use crate::playback::VideoDecodePreference;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use crate::renderer::metal::MetalRenderer;
use crate::renderer::metal::MetalRendererConfig;
#[cfg(feature = "libass")]
use crate::subtitle::{
    AssTrackResources, LibassRenderConfig, LibassSubtitleRenderer, SubtitleBitmapSet,
    SubtitleError, SubtitleRenderOutput, SubtitleRenderRequest, SubtitleRenderer,
    decoded_subtitle_frames_to_ass_script_with_style,
};
use crate::subtitle::{
    DecodedSubtitleFrame, SubtitleAssStyle, SubtitleRendererCore, SubtitleTrackConfig,
    SubtitleViewport, decoded_subtitle_frames_to_timeline,
};
use crate::trace;
#[cfg(target_os = "windows")]
use crate::windows::wasapi::{WasapiAudioOutput, WasapiAudioOutputConfig};
use crate::{PlayerError, Result};

const AUDIO_START_BUFFER: Duration = Duration::from_millis(250);
const AUDIO_PUMP_FRAME_LIMIT: usize = 16;
const AUDIO_PUMP_TIME_BUDGET: Duration = Duration::from_millis(4);
const AUDIO_FAST_RATE_PUMP_FRAME_LIMIT: usize = 48;
const AUDIO_FAST_RATE_PUMP_TIME_BUDGET: Duration = Duration::from_millis(8);
const PLAYBACK_RATE_EPSILON: f64 = 0.001;
const VIDEO_PUMP_FRAME_LIMIT: usize = 8;
const VIDEO_PUMP_TIME_BUDGET: Duration = Duration::from_millis(4);
const DANMAKU_PLAN_REQUEST_QUANTUM: Duration = Duration::from_millis(250);
const DANMAKU_PREPARE_REFRESH_MARGIN: Duration = Duration::from_secs(4);
const DANMAKU_PLAN_LOOKAHEAD: Duration = Duration::from_secs(8);
const DANMAKU_PLAN_LOOKBACK_PADDING: Duration = Duration::from_secs(2);
const DEFAULT_SUBTITLE_FONT_SCALE: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransitionFramePolicy {
    Clear,
    PreserveRendererSnapshot,
    PreserveTrackSwitchFrame,
}

#[derive(Debug, Clone)]
pub struct PresenterConfig {
    pub player: PlayerConfig,
    pub audio: PresenterAudioConfig,
    pub renderer: MetalRendererConfig,
    pub overlay: OverlayTimeline,
    pub danmaku: Option<DanmakuTimeline>,
    pub danmaku_config: DanmakuLayoutConfig,
    pub render_test_pattern_when_idle: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresenterAudioConfig {
    pub ring_buffer: AudioRingBufferConfig,
}

impl Default for PresenterAudioConfig {
    fn default() -> Self {
        #[cfg(target_os = "macos")]
        {
            let config = CoreAudioOutputConfig::default();
            Self {
                ring_buffer: config.ring_buffer,
            }
        }
        #[cfg(target_os = "ios")]
        {
            let config = IosAudioQueueOutputConfig::default();
            Self {
                ring_buffer: config.ring_buffer,
            }
        }
        #[cfg(target_os = "windows")]
        {
            let config = WasapiAudioOutputConfig::default();
            Self {
                ring_buffer: config.ring_buffer,
            }
        }
        #[cfg(target_os = "android")]
        {
            let config = AAudioOutputConfig::default();
            Self {
                ring_buffer: config.ring_buffer,
            }
        }
        #[cfg(not(any(
            target_os = "android",
            target_os = "macos",
            target_os = "ios",
            target_os = "windows"
        )))]
        {
            Self {
                ring_buffer: AudioRingBufferConfig {
                    capacity_frames: 192_000,
                    drop_oldest_on_overflow: true,
                },
            }
        }
    }
}

impl Default for PresenterConfig {
    fn default() -> Self {
        Self {
            player: PlayerConfig::default(),
            audio: PresenterAudioConfig::default(),
            renderer: MetalRendererConfig::default(),
            overlay: OverlayTimeline::default(),
            danmaku: None,
            danmaku_config: DanmakuLayoutConfig::default(),
            render_test_pattern_when_idle: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PresenterStats {
    pub decoded_video_frames: u64,
    pub rendered_video_frames: u64,
    pub rendered_test_frames: u64,
    pub pushed_audio_frames: u64,
    pub decoded_subtitle_frames: u64,
    pub overlay_frames: u64,
    pub danmaku_frames: u64,
    pub danmaku_items: u64,
    pub import_failures: u64,
    pub video_frame_backpressure_drops: u64,
    pub render_failures: u64,
    pub audio_failures: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PresenterRuntimeSnapshot {
    pub stats: PresenterStats,
    pub renderer: RendererRuntimeStats,
    pub output: crate::renderer::output::OutputRuntimeStatus,
    pub audio_output_queued_duration: Option<Duration>,
    pub audio_output_queued_frames: usize,
    pub audio_output_read_frames: u64,
    pub audio_output_written_frames: u64,
    pub audio_output_underflow_frames: u64,
    pub audio_output_runtime_stats: AudioOutputRuntimeStats,
    pub media_time: Duration,
    pub generation: u64,
    pub playing: bool,
    pub current_danmaku_items: usize,
    pub current_danmaku_atlas_version: u64,
    pub current_danmaku_atlas_bytes: usize,
    pub current_danmaku_viewport_width: u32,
    pub current_danmaku_viewport_height: u32,
    pub current_danmaku_placed_items: usize,
    pub current_danmaku_scroll_items: usize,
    pub current_danmaku_top_items: usize,
    pub current_danmaku_bottom_items: usize,
    pub current_danmaku_scroll_rows: usize,
    pub current_danmaku_scroll_track_min: usize,
    pub current_danmaku_scroll_track_max: usize,
    pub current_danmaku_scroll_min_y: f32,
    pub current_danmaku_scroll_max_y: f32,
    pub current_danmaku_scroll_bucket_count: usize,
    pub current_danmaku_scroll_buckets: [DanmakuDebugBucket; DANMAKU_DEBUG_BUCKETS],
    pub current_danmaku_prepared: DanmakuPreparedStats,
    pub last_tick_duration: Duration,
    pub last_pump_duration: Duration,
    pub last_audio_pump_duration: Duration,
    pub last_subtitle_pump_duration: Duration,
    pub last_video_pump_duration: Duration,
    pub last_clock_sync_duration: Duration,
    pub last_danmaku_plan_duration: Duration,
    pub last_render_duration: Duration,
    pub last_render_current_duration: Duration,
    pub last_render_test_duration: Duration,
}

pub struct PresenterRuntime {
    player: Player,
    renderer: Box<dyn RendererBackend>,
    video_frames: Receiver<PlayerVideoFrame>,
    audio_frames: Receiver<PlayerAudioFrame>,
    subtitle_frames: Receiver<PlayerSubtitleFrame>,
    audio_output: Box<dyn AudioOutputBackend>,
    audio_configured: bool,
    audio_started: bool,
    last_audio_clock_report: Option<AudioClockReportState>,
    last_audio_runtime_stats: AudioOutputRuntimeStats,
    playback_rate: f64,
    current_overlay: Option<OverlayFrame>,
    current_danmaku: Option<DanmakuRenderPlan>,
    current_danmaku_prepared: Option<CurrentDanmakuPrepared>,
    rejected_video_import_route: Option<RejectedVideoImportRoute>,
    current_media_time: Duration,
    current_generation: u64,
    current_output_viewport: Option<DanmakuViewport>,
    current_danmaku_viewport: Option<DanmakuViewport>,
    subtitle_font_scale: f64,
    subtitles: SubtitleFrameState,
    overlay: OverlayTimeline,
    render_test_pattern_when_idle: bool,
    danmaku_session: DanmakuSession,
    danmaku: DfmLayoutEngine,
    danmaku_planner: AsyncDanmakuPlanner,
    danmaku_generation: u64,
    danmaku_trace: DanmakuTimeTrace,
    stats: PresenterStats,
    last_tick_duration: Duration,
    last_pump_duration: Duration,
    last_audio_pump_duration: Duration,
    last_subtitle_pump_duration: Duration,
    last_video_pump_duration: Duration,
    last_clock_sync_duration: Duration,
    last_danmaku_plan_duration: Duration,
    last_render_duration: Duration,
    last_render_current_duration: Duration,
    last_render_test_duration: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RejectedVideoImportRoute {
    backend: DecoderBackend,
    mediacodec_surface: bool,
    generation: u64,
}

impl RejectedVideoImportRoute {
    fn new(backend: DecoderBackend, mediacodec_surface: bool, generation: u64) -> Self {
        Self {
            backend,
            mediacodec_surface: backend == DecoderBackend::MediaCodec && mediacodec_surface,
            generation,
        }
    }
}

fn should_reject_video_import(
    rejected: Option<RejectedVideoImportRoute>,
    candidate: RejectedVideoImportRoute,
) -> bool {
    rejected == Some(candidate)
}

fn should_report_video_frame_backpressure(drop_count: u64) -> bool {
    drop_count == 1 || drop_count.is_power_of_two()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AudioClockReportState {
    media_time: Duration,
    queued_frames: usize,
    read_frames: u64,
    underflow_frames: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct DanmakuPlanKey {
    media_time: Duration,
    viewport: DanmakuViewport,
    generation: u64,
}

#[derive(Debug, Clone, Copy)]
struct AsyncDanmakuPlanRequest {
    key: DanmakuPlanKey,
}

#[derive(Debug)]
struct AsyncDanmakuPlanResult {
    request: AsyncDanmakuPlanRequest,
    prepared: DfmPreparedLayout,
    window_start: Duration,
    window_end: Duration,
    elapsed: Duration,
}

#[derive(Debug)]
struct AsyncDanmakuPlannerState {
    revision: u64,
    config_revision: u64,
    timeline: DanmakuTimeline,
    config: DanmakuLayoutConfig,
    latest_request: Option<AsyncDanmakuPlanRequest>,
    shutdown: bool,
}

struct AsyncDanmakuPlanner {
    shared: Arc<(Mutex<AsyncDanmakuPlannerState>, Condvar)>,
    results: Receiver<AsyncDanmakuPlanResult>,
    last_requested: Option<DanmakuPlanKey>,
}

#[derive(Debug, Clone)]
struct CurrentDanmakuPrepared {
    request: AsyncDanmakuPlanRequest,
    prepared: DfmPreparedLayout,
    window_start: Duration,
    window_end: Duration,
}

impl AsyncDanmakuPlanner {
    fn new(
        engine: DfmLayoutEngine,
        timeline: DanmakuTimeline,
        config: DanmakuLayoutConfig,
    ) -> Self {
        let state = AsyncDanmakuPlannerState {
            revision: 0,
            config_revision: 1,
            timeline,
            config,
            latest_request: None,
            shutdown: false,
        };
        let shared = Arc::new((Mutex::new(state), Condvar::new()));
        let (result_tx, results) = crossbeam_channel::unbounded();
        let worker_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("erika-danmaku".to_string())
            .spawn(move || run_async_danmaku_planner(worker_shared, result_tx, engine))
            .expect("spawn erika danmaku planner");
        Self {
            shared,
            results,
            last_requested: None,
        }
    }

    fn set_timeline(&mut self, timeline: DanmakuTimeline) {
        self.update_configuration(Some(timeline), None);
    }

    fn clear_timeline(&mut self) {
        self.update_configuration(Some(DanmakuTimeline::default()), None);
    }

    fn set_config(&mut self, config: DanmakuLayoutConfig) {
        self.update_configuration(None, Some(config));
    }

    fn invalidate_requests(&mut self) {
        self.last_requested = None;
        let (lock, _) = &*self.shared;
        let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.latest_request = None;
    }

    fn request_plan(&mut self, key: DanmakuPlanKey) {
        if self.last_requested == Some(key) {
            return;
        }
        self.last_requested = Some(key);
        let request = AsyncDanmakuPlanRequest { key };
        let (lock, cvar) = &*self.shared;
        let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.latest_request = Some(request);
        state.revision = state.revision.saturating_add(1);
        cvar.notify_one();
    }

    fn try_recv(&self) -> Option<AsyncDanmakuPlanResult> {
        self.results.try_recv().ok()
    }

    fn update_configuration(
        &mut self,
        timeline: Option<DanmakuTimeline>,
        config: Option<DanmakuLayoutConfig>,
    ) {
        self.last_requested = None;
        let (lock, cvar) = &*self.shared;
        let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(timeline) = timeline {
            state.timeline = timeline;
        }
        if let Some(config) = config {
            state.config = config;
        }
        state.latest_request = None;
        state.config_revision = state.config_revision.saturating_add(1);
        state.revision = state.revision.saturating_add(1);
        cvar.notify_one();
    }
}

impl Drop for AsyncDanmakuPlanner {
    fn drop(&mut self) {
        let (lock, cvar) = &*self.shared;
        let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.shutdown = true;
        state.revision = state.revision.saturating_add(1);
        cvar.notify_one();
    }
}

#[derive(Debug, Clone)]
struct DanmakuTimeTrace {
    enabled: bool,
    log_path: Option<PathBuf>,
    samples: u64,
    last_event_time: Option<Duration>,
    last_event_generation: u64,
    last_player_time: Option<Duration>,
    last_player_generation: u64,
    last_video_time: Option<Duration>,
    last_video_generation: u64,
    last_plan_time: Option<Duration>,
    last_plan_generation: u64,
}

impl DanmakuTimeTrace {
    fn from_env() -> Self {
        let env_value = env::var("ERIKA_DANMAKU_TRACE").ok();
        let enabled = trace::env_flag("ERIKA_DANMAKU_TRACE");
        let log_path = enabled.then(|| {
            env::var_os("ERIKA_DANMAKU_TRACE_FILE")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/tmp/erika_danmaku_trace.log"))
        });
        if let Some(path) = &log_path {
            let _ = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(path)
                .and_then(|mut file| {
                    writeln!(
                        file,
                        "erika danmaku trace start pid={} debug_assertions={} env={}",
                        std::process::id(),
                        cfg!(debug_assertions),
                        env_value.as_deref().unwrap_or("<unset>"),
                    )
                });
        }
        Self {
            enabled,
            log_path,
            samples: 0,
            last_event_time: None,
            last_event_generation: 0,
            last_player_time: None,
            last_player_generation: 0,
            last_video_time: None,
            last_video_generation: 0,
            last_plan_time: None,
            last_plan_generation: 0,
        }
    }
}

impl PresenterRuntime {
    pub fn new(mut config: PresenterConfig) -> Result<Self> {
        let renderer_preference = config.player.renderer;
        let renderer = build_renderer(renderer_preference, config.renderer)?;
        let supports_mediacodec_surface = renderer.supports_mediacodec_surface_frames();
        resolve_presenter_player_config(
            &mut config.player,
            renderer_preference,
            supports_mediacodec_surface,
        );
        let player = Player::new(config.player);
        let video_frames = player.subscribe_video_frames();
        let audio_frames = player.subscribe_audio_frames();
        let subtitle_frames = player.subscribe_subtitle_frames();
        let mut danmaku_session = config
            .danmaku
            .map(DanmakuSession::from_timeline)
            .unwrap_or_default();
        let danmaku_timeline = danmaku_session.active_timeline_clone();
        let danmaku_config = config.danmaku_config;
        let danmaku = DfmLayoutEngine::new(danmaku_timeline.clone(), danmaku_config.clone());
        let danmaku_planner =
            AsyncDanmakuPlanner::new(danmaku.clone(), danmaku_timeline, danmaku_config);
        Ok(Self {
            player,
            renderer,
            video_frames,
            audio_frames,
            subtitle_frames,
            audio_output: build_audio_output(config.audio),
            audio_configured: false,
            audio_started: false,
            last_audio_clock_report: None,
            last_audio_runtime_stats: AudioOutputRuntimeStats::default(),
            playback_rate: 1.0,
            current_overlay: None,
            current_danmaku: None,
            current_danmaku_prepared: None,
            rejected_video_import_route: None,
            current_media_time: Duration::ZERO,
            current_generation: 1,
            current_output_viewport: None,
            current_danmaku_viewport: None,
            subtitle_font_scale: DEFAULT_SUBTITLE_FONT_SCALE,
            subtitles: SubtitleFrameState::default(),
            overlay: config.overlay,
            render_test_pattern_when_idle: config.render_test_pattern_when_idle,
            danmaku_session,
            danmaku,
            danmaku_planner,
            danmaku_generation: 1,
            danmaku_trace: DanmakuTimeTrace::from_env(),
            stats: PresenterStats::default(),
            last_tick_duration: Duration::ZERO,
            last_pump_duration: Duration::ZERO,
            last_audio_pump_duration: Duration::ZERO,
            last_subtitle_pump_duration: Duration::ZERO,
            last_video_pump_duration: Duration::ZERO,
            last_clock_sync_duration: Duration::ZERO,
            last_danmaku_plan_duration: Duration::ZERO,
            last_render_duration: Duration::ZERO,
            last_render_current_duration: Duration::ZERO,
            last_render_test_duration: Duration::ZERO,
        })
    }

    pub fn player(&self) -> &Player {
        &self.player
    }

    pub fn attach_surface(&mut self, surface: PlatformSurface) -> Result<()> {
        self.renderer.attach_surface(surface)?;
        if let Err(error) = self.player.attach_surface(surface) {
            let _ = self.renderer.detach_surface();
            return Err(error);
        }
        self.current_output_viewport = surface_danmaku_viewport(surface);
        self.clear_current_danmaku_state();
        Ok(())
    }

    pub fn detach_surface(&mut self) -> Result<()> {
        self.current_output_viewport = None;
        self.clear_current_danmaku_state();
        self.player.detach_surface()?;
        self.renderer.detach_surface()
    }

    pub fn resize_surface(&mut self, width: u32, height: u32, scale: f64) -> Result<()> {
        self.renderer.resize_surface(width, height, scale)?;
        self.current_output_viewport = Some(surface_dimensions_to_viewport(width, height, scale));
        self.clear_current_danmaku_state();
        self.last_audio_clock_report = None;
        Ok(())
    }

    pub fn open(&mut self, media: MediaRequest) -> Result<()> {
        self.quiesce_frame_output("open")?;
        self.reset_audio_output();
        self.clear_playback_visual_state(Duration::ZERO, TransitionFramePolicy::Clear);
        self.drain_pending_player_frames();
        self.current_generation = self.current_generation.saturating_add(1).max(1);
        let result = self.player.open(media);
        // Player::open joins the previous producer before returning, so this
        // second drain deterministically removes anything it emitted between
        // the first drain and shutdown. The new engine is still paused.
        self.drain_pending_player_frames();
        result
    }

    pub fn play(&mut self) -> Result<()> {
        if self.player.is_stopped_at_end() {
            self.reset_audio_output();
            self.drain_pending_player_frames();
            self.bump_danmaku_generation();
            self.clear_playback_visual_state(Duration::ZERO, TransitionFramePolicy::Clear);
        }
        self.player.play()
    }

    pub fn pause(&mut self) -> Result<()> {
        let result = self.player.pause();
        if let Err(error) = self.audio_output.pause() {
            self.stats.audio_failures += 1;
            eprintln!("Erika presenter audio pause failed: {error}");
        }
        self.audio_started = false;
        self.last_audio_clock_report = None;
        result
    }

    pub fn is_playing(&self) -> bool {
        self.player.state() == crate::core::PlayerState::Playing
    }

    pub fn media_time(&self) -> Duration {
        self.player.current_media_time()
    }

    pub fn duration(&self) -> Option<Duration> {
        self.player.duration()
    }

    pub fn stop(&mut self) -> Result<()> {
        let quiesced = self.quiesce_frame_output("stop")?;
        let result = self.player.stop();
        self.reset_audio_output();
        self.bump_danmaku_generation();
        self.clear_playback_visual_state(Duration::ZERO, TransitionFramePolicy::Clear);
        let transition = self.finish_frame_output_transition("stop", quiesced, true);
        result.and(transition)
    }

    pub fn close(&mut self) -> Result<()> {
        self.quiesce_frame_output("close")?;
        self.reset_audio_output();
        self.bump_danmaku_generation();
        self.clear_playback_visual_state(Duration::ZERO, TransitionFramePolicy::Clear);
        self.drain_pending_player_frames();
        let result = self.player.close();
        // Shutdown is joined at this point; no producer can refill a receiver.
        self.drain_pending_player_frames();
        result
    }

    pub fn seek(&mut self, position: Duration) -> Result<()> {
        let quiesced = self.quiesce_frame_output("seek")?;
        let result = self.player.seek(position);
        self.reset_audio_output();
        self.bump_danmaku_generation();
        self.clear_playback_visual_state(position, TransitionFramePolicy::PreserveRendererSnapshot);
        let transition = self.finish_frame_output_transition("seek", quiesced, true);
        result.and(transition)
    }

    pub fn set_playback_rate(&mut self, rate: f64) -> Result<()> {
        let next_rate = normalize_playback_rate(rate);
        self.player.set_playback_rate(next_rate)?;
        self.playback_rate = next_rate;
        self.audio_output.set_playback_rate(next_rate);
        self.last_audio_clock_report = None;
        Ok(())
    }

    pub fn set_volume(&mut self, volume: f64) {
        self.audio_output.set_volume(volume as f32);
    }

    pub fn volume(&self) -> f64 {
        self.audio_output.volume() as f64
    }

    pub fn set_subtitle_scale(&mut self, scale: f64) {
        let scale = normalize_subtitle_font_scale(scale);
        if (self.subtitle_font_scale - scale).abs() < 0.001 {
            return;
        }
        self.subtitle_font_scale = scale;
        self.refresh_current_overlay();
    }

    pub fn set_danmaku_timeline(&mut self, timeline: DanmakuTimeline) {
        self.danmaku_session.replace_default_track(
            timeline,
            "default",
            DanmakuTrackSource::Unknown,
        );
        self.sync_danmaku_engine_timeline();
        self.bump_danmaku_generation();
    }

    pub fn add_danmaku_track(
        &mut self,
        timeline: DanmakuTimeline,
        name: impl Into<String>,
        source: DanmakuTrackSource,
        offset_micros: i64,
    ) -> u64 {
        let track_id =
            self.danmaku_session
                .add_track_with_offset(timeline, name, source, offset_micros);
        self.sync_danmaku_engine_timeline();
        self.bump_danmaku_generation();
        track_id
    }

    pub fn remove_danmaku_track(&mut self, track_id: u64) -> bool {
        let removed = self.danmaku_session.remove_track(track_id);
        if removed {
            self.sync_danmaku_engine_timeline();
            self.bump_danmaku_generation();
        }
        removed
    }

    pub fn set_danmaku_track_enabled(&mut self, track_id: u64, enabled: bool) -> bool {
        let updated = self.danmaku_session.set_track_enabled(track_id, enabled);
        if updated {
            self.sync_danmaku_engine_timeline();
            self.bump_danmaku_generation();
        }
        updated
    }

    pub fn set_danmaku_track_offset(&mut self, track_id: u64, offset_micros: i64) -> bool {
        let updated = self
            .danmaku_session
            .set_track_offset(track_id, offset_micros);
        if updated {
            self.sync_danmaku_engine_timeline();
            self.bump_danmaku_generation();
        }
        updated
    }

    pub fn set_danmaku_global_offset(&mut self, offset_micros: i64) {
        self.danmaku_session.set_global_offset(offset_micros);
        self.sync_danmaku_engine_timeline();
        self.bump_danmaku_generation();
    }

    pub fn danmaku_tracks(&self) -> Vec<DanmakuTrackInfo> {
        self.danmaku_session.track_infos()
    }

    pub fn clear_danmaku(&mut self) {
        self.danmaku_session.clear();
        self.danmaku.clear_timeline();
        self.danmaku_planner.clear_timeline();
        self.clear_current_danmaku_state();
        self.bump_danmaku_generation();
    }

    pub fn set_danmaku_enabled(&mut self, enabled: bool) {
        let mut config = self.danmaku.config().clone();
        config.enabled = enabled;
        self.set_danmaku_config(config);
    }

    pub fn set_danmaku_font(&mut self, family: impl Into<String>, file_path: impl Into<String>) {
        let mut config = self.danmaku.config().clone();
        config.custom_font_family = family.into();
        config.custom_font_file_path = file_path.into();
        self.set_danmaku_config(config);
    }

    pub fn set_danmaku_config(&mut self, config: DanmakuLayoutConfig) {
        if !self.danmaku.set_config(config.clone()) {
            return;
        }
        self.danmaku_planner.set_config(config);
        self.clear_current_danmaku_state();
        self.bump_danmaku_generation();
    }

    pub fn danmaku_config(&self) -> Option<&DanmakuLayoutConfig> {
        Some(self.danmaku.config())
    }

    /// Switches the neural luma upscaler at runtime. Backends without an
    /// upscaler implementation ignore the request.
    pub fn set_luma_upscaler(&mut self, mode: crate::renderer::pipeline::LumaUpscalerMode) {
        self.renderer.set_luma_upscaler(mode);
    }

    pub fn set_output_headroom(&mut self, headroom: f32, known: bool) {
        self.renderer.set_output_headroom(headroom, known);
    }

    pub fn add_external_subtitle(&self, uri: impl Into<String>) -> Result<SubtitleTrackConfig> {
        self.player.add_external_subtitle(uri)
    }

    pub fn remove_subtitle_track(&self, track_id: i64) -> Result<()> {
        self.player.remove_subtitle_track(track_id)
    }

    pub fn select_audio_track(&mut self, track_id: Option<i64>) -> Result<()> {
        let quiesced = self.quiesce_frame_output("select_audio_track")?;
        let result = self.player.select_audio_track(track_id);
        self.reset_audio_output();
        self.bump_danmaku_generation();
        self.clear_playback_visual_state(
            self.current_media_time,
            TransitionFramePolicy::PreserveTrackSwitchFrame,
        );
        let transition = self.finish_frame_output_transition("select_audio_track", quiesced, true);
        result.and(transition)
    }

    pub fn select_subtitle_track(&mut self, track_id: Option<i64>) -> Result<()> {
        let quiesced = self.quiesce_frame_output("select_subtitle_track")?;
        let result = self.player.select_subtitle_track(track_id);
        self.reset_audio_output();
        self.bump_danmaku_generation();
        self.clear_playback_visual_state(
            self.current_media_time,
            TransitionFramePolicy::PreserveTrackSwitchFrame,
        );
        let transition =
            self.finish_frame_output_transition("select_subtitle_track", quiesced, true);
        result.and(transition)
    }

    pub fn tracks(&self) -> Vec<TrackInfo> {
        self.player.tracks()
    }

    pub fn track_selection(&self) -> TrackSelection {
        self.player.track_selection()
    }

    pub fn render_tick(&mut self, time_seconds: f64) -> Result<PresenterStats> {
        let tick_started = Instant::now();
        let pump_started = Instant::now();

        let subtitle_started = Instant::now();
        self.pump_subtitles();
        self.last_subtitle_pump_duration = subtitle_started.elapsed();

        let video_started = Instant::now();
        self.pump_video();
        self.last_video_pump_duration = video_started.elapsed();

        let audio_started = Instant::now();
        // Publish a callback-observed disconnection before clock/push recovery
        // advances the backend to recovering or stable during this tick.
        self.report_audio_output_runtime_stats();
        self.pump_audio();
        self.report_audio_output_runtime_stats();
        self.last_audio_pump_duration = audio_started.elapsed();

        let sync_started = Instant::now();
        self.sync_media_time_from_player();
        self.last_clock_sync_duration = sync_started.elapsed();

        let plan_started = Instant::now();
        self.refresh_stale_danmaku_plan();
        self.last_danmaku_plan_duration = plan_started.elapsed();
        self.last_pump_duration = pump_started.elapsed();
        let plan_time = self.current_danmaku.as_ref().map(|plan| plan.media_time);
        let plan_generation = self.current_danmaku.as_ref().map(|plan| plan.generation);
        let plan_items = self
            .current_danmaku
            .as_ref()
            .map_or(0, |plan| plan.items.len());
        self.trace_danmaku_time(
            "render_context",
            self.current_media_time,
            self.current_generation,
            Some(self.player.current_media_time()),
            None,
            plan_time,
            plan_generation,
            plan_items,
        );

        let context = RenderFrameContext::new(self.current_media_time, self.current_generation)
            .overlay(self.current_overlay.as_ref())
            .danmaku(self.current_danmaku.as_ref())
            .output_size(
                self.current_output_viewport
                    .map_or(0, |viewport| viewport.width),
                self.current_output_viewport
                    .map_or(0, |viewport| viewport.height),
            );
        let render_started = Instant::now();
        let render_result = self.renderer.render_current_frame(context);
        self.last_render_current_duration = render_started.elapsed();
        self.last_render_duration = self.last_render_current_duration;
        self.last_render_test_duration = Duration::ZERO;
        match render_result {
            Ok(true) => self.stats.rendered_video_frames += 1,
            Ok(false) => {
                if self.render_test_pattern_when_idle {
                    let render_started = Instant::now();
                    self.renderer.render_test_frame(time_seconds)?;
                    self.last_render_test_duration = render_started.elapsed();
                    self.last_render_duration = self.last_render_test_duration;
                    self.stats.rendered_test_frames += 1;
                }
            }
            Err(error) => {
                self.stats.render_failures += 1;
                return Err(error);
            }
        }

        self.last_tick_duration = tick_started.elapsed();
        if trace::enabled() {
            let renderer = self.renderer.runtime_stats();
            trace::log(format!(
                "[erika-presenter-trace] stage=render_tick media={} player={} gen={} playing={} tick_ms={:.3} pump_ms={:.3} audio_ms={:.3} subtitle_ms={:.3} video_ms={:.3} clock_ms={:.3} plan_ms={:.3} render_ms={:.3} render_current_ms={:.3} render_test_ms={:.3} stats_video={} stats_audio={} stats_subtitle={} stats_overlay={} renderer_rendered={} renderer_offscreen={} renderer_gpu_ms={:.3} audio_queued={} audio_queued_ms={} audio_underflow={} output={}x{} danmaku_items={}",
                duration_label(Some(self.current_media_time)),
                duration_label(Some(self.player.current_media_time())),
                self.current_generation,
                self.is_playing(),
                self.last_tick_duration.as_secs_f64() * 1000.0,
                self.last_pump_duration.as_secs_f64() * 1000.0,
                self.last_audio_pump_duration.as_secs_f64() * 1000.0,
                self.last_subtitle_pump_duration.as_secs_f64() * 1000.0,
                self.last_video_pump_duration.as_secs_f64() * 1000.0,
                self.last_clock_sync_duration.as_secs_f64() * 1000.0,
                self.last_danmaku_plan_duration.as_secs_f64() * 1000.0,
                self.last_render_duration.as_secs_f64() * 1000.0,
                self.last_render_current_duration.as_secs_f64() * 1000.0,
                self.last_render_test_duration.as_secs_f64() * 1000.0,
                self.stats.decoded_video_frames,
                self.stats.pushed_audio_frames,
                self.stats.decoded_subtitle_frames,
                self.stats.overlay_frames,
                renderer.rendered_frames,
                renderer.offscreen_frames,
                renderer.last_gpu_duration.as_secs_f64() * 1000.0,
                self.audio_output
                    .clock_snapshot()
                    .map(|snapshot| snapshot.queued_frames)
                    .unwrap_or(0),
                self.audio_output
                    .clock_snapshot()
                    .and_then(|snapshot| snapshot.queued_duration)
                    .map(|duration| duration.as_secs_f64() * 1000.0)
                    .unwrap_or(0.0),
                self.audio_output
                    .clock_snapshot()
                    .map(|snapshot| snapshot.underflow_frames)
                    .unwrap_or(0),
                self.current_output_viewport
                    .map_or(0, |viewport| viewport.width),
                self.current_output_viewport
                    .map_or(0, |viewport| viewport.height),
                self.current_danmaku
                    .as_ref()
                    .map_or(0, |plan| plan.items.len()),
            ));
        }
        Ok(self.stats)
    }

    pub fn capture_frame_rgba(&mut self, width: u32, height: u32) -> Result<Option<Vec<u8>>> {
        if width == 0 || height == 0 {
            return Err(PlayerError::Renderer(
                "capture size must be non-zero".to_string(),
            ));
        }

        self.pump_subtitles();
        self.pump_video();
        self.sync_media_time_from_player();
        self.refresh_stale_danmaku_plan();

        // Capture is a real render target with its own pixel viewport. Reusing
        // the surface-sized composition here makes resized screenshots either
        // drop danmaku (the renderer rejects mismatched plans) or scale a stale
        // subtitle atlas. Build both overlays for the requested target so the
        // public width/height API has the same composition semantics as an
        // equivalently sized presentation surface.
        let (capture_overlay, capture_danmaku) = self.capture_composition(width, height);

        let context = RenderFrameContext::new(self.current_media_time, self.current_generation)
            .overlay(Some(&capture_overlay))
            .danmaku(Some(&capture_danmaku))
            .output_size(width, height);
        self.renderer
            .capture_current_frame(context, width, height)
            .map(|capture| capture.map(|capture| capture.rgba))
    }

    fn capture_composition(
        &mut self,
        width: u32,
        height: u32,
    ) -> (OverlayFrame, DanmakuRenderPlan) {
        let capture_overlay_viewport = OverlayViewport::new(width, height);
        let mut capture_overlay = self
            .overlay
            .render(self.current_media_time, capture_overlay_viewport);
        let subtitle_style = self.subtitle_ass_style(capture_overlay.viewport);
        self.subtitles.append_to_overlay(
            self.current_media_time,
            &mut capture_overlay,
            subtitle_style,
        );
        let capture_danmaku = self.danmaku.render_plan(
            self.current_media_time,
            DanmakuViewport::new(width, height),
            self.current_generation,
        );
        (capture_overlay, capture_danmaku)
    }

    pub fn stats(&self) -> PresenterStats {
        self.stats
    }

    pub fn runtime_snapshot(&self) -> PresenterRuntimeSnapshot {
        let renderer = self.renderer.runtime_stats();
        let output = self.renderer.output_status();
        let (current_danmaku_items, current_danmaku_atlas_version, current_danmaku_atlas_bytes) =
            self.current_danmaku.as_ref().map_or((0, 0, 0), |plan| {
                let atlas = plan.atlas.as_ref();
                (
                    plan.items.len(),
                    atlas.map_or(0, |atlas| atlas.version),
                    atlas.map_or(0, |atlas| atlas.required_len().saturating_mul(2)),
                )
            });
        let frame_stats = self
            .current_danmaku
            .as_ref()
            .map_or(Default::default(), |plan| plan.frame_stats);
        let audio_snapshot = self.audio_output.clock_snapshot();
        PresenterRuntimeSnapshot {
            stats: self.stats,
            renderer,
            output,
            audio_output_queued_duration: audio_snapshot
                .and_then(|snapshot| snapshot.queued_duration),
            audio_output_queued_frames: audio_snapshot.map_or(0, |snapshot| snapshot.queued_frames),
            audio_output_read_frames: audio_snapshot.map_or(0, |snapshot| snapshot.read_frames),
            audio_output_written_frames: audio_snapshot
                .map_or(0, |snapshot| snapshot.written_frames),
            audio_output_underflow_frames: audio_snapshot
                .map_or(0, |snapshot| snapshot.underflow_frames),
            audio_output_runtime_stats: self.audio_output.runtime_stats(),
            media_time: self.current_media_time,
            generation: self.current_generation,
            playing: self.is_playing(),
            current_danmaku_items,
            current_danmaku_atlas_version,
            current_danmaku_atlas_bytes,
            current_danmaku_viewport_width: self
                .current_danmaku_viewport
                .map_or(0, |viewport| viewport.width),
            current_danmaku_viewport_height: self
                .current_danmaku_viewport
                .map_or(0, |viewport| viewport.height),
            current_danmaku_placed_items: frame_stats.placed_items,
            current_danmaku_scroll_items: frame_stats.scroll_items,
            current_danmaku_top_items: frame_stats.top_items,
            current_danmaku_bottom_items: frame_stats.bottom_items,
            current_danmaku_scroll_rows: frame_stats.scroll_rows,
            current_danmaku_scroll_track_min: frame_stats.scroll_track_min,
            current_danmaku_scroll_track_max: frame_stats.scroll_track_max,
            current_danmaku_scroll_min_y: frame_stats.scroll_min_y,
            current_danmaku_scroll_max_y: frame_stats.scroll_max_y,
            current_danmaku_scroll_bucket_count: frame_stats.scroll_bucket_count,
            current_danmaku_scroll_buckets: frame_stats.scroll_buckets,
            current_danmaku_prepared: frame_stats.prepared,
            last_tick_duration: self.last_tick_duration,
            last_pump_duration: self.last_pump_duration,
            last_audio_pump_duration: self.last_audio_pump_duration,
            last_subtitle_pump_duration: self.last_subtitle_pump_duration,
            last_video_pump_duration: self.last_video_pump_duration,
            last_clock_sync_duration: self.last_clock_sync_duration,
            last_danmaku_plan_duration: self.last_danmaku_plan_duration,
            last_render_duration: self.last_render_duration,
            last_render_current_duration: self.last_render_current_duration,
            last_render_test_duration: self.last_render_test_duration,
        }
    }

    fn pump_video(&mut self) {
        let started = Instant::now();
        let mut pumped = 0usize;
        loop {
            if pumped >= VIDEO_PUMP_FRAME_LIMIT || started.elapsed() >= VIDEO_PUMP_TIME_BUDGET {
                break;
            }
            match self.video_frames.try_recv() {
                Ok(frame) => {
                    if frame.generation != self.player.playback_generation() {
                        continue;
                    }
                    #[cfg(target_os = "android")]
                    let mediacodec_surface = frame.frame.is_mediacodec();
                    #[cfg(not(target_os = "android"))]
                    let mediacodec_surface = false;
                    let import_route = RejectedVideoImportRoute::new(
                        frame.decode_backend,
                        mediacodec_surface,
                        frame.generation,
                    );
                    if should_reject_video_import(self.rejected_video_import_route, import_route) {
                        continue;
                    }
                    self.stats.decoded_video_frames += 1;
                    match self.renderer.upload_player_frame(&frame) {
                        Ok(()) => {
                            if self.rejected_video_import_route.is_some() {
                                self.rejected_video_import_route = None;
                            }
                            let pts = frame.pts.unwrap_or(frame.media_time);
                            self.current_media_time = pts;
                            self.current_generation =
                                frame.generation.max(self.danmaku_generation).max(1);
                            self.update_overlay(
                                pts,
                                frame.generation,
                                frame.frame.width() as usize,
                                frame.frame.height() as usize,
                            );
                            let plan_time =
                                self.current_danmaku.as_ref().map(|plan| plan.media_time);
                            let plan_generation =
                                self.current_danmaku.as_ref().map(|plan| plan.generation);
                            let plan_items = self
                                .current_danmaku
                                .as_ref()
                                .map_or(0, |plan| plan.items.len());
                            self.trace_danmaku_time(
                                "video_frame",
                                pts,
                                self.current_generation,
                                None,
                                Some(pts),
                                plan_time,
                                plan_generation,
                                plan_items,
                            );
                            pumped += 1;
                        }
                        Err(PlayerError::RendererBackpressure(reason)) => {
                            self.stats.video_frame_backpressure_drops =
                                self.stats.video_frame_backpressure_drops.saturating_add(1);
                            let drop_count = self.stats.video_frame_backpressure_drops;
                            if should_report_video_frame_backpressure(drop_count) {
                                trace::diagnostic(
                                    serde_json::json!({
                                        "event": "video_frame_backpressure",
                                        "stage": "renderer_upload_drop",
                                        "backend": frame.decode_backend.as_str(),
                                        "mediaCodecMode": if import_route.mediacodec_surface {
                                            Some("surface_ahardwarebuffer")
                                        } else if frame.decode_backend == DecoderBackend::MediaCodec {
                                            Some("bytebuffer_cpu_upload")
                                        } else {
                                            None
                                        },
                                        "generation": frame.generation,
                                        "dropCount": drop_count,
                                        "action": "drop_current_frame_keep_decoder_and_render_previous",
                                        "reason": reason.as_str(),
                                    })
                                    .to_string(),
                                );
                            }
                            // Capacity pressure is expected under a temporarily
                            // saturated GPU. Keep the decoder route active, do not
                            // advance media time to a frame that was not uploaded,
                            // and leave queued frames for the next tick.
                            break;
                        }
                        Err(error) => {
                            self.stats.import_failures += 1;
                            let failure = VideoFrameImportFailure {
                                decode_backend: frame.decode_backend,
                                mediacodec_surface: import_route.mediacodec_surface,
                                codec: self.selected_video_codec(),
                                pixel_format: frame.frame.pixel_format(),
                                line_sizes: frame.frame.line_sizes(),
                                width: frame.frame.width(),
                                height: frame.frame.height(),
                                generation: frame.generation,
                                reason: error.to_string(),
                            };
                            trace::diagnostic(failure.structured_message());
                            if matches!(
                                frame.decode_backend,
                                DecoderBackend::MediaCodec | DecoderBackend::Software
                            ) {
                                self.rejected_video_import_route = Some(import_route);
                                // The decoder transition must not race a local
                                // frame, queued frames, or the Android recovery
                                // renderer's retained current payload.
                                drop(frame);
                                let quiesced =
                                    match self.quiesce_frame_output("video_import_failure") {
                                        Ok(quiesced) => quiesced,
                                        Err(error) => {
                                            trace::diagnostic(
                                                serde_json::json!({
                                                    "event": "video_frame_import_feedback_failed",
                                                    "backend": import_route.backend.as_str(),
                                                    "stage": "quiesce",
                                                    "reason": error.to_string(),
                                                })
                                                .to_string(),
                                            );
                                            break;
                                        }
                                    };
                                self.release_current_video_frame();
                                self.drain_pending_video_frames();
                                if let Err(report_error) =
                                    self.player.report_video_frame_import_failure(failure)
                                {
                                    trace::diagnostic(
                                        serde_json::json!({
                                            "event": "video_frame_import_feedback_failed",
                                            "backend": import_route.backend.as_str(),
                                            "reason": report_error.to_string(),
                                        })
                                        .to_string(),
                                    );
                                }
                                if let Err(error) = self.finish_frame_output_transition(
                                    "video_import_failure",
                                    quiesced,
                                    false,
                                ) {
                                    trace::diagnostic(
                                        serde_json::json!({
                                            "event": "video_frame_import_feedback_failed",
                                            "backend": import_route.backend.as_str(),
                                            "stage": "transition_finish",
                                            "reason": error.to_string(),
                                        })
                                        .to_string(),
                                    );
                                }
                                break;
                            }
                            eprintln!("Erika presenter video import failed: {error}");
                        }
                    }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => break,
            }
        }
    }

    fn selected_video_codec(&self) -> Option<String> {
        let selected = self.player.track_selection().video?;
        self.player
            .tracks()
            .into_iter()
            .find(|track| track.id == selected)
            .and_then(|track| track.codec)
    }

    fn update_overlay(&mut self, pts: Duration, generation: u64, width: usize, height: usize) {
        let viewport = DanmakuViewport::new(
            width.min(u32::MAX as usize) as u32,
            height.min(u32::MAX as usize) as u32,
        );
        let mut overlay = self
            .overlay
            .render(pts, OverlayViewport::new(viewport.width, viewport.height));
        let subtitle_style = self.subtitle_ass_style(overlay.viewport);
        self.subtitles
            .append_to_overlay(pts, &mut overlay, subtitle_style);
        if subtitle_diag_enabled() {
            eprintln!(
                "[erika-subtitle-diag] stage=update_overlay pts={} gen={} video={}x{} overlay={}",
                duration_label(Some(pts)),
                generation,
                viewport.width,
                viewport.height,
                overlay_debug_summary(&overlay),
            );
        }
        if !overlay.is_empty() {
            self.stats.overlay_frames += 1;
        }
        self.current_overlay = Some(overlay);
        let generation = generation.max(self.danmaku_generation).max(1);
        let danmaku_viewport = self.current_output_viewport.unwrap_or(viewport);
        self.current_danmaku_viewport = Some(danmaku_viewport);
        self.request_current_danmaku_plan(pts, danmaku_viewport, generation);
    }

    fn record_current_danmaku_stats(&mut self) {
        if let Some(plan) = &self.current_danmaku {
            if !plan.is_empty() {
                self.stats.danmaku_frames += 1;
                self.stats.danmaku_items += plan.items.len() as u64;
            }
        }
    }

    fn refresh_stale_danmaku_plan(&mut self) {
        self.apply_ready_danmaku_plans();
        self.refresh_current_danmaku_plan_from_prepared();
        self.request_current_danmaku_plan_for_current_time();
    }

    fn apply_ready_danmaku_plans(&mut self) {
        while let Some(result) = self.danmaku_planner.try_recv() {
            let accepted = self.danmaku_plan_result_is_current(&result);
            let prepared_items = result.prepared.items().len();
            if accepted {
                self.current_danmaku_prepared = Some(CurrentDanmakuPrepared {
                    request: result.request,
                    prepared: result.prepared,
                    window_start: result.window_start,
                    window_end: result.window_end,
                });
                self.last_danmaku_plan_duration = result.elapsed;
            }
            if self.danmaku_trace.enabled {
                self.trace_danmaku_time(
                    if accepted {
                        "prepared_async_ready"
                    } else {
                        "prepared_async_stale"
                    },
                    self.current_media_time,
                    self.current_generation,
                    None,
                    None,
                    Some(result.request.key.media_time),
                    Some(result.request.key.generation),
                    prepared_items,
                );
            }
        }
    }

    fn refresh_current_danmaku_plan_from_prepared(&mut self) {
        let Some(prepared) = &self.current_danmaku_prepared else {
            self.current_danmaku = None;
            return;
        };
        if !self.danmaku_prepared_covers_current_time(prepared) {
            self.current_danmaku = None;
            return;
        }
        let plan = self.danmaku.render_prepared_plan(
            &prepared.prepared,
            self.current_media_time,
            self.current_generation,
        );
        self.current_danmaku = Some(plan);
        self.record_current_danmaku_stats();
    }

    fn request_current_danmaku_plan_for_current_time(&mut self) {
        let Some(viewport) = self.current_danmaku_viewport else {
            return;
        };
        self.request_current_danmaku_plan(
            self.current_media_time,
            viewport,
            self.current_generation,
        );
    }

    fn request_current_danmaku_plan(
        &mut self,
        media_time: Duration,
        viewport: DanmakuViewport,
        generation: u64,
    ) {
        if self
            .current_danmaku_prepared
            .as_ref()
            .is_some_and(|prepared| {
                self.danmaku_prepared_covers_current_time(prepared)
                    && prepared.window_end.saturating_sub(media_time)
                        > DANMAKU_PREPARE_REFRESH_MARGIN
            })
        {
            return;
        }
        let key = DanmakuPlanKey {
            media_time: quantize_duration(media_time, DANMAKU_PLAN_REQUEST_QUANTUM),
            viewport,
            generation: generation.max(self.danmaku_generation).max(1),
        };
        self.danmaku_planner.request_plan(key);
    }

    fn danmaku_plan_result_is_current(&self, result: &AsyncDanmakuPlanResult) -> bool {
        let key = result.request.key;
        key.generation == self.current_generation
            && Some(key.viewport) == self.current_danmaku_viewport
            && result.window_start <= self.current_media_time
            && self.current_media_time <= result.window_end
    }

    fn danmaku_prepared_covers_current_time(&self, prepared: &CurrentDanmakuPrepared) -> bool {
        prepared.request.key.generation == self.current_generation
            && Some(prepared.request.key.viewport) == self.current_danmaku_viewport
            && prepared.window_start <= self.current_media_time
            && self.current_media_time <= prepared.window_end
    }

    fn sync_media_time_from_player(&mut self) {
        let player_time = self.player.current_media_time();
        let player_generation = self.player.playback_generation();
        self.current_generation = self
            .current_generation
            .max(player_generation)
            .max(self.danmaku_generation)
            .max(1);
        if player_time != self.current_media_time {
            self.current_media_time = player_time;
            if let Some(viewport) = self
                .current_overlay
                .as_ref()
                .map(|overlay| overlay.viewport)
            {
                let mut overlay = self.overlay.render(
                    player_time,
                    OverlayViewport::new(viewport.width, viewport.height),
                );
                let subtitle_style = self.subtitle_ass_style(overlay.viewport);
                self.subtitles
                    .append_to_overlay(player_time, &mut overlay, subtitle_style);
                if subtitle_diag_enabled() {
                    eprintln!(
                        "[erika-subtitle-diag] stage=clock_overlay player={} gen={} overlay_viewport={}x{} overlay={}",
                        duration_label(Some(player_time)),
                        self.current_generation,
                        viewport.width,
                        viewport.height,
                        overlay_debug_summary(&overlay),
                    );
                }
                self.current_overlay = Some(overlay);
            }
        }
        self.trace_danmaku_time(
            "player_clock",
            player_time,
            self.current_generation,
            Some(player_time),
            None,
            None,
            None,
            0,
        );
    }

    fn refresh_current_overlay(&mut self) {
        let Some(viewport) = self
            .current_overlay
            .as_ref()
            .map(|overlay| overlay.viewport)
        else {
            return;
        };
        let mut overlay = self.overlay.render(
            self.current_media_time,
            OverlayViewport::new(viewport.width, viewport.height),
        );
        let subtitle_style = self.subtitle_ass_style(overlay.viewport);
        self.subtitles
            .append_to_overlay(self.current_media_time, &mut overlay, subtitle_style);
        self.current_overlay = Some(overlay);
    }

    fn subtitle_ass_style(&self, viewport: OverlayViewport) -> SubtitleAssStyle {
        SubtitleAssStyle {
            font_scale: self.subtitle_font_scale,
            play_res_width: viewport.width,
            play_res_height: viewport.height,
        }
    }

    fn trace_danmaku_time(
        &mut self,
        stage: &'static str,
        media_time: Duration,
        generation: u64,
        player_time: Option<Duration>,
        video_time: Option<Duration>,
        plan_time: Option<Duration>,
        plan_generation: Option<u64>,
        plan_items: usize,
    ) {
        if !self.danmaku_trace.enabled {
            return;
        }
        let trace = &mut self.danmaku_trace;
        let event_rollback = trace.last_event_time.is_some_and(|last| {
            trace.last_event_generation == generation && duration_regressed(media_time, last)
        });
        let player_rollback = player_time.is_some_and(|time| {
            trace.last_player_generation == generation
                && trace
                    .last_player_time
                    .is_some_and(|last| duration_regressed(time, last))
        });
        let video_rollback = video_time.is_some_and(|time| {
            trace.last_video_generation == generation
                && trace
                    .last_video_time
                    .is_some_and(|last| duration_regressed(time, last))
        });
        let resolved_plan_generation = plan_generation.unwrap_or(generation);
        let plan_rollback = plan_time.is_some_and(|time| {
            trace.last_plan_generation == resolved_plan_generation
                && trace
                    .last_plan_time
                    .is_some_and(|last| duration_regressed(time, last))
        });
        let generation_changed =
            trace.last_event_generation != 0 && trace.last_event_generation != generation;
        let plan_mismatch = plan_time.is_some_and(|time| {
            time != media_time || plan_generation.is_some_and(|plan_gen| plan_gen != generation)
        });

        if trace.samples < 16
            || event_rollback
            || player_rollback
            || video_rollback
            || plan_rollback
            || generation_changed
            || plan_mismatch
        {
            let line = format!(
                "[erika-danmaku-trace] stage={stage} media={} gen={} player={} video={} plan={} plan_gen={} items={} last_event={} last_event_gen={} flags=event_back:{} player_back:{} video_back:{} plan_back:{} gen_change:{} plan_mismatch:{}",
                duration_label(Some(media_time)),
                generation,
                duration_label(player_time),
                duration_label(video_time),
                duration_label(plan_time),
                plan_generation
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                plan_items,
                duration_label(trace.last_event_time),
                trace.last_event_generation,
                event_rollback,
                player_rollback,
                video_rollback,
                plan_rollback,
                generation_changed,
                plan_mismatch,
            );
            eprintln!("{line}");
            if let Some(path) = &trace.log_path {
                let _ = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .and_then(|mut file| writeln!(file, "{line}"));
            }
            trace.samples = trace.samples.saturating_add(1);
        }

        trace.last_event_time = Some(media_time);
        trace.last_event_generation = generation;
        if let Some(time) = player_time {
            trace.last_player_time = Some(time);
            trace.last_player_generation = generation;
        }
        if let Some(time) = video_time {
            trace.last_video_time = Some(time);
            trace.last_video_generation = generation;
        }
        if let Some(time) = plan_time {
            trace.last_plan_time = Some(time);
            trace.last_plan_generation = resolved_plan_generation;
        }
    }

    fn bump_danmaku_generation(&mut self) {
        bump_generation(&mut self.current_generation, &mut self.danmaku_generation);
        self.danmaku_planner.invalidate_requests();
        self.clear_current_danmaku_state();
    }

    fn clear_current_danmaku_state(&mut self) {
        self.current_danmaku = None;
        self.current_danmaku_prepared = None;
        self.current_danmaku_viewport = None;
    }

    fn clear_playback_visual_state(
        &mut self,
        media_time: Duration,
        frame_policy: TransitionFramePolicy,
    ) {
        self.rejected_video_import_route = None;
        self.current_overlay = None;
        self.subtitles.clear();
        self.clear_current_danmaku_state();
        self.current_media_time = media_time;
        self.last_audio_clock_report = None;
        match frame_policy {
            TransitionFramePolicy::Clear => self.release_current_video_frame(),
            TransitionFramePolicy::PreserveRendererSnapshot => {
                self.preserve_current_video_frame_for_transition()
            }
            TransitionFramePolicy::PreserveTrackSwitchFrame => {
                self.preserve_current_video_frame_for_track_transition()
            }
        }
    }

    fn release_current_video_frame(&mut self) {
        if let Err(error) = self.renderer.clear_current_frame() {
            self.stats.render_failures += 1;
            eprintln!("Erika presenter renderer clear failed: {error}");
        }
    }

    fn preserve_current_video_frame_for_transition(&mut self) {
        if let Err(error) = self.renderer.preserve_current_frame_for_transition() {
            self.stats.render_failures += 1;
            trace::diagnostic(
                serde_json::json!({
                    "event": "frame_transition_snapshot",
                    "stage": "preserve_failed",
                    "reason": error.to_string(),
                    "action": "clear_current_frame",
                })
                .to_string(),
            );
            self.release_current_video_frame();
        }
    }

    fn preserve_current_video_frame_for_track_transition(&mut self) {
        if let Err(error) = self.renderer.preserve_current_frame_for_track_transition() {
            self.stats.render_failures += 1;
            trace::diagnostic(
                serde_json::json!({
                    "event": "frame_transition_snapshot",
                    "stage": "track_transition_preserve_failed",
                    "reason": error.to_string(),
                    "action": "clear_current_frame",
                })
                .to_string(),
            );
            self.release_current_video_frame();
        }
    }

    fn quiesce_frame_output(&mut self, operation: &'static str) -> Result<bool> {
        let started = Instant::now();
        trace::diagnostic(
            serde_json::json!({
                "event": "player_frame_output_transition",
                "stage": "quiesce_request",
                "operation": operation,
            })
            .to_string(),
        );
        match self.player.set_frame_output_quiesced(true) {
            Ok(active) => {
                trace::diagnostic(
                    serde_json::json!({
                        "event": "player_frame_output_transition",
                        "stage": "quiesce_acknowledged",
                        "operation": operation,
                        "active": active,
                        "elapsedMs": started.elapsed().as_millis(),
                    })
                    .to_string(),
                );
                Ok(active)
            }
            Err(error) => {
                trace::diagnostic(
                    serde_json::json!({
                        "event": "player_frame_output_transition",
                        "stage": "quiesce_failed",
                        "operation": operation,
                        "elapsedMs": started.elapsed().as_millis(),
                        "reason": error.to_string(),
                    })
                    .to_string(),
                );
                Err(error)
            }
        }
    }

    fn finish_frame_output_transition(
        &mut self,
        operation: &'static str,
        active: bool,
        drain_all_streams: bool,
    ) -> Result<()> {
        if !active {
            if drain_all_streams {
                self.drain_pending_player_frames();
            } else {
                self.drain_pending_video_frames();
            }
            return Ok(());
        }
        // A second quiesce command is a FIFO barrier behind the transition
        // command. Its ACK proves decoder replacement finished while output
        // remained disabled, so both drains are race-free.
        let barrier_started = Instant::now();
        trace::diagnostic(
            serde_json::json!({
                "event": "player_frame_output_transition",
                "stage": "transition_barrier_request",
                "operation": operation,
            })
            .to_string(),
        );
        let barrier_error = match self.player.set_frame_output_quiesced(true) {
            Ok(_) => {
                trace::diagnostic(
                    serde_json::json!({
                        "event": "player_frame_output_transition",
                        "stage": "transition_barrier_acknowledged",
                        "operation": operation,
                        "elapsedMs": barrier_started.elapsed().as_millis(),
                    })
                    .to_string(),
                );
                if drain_all_streams {
                    self.drain_pending_player_frames();
                } else {
                    self.drain_pending_video_frames();
                }
                None
            }
            Err(error) => {
                trace::diagnostic(
                    serde_json::json!({
                        "event": "player_frame_output_transition",
                        "stage": "transition_barrier_failed",
                        "operation": operation,
                        "elapsedMs": barrier_started.elapsed().as_millis(),
                        "reason": error.to_string(),
                    })
                    .to_string(),
                );
                Some(error)
            }
        };

        let resume_started = Instant::now();
        trace::diagnostic(
            serde_json::json!({
                "event": "player_frame_output_transition",
                "stage": "resume_request",
                "operation": operation,
            })
            .to_string(),
        );
        let resume_result = self.player.set_frame_output_quiesced(false);
        match &resume_result {
            Ok(_) => {
                trace::diagnostic(
                    serde_json::json!({
                        "event": "player_frame_output_transition",
                        "stage": "resume_acknowledged",
                        "operation": operation,
                        "elapsedMs": resume_started.elapsed().as_millis(),
                    })
                    .to_string(),
                );
                // If the first barrier timed out but resume was acknowledged,
                // FIFO ordering still proves the transition completed before
                // output resumed, so the pending receivers are now safe to drain.
                if barrier_error.is_some() {
                    if drain_all_streams {
                        self.drain_pending_player_frames();
                    } else {
                        self.drain_pending_video_frames();
                    }
                }
            }
            Err(error) => {
                trace::diagnostic(
                    serde_json::json!({
                        "event": "player_frame_output_transition",
                        "stage": "resume_failed",
                        "operation": operation,
                        "elapsedMs": resume_started.elapsed().as_millis(),
                        "reason": error.to_string(),
                    })
                    .to_string(),
                );
            }
        }

        match (barrier_error, resume_result) {
            (None, result) => result.map(|_| ()),
            (Some(barrier_error), Ok(_)) => Err(barrier_error),
            (Some(barrier_error), Err(resume_error)) => Err(PlayerError::Playback(format!(
                "frame output transition barrier failed: {barrier_error}; resume failed: {resume_error}",
            ))),
        }
    }

    fn sync_danmaku_engine_timeline(&mut self) {
        let timeline = self.danmaku_session.active_timeline_clone();
        self.danmaku.sync_timeline(&timeline);
        self.danmaku_planner.set_timeline(timeline);
        self.clear_current_danmaku_state();
    }

    fn pump_subtitles(&mut self) {
        loop {
            match self.subtitle_frames.try_recv() {
                Ok(frame) => {
                    if frame.generation != self.player.playback_generation() {
                        continue;
                    }
                    if subtitle_diag_enabled() {
                        eprintln!(
                            "[erika-subtitle-diag] stage=pump_subtitle gen={} track={} start={} end={} text_segments={} bitmap_planes={} empty={}",
                            frame.generation,
                            frame.frame.track_id,
                            duration_label(frame.frame.start),
                            duration_label(frame.frame.end),
                            frame.frame.text.len(),
                            frame.frame.bitmap.planes.len(),
                            frame.frame.is_empty(),
                        );
                    }
                    self.stats.decoded_subtitle_frames += 1;
                    self.subtitles.push(frame);
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => break,
            }
        }
    }

    fn pump_audio(&mut self) {
        let started = Instant::now();
        let mut pumped = 0usize;
        let (frame_limit, time_budget) = self.audio_pump_limits();
        loop {
            if pumped >= frame_limit || started.elapsed() >= time_budget {
                break;
            }
            match self.audio_frames.try_recv() {
                Ok(frame) => {
                    if frame.generation != self.player.playback_generation() {
                        continue;
                    }
                    self.push_audio(frame);
                    pumped += 1;
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => break,
            }
        }
        self.ensure_audio_started();
        if self.audio_started {
            self.report_audio_clock_snapshot();
        }
    }

    fn audio_pump_limits(&self) -> (usize, Duration) {
        if (self.playback_rate - 1.0).abs() > PLAYBACK_RATE_EPSILON {
            (
                AUDIO_FAST_RATE_PUMP_FRAME_LIMIT,
                AUDIO_FAST_RATE_PUMP_TIME_BUDGET,
            )
        } else {
            (AUDIO_PUMP_FRAME_LIMIT, AUDIO_PUMP_TIME_BUDGET)
        }
    }

    fn report_audio_clock_snapshot(&mut self) {
        let Some(snapshot) = self.audio_output.clock_snapshot() else {
            return;
        };
        // Report queue/underflow movement even when the engine later rejects the clock for sync.
        if !self.should_report_audio_clock(snapshot) {
            return;
        }
        trace::log(format!(
            "[erika-clock-trace] stage=presenter_audio_snapshot media={} queued={} queued_frames={} read={} written={} underflow={}",
            trace::duration_label(snapshot.media_time),
            trace::duration_label(snapshot.queued_duration),
            snapshot.queued_frames,
            snapshot.read_frames,
            snapshot.written_frames,
            snapshot.underflow_frames,
        ));
        let _ = self.player.update_audio_clock(snapshot);
    }

    fn should_report_audio_clock(&mut self, snapshot: AudioClockSnapshot) -> bool {
        let Some(media_time) = snapshot.media_time else {
            return false;
        };
        let next = AudioClockReportState {
            media_time,
            queued_frames: snapshot.queued_frames,
            read_frames: snapshot.read_frames,
            underflow_frames: snapshot.underflow_frames,
        };
        let should_report = self.last_audio_clock_report.is_none_or(|previous| {
            snapshot.read_frames > previous.read_frames
                || snapshot.underflow_frames > previous.underflow_frames
                || snapshot.queued_frames != previous.queued_frames
                || media_time > previous.media_time
        });
        if should_report {
            self.last_audio_clock_report = Some(next);
        }
        should_report
    }

    fn push_audio(&mut self, frame: PlayerAudioFrame) {
        if !self.audio_configured {
            if let Err(error) = self.audio_output.configure(frame.frame.format) {
                self.stats.audio_failures += 1;
                eprintln!("Erika presenter audio configure failed: {error}");
                return;
            }
            self.audio_configured = true;
            self.last_audio_clock_report = None;
        }
        match self.audio_output.push(frame.frame) {
            Ok(_) => self.stats.pushed_audio_frames += 1,
            Err(error) => {
                self.stats.audio_failures += 1;
                eprintln!("Erika presenter audio push failed: {error}");
                return;
            }
        }

        self.ensure_audio_started();
    }

    fn ensure_audio_started(&mut self) {
        if !self.is_playing()
            || self.audio_started
            || !self.audio_output_ready_to_start()
            || !self.audio_start_allowed()
        {
            return;
        }
        if let Err(error) = self.audio_output.start() {
            self.stats.audio_failures += 1;
            eprintln!("Erika presenter audio start failed: {error}");
            return;
        }
        self.audio_started = true;
        self.last_audio_clock_report = None;
    }

    fn audio_output_ready_to_start(&self) -> bool {
        self.audio_output
            .clock_snapshot()
            .and_then(|snapshot| snapshot.queued_duration)
            .is_some_and(|queued| queued >= AUDIO_START_BUFFER)
    }

    fn audio_start_allowed(&self) -> bool {
        self.player.track_selection().video.is_none() || self.stats.rendered_video_frames > 0
    }

    fn reset_audio_output(&mut self) {
        if let Err(error) = self.audio_output.stop() {
            self.stats.audio_failures += 1;
            eprintln!("Erika presenter audio reset failed: {error}");
        }
        self.audio_configured = false;
        self.audio_started = false;
        self.last_audio_clock_report = None;
    }

    fn report_audio_output_runtime_stats(&mut self) {
        let stats = self.audio_output.runtime_stats();
        if stats.transition_sequence == self.last_audio_runtime_stats.transition_sequence {
            return;
        }
        self.last_audio_runtime_stats = stats;
        let event = AudioOutputEvent { stats };
        trace::diagnostic(event.structured_message());
        self.player.report_audio_output_event(event);
    }

    fn drain_pending_player_frames(&mut self) {
        self.drain_pending_video_frames();
        while self.audio_frames.try_recv().is_ok() {}
        while self.subtitle_frames.try_recv().is_ok() {}
    }

    fn drain_pending_video_frames(&mut self) {
        while self.video_frames.try_recv().is_ok() {}
    }
}

fn normalize_playback_rate(rate: f64) -> f64 {
    if rate.is_finite() && rate > 0.0 {
        rate
    } else {
        1.0
    }
}

#[cfg(test)]
fn refresh_danmaku_plan(
    current_plan: &mut Option<DanmakuRenderPlan>,
    viewport: Option<DanmakuViewport>,
    engine: &mut DfmLayoutEngine,
    media_time: Duration,
    generation: u64,
) {
    let Some(viewport) = viewport else {
        return;
    };
    *current_plan = Some(engine.render_plan(media_time, viewport, generation));
}

fn run_async_danmaku_planner(
    shared: Arc<(Mutex<AsyncDanmakuPlannerState>, Condvar)>,
    results: Sender<AsyncDanmakuPlanResult>,
    mut engine: DfmLayoutEngine,
) {
    let (mut timeline, mut config, mut applied_config_revision) = {
        let (lock, _) = &*shared;
        let state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        (
            state.timeline.clone(),
            state.config.clone(),
            state.config_revision,
        )
    };
    let mut seen_revision = 0u64;

    loop {
        let (request, config_update) = {
            let (lock, cvar) = &*shared;
            let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            while !state.shutdown && state.revision == seen_revision {
                state = cvar
                    .wait(state)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
            if state.shutdown {
                return;
            }
            seen_revision = state.revision;
            let config_update = (state.config_revision != applied_config_revision).then(|| {
                (
                    state.config_revision,
                    state.timeline.clone(),
                    state.config.clone(),
                )
            });
            (state.latest_request, config_update)
        };

        if let Some((revision, next_timeline, next_config)) = config_update {
            timeline = next_timeline;
            config = next_config;
            engine.invalidate_stable_tracks();
            engine.set_config(config.clone());
            applied_config_revision = revision;
        }

        if let Some(request) = request {
            let started = Instant::now();
            let (window, window_start, window_end) =
                danmaku_plan_window(&timeline, request.key.media_time, &config);
            engine.sync_timeline(&window);
            let prepared = engine.prepare(request.key.viewport, request.key.generation);
            let elapsed = started.elapsed();
            if results
                .send(AsyncDanmakuPlanResult {
                    request,
                    prepared,
                    window_start,
                    window_end,
                    elapsed,
                })
                .is_err()
            {
                return;
            }
        }
    }
}

fn danmaku_plan_window(
    timeline: &DanmakuTimeline,
    media_time: Duration,
    config: &DanmakuLayoutConfig,
) -> (DanmakuTimeline, Duration, Duration) {
    let scroll_duration = if config.scroll_duration_seconds.is_finite() {
        config.scroll_duration_seconds.clamp(1.0, 60.0)
    } else {
        10.0
    };
    let lookback = Duration::from_secs_f32(scroll_duration) + DANMAKU_PLAN_LOOKBACK_PADDING;
    let start = media_time.checked_sub(lookback).unwrap_or(Duration::ZERO);
    let end = media_time + DANMAKU_PLAN_LOOKAHEAD;
    (timeline.window(start, end), start, end)
}

fn surface_danmaku_viewport(surface: PlatformSurface) -> Option<DanmakuViewport> {
    match surface {
        PlatformSurface::Metal(handle) => Some(surface_dimensions_to_viewport(
            handle.width,
            handle.height,
            handle.scale,
        )),
        PlatformSurface::Wgpu(handle) => Some(surface_dimensions_to_viewport(
            handle.width,
            handle.height,
            handle.scale,
        )),
        PlatformSurface::FlutterTexture(handle) => Some(surface_dimensions_to_viewport(
            handle.width,
            handle.height,
            handle.scale,
        )),
    }
}

fn surface_dimensions_to_viewport(width: u32, height: u32, scale: f64) -> DanmakuViewport {
    let scale = if scale.is_finite() {
        scale.max(1.0)
    } else {
        1.0
    };
    let pixel_width = ((width.max(1) as f64) * scale).round().min(u32::MAX as f64) as u32;
    let pixel_height = ((height.max(1) as f64) * scale)
        .round()
        .min(u32::MAX as f64) as u32;
    DanmakuViewport::with_scale(pixel_width, pixel_height, scale as f32)
}

fn normalize_subtitle_font_scale(scale: f64) -> f64 {
    if scale.is_finite() {
        scale.clamp(0.25, 4.0)
    } else {
        DEFAULT_SUBTITLE_FONT_SCALE
    }
}

fn bump_generation(current_generation: &mut u64, danmaku_generation: &mut u64) {
    *danmaku_generation = danmaku_generation.saturating_add(1).max(1);
    *current_generation = current_generation
        .saturating_add(1)
        .max(*danmaku_generation);
}

fn duration_regressed(next: Duration, previous: Duration) -> bool {
    previous
        .checked_sub(next)
        .is_some_and(|delta| delta > Duration::from_millis(5))
}

fn quantize_duration(value: Duration, quantum: Duration) -> Duration {
    if quantum.is_zero() {
        return value;
    }
    let quantum_micros = quantum.as_micros();
    let quantized = (value.as_micros() / quantum_micros) * quantum_micros;
    Duration::from_micros(quantized.min(u128::from(u64::MAX)) as u64)
}

fn duration_label(value: Option<Duration>) -> String {
    value
        .map(|duration| format!("{:.3}", duration.as_secs_f64()))
        .unwrap_or_else(|| "-".to_string())
}

fn subtitle_diag_enabled() -> bool {
    trace::env_flag("ERIKA_SUBTITLE_DIAG")
}

fn overlay_debug_summary(overlay: &OverlayFrame) -> String {
    let first_plane = overlay
        .subtitle_planes
        .first()
        .map(|plane| {
            let max_alpha = plane
                .rgba
                .chunks_exact(4)
                .map(|pixel| pixel[3])
                .max()
                .unwrap_or(0);
            format!(
                "first_rgba=x:{} y:{} w:{} h:{} max_a:{} bytes:{}",
                plane.x,
                plane.y,
                plane.width,
                plane.height,
                max_alpha,
                plane.rgba.len(),
            )
        })
        .unwrap_or_else(|| "first_rgba=none".to_string());
    let first_alpha = overlay
        .subtitle_alpha_planes
        .first()
        .map(|plane| {
            let max_alpha = plane.alpha.iter().copied().max().unwrap_or(0);
            format!(
                "first_alpha=x:{} y:{} w:{} h:{} max_a:{} bytes:{}",
                plane.placement.x,
                plane.placement.y,
                plane.placement.width,
                plane.placement.height,
                max_alpha,
                plane.alpha.len(),
            )
        })
        .unwrap_or_else(|| "first_alpha=none".to_string());
    format!(
        "viewport={}x{} rgba_planes={} alpha_planes={} changed={} {} {}",
        overlay.viewport.width,
        overlay.viewport.height,
        overlay.subtitle_planes.len(),
        overlay.subtitle_alpha_planes.len(),
        overlay.subtitle_changed,
        first_plane,
        first_alpha,
    )
}

impl Drop for PresenterRuntime {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn build_renderer(
    preference: RendererBackendPreference,
    _metal_config: MetalRendererConfig,
) -> Result<Box<dyn RendererBackend>> {
    match preference {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        RendererBackendPreference::PlatformNative | RendererBackendPreference::Auto => {
            Ok(Box::new(MetalRenderer::with_config(_metal_config)?))
        }
        #[cfg(target_os = "windows")]
        RendererBackendPreference::PlatformNative | RendererBackendPreference::Auto => {
            Ok(Box::new(
                crate::renderer::d3d11::D3d11Renderer::with_config(_metal_config)?,
            ))
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
        RendererBackendPreference::PlatformNative | RendererBackendPreference::Auto => {
            build_wgpu_renderer(_metal_config)
        }
        RendererBackendPreference::WgpuFallback => build_wgpu_renderer(_metal_config),
        RendererBackendPreference::FlutterTexture => Err(PlayerError::Renderer(
            "Flutter texture backend is not supported by the presenter runtime".to_string(),
        )),
    }
}

#[cfg(target_os = "windows")]
fn resolve_presenter_player_config(
    player: &mut PlayerConfig,
    renderer_preference: RendererBackendPreference,
    _supports_mediacodec_surface: bool,
) {
    if matches!(renderer_preference, RendererBackendPreference::WgpuFallback)
        && player.playback.video_decode == VideoDecodePreference::D3d11va
    {
        player.playback.video_decode = VideoDecodePreference::Software;
    }
}

#[cfg(target_os = "android")]
fn resolve_presenter_player_config(
    player: &mut PlayerConfig,
    _renderer_preference: RendererBackendPreference,
    supports_mediacodec_surface: bool,
) {
    if player.playback.video_decode == VideoDecodePreference::MediaCodec
        && !supports_mediacodec_surface
    {
        player.playback.video_decode = VideoDecodePreference::MediaCodecByteBuffer;
        trace::diagnostic(
            serde_json::json!({
                "event": "android_mediacodec_fallback",
                "stage": "renderer_capability_to_bytebuffer",
                "fromMode": "surface_ahardwarebuffer",
                "toMode": "bytebuffer_cpu_upload",
                "surfaceZeroCopyDisabled": true,
                "configurationFallback": true,
                "fallbackCount": 1,
                "reason": "active renderer does not expose Vulkan AHardwareBuffer import capability",
            })
            .to_string(),
        );
    }
}

#[cfg(not(any(target_os = "windows", target_os = "android")))]
fn resolve_presenter_player_config(
    _player: &mut PlayerConfig,
    _renderer_preference: RendererBackendPreference,
    _supports_mediacodec_surface: bool,
) {
}

fn build_audio_output(config: PresenterAudioConfig) -> Box<dyn AudioOutputBackend> {
    #[cfg(target_os = "macos")]
    {
        Box::new(CoreAudioOutput::new(CoreAudioOutputConfig {
            ring_buffer: config.ring_buffer,
        }))
    }
    #[cfg(target_os = "ios")]
    {
        Box::new(IosAudioQueueOutput::new(IosAudioQueueOutputConfig {
            ring_buffer: config.ring_buffer,
        }))
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(WasapiAudioOutput::new(WasapiAudioOutputConfig {
            ring_buffer: config.ring_buffer,
        }))
    }

    #[cfg(target_os = "android")]
    {
        Box::new(AAudioOutput::new(AAudioOutputConfig {
            ring_buffer: config.ring_buffer,
        }))
    }
    #[cfg(not(any(
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "windows"
    )))]
    {
        Box::new(BufferedAudioOutput::new(config.ring_buffer))
    }
}

#[cfg(feature = "wgpu")]
fn build_wgpu_renderer(config: MetalRendererConfig) -> Result<Box<dyn RendererBackend>> {
    #[cfg(target_os = "android")]
    let mut renderer = crate::renderer::wgpu::AndroidRecoveringWgpuRenderer::new_with_output_mode(
        config.output_mode,
    )?;
    #[cfg(not(target_os = "android"))]
    let mut renderer = crate::renderer::wgpu::WgpuRenderer::new()?;
    renderer.set_luma_upscaler(config.luma_upscaler);
    #[cfg(not(target_os = "android"))]
    if config.output_mode.is_edr() {
        trace::diagnostic(
            serde_json::json!({
                "event": "video_output_mode",
                "stage": "configuration_fallback",
                "renderer": "wgpu",
                "requested": "apple_edr",
                "active": "sdr",
                "reason": "Apple EDR is unavailable on the active wgpu platform surface",
            })
            .to_string(),
        );
    }
    Ok(Box::new(renderer))
}

#[cfg(not(feature = "wgpu"))]
fn build_wgpu_renderer(_config: MetalRendererConfig) -> Result<Box<dyn RendererBackend>> {
    Err(PlayerError::Renderer(
        "wgpu renderer backend requires the `wgpu` cargo feature".to_string(),
    ))
}

fn subtitle_is_active(frame: &PlayerSubtitleFrame, pts: Duration) -> bool {
    if frame.frame.is_empty() {
        return false;
    }
    if subtitle_start(frame).is_some_and(|start| pts < start) {
        return false;
    }
    if frame.frame.end.is_some_and(|end| pts >= end) {
        return false;
    }
    true
}

fn subtitle_start(frame: &PlayerSubtitleFrame) -> Option<Duration> {
    frame.frame.start.or(frame.pts)
}

#[derive(Debug, Default)]
struct SubtitleFrameState {
    frames: Vec<PlayerSubtitleFrame>,
    #[cfg(feature = "libass")]
    ass_renderer: CachedAssTrackRenderer,
    #[cfg(feature = "libass")]
    text_renderer: CachedLibassTextRenderer,
}

impl SubtitleFrameState {
    fn clear(&mut self) {
        self.frames.clear();
        #[cfg(feature = "libass")]
        {
            self.ass_renderer.clear();
            self.text_renderer.clear();
        }
    }

    fn push(&mut self, mut frame: PlayerSubtitleFrame) {
        self.retain_at(subtitle_start(&frame).unwrap_or(frame.media_time));
        if frame.frame.is_empty() {
            #[cfg(feature = "libass")]
            {
                self.ass_renderer.clear_track(frame.frame.track_id);
                self.text_renderer.clear();
            }
            self.frames
                .retain(|current| current.frame.track_id != frame.frame.track_id);
            return;
        }

        #[cfg(feature = "libass")]
        match self
            .ass_renderer
            .process_frame(&frame.frame, frame.generation)
        {
            Ok(true) => frame
                .frame
                .text
                .retain(|segment| segment.format != crate::subtitle::SubtitleTextFormat::Ass),
            Ok(false) => {}
            Err(error) => crate::trace::diagnostic(
                serde_json::json!({
                    "event": "subtitle_ass_track",
                    "stage": "chunk_rejected",
                    "trackId": frame.frame.track_id,
                    "generation": frame.generation,
                    "reason": error.to_string(),
                })
                .to_string(),
            ),
        }

        if frame.frame.is_empty() {
            return;
        }
        if frame.frame.end.is_none() {
            self.frames
                .retain(|current| current.frame.track_id != frame.frame.track_id);
        }
        self.frames.push(frame);
        self.frames
            .sort_by_key(|frame| subtitle_start(frame).unwrap_or(frame.media_time));
    }

    fn retain_at(&mut self, pts: Duration) {
        self.frames
            .retain(|frame| !frame.frame.is_empty() && frame.frame.end.is_none_or(|end| pts < end));
    }

    fn append_to_overlay(
        &mut self,
        pts: Duration,
        overlay: &mut OverlayFrame,
        style: SubtitleAssStyle,
    ) {
        self.retain_at(pts);
        let mut subtitle_changed = false;

        #[cfg(feature = "libass")]
        match self
            .ass_renderer
            .render(pts, overlay.viewport, style.font_scale)
        {
            Ok(Some(bitmaps)) => {
                subtitle_changed |= bitmaps.changed;
                overlay.subtitle_alpha_planes.extend(bitmaps.parts);
            }
            Ok(None) => {}
            Err(error) => crate::trace::diagnostic(
                serde_json::json!({
                    "event": "subtitle_ass_track",
                    "stage": "render_failed",
                    "reason": error.to_string(),
                })
                .to_string(),
            ),
        }

        let active = self
            .frames
            .iter()
            .filter(|frame| subtitle_is_active(frame, pts))
            .collect::<Vec<_>>();
        for frame in &active {
            if !frame.frame.bitmap.planes.is_empty() {
                overlay
                    .subtitle_planes
                    .extend(frame.frame.bitmap.planes.iter().cloned());
                subtitle_changed = true;
            }
        }

        let text_frames = active
            .iter()
            .filter(|frame| frame.frame.has_text())
            .map(|frame| frame.frame.clone())
            .collect::<Vec<_>>();
        if !text_frames.is_empty() {
            subtitle_changed |= self.append_text_subtitles(pts, overlay, &text_frames, style);
        }

        overlay.subtitle_changed |= subtitle_changed;
    }

    #[cfg(feature = "libass")]
    fn append_text_subtitles(
        &mut self,
        pts: Duration,
        overlay: &mut OverlayFrame,
        frames: &[DecodedSubtitleFrame],
        style: SubtitleAssStyle,
    ) -> bool {
        match self
            .text_renderer
            .render(pts, overlay.viewport, frames, style)
        {
            Ok(Some(bitmaps)) => {
                let changed = bitmaps.changed;
                overlay.subtitle_alpha_planes.extend(bitmaps.parts);
                changed
            }
            Ok(None) => false,
            Err(error) => {
                crate::trace::diagnostic(
                    serde_json::json!({
                        "event": "subtitle_text",
                        "stage": "render_failed",
                        "reason": error.to_string(),
                    })
                    .to_string(),
                );
                append_text_subtitles_debug(pts, overlay, frames);
                true
            }
        }
    }

    #[cfg(not(feature = "libass"))]
    fn append_text_subtitles(
        &mut self,
        pts: Duration,
        overlay: &mut OverlayFrame,
        frames: &[DecodedSubtitleFrame],
        _style: SubtitleAssStyle,
    ) -> bool {
        append_text_subtitles_debug(pts, overlay, frames);
        true
    }
}

#[cfg(feature = "libass")]
#[derive(Debug, Default)]
struct CachedAssTrackRenderer {
    generation: u64,
    track_id: Option<i64>,
    resources: Option<Arc<AssTrackResources>>,
    renderer: Option<LibassSubtitleRenderer>,
}

#[cfg(feature = "libass")]
impl CachedAssTrackRenderer {
    fn clear(&mut self) {
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.flush_events();
        }
        self.generation = 0;
        self.track_id = None;
        self.resources = None;
        self.renderer = None;
    }

    fn clear_track(&mut self, track_id: i64) {
        if self.track_id == Some(track_id) {
            self.clear();
        }
    }

    fn process_frame(
        &mut self,
        frame: &DecodedSubtitleFrame,
        generation: u64,
    ) -> crate::subtitle::Result<bool> {
        if !frame.has_ass_chunks() {
            return Ok(false);
        }
        let resources = frame.ass_track.as_ref().ok_or_else(|| {
            SubtitleError::Libass("decoded ASS event has no track resources".to_string())
        })?;
        let resources_changed = self
            .resources
            .as_ref()
            .is_none_or(|current| !Arc::ptr_eq(current, resources));
        if self.generation != generation
            || self.track_id != Some(frame.track_id)
            || resources_changed
        {
            self.clear();
            self.renderer = Some(LibassSubtitleRenderer::from_ass_track(
                frame.track_id,
                resources,
                LibassRenderConfig::default(),
            )?);
            self.generation = generation;
            self.track_id = Some(frame.track_id);
            self.resources = Some(resources.clone());
        }

        let renderer = self.renderer.as_mut().ok_or_else(|| {
            SubtitleError::Libass("ASS track renderer is unavailable".to_string())
        })?;
        let start = frame.start.unwrap_or(Duration::ZERO);
        for segment in frame
            .text
            .iter()
            .filter(|segment| segment.format == crate::subtitle::SubtitleTextFormat::Ass)
        {
            renderer.process_chunk(&segment.text, start, frame.end)?;
        }
        Ok(true)
    }

    fn render(
        &mut self,
        pts: Duration,
        viewport: OverlayViewport,
        font_scale: f64,
    ) -> crate::subtitle::Result<Option<SubtitleBitmapSet>> {
        let Some(renderer) = self.renderer.as_mut() else {
            return Ok(None);
        };
        renderer.set_font_scale(font_scale);
        match renderer.render(SubtitleRenderRequest::new(
            pts,
            viewport.width,
            viewport.height,
        ))? {
            SubtitleRenderOutput::Alpha(bitmaps) => Ok(Some(bitmaps)),
            SubtitleRenderOutput::Rgba(_) => Err(SubtitleError::Libass(
                "libass renderer returned RGBA output".to_string(),
            )),
        }
    }
}

#[cfg(feature = "libass")]
#[derive(Debug, Default)]
struct CachedLibassTextRenderer {
    script: Option<String>,
    renderer: Option<LibassSubtitleRenderer>,
}

#[cfg(feature = "libass")]
impl CachedLibassTextRenderer {
    fn clear(&mut self) {
        self.script = None;
        self.renderer = None;
    }

    fn render(
        &mut self,
        pts: Duration,
        viewport: OverlayViewport,
        frames: &[DecodedSubtitleFrame],
        style: SubtitleAssStyle,
    ) -> crate::subtitle::Result<Option<SubtitleBitmapSet>> {
        let fallback_end = pts.saturating_add(Duration::from_secs(24 * 60 * 60));
        let Some(script) =
            decoded_subtitle_frames_to_ass_script_with_style(frames.iter(), fallback_end, style)
        else {
            self.script = None;
            self.renderer = None;
            return Ok(None);
        };
        if self.script.as_ref() != Some(&script) {
            self.renderer = Some(LibassSubtitleRenderer::from_ass_script(
                script.as_bytes(),
                LibassRenderConfig::default(),
            )?);
            self.script = Some(script);
        }

        let Some(renderer) = self.renderer.as_mut() else {
            return Ok(None);
        };
        match renderer.render(SubtitleRenderRequest::new(
            pts,
            viewport.width,
            viewport.height,
        ))? {
            SubtitleRenderOutput::Alpha(bitmaps) => Ok(Some(bitmaps)),
            SubtitleRenderOutput::Rgba(_) => Err(SubtitleError::Libass(
                "libass renderer returned RGBA output".to_string(),
            )),
        }
    }
}

fn append_text_subtitles_debug(
    pts: Duration,
    overlay: &mut OverlayFrame,
    frames: &[DecodedSubtitleFrame],
) {
    let fallback_end = pts.saturating_add(Duration::from_secs(24 * 60 * 60));
    let timeline = decoded_subtitle_frames_to_timeline(frames.iter(), fallback_end);
    let frame = SubtitleRendererCore::new_debug(timeline)
        .render(
            pts,
            SubtitleViewport::new(overlay.viewport.width, overlay.viewport.height),
        )
        .frame;
    overlay.subtitle_planes.extend(frame.planes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_mediacodec_surface_route_allows_bytebuffer_recovery() {
        let rejected = RejectedVideoImportRoute::new(DecoderBackend::MediaCodec, true, 7);
        let byte_buffer = RejectedVideoImportRoute::new(DecoderBackend::MediaCodec, false, 7);

        assert!(!should_reject_video_import(Some(rejected), byte_buffer));
    }

    #[test]
    fn rejected_mediacodec_bytebuffer_route_suppresses_repeat_failures() {
        let rejected = RejectedVideoImportRoute::new(DecoderBackend::MediaCodec, false, 7);
        let next_byte_buffer = RejectedVideoImportRoute::new(DecoderBackend::MediaCodec, false, 7);
        let software = RejectedVideoImportRoute::new(DecoderBackend::Software, false, 7);

        assert!(should_reject_video_import(Some(rejected), next_byte_buffer));
        assert!(!should_reject_video_import(Some(rejected), software));
    }

    #[test]
    fn rejected_route_does_not_suppress_the_same_route_in_a_new_generation() {
        let rejected = RejectedVideoImportRoute::new(DecoderBackend::MediaCodec, true, 7);
        let reopened_surface = RejectedVideoImportRoute::new(DecoderBackend::MediaCodec, true, 8);

        assert!(!should_reject_video_import(
            Some(rejected),
            reopened_surface
        ));
    }

    #[test]
    fn video_frame_backpressure_logging_is_exponentially_throttled() {
        let reported = (1..=10)
            .filter(|count| should_report_video_frame_backpressure(*count))
            .collect::<Vec<_>>();

        assert_eq!(reported, vec![1, 2, 4, 8]);
    }
    use crate::danmaku::{DanmakuColor, DanmakuItem, DanmakuMode};
    use crate::subtitle::{
        DecodedSubtitleFrame, SubtitleBitmapPlane, SubtitleTextFormat, SubtitleTextSegment,
    };

    fn subtitle_frame(start: Duration, end: Option<Duration>) -> PlayerSubtitleFrame {
        let mut frame = DecodedSubtitleFrame::new(2, Some(start), end);
        frame.push_bitmap_plane(
            SubtitleBitmapPlane::new(0, 0, 1, 1, vec![255, 255, 255, 255]),
            false,
        );
        PlayerSubtitleFrame {
            frame,
            pts: Some(start),
            media_time: start,
            late_by: None,
            generation: 1,
        }
    }

    fn text_subtitle_frame(
        track_id: i64,
        start: Duration,
        end: Option<Duration>,
        text: &str,
    ) -> PlayerSubtitleFrame {
        let mut frame = DecodedSubtitleFrame::new(track_id, Some(start), end);
        frame.push_text(SubtitleTextSegment::new(
            SubtitleTextFormat::PlainText,
            text,
        ));
        PlayerSubtitleFrame {
            frame,
            pts: Some(start),
            media_time: start,
            late_by: None,
            generation: 1,
        }
    }

    #[cfg(feature = "libass")]
    fn ass_subtitle_frame(
        track_id: i64,
        generation: u64,
        start: Duration,
        end: Duration,
        read_order: u64,
        resources: Arc<AssTrackResources>,
    ) -> PlayerSubtitleFrame {
        let mut frame = DecodedSubtitleFrame::new(track_id, Some(start), Some(end));
        frame.push_text(SubtitleTextSegment::new(
            SubtitleTextFormat::Ass,
            format!("{read_order},0,Default,,0,0,0,,{{\\pos(100,80)\\fad(50,50)}}streamed"),
        ));
        frame.ass_track = Some(resources);
        PlayerSubtitleFrame {
            frame,
            pts: Some(start),
            media_time: start,
            late_by: None,
            generation,
        }
    }

    #[cfg(feature = "libass")]
    fn ass_test_header() -> String {
        r#"[Script Info]
ScriptType: v4.00+
PlayResX: 640
PlayResY: 360

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,32,&H00FFFFFF,&H000000FF,&H80000000,&H80000000,0,0,0,0,100,100,0,0,1,2,0,2,20,20,24,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
"#
        .to_string()
    }

    fn empty_overlay() -> OverlayFrame {
        OverlayFrame {
            pts: Duration::ZERO,
            viewport: OverlayViewport::new(640, 360),
            subtitle_planes: Vec::new(),
            subtitle_alpha_planes: Vec::new(),
            subtitle_changed: false,
        }
    }

    fn danmaku_item(id: u64, time: f64, text: &str) -> DanmakuItem {
        DanmakuItem {
            id,
            pts: Duration::from_secs_f64(time),
            text: text.to_string(),
            mode: DanmakuMode::Scroll,
            font_size: 24.0,
            color: DanmakuColor::WHITE,
            opacity: 1.0,
            is_self: false,
        }
    }

    fn danmaku_engine(text: &str) -> DfmLayoutEngine {
        let timeline = DanmakuTimeline::new(vec![danmaku_item(1, 1.0, text)]).unwrap();
        DfmLayoutEngine::new(timeline, DanmakuLayoutConfig::default())
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_wgpu_presenter_uses_software_decode_until_zero_copy_interop_exists() {
        let mut player = PlayerConfig::default();
        player.renderer = RendererBackendPreference::WgpuFallback;
        player.playback.video_decode = VideoDecodePreference::D3d11va;

        resolve_presenter_player_config(
            &mut player,
            RendererBackendPreference::WgpuFallback,
            false,
        );

        assert_eq!(
            player.playback.video_decode,
            VideoDecodePreference::Software
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_native_presenter_keeps_d3d11va_for_zero_copy_interop() {
        let mut player = PlayerConfig::default();
        player.renderer = RendererBackendPreference::PlatformNative;
        player.playback.video_decode = VideoDecodePreference::D3d11va;

        resolve_presenter_player_config(
            &mut player,
            RendererBackendPreference::PlatformNative,
            false,
        );

        assert_eq!(player.playback.video_decode, VideoDecodePreference::D3d11va);
    }

    #[cfg(target_os = "android")]
    #[test]
    fn android_presenter_uses_bytebuffer_when_ahb_interop_is_unavailable() {
        let mut player = PlayerConfig::default();
        player.playback.video_decode = VideoDecodePreference::MediaCodec;

        resolve_presenter_player_config(&mut player, RendererBackendPreference::Auto, false);

        assert_eq!(
            player.playback.video_decode,
            VideoDecodePreference::MediaCodecByteBuffer
        );
    }

    #[cfg(target_os = "android")]
    #[test]
    fn android_presenter_keeps_surface_mediacodec_when_ahb_interop_is_available() {
        let mut player = PlayerConfig::default();
        player.playback.video_decode = VideoDecodePreference::MediaCodec;

        resolve_presenter_player_config(&mut player, RendererBackendPreference::Auto, true);

        assert_eq!(
            player.playback.video_decode,
            VideoDecodePreference::MediaCodec
        );
    }

    #[cfg(target_os = "android")]
    #[test]
    fn android_presenter_preserves_explicit_non_surface_decode_modes() {
        for mode in [
            VideoDecodePreference::Software,
            VideoDecodePreference::MediaCodecByteBuffer,
        ] {
            let mut player = PlayerConfig::default();
            player.playback.video_decode = mode;

            resolve_presenter_player_config(&mut player, RendererBackendPreference::Auto, false);

            assert_eq!(player.playback.video_decode, mode);
        }
    }

    #[test]
    fn subtitle_active_window_respects_start_end_and_empty_frames() {
        let active = subtitle_frame(Duration::from_secs(1), Some(Duration::from_secs(3)));

        assert!(!subtitle_is_active(&active, Duration::from_millis(999)));
        assert!(subtitle_is_active(&active, Duration::from_secs(1)));
        assert!(subtitle_is_active(&active, Duration::from_millis(2999)));
        assert!(!subtitle_is_active(&active, Duration::from_secs(3)));

        let empty = PlayerSubtitleFrame {
            frame: DecodedSubtitleFrame::new(2, Some(Duration::ZERO), None),
            pts: Some(Duration::ZERO),
            media_time: Duration::ZERO,
            late_by: None,
            generation: 1,
        };
        assert!(!subtitle_is_active(&empty, Duration::ZERO));
    }

    #[test]
    fn subtitle_state_keeps_overlapping_bitmap_frames() {
        let mut state = SubtitleFrameState::default();
        state.push(subtitle_frame(
            Duration::from_secs(1),
            Some(Duration::from_secs(4)),
        ));
        state.push(subtitle_frame(
            Duration::from_secs(2),
            Some(Duration::from_secs(5)),
        ));
        let mut overlay = empty_overlay();

        state.append_to_overlay(
            Duration::from_secs(3),
            &mut overlay,
            SubtitleAssStyle::default(),
        );

        assert_eq!(overlay.subtitle_planes.len(), 2);
        assert!(overlay.subtitle_changed);
    }

    #[test]
    fn subtitle_state_expires_old_frames_and_empty_frame_clears_track() {
        let mut state = SubtitleFrameState::default();
        state.push(subtitle_frame(
            Duration::from_secs(1),
            Some(Duration::from_secs(2)),
        ));
        state.push(subtitle_frame(
            Duration::from_secs(3),
            Some(Duration::from_secs(5)),
        ));
        let mut overlay = empty_overlay();

        state.append_to_overlay(
            Duration::from_secs(4),
            &mut overlay,
            SubtitleAssStyle::default(),
        );

        assert_eq!(overlay.subtitle_planes.len(), 1);

        state.push(PlayerSubtitleFrame {
            frame: DecodedSubtitleFrame::new(2, Some(Duration::from_secs(4)), None),
            pts: Some(Duration::from_secs(4)),
            media_time: Duration::from_secs(4),
            late_by: None,
            generation: 1,
        });
        let mut overlay = empty_overlay();
        state.append_to_overlay(
            Duration::from_millis(4500),
            &mut overlay,
            SubtitleAssStyle::default(),
        );

        assert!(overlay.subtitle_planes.is_empty());
    }

    #[test]
    fn subtitle_state_clear_removes_open_ended_bitmap_frames() {
        let mut state = SubtitleFrameState::default();
        state.push(subtitle_frame(Duration::from_secs(1), None));
        let mut overlay = empty_overlay();

        state.append_to_overlay(
            Duration::from_secs(2),
            &mut overlay,
            SubtitleAssStyle::default(),
        );
        assert_eq!(overlay.subtitle_planes.len(), 1);

        state.clear();
        let mut overlay = empty_overlay();
        state.append_to_overlay(
            Duration::from_secs(3),
            &mut overlay,
            SubtitleAssStyle::default(),
        );

        assert!(overlay.subtitle_planes.is_empty());
    }

    #[test]
    fn subtitle_state_renders_text_frames_into_overlay() {
        let mut state = SubtitleFrameState::default();
        state.push(text_subtitle_frame(
            7,
            Duration::from_secs(1),
            Some(Duration::from_secs(3)),
            "hello",
        ));
        let mut overlay = empty_overlay();

        state.append_to_overlay(
            Duration::from_secs(2),
            &mut overlay,
            SubtitleAssStyle::default(),
        );

        #[cfg(feature = "libass")]
        assert!(!overlay.subtitle_alpha_planes.is_empty());
        #[cfg(not(feature = "libass"))]
        assert!(!overlay.subtitle_planes.is_empty());
        assert!(overlay.subtitle_changed);
    }

    #[cfg(feature = "libass")]
    #[test]
    fn subtitle_state_streams_ass_into_alpha_planes_and_clears_track() {
        let header = ass_test_header();
        let resources = Arc::new(AssTrackResources::new(
            2,
            Arc::<[u8]>::from(header.as_bytes()),
            Arc::<[crate::subtitle::SubtitleFontAttachment]>::from([]),
        ));
        let mut state = SubtitleFrameState::default();
        state.push(ass_subtitle_frame(
            2,
            3,
            Duration::ZERO,
            Duration::from_secs(2),
            7,
            resources,
        ));
        assert!(state.frames.is_empty());

        let mut overlay = empty_overlay();
        state.append_to_overlay(
            Duration::from_millis(500),
            &mut overlay,
            SubtitleAssStyle::default(),
        );
        assert!(overlay.subtitle_planes.is_empty());
        assert!(!overlay.subtitle_alpha_planes.is_empty());
        assert!(overlay.subtitle_changed);

        state.push(PlayerSubtitleFrame {
            frame: DecodedSubtitleFrame::new(2, Some(Duration::from_secs(1)), None),
            pts: Some(Duration::from_secs(1)),
            media_time: Duration::from_secs(1),
            late_by: None,
            generation: 4,
        });
        let mut cleared = empty_overlay();
        state.append_to_overlay(
            Duration::from_millis(750),
            &mut cleared,
            SubtitleAssStyle::default(),
        );
        assert!(cleared.subtitle_alpha_planes.is_empty());
        assert!(state.ass_renderer.renderer.is_none());
    }

    #[cfg(feature = "libass")]
    #[test]
    fn subtitle_state_rebuilds_ass_track_on_generation_change() {
        let header = ass_test_header();
        let resources = Arc::new(AssTrackResources::new(
            2,
            Arc::<[u8]>::from(header.as_bytes()),
            Arc::<[crate::subtitle::SubtitleFontAttachment]>::from([]),
        ));
        let mut state = SubtitleFrameState::default();
        state.push(ass_subtitle_frame(
            2,
            1,
            Duration::ZERO,
            Duration::from_secs(2),
            1,
            resources.clone(),
        ));
        state.push(ass_subtitle_frame(
            2,
            2,
            Duration::from_secs(10),
            Duration::from_secs(12),
            1,
            resources,
        ));

        assert_eq!(state.ass_renderer.generation, 2);
        let mut overlay = empty_overlay();
        state.append_to_overlay(
            Duration::from_millis(500),
            &mut overlay,
            SubtitleAssStyle::default(),
        );
        assert!(overlay.subtitle_alpha_planes.is_empty());
    }

    #[test]
    fn danmaku_generation_bump_clears_stale_plans_after_seek() {
        let mut generation = 7;
        let mut danmaku_generation = 4;

        bump_generation(&mut generation, &mut danmaku_generation);

        assert_eq!(danmaku_generation, 5);
        assert_eq!(generation, 8);
    }

    #[test]
    fn presenter_config_disables_idle_test_pattern_by_default() {
        assert!(!PresenterConfig::default().render_test_pattern_when_idle);
    }

    #[test]
    #[cfg(feature = "wgpu")]
    fn idle_tick_does_not_render_test_pattern_by_default() {
        let mut presenter = PresenterRuntime::new(PresenterConfig::default()).unwrap();

        let stats = presenter.render_tick(0.0).unwrap();

        assert_eq!(stats.rendered_test_frames, 0);
        assert_eq!(
            presenter.runtime_snapshot().last_render_test_duration,
            Duration::ZERO
        );
    }

    #[test]
    #[cfg(feature = "wgpu")]
    fn repeated_danmaku_config_does_not_bump_generation() {
        let mut presenter = PresenterRuntime::new(PresenterConfig::default()).unwrap();
        let original_generation = presenter.current_generation;

        presenter.set_danmaku_config(DanmakuLayoutConfig::default());

        assert_eq!(presenter.current_generation, original_generation);

        let mut config = DanmakuLayoutConfig::default();
        config.font_size += 1.0;
        presenter.set_danmaku_config(config.clone());
        let changed_generation = presenter.current_generation;

        assert!(changed_generation > original_generation);
        presenter.set_danmaku_config(config);

        assert_eq!(presenter.current_generation, changed_generation);
    }

    #[test]
    #[cfg(feature = "wgpu")]
    fn audio_clock_report_tracks_queue_and_underflow_changes() {
        let mut presenter = PresenterRuntime::new(PresenterConfig::default()).unwrap();

        let first = AudioClockSnapshot {
            media_time: Some(Duration::from_secs(1)),
            queued_duration: Some(Duration::from_millis(500)),
            queued_frames: 24_000,
            read_frames: 0,
            written_frames: 24_000,
            underflow_frames: 0,
        };
        assert!(presenter.should_report_audio_clock(first));
        assert!(!presenter.should_report_audio_clock(first));
        assert!(presenter.should_report_audio_clock(AudioClockSnapshot {
            media_time: Some(Duration::from_secs(1)),
            queued_duration: Some(Duration::from_millis(300)),
            queued_frames: 14_400,
            read_frames: 0,
            written_frames: 19_200,
            underflow_frames: 0,
        }));
        assert!(presenter.should_report_audio_clock(AudioClockSnapshot {
            media_time: Some(Duration::from_millis(100)),
            queued_duration: Some(Duration::ZERO),
            queued_frames: 0,
            read_frames: 0,
            written_frames: 24_000,
            underflow_frames: 512,
        }));

        presenter.playback_rate = 2.0;
        assert!(presenter.should_report_audio_clock(AudioClockSnapshot {
            media_time: Some(Duration::from_secs(1)),
            queued_duration: Some(Duration::from_millis(300)),
            queued_frames: 14_400,
            read_frames: 14_400,
            written_frames: 28_800,
            underflow_frames: 0,
        }));
    }

    #[test]
    #[cfg(feature = "wgpu")]
    fn audio_start_is_blocked_while_player_is_not_playing() {
        use crate::audio::{AudioOutputState, BufferedAudioOutput};
        use crate::ffmpeg::{PcmAudioFrame, PcmFormat};

        let mut presenter = PresenterRuntime::new(PresenterConfig::default()).unwrap();
        let format = PcmFormat::f32_interleaved(48_000, 2);
        let mut output = BufferedAudioOutput::new(AudioRingBufferConfig::default());
        output.configure(format).unwrap();
        output
            .push(PcmAudioFrame {
                format,
                pts: Some(Duration::ZERO),
                frames: 12_000,
                samples: vec![0.0; 24_000],
            })
            .unwrap();
        presenter.audio_output = Box::new(output);

        presenter.ensure_audio_started();

        assert!(!presenter.audio_started);
        assert_eq!(presenter.audio_output.state(), AudioOutputState::Stopped);
    }

    #[test]
    #[cfg(feature = "wgpu")]
    fn presenter_volume_is_clamped() {
        let mut presenter = PresenterRuntime::new(PresenterConfig::default()).unwrap();

        assert_eq!(presenter.volume(), 1.0);
        presenter.set_volume(0.4);
        assert!((presenter.volume() - 0.4).abs() < 0.000_001);
        presenter.set_volume(-1.0);
        assert_eq!(presenter.volume(), 0.0);
        presenter.set_volume(f64::NAN);
        assert_eq!(presenter.volume(), 1.0);
    }

    #[test]
    fn surface_dimensions_are_converted_to_full_output_danmaku_viewport() {
        let viewport = surface_dimensions_to_viewport(800, 450, 2.0);

        assert_eq!(viewport, DanmakuViewport::with_scale(1600, 900, 2.0));
    }

    #[test]
    #[cfg(feature = "wgpu")]
    fn resized_capture_rebuilds_subtitle_and_danmaku_for_capture_viewport() {
        let mut presenter = PresenterRuntime::new(PresenterConfig::default()).unwrap();
        presenter.subtitles.push(subtitle_frame(
            Duration::from_secs(1),
            Some(Duration::from_secs(3)),
        ));
        presenter.danmaku = danmaku_engine("capture viewport");
        presenter.current_media_time = Duration::from_millis(1500);
        presenter.current_generation = 9;

        let (overlay, danmaku) = presenter.capture_composition(320, 180);

        assert_eq!(overlay.viewport, OverlayViewport::new(320, 180));
        assert_eq!(overlay.subtitle_planes.len(), 1);
        assert_eq!(danmaku.viewport, DanmakuViewport::new(320, 180));
        assert_eq!(danmaku.generation, 9);
        assert!(!danmaku.items.is_empty());
    }

    #[test]
    fn stale_danmaku_plan_refreshes_without_new_video_frame() {
        let mut engine = danmaku_engine("first track");
        let mut current_plan = Some(engine.render_plan(
            Duration::from_millis(1500),
            DanmakuViewport::new(640, 360),
            1,
        ));
        let first_item_id = current_plan.as_ref().unwrap().items[0].item_id;

        engine.set_timeline(
            DanmakuTimeline::new(vec![danmaku_item(2, 1.0, "switched track")]).unwrap(),
        );
        refresh_danmaku_plan(
            &mut current_plan,
            Some(DanmakuViewport::new(640, 360)),
            &mut engine,
            Duration::from_millis(1500),
            2,
        );

        let refreshed = current_plan.unwrap();
        assert_eq!(first_item_id, 1);
        assert_eq!(refreshed.generation, 2);
        assert_eq!(refreshed.media_time, Duration::from_millis(1500));
        assert_eq!(refreshed.items[0].item_id, 2);
    }

    #[test]
    #[cfg(feature = "wgpu")]
    fn presenter_danmaku_session_merges_tracks_and_applies_track_controls() {
        let mut presenter = PresenterRuntime::new(PresenterConfig::default()).unwrap();
        let first = DanmakuTimeline::new(vec![danmaku_item(1, 1.0, "first")]).unwrap();
        let second = DanmakuTimeline::new(vec![danmaku_item(2, 2.0, "second")]).unwrap();

        let first_id = presenter.add_danmaku_track(first, "first", DanmakuTrackSource::Json, 0);
        let second_id =
            presenter.add_danmaku_track(second, "second", DanmakuTrackSource::Json, -1_000_000);

        assert_eq!(presenter.danmaku_tracks().len(), 2);
        let plan = presenter.danmaku.render_plan(
            Duration::from_millis(1500),
            DanmakuViewport::new(640, 360),
            1,
        );
        assert!(plan.items.iter().any(|item| item.item_id >> 48 == first_id));
        assert!(
            plan.items
                .iter()
                .any(|item| item.item_id >> 48 == second_id)
        );

        assert!(presenter.set_danmaku_track_enabled(first_id, false));
        let plan = presenter.danmaku.render_plan(
            Duration::from_millis(1500),
            DanmakuViewport::new(640, 360),
            2,
        );
        assert!(!plan.items.iter().any(|item| item.item_id >> 48 == first_id));
        assert!(
            plan.items
                .iter()
                .any(|item| item.item_id >> 48 == second_id)
        );

        assert!(presenter.remove_danmaku_track(second_id));
        assert_eq!(presenter.danmaku_tracks().len(), 1);
        assert!(presenter.remove_danmaku_track(first_id));
        assert!(presenter.danmaku_tracks().is_empty());
    }
}
