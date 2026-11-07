//! Playback control from other devices, in the words of NipaPlay's remote
//! control: what its panel reads and what it sends. The remote module takes
//! the requests off the network, after asking the user, and hands them here.

use super::*;
use serde_json::{Value, json};

/// The panes a client lists, which say which `parameters` it may ask for.
const MENUS: &[(&str, &str, &str, &str)] = &[
    ("audioTracks", "音频", "audiotrack", "音轨"),
    ("subtitleTracks", "字幕", "subtitles", "字幕轨道"),
    ("danmakuSettings", "弹幕", "comment", "弹幕设置"),
    ("playbackRate", "播放", "speed", "倍速"),
];

impl ErikaEngine {
    fn control_snapshot(&self) -> Value {
        let state = &self.state;
        let status = match (state.playing, state.paused) {
            (false, _) => "idle",
            (true, true) => "paused",
            (true, false) => "playing",
        };
        let player = &state.player;
        let position = (state.elapsed * 1000.0).round() as i64;
        let duration = (state.duration * 1000.0).round() as i64;
        json!({
            "status": status,
            "hasVideo": state.playing,
            "isPaused": state.paused,
            "canPlayPreviousEpisode": player.has_previous,
            "canPlayNextEpisode": player.has_next,
            "positionMs": position,
            "durationMs": duration,
            "progress": state.progress,
            "bufferedPositionMs": position,
            "playbackRate": self.rate(),
            "speedBoostRate": 2.0,
            "skipSeconds": 10,
            "seekStepSeconds": 10.0,
            "currentVideoPath": state.title,
            "animeTitle": state.title,
            "episodeTitle": "",
            "animeId": null,
            "episodeId": null,
            "danmakuTrackCount": i64::from(player.danmaku_count > 0),
            "danmakuVisible": state.preferences.show_danmaku,
            "subtitleTrackCount": player.subtitle_tracks.len(),
            "audioTrackCount": player.audio_tracks.len(),
            "isStreaming": false,
        })
    }

    fn control_parameters(&self, pane: Option<&str>) -> Value {
        let state = &self.state;
        let tracks = |tracks: &[MediaTrack]| -> Vec<Value> {
            tracks
                .iter()
                .map(|track| json!({ "value": track.id, "label": track.name }))
                .collect()
        };
        let all = [
            (
                "audioTracks",
                json!({
                    "paneId": "audioTracks", "key": "audio.activeTrackIndex", "label": "音轨",
                    "type": "select", "value": state.player.audio_track,
                    "options": tracks(&state.player.audio_tracks),
                }),
            ),
            (
                "subtitleTracks",
                json!({
                    "paneId": "subtitleTracks", "key": "subtitle.activeTrackIndex", "label": "字幕",
                    "type": "select", "value": state.player.subtitle_track,
                    "options": tracks(&state.player.subtitle_tracks),
                }),
            ),
            (
                "danmakuSettings",
                json!({
                    "paneId": "danmakuSettings", "key": "danmaku.visible", "label": "显示弹幕",
                    "type": "bool", "value": state.preferences.show_danmaku,
                }),
            ),
            (
                "danmakuSettings",
                json!({
                    "paneId": "danmakuSettings", "key": "danmaku.opacity", "label": "透明度",
                    "type": "double", "value": state.danmaku_settings.opacity,
                    "min": 0.0, "max": 1.0, "step": 0.01,
                }),
            ),
            (
                "playbackRate",
                json!({
                    "paneId": "playbackRate", "key": "playback.rate", "label": "倍速",
                    "type": "double", "value": self.rate(),
                    "min": 0.25, "max": 3.0, "step": 0.25,
                }),
            ),
        ];
        let chosen: Vec<&Value> = all
            .iter()
            .filter(|(name, _)| pane.is_none_or(|pane| pane == *name))
            .map(|(_, parameter)| parameter)
            .collect();
        json!(chosen)
    }

    /// The `data` of the control state.
    fn control_state(&self, args: &Value) -> Value {
        let menus: Vec<Value> = MENUS
            .iter()
            .map(|(pane, category, icon, title)| {
                json!({ "paneId": pane, "category": category, "icon": icon, "title": title })
            })
            .collect();
        let parameters = if args["includeParameters"] == true {
            self.control_parameters(args["paneId"].as_str())
        } else {
            json!([])
        };
        json!({
            "available": true,
            "message": "ok",
            "snapshot": self.control_snapshot(),
            "menus": menus,
            "parameters": parameters,
        })
    }

    fn control_parameter(&mut self, key: &str, value: &Value) -> Result<(), CommandError> {
        let number = value.as_f64().unwrap_or(0.0);
        let on = value.as_bool().unwrap_or(false);
        match key {
            "playback.rate" => self.play_at(number),
            "danmaku.visible" => erika::Commands::set_preference(self, "show_danmaku".into(), on),
            "danmaku.opacity" => self.danmaku_number("opacity", number),
            "subtitle.activeTrackIndex" => self.select_subtitle(number as i64),
            "audio.activeTrackIndex" => self.select_audio(number as i64),
            _ => Err(CommandError::new("unknown_key", format!("unknown key {key}"))),
        }
    }

    fn control_command(&mut self, command: &str, args: &Value) -> Result<(), CommandError> {
        use erika::Commands;
        match command {
            "toggle_play_pause" => self.toggle_pause(),
            "play_next_episode" => self.change_episode(1),
            "play_previous_episode" => self.change_episode(-1),
            "skip" => self.skip(10.0),
            "seek_by_seconds" => self.skip(args["seconds"].as_f64().unwrap_or(0.0)),
            "toggle_playback_rate" => {
                let rate = if self.rate() > 1.0 { 1.0 } else { 2.0 };
                self.play_at(rate)
            }
            "send_danmaku" => {
                self.send_danmaku(args["comment"].as_str().unwrap_or_default().to_string())
            }
            "set_parameter" => {
                let key = args["key"].as_str().unwrap_or_default().to_string();
                self.control_parameter(&key, &args["value"])
            }
            _ => Err(CommandError::new("unknown_command", format!("unknown command {command}"))),
        }
    }

    /// One request of the remote module: `state`, `command` or
    /// `fallback_font`, with its arguments as JSON; the answer is JSON too.
    pub(crate) fn remote_control_request(&mut self, action: &str, args: &str) -> String {
        let args: Value = serde_json::from_str(args).unwrap_or(Value::Null);
        let answer = if action == "fallback_font" {
            kernel::page_font().map_or(Value::Null, |path| json!({ "path": path }))
        } else if action == "command" {
            let result = self.control_command(
                args["command"].as_str().unwrap_or_default(),
                &args["args"],
            );
            let (success, message) = match result {
                Ok(()) => (true, "ok".to_string()),
                Err(error) => (false, format!("执行失败: {}", error.message)),
            };
            let mut payload = self.control_state(&Value::Null);
            payload["receiverEnabled"] = json!(true);
            json!({ "success": success, "message": message, "payload": payload })
        } else {
            self.control_state(&args)
        };
        answer.to_string()
    }
}
