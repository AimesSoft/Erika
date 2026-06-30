use std::collections::VecDeque;
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{
    Receiver, RecvTimeoutError, Sender, TryRecvError, TrySendError, bounded, unbounded,
};
use thiserror::Error;

use crate::audio::AudioClockSnapshot;
use crate::core::{
    MediaRequest, MediaSourceHint, TrackInfo, TrackKind, TrackSelection, VideoParams,
};
use crate::ffmpeg::{
    self, AudioResampler, Decoder, DecoderBackend, DecoderConfig, DecoderOutputFrame, Demuxer,
    Frame, OwnedCodecParameters, PcmAudioFrame, PcmFormat, StreamSelection, SubtitleDecoder,
};
use crate::source::{self, source_from_uri_with_hint};
use crate::subtitle::{DecodedSubtitleFrame, SubtitleTrackConfig, SubtitleTrackSource};
use crate::trace;

#[derive(Debug, Error)]
pub enum PlaybackError {
    #[error("ffmpeg error: {0}")]
    Ffmpeg(#[from] ffmpeg::FfmpegError),
    #[error("source error: {0}")]
    Source(#[from] source::SourceError),
    #[error("no video track found")]
    NoVideoTrack,
    #[error("selected decoder output is not a video frame")]
    UnexpectedDecoderOutput,
    #[error("no subtitle track found")]
    NoSubtitleTrack,
    #[error("track not found: kind={kind:?} id={track_id}")]
    TrackNotFound { kind: TrackKind, track_id: i64 },
    #[error("subtitle track is not removable: {0}")]
    SubtitleTrackNotRemovable(i64),
    #[error("demux worker error: {0}")]
    DemuxWorker(String),
}

pub type Result<T> = std::result::Result<T, PlaybackError>;

const DEFAULT_VIDEO_FRAME_QUEUE_LIMIT: usize = 8;
const DEFAULT_AUDIO_FRAME_QUEUE_LIMIT: usize = 16;
const STREAMING_VIDEO_FRAME_QUEUE_LIMIT: usize = 48;
const STREAMING_AUDIO_FRAME_QUEUE_LIMIT: usize = 128;
const D3D11VA_VIDEO_FRAME_QUEUE_LIMIT: usize = 8;
const SUBTITLE_FRAME_QUEUE_LIMIT: usize = 32;
const EXTERNAL_SUBTITLE_LOOKAHEAD: Duration = Duration::from_secs(5);
const DEFAULT_AUDIO_LEAD_TIME: Duration = Duration::from_millis(120);
const STREAMING_AUDIO_LEAD_TIME: Duration = Duration::from_millis(1500);
const DEMUX_PACKET_QUEUE_LIMIT: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlaybackQueueLimits {
    video_frames: usize,
    audio_frames: usize,
    subtitle_frames: usize,
}

impl PlaybackQueueLimits {
    fn for_request(request: &MediaRequest) -> Self {
        if request_uses_http_source(request) {
            Self {
                video_frames: STREAMING_VIDEO_FRAME_QUEUE_LIMIT,
                audio_frames: STREAMING_AUDIO_FRAME_QUEUE_LIMIT,
                subtitle_frames: SUBTITLE_FRAME_QUEUE_LIMIT,
            }
        } else {
            Self::default()
        }
    }
}

impl Default for PlaybackQueueLimits {
    fn default() -> Self {
        Self {
            video_frames: DEFAULT_VIDEO_FRAME_QUEUE_LIMIT,
            audio_frames: DEFAULT_AUDIO_FRAME_QUEUE_LIMIT,
            subtitle_frames: SUBTITLE_FRAME_QUEUE_LIMIT,
        }
    }
}

enum DemuxMessage {
    Packet {
        generation: u64,
        packet: ffmpeg::Packet,
    },
    Eof {
        generation: u64,
    },
    Error {
        generation: u64,
        message: String,
    },
}

enum DemuxCommand {
    Start {
        generation: u64,
    },
    SetSelection {
        generation: u64,
        selection: StreamSelection,
    },
    Seek {
        generation: u64,
        position: Duration,
    },
    Stop,
}

enum PumpInput {
    Packet(ffmpeg::Packet),
    Eof,
    Empty,
}

struct AsyncDemuxer {
    packets: Receiver<DemuxMessage>,
    commands: Sender<DemuxCommand>,
    generation: u64,
    active: bool,
}

impl AsyncDemuxer {
    fn spawn(demuxer: Demuxer) -> Self {
        let (packet_sender, packets) = bounded(DEMUX_PACKET_QUEUE_LIMIT);
        let (commands, command_receiver) = unbounded();
        thread::Builder::new()
            .name("erika-demux".to_string())
            .spawn(move || run_demux_worker(demuxer, packet_sender, command_receiver))
            .expect("spawn erika demux worker");
        Self {
            packets,
            commands,
            generation: 1,
            active: false,
        }
    }

    fn start(&mut self) -> Result<()> {
        if self.active {
            return Ok(());
        }
        self.commands
            .send(DemuxCommand::Start {
                generation: self.generation,
            })
            .map_err(|_| PlaybackError::DemuxWorker("demux command channel closed".to_string()))?;
        self.active = true;
        Ok(())
    }

    fn set_stream_selection(&mut self, selection: StreamSelection) -> Result<()> {
        self.generation = self.generation.saturating_add(1).max(1);
        self.active = false;
        self.drain_stale_packets();
        self.commands
            .send(DemuxCommand::SetSelection {
                generation: self.generation,
                selection,
            })
            .map_err(|_| PlaybackError::DemuxWorker("demux command channel closed".to_string()))
    }

    fn seek(&mut self, position: Duration) -> Result<()> {
        self.generation = self.generation.saturating_add(1).max(1);
        self.active = false;
        self.drain_stale_packets();
        self.commands
            .send(DemuxCommand::Seek {
                generation: self.generation,
                position,
            })
            .map_err(|_| PlaybackError::DemuxWorker("demux command channel closed".to_string()))
    }

    fn poll(&mut self) -> Result<PumpInput> {
        self.start()?;
        loop {
            match self.packets.try_recv() {
                Ok(DemuxMessage::Packet { generation, packet })
                    if generation == self.generation =>
                {
                    return Ok(PumpInput::Packet(packet));
                }
                Ok(DemuxMessage::Eof { generation }) if generation == self.generation => {
                    return Ok(PumpInput::Eof);
                }
                Ok(DemuxMessage::Error {
                    generation,
                    message,
                }) if generation == self.generation => {
                    return Err(PlaybackError::DemuxWorker(message));
                }
                Ok(_) => continue,
                Err(TryRecvError::Empty) => return Ok(PumpInput::Empty),
                Err(TryRecvError::Disconnected) => {
                    return Err(PlaybackError::DemuxWorker(
                        "demux packet channel closed".to_string(),
                    ));
                }
            }
        }
    }

    fn drain_stale_packets(&mut self) {
        while self.packets.try_recv().is_ok() {}
    }
}

impl Drop for AsyncDemuxer {
    fn drop(&mut self) {
        let _ = self.commands.send(DemuxCommand::Stop);
    }
}

fn run_demux_worker(
    mut demuxer: Demuxer,
    packets: Sender<DemuxMessage>,
    commands: Receiver<DemuxCommand>,
) {
    let mut generation = 1u64;
    let mut eof = false;
    let mut active = false;
    let mut pending_seek = None;
    loop {
        while let Ok(command) = commands.try_recv() {
            if !handle_demux_command(
                &mut demuxer,
                &packets,
                command,
                &mut generation,
                &mut eof,
                &mut active,
                &mut pending_seek,
            ) {
                return;
            }
        }

        if eof || !active {
            match commands.recv() {
                Ok(command) => {
                    if !handle_demux_command(
                        &mut demuxer,
                        &packets,
                        command,
                        &mut generation,
                        &mut eof,
                        &mut active,
                        &mut pending_seek,
                    ) {
                        return;
                    }
                }
                Err(_) => return,
            }
            continue;
        }

        let read_started = Instant::now();
        let message = match demuxer.read_packet() {
            Ok(Some(packet)) => {
                let read_elapsed = read_started.elapsed();
                if trace::enabled() && read_elapsed > Duration::from_millis(20) {
                    trace::log(format!(
                        "[erika-playback-trace] stage=demux_read_packet gen={} stream={} elapsed_ms={:.3} queue_len={}",
                        generation,
                        packet.stream_index(),
                        read_elapsed.as_secs_f64() * 1000.0,
                        packets.len(),
                    ));
                }
                DemuxMessage::Packet { generation, packet }
            }
            Ok(None) => {
                eof = true;
                trace::log(format!(
                    "[erika-playback-trace] stage=demux_read_eof gen={} elapsed_ms={:.3} queue_len={}",
                    generation,
                    read_started.elapsed().as_secs_f64() * 1000.0,
                    packets.len(),
                ));
                DemuxMessage::Eof { generation }
            }
            Err(error) => {
                eof = true;
                trace::log(format!(
                    "[erika-playback-trace] stage=demux_read_error gen={} elapsed_ms={:.3} queue_len={} error={}",
                    generation,
                    read_started.elapsed().as_secs_f64() * 1000.0,
                    packets.len(),
                    error,
                ));
                DemuxMessage::Error {
                    generation,
                    message: error.to_string(),
                }
            }
        };
        if !send_demux_message(
            &mut demuxer,
            &packets,
            &commands,
            message,
            &mut generation,
            &mut eof,
            &mut active,
            &mut pending_seek,
        ) {
            return;
        }
    }
}

fn send_demux_message(
    demuxer: &mut Demuxer,
    packets: &Sender<DemuxMessage>,
    commands: &Receiver<DemuxCommand>,
    mut message: DemuxMessage,
    generation: &mut u64,
    eof: &mut bool,
    active: &mut bool,
    pending_seek: &mut Option<(u64, Duration)>,
) -> bool {
    let message_generation = demux_message_generation(&message);
    let started = Instant::now();
    loop {
        while let Ok(command) = commands.try_recv() {
            trace_demux_send_wait(started, packets.len(), "command");
            if !handle_demux_command(
                demuxer,
                packets,
                command,
                generation,
                eof,
                active,
                pending_seek,
            ) {
                return false;
            }
            if !*active || *generation != message_generation {
                return true;
            }
        }

        match packets.try_send(message) {
            Ok(()) => {
                trace_demux_send_wait(started, packets.len(), "sent");
                return true;
            }
            Err(TrySendError::Disconnected(_)) => return false,
            Err(TrySendError::Full(returned)) => {
                message = returned;
                match commands.recv_timeout(Duration::from_millis(5)) {
                    Ok(command) => {
                        trace_demux_send_wait(started, packets.len(), "command_after_full");
                        if !handle_demux_command(
                            demuxer,
                            packets,
                            command,
                            generation,
                            eof,
                            active,
                            pending_seek,
                        ) {
                            return false;
                        }
                        if !*active || *generation != message_generation {
                            return true;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return false,
                }
            }
        }
    }
}

fn demux_message_generation(message: &DemuxMessage) -> u64 {
    match message {
        DemuxMessage::Packet { generation, .. }
        | DemuxMessage::Eof { generation }
        | DemuxMessage::Error { generation, .. } => *generation,
    }
}

fn trace_demux_send_wait(started: Instant, queue_len: usize, outcome: &'static str) {
    if trace::enabled() && started.elapsed() > Duration::from_millis(20) {
        trace::log(format!(
            "[erika-playback-trace] stage=demux_send_wait outcome={} elapsed_ms={:.3} queue_len={}",
            outcome,
            started.elapsed().as_secs_f64() * 1000.0,
            queue_len,
        ));
    }
}

fn handle_demux_command(
    demuxer: &mut Demuxer,
    packets: &Sender<DemuxMessage>,
    command: DemuxCommand,
    generation: &mut u64,
    eof: &mut bool,
    active: &mut bool,
    pending_seek: &mut Option<(u64, Duration)>,
) -> bool {
    match command {
        DemuxCommand::Start {
            generation: next_generation,
        } => {
            *generation = next_generation;
            if let Some((seek_generation, position)) = pending_seek.take() {
                *generation = seek_generation;
                *eof = false;
                if let Err(error) = demuxer.seek(position) {
                    let _ = packets.send(DemuxMessage::Error {
                        generation: *generation,
                        message: error.to_string(),
                    });
                    *eof = true;
                    *active = false;
                    return true;
                }
            }
            *active = true;
            true
        }
        DemuxCommand::SetSelection {
            generation: next_generation,
            selection,
        } => {
            *generation = next_generation;
            *eof = false;
            *active = false;
            *pending_seek = None;
            if let Err(error) = demuxer.set_stream_selection(selection) {
                let _ = packets.send(DemuxMessage::Error {
                    generation: *generation,
                    message: error.to_string(),
                });
                *eof = true;
            }
            true
        }
        DemuxCommand::Seek {
            generation: next_generation,
            position,
        } => {
            *generation = next_generation;
            *eof = false;
            *active = false;
            *pending_seek = Some((*generation, position));
            true
        }
        DemuxCommand::Stop => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoDecodePreference {
    Software,
    VideoToolbox,
    D3d11va,
}

impl VideoDecodePreference {
    fn decoder_config(self) -> DecoderConfig {
        match self {
            Self::Software => DecoderConfig::software(),
            Self::VideoToolbox => DecoderConfig::videotoolbox(),
            Self::D3d11va => DecoderConfig::d3d11va(),
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
impl Default for VideoDecodePreference {
    fn default() -> Self {
        Self::VideoToolbox
    }
}

#[cfg(target_os = "windows")]
impl Default for VideoDecodePreference {
    fn default() -> Self {
        Self::D3d11va
    }
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
impl Default for VideoDecodePreference {
    fn default() -> Self {
        Self::Software
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackSessionConfig {
    pub video_decode: VideoDecodePreference,
    pub audio_output: PcmFormat,
    pub timing: PlaybackTimingConfig,
}

impl Default for PlaybackSessionConfig {
    fn default() -> Self {
        Self {
            video_decode: VideoDecodePreference::default(),
            audio_output: PcmFormat::default(),
            timing: PlaybackTimingConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenedMediaInfo {
    pub uri: String,
    pub duration: Option<Duration>,
    pub tracks: Vec<TrackInfo>,
    pub video_params: Option<VideoParams>,
    pub selected_video_track: Option<i64>,
    pub selected_audio_track: Option<i64>,
    pub selected_subtitle_track: Option<i64>,
    pub subtitle_tracks: Vec<SubtitleTrackConfig>,
    pub video_decode_backend: Option<DecoderBackend>,
    pub audio_output: Option<PcmFormat>,
}

impl OpenedMediaInfo {
    pub fn track_selection(&self) -> TrackSelection {
        TrackSelection {
            video: self.selected_video_track,
            audio: self.selected_audio_track,
            subtitle: self.selected_subtitle_track,
        }
    }
}

pub struct PlaybackSession {
    demuxer: AsyncDemuxer,
    codec_parameters: Vec<OwnedCodecParameters>,
    video_decoder: Option<Decoder>,
    audio_decoder: Option<Decoder>,
    subtitle_decoder: Option<SubtitleDecoder>,
    external_subtitles: Vec<ExternalSubtitleSession>,
    audio_resampler: Option<AudioResampler>,
    audio_output: PcmFormat,
    info: OpenedMediaInfo,
    video_frames: VecDeque<Frame>,
    audio_frames: VecDeque<PcmAudioFrame>,
    subtitle_frames: VecDeque<DecodedSubtitleFrame>,
    pending_video_packets: VecDeque<ffmpeg::Packet>,
    queue_limits: PlaybackQueueLimits,
    eof: bool,
}

unsafe impl Send for PlaybackSession {}

impl PlaybackSession {
    pub fn open(request: &MediaRequest, config: PlaybackSessionConfig) -> Result<Self> {
        let queue_limits = PlaybackQueueLimits::for_request(request);
        let source = source_from_uri_with_hint(&request.uri, request.source_hint)?;
        let mut demuxer = Demuxer::open_source(source)?;
        let mut probe = demuxer.probe().clone();
        let codec_parameters = probe
            .tracks
            .iter()
            .map(|track| demuxer.owned_codec_parameters(track.id as i32))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let selected_video_track = demuxer
            .probe()
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Video)
            .map(|track| track.id as i32);
        let selected_audio_track = demuxer
            .probe()
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Audio)
            .map(|track| track.id as i32);
        let selected_subtitle_track = demuxer
            .probe()
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Subtitle)
            .map(|track| track.id as i32);

        let mut video_decoder = None;
        let mut selected_streams = Vec::new();
        if let Some(stream_index) = selected_video_track {
            selected_streams.push(stream_index);
            let parameters = codec_parameters_for(&codec_parameters, stream_index)?;
            let decoder_config = config.video_decode.decoder_config();
            video_decoder = Some(
                match Decoder::open_owned_with_config(parameters, decoder_config) {
                    Ok(decoder) => decoder,
                    Err(error)
                        if should_fallback_video_decoder_open_error(decoder_config.backend) =>
                    {
                        eprintln!(
                            "Erika playback {:?} decoder open failed: {error}; falling back to software",
                            decoder_config.backend
                        );
                        Decoder::open_owned_with_config(parameters, DecoderConfig::software())?
                    }
                    Err(error) => return Err(error.into()),
                },
            );
        }
        let mut audio_decoder = None;
        if let Some(stream_index) = selected_audio_track {
            selected_streams.push(stream_index);
            let parameters = codec_parameters_for(&codec_parameters, stream_index)?;
            audio_decoder = Some(Decoder::open_owned(parameters)?);
        }
        let mut subtitle_decoder = None;
        let mut opened_subtitle_track = None;
        if let Some(stream_index) = selected_subtitle_track {
            match codec_parameters_for(&codec_parameters, stream_index)
                .and_then(|parameters| SubtitleDecoder::open_owned(parameters).map_err(Into::into))
            {
                Ok(decoder) => {
                    selected_streams.push(stream_index);
                    opened_subtitle_track = Some(stream_index);
                    subtitle_decoder = Some(decoder);
                }
                Err(error) => {
                    eprintln!("Erika playback subtitle decoder open failed: {error}");
                }
            }
        }
        if !selected_streams.is_empty() {
            demuxer.set_stream_selection(StreamSelection::only(selected_streams))?;
        }

        let video_params = selected_video_track.and_then(|stream_index| {
            probe
                .video
                .iter()
                .find(|video| video.track_id == stream_index as i64)
                .map(|video| video.params.clone())
        });
        mark_selected_tracks(
            &mut probe.tracks,
            selected_video_track.map(i64::from),
            selected_audio_track.map(i64::from),
            opened_subtitle_track.map(i64::from),
        );
        let info = OpenedMediaInfo {
            uri: probe.uri,
            duration: probe.duration,
            tracks: probe.tracks,
            video_params,
            selected_video_track: selected_video_track.map(i64::from),
            selected_audio_track: selected_audio_track.map(i64::from),
            selected_subtitle_track: opened_subtitle_track.map(i64::from),
            subtitle_tracks: probe.subtitles,
            video_decode_backend: video_decoder.as_ref().map(Decoder::backend),
            audio_output: audio_decoder.as_ref().map(|_| config.audio_output),
        };

        Ok(Self {
            demuxer: AsyncDemuxer::spawn(demuxer),
            codec_parameters,
            video_decoder,
            audio_decoder,
            subtitle_decoder,
            external_subtitles: Vec::new(),
            audio_resampler: None,
            audio_output: config.audio_output,
            info,
            video_frames: VecDeque::new(),
            audio_frames: VecDeque::new(),
            subtitle_frames: VecDeque::new(),
            pending_video_packets: VecDeque::new(),
            queue_limits,
            eof: false,
        })
    }

    pub fn info(&self) -> &OpenedMediaInfo {
        &self.info
    }

    pub fn track_selection(&self) -> TrackSelection {
        self.info.track_selection()
    }

    pub fn is_eof(&self) -> bool {
        self.eof
    }

    pub fn next_video_frame(&mut self) -> Result<Option<Frame>> {
        if self.video_decoder.is_none() {
            return Ok(None);
        }
        while self.video_frames.is_empty() && !self.eof {
            if !self.pump_once()? {
                break;
            }
        }
        Ok(self.video_frames.pop_front())
    }

    pub fn next_audio_frame(&mut self) -> Result<Option<PcmAudioFrame>> {
        if self.audio_decoder.is_none() {
            return Ok(None);
        }
        while self.audio_frames.is_empty() && !self.eof {
            if !self.pump_once()? {
                break;
            }
        }
        Ok(self.audio_frames.pop_front())
    }

    pub fn next_audio_frame_bounded(
        &mut self,
        max_packets: usize,
        max_duration: Duration,
    ) -> Result<Option<PcmAudioFrame>> {
        if self.audio_decoder.is_none() {
            return Ok(None);
        }

        let started = Instant::now();
        let mut pumped_packets = 0usize;
        while self.audio_frames.is_empty() && !self.eof && pumped_packets < max_packets {
            if started.elapsed() >= max_duration {
                break;
            }
            if self.pump_once()? {
                pumped_packets = pumped_packets.saturating_add(1);
            } else {
                break;
            }
        }

        let frame = self.audio_frames.pop_front();
        if trace::enabled()
            && (pumped_packets > 0 || frame.is_none() || started.elapsed() >= max_duration)
        {
            trace::log(format!(
                "[erika-playback-trace] stage=session_audio_pump packets={} max_packets={} produced={} queued_audio={} eof={} pending_video={} elapsed_ms={:.3}",
                pumped_packets,
                max_packets,
                frame.is_some(),
                self.audio_frames.len(),
                self.eof,
                self.pending_video_packets.len(),
                started.elapsed().as_secs_f64() * 1000.0,
            ));
        }
        Ok(frame)
    }

    pub fn next_subtitle_frame(
        &mut self,
        media_time: Duration,
    ) -> Result<Option<DecodedSubtitleFrame>> {
        let Some(selected_track) = self.info.selected_subtitle_track else {
            return Ok(None);
        };
        if self.subtitle_decoder.is_none()
            && self.selected_external_subtitle(selected_track).is_none()
        {
            return Ok(None);
        }
        self.pump_external_subtitles(selected_track, media_time)?;
        Ok(self.pop_ready_subtitle(media_time))
    }

    pub fn add_external_subtitle(
        &mut self,
        config: SubtitleTrackConfig,
        media_time: Duration,
    ) -> Result<(SubtitleTrackConfig, Option<DecodedSubtitleFrame>)> {
        let previous_subtitle = self.info.selected_subtitle_track;
        let mut external = ExternalSubtitleSession::open(config)?;
        external.seek(media_time)?;
        let track = external.track().clone();
        let mut info = TrackInfo::external(track.id, TrackKind::Subtitle);
        info.title = track.title.clone();
        info.language = track.language.clone();
        info.selected = true;
        self.info.tracks.push(info);
        self.info.subtitle_tracks.push(track.clone());
        self.external_subtitles.push(external);
        self.select_subtitle_track_internal(Some(track.id))?;
        Ok((track, clear_subtitle_frame(previous_subtitle, media_time)))
    }

    pub fn remove_subtitle_track(
        &mut self,
        track_id: i64,
        media_time: Duration,
    ) -> Result<Option<DecodedSubtitleFrame>> {
        let was_selected = self.info.selected_subtitle_track == Some(track_id);
        let Some(index) = self
            .external_subtitles
            .iter()
            .position(|track| track.track().id == track_id)
        else {
            let is_embedded = self
                .info
                .subtitle_tracks
                .iter()
                .any(|track| track.id == track_id && !track.can_remove());
            return if is_embedded {
                Err(PlaybackError::SubtitleTrackNotRemovable(track_id))
            } else {
                Ok(None)
            };
        };
        self.external_subtitles.remove(index);
        self.subtitle_frames
            .retain(|frame| frame.track_id != track_id);
        self.info.tracks.retain(|track| track.id != track_id);
        self.info
            .subtitle_tracks
            .retain(|track| track.id != track_id);
        if was_selected {
            self.info.selected_subtitle_track = None;
            mark_selected_tracks(
                &mut self.info.tracks,
                self.info.selected_video_track,
                self.info.selected_audio_track,
                self.info.selected_subtitle_track,
            );
        }
        Ok(clear_subtitle_frame(Some(track_id), media_time))
    }

    pub fn select_audio_track(&mut self, track_id: Option<i64>) -> Result<()> {
        match track_id {
            Some(id) => {
                let stream_index = self.embedded_track_stream_index(id, TrackKind::Audio)?;
                let parameters = self.codec_parameters(stream_index)?;
                let decoder = Decoder::open_owned(parameters)?;
                self.audio_decoder = Some(decoder);
                self.info.selected_audio_track = Some(id);
                self.info.audio_output = Some(self.audio_output);
            }
            None => {
                self.audio_decoder = None;
                self.info.selected_audio_track = None;
                self.info.audio_output = None;
            }
        }
        self.audio_resampler = None;
        self.audio_frames.clear();
        self.update_demux_selection()?;
        self.mark_selected_tracks();
        Ok(())
    }

    pub fn select_subtitle_track(
        &mut self,
        track_id: Option<i64>,
        media_time: Duration,
    ) -> Result<Option<DecodedSubtitleFrame>> {
        let previous = self.info.selected_subtitle_track;
        self.select_subtitle_track_internal(track_id)?;
        Ok(clear_subtitle_frame(previous, media_time))
    }

    fn select_subtitle_track_internal(&mut self, track_id: Option<i64>) -> Result<()> {
        match track_id {
            Some(id) => match self.subtitle_track_source(id)? {
                SubtitleTrackSource::Embedded { stream_index } => {
                    let stream_index = stream_index_i32(stream_index, TrackKind::Subtitle, id)?;
                    let decoder =
                        SubtitleDecoder::open_owned(self.codec_parameters(stream_index)?)?;
                    self.subtitle_decoder = Some(decoder);
                    self.info.selected_subtitle_track = Some(id);
                }
                SubtitleTrackSource::External { .. } => {
                    self.subtitle_decoder = None;
                    self.info.selected_subtitle_track = Some(id);
                }
            },
            None => {
                self.subtitle_decoder = None;
                self.info.selected_subtitle_track = None;
            }
        }
        self.subtitle_frames.clear();
        self.update_demux_selection()?;
        self.mark_selected_tracks();
        Ok(())
    }

    pub fn seek(&mut self, position: Duration) -> Result<()> {
        self.demuxer.seek(position)?;
        if let Some(decoder) = &mut self.video_decoder {
            decoder.flush();
        }
        if let Some(decoder) = &mut self.audio_decoder {
            decoder.flush();
        }
        if let Some(decoder) = &mut self.subtitle_decoder {
            decoder.flush();
        }
        for external in &mut self.external_subtitles {
            external.seek(position)?;
        }
        self.audio_resampler = None;
        self.pending_video_packets.clear();
        self.video_frames.clear();
        self.audio_frames.clear();
        self.subtitle_frames.clear();
        self.eof = false;
        Ok(())
    }

    fn pump_external_subtitles(&mut self, track_id: i64, media_time: Duration) -> Result<()> {
        if let Some(external) = self
            .external_subtitles
            .iter_mut()
            .find(|track| track.track().id == track_id)
        {
            external.pump_until(media_time)?;
        }
        Ok(())
    }

    fn pop_ready_subtitle(&mut self, media_time: Duration) -> Option<DecodedSubtitleFrame> {
        let embedded_start = self.subtitle_frames.front().map(decoded_subtitle_start);
        let external_starts = self
            .external_subtitles
            .iter()
            .enumerate()
            .filter(|(_, external)| Some(external.track().id) == self.info.selected_subtitle_track)
            .filter_map(|(index, external)| external.peek_start().map(|start| (index, start)));
        let candidate = select_ready_subtitle(embedded_start, external_starts, media_time)?;

        match candidate {
            SubtitleQueueCandidate::Embedded { .. } => self.subtitle_frames.pop_front(),
            SubtitleQueueCandidate::External { index, .. } => {
                self.external_subtitles[index].pop_front()
            }
        }
    }

    fn selected_external_subtitle(&self, track_id: i64) -> Option<&ExternalSubtitleSession> {
        self.external_subtitles
            .iter()
            .find(|track| track.track().id == track_id)
    }

    fn embedded_track_stream_index(&self, track_id: i64, kind: TrackKind) -> Result<i32> {
        let Some(track) = self
            .info
            .tracks
            .iter()
            .find(|track| track.id == track_id && track.kind == kind)
        else {
            return Err(PlaybackError::TrackNotFound { kind, track_id });
        };
        stream_index_i32(track.id, kind, track_id)
    }

    fn codec_parameters(&self, stream_index: i32) -> Result<&OwnedCodecParameters> {
        codec_parameters_for(&self.codec_parameters, stream_index)
    }

    fn subtitle_track_source(&self, track_id: i64) -> Result<SubtitleTrackSource> {
        self.info
            .subtitle_tracks
            .iter()
            .find(|track| track.id == track_id)
            .map(|track| track.source.clone())
            .ok_or(PlaybackError::TrackNotFound {
                kind: TrackKind::Subtitle,
                track_id,
            })
    }

    fn update_demux_selection(&mut self) -> Result<()> {
        let mut streams = Vec::new();
        if let Some(track_id) = self.info.selected_video_track {
            streams.push(self.embedded_track_stream_index(track_id, TrackKind::Video)?);
        }
        if let Some(track_id) = self.info.selected_audio_track {
            streams.push(self.embedded_track_stream_index(track_id, TrackKind::Audio)?);
        }
        if let Some(track_id) = self.info.selected_subtitle_track {
            if let SubtitleTrackSource::Embedded { stream_index } =
                self.subtitle_track_source(track_id)?
            {
                streams.push(stream_index_i32(
                    stream_index,
                    TrackKind::Subtitle,
                    track_id,
                )?);
            }
        }
        if streams.is_empty() {
            self.demuxer.set_stream_selection(StreamSelection::all())?;
        } else {
            self.demuxer
                .set_stream_selection(StreamSelection::only(streams))?;
        }
        Ok(())
    }

    fn mark_selected_tracks(&mut self) {
        mark_selected_tracks(
            &mut self.info.tracks,
            self.info.selected_video_track,
            self.info.selected_audio_track,
            self.info.selected_subtitle_track,
        );
    }

    fn pump_once(&mut self) -> Result<bool> {
        if self.route_pending_video_packets()? {
            return Ok(true);
        }
        match self.demuxer.poll()? {
            PumpInput::Packet(packet) => {
                self.route_packet(packet)?;
                Ok(true)
            }
            PumpInput::Eof => {
                self.finish_decoders()?;
                Ok(true)
            }
            PumpInput::Empty => Ok(false),
        }
    }

    fn route_packet(&mut self, packet: ffmpeg::Packet) -> Result<()> {
        if self
            .video_decoder
            .as_ref()
            .is_some_and(|decoder| packet.stream_index() == decoder.stream_index())
        {
            if self.should_defer_video_packet() {
                self.pending_video_packets.push_back(packet);
                return Ok(());
            }
            let _ = self.route_video_packet(packet)?;
            return Ok(());
        }

        if self
            .audio_decoder
            .as_ref()
            .is_some_and(|decoder| packet.stream_index() == decoder.stream_index())
        {
            {
                let decoder = self.audio_decoder.as_mut().expect("audio decoder exists");
                decoder.send_packet(&packet)?;
            }
            self.drain_audio_frames()?;
            trim_audio_queue(&mut self.audio_frames, self.queue_limits.audio_frames);
            return Ok(());
        }

        if self
            .subtitle_decoder
            .as_ref()
            .is_some_and(|decoder| packet.stream_index() == decoder.stream_index())
        {
            let decoder = self
                .subtitle_decoder
                .as_mut()
                .expect("subtitle decoder exists");
            if let Some(frame) = decoder.decode_packet(&packet)? {
                self.subtitle_frames.push_back(frame);
                trim_subtitle_queue(&mut self.subtitle_frames, self.queue_limits.subtitle_frames);
            }
        }
        Ok(())
    }

    fn route_pending_video_packets(&mut self) -> Result<bool> {
        let mut routed_any = false;
        while !self.should_defer_video_packet() {
            let Some(packet) = self.pending_video_packets.pop_front() else {
                return Ok(routed_any);
            };
            if !self.route_video_packet(packet)? {
                return Ok(routed_any);
            }
            routed_any = true;
        }
        Ok(routed_any)
    }

    fn route_video_packet(&mut self, packet: ffmpeg::Packet) -> Result<bool> {
        let video_frame_limit = self.active_video_frame_queue_limit();
        let before_frames = self.video_frames.len();
        let decoder = self.video_decoder.as_mut().expect("video decoder exists");
        match decoder.send_packet(&packet) {
            Ok(()) => {
                drain_video_frames(decoder, &mut self.video_frames)?;
                trim_video_queue(&mut self.video_frames, video_frame_limit);
                Ok(true)
            }
            Err(error) if error.is_again() => {
                drain_video_frames(decoder, &mut self.video_frames)?;
                trim_video_queue(&mut self.video_frames, video_frame_limit);
                match decoder.send_packet(&packet) {
                    Ok(()) => {
                        drain_video_frames(decoder, &mut self.video_frames)?;
                        trim_video_queue(&mut self.video_frames, video_frame_limit);
                        Ok(true)
                    }
                    Err(error) if error.is_again() => {
                        self.pending_video_packets.push_front(packet);
                        Ok(self.video_frames.len() > before_frames)
                    }
                    Err(error) => Err(error.into()),
                }
            }
            Err(error) => Err(error.into()),
        }
    }

    fn should_defer_video_packet(&self) -> bool {
        self.video_decoder
            .as_ref()
            .is_some_and(|decoder| decoder.backend() == DecoderBackend::D3d11va)
            && self.video_frames.len() >= D3D11VA_VIDEO_FRAME_QUEUE_LIMIT
    }

    fn active_video_frame_queue_limit(&self) -> usize {
        if self
            .video_decoder
            .as_ref()
            .is_some_and(|decoder| decoder.backend() == DecoderBackend::D3d11va)
        {
            D3D11VA_VIDEO_FRAME_QUEUE_LIMIT
        } else {
            self.queue_limits.video_frames
        }
    }

    fn finish_decoders(&mut self) -> Result<()> {
        if self.eof {
            return Ok(());
        }
        while self.route_pending_video_packets()? {}
        let video_frame_limit = self.active_video_frame_queue_limit();
        if let Some(decoder) = &mut self.video_decoder {
            decoder.send_eof()?;
            drain_video_frames(decoder, &mut self.video_frames)?;
            trim_video_queue(&mut self.video_frames, video_frame_limit);
        }
        if self.audio_decoder.is_some() {
            {
                let decoder = self.audio_decoder.as_mut().expect("audio decoder exists");
                decoder.send_eof()?;
            }
            self.drain_audio_frames()?;
            trim_audio_queue(&mut self.audio_frames, self.queue_limits.audio_frames);
        }
        self.eof = true;
        Ok(())
    }

    fn drain_audio_frames(&mut self) -> Result<()> {
        loop {
            let output = {
                let decoder = self.audio_decoder.as_mut().expect("audio decoder exists");
                decoder.receive_frame()?
            };
            match output {
                DecoderOutputFrame::Frame(frame) => {
                    let pcm = self.convert_audio_frame(frame)?;
                    self.audio_frames.push_back(pcm);
                }
                DecoderOutputFrame::NeedMoreInput | DecoderOutputFrame::EndOfStream => {
                    return Ok(());
                }
            }
        }
    }

    fn convert_audio_frame(&mut self, frame: Frame) -> Result<PcmAudioFrame> {
        if self.audio_resampler.is_none() {
            self.audio_resampler = Some(AudioResampler::new_from_frame(&frame, self.audio_output)?);
        }
        self.audio_resampler
            .as_mut()
            .expect("audio resampler exists")
            .convert(&frame)
            .map_err(PlaybackError::from)
    }
}

enum SubtitleQueueCandidate {
    Embedded { start: Duration },
    External { index: usize, start: Duration },
}

impl SubtitleQueueCandidate {
    fn start(&self) -> Duration {
        match self {
            Self::Embedded { start } | Self::External { start, .. } => *start,
        }
    }
}

fn select_ready_subtitle(
    embedded_start: Option<Duration>,
    external_starts: impl IntoIterator<Item = (usize, Duration)>,
    media_time: Duration,
) -> Option<SubtitleQueueCandidate> {
    let embedded = embedded_start
        .filter(|start| *start <= media_time)
        .map(|start| SubtitleQueueCandidate::Embedded { start });
    external_starts
        .into_iter()
        .filter_map(|(index, start)| {
            (start <= media_time).then_some(SubtitleQueueCandidate::External { index, start })
        })
        .chain(embedded)
        .min_by_key(SubtitleQueueCandidate::start)
}

fn decoded_subtitle_start(frame: &DecodedSubtitleFrame) -> Duration {
    frame.start.unwrap_or(Duration::ZERO)
}

struct ExternalSubtitleSession {
    demuxer: Demuxer,
    decoder: SubtitleDecoder,
    track: SubtitleTrackConfig,
    frames: VecDeque<DecodedSubtitleFrame>,
    eof: bool,
}

impl ExternalSubtitleSession {
    fn open(mut config: SubtitleTrackConfig) -> Result<Self> {
        let uri = match &config.source {
            crate::subtitle::SubtitleTrackSource::External { uri } => uri.clone(),
            crate::subtitle::SubtitleTrackSource::Embedded { stream_index } => {
                return Err(PlaybackError::SubtitleTrackNotRemovable(*stream_index));
            }
        };
        let source = source_from_uri_with_hint(&uri, crate::core::MediaSourceHint::Auto)?;
        let mut demuxer = Demuxer::open_source(source)?;
        let stream_index = demuxer
            .probe()
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Subtitle)
            .map(|track| track.id as i32)
            .ok_or(PlaybackError::NoSubtitleTrack)?;
        let decoder = demuxer.open_subtitle_decoder(stream_index)?;
        demuxer.set_stream_selection(StreamSelection::only([stream_index]))?;

        if let Some(probed) = demuxer
            .probe()
            .subtitles
            .iter()
            .find(|track| track.source.is_embedded())
        {
            config.language = config.language.or_else(|| probed.language.clone());
            config.title = config.title.or_else(|| probed.title.clone());
        }
        if config.title.is_none() {
            config.title = external_subtitle_title(&uri);
        }

        Ok(Self {
            demuxer,
            decoder,
            track: config,
            frames: VecDeque::new(),
            eof: false,
        })
    }

    fn track(&self) -> &SubtitleTrackConfig {
        &self.track
    }

    fn pump_until(&mut self, media_time: Duration) -> Result<()> {
        let lookahead = media_time.saturating_add(EXTERNAL_SUBTITLE_LOOKAHEAD);
        while self.frames.len() < SUBTITLE_FRAME_QUEUE_LIMIT && !self.eof {
            if self
                .frames
                .back()
                .and_then(|frame| frame.start)
                .is_some_and(|start| start > lookahead)
            {
                break;
            }

            match self.demuxer.read_packet()? {
                Some(packet) => {
                    if let Some(frame) = self.decoder.decode_packet(&packet)? {
                        self.frames.push_back(frame.with_track_id(self.track.id));
                    }
                }
                None => self.eof = true,
            }
        }
        Ok(())
    }

    fn peek_start(&self) -> Option<Duration> {
        self.frames
            .front()
            .and_then(|frame| frame.start)
            .or_else(|| self.frames.front().map(|_| Duration::ZERO))
    }

    fn pop_front(&mut self) -> Option<DecodedSubtitleFrame> {
        self.frames.pop_front()
    }

    fn seek(&mut self, position: Duration) -> Result<()> {
        self.demuxer.seek(position)?;
        self.decoder.flush();
        self.frames.clear();
        self.eof = false;
        Ok(())
    }
}

fn external_subtitle_title(uri: &str) -> Option<String> {
    let leaf = uri
        .rsplit_once('/')
        .map(|(_, leaf)| leaf)
        .unwrap_or(uri)
        .trim();
    (!leaf.is_empty()).then_some(leaf.to_string())
}

fn codec_parameters_for(
    parameters: &[OwnedCodecParameters],
    stream_index: i32,
) -> Result<&OwnedCodecParameters> {
    parameters
        .iter()
        .find(|parameters| parameters.stream_index() == stream_index)
        .ok_or(PlaybackError::TrackNotFound {
            kind: TrackKind::Video,
            track_id: i64::from(stream_index),
        })
}

fn mark_selected_tracks(
    tracks: &mut [TrackInfo],
    selected_video: Option<i64>,
    selected_audio: Option<i64>,
    selected_subtitle: Option<i64>,
) {
    for track in tracks {
        track.selected = match track.kind {
            TrackKind::Video => selected_video == Some(track.id),
            TrackKind::Audio => selected_audio == Some(track.id),
            TrackKind::Subtitle => selected_subtitle == Some(track.id),
        };
    }
}

fn clear_subtitle_frame(
    track_id: Option<i64>,
    media_time: Duration,
) -> Option<DecodedSubtitleFrame> {
    track_id.map(|track_id| DecodedSubtitleFrame::new(track_id, Some(media_time), None))
}

fn stream_index_i32(stream_index: i64, kind: TrackKind, track_id: i64) -> Result<i32> {
    i32::try_from(stream_index).map_err(|_| PlaybackError::TrackNotFound { kind, track_id })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackRunState {
    Paused,
    Playing,
    Stopped,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackClockMode {
    Wall,
    AudioMaster,
}

impl Default for PlaybackClockMode {
    fn default() -> Self {
        Self::AudioMaster
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioSyncConfig {
    pub enabled: bool,
    pub deadband: Duration,
    pub max_correction_per_frame: Duration,
    pub snap_threshold: Duration,
}

impl Default for AudioSyncConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            deadband: Duration::from_millis(5),
            max_correction_per_frame: Duration::from_millis(5),
            snap_threshold: Duration::from_millis(250),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackTimingConfig {
    pub clock_mode: PlaybackClockMode,
    pub video_scheduler: VideoFrameScheduler,
    pub audio_lead_time: Duration,
    pub audio_sync: AudioSyncConfig,
}

impl Default for PlaybackTimingConfig {
    fn default() -> Self {
        Self {
            clock_mode: PlaybackClockMode::default(),
            video_scheduler: VideoFrameScheduler::default(),
            audio_lead_time: DEFAULT_AUDIO_LEAD_TIME,
            audio_sync: AudioSyncConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackClockSource {
    Wall,
    Audio,
    Display,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockCorrectionDirection {
    None,
    Forward,
    Backward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockCorrection {
    pub source: PlaybackClockSource,
    pub direction: ClockCorrectionDirection,
    pub drift: Duration,
    pub applied: Duration,
    pub snapped: bool,
}

impl ClockCorrection {
    pub const fn none(source: PlaybackClockSource) -> Self {
        Self {
            source,
            direction: ClockCorrectionDirection::None,
            drift: Duration::ZERO,
            applied: Duration::ZERO,
            snapped: false,
        }
    }
}

impl Default for PlaybackClockSource {
    fn default() -> Self {
        Self::Wall
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaybackClockSnapshot {
    pub media_time: Duration,
    pub source: PlaybackClockSource,
    pub rate: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackClock {
    base_media_time: Duration,
    anchor: Option<Instant>,
    rate: f64,
    source: PlaybackClockSource,
}

impl PlaybackClock {
    pub fn paused_at(media_time: Duration) -> Self {
        Self {
            base_media_time: media_time,
            anchor: None,
            rate: 1.0,
            source: PlaybackClockSource::Wall,
        }
    }

    pub fn running_at(media_time: Duration, now: Instant) -> Self {
        let mut clock = Self::paused_at(media_time);
        clock.anchor = Some(now);
        clock
    }

    pub fn media_time_at(&self, now: Instant) -> Duration {
        let Some(anchor) = self.anchor else {
            return self.base_media_time;
        };
        self.base_media_time
            .saturating_add(scale_duration(elapsed_since(anchor, now), self.rate))
    }

    pub fn snapshot_at(&self, now: Instant) -> PlaybackClockSnapshot {
        PlaybackClockSnapshot {
            media_time: self.media_time_at(now),
            source: self.source,
            rate: self.rate,
        }
    }

    pub fn is_running(&self) -> bool {
        self.anchor.is_some()
    }

    pub fn source(&self) -> PlaybackClockSource {
        self.source
    }

    pub fn rate(&self) -> f64 {
        self.rate
    }

    pub fn play(&mut self, now: Instant) {
        if self.anchor.is_none() {
            self.anchor = Some(now);
        }
    }

    pub fn pause(&mut self, now: Instant) {
        self.base_media_time = self.media_time_at(now);
        self.anchor = None;
    }

    pub fn seek(&mut self, media_time: Duration, now: Instant) {
        self.base_media_time = media_time;
        if self.anchor.is_some() {
            self.anchor = Some(now);
        }
    }

    pub fn reset(&mut self, media_time: Duration, running: bool, now: Instant) {
        self.base_media_time = media_time;
        self.anchor = running.then_some(now);
        self.source = PlaybackClockSource::Wall;
    }

    pub fn sync_to(&mut self, media_time: Duration, now: Instant, source: PlaybackClockSource) {
        self.base_media_time = media_time;
        if self.anchor.is_some() {
            self.anchor = Some(now);
        }
        self.source = source;
    }

    pub fn discipline_to(
        &mut self,
        reference_time: Duration,
        now: Instant,
        source: PlaybackClockSource,
        config: AudioSyncConfig,
    ) -> ClockCorrection {
        self.source = source;
        if !config.enabled {
            return ClockCorrection::none(source);
        }

        let current = self.media_time_at(now);
        let drift_nanos = duration_to_nanos(reference_time) - duration_to_nanos(current);
        let drift_abs = nanos_to_duration(drift_nanos.abs());
        let direction = if drift_nanos > 0 {
            ClockCorrectionDirection::Forward
        } else if drift_nanos < 0 {
            ClockCorrectionDirection::Backward
        } else {
            ClockCorrectionDirection::None
        };

        if drift_abs <= config.deadband || direction == ClockCorrectionDirection::None {
            return ClockCorrection {
                source,
                direction,
                drift: drift_abs,
                applied: Duration::ZERO,
                snapped: false,
            };
        }

        if drift_abs >= config.snap_threshold {
            self.sync_to(reference_time, now, source);
            return ClockCorrection {
                source,
                direction,
                drift: drift_abs,
                applied: drift_abs,
                snapped: true,
            };
        }

        let applied = drift_abs.min(config.max_correction_per_frame);
        let correction_nanos = match direction {
            ClockCorrectionDirection::Forward => duration_to_nanos(applied),
            ClockCorrectionDirection::Backward => -duration_to_nanos(applied),
            ClockCorrectionDirection::None => 0,
        };
        self.sync_to(add_signed_duration(current, correction_nanos), now, source);
        ClockCorrection {
            source,
            direction,
            drift: drift_abs,
            applied,
            snapped: false,
        }
    }

    pub fn set_rate(&mut self, rate: f64, now: Instant) {
        self.base_media_time = self.media_time_at(now);
        self.rate = sanitize_playback_rate(rate);
        if self.anchor.is_some() {
            self.anchor = Some(now);
        }
    }
}

impl Default for PlaybackClock {
    fn default() -> Self {
        Self::paused_at(Duration::ZERO)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoFrameDecision {
    Present { late_by: Option<Duration> },
    Wait { early_by: Duration },
    Drop { late_by: Duration },
}

impl VideoFrameDecision {
    pub fn late_by(self) -> Option<Duration> {
        match self {
            Self::Present { late_by } => late_by,
            Self::Drop { late_by } => Some(late_by),
            Self::Wait { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoFrameScheduler {
    pub lead_time: Duration,
    pub drop_tolerance: Duration,
    pub max_consecutive_drops: usize,
}

impl VideoFrameScheduler {
    pub fn new(lead_time: Duration, drop_tolerance: Duration) -> Self {
        Self {
            lead_time,
            drop_tolerance,
            max_consecutive_drops: 5,
        }
    }

    pub fn schedule(
        self,
        pts: Option<Duration>,
        media_time: Duration,
        first_frame: bool,
    ) -> VideoFrameDecision {
        let Some(pts) = pts else {
            return VideoFrameDecision::Present { late_by: None };
        };
        if first_frame {
            return VideoFrameDecision::Present {
                late_by: media_time.checked_sub(pts),
            };
        }
        if media_time.saturating_add(self.lead_time) < pts {
            return VideoFrameDecision::Wait {
                early_by: pts.saturating_sub(media_time.saturating_add(self.lead_time)),
            };
        }
        let late_by = media_time.checked_sub(pts);
        if late_by.is_some_and(|late| late > self.drop_tolerance) {
            return VideoFrameDecision::Drop {
                late_by: late_by.expect("checked above"),
            };
        }
        VideoFrameDecision::Present { late_by }
    }
}

impl Default for VideoFrameScheduler {
    fn default() -> Self {
        Self::new(Duration::from_millis(4), Duration::from_millis(120))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplaySyncConfig {
    pub enabled: bool,
    pub vsync_interval: Duration,
    pub allow_zero_vsyncs: bool,
}

impl DisplaySyncConfig {
    pub fn for_refresh_rate_hz(refresh_rate_hz: f64) -> Self {
        let interval = if refresh_rate_hz.is_finite() && refresh_rate_hz > 0.0 {
            Duration::from_secs_f64(1.0 / refresh_rate_hz)
        } else {
            Duration::from_millis(16)
        };
        Self {
            vsync_interval: interval,
            ..Self::default()
        }
    }
}

impl Default for DisplaySyncConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            vsync_interval: Duration::from_nanos(16_666_667),
            allow_zero_vsyncs: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DisplaySyncState {
    residual_error_nanos: i128,
}

impl DisplaySyncState {
    pub fn reset(&mut self) {
        self.residual_error_nanos = 0;
    }

    pub fn residual_error_nanos(&self) -> i128 {
        self.residual_error_nanos
    }

    pub fn schedule_frame(
        &mut self,
        frame_duration: Duration,
        config: DisplaySyncConfig,
    ) -> DisplayFrameSchedule {
        if !config.enabled || config.vsync_interval.is_zero() {
            return DisplayFrameSchedule {
                vsyncs: 1,
                scheduled_duration: frame_duration,
                residual_error_nanos: self.residual_error_nanos,
            };
        }

        let vsync_nanos = duration_to_nanos(config.vsync_interval).max(1);
        let target_nanos = duration_to_nanos(frame_duration) + self.residual_error_nanos;
        let rounded = if target_nanos <= 0 {
            0
        } else {
            (target_nanos + vsync_nanos / 2) / vsync_nanos
        };
        let min_vsyncs = if config.allow_zero_vsyncs { 0 } else { 1 };
        let vsyncs = rounded.max(min_vsyncs).min(u32::MAX as i128) as u32;
        let scheduled_nanos = vsyncs as i128 * vsync_nanos;
        self.residual_error_nanos = target_nanos - scheduled_nanos;

        DisplayFrameSchedule {
            vsyncs,
            scheduled_duration: nanos_to_duration(scheduled_nanos),
            residual_error_nanos: self.residual_error_nanos,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayFrameSchedule {
    pub vsyncs: u32,
    pub scheduled_duration: Duration,
    pub residual_error_nanos: i128,
}

pub struct TimedVideoFrame {
    pub frame: Frame,
    pub pts: Option<Duration>,
    pub media_time: Duration,
    pub late_by: Option<Duration>,
}

pub struct TimedAudioFrame {
    pub frame: PcmAudioFrame,
    pub pts: Option<Duration>,
    pub media_time: Duration,
    pub late_by: Option<Duration>,
}

pub struct TimedSubtitleFrame {
    pub frame: DecodedSubtitleFrame,
    pub pts: Option<Duration>,
    pub media_time: Duration,
    pub late_by: Option<Duration>,
}

pub struct VideoPlaybackEngine {
    session: PlaybackSession,
    state: PlaybackRunState,
    clock: PlaybackClock,
    timing: PlaybackTimingConfig,
    pending_frame: Option<Frame>,
    pending_audio: Option<PcmAudioFrame>,
    pending_subtitle: Option<DecodedSubtitleFrame>,
    last_presented_pts: Option<Duration>,
    eof: bool,
    waiting_for_first_frame: bool,
    seek_floor: Option<Duration>,
}

unsafe impl Send for VideoPlaybackEngine {}

impl VideoPlaybackEngine {
    pub fn open(request: &MediaRequest, config: PlaybackSessionConfig) -> Result<Self> {
        let timing = playback_timing_for_request(request, config.timing);
        Ok(Self::from_session_with_timing(
            PlaybackSession::open(request, config)?,
            timing,
        ))
    }

    pub fn from_session(session: PlaybackSession) -> Self {
        Self::from_session_with_timing(session, PlaybackTimingConfig::default())
    }

    pub fn from_session_with_timing(
        session: PlaybackSession,
        timing: PlaybackTimingConfig,
    ) -> Self {
        Self {
            session,
            state: PlaybackRunState::Paused,
            clock: PlaybackClock::default(),
            timing,
            pending_frame: None,
            pending_audio: None,
            pending_subtitle: None,
            last_presented_pts: None,
            eof: false,
            waiting_for_first_frame: false,
            seek_floor: None,
        }
    }

    pub fn info(&self) -> &OpenedMediaInfo {
        self.session.info()
    }

    pub fn track_selection(&self) -> TrackSelection {
        self.session.track_selection()
    }

    pub fn add_external_subtitle(
        &mut self,
        config: SubtitleTrackConfig,
    ) -> Result<(SubtitleTrackConfig, Option<TimedSubtitleFrame>)> {
        let media_time = self.media_time();
        let (track, clear_frame) = self.session.add_external_subtitle(config, media_time)?;
        self.pending_subtitle = None;
        Ok((
            track,
            clear_frame.map(|frame| TimedSubtitleFrame {
                frame,
                pts: Some(media_time),
                media_time,
                late_by: None,
            }),
        ))
    }

    pub fn remove_subtitle_track(&mut self, track_id: i64) -> Result<Option<TimedSubtitleFrame>> {
        let media_time = self.media_time();
        let Some(frame) = self.session.remove_subtitle_track(track_id, media_time)? else {
            return Ok(None);
        };
        self.pending_subtitle = None;
        Ok(Some(TimedSubtitleFrame {
            frame,
            pts: Some(media_time),
            media_time,
            late_by: None,
        }))
    }

    pub fn select_audio_track(&mut self, track_id: Option<i64>) -> Result<()> {
        let media_time = self.media_time();
        self.session.select_audio_track(track_id)?;
        self.reset_streams_at(media_time)?;
        Ok(())
    }

    pub fn select_subtitle_track(
        &mut self,
        track_id: Option<i64>,
    ) -> Result<Option<TimedSubtitleFrame>> {
        let media_time = self.media_time();
        let frame = self.session.select_subtitle_track(track_id, media_time)?;
        self.reset_streams_at(media_time)?;
        Ok(frame.map(|frame| TimedSubtitleFrame {
            frame,
            pts: Some(media_time),
            media_time,
            late_by: None,
        }))
    }

    fn reset_streams_at(&mut self, media_time: Duration) -> Result<()> {
        self.session.seek(media_time)?;
        let now = Instant::now();
        let before = self.clock.media_time_at(now);
        self.clock
            .reset(media_time, self.state == PlaybackRunState::Playing, now);
        trace_clock_reset("reset_streams_at", before, media_time, self.state);
        self.pending_frame = None;
        self.pending_audio = None;
        self.pending_subtitle = None;
        self.last_presented_pts = None;
        self.eof = false;
        self.waiting_for_first_frame = self.state == PlaybackRunState::Playing;
        self.seek_floor = Some(media_time);
        Ok(())
    }

    pub fn state(&self) -> PlaybackRunState {
        self.state
    }

    pub(crate) fn should_prefill_audio(&self) -> bool {
        self.state == PlaybackRunState::Playing
    }

    pub fn play(&mut self) {
        if matches!(
            self.state,
            PlaybackRunState::Playing | PlaybackRunState::Ended
        ) {
            return;
        }
        let now = Instant::now();
        let before = self.clock.media_time_at(now);
        let waiting_for_first_frame = self.last_presented_pts.is_none();
        if !waiting_for_first_frame {
            self.clock.play(now);
        }
        trace::log(format!(
            "[erika-clock-trace] stage=engine_play before={} after={} state_before={:?} waiting_for_first_frame={}",
            trace::duration_label(Some(before)),
            trace::duration_label(Some(self.clock.media_time_at(now))),
            self.state,
            waiting_for_first_frame,
        ));
        self.state = PlaybackRunState::Playing;
        self.waiting_for_first_frame = waiting_for_first_frame;
    }

    pub fn pause(&mut self) {
        if self.state != PlaybackRunState::Playing {
            return;
        }
        let now = Instant::now();
        let before = self.clock.media_time_at(now);
        self.clock.pause(now);
        trace::log(format!(
            "[erika-clock-trace] stage=engine_pause before={} after={}",
            trace::duration_label(Some(before)),
            trace::duration_label(Some(self.clock.media_time_at(now))),
        ));
        self.state = PlaybackRunState::Paused;
    }

    pub fn stop(&mut self) {
        let now = Instant::now();
        let before = self.clock.media_time_at(now);
        self.clock.reset(Duration::ZERO, false, now);
        trace_clock_reset("stop", before, Duration::ZERO, self.state);
        self.pending_frame = None;
        self.pending_audio = None;
        self.pending_subtitle = None;
        self.last_presented_pts = None;
        self.state = PlaybackRunState::Stopped;
        self.eof = false;
        self.waiting_for_first_frame = false;
        self.seek_floor = None;
    }

    pub fn seek(&mut self, position: Duration) -> Result<()> {
        self.session.seek(position)?;
        let now = Instant::now();
        let before = self.clock.media_time_at(now);
        self.clock
            .reset(position, self.state == PlaybackRunState::Playing, now);
        trace_clock_reset("seek", before, position, self.state);
        self.pending_frame = None;
        self.pending_audio = None;
        self.pending_subtitle = None;
        self.last_presented_pts = None;
        self.eof = false;
        self.waiting_for_first_frame = self.state == PlaybackRunState::Playing;
        self.seek_floor = Some(position);
        Ok(())
    }

    pub fn media_time(&self) -> Duration {
        self.clock.media_time_at(Instant::now())
    }

    pub fn clock_snapshot(&self) -> PlaybackClockSnapshot {
        self.clock.snapshot_at(Instant::now())
    }

    pub fn timing_config(&self) -> PlaybackTimingConfig {
        self.timing
    }

    pub fn set_timing_config(&mut self, timing: PlaybackTimingConfig) {
        self.timing = timing;
    }

    pub fn set_playback_rate(&mut self, rate: f64) {
        let now = Instant::now();
        let before = self.clock.media_time_at(now);
        let before_rate = self.clock.rate();
        self.clock.set_rate(rate, now);
        trace::log(format!(
            "[erika-clock-trace] stage=engine_set_rate before={} after={} rate_before={:.3} rate_after={:.3}",
            trace::duration_label(Some(before)),
            trace::duration_label(Some(self.clock.media_time_at(now))),
            before_rate,
            self.clock.rate(),
        ));
    }

    pub fn sync_to_audio_clock(&mut self, snapshot: AudioClockSnapshot) -> Option<ClockCorrection> {
        if self.state != PlaybackRunState::Playing {
            return None;
        }
        let media_time = snapshot.media_time?;
        let now = Instant::now();
        let before = self.clock.media_time_at(now);
        let correction = self.clock.discipline_to(
            media_time,
            now,
            PlaybackClockSource::Audio,
            self.timing.audio_sync,
        );
        let after = self.clock.media_time_at(now);
        trace_clock_correction(
            "output_audio_clock",
            before,
            media_time,
            after,
            correction,
            Some(snapshot),
        );
        Some(correction)
    }

    pub fn next_audio_frame(&mut self) -> Result<Option<PcmAudioFrame>> {
        self.session.next_audio_frame()
    }

    pub fn tick_audio(&mut self) -> Result<Option<TimedAudioFrame>> {
        if self.state != PlaybackRunState::Playing {
            return Ok(None);
        }
        self.ensure_pending_audio()?;
        let Some(frame) = self.pending_audio.as_ref() else {
            return Ok(None);
        };

        let pts = frame.pts;
        let now = Instant::now();
        let media_time = self.clock.media_time_at(now);
        if pts.is_some_and(|pts| pts > media_time + self.timing.audio_lead_time) {
            return Ok(None);
        }

        let frame = self.pending_audio.take().expect("pending audio exists");
        let late_by = pts.and_then(|pts| media_time.checked_sub(pts));
        Ok(Some(TimedAudioFrame {
            frame,
            pts,
            media_time,
            late_by,
        }))
    }

    pub fn tick_audio_bounded(
        &mut self,
        max_packets: usize,
        max_decode_duration: Duration,
    ) -> Result<Option<TimedAudioFrame>> {
        if self.state != PlaybackRunState::Playing {
            return Ok(None);
        }
        self.ensure_pending_audio_bounded(max_packets, max_decode_duration)?;
        let Some(frame) = self.pending_audio.as_ref() else {
            return Ok(None);
        };

        let pts = frame.pts;
        let now = Instant::now();
        let media_time = self.clock.media_time_at(now);
        if pts.is_some_and(|pts| pts > media_time + self.timing.audio_lead_time) {
            return Ok(None);
        }

        let frame = self.pending_audio.take().expect("pending audio exists");
        let late_by = pts.and_then(|pts| media_time.checked_sub(pts));
        Ok(Some(TimedAudioFrame {
            frame,
            pts,
            media_time,
            late_by,
        }))
    }

    pub fn tick_subtitle(&mut self) -> Result<Option<TimedSubtitleFrame>> {
        if self.state != PlaybackRunState::Playing {
            return Ok(None);
        }
        let media_time = self.media_time();
        self.ensure_pending_subtitle(media_time)?;
        let Some(frame) = self.pending_subtitle.as_ref() else {
            return Ok(None);
        };

        let pts = frame.start;
        if pts.is_some_and(|pts| pts > media_time) {
            return Ok(None);
        }

        let frame = self
            .pending_subtitle
            .take()
            .expect("pending subtitle exists");
        let late_by = pts.and_then(|pts| media_time.checked_sub(pts));
        Ok(Some(TimedSubtitleFrame {
            frame,
            pts,
            media_time,
            late_by,
        }))
    }

    pub fn tick(&mut self) -> Result<Option<TimedVideoFrame>> {
        if self.state != PlaybackRunState::Playing {
            return Ok(None);
        }
        let mut consecutive_drops = 0usize;
        loop {
            self.ensure_pending_frame()?;
            let Some(frame) = self.pending_frame.as_ref() else {
                return Ok(None);
            };

            let pts = frame.pts().and_then(|pts| pts.as_duration());
            if self.should_drop_seek_preroll(pts) {
                let _ = self.pending_frame.take();
                continue;
            }
            let should_present_first = self.last_presented_pts.is_none();
            if should_present_first && self.waiting_for_first_frame {
                let now = Instant::now();
                let before = self.clock.media_time_at(now);
                self.clock.sync_to(
                    pts.unwrap_or(Duration::ZERO),
                    now,
                    PlaybackClockSource::Wall,
                );
                self.clock.play(now);
                trace::log(format!(
                    "[erika-clock-trace] stage=first_video_sync pts={} before={} after={} state={:?}",
                    trace::duration_label(pts),
                    trace::duration_label(Some(before)),
                    trace::duration_label(Some(self.clock.media_time_at(now))),
                    self.state,
                ));
                self.waiting_for_first_frame = false;
            }

            let media_time = self.media_time();
            match self
                .timing
                .video_scheduler
                .schedule(pts, media_time, should_present_first)
            {
                VideoFrameDecision::Wait { .. } => return Ok(None),
                VideoFrameDecision::Drop { .. }
                    if consecutive_drops < self.timing.video_scheduler.max_consecutive_drops =>
                {
                    let _ = self.pending_frame.take();
                    consecutive_drops += 1;
                }
                decision => {
                    let frame = self.pending_frame.take().expect("pending frame exists");
                    self.last_presented_pts = pts;
                    return Ok(Some(TimedVideoFrame {
                        frame,
                        pts,
                        media_time,
                        late_by: decision.late_by(),
                    }));
                }
            }
        }
    }

    fn should_drop_seek_preroll(&mut self, pts: Option<Duration>) -> bool {
        let Some(target) = self.seek_floor else {
            return false;
        };
        let Some(pts) = pts else {
            self.seek_floor = None;
            return false;
        };
        if pts < target {
            true
        } else {
            self.seek_floor = None;
            false
        }
    }

    fn ensure_pending_frame(&mut self) -> Result<()> {
        if self.pending_frame.is_some() || self.eof {
            return Ok(());
        }
        self.pending_frame = self.session.next_video_frame()?;
        if self.pending_frame.is_none() && self.session.is_eof() {
            self.eof = true;
            self.state = PlaybackRunState::Ended;
            let now = Instant::now();
            let media_time = self.info().duration.unwrap_or_else(|| self.media_time());
            let before = self.clock.media_time_at(now);
            self.clock.reset(media_time, false, now);
            trace_clock_reset("eof", before, media_time, self.state);
        }
        Ok(())
    }

    fn ensure_pending_audio(&mut self) -> Result<()> {
        if self.pending_audio.is_some() || self.eof {
            return Ok(());
        }
        self.pending_audio = self.session.next_audio_frame()?;
        Ok(())
    }

    fn ensure_pending_audio_bounded(
        &mut self,
        max_packets: usize,
        max_decode_duration: Duration,
    ) -> Result<()> {
        if self.pending_audio.is_some() || self.eof {
            return Ok(());
        }
        self.pending_audio = self
            .session
            .next_audio_frame_bounded(max_packets, max_decode_duration)?;
        Ok(())
    }

    fn ensure_pending_subtitle(&mut self, media_time: Duration) -> Result<()> {
        if self.pending_subtitle.is_some() || self.eof {
            return Ok(());
        }
        self.pending_subtitle = self.session.next_subtitle_frame(media_time)?;
        Ok(())
    }
}

fn playback_timing_for_request(
    request: &MediaRequest,
    mut timing: PlaybackTimingConfig,
) -> PlaybackTimingConfig {
    if request_uses_http_source(request) && timing.audio_lead_time == DEFAULT_AUDIO_LEAD_TIME {
        timing.audio_lead_time = STREAMING_AUDIO_LEAD_TIME;
    }
    timing
}

fn request_uses_http_source(request: &MediaRequest) -> bool {
    match request.source_hint {
        MediaSourceHint::Http => true,
        MediaSourceHint::Auto => is_http_uri(&request.uri),
        MediaSourceHint::LocalFile => false,
    }
}

fn is_http_uri(uri: &str) -> bool {
    uri.starts_with("http://") || uri.starts_with("https://")
}

fn trace_clock_reset(
    stage: &'static str,
    before: Duration,
    target: Duration,
    state: PlaybackRunState,
) {
    trace::log(format!(
        "[erika-clock-trace] stage=clock_reset:{stage} before={} target={} delta={:.3} state={:?} back={}",
        trace::duration_label(Some(before)),
        trace::duration_label(Some(target)),
        trace::duration_diff(before, target).as_secs_f64(),
        state,
        trace::duration_regressed(target, before),
    ));
}

fn trace_clock_correction(
    stage: &'static str,
    before: Duration,
    reference: Duration,
    after: Duration,
    correction: ClockCorrection,
    snapshot: Option<AudioClockSnapshot>,
) {
    let should_log = correction.direction != ClockCorrectionDirection::None
        || correction.snapped
        || trace::duration_regressed(after, before)
        || trace::duration_diff(before, reference) > Duration::from_millis(50);
    if !should_log {
        return;
    }
    let snapshot_suffix = snapshot.map_or_else(String::new, |snapshot| {
        format!(
            " queued={} queued_frames={} read={} written={} underflow={}",
            trace::duration_label(snapshot.queued_duration),
            snapshot.queued_frames,
            snapshot.read_frames,
            snapshot.written_frames,
            snapshot.underflow_frames,
        )
    });
    trace::log(format!(
        "[erika-clock-trace] stage=clock_discipline:{stage} before={} reference={} after={} drift={} applied={} direction={:?} snapped={} back={}{}",
        trace::duration_label(Some(before)),
        trace::duration_label(Some(reference)),
        trace::duration_label(Some(after)),
        trace::duration_label(Some(correction.drift)),
        trace::duration_label(Some(correction.applied)),
        correction.direction,
        correction.snapped,
        trace::duration_regressed(after, before),
        snapshot_suffix,
    ));
}

fn drain_video_frames(decoder: &mut Decoder, frames: &mut VecDeque<Frame>) -> Result<()> {
    loop {
        match decoder.receive_frame()? {
            DecoderOutputFrame::Frame(frame) => frames.push_back(frame),
            DecoderOutputFrame::NeedMoreInput | DecoderOutputFrame::EndOfStream => return Ok(()),
        }
    }
}

fn trim_video_queue(frames: &mut VecDeque<Frame>, limit: usize) {
    while frames.len() > limit {
        let _ = frames.pop_back();
    }
}

fn trim_audio_queue(frames: &mut VecDeque<PcmAudioFrame>, limit: usize) {
    while frames.len() > limit {
        let _ = frames.pop_front();
    }
}

fn trim_subtitle_queue(frames: &mut VecDeque<DecodedSubtitleFrame>, limit: usize) {
    while frames.len() > limit {
        let _ = frames.pop_front();
    }
}

fn sanitize_playback_rate(rate: f64) -> f64 {
    if rate.is_finite() && rate > 0.0 {
        rate
    } else {
        1.0
    }
}

fn should_fallback_video_decoder_open_error(backend: DecoderBackend) -> bool {
    matches!(backend, DecoderBackend::D3d11va)
}

fn elapsed_since(anchor: Instant, now: Instant) -> Duration {
    now.checked_duration_since(anchor).unwrap_or(Duration::ZERO)
}

fn scale_duration(duration: Duration, rate: f64) -> Duration {
    let seconds = duration.as_secs_f64() * sanitize_playback_rate(rate);
    if seconds.is_finite() && seconds > 0.0 {
        Duration::from_secs_f64(seconds)
    } else {
        Duration::ZERO
    }
}

fn duration_to_nanos(duration: Duration) -> i128 {
    duration.as_nanos().min(i128::MAX as u128) as i128
}

fn nanos_to_duration(nanos: i128) -> Duration {
    Duration::from_nanos(nanos.max(0).min(u64::MAX as i128) as u64)
}

fn add_signed_duration(base: Duration, delta_nanos: i128) -> Duration {
    nanos_to_duration(duration_to_nanos(base).saturating_add(delta_nanos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn decoder_open_fallback_is_limited_to_d3d11va() {
        assert!(should_fallback_video_decoder_open_error(
            DecoderBackend::D3d11va
        ));
        assert!(!should_fallback_video_decoder_open_error(
            DecoderBackend::VideoToolbox
        ));
        assert!(!should_fallback_video_decoder_open_error(
            DecoderBackend::Software
        ));
    }

    #[test]
    fn opened_media_info_keeps_probe_summary() {
        let mut video_track = TrackInfo::embedded(0, TrackKind::Video);
        video_track.codec = Some("hevc".to_string());
        let info = OpenedMediaInfo {
            uri: "file:///tmp/test.mp4".to_string(),
            duration: Some(Duration::from_secs(12)),
            tracks: vec![video_track],
            video_params: Some(VideoParams {
                width: 3840,
                height: 2160,
                primaries: crate::core::ColorPrimaries::Bt2020,
                transfer: crate::core::TransferFunction::Pq,
            }),
            selected_video_track: Some(0),
            selected_audio_track: Some(1),
            selected_subtitle_track: Some(2),
            subtitle_tracks: vec![crate::subtitle::SubtitleTrackConfig::embedded(2, 2)],
            video_decode_backend: Some(DecoderBackend::Software),
            audio_output: Some(PcmFormat::default()),
        };

        assert_eq!(info.duration, Some(Duration::from_secs(12)));
        assert_eq!(info.tracks.len(), 1);
        assert_eq!(
            info.video_params.as_ref().map(|params| params.width),
            Some(3840)
        );
        assert_eq!(info.selected_video_track, Some(0));
        assert_eq!(info.selected_audio_track, Some(1));
        assert_eq!(info.selected_subtitle_track, Some(2));
        assert_eq!(info.subtitle_tracks.len(), 1);
        assert_eq!(info.video_decode_backend, Some(DecoderBackend::Software));
        assert_eq!(info.audio_output, Some(PcmFormat::default()));
    }

    #[test]
    fn subtitle_queue_selects_ready_external_before_future_embedded() {
        let candidate = select_ready_subtitle(
            Some(Duration::from_secs(10)),
            [(3, Duration::from_secs(1))],
            Duration::from_secs(2),
        )
        .unwrap();

        assert!(matches!(
            candidate,
            SubtitleQueueCandidate::External {
                index: 3,
                start
            } if start == Duration::from_secs(1)
        ));
    }

    #[test]
    fn subtitle_queue_selects_earliest_ready_candidate() {
        let candidate = select_ready_subtitle(
            Some(Duration::from_secs(1)),
            [(0, Duration::from_secs(2))],
            Duration::from_secs(3),
        )
        .unwrap();

        assert!(matches!(
            candidate,
            SubtitleQueueCandidate::Embedded { start } if start == Duration::from_secs(1)
        ));
    }

    #[test]
    fn subtitle_queue_waits_when_no_candidate_is_ready() {
        assert!(
            select_ready_subtitle(
                Some(Duration::from_secs(5)),
                [(0, Duration::from_secs(6))],
                Duration::from_secs(4),
            )
            .is_none()
        );
    }

    #[test]
    fn external_subtitle_session_decodes_text_frames_with_external_track_id() {
        let path = std::env::temp_dir().join(format!(
            "erika-external-subtitle-{}.srt",
            std::process::id()
        ));
        fs::write(
            &path,
            "1\n00:00:01,000 --> 00:00:03,000\nExternal subtitle\n",
        )
        .unwrap();
        let config = SubtitleTrackConfig::external(1_000_007, path.to_string_lossy());

        let mut external = ExternalSubtitleSession::open(config).unwrap();
        external.pump_until(Duration::from_secs(2)).unwrap();
        let frame = external.pop_front().unwrap();

        assert_eq!(external.track().id, 1_000_007);
        assert_eq!(frame.track_id, 1_000_007);
        assert_eq!(frame.start, Some(Duration::from_secs(1)));
        assert_eq!(frame.end, Some(Duration::from_secs(3)));
        assert_eq!(frame.text.len(), 1);
        assert_eq!(frame.text[0].display_text(), "External subtitle");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn playback_clock_tracks_pause_seek_and_rate() {
        let t0 = Instant::now();
        let mut clock = PlaybackClock::paused_at(Duration::from_secs(10));

        assert_eq!(clock.media_time_at(t0), Duration::from_secs(10));
        clock.play(t0);
        assert_eq!(
            clock.media_time_at(t0 + Duration::from_millis(250)),
            Duration::from_millis(10_250)
        );

        clock.pause(t0 + Duration::from_millis(500));
        assert_eq!(
            clock.media_time_at(t0 + Duration::from_secs(10)),
            Duration::from_millis(10_500)
        );

        clock.seek(Duration::from_secs(2), t0 + Duration::from_secs(10));
        clock.play(t0 + Duration::from_secs(10));
        clock.set_rate(2.0, t0 + Duration::from_secs(11));

        assert_eq!(
            clock.media_time_at(t0 + Duration::from_secs(12)),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn playback_clock_disciplines_toward_audio_master() {
        let t0 = Instant::now();
        let mut clock = PlaybackClock::running_at(Duration::from_secs(10), t0);
        let config = AudioSyncConfig {
            deadband: Duration::from_millis(5),
            max_correction_per_frame: Duration::from_millis(20),
            snap_threshold: Duration::from_millis(250),
            ..AudioSyncConfig::default()
        };

        let correction = clock.discipline_to(
            Duration::from_millis(10_100),
            t0 + Duration::from_millis(50),
            PlaybackClockSource::Audio,
            config,
        );

        assert_eq!(correction.source, PlaybackClockSource::Audio);
        assert_eq!(correction.direction, ClockCorrectionDirection::Forward);
        assert_eq!(correction.drift, Duration::from_millis(50));
        assert_eq!(correction.applied, Duration::from_millis(20));
        assert!(!correction.snapped);
        assert_eq!(clock.source(), PlaybackClockSource::Audio);
        assert_eq!(
            clock.media_time_at(t0 + Duration::from_millis(50)),
            Duration::from_millis(10_070)
        );
    }

    #[test]
    fn playback_clock_snaps_on_large_audio_drift() {
        let t0 = Instant::now();
        let mut clock = PlaybackClock::running_at(Duration::from_secs(10), t0);

        let correction = clock.discipline_to(
            Duration::from_secs(11),
            t0,
            PlaybackClockSource::Audio,
            AudioSyncConfig::default(),
        );

        assert_eq!(correction.direction, ClockCorrectionDirection::Forward);
        assert_eq!(correction.drift, Duration::from_secs(1));
        assert!(correction.snapped);
        assert_eq!(clock.media_time_at(t0), Duration::from_secs(11));
    }

    #[test]
    fn http_playback_uses_deeper_audio_lead() {
        let request = MediaRequest::new("https://example.invalid/video.mkv");
        let timing = playback_timing_for_request(&request, PlaybackTimingConfig::default());

        assert_eq!(timing.audio_lead_time, STREAMING_AUDIO_LEAD_TIME);
    }

    #[test]
    fn custom_audio_lead_is_preserved_for_http_playback() {
        let request = MediaRequest::new("https://example.invalid/video.mkv");
        let custom = PlaybackTimingConfig {
            audio_lead_time: Duration::from_millis(750),
            ..PlaybackTimingConfig::default()
        };
        let timing = playback_timing_for_request(&request, custom);

        assert_eq!(timing.audio_lead_time, Duration::from_millis(750));
    }

    #[test]
    fn local_playback_keeps_default_audio_lead() {
        let request = MediaRequest::new("/tmp/video.mkv");
        let timing = playback_timing_for_request(&request, PlaybackTimingConfig::default());

        assert_eq!(timing.audio_lead_time, DEFAULT_AUDIO_LEAD_TIME);
    }

    #[test]
    fn http_requests_get_deeper_playback_queue_limits() {
        let request = MediaRequest::new("https://example.invalid/video.mkv");
        let limits = PlaybackQueueLimits::for_request(&request);

        assert_eq!(limits.video_frames, STREAMING_VIDEO_FRAME_QUEUE_LIMIT);
        assert_eq!(limits.audio_frames, STREAMING_AUDIO_FRAME_QUEUE_LIMIT);
        assert_eq!(limits.subtitle_frames, SUBTITLE_FRAME_QUEUE_LIMIT);
    }

    #[test]
    fn local_requests_keep_default_playback_queue_limits() {
        let request = MediaRequest::new("/tmp/video.mkv");
        let limits = PlaybackQueueLimits::for_request(&request);

        assert_eq!(limits, PlaybackQueueLimits::default());
    }

    #[test]
    fn video_frame_scheduler_waits_presents_and_drops() {
        let scheduler =
            VideoFrameScheduler::new(Duration::from_millis(4), Duration::from_millis(80));

        assert_eq!(
            scheduler.schedule(
                Some(Duration::from_millis(110)),
                Duration::from_millis(100),
                false
            ),
            VideoFrameDecision::Wait {
                early_by: Duration::from_millis(6)
            }
        );
        assert_eq!(
            scheduler.schedule(
                Some(Duration::from_millis(98)),
                Duration::from_millis(100),
                false
            ),
            VideoFrameDecision::Present {
                late_by: Some(Duration::from_millis(2))
            }
        );
        assert_eq!(
            scheduler.schedule(
                Some(Duration::from_millis(10)),
                Duration::from_millis(100),
                false
            ),
            VideoFrameDecision::Drop {
                late_by: Duration::from_millis(90)
            }
        );
    }

    #[test]
    fn video_frame_scheduler_always_presents_first_frame() {
        let scheduler = VideoFrameScheduler::new(Duration::ZERO, Duration::from_millis(1));

        assert_eq!(
            scheduler.schedule(
                Some(Duration::from_millis(10)),
                Duration::from_millis(100),
                true
            ),
            VideoFrameDecision::Present {
                late_by: Some(Duration::from_millis(90))
            }
        );
    }

    #[test]
    fn display_sync_quantizes_frames_to_vsyncs_and_carries_error() {
        let mut state = DisplaySyncState::default();
        let config = DisplaySyncConfig::for_refresh_rate_hz(60.0);
        let first = state.schedule_frame(Duration::from_secs_f64(1.0 / 24.0), config);
        let second = state.schedule_frame(Duration::from_secs_f64(1.0 / 24.0), config);

        assert_eq!(first.vsyncs + second.vsyncs, 5);
        assert_ne!(first.vsyncs, second.vsyncs);
        assert!(first.residual_error_nanos.signum() != second.residual_error_nanos.signum());
    }
}
