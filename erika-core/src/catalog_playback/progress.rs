//! Observe the kernel, never the optimistic UI clock or simulated media.
use super::*;
use erika_catalog::playback::Intervals;
use web_time::Instant;

#[derive(Default)]
pub(super) struct Progress {
    last: Option<(Instant, f64, bool, f64)>,
    pending: Intervals,
    ended: bool,
}
impl Progress {
    pub fn interrupt(&mut self) {
        self.last = None;
    }
    fn observe(
        &mut self,
        p: &crate::kernel::Playback,
        rate: f64,
        seeking: bool,
        at: Instant,
    ) -> erika_catalog::Result<()> {
        let position = p.position;
        if seeking
            || !p.loaded
            || p.error.is_some()
            || !position.is_finite()
            || position < 0.
            || !rate.is_finite()
            || rate <= 0.
        {
            self.interrupt();
            return Ok(());
        }
        let previous = self.last.replace((at, position, p.playing, rate));
        let Some((time, start, playing, previous_rate)) = previous else {
            return Ok(());
        };
        let delta = position - start;
        // Half a second permits a coarse kernel clock, not a multi-second seek.
        // Clock discontinuities establish a new baseline and add no coverage.
        let budget = at.saturating_duration_since(time).as_secs_f64() * previous_rate + 0.5;
        if delta >= 0. && delta <= budget {
            if playing && delta > 0. {
                self.pending.add(start, position)?;
            }
            // EOF is a kernel event, not necessarily a final clock increment.
            // A decoder may stop on the last frame just short of duration and
            // publish EOF in the following sample with the same position.
            self.ended |= p.ended && p.duration.is_some_and(|d| d.is_finite() && d > 0.);
        }
        Ok(())
    }
}
impl ErikaEngine {
    pub(crate) fn interrupt_played(&mut self) {
        self.catalog_playback.progress.interrupt();
    }
    pub(crate) fn observe_played(
        &mut self,
        playback: &crate::kernel::Playback,
        seeking: bool,
        at: Instant,
    ) {
        if !self.state.video || self.catalog_playback.binding.is_none() {
            return;
        }
        let rate = self.rate();
        if let Err(error) = self
            .catalog_playback
            .progress
            .observe(playback, rate, seeking, at)
        {
            self.notify(Notice::OperationFailed {
                reason: error.message,
            });
        }
    }
    pub(crate) fn save_played(&mut self) {
        let Some((file, revision, _)) = self.catalog_playback.clock else {
            return;
        };
        if self.file != Some(file)
            || (self.catalog_playback.progress.pending.is_empty()
                && !self.catalog_playback.progress.ended)
        {
            return;
        }
        let progress = &self.catalog_playback.progress;
        let result = self.db.catalog_record_playback(
            file,
            revision,
            &progress.pending,
            self.state.duration,
            progress.ended,
        );
        match result {
            Ok(changed) => {
                self.catalog_playback.progress.pending = Default::default();
                self.catalog_playback.progress.ended = false;
                if !changed.is_empty() {
                    #[cfg(feature = "bern-module-anime")]
                    erika_anime_protocol::anime::calls::history_changed();
                    #[cfg(feature = "bern-module-library")]
                    erika_library_protocol::library::calls::history_changed();
                }
            }
            Err(error) if error.code == "conflict" => {
                self.catalog_playback.progress = Default::default()
            }
            Err(error) => self.notify(Notice::OperationFailed {
                reason: error.message,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::Playback;
    use std::time::Duration;
    #[test]
    fn seeks_pauses_invalid_clocks_and_old_eof_do_not_create_coverage() {
        let mut p = Progress::default();
        let at = Instant::now();
        let mut frame = Playback {
            loaded: true,
            playing: true,
            position: 0.,
            duration: Some(100.),
            ..Default::default()
        };
        p.observe(&frame, 1., false, at).unwrap();
        frame.position = 10.;
        p.observe(&frame, 1., false, at + Duration::from_secs(10))
            .unwrap();
        frame.position = 95.;
        p.observe(&frame, 1., true, at + Duration::from_secs(11))
            .unwrap();
        frame.position = 96.;
        p.observe(&frame, 1., false, at + Duration::from_secs(12))
            .unwrap();
        frame.position = 100.;
        frame.ended = true;
        frame.playing = false;
        p.observe(&frame, 1., false, at + Duration::from_secs(16))
            .unwrap();
        assert!(!p.pending.complete(0., 100.));
        assert!(p.pending.complete(0., 10.));
        assert!(p.ended);
        let before = p.pending.clone();
        p.observe(&frame, 1., false, at + Duration::from_secs(17))
            .unwrap();
        frame.position = f64::NAN;
        p.observe(&frame, 1., false, at + Duration::from_secs(18))
            .unwrap();
        assert_eq!(before, p.pending);
    }
    #[test]
    fn eof_without_a_final_clock_increment_survives_a_coverage_flush() {
        let mut p = Progress::default();
        let at = Instant::now();
        let mut frame = Playback {
            loaded: true,
            playing: true,
            duration: Some(100.),
            ..Default::default()
        };
        p.observe(&frame, 1., false, at).unwrap();
        frame.position = 99.96;
        p.observe(&frame, 1., false, at + Duration::from_secs(100))
            .unwrap();
        assert!(p.pending.complete(0., 100.));
        p.pending = Default::default();
        frame.playing = false;
        frame.ended = true;
        p.observe(&frame, 1., false, at + Duration::from_millis(100250))
            .unwrap();
        assert!(p.ended);
        assert!(p.pending.is_empty());
        p.ended = false;
        p.interrupt();
        p.observe(&frame, 1., true, at + Duration::from_secs(101))
            .unwrap();
        assert!(!p.ended);
    }

    #[test]
    fn speed_and_real_elapsed_time_bound_unannounced_position_jumps() {
        let mut p = Progress::default();
        let at = Instant::now();
        let mut frame = Playback {
            loaded: true,
            playing: true,
            ..Default::default()
        };
        p.observe(&frame, 2., false, at).unwrap();
        frame.position = 20.;
        p.observe(&frame, 2., false, at + Duration::from_secs(10))
            .unwrap();
        assert!(p.pending.complete(0., 20.));
        frame.position = 90.;
        p.observe(&frame, 2., false, at + Duration::from_secs(11))
            .unwrap();
        assert!(!p.pending.complete(0., 100.));
        frame.playing = false;
        p.observe(&frame, 2., false, at + Duration::from_secs(12))
            .unwrap();
        frame.position = 100.;
        p.observe(&frame, 2., false, at + Duration::from_secs(50))
            .unwrap();
        assert!(!p.pending.complete(90., 100.));
    }
}
