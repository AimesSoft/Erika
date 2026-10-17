//! Playback UI state. Time is simulated until liberika supplies the timeline.
use super::*;

pub fn initial() -> PlayerState {
    PlayerState {
        panel: false,
        tab: PlayerTab::Danmaku,
        episode: 1,
        episodes: false,
        subtitle_track: SubtitleTrack::Chinese,
        subtitle_label: "简中".into(),
        audio_track: AudioTrack::Japanese,
        enhancement: Enhancement::Anime4K,
        ratio: PictureRatio::Original,
        hdr: HdrMode::Auto,
        volume: 0.7,
        brightness: 0.5,
        subtitle_size: 0.5,
        subtitle_delay: 0.0,
        audio_delay: 0.0,
        skip_intro: true,
        feedback: false,
        subtitle_line: "这一次，我一定要救下她。".into(),
        sent_danmaku: String::new(),
        subtitle_file: String::new(),
    }
}
impl ErikaEngine {
    pub(super) fn reset_player(&mut self, subtitle: &str) {
        let digits: String = subtitle
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit())
            .collect();
        self.state.player.episode = digits.parse::<i64>().unwrap_or(1).clamp(1, 12);
        self.state.player.episodes = subtitle.contains('集');
        self.state.player.panel = false;
        self.state.player.feedback = false;
        self.state.player.sent_danmaku.clear();
        self.feedback_remaining = 0.0;
        self.danmaku_remaining = 0.0;
    }
    pub(super) fn update_position(&mut self, seconds: f64) {
        self.state.progress = seconds.clamp(0.0, LENGTH) / LENGTH;
        self.state.elapsed = clock(self.state.progress * LENGTH);
        self.update_subtitle();
    }
    pub(super) fn update_subtitle(&mut self) {
        self.state.player.subtitle_label = match self.state.player.subtitle_track {
            SubtitleTrack::Chinese => "简中",
            SubtitleTrack::Japanese => "日本語",
            SubtitleTrack::Off => "关闭",
        }
        .into();
        let seconds = (self.state.progress * LENGTH - self.state.player.subtitle_delay).max(0.0);
        let i = ((seconds / 4.0) as usize) % 4;
        self.state.player.subtitle_line = match self.state.player.subtitle_track {
            SubtitleTrack::Chinese => [
                "这一次，我一定要救下她。",
                "……又回到那一天了吗？",
                "别怕，我就在这里。",
                "雪停之前，一起回家吧。",
            ][i],
            SubtitleTrack::Japanese => [
                "今度こそ、彼女を救う。",
                "……また、あの日に戻ったのか。",
                "怖がらないで、ここにいるよ。",
                "雪が止む前に、一緒に帰ろう。",
            ][i],
            SubtitleTrack::Off => "",
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
        if self.state.paused {
            return;
        }
        let speed = match self.state.preferences.speed {
            PlaybackSpeed::Half => 0.5,
            PlaybackSpeed::Slow => 0.75,
            PlaybackSpeed::Normal => 1.0,
            PlaybackSpeed::Fast => 1.25,
            PlaybackSpeed::Faster => 1.5,
            PlaybackSpeed::Double => 2.0,
        };
        self.update_position(self.state.progress * LENGTH + 0.25 * speed);
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
        self.state.subtitle = format!("第 {episode} 集");
        self.state.paused = false;
        self.update_position(0.0);
        Ok(())
    }
    pub(super) fn player_number(&mut self, key: &str, value: f64) -> Result<(), CommandError> {
        if !value.is_finite() {
            return Err(CommandError::new("invalid_value", "value must be finite"));
        }
        match key {
            "volume" => self.state.player.volume = value.clamp(0.0, 1.0),
            "brightness" => self.state.player.brightness = value.clamp(0.0, 1.0),
            "subtitle_size" => self.state.player.subtitle_size = value.clamp(0.0, 1.0),
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
    use erika::Commands;
    fn playing() -> ErikaEngine {
        let mut e = ErikaEngine::default();
        e.play("葬送的芙莉莲".into(), "第 4 集".into(), still(), 0.35)
            .unwrap();
        e
    }
    #[test]
    fn seeking_clamps_and_never_toggles_pause() {
        let mut e = playing();
        e.toggle_pause().unwrap();
        e.skip(10.0).unwrap();
        assert!(e.state.paused);
        assert_eq!(e.state.elapsed, "8:39");
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
}
