// SPDX-License-Identifier: MPL-2.0
// The audio owner is independent of rendering. Only this thread creates,
// calls and drops the backend; platform audio callbacks keep their own model.
use super::PresenterAudioConfig;
#[cfg(target_os = "android")]
use crate::android::aaudio::{AAudioOutput, AAudioOutputConfig};
#[cfg(target_os = "macos")]
use crate::apple::coreaudio::{CoreAudioOutput, CoreAudioOutputConfig};
#[cfg(any(target_os = "ios", target_os = "tvos"))]
use crate::apple::iosaudio::{IosAudioQueueOutput, IosAudioQueueOutputConfig};
#[cfg(test)]
use crate::audio::AudioRingBufferConfig;
#[cfg(not(any(
    target_os = "android",
    target_os = "macos",
    any(target_os = "ios", target_os = "tvos"),
    target_os = "windows",
    target_env = "ohos"
)))]
use crate::audio::BufferedAudioOutput;
use crate::audio::{
    AUDIO_OUTPUT_QUEUE_HIGH_WATER, AudioClockSnapshot, AudioOutputBackend, AudioOutputRuntimeStats,
};
use crate::core::{AudioOutputEvent, Player, PlayerAudioFrame, PlayerState};
#[cfg(target_env = "ohos")]
use crate::ohos::ohaudio::{OHAudioOutput, OHAudioOutputConfig};

#[cfg(target_os = "windows")]
use crate::windows::wasapi::{WasapiAudioOutput, WasapiAudioOutputConfig};
use crate::{PlayerError, Result, trace};
use crossbeam_channel::{Receiver, Sender, bounded};
use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
const AUDIO_START_BUFFER: Duration = Duration::from_millis(250);
const AUDIO_PUMP_FRAME_LIMIT: usize = 16;
const AUDIO_PUMP_TIME_BUDGET: Duration = Duration::from_millis(4);
// Bound each slice so device commands remain responsive during SoundTouch warmup.
const AUDIO_FAST_RATE_PUMP_FRAME_LIMIT: usize = 24;
const AUDIO_FAST_RATE_PUMP_TIME_BUDGET: Duration = Duration::from_millis(4);
const PLAYBACK_RATE_EPSILON: f64 = 0.001;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AudioClockReportState {
    media_time: Duration,
    queued_frames: usize,
    read_frames: u64,
    underflow_frames: u64,
}

#[derive(Debug, Clone, Copy)]
struct PendingPlaybackRate {
    rate: f64,
    commit_at: Instant,
}

const AUDIO_POLL_INTERVAL: Duration = Duration::from_millis(2);
const AUDIO_COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy)]
pub(super) struct AudioServiceSnapshot {
    pub clock: Option<AudioClockSnapshot>,
    pub runtime: AudioOutputRuntimeStats,
    pub playback_rate: f64,
    pub volume: f32,
    pub pushed_audio_frames: u64,
    pub audio_failures: u64,
    pub pump_duration: Duration,
}

#[derive(Debug)]
pub(super) enum AudioCommand {
    Quiesce,
    Resume,
    Drain,
    Reset,
    ResetCommitted,
    CommitPendingRate,
    Pause,
    SetRate(f64),
    SetVolume(f32),
    VideoPresented(u64),
    Shutdown,
}

struct Request {
    command: AudioCommand,
    reply: Sender<Result<()>>,
    deadline: Instant,
}

pub(super) struct AudioService {
    commands: Sender<Request>,
    snapshot: Arc<Mutex<AudioServiceSnapshot>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl AudioService {
    pub fn new(
        player: Player,
        frames: Receiver<PlayerAudioFrame>,
        config: PresenterAudioConfig,
    ) -> Result<Self> {
        Self::with_factory(player, frames, move || build_audio_output(config))
    }

    pub fn with_factory(
        player: Player,
        frames: Receiver<PlayerAudioFrame>,
        factory: impl FnOnce() -> Box<dyn AudioOutputBackend> + Send + 'static,
    ) -> Result<Self> {
        let (commands, requests) = bounded::<Request>(16);
        let (ready, initialized) = bounded(1);
        let worker = thread::Builder::new()
            .name("erika-audio".into())
            .spawn(move || {
                let mut pump = AudioPump::new(player, frames, factory());
                let shared = Arc::new(Mutex::new(pump.snapshot()));
                if ready.send(shared.clone()).is_err() {
                    return;
                }
                loop {
                    let request = if pump.quiesced {
                        requests.recv().map_err(|_| crossbeam_channel::RecvTimeoutError::Disconnected)
                    } else {
                        let interval = if pump.is_playing() || pump.audio_started {
                            AUDIO_POLL_INTERVAL
                        } else {
                            Duration::from_millis(100)
                        };
                        requests.recv_timeout(interval)
                    };
                    match request {
                        Ok(request) => {
                            let shutdown = matches!(request.command, AudioCommand::Shutdown);
                            if !shutdown && Instant::now() >= request.deadline {
                                let _ = request.reply.send(Err(PlayerError::Playback(
                                    "audio command expired before execution".into(),
                                )));
                                continue;
                            }
                            let action = format!("{:?}", request.command);
                            let result = pump.handle(request.command);
                            let snapshot = pump.snapshot();
                            *shared.lock().expect("audio snapshot mutex poisoned") = snapshot;
                            trace::log(format!(
                                "[erika-audio-owner] action={action} acknowledged={} generation={} quiesced={}",
                                result.is_ok(), pump.player.playback_generation(), pump.quiesced,
                            ));
                            let _ = request.reply.send(result);
                            if shutdown {
                                break;
                            }
                        }
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                        Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                    }
                    if !pump.quiesced {
                        let started = Instant::now();
                        if let Err(error) = pump.run_slice() {
                            pump.stats.audio_failures += 1;
                            pump.quiesced = true;
                            trace::diagnostic(format!("audio owner suspended after failure: {error}"));
                        }
                        pump.stats.pump_duration = started.elapsed();
                        let snapshot = pump.snapshot();
                        *shared.lock().expect("audio snapshot mutex poisoned") = snapshot;
                    }
                }
                // Device destruction stays on its creation thread, before join returns.
                let _ = pump.reset_audio_output();
            })
            .map_err(|error| PlayerError::Playback(format!("cannot start audio owner: {error}")))?;
        let snapshot = initialized
            .recv()
            .map_err(|_| PlayerError::Playback("audio owner failed to initialize".into()))?;
        Ok(Self {
            commands,
            snapshot,
            worker: Some(worker),
        })
    }

    pub fn command(&self, command: AudioCommand) -> Result<()> {
        let (reply, response) = bounded(1);
        let deadline = Instant::now() + AUDIO_COMMAND_TIMEOUT;
        self.commands
            .send_timeout(
                Request {
                    command,
                    reply,
                    deadline,
                },
                AUDIO_COMMAND_TIMEOUT,
            )
            .map_err(|error| {
                PlayerError::Playback(format!("audio command send failed: {error}"))
            })?;
        response
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| {
                PlayerError::Playback(format!("audio command acknowledgement failed: {error}"))
            })?
    }

    pub fn snapshot(&self) -> AudioServiceSnapshot {
        *self.snapshot.lock().expect("audio snapshot mutex poisoned")
    }
}

impl Drop for AudioService {
    fn drop(&mut self) {
        let _ = self.command(AudioCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Default)]
struct AudioCounters {
    pushed_audio_frames: u64,
    audio_failures: u64,
    pump_duration: Duration,
}

struct AudioPump {
    player: Player,
    audio_frames: Receiver<PlayerAudioFrame>,
    audio_output: Box<dyn AudioOutputBackend>,
    audio_configured: bool,
    audio_started: bool,
    last_audio_clock_report: Option<AudioClockReportState>,
    last_audio_runtime_stats: AudioOutputRuntimeStats,
    playback_rate: f64,
    pending_playback_rate: Option<PendingPlaybackRate>,
    video_ready_generation: Option<u64>,
    quiesced: bool,
    stats: AudioCounters,
}

impl AudioPump {
    fn new(
        player: Player,
        audio_frames: Receiver<PlayerAudioFrame>,
        audio_output: Box<dyn AudioOutputBackend>,
    ) -> Self {
        Self {
            player,
            audio_frames,
            audio_output,
            audio_configured: false,
            audio_started: false,
            last_audio_clock_report: None,
            last_audio_runtime_stats: AudioOutputRuntimeStats::default(),
            playback_rate: 1.0,
            pending_playback_rate: None,
            video_ready_generation: None,
            quiesced: true,
            stats: AudioCounters::default(),
        }
    }

    fn snapshot(&self) -> AudioServiceSnapshot {
        AudioServiceSnapshot {
            clock: self.audio_output.clock_snapshot(),
            runtime: self.audio_output.runtime_stats(),
            playback_rate: self.playback_rate,
            volume: self.audio_output.volume(),
            pushed_audio_frames: self.stats.pushed_audio_frames,
            audio_failures: self.stats.audio_failures,
            pump_duration: self.stats.pump_duration,
        }
    }

    fn is_playing(&self) -> bool {
        self.player.state() == PlayerState::Playing
    }

    fn run_slice(&mut self) -> Result<()> {
        match self.player.state() {
            PlayerState::Closed | PlayerState::Error => {
                self.quiesced = true;
                return self.reset_audio_output_with_committed_rate();
            }
            PlayerState::Paused if self.audio_started => self.pause_output()?,
            _ => {}
        }
        self.commit_pending_playback_rate()?;
        self.report_audio_output_runtime_stats();
        self.pump_audio();
        self.report_audio_output_runtime_stats();
        // The worker owns EOF publication. Once all PCM has reached the device,
        // sleep until a replay/open/control command. Do not stop the backend
        // here: an empty ring does not prove its hardware tail has played.
        if self.player.is_stopped_at_end()
            && self.audio_frames.is_empty()
            && self
                .audio_output
                .clock_snapshot()
                .is_none_or(|s| s.queued_frames == 0)
        {
            self.quiesced = true;
        }
        Ok(())
    }

    fn pause_output(&mut self) -> Result<()> {
        if self.pending_playback_rate.is_some() {
            self.reset_audio_output()?;
            return self.commit_pending_playback_rate_now();
        }
        self.audio_output
            .pause()
            .map_err(|error| PlayerError::Playback(error.to_string()))?;
        self.audio_started = false;
        self.last_audio_clock_report = None;
        Ok(())
    }

    fn handle(&mut self, command: AudioCommand) -> Result<()> {
        match command {
            AudioCommand::Quiesce => {
                self.quiesced = true;
                self.player.invalidate_audio_clock();
            }
            AudioCommand::Resume => self.quiesced = false,
            AudioCommand::Drain => {
                if !self.quiesced {
                    return Err(PlayerError::Playback(
                        "audio drain requires quiesce acknowledgement".into(),
                    ));
                }
                while self.audio_frames.try_recv().is_ok() {}
            }
            AudioCommand::Reset => return self.reset_audio_output(),
            AudioCommand::ResetCommitted => return self.reset_audio_output_with_committed_rate(),
            AudioCommand::CommitPendingRate => return self.commit_pending_playback_rate_now(),
            AudioCommand::Pause => return self.pause_output(),
            AudioCommand::SetRate(rate) => return self.set_playback_rate(rate),
            AudioCommand::SetVolume(volume) => self.set_volume(volume as f64),
            AudioCommand::VideoPresented(generation) => {
                self.video_ready_generation = Some(generation)
            }
            AudioCommand::Shutdown => {
                self.quiesced = true;
                return self.reset_audio_output_with_committed_rate();
            }
        }
        Ok(())
    }
    fn pump_audio(&mut self) {
        let started = Instant::now();
        let mut pumped = 0usize;
        let (frame_limit, time_budget) = self.audio_pump_limits();
        loop {
            if pumped >= frame_limit || started.elapsed() >= time_budget {
                break;
            }
            if !self.audio_output.can_accept_audio_frame()
                || self
                    .audio_output
                    .clock_snapshot()
                    .and_then(|s| s.queued_duration)
                    .is_some_and(|q| q >= AUDIO_OUTPUT_QUEUE_HIGH_WATER)
            {
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
        let rate = self
            .pending_playback_rate
            .map_or(self.playback_rate, |pending| pending.rate);
        audio_pump_limits_for_rate(rate)
    }

    fn report_audio_clock_snapshot(&mut self) {
        // During a rate transition the audio ring already uses the requested
        // rate while the player clock still uses the old one. Do not enqueue a
        // mixed-rate clock sample; the first sample after commit is coherent.
        if self.pending_playback_rate.is_some() {
            return;
        }
        // configure/start/push can recover the device during this pump. Publish
        // its new epoch before sampling, not only after feedback is enqueued.
        self.report_audio_output_runtime_stats();
        let Some(observation) = self
            .player
            .capture_audio_clock(|| self.audio_output.clock_snapshot())
        else {
            return;
        };
        let snapshot = observation.snapshot();
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
        let _ = self.player.update_audio_clock_observation(observation);
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
            self.player.invalidate_audio_clock();
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
        self.player.track_selection().video.is_none()
            || self.video_ready_generation == Some(self.player.playback_generation())
    }

    fn reset_audio_output(&mut self) -> Result<()> {
        self.player.invalidate_audio_clock();
        if let Err(error) = self.audio_output.stop() {
            self.stats.audio_failures += 1;
            return Err(PlayerError::Playback(format!(
                "audio reset failed: {error}"
            )));
        }
        self.audio_configured = false;
        self.audio_started = false;
        self.last_audio_clock_report = None;
        Ok(())
    }

    fn reset_audio_output_with_committed_rate(&mut self) -> Result<()> {
        self.pending_playback_rate = None;
        self.reset_audio_output()?;
        self.audio_output.set_playback_rate(self.playback_rate);
        Ok(())
    }

    fn commit_pending_playback_rate(&mut self) -> Result<()> {
        let Some(pending) = self.pending_playback_rate else {
            return Ok(());
        };
        if !self.is_playing() || Instant::now() < pending.commit_at {
            return Ok(());
        }
        self.commit_pending_playback_rate_now()
    }

    fn commit_pending_playback_rate_now(&mut self) -> Result<()> {
        let Some(rate) = self.pending_playback_rate.map(|pending| pending.rate) else {
            return Ok(());
        };
        if let Err(error) = self.player.set_playback_rate(rate) {
            self.reset_audio_output_with_committed_rate()?;
            return Err(error);
        }
        self.playback_rate = rate;
        self.pending_playback_rate = None;
        self.player.invalidate_audio_clock();
        self.last_audio_clock_report = None;
        Ok(())
    }

    fn report_audio_output_runtime_stats(&mut self) {
        let stats = self.audio_output.runtime_stats();
        if stats.transition_sequence == self.last_audio_runtime_stats.transition_sequence {
            return;
        }
        self.player.invalidate_audio_clock();
        self.last_audio_clock_report = None;
        self.last_audio_runtime_stats = stats;
        let event = AudioOutputEvent { stats };
        trace::diagnostic(event.structured_message());
        self.player.report_audio_output_event(event);
    }

    pub fn set_playback_rate(&mut self, rate: f64) -> Result<()> {
        let next_rate = normalize_playback_rate(rate);
        if playback_rate_request_is_idempotent(
            self.playback_rate,
            self.pending_playback_rate,
            next_rate,
        ) {
            return Ok(());
        }
        self.player.invalidate_audio_clock();
        self.audio_output.set_playback_rate(next_rate);
        self.last_audio_clock_report = None;

        let bridge = audio_transition_bridge(
            self.audio_output.clock_snapshot(),
            self.audio_output.queued_output_duration(),
        );
        if self.is_playing()
            && let Some(bridge) = bridge
        {
            self.pending_playback_rate = Some(PendingPlaybackRate {
                rate: next_rate,
                commit_at: Instant::now() + bridge,
            });
            return Ok(());
        }

        // A paused output keeps its queued PCM. Discard it before committing a
        // new rate so a later resume cannot play old-rate samples while the
        // player clock is already running at the new rate.
        if !self.is_playing() && bridge.is_some() {
            self.reset_audio_output()?;
        }
        if let Err(error) = self.player.set_playback_rate(next_rate) {
            self.audio_output.set_playback_rate(self.playback_rate);
            return Err(error);
        }
        self.playback_rate = next_rate;
        self.pending_playback_rate = None;
        Ok(())
    }

    pub fn set_volume(&mut self, volume: f64) {
        self.audio_output.set_volume(volume as f32);
    }
}
fn normalize_playback_rate(rate: f64) -> f64 {
    if rate.is_finite() && rate > 0.0 {
        rate
    } else {
        1.0
    }
}

fn playback_rate_matches(lhs: f64, rhs: f64) -> bool {
    (lhs - rhs).abs() <= PLAYBACK_RATE_EPSILON
}

fn playback_rate_request_is_idempotent(
    current_rate: f64,
    pending: Option<PendingPlaybackRate>,
    next_rate: f64,
) -> bool {
    pending.is_some_and(|pending| playback_rate_matches(pending.rate, next_rate))
        || (pending.is_none() && playback_rate_matches(current_rate, next_rate))
}

fn audio_transition_bridge(
    snapshot: Option<AudioClockSnapshot>,
    queued_output_duration: Duration,
) -> Option<Duration> {
    snapshot
        .and_then(|snapshot| snapshot.queued_duration)
        .map(|duration| duration.saturating_add(queued_output_duration))
        .filter(|duration| !duration.is_zero())
}

fn audio_pump_limits_for_rate(rate: f64) -> (usize, Duration) {
    if (rate - 1.0).abs() > PLAYBACK_RATE_EPSILON {
        (
            AUDIO_FAST_RATE_PUMP_FRAME_LIMIT,
            AUDIO_FAST_RATE_PUMP_TIME_BUDGET,
        )
    } else {
        (AUDIO_PUMP_FRAME_LIMIT, AUDIO_PUMP_TIME_BUDGET)
    }
}

fn build_audio_output(config: PresenterAudioConfig) -> Box<dyn AudioOutputBackend> {
    #[cfg(target_os = "macos")]
    {
        Box::new(CoreAudioOutput::new(CoreAudioOutputConfig {
            ring_buffer: config.ring_buffer,
        }))
    }
    #[cfg(any(target_os = "ios", target_os = "tvos"))]
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
    #[cfg(target_env = "ohos")]
    {
        Box::new(OHAudioOutput::new(OHAudioOutputConfig {
            ring_buffer: config.ring_buffer,
        }))
    }
    #[cfg(not(any(
        target_os = "android",
        target_os = "macos",
        any(target_os = "ios", target_os = "tvos"),
        target_os = "windows",
        target_env = "ohos"
    )))]
    {
        Box::new(BufferedAudioOutput::new(config.ring_buffer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PlayerConfig, audio::BufferedAudioOutput};
    fn test_pump() -> AudioPump {
        let player = Player::new(PlayerConfig::default());
        let frames = player.subscribe_audio_frames();
        AudioPump::new(
            player,
            frames,
            Box::new(BufferedAudioOutput::new(AudioRingBufferConfig::default())),
        )
    }

    struct BlockingOutput {
        output: BufferedAudioOutput,
        entered: Sender<()>,
        release: Receiver<()>,
        block_next: bool,
        fail_stop: bool,
        recover_on_push: bool,
    }

    impl AudioOutputBackend for BlockingOutput {
        fn configure(&mut self, format: crate::ffmpeg::PcmFormat) -> crate::audio::Result<()> {
            self.output.configure(format)
        }
        fn start(&mut self) -> crate::audio::Result<()> {
            self.output.start()
        }
        fn pause(&mut self) -> crate::audio::Result<()> {
            self.output.pause()
        }
        fn stop(&mut self) -> crate::audio::Result<()> {
            if self.fail_stop {
                return Err(crate::audio::AudioError::Backend(
                    "injected stop failure".into(),
                ));
            }
            self.output.stop()
        }
        fn set_volume(&mut self, volume: f32) {
            self.output.set_volume(volume);
        }
        fn volume(&self) -> f32 {
            self.output.volume()
        }
        fn push(
            &mut self,
            frame: crate::ffmpeg::PcmAudioFrame,
        ) -> crate::audio::Result<crate::audio::AudioPushResult> {
            if self.block_next {
                self.block_next = false;
                self.entered.send(()).unwrap();
                self.release.recv_timeout(Duration::from_secs(1)).unwrap();
            }
            self.output.push(frame)
        }
        fn state(&self) -> crate::audio::AudioOutputState {
            self.output.state()
        }
        fn stats(&self) -> crate::audio::AudioRingBufferStats {
            self.output.stats()
        }
        fn clock_snapshot(&self) -> Option<AudioClockSnapshot> {
            Some(self.output.clock_snapshot())
        }
        fn runtime_stats(&self) -> AudioOutputRuntimeStats {
            if !self.recover_on_push {
                return AudioOutputRuntimeStats::default();
            }
            let recovered = self.output.stats().written_frames > 0;
            AudioOutputRuntimeStats {
                recovery_state: if recovered {
                    crate::audio::AudioRecoveryState::Stable
                } else {
                    crate::audio::AudioRecoveryState::Disconnected
                },
                transition_sequence: if recovered { 2 } else { 1 },
                recovery_count: u64::from(recovered),
                ..Default::default()
            }
        }
    }

    fn test_frame(generation: u64) -> PlayerAudioFrame {
        PlayerAudioFrame {
            generation,
            frame: crate::ffmpeg::PcmAudioFrame {
                format: crate::ffmpeg::PcmFormat::f32_interleaved(48000, 1),
                pts: Some(Duration::ZERO),
                frames: 480,
                samples: vec![0.0; 480],
            },
        }
    }

    #[test]
    fn quiesce_ack_waits_for_in_flight_push_and_fences_drain_and_reset() {
        let player = Player::new(PlayerConfig::default());
        let generation = player.playback_generation();
        let (frames, receiver) = bounded(4);
        let (entered, started) = bounded(1);
        let (release, resume) = bounded(1);
        let service = AudioService::with_factory(player, receiver, move || {
            Box::new(BlockingOutput {
                output: BufferedAudioOutput::new(AudioRingBufferConfig::default()),
                entered,
                release: resume,
                block_next: true,
                fail_stop: false,
                recover_on_push: false,
            })
        })
        .unwrap();
        frames.send(test_frame(generation)).unwrap();
        service.command(AudioCommand::Resume).unwrap();
        started.recv_timeout(Duration::from_secs(1)).unwrap();
        thread::scope(|scope| {
            let (done, acknowledged) = bounded(1);
            let service = &service;
            scope.spawn(move || {
                done.send(service.command(AudioCommand::Quiesce)).unwrap();
            });
            assert!(
                acknowledged
                    .recv_timeout(Duration::from_millis(20))
                    .is_err()
            );
            release.send(()).unwrap();
            acknowledged
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap();
        });
        let pushes = service.snapshot().pushed_audio_frames;
        assert_eq!(pushes, 1);
        frames.send(test_frame(generation)).unwrap();
        service.command(AudioCommand::Drain).unwrap();
        service.command(AudioCommand::Reset).unwrap();
        service.command(AudioCommand::Resume).unwrap();
        service.command(AudioCommand::Quiesce).unwrap();
        assert_eq!(service.snapshot().pushed_audio_frames, pushes);
        assert_eq!(service.snapshot().clock.unwrap().queued_frames, 0);
    }

    #[test]
    fn expired_resume_cannot_reactivate_a_quiesced_owner() {
        let player = Player::new(PlayerConfig::default());
        let frames = player.subscribe_audio_frames();
        let service = AudioService::with_factory(player, frames, || {
            Box::new(BufferedAudioOutput::new(AudioRingBufferConfig::default()))
        })
        .unwrap();
        let (reply, result) = bounded(1);
        service
            .commands
            .send(Request {
                command: AudioCommand::Resume,
                reply,
                deadline: Instant::now() - Duration::from_millis(1),
            })
            .unwrap();
        assert!(
            result
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .is_err()
        );
        service
            .command(AudioCommand::Drain)
            .expect("expired resume must leave owner quiesced");
    }

    #[test]
    fn failed_device_reset_is_not_acknowledged_as_success() {
        let (entered, _) = bounded(1);
        let (_, release) = bounded(1);
        let mut pump = test_pump();
        pump.audio_output = Box::new(BlockingOutput {
            output: BufferedAudioOutput::new(AudioRingBufferConfig::default()),
            entered,
            release,
            block_next: false,
            fail_stop: true,
            recover_on_push: false,
        });
        pump.audio_configured = true;
        assert!(pump.handle(AudioCommand::Reset).is_err());
        assert!(pump.audio_configured);
        assert_eq!(pump.stats.audio_failures, 1);
    }

    #[test]
    fn device_recovery_events_are_published_even_without_a_presenter_tick() {
        let (entered, _) = bounded(1);
        let (_, release) = bounded(1);
        let player = Player::new(PlayerConfig::default());
        let events = player.subscribe();
        let generation = player.playback_generation();
        let (sender, frames) = bounded(1);
        let mut pump = AudioPump::new(
            player,
            frames,
            Box::new(BlockingOutput {
                output: BufferedAudioOutput::new(AudioRingBufferConfig::default()),
                entered,
                release,
                block_next: false,
                fail_stop: false,
                recover_on_push: true,
            }),
        );
        sender.send(test_frame(generation)).unwrap();
        pump.run_slice().unwrap();
        let states: Vec<_> = events
            .try_iter()
            .filter_map(|event| match event {
                crate::core::PlayerEvent::AudioOutputChanged(event) => {
                    Some(event.stats.recovery_state)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            states,
            vec![
                crate::audio::AudioRecoveryState::Disconnected,
                crate::audio::AudioRecoveryState::Stable
            ]
        );
        assert_eq!(pump.last_audio_runtime_stats.recovery_count, 1);
        assert_eq!(pump.player.state(), PlayerState::Idle);
    }

    #[test]
    fn output_ignores_pcm_from_an_old_generation() {
        let player = Player::new(PlayerConfig::default());
        let generation = player.playback_generation();
        let (sender, frames) = bounded(2);
        let mut pump = AudioPump::new(
            player,
            frames,
            Box::new(BufferedAudioOutput::new(AudioRingBufferConfig::default())),
        );
        sender.send(test_frame(generation - 1)).unwrap();
        sender.send(test_frame(generation)).unwrap();
        pump.pump_audio();
        assert_eq!(pump.stats.pushed_audio_frames, 1);
        assert_eq!(
            pump.audio_output.clock_snapshot().unwrap().queued_frames,
            480
        );
    }

    #[test]
    fn repeated_playback_rate_request_keeps_pending_transition_deadline() {
        let deadline = Instant::now() + Duration::from_millis(250);
        let pending = PendingPlaybackRate {
            rate: 2.0,
            commit_at: deadline,
        };

        assert!(playback_rate_request_is_idempotent(1.0, Some(pending), 2.0));
        assert_eq!(pending.commit_at, deadline);
        assert!(playback_rate_request_is_idempotent(2.0, None, 2.0));
        assert!(!playback_rate_request_is_idempotent(
            1.0,
            Some(pending),
            1.5
        ));
    }

    #[test]
    fn audio_transition_bridge_includes_platform_output_duration() {
        let snapshot = AudioClockSnapshot {
            media_time: Some(Duration::from_secs(1)),
            queued_duration: Some(Duration::from_millis(250)),
            queued_frames: 12_000,
            read_frames: 0,
            written_frames: 12_000,
            underflow_frames: 0,
        };

        assert_eq!(
            audio_transition_bridge(Some(snapshot), Duration::from_millis(60)),
            Some(Duration::from_millis(310))
        );
        assert_eq!(
            audio_transition_bridge(
                Some(AudioClockSnapshot {
                    queued_duration: Some(Duration::ZERO),
                    ..snapshot
                }),
                Duration::from_millis(60),
            ),
            Some(Duration::from_millis(60))
        );
    }

    #[test]
    fn audio_reset_preserves_pending_rate_for_transition() {
        let mut presenter = test_pump();
        presenter.pending_playback_rate = Some(PendingPlaybackRate {
            rate: 2.0,
            commit_at: Instant::now(),
        });

        presenter.reset_audio_output().unwrap();
        assert_eq!(
            presenter.pending_playback_rate.map(|pending| pending.rate),
            Some(2.0)
        );
    }

    #[test]
    fn terminal_audio_reset_restores_committed_rate() {
        use crate::audio::BufferedAudioOutput;
        use crate::ffmpeg::{PcmAudioFrame, PcmFormat};

        let mut presenter = test_pump();
        let mut output = BufferedAudioOutput::new(AudioRingBufferConfig::default());
        output.set_playback_rate(2.0);
        presenter.audio_output = Box::new(output);
        presenter.playback_rate = 1.0;
        presenter.pending_playback_rate = Some(PendingPlaybackRate {
            rate: 2.0,
            commit_at: Instant::now(),
        });

        presenter.reset_audio_output_with_committed_rate().unwrap();
        let format = PcmFormat::f32_interleaved(48_000, 2);
        presenter
            .audio_output
            .push(PcmAudioFrame {
                format,
                pts: Some(Duration::ZERO),
                frames: 24_000,
                samples: vec![0.0; 48_000],
            })
            .unwrap();

        assert_eq!(
            presenter
                .audio_output
                .clock_snapshot()
                .unwrap()
                .queued_frames,
            24_000
        );
        assert!(presenter.pending_playback_rate.is_none());
    }

    #[test]
    fn failed_pending_rate_commit_clears_transition_state() {
        let mut presenter = test_pump();
        presenter.pending_playback_rate = Some(PendingPlaybackRate {
            rate: 2.0,
            commit_at: Instant::now(),
        });

        assert!(presenter.commit_pending_playback_rate_now().is_err());
        assert!(presenter.pending_playback_rate.is_none());
        assert_eq!(presenter.playback_rate, 1.0);
    }

    #[test]
    fn playback_rate_uses_fast_audio_pump_limits() {
        assert_eq!(
            audio_pump_limits_for_rate(1.0),
            (AUDIO_PUMP_FRAME_LIMIT, AUDIO_PUMP_TIME_BUDGET)
        );
        assert_eq!(
            audio_pump_limits_for_rate(2.0),
            (
                AUDIO_FAST_RATE_PUMP_FRAME_LIMIT,
                AUDIO_FAST_RATE_PUMP_TIME_BUDGET
            )
        );
    }

    #[test]
    fn audio_clock_report_tracks_queue_and_underflow_changes() {
        let mut presenter = test_pump();

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
    fn audio_start_is_blocked_while_player_is_not_playing() {
        use crate::audio::{AudioOutputState, BufferedAudioOutput};
        use crate::ffmpeg::{PcmAudioFrame, PcmFormat};

        let mut presenter = test_pump();
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
}
