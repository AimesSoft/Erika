//! liberika as Bern's media kernel. A thread of its own owns one
//! `PresenterRuntime` per player: it performs the hub's requests and the
//! engine's own settings, draws into the Metal layer of the video view
//! that shows the player on each of that view's display frames, keeps the
//! audio going while no view shows it, and reports what it sees to the hub
//! and to the engine.

use super::{Chapter, DanmakuConfig, Kernel, Operation, OperationResult, PLAYER, Playback, Track};
use bern::media::kernel::{
    self as bern_kernel, ExecOp, ExecRequest, FrameTiming, KernelHub, MediaCapabilities,
    MediaCommand, MediaExecutor, MediaKernel, Observation, Phase, SurfaceHandle, SurfaceKind,
    SurfaceSize, VideoSurface,
};
use bern::{CommandError, Value};
use erika_protocol::erika::Enhancement;
use liberika::danmaku::{
    DanmakuColor, DanmakuItem, DanmakuLayoutConfig, DanmakuMode, DanmakuTimeline,
    DanmakuTrackSource,
};
use liberika::presenter::{PresenterConfig, PresenterRuntime};
use liberika::renderer::pipeline::LumaUpscalerMode;
use liberika::{
    MediaRequest, MetalSurfaceHandle, PlatformSurface, PlayerEvent, PlayerState, TrackInfo,
    TrackKind, TrackSelection,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often a player reports its position while nothing else changes.
const REPORT_EVERY: Duration = Duration::from_millis(100);
/// How often a player no view shows pumps its audio.
const AUDIO_EVERY: Duration = Duration::from_millis(10);

type Hub = Arc<Mutex<Option<Arc<dyn KernelHub>>>>;

pub struct Liberika {
    sender: Sender<Message>,
    hub: Hub,
    playback: Arc<Mutex<Playback>>,
    results: Arc<Mutex<Vec<OperationResult>>>,
    frames: Arc<Frames>,
}

/// The latest display frame of each view, and whether the worker was told.
#[derive(Default)]
struct Frames {
    pending: AtomicBool,
    latest: Mutex<HashMap<String, (FrameTiming, Instant)>>,
}

enum Message {
    Request(ExecRequest),
    Attach {
        view: String,
        player: String,
        surface: VideoSurface,
    },
    Resize {
        view: String,
        size: SurfaceSize,
    },
    Detach {
        view: String,
        done: Sender<()>,
    },
    Frame,
    Set(Setting),
    Close,
    Detached,
}

enum Setting {
    StartAt(f64),
    Rate(f64),
    HardwareDecode(bool),
    AudioTrack(i64),
    SubtitleTrack(Option<i64>),
    AddSubtitle(String),
    SubtitleScale(f64),
    SubtitleSupersampling(bool),
    Upscaler(LumaUpscalerMode),
    Capture(PathBuf, Option<u32>),
    Danmaku(DanmakuConfig),
    AddDanmaku(String, DanmakuTimeline),
    SendDanmaku(String, u32),
}

impl Liberika {
    /// Start the kernel's thread and install the kernel in Bern.
    pub fn install() -> Arc<dyn Kernel> {
        let (sender, receiver) = channel();
        let hub: Hub = Arc::default();
        let playback = Arc::new(Mutex::new(Playback::default()));
        let results = Arc::new(Mutex::new(Vec::new()));
        let frames = Arc::new(Frames::default());
        let shared = (
            Arc::clone(&hub),
            Arc::clone(&playback),
            Arc::clone(&frames),
            Arc::clone(&results),
        );
        // Presenters stay on the thread that creates them.
        std::thread::Builder::new()
            .name("liberika".into())
            .spawn(move || {
                let (hub, playback, frames, results) = shared;
                Worker {
                    receiver,
                    hub,
                    playback,
                    results,
                    frames,
                    players: HashMap::new(),
                    views: Vec::new(),
                    settings: Settings::default(),
                }
                .run()
            })
            .expect("start the liberika thread");
        let kernel = Arc::new(Liberika {
            sender,
            hub,
            playback,
            frames,
            results,
        });
        bern_kernel::install(kernel.clone());
        kernel
    }

    fn send(&self, message: Message) {
        let _ = self.sender.send(message);
    }

    /// Send a standard media command through the hub, as every entry does,
    /// so it is validated, versioned and seen by all of them.
    fn dispatch(&self, command: MediaCommand, params: Vec<Value>) {
        let Some(hub) = self.hub.lock().expect("hub lock").clone() else {
            return;
        };
        hub.dispatch(
            command,
            params,
            Box::new(move |result| {
                if let Err(error) = result {
                    eprintln!(
                        "liberika: {} failed: {} {}",
                        command.id(),
                        error.code,
                        error.message
                    );
                }
            }),
        );
    }
}

fn player() -> Value {
    Value::Text(PLAYER.into())
}

impl Kernel for Liberika {
    fn ready(&self) -> bool {
        self.hub.lock().expect("hub lock").is_some()
    }

    fn open(&self, url: &str, start: f64) {
        // The previous media is gone from what the engine reads at once.
        *self.playback.lock().expect("playback lock") = Playback::default();
        self.send(Message::Set(Setting::StartAt(start)));
        self.dispatch(
            MediaCommand::Open,
            vec![player(), Value::Record(vec![Value::Text(url.into())])],
        );
        self.dispatch(MediaCommand::Play, vec![player()]);
    }

    fn play(&self) {
        self.dispatch(MediaCommand::Play, vec![player()]);
    }

    fn pause(&self) {
        self.dispatch(MediaCommand::Pause, vec![player()]);
    }

    fn seek(&self, seconds: f64) {
        let Some(duration) = self.playback().duration else {
            return;
        };
        let position = (seconds.clamp(0.0, duration) * 1000.0) as i64;
        self.dispatch(MediaCommand::Seek, vec![player(), Value::Int(position)]);
    }

    fn set_volume(&self, volume: f64) {
        self.dispatch(
            MediaCommand::SetVolume,
            vec![player(), Value::Float(volume.clamp(0.0, 1.0))],
        );
    }

    fn set_rate(&self, rate: f64) {
        self.send(Message::Set(Setting::Rate(rate)));
    }

    fn set_hardware_decode(&self, enabled: bool) {
        self.send(Message::Set(Setting::HardwareDecode(enabled)));
    }

    fn select_audio(&self, id: i64) {
        self.send(Message::Set(Setting::AudioTrack(id)));
    }

    fn select_subtitle(&self, id: Option<i64>) {
        self.send(Message::Set(Setting::SubtitleTrack(id)));
    }

    fn add_subtitle(&self, url: &str) {
        self.send(Message::Set(Setting::AddSubtitle(url.into())));
    }

    fn set_subtitle_scale(&self, scale: f64) {
        self.send(Message::Set(Setting::SubtitleScale(scale)));
    }

    fn set_subtitle_supersampling(&self, enabled: bool) {
        self.send(Message::Set(Setting::SubtitleSupersampling(enabled)));
    }

    fn set_enhancement(&self, enhancement: Enhancement) {
        let mode = match enhancement {
            Enhancement::Off => LumaUpscalerMode::Off,
            Enhancement::ArtCnn => LumaUpscalerMode::ArtCnnC4F16,
            Enhancement::ArtCnnQuality => LumaUpscalerMode::ArtCnnC4F32,
            Enhancement::ArtCnnDenoise => LumaUpscalerMode::ArtCnnC4F16Ds,
        };
        self.send(Message::Set(Setting::Upscaler(mode)));
    }

    fn set_danmaku(&self, config: &DanmakuConfig) {
        self.send(Message::Set(Setting::Danmaku(config.clone())));
    }

    fn add_danmaku(&self, name: &str, contents: &str) -> Result<usize, String> {
        let timeline = DanmakuTimeline::parse_auto(contents).map_err(|error| error.to_string())?;
        let count = timeline.len();
        self.send(Message::Set(Setting::AddDanmaku(name.into(), timeline)));
        Ok(count)
    }

    fn send_danmaku(&self, text: &str, rgb: u32) {
        self.send(Message::Set(Setting::SendDanmaku(text.into(), rgb)));
    }

    fn capture(&self, path: &Path, width: Option<u32>) {
        self.send(Message::Set(Setting::Capture(path.to_path_buf(), width)));
    }

    fn close(&self) {
        self.send(Message::Close);
    }

    fn take_results(&self) -> Vec<OperationResult> {
        std::mem::take(&mut *self.results.lock().expect("result lock"))
    }

    fn playback(&self) -> Playback {
        self.playback.lock().expect("playback lock").clone()
    }
}

impl MediaExecutor for Liberika {
    fn execute(&self, request: ExecRequest) {
        self.send(Message::Request(request));
    }
}

impl MediaKernel for Liberika {
    fn capabilities(&self) -> MediaCapabilities {
        MediaCapabilities {
            backend: "liberika".into(),
            backend_version: "0.2".into(),
            background_playback: true,
            multi_view: false,
        }
    }

    fn surfaces(&self) -> Vec<SurfaceKind> {
        vec![SurfaceKind::MetalLayer]
    }

    fn attached(&self, hub: Arc<dyn KernelHub>) {
        *self.hub.lock().expect("hub lock") = Some(hub);
    }

    fn detached(&self) {
        *self.hub.lock().expect("hub lock") = None;
        self.send(Message::Detached);
    }

    fn attach_surface(&self, view: &str, player: &str, surface: VideoSurface) {
        self.send(Message::Attach {
            view: view.into(),
            player: player.into(),
            surface,
        });
    }

    fn resize_surface(&self, view: &str, size: SurfaceSize) {
        self.send(Message::Resize {
            view: view.into(),
            size,
        });
    }

    fn detach_surface(&self, view: &str) {
        let (done, detached) = channel();
        self.send(Message::Detach {
            view: view.into(),
            done,
        });
        // The view frees its layer next; a frame in flight finishes first.
        let _ = detached.recv_timeout(Duration::from_secs(2));
    }

    fn frame(&self, view: &str, timing: FrameTiming) {
        self.frames
            .latest
            .lock()
            .expect("frames lock")
            .insert(view.to_string(), (timing, Instant::now()));
        if !self.frames.pending.swap(true, Ordering::AcqRel) {
            self.send(Message::Frame);
        }
    }
}

/// Erika's own settings of its player, kept for each media it opens.
#[derive(Default)]
struct Settings {
    start_at: f64,
    rate: Option<f64>,
    hardware_decode: Option<bool>,
    subtitle_scale: Option<f64>,
    subtitle_supersampling: bool,
    upscaler: LumaUpscalerMode,
    danmaku: Option<DanmakuConfig>,
}

/// liberika's layout configuration for Erika's danmaku settings.
fn danmaku_layout(config: &DanmakuConfig) -> DanmakuLayoutConfig {
    DanmakuLayoutConfig {
        enabled: config.enabled,
        supersampling: config.supersampling,
        opacity: config.opacity.clamp(0.0, 1.0) as f32,
        font_size: config.font_size as f32,
        display_area: config.area.clamp(0.1, 1.0) as f32,
        scroll_duration_seconds: config.scroll_seconds as f32,
        merge_duplicates: config.merge,
        block_top: config.block_top,
        block_bottom: config.block_bottom,
        block_scroll: config.block_scroll,
        random_colors: config.random_colors,
        ..DanmakuLayoutConfig::default()
    }
}

fn micros(seconds: f64) -> i64 {
    (seconds * 1_000_000.0).round() as i64
}

struct View {
    token: String,
    player: String,
    surface: VideoSurface,
}

struct Session {
    presenter: PresenterRuntime,
    hardware_decode: bool,
    events: crossbeam_channel::Receiver<PlayerEvent>,
    /// The hub's source version of what is open.
    version: u64,
    loaded: bool,
    source: Option<String>,
    chapters: Vec<Chapter>,
    /// The view the presenter draws into.
    view: Option<String>,
    volume: f64,
    muted: bool,
    buffering: bool,
    error: Option<String>,
    tracks: Vec<TrackInfo>,
    selection: TrackSelection,
    /// Frames rendered when the media opened.
    rendered_at_open: u64,
    /// The picture captured last.
    still: Option<PathBuf>,
    /// The comments the user sent while this media played, and the track
    /// that shows them.
    own_danmaku: Vec<DanmakuItem>,
    own_track: Option<u64>,
    reported: Option<(Phase, Instant)>,
    audio_ticked: Instant,
}

impl Session {
    fn new(hardware_decode: bool) -> Result<Self, CommandError> {
        let mut config = PresenterConfig::default();
        if !hardware_decode {
            config.player.playback.video_decode =
                liberika::playback::VideoDecodePreference::Software;
        }
        let presenter = PresenterRuntime::new(config)
            .map_err(|error| CommandError::new("kernel_failed", error.to_string()))?;
        let events = presenter.player().subscribe();
        Ok(Self {
            presenter,
            hardware_decode,
            events,
            version: 0,
            loaded: false,
            source: None,
            chapters: Vec::new(),
            view: None,
            volume: 1.0,
            muted: false,
            buffering: false,
            error: None,
            tracks: Vec::new(),
            selection: TrackSelection::default(),
            rendered_at_open: 0,
            still: None,
            own_danmaku: Vec::new(),
            own_track: None,
            reported: None,
            audio_ticked: Instant::now(),
        })
    }

    /// Playing with no view to draw in: only the audio moves the clock.
    fn needs_audio_ticks(&self) -> bool {
        self.loaded && self.view.is_none() && self.presenter.is_playing()
    }

    fn apply_volume(&mut self) {
        let volume = if self.muted { 0.0 } else { self.volume };
        self.presenter.set_volume(volume);
    }

    fn drain_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                PlayerEvent::TracksChanged(tracks) => self.tracks = tracks,
                PlayerEvent::TrackSelectionChanged(selection) => self.selection = selection,
                PlayerEvent::BufferingChanged(buffering) => self.buffering = buffering,
                PlayerEvent::Error(error) => self.error = Some(error.to_string()),
                _ => {}
            }
        }
    }

    fn observation(&self) -> Observation {
        let player = self.presenter.player();
        let state = player.state();
        let duration = self.presenter.duration();
        let phase = if self.error.is_some() || state == PlayerState::Error {
            Phase::Failed
        } else if !self.loaded {
            if state == PlayerState::Opening {
                Phase::Loading
            } else {
                Phase::Idle
            }
        } else {
            match state {
                PlayerState::Opening => Phase::Loading,
                PlayerState::Ready => Phase::Ready,
                PlayerState::Playing if self.buffering => Phase::Buffering,
                PlayerState::Playing => Phase::Playing,
                PlayerState::Paused => Phase::Paused,
                PlayerState::Stopped if player.is_stopped_at_end() => Phase::Ended,
                PlayerState::Stopped => Phase::Paused,
                PlayerState::Idle | PlayerState::Closed => Phase::Idle,
                PlayerState::Error => Phase::Failed,
            }
        };
        Observation {
            phase,
            position_ms: self.presenter.media_time().as_millis() as i64,
            duration_ms: duration.map(|duration| duration.as_millis() as i64),
            seekable: self.loaded && duration.is_some(),
            volume: self.volume,
            muted: self.muted,
            error: self.error.clone(),
        }
    }

    fn playback(&self, observation: &Observation) -> Playback {
        let tracks = |kind: TrackKind| -> Vec<Track> {
            self.tracks
                .iter()
                .filter(|track| track.kind == kind)
                .enumerate()
                .map(|(index, track)| Track {
                    id: track.id,
                    name: track_name(track, index),
                })
                .collect()
        };
        let video = self
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Video && Some(track.id) == self.selection.video)
            .or_else(|| {
                self.tracks
                    .iter()
                    .find(|track| track.kind == TrackKind::Video)
            })
            .map(|track| {
                let codec = track.codec.as_deref().unwrap_or_default().to_uppercase();
                match (codec.is_empty(), track.height) {
                    (false, 0) => codec,
                    (false, height) => format!("{codec} · {height}p"),
                    (true, height) => format!("{height}p"),
                }
            })
            .unwrap_or_default();
        Playback {
            source: self.source.clone(),
            loaded: self.loaded && observation.phase != Phase::Failed,
            playing: matches!(observation.phase, Phase::Playing | Phase::Buffering),
            ended: observation.phase == Phase::Ended,
            picture: self.presenter.stats().rendered_video_frames > self.rendered_at_open,
            position: observation.position_ms as f64 / 1000.0,
            duration: observation
                .duration_ms
                .map(|duration| duration as f64 / 1000.0),
            chapters: self.chapters.clone(),
            audio_tracks: tracks(TrackKind::Audio),
            subtitle_tracks: tracks(TrackKind::Subtitle),
            audio_track: self.selection.audio,
            subtitle_track: self.selection.subtitle,
            video,
            still: self.still.clone(),
            error: observation.error.clone(),
        }
    }

    /// Show `text` as the user's comment now, in `rgb`: a moment ahead of
    /// the picture, before the danmaku delay moves it, on a track of the
    /// user's own comments, which liberika frames.
    fn send_danmaku(&mut self, text: String, rgb: u32, delay: f64) {
        let at = self.presenter.media_time().as_secs_f64() + 0.2 - delay;
        self.own_danmaku.push(DanmakuItem {
            id: self.own_danmaku.len() as u64 + 1,
            pts: Duration::from_secs_f64(at.max(0.0)),
            text,
            mode: DanmakuMode::Scroll,
            font_size: 25.0,
            color: DanmakuColor::rgb_u8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8),
            opacity: 1.0,
            is_self: true,
        });
        let Ok(timeline) = DanmakuTimeline::new(self.own_danmaku.clone()) else {
            return;
        };
        if let Some(track) = self.own_track.take() {
            self.presenter.remove_danmaku_track(track);
        }
        self.own_track =
            Some(
                self.presenter
                    .add_danmaku_track(timeline, "own", DanmakuTrackSource::Manual, 0),
            );
    }

    /// Save the picture with its subtitles, `width` wide or as wide as the
    /// video, as a PNG at `path`.
    fn capture(&mut self, path: &Path, width: Option<u32>) -> Result<(), String> {
        let (video_width, video_height) = self
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Video && track.width > 0 && track.height > 0)
            .map(|track| (track.width, track.height))
            .unwrap_or((1920, 1080));
        let width = width.unwrap_or(video_width).clamp(1, 7680);
        let height = ((u64::from(width) * u64::from(video_height)) / u64::from(video_width))
            .clamp(1, 7680) as u32;
        let rgba = self
            .presenter
            .capture_frame_rgba(width, height)
            .map_err(|error| error.to_string())?
            .ok_or("no picture yet")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
        }
        let partial = path.with_extension("partial");
        let file = std::fs::File::create(&partial).map_err(|error| error.to_string())?;
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .and_then(|mut writer| writer.write_image_data(&rgba))
            .map_err(|error| error.to_string())?;
        std::fs::rename(&partial, path).map_err(|error| error.to_string())?;
        self.still = Some(path.to_path_buf());
        Ok(())
    }
}

/// A track as the file names it: its title, else its language, with its
/// codec, such as `简体中文 · ASS` or `JPN · OPUS`.
fn track_name(track: &TrackInfo, index: usize) -> String {
    let name = track
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .or_else(|| {
            track
                .language
                .clone()
                .filter(|language| !language.is_empty() && language != "und")
                .map(|language| language.to_uppercase())
        })
        .unwrap_or_else(|| format!("#{}", index + 1));
    match track.codec.as_deref() {
        Some(codec) if !codec.is_empty() => format!("{name} · {}", codec.to_uppercase()),
        _ => name,
    }
}

fn failed(error: impl std::fmt::Display) -> CommandError {
    CommandError::new("media_error", error.to_string())
}

fn platform_surface(surface: &VideoSurface) -> PlatformSurface {
    let SurfaceHandle::MetalLayer(layer) = surface.handle;
    PlatformSurface::Metal(MetalSurfaceHandle::new(
        layer,
        surface.size.width,
        surface.size.height,
        surface.size.scale,
    ))
}

struct Worker {
    receiver: Receiver<Message>,
    hub: Hub,
    playback: Arc<Mutex<Playback>>,
    results: Arc<Mutex<Vec<OperationResult>>>,
    frames: Arc<Frames>,
    players: HashMap<String, Session>,
    /// Attached views, in the order they attached: a player shows in the
    /// last of its views.
    views: Vec<View>,
    settings: Settings,
}

impl Worker {
    fn run(&mut self) {
        loop {
            let wait = if self.players.values().any(Session::needs_audio_ticks) {
                AUDIO_EVERY
            } else {
                Duration::from_millis(250)
            };
            match self.receiver.recv_timeout(wait) {
                Ok(Message::Detached) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(message) => self.handle(message),
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.pump_audio();
            self.report(false);
        }
        for (_, mut session) in self.players.drain() {
            let _ = session.presenter.close();
        }
    }

    fn hub(&self) -> Option<Arc<dyn KernelHub>> {
        self.hub.lock().expect("hub lock").clone()
    }

    fn handle(&mut self, message: Message) {
        match message {
            Message::Request(request) => {
                let result = self.perform(&request);
                if let Err(error) = &result {
                    eprintln!("liberika: {:?} failed: {}", request.op, error.message);
                }
                if let Some(hub) = self.hub() {
                    hub.complete(request.request, result);
                }
                self.report(true);
            }
            Message::Attach {
                view,
                player,
                surface,
            } => {
                self.views.retain(|existing| existing.token != view);
                self.views.push(View {
                    token: view,
                    player: player.clone(),
                    surface,
                });
                if let Err(error) = self.show(&player) {
                    eprintln!("liberika: cannot show `{player}`: {}", error.message);
                }
            }
            Message::Resize { view, size } => {
                if let Some(existing) = self
                    .views
                    .iter_mut()
                    .find(|existing| existing.token == view)
                {
                    existing.surface.size = size;
                }
                for session in self.players.values_mut() {
                    if session.view.as_deref() == Some(&view) {
                        let _ =
                            session
                                .presenter
                                .resize_surface(size.width, size.height, size.scale);
                    }
                }
            }
            Message::Detach { view, done } => {
                let player = self
                    .views
                    .iter()
                    .position(|existing| existing.token == view)
                    .map(|index| self.views.remove(index).player);
                if let Some(player) = player
                    && let Some(session) = self.players.get_mut(&player)
                    && session.view.as_deref() == Some(&view)
                {
                    let _ = session.presenter.detach_surface();
                    session.view = None;
                    // An earlier view of the player shows it again.
                    let _ = self.show(&player);
                }
                let _ = done.send(());
            }
            Message::Frame => self.frame(),
            Message::Set(setting) => self.set(setting),
            Message::Close => {
                if let Some(mut session) = self.players.remove(PLAYER) {
                    let _ = session.presenter.close();
                    if let Some(hub) = self.hub() {
                        hub.observe(PLAYER, session.version, Observation::idle());
                    }
                }
                *self.playback.lock().expect("playback lock") = Playback::default();
            }
            Message::Detached => {}
        }
    }

    /// The session of `player`, created on first use and drawing into the
    /// last view attached for it.
    fn session(&mut self, player: &str) -> Result<&mut Session, CommandError> {
        if !self.players.contains_key(player) {
            self.players.insert(
                player.to_string(),
                Session::new(self.settings.hardware_decode.unwrap_or(true))?,
            );
            self.show(player)?;
        }
        Ok(self.players.get_mut(player).expect("session exists"))
    }

    /// Draw `player` into its last attached view, if it is not already.
    fn show(&mut self, player: &str) -> Result<(), CommandError> {
        let Some(view) = self.views.iter().rev().find(|view| view.player == player) else {
            return Ok(());
        };
        let Some(session) = self.players.get_mut(player) else {
            return Ok(());
        };
        if session.view.as_deref() == Some(&view.token) {
            return Ok(());
        }
        if session.view.is_some() {
            let _ = session.presenter.detach_surface();
        }
        session.view = None;
        session
            .presenter
            .attach_surface(platform_surface(&view.surface))
            .map_err(failed)?;
        session.view = Some(view.token.clone());
        Ok(())
    }

    fn perform(&mut self, request: &ExecRequest) -> Result<(), CommandError> {
        match &request.op {
            ExecOp::Load { url } => {
                let start = std::mem::take(&mut self.settings.start_at);
                let hardware = self.settings.hardware_decode.unwrap_or(true);
                if self
                    .players
                    .get(&request.player)
                    .is_some_and(|s| s.hardware_decode != hardware)
                {
                    let replacement = Session::new(hardware)?;
                    if let Some(mut old) = self.players.remove(&request.player) {
                        let _ = old.presenter.detach_surface();
                    }
                    self.players.insert(request.player.clone(), replacement);
                    self.show(&request.player)?;
                }
                let session = self.session(&request.player)?;
                session.version = request.source_version;
                session.loaded = false;
                session.source = Some(url.clone());
                session.chapters.clear();
                session.error = None;
                session.buffering = false;
                session.tracks.clear();
                session.selection = TrackSelection::default();
                session.reported = None;
                session.still = None;
                session.own_danmaku.clear();
                session.own_track = None;
                session.presenter.clear_danmaku();
                session.rendered_at_open = session.presenter.stats().rendered_video_frames;
                let media = MediaRequest {
                    http_headers: super::link_headers(&url),
                    ..MediaRequest::new(url.clone())
                };
                if let Err(error) = session.presenter.open(media) {
                    session.error = Some(error.to_string());
                    return Err(CommandError::new("load_failed", error.to_string()));
                }
                session.loaded = true;
                session.tracks = session.presenter.tracks();
                session.chapters = session
                    .presenter
                    .player()
                    .chapters()
                    .into_iter()
                    .map(|chapter| Chapter {
                        title: chapter.title,
                        start: chapter.start.as_secs_f64(),
                        end: chapter.end.map(|end| end.as_secs_f64()),
                    })
                    .collect();
                session.selection = session.presenter.track_selection();
                session.apply_volume();
                if request.player == PLAYER {
                    self.apply_settings();
                    if start > 0.0 {
                        let session = self.players.get_mut(PLAYER).expect("session exists");
                        let _ = session.presenter.seek(Duration::from_secs_f64(start));
                    }
                }
                Ok(())
            }
            ExecOp::Unload => {
                if let Some(session) = self.players.get_mut(&request.player) {
                    session.version = request.source_version;
                    session.loaded = false;
                    session.presenter.stop().map_err(failed)?;
                }
                Ok(())
            }
            ExecOp::Release => {
                if let Some(mut session) = self.players.remove(&request.player) {
                    let _ = session.presenter.close();
                }
                Ok(())
            }
            op => {
                let session = self
                    .players
                    .get_mut(&request.player)
                    .ok_or_else(|| CommandError::new("no_source", "nothing is open"))?;
                if session.version != request.source_version {
                    return Err(CommandError::new(
                        "stale_source",
                        "a newer source is loaded",
                    ));
                }
                match op {
                    ExecOp::Play => {
                        if session.presenter.player().is_stopped_at_end() {
                            session.presenter.seek(Duration::ZERO).map_err(failed)?;
                        }
                        session.presenter.play().map_err(failed)
                    }
                    ExecOp::Pause => session.presenter.pause().map_err(failed),
                    ExecOp::Seek { position_ms } => session
                        .presenter
                        .seek(Duration::from_millis((*position_ms).max(0) as u64))
                        .map_err(failed),
                    ExecOp::SetVolume { volume } => {
                        session.volume = volume.clamp(0.0, 1.0);
                        session.apply_volume();
                        Ok(())
                    }
                    ExecOp::SetMuted { muted } => {
                        session.muted = *muted;
                        session.apply_volume();
                        Ok(())
                    }
                    ExecOp::Load { .. } | ExecOp::Unload | ExecOp::Release => unreachable!(),
                }
            }
        }
    }

    /// Erika's settings, on the media open in its player.
    fn apply_settings(&mut self) {
        let Some(session) = self.players.get_mut(PLAYER) else {
            return;
        };
        if let Some(rate) = self.settings.rate {
            let _ = session.presenter.set_playback_rate(rate);
        }
        if let Some(scale) = self.settings.subtitle_scale {
            session.presenter.set_subtitle_scale(scale);
        }
        session.presenter.set_luma_upscaler(self.settings.upscaler);
        session
            .presenter
            .set_subtitle_supersampling(self.settings.subtitle_supersampling);
        if let Some(danmaku) = &self.settings.danmaku {
            session
                .presenter
                .set_danmaku_config(danmaku_layout(danmaku));
            session
                .presenter
                .set_danmaku_global_offset(micros(danmaku.delay));
        }
    }

    fn set(&mut self, setting: Setting) {
        let visual_setting = matches!(&setting, Setting::Danmaku(_));
        match setting {
            Setting::StartAt(seconds) => self.settings.start_at = seconds.max(0.0),
            Setting::Rate(rate) => self.settings.rate = Some(rate),
            Setting::HardwareDecode(enabled) => self.settings.hardware_decode = Some(enabled),
            Setting::SubtitleScale(scale) => self.settings.subtitle_scale = Some(scale),
            Setting::SubtitleSupersampling(enabled) => {
                self.settings.subtitle_supersampling = enabled
            }
            Setting::Upscaler(mode) => self.settings.upscaler = mode,
            Setting::Danmaku(ref config) => self.settings.danmaku = Some(config.clone()),
            Setting::AudioTrack(_)
            | Setting::SubtitleTrack(_)
            | Setting::AddSubtitle(_)
            | Setting::Capture(..)
            | Setting::AddDanmaku(..)
            | Setting::SendDanmaku(..) => {}
        }
        let delay = self
            .settings
            .danmaku
            .as_ref()
            .map_or(0.0, |danmaku| danmaku.delay);
        let Some(session) = self
            .players
            .get_mut(PLAYER)
            .filter(|session| session.loaded)
        else {
            let operation = match setting {
                Setting::Capture(path, None) => Some(Operation::Screenshot(path)),
                Setting::AddSubtitle(url) => Some(Operation::Subtitle(url)),
                _ => None,
            };
            if let Some(operation) = operation {
                self.results.lock().unwrap().push(OperationResult {
                    source: None,
                    operation,
                    result: Err("视频尚未加载完成".into()),
                });
            }
            return;
        };
        let result = match setting {
            Setting::StartAt(_) | Setting::HardwareDecode(_) => Ok(()),
            Setting::Rate(rate) => session.presenter.set_playback_rate(rate),
            Setting::SubtitleScale(scale) => {
                session.presenter.set_subtitle_scale(scale);
                Ok(())
            }
            Setting::Upscaler(mode) => {
                session.presenter.set_luma_upscaler(mode);
                Ok(())
            }
            Setting::SubtitleSupersampling(enabled) => {
                session.presenter.set_subtitle_supersampling(enabled);
                Ok(())
            }
            Setting::AudioTrack(id) => session.presenter.select_audio_track(Some(id)),
            Setting::SubtitleTrack(id) => session.presenter.select_subtitle_track(id),
            Setting::AddSubtitle(url) => {
                let result = session
                    .presenter
                    .add_external_subtitle(url.clone())
                    .and_then(|track| session.presenter.select_subtitle_track(Some(track.id)))
                    .map_err(|e| e.to_string());
                self.results.lock().unwrap().push(OperationResult {
                    source: session.source.clone(),
                    operation: Operation::Subtitle(url),
                    result,
                });
                Ok(())
            }
            Setting::Capture(path, width) => {
                let result = session.capture(&path, width);
                if width.is_none() {
                    self.results.lock().unwrap().push(OperationResult {
                        source: session.source.clone(),
                        operation: Operation::Screenshot(path),
                        result,
                    });
                }
                Ok(())
            }
            Setting::Danmaku(config) => {
                session
                    .presenter
                    .set_danmaku_config(danmaku_layout(&config));
                session
                    .presenter
                    .set_danmaku_global_offset(micros(config.delay));
                Ok(())
            }
            Setting::AddDanmaku(name, timeline) => {
                for track in session
                    .presenter
                    .danmaku_tracks()
                    .into_iter()
                    .filter(|track| track.name == name)
                {
                    session.presenter.remove_danmaku_track(track.id);
                }
                let source = DanmakuTrackSource::File(PathBuf::from(&name));
                session
                    .presenter
                    .add_danmaku_track(timeline, name, source, 0);
                Ok(())
            }
            Setting::SendDanmaku(text, rgb) => {
                session.send_danmaku(text, rgb, delay);
                Ok(())
            }
        };
        if let Err(error) = result {
            self.results.lock().unwrap().push(OperationResult {
                source: session.source.clone(),
                operation: Operation::Setting,
                result: Err(error.to_string()),
            });
        }
        // Visual settings do not change the player's tracks or observation.
        // A live slider must not query the media worker and publish a full
        // player snapshot for each pointer update.
        if !visual_setting {
            session.tracks = session.presenter.tracks();
            session.selection = session.presenter.track_selection();
            self.report(true);
        }
    }

    /// Draw the frame each view's display prepares, in the player it shows.
    fn frame(&mut self) {
        self.frames.pending.store(false, Ordering::Release);
        let latest = std::mem::take(&mut *self.frames.latest.lock().expect("frames lock"));
        for (view, (timing, fired)) in latest {
            let Some(session) = self
                .players
                .values_mut()
                .find(|session| session.view.as_deref() == Some(&view))
            else {
                continue;
            };
            if !session.loaded {
                continue;
            }
            // The display's target instant, on this thread's clock.
            let delay = (timing.target - timing.now).clamp(-0.25, 0.25);
            let at = if delay >= 0.0 {
                fired + Duration::from_secs_f64(delay)
            } else {
                fired
                    .checked_sub(Duration::from_secs_f64(-delay))
                    .unwrap_or(fired)
            };
            if let Err(error) = session.presenter.render_tick_at(timing.target, at) {
                eprintln!("liberika: frame failed: {error}");
            }
        }
        self.report(false);
    }

    /// Keep the audio and the clock going for players no view shows.
    fn pump_audio(&mut self) {
        for session in self.players.values_mut() {
            if session.needs_audio_ticks() && session.audio_ticked.elapsed() >= AUDIO_EVERY {
                session.audio_ticked = Instant::now();
                let _ = session.presenter.audio_only_tick();
            }
        }
    }

    /// Tell the hub, and the engine for Erika's player, what each player
    /// does: at once when `now` or the phase changed, else every
    /// `REPORT_EVERY`.
    fn report(&mut self, now: bool) {
        let hub = self.hub();
        for (id, session) in self.players.iter_mut() {
            session.drain_events();
            let observation = session.observation();
            let due = now
                || match session.reported {
                    Some((phase, at)) => phase != observation.phase || at.elapsed() >= REPORT_EVERY,
                    None => true,
                };
            if !due {
                continue;
            }
            session.reported = Some((observation.phase, Instant::now()));
            if id == PLAYER {
                *self.playback.lock().expect("playback lock") = session.playback(&observation);
            }
            if let Some(hub) = &hub {
                hub.observe(id, session.version, observation);
            }
        }
    }
}
