//! Playback state. liberika plays the files and links the user opens (see
//! `kernel`); the library's sample episodes, and any media where no kernel
//! runs, play on a simulated clock over their stills.
use super::*;
use crate::kernel::{Playback, Track};

/// The tracks simulated media offers.
fn sample_subtitles() -> Vec<MediaTrack> {
    vec![
        MediaTrack { id: 1, name: "简体中文 · ASS".into() },
        MediaTrack { id: 2, name: "日本語 · ASS".into() },
    ]
}

fn sample_audio() -> Vec<MediaTrack> {
    vec![
        MediaTrack { id: 1, name: "日本語 · FLAC 2.0".into() },
        MediaTrack { id: 2, name: "中文配音 · AAC 2.0".into() },
    ]
}

pub fn initial() -> PlayerState {
    let mut player = PlayerState {
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
        enhancement: Enhancement::ArtCnn,
        ratio: PictureRatio::Original,
        hdr: HdrMode::Auto,
        volume: 0.7,
        brightness: 0.5,
        subtitle_size: 0.5,
        subtitle_delay: 0.0,
        audio_delay: 0.0,
        skip_intro: true,
        feedback: false,
        subtitle_line: String::new(),
        sent_danmaku: String::new(),
        subtitle_file: String::new(),
    };
    simulate_tracks(&mut player);
    player
}

/// The sample tracks, the first of each selected.
fn simulate_tracks(player: &mut PlayerState) {
    player.subtitle_tracks = sample_subtitles();
    player.audio_tracks = sample_audio();
    player.subtitle_track = 1;
    player.audio_track = 1;
    player.video_info = "HEVC · 1080p".into();
    name_tracks(player);
}

/// The selected tracks' names.
fn name_tracks(player: &mut PlayerState) {
    let name = |tracks: &[MediaTrack], id: i64| {
        tracks.iter().find(|track| track.id == id).map(|track| track.name.clone()).unwrap_or_default()
    };
    player.subtitle_name = name(&player.subtitle_tracks, player.subtitle_track);
    player.audio_name = name(&player.audio_tracks, player.audio_track);
}

fn media_tracks(tracks: &[Track]) -> Vec<MediaTrack> {
    tracks.iter().map(|track| MediaTrack { id: track.id, name: track.name.clone() }).collect()
}

/// Media the kernel reads: local files and web addresses.
fn playable(url: &str) -> bool {
    url.starts_with("file:") || url.starts_with("http://") || url.starts_with("https://")
}

/// The kernel's subtitle scale for the size slider: 1 at its middle.
fn subtitle_scale(size: f64) -> f64 {
    0.5 + size.clamp(0.0, 1.0)
}

impl ErikaEngine {
    /// Start the player afresh on `episode` of a series, or on a file.
    pub(super) fn reset_player(&mut self, episode: Option<i64>) {
        self.state.player.episode = episode.unwrap_or(1).clamp(1, 12);
        self.state.player.episodes = episode.is_some();
        self.state.player.panel = false;
        self.state.player.feedback = false;
        self.state.player.sent_danmaku.clear();
        self.feedback_remaining = 0.0;
        self.danmaku_remaining = 0.0;
    }

    /// The kernel, while the frontend hosts it.
    fn kernel(&self) -> Option<Arc<dyn Kernel>> {
        self.kernel.clone().filter(|kernel| kernel.ready())
    }

    /// Play `source` with the kernel from `start` seconds when the kernel
    /// runs and reads it; otherwise the player simulates over the still.
    pub(super) fn load(&mut self, source: Option<MediaSource>, start: f64) {
        self.pause_wanted = None;
        self.seek_wanted = None;
        self.still_wanted = None;
        let kernel = self.kernel();
        match (kernel, source) {
            (Some(kernel), Some(source)) if playable(&source.url) => {
                let start = if start.is_finite() { start.max(0.0) } else { 0.0 };
                self.first_still_at = start + 3.0;
                self.state.video = true;
                self.state.progress = 0.0;
                self.state.elapsed = start;
                self.state.duration = 0.0;
                let player = &mut self.state.player;
                player.subtitle_line.clear();
                player.subtitle_tracks.clear();
                player.audio_tracks.clear();
                player.subtitle_track = -1;
                player.audio_track = -1;
                player.video_info.clear();
                name_tracks(player);
                kernel.set_rate(self.rate());
                kernel.set_subtitle_scale(subtitle_scale(self.state.player.subtitle_size));
                kernel.set_enhancement(self.enhancement());
                kernel.open(&source.url, start);
                // The hub sets the volume of the player that has the media.
                kernel.set_volume(self.state.player.volume);
                if self.seek_wanted.is_none() && start > 0.0 {
                    self.seek_wanted = Some((start, 0));
                }
                self.source = Some(source);
            }
            _ => {
                self.unload();
                simulate_tracks(&mut self.state.player);
                self.update_subtitle();
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
        let dir = std::env::temp_dir().join("erika-stills");
        let run = std::process::id().to_string();
        if self.stills == 0 {
            // Earlier runs' pictures: nothing lists them any more.
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                if !entry.file_name().to_string_lossy().starts_with(&format!("{run}-")) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        self.stills += 1;
        let path = dir.join(format!("{run}-{}.png", self.stills));
        kernel.capture(&path, Some(480));
        self.still_wanted = Some(path);
    }

    /// Save what is on screen in Pictures › Erika, named after the title
    /// and the time in it.
    pub(super) fn save_screenshot(&mut self) -> Result<(), CommandError> {
        let kernel = self.kernel().filter(|_| self.state.video);
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
        let file = format!("{title} {:02}-{:02}-{:02}.png", seconds / 3600, seconds / 60 % 60, seconds % 60);
        kernel.capture(&std::path::Path::new(&home).join("Pictures/Erika").join(&file), None);
        self.notify(Notice::ScreenshotSaved { file });
        Ok(())
    }

    /// The kernel lets go of what it played.
    pub(super) fn unload(&mut self) {
        if self.state.video
            && let Some(kernel) = &self.kernel
        {
            kernel.close();
        }
        self.state.video = false;
        self.source = None;
    }

    /// The playback rate the speed preference asks for.
    fn rate(&self) -> f64 {
        match self.state.preferences.speed {
            PlaybackSpeed::Half => 0.5,
            PlaybackSpeed::Slow => 0.75,
            PlaybackSpeed::Normal => 1.0,
            PlaybackSpeed::Fast => 1.25,
            PlaybackSpeed::Faster => 1.5,
            PlaybackSpeed::Double => 2.0,
        }
    }

    /// The upscaler in use: the chosen one while the enhance module is on.
    fn enhancement(&self) -> Enhancement {
        if self.state.features.enhance { self.state.player.enhancement.clone() } else { Enhancement::Off }
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
            let seconds = if duration > 0.0 { seconds.clamp(0.0, duration) } else { seconds.max(0.0) };
            if let Some(kernel) = self.kernel() {
                kernel.seek(seconds);
            }
            self.seek_wanted = Some((seconds, 0));
            self.state.elapsed = seconds;
            self.state.progress = if duration > 0.0 { seconds / duration } else { 0.0 };
            return;
        }
        self.state.progress = seconds.clamp(0.0, LENGTH) / LENGTH;
        self.state.elapsed = self.state.progress * LENGTH;
        self.update_subtitle();
    }

    /// Pause, or play on: from the start once it ended.
    pub(super) fn toggle(&mut self) {
        if self.state.progress >= 1.0 {
            self.update_position(0.0);
        }
        self.state.paused = !self.state.paused;
        if self.state.video
            && let Some(kernel) = self.kernel()
        {
            if self.state.paused { kernel.pause() } else { kernel.play() }
            self.pause_wanted = Some((self.state.paused, 0));
            if self.state.paused {
                self.capture_still();
            }
        }
    }

    /// The simulated subtitle line; the kernel draws real subtitles itself.
    pub(super) fn update_subtitle(&mut self) {
        if self.state.video {
            self.state.player.subtitle_line.clear();
            return;
        }
        let seconds = (self.state.progress * LENGTH - self.state.player.subtitle_delay).max(0.0);
        let i = ((seconds / 4.0) as usize) % 4;
        self.state.player.subtitle_line = match self.state.player.subtitle_track {
            1 => [
                "这一次，我一定要救下她。",
                "……又回到那一天了吗？",
                "别怕，我就在这里。",
                "雪停之前，一起回家吧。",
            ][i],
            2 => [
                "今度こそ、彼女を救う。",
                "……また、あの日に戻ったのか。",
                "怖がらないで、ここにいるよ。",
                "雪が止む前に、一緒に帰ろう。",
            ][i],
            _ => "",
        }
        .into();
    }

    pub(super) fn tick_player(&mut self) {
        if !self.state.playing {
            return;
        }
        self.feedback_remaining = (self.feedback_remaining - 0.25).max(0.0);
        self.state.player.feedback = self.feedback_remaining > 0.0;
        self.danmaku_remaining = (self.danmaku_remaining - 0.25).max(0.0);
        if self.danmaku_remaining == 0.0 {
            self.state.player.sent_danmaku.clear();
        }
        if self.state.video {
            if let Some(kernel) = self.kernel() {
                self.follow(kernel.playback());
            }
            return;
        }
        if self.state.paused {
            return;
        }
        self.update_position(self.state.progress * LENGTH + 0.25 * self.rate());
        if self.state.progress >= 1.0 {
            if self.state.player.episodes
                && self.state.player.episode < 12
                && self.state.preferences.autoplay
                && self.state.preferences.playback_end == PlaybackEnd::Next
            {
                let _ = self.player_episode(self.state.player.episode + 1);
            } else {
                self.state.paused = true;
            }
        }
    }

    /// Take on what the kernel plays. A pause or seek the user just asked
    /// for holds for a few ticks until the kernel shows it. When the media
    /// fails, the frame's video says why.
    fn follow(&mut self, playback: Playback) {
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
            && self.still_wanted.is_none()
            && (playback.position >= self.first_still_at || playback.ended)
        {
            self.capture_still();
        }
        if let Some(still) = &playback.still
            && self.still_wanted.as_ref() == Some(still)
        {
            self.state.still = MediaSource::file(still);
            self.still_wanted = None;
        }
        let duration = playback.duration.unwrap_or(0.0).max(0.0);
        self.state.duration = duration;
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
        self.state.progress = if duration > 0.0 { position / duration } else { 0.0 };
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
    }

    pub(super) fn select_subtitle(&mut self, id: i64) -> Result<(), CommandError> {
        if id >= 0 && !self.state.player.subtitle_tracks.iter().any(|track| track.id == id) {
            return Err(CommandError::new("no_such_track", "the media has no such subtitle track"));
        }
        self.state.player.subtitle_track = id.max(-1);
        name_tracks(&mut self.state.player);
        if self.state.video
            && let Some(kernel) = self.kernel()
        {
            kernel.select_subtitle((id >= 0).then_some(id));
        }
        self.update_subtitle();
        Ok(())
    }

    pub(super) fn select_audio(&mut self, id: i64) -> Result<(), CommandError> {
        if !self.state.player.audio_tracks.iter().any(|track| track.id == id) {
            return Err(CommandError::new("no_such_track", "the media has no such audio track"));
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
        if !(1..=12).contains(&episode) {
            return Err(CommandError::new(
                "episode_out_of_range",
                "episode must be between 1 and 12",
            ));
        }
        self.state.player.episode = episode;
        self.state.subtitle = PlayingNote::Episode { episode };
        self.state.paused = false;
        self.update_position(0.0);
        Ok(())
    }

    pub(super) fn player_number(&mut self, key: &str, value: f64) -> Result<(), CommandError> {
        if !value.is_finite() {
            return Err(CommandError::new("invalid_value", "value must be finite"));
        }
        let kernel = self.kernel().filter(|_| self.state.video);
        match key {
            "volume" => {
                self.state.player.volume = value.clamp(0.0, 1.0);
                if let Some(kernel) = kernel {
                    kernel.set_volume(self.state.player.volume);
                }
            }
            "brightness" => self.state.player.brightness = value.clamp(0.0, 1.0),
            "subtitle_size" => {
                self.state.player.subtitle_size = value.clamp(0.0, 1.0);
                if let Some(kernel) = kernel {
                    kernel.set_subtitle_scale(subtitle_scale(self.state.player.subtitle_size));
                }
            }
            "subtitle_delay" => self.state.player.subtitle_delay = value.clamp(-10.0, 10.0),
            "audio_delay" => self.state.player.audio_delay = value.clamp(-1000.0, 1000.0),
            _ => {
                return Err(CommandError::new(
                    "unknown_player_option",
                    "unknown player option",
                ));
            }
        }
        self.update_subtitle();
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
        e.play("葬送的芙莉莲".into(), 4, still(), 0.35).unwrap();
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
        assert_eq!(e.state.player.episode, 5);
        e.select_episode(12).unwrap();
        e.seek(1.0).unwrap();
        e.player_tick().unwrap();
        assert!(e.state.paused);
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
    fn simulated_tracks_switch_the_sample_subtitles() {
        let mut e = playing();
        e.seek(0.0).unwrap();
        assert_eq!(e.state.player.subtitle_name, "简体中文 · ASS");
        assert_eq!(e.state.player.subtitle_line, "这一次，我一定要救下她。");
        e.select_subtitle_track(2).unwrap();
        assert_eq!(e.state.player.subtitle_line, "今度こそ、彼女を救う。");
        e.select_subtitle_track(-1).unwrap();
        assert_eq!(e.state.player.subtitle_line, "");
        assert_eq!(e.state.player.subtitle_name, "");
        assert_eq!(e.select_subtitle_track(7).unwrap_err().code, "no_such_track");
        e.select_audio_track(2).unwrap();
        assert_eq!(e.state.player.audio_name, "中文配音 · AAC 2.0");
        assert_eq!(e.select_audio_track(-1).unwrap_err().code, "no_such_track");
    }

    /// Records what the engine asks and answers with `playback`.
    #[derive(Default)]
    struct FakeKernel {
        calls: Mutex<Vec<String>>,
        playback: Mutex<Playback>,
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
        fn ready(&self) -> bool { true }
        fn open(&self, url: &str, start: f64) { self.call(format!("open {url} {start}")) }
        fn play(&self) { self.call("play".into()) }
        fn pause(&self) { self.call("pause".into()) }
        fn seek(&self, seconds: f64) { self.call(format!("seek {seconds}")) }
        fn set_volume(&self, volume: f64) { self.call(format!("volume {volume}")) }
        fn set_rate(&self, rate: f64) { self.call(format!("rate {rate}")) }
        fn select_audio(&self, id: i64) { self.call(format!("audio {id}")) }
        fn select_subtitle(&self, id: Option<i64>) { self.call(format!("subtitle {id:?}")) }
        fn add_subtitle(&self, url: &str) { self.call(format!("add {url}")) }
        fn set_subtitle_scale(&self, scale: f64) { self.call(format!("scale {scale}")) }
        fn set_enhancement(&self, enhancement: Enhancement) { self.call(format!("enhance {enhancement:?}")) }
        fn capture(&self, path: &std::path::Path, width: Option<u32>) { self.call(format!("capture {} {width:?}", path.display())) }
        fn close(&self) { self.call("close".into()) }
        fn playback(&self) -> Playback { self.playback.lock().unwrap().clone() }
    }

    fn with_kernel() -> (ErikaEngine, Arc<FakeKernel>) {
        let kernel = Arc::new(FakeKernel::default());
        let mut engine = ErikaEngine::default();
        engine.kernel = Some(kernel.clone());
        (engine, kernel)
    }

    #[test]
    fn files_play_through_the_kernel_and_follow_what_it_reports() {
        let (mut e, kernel) = with_kernel();
        e.open_file(MediaSource::new("file:///Videos/%E5%BD%B1.mkv")).unwrap();
        assert!(e.state.video && e.state.playing);
        assert_eq!(e.state.player.subtitle_line, "");
        let calls = kernel.calls();
        let open = calls.iter().position(|call| call == "open file:///Videos/%E5%BD%B1.mkv 0").unwrap();
        assert_eq!(calls[open + 1..], ["volume 0.7"]);
        *kernel.playback.lock().unwrap() = Playback {
            loaded: true,
            playing: true,
            position: 30.0,
            duration: Some(120.0),
            subtitle_tracks: vec![Track { id: 3, name: "简体中文 · ASS".into() }],
            subtitle_track: Some(3),
            audio_tracks: vec![Track { id: 1, name: "JPN · OPUS".into() }],
            audio_track: Some(1),
            video: "HEVC · 1080p".into(),
            ..Playback::default()
        };
        e.player_tick().unwrap();
        assert_eq!((e.state.elapsed, e.state.duration, e.state.progress), (30.0, 120.0, 0.25));
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
        assert_eq!(e.state.recents[0].source, Some(MediaSource::new("file:///Videos/%E5%BD%B1.mkv")));
    }

    #[test]
    fn the_first_picture_becomes_the_still() {
        let (mut e, kernel) = with_kernel();
        e.open_file(MediaSource::new("file:///Videos/a.mkv")).unwrap();
        kernel.calls();
        let sample = e.state.still.clone();
        *kernel.playback.lock().unwrap() = Playback { loaded: true, playing: true, picture: true, position: 1.0, ..Playback::default() };
        e.player_tick().unwrap();
        assert!(kernel.calls().is_empty(), "not during the fade-in");
        kernel.playback.lock().unwrap().position = 3.5;
        e.player_tick().unwrap();
        let calls = kernel.calls();
        let path = calls[0].strip_prefix("capture ").unwrap().trim_end_matches(" Some(480)").to_string();
        // Asked once, until the kernel has written it.
        e.player_tick().unwrap();
        assert!(kernel.calls().is_empty());
        assert_eq!(e.state.still, sample);
        kernel.playback.lock().unwrap().still = Some(path.clone().into());
        e.player_tick().unwrap();
        assert_eq!(e.state.still, MediaSource::file(&path));
        e.return_to_frame().unwrap();
        assert_eq!(e.state.recents[0].still, MediaSource::file(&path));
    }

    #[test]
    fn screenshots_are_saved_at_the_videos_size_and_named_after_the_moment() {
        let (mut e, kernel) = with_kernel();
        e.open_file(MediaSource::new("file:///Videos/a%3Ab.mkv")).unwrap();
        *kernel.playback.lock().unwrap() = Playback { loaded: true, position: 3725.0, duration: Some(4000.0), ..Playback::default() };
        e.player_tick().unwrap();
        kernel.calls();
        e.screenshot().unwrap();
        let call = kernel.calls().pop().unwrap();
        assert!(call.ends_with("/Pictures/Erika/a_b.mkv 01-02-05.png None"), "{call}");
        assert_eq!(e.state.notice, Some(Notice::ScreenshotSaved { file: "a_b.mkv 01-02-05.png".into() }));
        // Sample episodes have no picture.
        e.play("葬送的芙莉莲".into(), 4, still(), 0.35).unwrap();
        e.screenshot().unwrap();
        assert_eq!(e.state.notice, Some(Notice::ScreenshotPending));
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
    fn sample_episodes_and_magnet_links_still_simulate() {
        let (mut e, kernel) = with_kernel();
        e.open_link("magnet:?xt=urn:btih:abc".into()).unwrap();
        assert!(!e.state.video);
        e.play("葬送的芙莉莲".into(), 4, still(), 0.35).unwrap();
        assert!(!e.state.video);
        assert!(kernel.calls().is_empty());
        e.open_link("https://example.com/a.mp4".into()).unwrap();
        assert!(e.state.video);
        e.play("葬送的芙莉莲".into(), 4, still(), 0.35).unwrap();
        assert!(!e.state.video);
        assert_eq!(kernel.calls().last().unwrap(), "close");
    }
}
