use std::collections::VecDeque;
use std::time::Duration;

use soundtouch::{Setting, SoundTouch};
use thiserror::Error;

use crate::ffmpeg::{PcmAudioFrame, PcmFormat};
use crate::trace;

const RATE_CHANGE_AUDIO_BRIDGE: Duration = Duration::from_millis(80);
const SOUNDTOUCH_SEQUENCE_MS: i32 = 25;
const SOUNDTOUCH_SEEK_WINDOW_MS: i32 = 12;
const SOUNDTOUCH_OVERLAP_MS: i32 = 6;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AudioError {
    #[error("audio format changed from {expected:?} to {actual:?}")]
    FormatChanged {
        expected: PcmFormat,
        actual: PcmFormat,
    },
    #[error("audio format is not configured")]
    FormatNotConfigured,
    #[error("audio channel count must be greater than zero")]
    InvalidChannelCount,
    #[error("audio backend error: {0}")]
    Backend(String),
}

pub type Result<T> = std::result::Result<T, AudioError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioOutputState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioRingBufferConfig {
    pub capacity_frames: usize,
    pub drop_oldest_on_overflow: bool,
}

impl Default for AudioRingBufferConfig {
    fn default() -> Self {
        Self {
            capacity_frames: 192_000,
            drop_oldest_on_overflow: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudioRingBufferStats {
    pub queued_frames: usize,
    pub queued_samples: usize,
    pub written_frames: u64,
    pub read_frames: u64,
    pub dropped_frames: u64,
    pub underflow_frames: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioPushResult {
    pub accepted_frames: usize,
    pub dropped_frames: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioReadResult {
    pub frames: usize,
    pub samples: usize,
    pub underflow_frames: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioClockSnapshot {
    pub media_time: Option<Duration>,
    pub queued_duration: Option<Duration>,
    pub queued_frames: usize,
    pub read_frames: u64,
    pub written_frames: u64,
    pub underflow_frames: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AudioTimelineSegment {
    start: Option<Duration>,
    frames: usize,
    media_frames_per_output_frame: f64,
}

impl AudioTimelineSegment {
    fn new(start: Option<Duration>, frames: usize, media_frames_per_output_frame: f64) -> Self {
        Self {
            start,
            frames,
            media_frames_per_output_frame,
        }
    }
}

#[derive(Debug)]
struct AudioTempoProcessor {
    format: PcmFormat,
    playback_rate: f64,
    processor: SoundTouch,
    pending_pts: Option<Duration>,
}

impl AudioTempoProcessor {
    fn new(format: PcmFormat, playback_rate: f64) -> Self {
        let playback_rate = normalize_playback_rate(playback_rate);
        let mut processor = SoundTouch::new();
        processor
            .set_sample_rate(format.sample_rate.max(8_000))
            .set_channels(format.channels.max(1) as u32)
            .set_tempo(playback_rate)
            .set_setting(Setting::UseQuickseek, 1)
            .set_setting(Setting::SequenceMs, SOUNDTOUCH_SEQUENCE_MS)
            .set_setting(Setting::SeekwindowMs, SOUNDTOUCH_SEEK_WINDOW_MS)
            .set_setting(Setting::OverlapMs, SOUNDTOUCH_OVERLAP_MS);
        Self {
            format,
            playback_rate,
            processor,
            pending_pts: None,
        }
    }

    fn matches(&self, format: PcmFormat, playback_rate: f64) -> bool {
        self.format == format
            && (self.playback_rate - normalize_playback_rate(playback_rate)).abs() < 0.001
    }

    fn process(&mut self, frame: PcmAudioFrame) -> (Vec<f32>, Option<Duration>, f64) {
        let channels = self.format.channels.max(1) as usize;
        let input_frames = frame.samples.len() / channels;
        if input_frames == 0 {
            return (Vec::new(), frame.pts, self.playback_rate);
        }
        if self.pending_pts.is_none() {
            self.pending_pts = frame.pts;
        }
        self.processor.put_samples(&frame.samples, input_frames);
        let output = self.receive_available();
        let start = self.pending_pts;
        let output_frames = output.len() / channels;
        if output_frames > 0 {
            if let Some(start) = self.pending_pts {
                self.pending_pts = offset_pts_scaled(
                    start,
                    output_frames,
                    self.format.sample_rate,
                    self.playback_rate,
                );
            }
        }
        (output, start, self.playback_rate)
    }

    fn receive_available(&mut self) -> Vec<f32> {
        const OUTPUT_FRAMES: usize = 4096;
        let channels = self.format.channels.max(1) as usize;
        let mut chunk = vec![0.0f32; OUTPUT_FRAMES * channels];
        let mut output = Vec::new();
        loop {
            let frames = self.processor.receive_samples(&mut chunk, OUTPUT_FRAMES);
            if frames == 0 {
                break;
            }
            output.extend_from_slice(&chunk[..frames * channels]);
            if frames < OUTPUT_FRAMES {
                break;
            }
        }
        output
    }
}

#[derive(Debug)]
pub struct AudioRingBuffer {
    config: AudioRingBufferConfig,
    format: Option<PcmFormat>,
    samples: VecDeque<f32>,
    timeline: VecDeque<AudioTimelineSegment>,
    last_media_time: Option<Duration>,
    playback_rate: f64,
    tempo_processor: Option<AudioTempoProcessor>,
    stats: AudioRingBufferStats,
}

impl AudioRingBuffer {
    pub fn new(config: AudioRingBufferConfig) -> Self {
        Self {
            config,
            format: None,
            samples: VecDeque::new(),
            timeline: VecDeque::new(),
            last_media_time: None,
            playback_rate: 1.0,
            tempo_processor: None,
            stats: AudioRingBufferStats::default(),
        }
    }

    pub fn with_format(config: AudioRingBufferConfig, format: PcmFormat) -> Result<Self> {
        let mut buffer = Self::new(config);
        buffer.configure(format)?;
        Ok(buffer)
    }

    pub fn configure(&mut self, format: PcmFormat) -> Result<()> {
        if format.channels == 0 {
            return Err(AudioError::InvalidChannelCount);
        }
        self.format = Some(format);
        self.clear();
        Ok(())
    }

    pub fn format(&self) -> Option<PcmFormat> {
        self.format
    }

    pub fn capacity_frames(&self) -> usize {
        self.config.capacity_frames
    }

    pub fn queued_frames(&self) -> usize {
        let Some(format) = self.format else {
            return 0;
        };
        self.samples.len() / format.channels as usize
    }

    pub fn queued_duration(&self) -> Option<Duration> {
        let format = self.format?;
        if format.sample_rate == 0 {
            return None;
        }
        Some(Duration::from_secs_f64(
            self.queued_frames() as f64 / format.sample_rate as f64,
        ))
    }

    pub fn stats(&self) -> AudioRingBufferStats {
        AudioRingBufferStats {
            queued_frames: self.queued_frames(),
            queued_samples: self.samples.len(),
            ..self.stats
        }
    }

    pub fn clock_snapshot(&self) -> AudioClockSnapshot {
        AudioClockSnapshot {
            media_time: self
                .timeline
                .front()
                .and_then(|segment| segment.start)
                .or(self.last_media_time),
            queued_duration: self.queued_duration(),
            queued_frames: self.queued_frames(),
            read_frames: self.stats.read_frames,
            written_frames: self.stats.written_frames,
            underflow_frames: self.stats.underflow_frames,
        }
    }

    pub fn clear(&mut self) {
        self.samples.clear();
        self.timeline.clear();
        self.last_media_time = None;
        self.tempo_processor = None;
        self.stats.queued_frames = 0;
        self.stats.queued_samples = 0;
    }

    pub fn set_playback_rate(&mut self, rate: f64) {
        let rate = normalize_playback_rate(rate);
        if (self.playback_rate - rate).abs() <= 0.001 {
            return;
        }
        self.playback_rate = rate;
        self.tempo_processor = None;
        self.trim_queued_to_front_frames(self.rate_change_bridge_frames());
    }

    pub fn push_frame(&mut self, frame: PcmAudioFrame) -> Result<AudioPushResult> {
        match self.format {
            Some(format) if format != frame.format => {
                return Err(AudioError::FormatChanged {
                    expected: format,
                    actual: frame.format,
                });
            }
            Some(_) => {}
            None => self.configure(frame.format)?,
        }

        let format = self.format.expect("audio format exists");
        let channels = format.channels as usize;
        let original_frames = frame.samples.len() / channels;
        let mut dropped_frames = 0usize;
        let frame_pts = frame.pts;
        let (frame_samples, segment_start, media_frames_per_output_frame) =
            self.prepare_frame_samples(frame, format);
        let incoming_frames = frame_samples.len() / channels;
        if incoming_frames == 0 {
            return Ok(AudioPushResult {
                accepted_frames: 0,
                dropped_frames: 0,
            });
        }
        let media_frames_per_output_frame = if media_frames_per_output_frame > 0.0 {
            media_frames_per_output_frame
        } else {
            original_frames as f64 / incoming_frames.max(1) as f64
        };

        if self.config.drop_oldest_on_overflow {
            while self.queued_frames() + incoming_frames > self.config.capacity_frames {
                if !self.drop_oldest_frame(channels) {
                    break;
                }
                dropped_frames += 1;
            }
        }

        let skip_incoming_frames = if self.config.drop_oldest_on_overflow {
            incoming_frames.saturating_sub(self.config.capacity_frames)
        } else {
            0
        };
        let skipped_incoming_samples = skip_incoming_frames * channels;
        let free_frames = self
            .config
            .capacity_frames
            .saturating_sub(self.queued_frames());
        let accepted_frames = incoming_frames
            .saturating_sub(skip_incoming_frames)
            .min(free_frames);
        let accepted_samples = accepted_frames * channels;
        self.samples.extend(
            frame_samples
                .into_iter()
                .skip(skipped_incoming_samples)
                .take(accepted_samples),
        );
        self.push_timeline_segment(
            segment_start.and_then(|pts| {
                offset_pts_scaled(
                    pts,
                    skip_incoming_frames,
                    format.sample_rate,
                    media_frames_per_output_frame,
                )
            }),
            accepted_frames,
            media_frames_per_output_frame,
        );
        self.stats.written_frames += accepted_frames as u64;
        self.stats.dropped_frames +=
            (dropped_frames + incoming_frames.saturating_sub(accepted_frames)) as u64;

        if trace::enabled()
            && (dropped_frames > 0
                || accepted_frames < incoming_frames
                || self.queued_frames() >= self.config.capacity_frames.saturating_sub(256))
        {
            trace::log(format!(
                "[erika-audio-trace] stage=push pts={} incoming_frames={} accepted_frames={} dropped_frames={} queued_frames={} queued_duration={} written_frames={} dropped_total={} sample_rate={} channels={} capacity_frames={} drop_oldest={}",
                frame_pts.map_or_else(
                    || "-".to_string(),
                    |pts| format!("{:.3}", pts.as_secs_f64())
                ),
                incoming_frames,
                accepted_frames,
                dropped_frames + incoming_frames.saturating_sub(accepted_frames),
                self.queued_frames(),
                self.queued_duration().map_or_else(
                    || "-".to_string(),
                    |duration| format!("{:.3}", duration.as_secs_f64())
                ),
                self.stats.written_frames,
                self.stats.dropped_frames,
                format!("{}", format.sample_rate),
                channels,
                self.config.capacity_frames,
                self.config.drop_oldest_on_overflow,
            ));
        }

        Ok(AudioPushResult {
            accepted_frames,
            dropped_frames: dropped_frames + incoming_frames.saturating_sub(accepted_frames),
        })
    }

    pub fn read_interleaved(&mut self, output: &mut [f32]) -> Result<AudioReadResult> {
        let format = self.format.ok_or(AudioError::FormatNotConfigured)?;
        let channels = format.channels as usize;
        if channels == 0 {
            return Err(AudioError::InvalidChannelCount);
        }

        let requested_frames = output.len() / channels;
        let requested_samples = requested_frames * channels;
        let mut read_samples = 0usize;
        for sample in output.iter_mut().take(requested_samples) {
            if let Some(value) = self.samples.pop_front() {
                *sample = value;
                read_samples += 1;
            } else {
                *sample = 0.0;
            }
        }
        for sample in output.iter_mut().skip(requested_samples) {
            *sample = 0.0;
        }

        let read_frames = read_samples / channels;
        let underflow_frames = requested_frames.saturating_sub(read_frames);
        self.advance_timeline(read_frames, format.sample_rate);
        self.stats.read_frames += read_frames as u64;
        self.stats.underflow_frames += underflow_frames as u64;

        if trace::enabled() && (underflow_frames > 0 || read_frames == 0 && requested_frames > 0) {
            trace::log(format!(
                "[erika-audio-trace] stage=read requested_frames={} read_frames={} underflow_frames={} queued_frames={} queued_duration={} read_total={} underflow_total={} sample_rate={} channels={} state=buffered",
                requested_frames,
                read_frames,
                underflow_frames,
                self.queued_frames(),
                self.queued_duration().map_or_else(
                    || "-".to_string(),
                    |duration| format!("{:.3}", duration.as_secs_f64())
                ),
                self.stats.read_frames,
                self.stats.underflow_frames,
                format.sample_rate,
                channels,
            ));
        }

        Ok(AudioReadResult {
            frames: read_frames,
            samples: read_samples,
            underflow_frames,
        })
    }

    fn prepare_frame_samples(
        &mut self,
        frame: PcmAudioFrame,
        format: PcmFormat,
    ) -> (Vec<f32>, Option<Duration>, f64) {
        if (self.playback_rate - 1.0).abs() <= 0.001 {
            self.tempo_processor = None;
            return (frame.samples, frame.pts, 1.0);
        }
        let processor = self
            .tempo_processor
            .get_or_insert_with(|| AudioTempoProcessor::new(format, self.playback_rate));
        if !processor.matches(format, self.playback_rate) {
            *processor = AudioTempoProcessor::new(format, self.playback_rate);
        }
        processor.process(frame)
    }

    fn drop_oldest_frame(&mut self, channels: usize) -> bool {
        if self.samples.len() < channels {
            self.samples.clear();
            return false;
        }
        for _ in 0..channels {
            let _ = self.samples.pop_front();
        }
        self.advance_timeline(1, self.format.map_or(0, |format| format.sample_rate));
        true
    }

    fn rate_change_bridge_frames(&self) -> usize {
        self.format.map_or(0, |format| {
            (format.sample_rate as f64 * RATE_CHANGE_AUDIO_BRIDGE.as_secs_f64()).round() as usize
        })
    }

    fn trim_queued_to_front_frames(&mut self, frames: usize) {
        let Some(format) = self.format else {
            self.samples.clear();
            self.timeline.clear();
            return;
        };
        let channels = format.channels as usize;
        let target_samples = frames.saturating_mul(channels);
        if self.samples.len() > target_samples {
            self.samples.truncate(target_samples);
        }
        self.trim_timeline_to_front_frames(self.samples.len() / channels.max(1));
    }

    fn trim_timeline_to_front_frames(&mut self, mut frames: usize) {
        let mut trimmed = VecDeque::new();
        while frames > 0 {
            let Some(mut segment) = self.timeline.pop_front() else {
                break;
            };
            if segment.frames <= frames {
                frames -= segment.frames;
                trimmed.push_back(segment);
            } else {
                segment.frames = frames;
                trimmed.push_back(segment);
                break;
            }
        }
        self.timeline = trimmed;
    }

    fn push_timeline_segment(
        &mut self,
        start: Option<Duration>,
        frames: usize,
        media_frames_per_output_frame: f64,
    ) {
        if frames == 0 {
            return;
        }
        self.timeline.push_back(AudioTimelineSegment::new(
            start,
            frames,
            media_frames_per_output_frame,
        ));
    }

    fn advance_timeline(&mut self, mut frames: usize, sample_rate: u32) {
        while frames > 0 {
            let Some(front) = self.timeline.front_mut() else {
                break;
            };
            let consumed = frames.min(front.frames);
            if let Some(start) = front.start {
                self.last_media_time = offset_pts_scaled(
                    start,
                    consumed,
                    sample_rate,
                    front.media_frames_per_output_frame,
                );
                front.start = self.last_media_time;
            }
            front.frames -= consumed;
            frames -= consumed;
            if front.frames == 0 {
                let _ = self.timeline.pop_front();
            }
        }
    }
}

pub trait AudioOutputBackend {
    fn configure(&mut self, format: PcmFormat) -> Result<()>;
    fn start(&mut self) -> Result<()>;
    fn pause(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn set_volume(&mut self, volume: f32);
    fn volume(&self) -> f32;
    fn set_playback_rate(&mut self, _rate: f64) {}
    fn push(&mut self, frame: PcmAudioFrame) -> Result<AudioPushResult>;
    fn state(&self) -> AudioOutputState;
    fn stats(&self) -> AudioRingBufferStats;
    fn clock_snapshot(&self) -> Option<AudioClockSnapshot> {
        None
    }
}

#[derive(Debug)]
pub struct BufferedAudioOutput {
    state: AudioOutputState,
    buffer: AudioRingBuffer,
    volume: f32,
}

impl BufferedAudioOutput {
    pub fn new(config: AudioRingBufferConfig) -> Self {
        Self {
            state: AudioOutputState::Stopped,
            buffer: AudioRingBuffer::new(config),
            volume: 1.0,
        }
    }

    pub fn buffer(&self) -> &AudioRingBuffer {
        &self.buffer
    }

    pub fn buffer_mut(&mut self) -> &mut AudioRingBuffer {
        &mut self.buffer
    }

    pub fn read_interleaved(&mut self, output: &mut [f32]) -> Result<AudioReadResult> {
        let result = self.buffer.read_interleaved(output)?;
        apply_volume(output, self.volume);
        Ok(result)
    }

    pub fn clock_snapshot(&self) -> AudioClockSnapshot {
        self.buffer.clock_snapshot()
    }
}

impl AudioOutputBackend for BufferedAudioOutput {
    fn configure(&mut self, format: PcmFormat) -> Result<()> {
        self.buffer.configure(format)
    }

    fn start(&mut self) -> Result<()> {
        self.state = AudioOutputState::Playing;
        Ok(())
    }

    fn pause(&mut self) -> Result<()> {
        self.state = AudioOutputState::Paused;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.state = AudioOutputState::Stopped;
        self.buffer.clear();
        Ok(())
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = normalize_volume(volume);
    }

    fn volume(&self) -> f32 {
        self.volume
    }

    fn set_playback_rate(&mut self, rate: f64) {
        self.buffer.set_playback_rate(rate);
    }

    fn push(&mut self, frame: PcmAudioFrame) -> Result<AudioPushResult> {
        self.buffer.push_frame(frame)
    }

    fn state(&self) -> AudioOutputState {
        self.state
    }

    fn stats(&self) -> AudioRingBufferStats {
        self.buffer.stats()
    }

    fn clock_snapshot(&self) -> Option<AudioClockSnapshot> {
        Some(self.buffer.clock_snapshot())
    }
}

pub fn normalize_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

pub fn apply_volume(samples: &mut [f32], volume: f32) {
    let volume = normalize_volume(volume);
    if (volume - 1.0).abs() <= f32::EPSILON {
        return;
    }
    for sample in samples {
        *sample *= volume;
    }
}

fn offset_pts_scaled(
    pts: Duration,
    frames: usize,
    sample_rate: u32,
    media_frames_per_output_frame: f64,
) -> Option<Duration> {
    if sample_rate == 0 {
        return Some(pts);
    }
    let media_frames = frames as f64 * media_frames_per_output_frame.max(0.0);
    Some(pts + Duration::from_secs_f64(media_frames / sample_rate as f64))
}

fn normalize_playback_rate(rate: f64) -> f64 {
    if rate.is_finite() && rate > 0.0 {
        rate.clamp(0.25, 4.0)
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffmpeg::PcmSampleFormat;

    fn stereo_format() -> PcmFormat {
        PcmFormat {
            sample_rate: 48_000,
            channels: 2,
            sample_format: PcmSampleFormat::F32Interleaved,
        }
    }

    fn frame(samples: Vec<f32>) -> PcmAudioFrame {
        PcmAudioFrame {
            format: stereo_format(),
            pts: None,
            frames: samples.len() / 2,
            samples,
        }
    }

    fn timed_frame(pts: Duration, frames: usize) -> PcmAudioFrame {
        PcmAudioFrame {
            format: stereo_format(),
            pts: Some(pts),
            frames,
            samples: vec![0.0; frames * 2],
        }
    }

    #[test]
    fn ring_buffer_reads_interleaved_samples_and_zero_fills_underflow() {
        let mut buffer = AudioRingBuffer::with_format(
            AudioRingBufferConfig {
                capacity_frames: 8,
                drop_oldest_on_overflow: true,
            },
            stereo_format(),
        )
        .unwrap();
        buffer.push_frame(frame(vec![0.1, 0.2, 0.3, 0.4])).unwrap();

        let mut output = [1.0; 6];
        let result = buffer.read_interleaved(&mut output).unwrap();

        assert_eq!(result.frames, 2);
        assert_eq!(result.underflow_frames, 1);
        assert_eq!(output, [0.1, 0.2, 0.3, 0.4, 0.0, 0.0]);
    }

    #[test]
    fn ring_buffer_drops_oldest_frames_on_overflow() {
        let mut buffer = AudioRingBuffer::with_format(
            AudioRingBufferConfig {
                capacity_frames: 2,
                drop_oldest_on_overflow: true,
            },
            stereo_format(),
        )
        .unwrap();

        let result = buffer
            .push_frame(frame(vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6]))
            .unwrap();
        let mut output = [0.0; 4];
        buffer.read_interleaved(&mut output).unwrap();

        assert_eq!(result.accepted_frames, 2);
        assert_eq!(result.dropped_frames, 1);
        assert_eq!(output, [0.3, 0.4, 0.5, 0.6]);
    }

    #[test]
    fn buffered_audio_output_tracks_state() {
        let mut output = BufferedAudioOutput::new(AudioRingBufferConfig::default());

        output.configure(stereo_format()).unwrap();
        output.start().unwrap();
        assert_eq!(output.state(), AudioOutputState::Playing);
        output.pause().unwrap();
        assert_eq!(output.state(), AudioOutputState::Paused);
        output.stop().unwrap();
        assert_eq!(output.state(), AudioOutputState::Stopped);
    }

    #[test]
    fn volume_helpers_clamp_and_apply_gain() {
        assert_eq!(normalize_volume(1.0), 1.0);
        assert_eq!(normalize_volume(-1.0), 0.0);
        assert_eq!(normalize_volume(2.0), 1.0);
        assert_eq!(normalize_volume(f32::NAN), 1.0);

        let mut samples = [1.0, -0.5, 0.25, 0.0];
        apply_volume(&mut samples, 0.5);
        assert_eq!(samples, [0.5, -0.25, 0.125, 0.0]);
    }

    #[test]
    fn buffered_audio_output_volume_is_clamped() {
        let mut output = BufferedAudioOutput::new(AudioRingBufferConfig::default());

        assert_eq!(output.volume(), 1.0);
        output.set_volume(0.25);
        assert_eq!(output.volume(), 0.25);
        output.set_volume(-1.0);
        assert_eq!(output.volume(), 0.0);
        output.set_volume(f32::NAN);
        assert_eq!(output.volume(), 1.0);
    }

    #[test]
    fn ring_buffer_clock_snapshot_tracks_front_audio_pts() {
        let mut buffer = AudioRingBuffer::with_format(
            AudioRingBufferConfig {
                capacity_frames: 48_000,
                drop_oldest_on_overflow: true,
            },
            stereo_format(),
        )
        .unwrap();
        buffer
            .push_frame(timed_frame(Duration::from_secs(10), 480))
            .unwrap();

        assert_eq!(
            buffer.clock_snapshot().media_time,
            Some(Duration::from_secs(10))
        );

        let mut output = vec![0.0; 240 * 2];
        buffer.read_interleaved(&mut output).unwrap();

        assert_eq!(
            buffer.clock_snapshot().media_time,
            Some(Duration::from_millis(10_005))
        );
        assert_eq!(buffer.clock_snapshot().queued_frames, 240);
    }

    #[test]
    fn ring_buffer_fast_rate_preserves_pitch_and_media_timeline() {
        let mut buffer = AudioRingBuffer::with_format(
            AudioRingBufferConfig {
                capacity_frames: 48_000,
                drop_oldest_on_overflow: true,
            },
            stereo_format(),
        )
        .unwrap();
        buffer.set_playback_rate(2.0);
        for index in 0..8 {
            buffer
                .push_frame(timed_frame(
                    Duration::from_secs(10)
                        + Duration::from_secs_f64(index as f64 * 2048.0 / 48_000.0),
                    2048,
                ))
                .unwrap();
        }

        let queued_frames = buffer.clock_snapshot().queued_frames;
        assert!(queued_frames > 0);
        assert!(queued_frames < 16_384);

        let mut output = vec![0.0; queued_frames * 2];
        let result = buffer.read_interleaved(&mut output).unwrap();

        assert_eq!(result.frames, queued_frames);
        let media_time = buffer.clock_snapshot().media_time.unwrap();
        let expected = Duration::from_secs(10)
            + Duration::from_secs_f64(queued_frames as f64 * 2.0 / 48_000.0);
        assert!(duration_abs_diff(media_time, expected) < Duration::from_millis(2));
    }

    #[test]
    fn ring_buffer_rate_change_keeps_short_audio_bridge() {
        let mut buffer = AudioRingBuffer::with_format(
            AudioRingBufferConfig {
                capacity_frames: 48_000,
                drop_oldest_on_overflow: true,
            },
            stereo_format(),
        )
        .unwrap();
        buffer
            .push_frame(timed_frame(Duration::from_secs(4), 4_800))
            .unwrap();

        buffer.set_playback_rate(2.0);

        assert_eq!(buffer.clock_snapshot().queued_frames, 3_840);
        assert_eq!(
            buffer.clock_snapshot().media_time,
            Some(Duration::from_secs(4))
        );
    }

    #[test]
    fn ring_buffer_clock_survives_frame_drop() {
        let mut buffer = AudioRingBuffer::with_format(
            AudioRingBufferConfig {
                capacity_frames: 2,
                drop_oldest_on_overflow: true,
            },
            stereo_format(),
        )
        .unwrap();
        buffer
            .push_frame(timed_frame(Duration::from_secs(1), 4))
            .unwrap();

        assert_eq!(
            buffer.clock_snapshot().media_time,
            Some(Duration::from_nanos(1_000_041_667))
        );
    }

    fn duration_abs_diff(a: Duration, b: Duration) -> Duration {
        a.checked_sub(b)
            .or_else(|| b.checked_sub(a))
            .unwrap_or(Duration::ZERO)
    }
}
