//! What plugins add to the player: entries in playback settings and
//! drawings along the bottom of the picture that
//! `extend.progressStrip` and `extend.danmakuDensity` turn on, ported from
//! NipaPlay's 底部进度条 and 弹幕密度曲线.

use super::{hooks, text_of};
use crate::ErikaEngine;
use bern::CommandError;
use erika_protocol::erika::{PluginColor, PluginEntryKind, PluginMenuRow};
use serde_json::Value;

/// The current placement and its legacy spelling. Both go to playback
/// settings; installed plugins do not have to be repackaged.
const PLAYER_MENU: &str = "playerMenu";
const LEGACY_DANMAKU_MENU: &str = "danmakuMenu";

/// How finely the progress line moves: the share played, in steps.
const STRIP_STEPS: usize = 400;

/// The density chart's stretches: at most this many, none shorter than a
/// second (NipaPlay's 150 and 1.0).
const DENSITY_SEGMENTS: usize = 150;
const DENSITY_SECONDS: f64 = 1.0;

/// Whether a `pluginUIEntries` item goes in playback settings.
pub(super) fn in_menu(entry: &Value) -> bool {
    matches!(
        text_of(entry, "placement").as_str(),
        PLAYER_MENU | LEGACY_DANMAKU_MENU
    )
}

/// The theme color a plugin names: `accent`, `red`, `orange` or `green`.
/// The drawings follow the system's light or dark appearance, so there is
/// no white: it would draw black in light appearance.
pub(super) fn color_named(name: &str) -> Option<PluginColor> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "accent" => PluginColor::Accent,
        "red" => PluginColor::Red,
        "orange" => PluginColor::Orange,
        "green" => PluginColor::Green,
        _ => return None,
    })
}

fn color_name(color: PluginColor) -> &'static str {
    match color {
        PluginColor::Accent => "accent",
        PluginColor::Red => "red",
        PluginColor::Orange => "orange",
        PluginColor::Green => "green",
    }
}

/// What `extend.progressStrip(color)` and `extend.danmakuDensity(color)`
/// were given: nothing for `null` or `false`, the color named, or the
/// accent for anything else.
pub(super) fn shown_in(value: Option<&Value>) -> Option<PluginColor> {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(Value::String(name)) => Some(color_named(name).unwrap_or(PluginColor::Accent)),
        Some(_) => Some(PluginColor::Accent),
    }
}

/// The comments in each of `segments` equal stretches of `duration`
/// seconds, then each the mean of it and its neighbors, rounded.
pub(crate) fn density(times: &[f64], duration: f64) -> Vec<f64> {
    if times.is_empty() || duration <= 0.0 {
        return Vec::new();
    }
    let segments = DENSITY_SEGMENTS.min((duration / DENSITY_SECONDS).ceil() as usize);
    if segments == 0 {
        return Vec::new();
    }
    let length = duration / segments as f64;
    let mut counts = vec![0u32; segments];
    for &time in times {
        if (0.0..=duration).contains(&time) {
            counts[((time / length) as usize).min(segments - 1)] += 1;
        }
    }
    if counts.iter().all(|&count| count == 0) {
        return Vec::new();
    }
    if segments <= 3 {
        return counts.iter().map(|&count| count as f64).collect();
    }
    (0..segments)
        .map(|index| {
            let around = &counts[index.saturating_sub(1)..=(index + 1).min(segments - 1)];
            let sum: u32 = around.iter().sum();
            (sum as f64 / around.len() as f64).round()
        })
        .collect()
}

/// The progress line's values: 1 up to `progress` (0–1) and 0 after it,
/// `STRIP_STEPS + 1` of them across the width.
pub(crate) fn strip(progress: f64) -> Vec<f64> {
    let filled = (progress.clamp(0.0, 1.0) * STRIP_STEPS as f64).round() as usize;
    (0..=STRIP_STEPS)
        .map(|step| {
            if filled > 0 && step <= filled {
                1.0
            } else {
                0.0
            }
        })
        .collect()
}

impl ErikaEngine {
    /// The settings the running plugins put in playback settings, as they
    /// are now.
    pub(super) fn show_plugin_menu(&mut self) {
        let mut rows = Vec::new();
        for plugin in &self.plugins.installed {
            let id = &plugin.pack.id;
            if !plugin.loaded || !self.plugins.runtimes.contains_key(id) {
                continue;
            }
            for entry in plugin.entries.iter().filter(|entry| in_menu(entry)) {
                let entry_id = text_of(entry, "id");
                let title = text_of(entry, "title");
                if entry_id.is_empty() || title.is_empty() {
                    continue;
                }
                let row = if let Some(enabled) = entry.get("enabled") {
                    let on = self
                        .plugins
                        .saved
                        .switches
                        .get(id)
                        .and_then(|values| values.get(&entry_id).copied())
                        .unwrap_or(enabled.as_bool().unwrap_or(false));
                    PluginMenuRow {
                        key: format!("{id}/{entry_id}"),
                        plugin: id.clone(),
                        id: entry_id,
                        title,
                        kind: PluginEntryKind::Switch,
                        on,
                        color: PluginColor::Accent,
                    }
                } else if let Some(setting) = entry.get("colorSetting") {
                    let color = self
                        .plugins
                        .saved
                        .text
                        .get(id)
                        .and_then(|values| values.get(&entry_id))
                        .and_then(|name| color_named(name))
                        .or_else(|| color_named(&text_of(setting, "default")))
                        .unwrap_or(PluginColor::Accent);
                    PluginMenuRow {
                        key: format!("{id}/{entry_id}"),
                        plugin: id.clone(),
                        id: entry_id,
                        title,
                        kind: PluginEntryKind::Color,
                        on: false,
                        color,
                    }
                } else {
                    continue;
                };
                rows.push(row);
            }
        }
        self.state.player.plugin_menu_shown = !rows.is_empty();
        if self.state.player.plugin_menu != rows {
            self.state.player.plugin_menu = rows;
        }
    }

    /// The progress line and the density chart, as the plugins asked and
    /// for what plays.
    pub(super) fn show_plugin_overlays(&mut self) {
        let extensions = self.plugins.extensions();
        let player = &mut self.state.player;
        player.progress_strip_color = extensions.strip.unwrap_or(PluginColor::Accent);
        player.danmaku_density_color = extensions.density.unwrap_or(PluginColor::Accent);
        self.refresh_progress_strip();
        self.refresh_danmaku_density();
    }

    /// The progress line as far as the video played, while a plugin shows it.
    pub(crate) fn refresh_progress_strip(&mut self) {
        let shown = self.state.video && self.plugins.extensions().strip.is_some();
        let values = if shown {
            strip(self.state.progress)
        } else {
            Vec::new()
        };
        let player = &mut self.state.player;
        player.progress_strip_shown = shown;
        if player.progress_strip != values {
            player.progress_strip = values;
        }
    }

    /// The density of the comments shown, while a plugin shows it.
    pub(crate) fn refresh_danmaku_density(&mut self) {
        let shown = self.state.video && self.plugins.extensions().density.is_some();
        let values = if shown {
            density(&hooks::shown_times(&self.plugins), self.state.duration)
        } else {
            Vec::new()
        };
        let player = &mut self.state.player;
        player.danmaku_density_shown = !values.is_empty();
        if player.danmaku_density != values {
            player.danmaku_density = values;
        }
    }

    fn menu_row(&self, plugin: &str, entry: &str) -> Result<(), CommandError> {
        if self
            .state
            .player
            .plugin_menu
            .iter()
            .any(|row| row.plugin == plugin && row.id == entry)
        {
            Ok(())
        } else {
            Err(CommandError::new(
                "unknown_plugin_setting",
                "没有这个插件设置",
            ))
        }
    }

    pub(crate) fn set_plugin_menu_switch_command(
        &mut self,
        plugin: String,
        entry: String,
        value: bool,
    ) -> Result<(), CommandError> {
        self.menu_row(&plugin, &entry)?;
        self.plugins
            .saved
            .switches
            .entry(plugin.clone())
            .or_default()
            .insert(entry.clone(), value);
        self.plugins.saved.save(&self.db);
        self.show_plugin_menu();
        // As a switch on the plugin's own page: its action runs.
        self.invoke_action(&plugin, &entry);
        Ok(())
    }

    pub(crate) fn set_plugin_menu_color_command(
        &mut self,
        plugin: String,
        entry: String,
        color: PluginColor,
    ) -> Result<(), CommandError> {
        self.menu_row(&plugin, &entry)?;
        self.plugins
            .saved
            .text
            .entry(plugin.clone())
            .or_default()
            .insert(entry.clone(), color_name(color).into());
        self.plugins.saved.save(&self.db);
        self.show_plugin_menu();
        self.invoke_action(&plugin, &entry);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_counts_each_stretch_and_smooths_it() {
        // Ten seconds: ten one-second stretches.
        let times = [0.5, 0.6, 0.7, 5.5, 9.9];
        let values = density(&times, 10.0);
        assert_eq!(values.len(), 10);
        // The first: (3 + 0) / 2; the second: (3 + 0 + 0) / 3.
        assert_eq!(values[0], 2.0);
        assert_eq!(values[1], 1.0);
        assert_eq!(values[5], 0.0);
        assert_eq!(values[9], 1.0);
        // A long video: 150 stretches, comments past its end left out.
        assert_eq!(density(&[1.0, 1500.0], 1440.0).len(), 150);
        assert!(density(&[], 10.0).is_empty());
        assert!(density(&[20.0], 10.0).is_empty());
    }

    #[test]
    fn the_strip_fills_up_to_the_position() {
        assert!(strip(0.0).iter().all(|&value| value == 0.0));
        assert!(strip(1.0).iter().all(|&value| value == 1.0));
        let half = strip(0.5);
        assert_eq!(half.len(), STRIP_STEPS + 1);
        assert_eq!(half[STRIP_STEPS / 2], 1.0);
        assert_eq!(half[STRIP_STEPS / 2 + 1], 0.0);
    }

    #[test]
    fn current_and_legacy_placements_share_playback_settings() {
        for placement in ["playerMenu", "danmakuMenu"] {
            assert!(in_menu(&serde_json::json!({ "placement": placement })));
        }
        assert!(!in_menu(&serde_json::json!({ "id": "about" })));
    }

    #[test]
    fn colors_are_named_as_plugins_write_them() {
        assert_eq!(shown_in(Some(&Value::from("red"))), Some(PluginColor::Red));
        assert_eq!(
            shown_in(Some(&Value::from("#ff7274"))),
            Some(PluginColor::Accent)
        );
        assert_eq!(
            shown_in(Some(&Value::Bool(true))),
            Some(PluginColor::Accent)
        );
        assert_eq!(shown_in(Some(&Value::Null)), None);
        assert_eq!(shown_in(None), None);
        assert_eq!(color_name(PluginColor::Green), "green");
    }
}
