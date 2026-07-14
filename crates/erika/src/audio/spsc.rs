use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use super::{AudioPushResult, AudioRingBufferStats};

const EMPTY_POSITION: u64 = u64::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PcmSpscPushResult {
    pub start_position: u64,
    pub input_offset_frames: usize,
    pub accepted_frames: usize,
    pub dropped_frames: usize,
}

struct PcmSpscSlot {
    version: AtomicU64,
    position: AtomicU64,
}

impl PcmSpscSlot {
    fn empty() -> Self {
        Self {
            version: AtomicU64::new(0),
            position: AtomicU64::new(EMPTY_POSITION),
        }
    }
}

/// A bounded single-producer/single-consumer interleaved PCM ring.
///
/// Storage is allocated once. The consumer path performs only atomic loads,
/// sample copies, zero filling, and atomic counter updates. Slot
/// versions prevent a producer-side drop-oldest overwrite from exposing a torn
/// multi-channel frame to the realtime consumer.
pub(crate) struct PcmSpscRing {
    capacity_frames: usize,
    channels: usize,
    slots: Box<[PcmSpscSlot]>,
    source_samples: Box<[AtomicU32]>,
    output_samples: Box<[AtomicU32]>,
    read_position: AtomicU64,
    write_position: AtomicU64,
    written_frames: AtomicU64,
    read_frames: AtomicU64,
    dropped_frames: AtomicU64,
    underflow_frames: AtomicU64,
}

impl PcmSpscRing {
    pub(crate) fn new(capacity_frames: usize, channels: usize) -> Option<Self> {
        if capacity_frames == 0 || channels == 0 {
            return None;
        }
        let sample_capacity = capacity_frames.checked_mul(channels)?;
        let slots = (0..capacity_frames)
            .map(|_| PcmSpscSlot::empty())
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let source_samples = (0..sample_capacity)
            .map(|_| AtomicU32::new(0.0f32.to_bits()))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let output_samples = (0..sample_capacity)
            .map(|_| AtomicU32::new(0.0f32.to_bits()))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Some(Self {
            capacity_frames,
            channels,
            slots,
            source_samples,
            output_samples,
            read_position: AtomicU64::new(0),
            write_position: AtomicU64::new(0),
            written_frames: AtomicU64::new(0),
            read_frames: AtomicU64::new(0),
            dropped_frames: AtomicU64::new(0),
            underflow_frames: AtomicU64::new(0),
        })
    }

    pub(crate) fn channels(&self) -> usize {
        self.channels
    }

    pub(crate) fn read_position(&self) -> u64 {
        self.read_position.load(Ordering::Acquire)
    }

    pub(crate) fn push_interleaved(
        &self,
        input: &[f32],
        drop_oldest_on_overflow: bool,
        volume: f32,
    ) -> PcmSpscPushResult {
        let original_frames = input.len() / self.channels;
        let mut input_offset_frames = 0usize;
        let mut candidate_frames = original_frames;
        if candidate_frames > self.capacity_frames {
            if drop_oldest_on_overflow {
                input_offset_frames = candidate_frames - self.capacity_frames;
            }
            candidate_frames = self.capacity_frames;
        }

        let write = self.write_position.load(Ordering::Relaxed);
        let mut dropped_queued_frames = 0usize;
        let accepted_frames = if drop_oldest_on_overflow {
            let minimum_read = write
                .saturating_add(candidate_frames as u64)
                .saturating_sub(self.capacity_frames as u64);
            let previous_read = self.read_position.fetch_max(minimum_read, Ordering::AcqRel);
            dropped_queued_frames = minimum_read.saturating_sub(previous_read) as usize;
            candidate_frames
        } else {
            let read = self.read_position.load(Ordering::Acquire).min(write);
            let queued = write.saturating_sub(read).min(self.capacity_frames as u64) as usize;
            candidate_frames.min(self.capacity_frames.saturating_sub(queued))
        };

        for frame_index in 0..accepted_frames {
            let position = write.saturating_add(frame_index as u64);
            let slot_index = position as usize % self.capacity_frames;
            let slot = &self.slots[slot_index];
            slot.version.fetch_add(1, Ordering::AcqRel);
            let input_frame = input_offset_frames + frame_index;
            let input_base = input_frame * self.channels;
            let output_base = slot_index * self.channels;
            for channel in 0..self.channels {
                let sample = input[input_base + channel];
                self.source_samples[output_base + channel]
                    .store(sample.to_bits(), Ordering::Relaxed);
                self.output_samples[output_base + channel]
                    .store((sample * volume).to_bits(), Ordering::Relaxed);
            }
            slot.position.store(position, Ordering::Relaxed);
            slot.version.fetch_add(1, Ordering::Release);
        }
        self.write_position.store(
            write.saturating_add(accepted_frames as u64),
            Ordering::Release,
        );

        let dropped_frames = original_frames
            .saturating_sub(accepted_frames)
            .saturating_add(dropped_queued_frames);
        self.written_frames
            .fetch_add(accepted_frames as u64, Ordering::Relaxed);
        self.dropped_frames
            .fetch_add(dropped_frames as u64, Ordering::Relaxed);

        PcmSpscPushResult {
            start_position: write,
            input_offset_frames,
            accepted_frames,
            dropped_frames,
        }
    }

    pub(crate) fn read_interleaved(&self, output: &mut [f32]) -> usize {
        let requested_frames = output.len() / self.channels;
        let requested_samples = requested_frames * self.channels;
        output[requested_samples..].fill(0.0);

        let read = self.read_position.load(Ordering::Acquire);
        let write = self.write_position.load(Ordering::Acquire);
        let available_frames = write.saturating_sub(read).min(self.capacity_frames as u64) as usize;
        let candidate_frames = requested_frames.min(available_frames);
        let mut valid_frames = 0usize;

        for frame_index in 0..candidate_frames {
            let position = read.saturating_add(frame_index as u64);
            let slot_index = position as usize % self.capacity_frames;
            let slot = &self.slots[slot_index];
            let output_base = frame_index * self.channels;
            let version_before = slot.version.load(Ordering::Acquire);
            let position_before = slot.position.load(Ordering::Acquire);
            if version_before & 1 != 0 || position_before != position {
                output[output_base..output_base + self.channels].fill(0.0);
                continue;
            }
            let sample_base = slot_index * self.channels;
            for channel in 0..self.channels {
                output[output_base + channel] = f32::from_bits(
                    self.output_samples[sample_base + channel].load(Ordering::Relaxed),
                );
            }
            let version_after = slot.version.load(Ordering::Acquire);
            let read_floor = self.read_position.load(Ordering::Acquire);
            if version_before != version_after || version_after & 1 != 0 || read_floor > position {
                output[output_base..output_base + self.channels].fill(0.0);
            } else {
                valid_frames += 1;
            }
        }
        output[candidate_frames * self.channels..requested_samples].fill(0.0);
        self.read_position.fetch_max(
            read.saturating_add(candidate_frames as u64),
            Ordering::AcqRel,
        );

        let underflow_frames = requested_frames.saturating_sub(valid_frames);
        self.read_frames
            .fetch_add(valid_frames as u64, Ordering::Relaxed);
        self.underflow_frames
            .fetch_add(underflow_frames as u64, Ordering::Relaxed);
        valid_frames
    }

    pub(crate) fn clear(&self) {
        let write = self.write_position.load(Ordering::Acquire);
        self.read_position.store(write, Ordering::Release);
    }

    pub(crate) fn set_volume(&self, volume: f32) {
        let read = self.read_position.load(Ordering::Acquire);
        let write = self.write_position.load(Ordering::Acquire);
        let frames = write.saturating_sub(read).min(self.capacity_frames as u64) as usize;
        for frame_index in 0..frames {
            let position = read.saturating_add(frame_index as u64);
            let slot_index = position as usize % self.capacity_frames;
            let slot = &self.slots[slot_index];
            if slot.position.load(Ordering::Acquire) != position {
                continue;
            }
            slot.version.fetch_add(1, Ordering::AcqRel);
            let sample_base = slot_index * self.channels;
            for channel in 0..self.channels {
                let sample = f32::from_bits(
                    self.source_samples[sample_base + channel].load(Ordering::Relaxed),
                );
                self.output_samples[sample_base + channel]
                    .store((sample * volume).to_bits(), Ordering::Relaxed);
            }
            slot.version.fetch_add(1, Ordering::Release);
        }
    }

    pub(crate) fn stats(&self) -> AudioRingBufferStats {
        let write = self.write_position.load(Ordering::Acquire);
        let read = self.read_position.load(Ordering::Acquire).min(write);
        let queued_frames = write.saturating_sub(read).min(self.capacity_frames as u64) as usize;
        AudioRingBufferStats {
            queued_frames,
            queued_samples: queued_frames.saturating_mul(self.channels),
            written_frames: self.written_frames.load(Ordering::Relaxed),
            read_frames: self.read_frames.load(Ordering::Relaxed),
            dropped_frames: self.dropped_frames.load(Ordering::Relaxed),
            underflow_frames: self.underflow_frames.load(Ordering::Relaxed),
        }
    }
}

impl From<PcmSpscPushResult> for AudioPushResult {
    fn from(value: PcmSpscPushResult) -> Self {
        Self {
            accepted_frames: value.accepted_frames,
            dropped_frames: value.dropped_frames,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use super::*;

    #[test]
    fn ring_reads_interleaved_frames_and_zero_fills_underflow() {
        let ring = PcmSpscRing::new(4, 2).unwrap();
        let pushed = ring.push_interleaved(&[0.1, 0.2, 0.3, 0.4], true, 0.5);
        let mut output = [1.0; 6];

        let read = ring.read_interleaved(&mut output);

        assert_eq!(pushed.accepted_frames, 2);
        assert_eq!(read, 2);
        assert_eq!(output, [0.05, 0.1, 0.15, 0.2, 0.0, 0.0]);
        assert_eq!(ring.stats().underflow_frames, 1);
    }

    #[test]
    fn ring_drops_oldest_complete_frames_on_overflow() {
        let ring = PcmSpscRing::new(2, 2).unwrap();
        ring.push_interleaved(&[1.0, 10.0, 2.0, 20.0], true, 1.0);
        let pushed = ring.push_interleaved(&[3.0, 30.0], true, 1.0);
        let mut output = [0.0; 4];

        ring.read_interleaved(&mut output);

        assert_eq!(pushed.dropped_frames, 1);
        assert_eq!(output, [2.0, 20.0, 3.0, 30.0]);
        assert_eq!(ring.stats().dropped_frames, 1);
    }

    #[test]
    fn concurrent_frames_are_never_torn_between_channels() {
        const FRAMES: u32 = 20_000;
        let ring = Arc::new(PcmSpscRing::new(64, 2).unwrap());
        let producer_ring = Arc::clone(&ring);
        let producer = thread::spawn(move || {
            for value in 1..=FRAMES {
                let sample = value as f32;
                producer_ring.push_interleaved(&[sample, -sample], true, 1.0);
            }
        });

        let mut output = [0.0f32; 32];
        while !producer.is_finished() || ring.stats().queued_frames > 0 {
            ring.read_interleaved(&mut output);
            for pair in output.chunks_exact(2) {
                assert!(
                    pair == [0.0, 0.0] || pair[0] == -pair[1],
                    "torn stereo frame: {pair:?}"
                );
            }
            thread::yield_now();
        }
        producer.join().unwrap();
    }

    #[test]
    fn volume_changes_rescale_already_queued_frames_off_callback() {
        let ring = PcmSpscRing::new(2, 2).unwrap();
        ring.push_interleaved(&[1.0, -1.0, 0.5, -0.5], true, 1.0);

        ring.set_volume(0.25);
        let mut output = [0.0; 4];
        ring.read_interleaved(&mut output);

        assert_eq!(output, [0.25, -0.25, 0.125, -0.125]);
    }
}
