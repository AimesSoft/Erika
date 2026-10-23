//! Durable playback options; live renderer state and media tracks are not saved.
use super::*;

impl ErikaEngine {
    pub(super) fn restore_playback_preferences(&mut self) {
        use erika::Commands;
        for key in [
            "remember_position",
            "autoplay",
            "hardware_decode",
            "show_danmaku",
            "auto_match",
            "reduce_motion",
        ] {
            if let Some(value) = self.db.setting(key).and_then(|v| v.parse().ok()) {
                let _ = self.set_preference(key.into(), value);
            }
        }
        for key in ["volume", "subtitle_size"] {
            if let Some(value) = self
                .db
                .setting(&format!("player.{key}"))
                .and_then(|v| v.parse().ok())
            {
                let _ = self.player_number(key, value);
            }
        }
        for key in ["opacity", "font_size", "speed", "delay"] {
            if let Some(value) = self
                .db
                .setting(&format!("danmaku.{key}"))
                .and_then(|v| v.parse().ok())
            {
                let _ = self.danmaku_number(key, value);
            }
        }
        for key in ["merge", "block_top", "block_bottom", "block_scroll"] {
            if let Some(value) = self
                .db
                .setting(&format!("danmaku.{key}"))
                .and_then(|v| v.parse().ok())
            {
                let _ = self.danmaku_switch(key, value);
            }
        }
        self.state.preferences.speed = match self.db.setting("playback.speed").as_deref() {
            Some("Half") => PlaybackSpeed::Half,
            Some("Slow") => PlaybackSpeed::Slow,
            Some("Fast") => PlaybackSpeed::Fast,
            Some("Faster") => PlaybackSpeed::Faster,
            Some("Double") => PlaybackSpeed::Double,
            _ => PlaybackSpeed::Normal,
        };
        self.state.preferences.danmaku_area = match self.db.setting("danmaku.area").as_deref() {
            Some("Quarter") => DanmakuArea::Quarter,
            Some("Full") => DanmakuArea::Full,
            _ => DanmakuArea::Half,
        };
        self.state.player.enhancement = match self.db.setting("player.enhancement").as_deref() {
            Some("ArtCnn") => Enhancement::ArtCnn,
            Some("ArtCnnQuality") => Enhancement::ArtCnnQuality,
            Some("ArtCnnDenoise") => Enhancement::ArtCnnDenoise,
            _ => Enhancement::Off,
        };
        self.state.preferences.playback_end = match self.db.setting("playback.end").as_deref() {
            Some("Stop") => PlaybackEnd::Stop,
            _ => PlaybackEnd::Next,
        };
    }
}
