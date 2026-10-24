//! Playback state. liberika plays the files and links the user opens (see
//! `kernel`), with their subtitles and danmaku; the library's sample
//! episodes, and any media where no kernel runs, play on a simulated clock
//! over their stills, without either.
use super::*;
use crate::kernel::{Playback, Track};

/// The audio tracks simulated media offers.
fn sample_audio() -> Vec<MediaTrack> {
    vec![
        MediaTrack {
            id: 1,
            name: "日本語 · FLAC 2.0".into(),
        },
        MediaTrack {
            id: 2,
            name: "中文配音 · AAC 2.0".into(),
        },
    ]
}

pub fn initial() -> PlayerState {
    let mut player = PlayerState {
        danmaku_text: String::new(),
        sending_danmaku: false,
        danmaku_sent_revision: 0,
        queue_index: 1,
        episode_list: Vec::new(),
        has_previous: false,
        has_next: false,
        panel: false,
        tab: PlayerTab::Danmaku,
        episode: 1,
        episodes: false,
        subtitle_tracks: Vec::new(),
        subtitle_track: -1,
        audio_tracks: Vec::new(),
        audio_track: -1,
        subtitle_name: String::new(),
        audio_name: String::new(),
        video_info: String::new(),
        enhancement: Enhancement::Off,
        volume: 0.7,
        volume_percent: 70,
        subtitle_size: 0.5,
        subtitle_scale_percent: 100,
        subtitle_supersampling: true,
        skip_intro: true,
        chapters: Vec::new(),
        has_chapters: false,
        current_chapter: -1,
        skip_kind: SkipKind::None,
        feedback: false,
        subtitle_file: String::new(),
        danmaku_source: String::new(),
        danmaku_count: 0,
    };
    simulate_tracks(&mut player);
    player
}

/// The sample audio tracks, the first selected; no subtitles.
fn simulate_tracks(player: &mut PlayerState) {
    player.subtitle_tracks.clear();
    player.audio_tracks = sample_audio();
    player.subtitle_track = -1;
    player.audio_track = 1;
    player.video_info = "HEVC · 1080p".into();
    name_tracks(player);
}

/// The selected tracks' names.
fn name_tracks(player: &mut PlayerState) {
    let name = |tracks: &[MediaTrack], id: i64| {
        tracks
            .iter()
            .find(|track| track.id == id)
            .map(|track| track.name.clone())
            .unwrap_or_default()
    };
    player.subtitle_name = name(&player.subtitle_tracks, player.subtitle_track);
    player.audio_name = name(&player.audio_tracks, player.audio_track);
}

fn media_tracks(tracks: &[Track]) -> Vec<MediaTrack> {
    tracks
        .iter()
        .map(|track| MediaTrack {
            id: track.id,
            name: track.name.clone(),
        })
        .collect()
}

/// Media the kernel reads: local files and web addresses.
fn playable(url: &str, browser: bool) -> bool {
    url.starts_with("file:")
        || url.starts_with("http://")
        || url.starts_with("https://")
        || (browser && url.starts_with("blob:"))
}

/// The kernel's subtitle scale for the size slider: 1 at its middle.
fn subtitle_scale(size: f64) -> f64 {
    0.5 + size.clamp(0.0, 1.0)
}

/// The rates of the speed preference.
const PRESET_RATES: [(PlaybackSpeed, f64); 6] = [
    (PlaybackSpeed::Half, 0.5),
    (PlaybackSpeed::Slow, 0.75),
    (PlaybackSpeed::Normal, 1.0),
    (PlaybackSpeed::Fast, 1.25),
    (PlaybackSpeed::Faster, 1.5),
    (PlaybackSpeed::Double, 2.0),
];

/// `0.25×`, `1.0×`, `1.75×`: a rate as the speed menu writes it.
pub(crate) fn speed_label(rate: f64) -> String {
    if rate.fract() == 0.0 {
        format!("{rate:.1}×")
    } else {
        let text = format!("{rate:.2}");
        format!("{}×", text.trim_end_matches('0'))
    }
}

impl ErikaEngine {
    pub(super) fn source_name(&self, source: &MediaSource) -> String {
        self.kernel
            .as_ref()
            .and_then(|k| k.file_name(&source.url))
            .or_else(|| source.file_name())
            .unwrap_or_else(|| source.url.clone())
    }
    /// Start the player afresh on `episode` of a series, or on a file.
    pub(super) fn reset_player(&mut self, episode: Option<i64>) {
        self.state.player.episode = episode.unwrap_or(1);
        self.state.player.episodes = episode.is_some();
        self.state.player.panel = false;
        self.state.player.feedback = false;
        self.state.player.subtitle_file.clear();
        self.state.player.danmaku_source.clear();
        self.state.player.danmaku_count = 0;
        self.feedback_remaining = 0.0;
    }

    /// The kernel, while the frontend hosts it.
    pub(super) fn kernel(&self) -> Option<Arc<dyn Kernel>> {
        self.kernel.clone().filter(|kernel| kernel.ready())
    }

    /// Play `source` with the kernel from `start` seconds when the kernel
    /// runs and reads it; otherwise the player simulates over the still.
    pub(super) fn load(&mut self, source: Option<MediaSource>, start: f64) {
        self.clear_chapters();
        self.pause_wanted = None;
        self.seek_wanted = None;
        self.still_wanted = None;
        let kernel = self.kernel();
        match (kernel, source) {
            (Some(kernel), Some(source)) if playable(&source.url, kernel.is_browser()) => {
                if let Err(reason) = kernel.prepare_source(&source.url) {
                    self.unload();
                    self.state.paused = true;
                    self.notify(Notice::OperationFailed { reason });
                    return;
                }
                let start = if start.is_finite() {
                    start.max(0.0)
                } else {
                    0.0
                };
                self.first_still_at = start + 3.0;
                self.state.video = true;
                self.state.progress = 0.0;
                self.state.elapsed = start;
                self.state.duration = 0.0;
                let player = &mut self.state.player;
                player.subtitle_tracks.clear();
                player.audio_tracks.clear();
                player.subtitle_track = -1;
                player.audio_track = -1;
                player.video_info.clear();
                name_tracks(player);
                kernel.set_hardware_decode(self.state.preferences.hardware_decode);
                kernel.set_rate(self.rate());
                kernel.set_subtitle_scale(subtitle_scale(self.state.player.subtitle_size));
                kernel.set_subtitle_supersampling(self.state.player.subtitle_supersampling);
                kernel.set_enhancement(self.enhancement());
                kernel.set_danmaku(&self.danmaku_config());
                kernel.open(&source.url, start);
                // The hub sets the volume of the player that has the media.
                kernel.set_volume(self.state.player.volume);
                if self.seek_wanted.is_none() && start > 0.0 {
                    self.seek_wanted = Some((start, 0));
                }
                self.source = Some(source.clone());
                self.plugin_event("videoLoaded", self.plugin_media());
                self.mount_danmaku_beside(&source);
            }
            _ => {
                self.unload();
                simulate_tracks(&mut self.state.player);
            }
        }
    }

    /// Ask the kernel for the picture on screen, which the mini player,
    /// the frame's transitions and the recent videos then show.
    pub(super) fn capture_still(&mut self) {
        if !self.state.video {
            return;
        }
        let Some(kernel) = self.kernel() else {
            return;
        };
        let dir = if kernel.is_browser() {
            std::path::PathBuf::new()
        } else {
            history::stills_dir()
        };
        if self.stills == 0 && !kernel.is_browser() {
            history::forget_stills(&self.db, &self.run);
        }
        self.stills += 1;
        let path = dir.join(format!("{}{}.png", self.run, self.stills));
        kernel.capture(&path, Some(480));
        self.still_wanted = Some(path);
    }

    /// Save what is on screen in Pictures › Erika, named after the title
    /// and the time in it.
    pub(super) fn save_screenshot(&mut self) -> Result<(), CommandError> {
        let kernel = self.kernel().filter(|_| self.state.video);
        if let Some(kernel) = &kernel
            && kernel.is_browser()
        {
            kernel.capture(std::path::Path::new("Erika.png"), None);
            return Ok(());
        }
        let (Some(kernel), Some(home)) = (kernel, std::env::var_os("HOME")) else {
            self.notify(Notice::ScreenshotPending);
            return Ok(());
        };
        let seconds = self.state.elapsed.max(0.0) as u64;
        let title: String = self
            .state
            .title
            .chars()
            .map(|c| if c == '/' || c == ':' { '_' } else { c })
            .collect();
        let file = format!(
            "{title} {:02}-{:02}-{:02}.png",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        );
        kernel.capture(
            &std::path::Path::new(&home)
                .join("Pictures/Erika")
                .join(&file),
            None,
        );
        Ok(())
    }

    /// The kernel lets go of what it played.
    pub(super) fn unload(&mut self) {
        self.clear_chapters();
        if self.state.video
            && let Some(kernel) = &self.kernel
        {
            kernel.close();
        }
        self.state.video = false;
        self.source = None;
    }

    /// The playback rate: a plugin's, or the speed preference's.
    pub(crate) fn rate(&self) -> f64 {
        if let Some(rate) = self.rate_override {
            return rate;
        }
        PRESET_RATES
            .iter()
            .find(|(speed, _)| *speed == self.state.preferences.speed)
            .map_or(1.0, |(_, rate)| *rate)
    }

    /// The upscaler in use: the chosen one while the enhance module is on.
    fn enhancement(&self) -> Enhancement {
        if self.state.features.enhance {
            self.state.player.enhancement.clone()
        } else {
            Enhancement::Off
        }
    }

    /// The rates the speed menu offers: the preference's six and those the
    /// running plugins add, the one playing selected.
    pub(crate) fn show_speeds(&mut self) {
        let current = self.rate();
        let mut rates: Vec<f64> = PRESET_RATES.iter().map(|(_, rate)| *rate).collect();
        rates.extend(self.plugins.extensions().speeds);
        rates.sort_by(f64::total_cmp);
        rates.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        self.state.speeds = rates
            .into_iter()
            .map(|rate| SpeedOption {
                id: format!("{rate}"),
                rate,
                label: speed_label(rate),
                selected: (rate - current).abs() < 1e-6,
            })
            .collect();
    }

    /// Play at `rate`: a preset is the speed preference, kept; another rate
    /// a plugin offers plays until the preference changes.
    pub(crate) fn play_at(&mut self, rate: f64) -> Result<(), CommandError> {
        if !self.state.speeds.iter().any(|option| (option.rate - rate).abs() < 1e-6) {
            return Err(CommandError::new("invalid_rate", format!("{rate}× is not offered")));
        }
        match PRESET_RATES.iter().find(|(_, preset)| (preset - rate).abs() < 1e-6) {
            Some((speed, _)) => erika::Commands::set_speed(self, speed.clone()),
            None => {
                self.set_rate_override(Some(rate));
                Ok(())
            }
        }
    }

    /// Play at `rate` in place of the speed preference, or at its own
    /// again.
    pub(crate) fn set_rate_override(&mut self, rate: Option<f64>) {
        self.rate_override = rate.map(|rate| rate.clamp(0.0625, 16.0));
        self.apply_speed();
        self.show_speeds();
    }

    pub(super) fn apply_speed(&mut self) {
        if self.state.video
            && let Some(kernel) = self.kernel()
        {
            kernel.set_rate(self.rate());
        }
    }

    pub(super) fn apply_enhancement(&mut self) {
        if self.state.video
            && let Some(kernel) = self.kernel()
        {
            kernel.set_enhancement(self.enhancement());
        }
    }

    pub(super) fn update_position(&mut self, seconds: f64) {
        if self.state.video {
            let duration = self.state.duration;
            let seconds = if duration > 0.0 {
                seconds.clamp(0.0, duration)
            } else {
                seconds.max(0.0)
            };
            if let Some(kernel) = self.kernel() {
                kernel.seek(seconds);
            }
            self.seek_wanted = Some((seconds, 0));
            self.state.elapsed = seconds;
            self.plugin_event("seek", serde_json::json!({ "time": seconds, "duration": duration }));
            self.state.progress = if duration > 0.0 {
                seconds / duration
            } else {
                0.0
            };
            self.update_chapter_position();
            return;
        }
        self.state.progress = seconds.clamp(0.0, LENGTH) / LENGTH;
        self.state.elapsed = self.state.progress * LENGTH;
    }

    /// Pause, or play on: from the start once it ended.
    pub(super) fn toggle(&mut self) {
        if self.state.progress >= 1.0 {
            self.update_position(0.0);
        }
        self.state.paused = !self.state.paused;
        self.plugin_event(if self.state.paused { "pause" } else { "play" }, self.plugin_media());
        if self.state.video
            && let Some(kernel) = self.kernel()
        {
            if self.state.paused {
                kernel.pause()
            } else {
                kernel.play()
            }
            self.pause_wanted = Some((self.state.paused, 0));
            if self.state.paused {
                self.capture_still();
            }
        }
    }

    pub(super) fn take_kernel_results(&mut self) {
        let results = self
            .kernel
            .as_ref()
            .map(|k| k.take_results())
            .unwrap_or_default();
        for result in results {
            if matches!(result.operation, kernel::Operation::Subtitle(_))
                && result.source.is_some()
                && self.source.as_ref().map(|s| &s.url) != result.source.as_ref()
            {
                continue;
            }
            match result.result {
                Err(reason) => self.notify(Notice::OperationFailed { reason }),
                Ok(()) => match result.operation {
                    kernel::Operation::Screenshot(path) => {
                        let file = path.to_string_lossy().into_owned();
                        if self.kernel.as_ref().is_some_and(|k| k.is_browser()) {
                            self.notify(Notice::ScreenshotDownload { file });
                        } else {
                            self.notify(Notice::ScreenshotSaved { file });
                        }
                    }
                    kernel::Operation::Subtitle(url) => {
                        let file = self.source_name(&MediaSource::new(url));
                        self.state.player.subtitle_file = file.clone();
                        self.notify(Notice::SubtitleChosen { file });
                    }
                    kernel::Operation::Setting => {}
                },
            }
        }
    }

    pub(super) fn tick_player(&mut self) {
        self.take_kernel_results();
        if !self.state.playing {
            return;
        }
        self.feedback_remaining = (self.feedback_remaining - 0.25).max(0.0);
        self.state.player.feedback = self.feedback_remaining > 0.0;
        if self.state.video {
            if let Some(kernel) = self.kernel() {
                self.follow(kernel.playback());
                while let Some((file, result)) = kernel.take_danmaku_result() {
                    match result {
                        Ok(count) => {
                            self.add_danmaku_source(file.clone(), count);
                            self.notify(Notice::DanmakuMounted {
                                file,
                                count: count as i64,
                            });
                        }
                        Err(_) => self.notify(Notice::DanmakuUnreadable { file }),
                    }
                }
                if let Some(url) = kernel.captured_source() {
                    self.state.still = MediaSource::new(url);
                    self.still_wanted = None;
                }
            }
            self.save_history_now_and_then();
            return;
        }
        if self.state.paused {
            return;
        }
        self.update_position(self.state.progress * LENGTH + 0.25 * self.rate());
        if self.state.progress >= 1.0 {
            self.advance_at_end();
        }
    }

    /// Take on what the kernel plays. A pause or seek the user just asked
    /// for holds for a few ticks until the kernel shows it. When the media
    /// fails, the frame's video says why.
    fn follow(&mut self, playback: Playback) {
        if playback.source.as_deref().is_some_and(|source| {
            self.source.as_ref().map(|current| current.url.as_str()) != Some(source)
        }) {
            return;
        }
        if playback.error.is_some() {
            self.state.paused = true;
            return;
        }
        if !playback.loaded {
            return;
        }
        // A picture of the media, past its fade-in, stands for it from now
        // on.
        if playback.picture
            && playback.still.is_none()
            && !self.state.still.url.starts_with("blob:")
            && self.still_wanted.is_none()
            && (playback.position >= self.first_still_at || playback.ended)
        {
            self.capture_still();
        }
        if let Some(still) = &playback.still
            && self.still_wanted.as_ref() == Some(still)
        {
            self.state.still = MediaSource::file(still);
            self.still_path = Some(still.clone());
            self.still_wanted = None;
        }
        let duration = playback.duration.unwrap_or(0.0).max(0.0);
        self.state.duration = duration;
        self.refresh_chapters(&playback.chapters);
        let mut position = playback.position.max(0.0);
        if let Some((target, ticks)) = self.seek_wanted {
            if (position - target).abs() < 1.5 || ticks >= 8 {
                self.seek_wanted = None;
            } else {
                self.seek_wanted = Some((target, ticks + 1));
                position = target;
            }
        }
        if duration > 0.0 {
            position = position.min(duration);
        }
        self.state.elapsed = position;
        self.state.progress = if duration > 0.0 {
            position / duration
        } else {
            0.0
        };
        let paused = !playback.playing;
        match self.pause_wanted {
            Some((wanted, ticks)) if wanted != paused && ticks < 4 => {
                self.pause_wanted = Some((wanted, ticks + 1));
            }
            _ => {
                self.pause_wanted = None;
                self.state.paused = paused;
            }
        }
        if playback.ended {
            self.state.progress = 1.0;
            self.state.elapsed = duration;
            self.state.paused = true;
        }
        let player = &mut self.state.player;
        player.subtitle_tracks = media_tracks(&playback.subtitle_tracks);
        player.audio_tracks = media_tracks(&playback.audio_tracks);
        player.subtitle_track = playback.subtitle_track.unwrap_or(-1);
        player.audio_track = playback.audio_track.unwrap_or(-1);
        player.video_info = playback.video;
        name_tracks(player);
        self.update_chapter_position();
        if playback.ended && self.seek_wanted.is_none() {
            self.advance_at_end();
        }
    }

    pub(super) fn select_subtitle(&mut self, id: i64) -> Result<(), CommandError> {
        if id >= 0
            && !self
                .state
                .player
                .subtitle_tracks
                .iter()
                .any(|track| track.id == id)
        {
            return Err(CommandError::new(
                "no_such_track",
                "the media has no such subtitle track",
            ));
        }
        self.state.player.subtitle_track = id.max(-1);
        name_tracks(&mut self.state.player);
        if self.state.video
            && let Some(kernel) = self.kernel()
        {
            kernel.select_subtitle((id >= 0).then_some(id));
        }
        Ok(())
    }

    pub(super) fn select_audio(&mut self, id: i64) -> Result<(), CommandError> {
        if !self
            .state
            .player
            .audio_tracks
            .iter()
            .any(|track| track.id == id)
        {
            return Err(CommandError::new(
                "no_such_track",
                "the media has no such audio track",
            ));
        }
        self.state.player.audio_track = id;
        name_tracks(&mut self.state.player);
        if self.state.video
            && let Some(kernel) = self.kernel()
        {
            kernel.select_audio(id);
        }
        Ok(())
    }

    pub(super) fn add_subtitle(&mut self, source: &MediaSource) {
        if self.state.video
            && let Some(kernel) = self.kernel()
        {
            kernel.add_subtitle(&source.url);
        }
    }

    pub(super) fn player_episode(&mut self, episode: i64) -> Result<(), CommandError> {
        self.require_playing()?;
        if !self.state.player.episodes {
            return Err(CommandError::new(
                "no_episodes",
                "this media has no episode list",
            ));
        }
        self.play_queue(episode, true)
    }

    pub(super) fn player_number(&mut self, key: &str, value: f64) -> Result<(), CommandError> {
        if !value.is_finite() {
            return Err(CommandError::new("invalid_value", "value must be finite"));
        }
        let kernel = self.kernel().filter(|_| self.state.video);
        match key {
            "volume" => {
                self.state.player.volume = value.clamp(0.0, 1.0);
                self.state.player.volume_percent =
                    (self.state.player.volume * 100.0).round() as i64;
                if let Some(kernel) = kernel {
                    kernel.set_volume(self.state.player.volume);
                }
            }
            "subtitle_size" => {
                self.state.player.subtitle_size = value.clamp(0.0, 1.0);
                self.state.player.subtitle_scale_percent =
                    (subtitle_scale(self.state.player.subtitle_size) * 100.0).round() as i64;
                if let Some(kernel) = kernel {
                    kernel.set_subtitle_scale(subtitle_scale(self.state.player.subtitle_size));
                }
            }
            _ => {
                return Err(CommandError::new(
                    "unknown_player_option",
                    "unknown player option",
                ));
            }
        }
        self.db.set_setting(
            &format!("player.{key}"),
            Some(&value.clamp(0.0, 1.0).to_string()),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::Kernel;
    use erika::Commands;
    use std::sync::Mutex;

    fn playing() -> ErikaEngine {
        let mut e = ErikaEngine::default();
        e.play(
            "葬送的芙莉莲".into(),
            4,
            still(),
            0.35,
            MediaSource::new("file:///Videos/frieren-04.mkv"),
        )
        .unwrap();
        e
    }
    #[test]
    fn seeking_clamps_and_never_toggles_pause() {
        let mut e = playing();
        e.toggle_pause().unwrap();
        e.skip(10.0).unwrap();
        assert!(e.state.paused);
        assert_eq!(e.state.elapsed.floor(), 8.0 * 60.0 + 39.0);
        e.skip(-10000.0).unwrap();
        assert_eq!(e.state.progress, 0.0);
        e.seek(2.0).unwrap();
        assert_eq!(e.state.progress, 1.0);
        assert!(e.seek(f64::NAN).is_err());
    }
    #[test]
    fn clock_obeys_pause_rate_and_end_policy() {
        let mut e = playing();
        e.seek(0.0).unwrap();
        e.set_speed(PlaybackSpeed::Double).unwrap();
        e.player_tick().unwrap();
        assert!((e.state.progress * LENGTH - 0.5).abs() < 0.001);
        e.toggle_pause().unwrap();
        let position = e.state.progress;
        e.player_tick().unwrap();
        assert_eq!(position, e.state.progress);
        e.toggle_pause().unwrap();
        e.seek(1.0).unwrap();
        e.player_tick().unwrap();
        // A single source has no invented following episode.
        assert_eq!(e.state.player.episode, 4);
        assert!(e.state.paused);
        assert!(e.select_episode(12).is_err());
    }
    #[test]
    fn panels_and_episode_boundaries_preserve_the_player() {
        let mut e = playing();
        e.player_panel(true).unwrap();
        assert!(e.state.expanded);
        assert!(e.select_episode(13).is_err());
        assert_eq!(e.state.player.episode, 4);
        e.minimize().unwrap();
        assert!(!e.state.player.panel);
        e.expand().unwrap();
        assert!(e.state.expanded);
        e.open_file(MediaSource::new("file:///video.mp4")).unwrap();
        assert!(!e.state.player.episodes);
        assert!(e.change_episode(1).is_err());
    }
    #[test]
    fn simulated_media_has_no_subtitles() {
        let mut e = playing();
        assert!(e.state.player.subtitle_tracks.is_empty());
        assert_eq!(
            (
                e.state.player.subtitle_track,
                e.state.player.subtitle_name.as_str()
            ),
            (-1, "")
        );
        assert_eq!(
            e.select_subtitle_track(1).unwrap_err().code,
            "no_such_track"
        );
        e.select_subtitle_track(-1).unwrap();
        e.select_audio_track(2).unwrap();
        assert_eq!(e.state.player.audio_name, "中文配音 · AAC 2.0");
        assert_eq!(e.select_audio_track(-1).unwrap_err().code, "no_such_track");
    }

    /// Records what the engine asks and answers with `playback`.
    #[derive(Default)]
    struct FakeKernel {
        calls: Mutex<Vec<String>>,
        playback: Mutex<Playback>,
        results: Mutex<Vec<crate::kernel::OperationResult>>,
    }

    impl FakeKernel {
        fn call(&self, call: String) {
            self.calls.lock().unwrap().push(call);
        }
        fn calls(&self) -> Vec<String> {
            std::mem::take(&mut *self.calls.lock().unwrap())
        }
    }

    impl Kernel for FakeKernel {
        fn ready(&self) -> bool {
            true
        }
        fn open(&self, url: &str, start: f64) {
            self.call(format!("open {url} {start}"))
        }
        fn play(&self) {
            self.call("play".into())
        }
        fn pause(&self) {
            self.call("pause".into())
        }
        fn seek(&self, seconds: f64) {
            self.call(format!("seek {seconds}"))
        }
        fn set_volume(&self, volume: f64) {
            self.call(format!("volume {volume}"))
        }
        fn set_rate(&self, rate: f64) {
            self.call(format!("rate {rate}"))
        }
        fn select_audio(&self, id: i64) {
            self.call(format!("audio {id}"))
        }
        fn select_subtitle(&self, id: Option<i64>) {
            self.call(format!("subtitle {id:?}"))
        }
        fn add_subtitle(&self, url: &str) {
            self.call(format!("add {url}"))
        }
        fn set_subtitle_scale(&self, scale: f64) {
            self.call(format!("scale {scale}"))
        }
        fn set_subtitle_supersampling(&self, enabled: bool) {
            self.call(format!("subtitle supersampling {enabled}"))
        }
        fn set_enhancement(&self, enhancement: Enhancement) {
            self.call(format!("enhance {enhancement:?}"))
        }
        fn set_danmaku(&self, config: &crate::kernel::DanmakuConfig) {
            self.call(format!(
                "danmaku {} {} {} {} {} {}",
                config.enabled,
                config.opacity,
                config.font_size,
                config.area,
                config.scroll_seconds,
                config.delay
            ))
        }
        fn add_danmaku(&self, name: &str, contents: &str) -> Result<usize, String> {
            self.call(format!("add danmaku {name}"));
            contents.parse::<usize>().map_err(|error| error.to_string())
        }
        fn send_danmaku(&self, text: &str) {
            self.call(format!("send {text}"))
        }
        fn capture(&self, path: &std::path::Path, width: Option<u32>) {
            self.call(format!("capture {} {width:?}", path.display()))
        }
        fn close(&self) {
            self.call("close".into())
        }
        fn playback(&self) -> Playback {
            self.playback.lock().unwrap().clone()
        }
        fn take_results(&self) -> Vec<crate::kernel::OperationResult> {
            std::mem::take(&mut *self.results.lock().unwrap())
        }
    }

    fn with_kernel() -> (ErikaEngine, Arc<FakeKernel>) {
        let kernel = Arc::new(FakeKernel::default());
        let mut engine = ErikaEngine::default();
        engine.kernel = Some(kernel.clone());
        (engine, kernel)
    }

    #[test]
    fn subtitle_quality_is_independent_from_picture_enhancement_and_saved() {
        let (mut e, kernel) = with_kernel();
        assert_eq!(e.state.player.enhancement, Enhancement::Off);
        assert!(e.state.player.subtitle_supersampling);
        e.open_file(MediaSource::new("file:///Videos/a.mkv"))
            .unwrap();
        let calls = kernel.calls();
        assert!(calls.contains(&"enhance Off".to_string()));
        assert!(calls.contains(&"subtitle supersampling true".to_string()));

        e.set_subtitle_supersampling(false).unwrap();
        assert_eq!(kernel.calls(), ["subtitle supersampling false"]);
        assert_eq!(
            e.db.setting("subtitle_supersampling").as_deref(),
            Some("false")
        );
        e.open_file(MediaSource::new("file:///Videos/b.mkv"))
            .unwrap();
        assert!(!e.state.player.subtitle_supersampling);
        assert!(
            kernel
                .calls()
                .contains(&"subtitle supersampling false".to_string())
        );
        e.set_subtitle_supersampling(true).unwrap();
        assert_eq!(
            e.db.setting("subtitle_supersampling").as_deref(),
            Some("true")
        );
    }

    #[test]
    fn files_play_through_the_kernel_and_follow_what_it_reports() {
        let (mut e, kernel) = with_kernel();
        e.open_file(MediaSource::new("file:///Videos/%E5%BD%B1.mkv"))
            .unwrap();
        assert!(e.state.video && e.state.playing);
        let calls = kernel.calls();
        let open = calls
            .iter()
            .position(|call| call == "open file:///Videos/%E5%BD%B1.mkv 0")
            .unwrap();
        assert_eq!(calls[open + 1..], ["volume 0.7"]);
        *kernel.playback.lock().unwrap() = Playback {
            loaded: true,
            playing: true,
            position: 30.0,
            duration: Some(120.0),
            subtitle_tracks: vec![Track {
                id: 3,
                name: "简体中文 · ASS".into(),
            }],
            subtitle_track: Some(3),
            audio_tracks: vec![Track {
                id: 1,
                name: "JPN · OPUS".into(),
            }],
            audio_track: Some(1),
            video: "HEVC · 1080p".into(),
            ..Playback::default()
        };
        e.player_tick().unwrap();
        assert_eq!(
            (e.state.elapsed, e.state.duration, e.state.progress),
            (30.0, 120.0, 0.25)
        );
        assert!(!e.state.paused);
        assert_eq!(e.state.player.subtitle_name, "简体中文 · ASS");
        assert_eq!(e.state.player.audio_name, "JPN · OPUS");
        // A pause holds until the kernel reports it.
        e.toggle_pause().unwrap();
        let calls = kernel.calls();
        assert_eq!(calls[0], "pause");
        assert!(calls[1].starts_with("capture ") && calls[1].ends_with(".png Some(480)"));
        e.player_tick().unwrap();
        assert!(e.state.paused);
        e.skip(10.0).unwrap();
        assert_eq!(kernel.calls(), ["seek 40"]);
        e.player_tick().unwrap();
        assert_eq!(e.state.elapsed, 40.0);
        e.select_subtitle_track(-1).unwrap();
        e.set_player_number("volume".into(), 0.4).unwrap();
        e.set_speed(PlaybackSpeed::Double).unwrap();
        assert_eq!(kernel.calls(), ["subtitle None", "volume 0.4", "rate 2"]);
        kernel.playback.lock().unwrap().ended = true;
        e.player_tick().unwrap();
        assert!(e.state.paused && e.state.progress == 1.0);
        // Back in the empty frame, the file plays on from where it was.
        e.return_to_frame().unwrap();
        assert!(!e.state.video);
        assert_eq!(kernel.calls(), ["close"]);
        assert_eq!(
            e.state.recents[0].source,
            Some(MediaSource::new("file:///Videos/%E5%BD%B1.mkv"))
        );
    }

    #[test]
    fn the_first_picture_becomes_the_still() {
        let (mut e, kernel) = with_kernel();
        e.open_file(MediaSource::new("file:///Videos/a.mkv"))
            .unwrap();
        kernel.calls();
        let sample = e.state.still.clone();
        *kernel.playback.lock().unwrap() = Playback {
            loaded: true,
            playing: true,
            picture: true,
            position: 1.0,
            ..Playback::default()
        };
        e.player_tick().unwrap();
        assert!(kernel.calls().is_empty(), "not during the fade-in");
        kernel.playback.lock().unwrap().position = 3.5;
        e.player_tick().unwrap();
        let calls = kernel.calls();
        let path = calls[0]
            .strip_prefix("capture ")
            .unwrap()
            .trim_end_matches(" Some(480)")
            .to_string();
        // Asked once, until the kernel has written it.
        e.player_tick().unwrap();
        assert!(kernel.calls().is_empty());
        assert_eq!(e.state.still, sample);
        kernel.playback.lock().unwrap().still = Some(path.clone().into());
        e.player_tick().unwrap();
        assert_eq!(e.state.still, MediaSource::file(&path));
        // The recent videos show it while it is there.
        std::fs::create_dir_all(std::path::Path::new(&path).parent().unwrap()).unwrap();
        std::fs::write(&path, b"png").unwrap();
        e.return_to_frame().unwrap();
        assert_eq!(e.state.recents[0].still, MediaSource::file(&path));
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn recent_videos_and_where_each_file_stopped_are_kept() {
        let (mut e, kernel) = with_kernel();
        assert!(e.state.recents.is_empty());
        assert_eq!(e.return_to_frame().unwrap_err().code, "nothing_playing");
        assert_eq!(e.continue_last().unwrap_err().code, "no_such_recent");
        let dir = std::env::temp_dir().join(format!("erika-history-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["one", "two", "three", "four"] {
            std::fs::write(dir.join(format!("{name}.mkv")), name).unwrap();
        }
        let play = |e: &mut ErikaEngine, name: &str, position: f64| {
            e.open_file(MediaSource::file(dir.join(name))).unwrap();
            *kernel.playback.lock().unwrap() = Playback {
                loaded: true,
                playing: true,
                position,
                duration: Some(1440.0),
                ..Playback::default()
            };
            e.player_tick().unwrap();
            e.return_to_frame().unwrap();
        };
        for name in ["one.mkv", "two.mkv", "three.mkv", "four.mkv"] {
            play(&mut e, name, 600.0);
        }
        // The newest first, and no more than three.
        let names: Vec<_> = e
            .state
            .recents
            .iter()
            .map(|recent| recent.name.as_str())
            .collect();
        assert_eq!(names, ["four.mkv", "three.mkv", "two.mkv"]);
        assert_eq!(e.state.recents[0].progress, 600.0 / 1440.0);
        assert_eq!(e.state.recents[0].remaining, 840.0);
        // Played again, it picks up where it stopped and moves to the front.
        kernel.calls();
        e.play_recent("two.mkv".into()).unwrap();
        assert_eq!(e.state.elapsed, 600.0);
        assert!(
            kernel
                .calls()
                .iter()
                .any(|call| call.starts_with("open ") && call.ends_with(" 600"))
        );
        e.return_to_frame().unwrap();
        assert_eq!(e.state.recents[0].name, "two.mkv");
        // A copy of a file is the same file: it picks up there too.
        std::fs::write(dir.join("copy.mkv"), "two").unwrap();
        kernel.calls();
        e.open_file(MediaSource::file(dir.join("copy.mkv")))
            .unwrap();
        assert_eq!(e.state.elapsed, 600.0);
        e.close_player().unwrap();
        assert_eq!(
            e.play_recent("gone.mkv".into()).unwrap_err().code,
            "no_such_recent"
        );
        e.continue_last().unwrap();
        assert_eq!(e.state.title, "copy");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn screenshots_are_saved_at_the_videos_size_and_named_after_the_moment() {
        let (mut e, kernel) = with_kernel();
        e.open_file(MediaSource::new("file:///Videos/a%3Ab.mkv"))
            .unwrap();
        *kernel.playback.lock().unwrap() = Playback {
            loaded: true,
            position: 3725.0,
            duration: Some(4000.0),
            ..Playback::default()
        };
        e.player_tick().unwrap();
        kernel.calls();
        e.screenshot().unwrap();
        let call = kernel.calls().pop().unwrap();
        assert!(
            call.ends_with("/Pictures/Erika/a_b.mkv 01-02-05.png None"),
            "{call}"
        );
        assert_eq!(e.state.notice, None);
        let path = std::path::PathBuf::from("/Pictures/Erika/a_b.mkv 01-02-05.png");
        kernel
            .results
            .lock()
            .unwrap()
            .push(crate::kernel::OperationResult {
                source: e.source.as_ref().map(|s| s.url.clone()),
                operation: crate::kernel::Operation::Screenshot(path.clone()),
                result: Ok(()),
            });
        e.player_tick().unwrap();
        assert_eq!(
            e.state.notice,
            Some(Notice::ScreenshotSaved {
                file: path.to_string_lossy().into_owned()
            })
        );
        kernel
            .results
            .lock()
            .unwrap()
            .push(crate::kernel::OperationResult {
                source: None,
                operation: crate::kernel::Operation::Screenshot(path),
                result: Err("disk full".into()),
            });
        e.player_tick().unwrap();
        assert_eq!(
            e.state.notice,
            Some(Notice::OperationFailed {
                reason: "disk full".into()
            })
        );
    }

    #[test]
    fn slider_readouts_match_units_and_clamped_settings() {
        let (mut e, _) = with_kernel();
        assert_eq!(e.state.player.volume_percent, 70);
        assert_eq!(e.state.player.subtitle_scale_percent, 100);
        assert_eq!(e.state.danmaku_settings.opacity_percent, 80);
        assert_eq!(e.state.danmaku_settings.font_points, "29.0");
        assert_eq!(e.state.danmaku_settings.scroll_seconds, "10.0");
        assert_eq!(e.state.danmaku_settings.delay_seconds, "0.0");
        e.set_player_number("volume".into(), 2.0).unwrap();
        e.set_player_number("subtitle_size".into(), 0.0).unwrap();
        assert_eq!(e.state.player.volume_percent, 100);
        assert_eq!(e.state.player.subtitle_scale_percent, 50);
        e.set_player_number("subtitle_size".into(), 1.0).unwrap();
        assert_eq!(e.state.player.subtitle_scale_percent, 150);
        e.set_danmaku_number("opacity".into(), 0.29).unwrap();
        e.set_danmaku_number("font_size".into(), 0.29).unwrap();
        e.set_danmaku_number("speed".into(), 1.0).unwrap();
        e.set_danmaku_number("delay".into(), -2.5).unwrap();
        assert_eq!(e.state.danmaku_settings.opacity_percent, 29);
        assert_eq!(e.state.danmaku_settings.font_points, "25.8");
        assert_eq!(e.state.danmaku_settings.scroll_seconds, "6.0");
        assert_eq!(e.state.danmaku_settings.delay_seconds, "2.5");
        assert_eq!(e.danmaku_config().delay, -2.5);
    }

    #[test]
    fn real_chapters_seek_and_skip_to_interval_ends_without_toggling_pause() {
        use crate::kernel::Chapter;
        let (mut e, kernel) = with_kernel();
        e.open_file(MediaSource::new("file:///Videos/chapters.mkv"))
            .unwrap();
        kernel.calls();
        let playback = Playback {
            loaded: true,
            playing: true,
            duration: Some(100.0),
            position: 12.0,
            chapters: vec![
                Chapter {
                    title: "Prologue".into(),
                    start: 0.0,
                    end: None,
                },
                Chapter {
                    title: "OP".into(),
                    start: 10.0,
                    end: None,
                },
                Chapter {
                    title: "Part A".into(),
                    start: 30.0,
                    end: None,
                },
                Chapter {
                    title: "ED".into(),
                    start: 90.0,
                    end: Some(100.0),
                },
            ],
            ..Playback::default()
        };
        e.follow(playback.clone());
        assert_eq!(e.state.player.chapters[1].at, 0.1);
        assert_eq!(e.state.player.current_chapter, 1);
        assert_eq!(e.state.player.skip_kind, SkipKind::Opening);
        e.skip_segment().unwrap();
        assert_eq!(kernel.calls(), ["seek 30"]);
        assert_eq!(e.state.elapsed, 30.0);
        assert!(!e.state.paused);
        assert_eq!(e.state.player.skip_kind, SkipKind::None);
        // A click outside OP/ED is a no-op, never the former +85 seconds.
        e.skip_segment().unwrap();
        assert!(kernel.calls().is_empty());
        e.state.paused = true;
        e.seek_chapter(3).unwrap();
        assert_eq!(kernel.calls(), ["seek 90"]);
        assert!(e.state.paused);
        assert_eq!(e.state.player.skip_kind, SkipKind::Ending);
        e.skip_segment().unwrap();
        assert_eq!(kernel.calls(), ["seek 99.5"]);
        assert_eq!(e.state.player.skip_kind, SkipKind::None);
        assert!(e.seek_chapter(20).is_err());
        assert!(kernel.calls().is_empty());
        e.seek_chapter(1).unwrap();
        kernel.calls();
        e.set_skip_intro(false).unwrap();
        assert_eq!(e.db.setting("skip_segments").as_deref(), Some("false"));
        e.skip_segment().unwrap();
        assert!(kernel.calls().is_empty());
        e.set_skip_intro(true).unwrap();
        assert_eq!(e.state.player.skip_kind, SkipKind::Opening);
        // A new source clears metadata immediately, and rejects a late old report.
        e.open_file(MediaSource::new("file:///Videos/plain.mkv"))
            .unwrap();
        assert!(!e.state.player.has_chapters);
        e.follow(Playback {
            source: Some("file:///Videos/chapters.mkv".into()),
            ..playback
        });
        assert!(!e.state.player.has_chapters);
        e.follow(Playback {
            loaded: true,
            duration: Some(60.0),
            ..Playback::default()
        });
        assert!(e.state.player.chapters.is_empty());
        assert_eq!(e.state.player.skip_kind, SkipKind::None);
    }

    #[test]
    fn delayed_duration_repositions_chapters_and_preserves_post_credit_content() {
        use crate::kernel::Chapter;
        let (mut e, kernel) = with_kernel();
        e.open_file(MediaSource::new("file:///Videos/chapters.mkv"))
            .unwrap();
        kernel.calls();
        let playback = Playback {
            loaded: true,
            position: 92.0,
            chapters: vec![
                Chapter {
                    title: "ED".into(),
                    start: 90.0,
                    end: Some(200.0),
                },
                Chapter {
                    title: "Preview".into(),
                    start: 95.0,
                    end: None,
                },
            ],
            ..Playback::default()
        };
        e.follow(playback.clone());
        assert_eq!(e.state.player.chapters[0].at, 0.0);
        e.follow(Playback {
            duration: Some(100.0),
            ..playback
        });
        assert_eq!(e.state.player.chapters[0].at, 0.9);
        e.skip_segment().unwrap();
        assert_eq!(kernel.calls(), ["seek 95"]);
        assert_eq!(e.state.player.current_chapter, 1);
        assert_eq!(e.state.player.skip_kind, SkipKind::None);
        e.close_player().unwrap();
        assert!(e.state.player.chapters.is_empty());
    }

    #[test]
    fn danmaku_settings_reach_the_kernel_in_its_terms() {
        let (mut e, kernel) = with_kernel();
        e.open_file(MediaSource::new("file:///Videos/a.mkv"))
            .unwrap();
        let calls = kernel.calls();
        let open = calls
            .iter()
            .position(|call| call.starts_with("open "))
            .unwrap();
        // Before the media opens: 80% opacity, 29 points, half the height,
        // 10 seconds across, no delay.
        assert!(
            calls[..open].contains(&"danmaku true 0.8 29 0.5 10 0".to_string()),
            "{calls:?}"
        );
        e.set_danmaku_number("speed".into(), 1.0).unwrap();
        e.set_danmaku_number("delay".into(), -2.5).unwrap();
        e.set_danmaku_area(DanmakuArea::Full).unwrap();
        e.set_preference("show_danmaku".into(), false).unwrap();
        assert_eq!(
            kernel.calls(),
            [
                "danmaku true 0.8 29 0.5 6 0",
                "danmaku true 0.8 29 0.5 6 -2.5",
                "danmaku true 0.8 29 1 6 -2.5",
                "danmaku false 0.8 29 1 6 -2.5",
            ]
        );
        assert_eq!(
            e.set_danmaku_switch("block_colored".into(), true)
                .unwrap_err()
                .code,
            "unknown_danmaku_option"
        );
        e.set_danmaku_switch("block_top".into(), true).unwrap();
        assert!(e.state.danmaku_settings.block_top);
        // Manual opacity changes still reach the renderer.
        e.set_danmaku_number("opacity".into(), 0.5).unwrap();
        assert_eq!(e.danmaku_config().opacity, 0.5);
        assert!(
            kernel
                .calls()
                .contains(&"danmaku false 0.5 29 1 6 -2.5".to_string())
        );
    }

    #[test]
    fn high_resolution_danmaku_is_saved_and_independent_from_layout() {
        let (mut e, _) = with_kernel();
        assert!(e.state.danmaku_settings.supersampling);
        let original = e.danmaku_config();
        e.set_danmaku_switch("supersampling".into(), false).unwrap();
        assert!(!e.danmaku_config().supersampling);
        assert_eq!(
            e.db.setting("danmaku_supersampling").as_deref(),
            Some("false")
        );
        assert_eq!(e.danmaku_config().font_size, original.font_size);
        assert_eq!(e.danmaku_config().scroll_seconds, original.scroll_seconds);
        e.open_file(MediaSource::new("file:///Videos/a.mkv"))
            .unwrap();
        assert!(!e.danmaku_config().supersampling);
        e.set_danmaku_switch("supersampling".into(), true).unwrap();
        assert!(e.danmaku_config().supersampling);
        assert_eq!(
            e.db.setting("danmaku_supersampling").as_deref(),
            Some("true")
        );
    }

    #[test]
    fn danmaku_files_are_mounted_by_hand_and_from_beside_the_video() {
        let dir = std::env::temp_dir().join(format!("erika-danmaku-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // The fake kernel takes a file's text as its number of comments.
        std::fs::write(dir.join("ep.xml"), "120").unwrap();
        std::fs::write(dir.join("more.json"), "30").unwrap();
        std::fs::write(dir.join("broken.xml"), "not danmaku").unwrap();
        let (mut e, kernel) = with_kernel();
        e.mount_danmaku(MediaSource::file(dir.join("more.json")))
            .unwrap();
        assert_eq!(e.state.notice, Some(Notice::DanmakuNeedsVideo));
        e.open_file(MediaSource::file(dir.join("ep.mkv"))).unwrap();
        assert!(kernel.calls().contains(&"add danmaku ep.xml".to_string()));
        assert_eq!(
            (
                e.state.player.danmaku_source.as_str(),
                e.state.player.danmaku_count
            ),
            ("ep.xml", 120)
        );
        e.mount_danmaku(MediaSource::file(dir.join("more.json")))
            .unwrap();
        assert_eq!(
            e.state.notice,
            Some(Notice::DanmakuMounted {
                file: "more.json".into(),
                count: 30
            })
        );
        assert_eq!(
            (
                e.state.player.danmaku_source.as_str(),
                e.state.player.danmaku_count
            ),
            ("ep.xml、more.json", 150)
        );
        e.mount_danmaku(MediaSource::file(dir.join("broken.xml")))
            .unwrap();
        assert_eq!(
            e.state.notice,
            Some(Notice::DanmakuUnreadable {
                file: "broken.xml".into()
            })
        );
        // Another video starts without them.
        e.open_file(MediaSource::new("file:///Videos/b.mkv"))
            .unwrap();
        assert_eq!(
            (
                e.state.player.danmaku_source.as_str(),
                e.state.player.danmaku_count
            ),
            ("", 0)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_users_danmaku_shows_through_the_kernel() {
        let (mut e, kernel) = with_kernel();
        e.open_file(MediaSource::new("file:///Videos/a.mkv"))
            .unwrap();
        kernel.calls();
        assert!(e.send_danmaku("  前方高能  ".into()).is_err());
        assert!(kernel.calls().is_empty());
        assert_eq!(e.state.player.danmaku_text, "  前方高能  ");
        e.send_own_danmaku("  前方高能  ");
        assert_eq!(kernel.calls(), ["send 前方高能"]);
    }

    #[test]
    fn real_queue_switches_sources_and_end_obeys_autoplay() {
        let (mut e, kernel) = with_kernel();
        let dir = std::env::temp_dir().join(format!("erika-queue-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["EP10.mkv", "EP1.mkv", "EP2.mkv", "notes.txt"] {
            std::fs::write(dir.join(name), name).unwrap();
        }
        e.open_folder(MediaSource::file(&dir)).unwrap();
        assert_eq!(
            e.state
                .player
                .episode_list
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            ["EP1", "EP2", "EP10"]
        );
        let first = e.source.clone().unwrap();
        e.state.player.subtitle_file = "EP1.srt".into();
        kernel.calls();
        *kernel.playback.lock().unwrap() = Playback {
            source: Some(first.url.clone()),
            loaded: true,
            ended: true,
            duration: Some(100.0),
            position: 100.0,
            ..Playback::default()
        };
        e.player_tick().unwrap();
        assert_eq!(e.state.player.queue_index, 2);
        assert!(e.state.player.subtitle_file.is_empty());
        assert!(kernel.calls().iter().any(|c| c.contains("EP2.mkv 0")));
        e.player_tick().unwrap(); // The old EOF must not skip EP2 as well.
        assert_eq!(e.state.player.queue_index, 2);
        e.select_episode(3).unwrap();
        assert!(!e.state.player.has_next);
        assert!(e.change_episode(1).is_err());
        e.set_preference("autoplay".into(), false).unwrap();
        e.select_episode(1).unwrap();
        *kernel.playback.lock().unwrap() = Playback {
            source: Some(first.url),
            loaded: true,
            ended: true,
            duration: Some(100.0),
            position: 100.0,
            ..Playback::default()
        };
        e.player_tick().unwrap();
        assert_eq!(e.state.player.queue_index, 1);
        assert!(e.state.paused);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn send_acknowledgements_preserve_failed_drafts_and_ignore_previous_playback() {
        use crate::matching::Found;
        let (mut e, kernel) = with_kernel();
        let source = MediaSource::new("file:///Videos/a.mkv");
        e.open_file(source.clone()).unwrap();
        let revision = e.match_revision;
        e.state.player.danmaku_text = "草稿".into();
        e.state.player.sending_danmaku = true;
        assert_eq!(
            e.send_online_danmaku("重复请求".into()).unwrap_err().code,
            "sending"
        );
        assert_eq!(e.state.player.danmaku_text, "草稿");
        e.found.lock().unwrap().push(Found::Sent {
            revision,
            text: "草稿".into(),
            result: Err("离线".into()),
        });
        e.take_found();
        assert!(!e.state.player.sending_danmaku);
        assert_eq!(e.state.player.danmaku_text, "草稿");
        assert_eq!(e.state.player.danmaku_sent_revision, 0);
        e.open_file(source).unwrap(); // Even the same URL is a new playback.
        kernel.calls();
        e.found.lock().unwrap().push(Found::Sent {
            revision,
            text: "草稿".into(),
            result: Ok(()),
        });
        e.take_found();
        assert!(kernel.calls().is_empty());
        assert!(e.state.player.danmaku_text.is_empty());
        assert_eq!(e.state.player.danmaku_sent_revision, 1);
        e.state.player.danmaku_text = "当前弹幕".into();
        e.found.lock().unwrap().push(Found::Sent {
            revision: e.match_revision,
            text: "当前弹幕".into(),
            result: Ok(()),
        });
        e.take_found();
        assert_eq!(kernel.calls(), ["send 当前弹幕"]);
    }

    #[test]
    fn disabled_resume_and_saved_controls_are_honored() {
        let (mut e, _) = with_kernel();
        let fingerprint = erika_store::Fingerprint {
            md5: "01234567890123456789012345678901".into(),
            size: 123,
        };
        let file = e.db.opened("file:///video.mkv", Some(&fingerprint));
        e.db.save_progress(file.unwrap(), 120.0, 500.0);
        e.set_preference("remember_position".into(), false).unwrap();
        assert_eq!(e.saved_position(file), (0.0, 0.0));
        e.set_player_number("volume".into(), 0.25).unwrap();
        e.set_danmaku_number("opacity".into(), 0.61).unwrap();
        e.set_danmaku_switch("block_top".into(), true).unwrap();
        e.set_speed(PlaybackSpeed::Faster).unwrap();
        e.state.preferences = settings::default_preferences();
        e.state.player = initial();
        e.state.danmaku_settings = danmaku::initial();
        e.restore_playback_preferences();
        assert!(!e.state.preferences.remember_position);
        assert_eq!(e.state.player.volume_percent, 25);
        assert_eq!(e.state.danmaku_settings.opacity_percent, 61);
        assert!(e.state.danmaku_settings.block_top);
        assert_eq!(e.state.preferences.speed, PlaybackSpeed::Faster);
    }

    #[test]
    fn a_failing_file_stops_the_clock() {
        let (mut e, kernel) = with_kernel();
        e.open_file(MediaSource::new("file:///broken.mkv")).unwrap();
        kernel.playback.lock().unwrap().error = Some("invalid data".into());
        e.player_tick().unwrap();
        assert!(e.state.paused && e.state.video);
        assert_eq!((e.state.elapsed, e.state.notice.clone()), (0.0, None));
    }

    #[test]
    fn episodes_play_their_files_and_magnets_do_not_fake_playback() {
        let (mut e, kernel) = with_kernel();
        e.open_link("magnet:?xt=urn:btih:abc".into()).unwrap();
        assert!(!e.state.video);
        assert!(kernel.calls().is_empty());
        // An episode the user has no file of plays nothing and says so.
        e.close_player().unwrap();
        e.play(
            "葬送的芙莉莲".into(),
            4,
            still(),
            0.35,
            MediaSource::new(""),
        )
        .unwrap();
        assert!(!e.state.playing);
        assert_eq!(e.state.notice, Some(Notice::NoEpisodeFile));
        e.play(
            "葬送的芙莉莲".into(),
            4,
            still(),
            0.35,
            MediaSource::new("file:///Videos/frieren-04.mkv"),
        )
        .unwrap();
        assert!(e.state.video);
        assert_eq!(e.state.subtitle, PlayingNote::Episode { episode: 4 });
        assert!(
            kernel
                .calls()
                .contains(&"open file:///Videos/frieren-04.mkv 0".to_string())
        );
    }
}
