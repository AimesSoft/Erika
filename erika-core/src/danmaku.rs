//! Danmaku: liberika draws the comments of the files the user mounts, and
//! the user's own, over the video; the settings say how.
use super::*;
use crate::kernel::DanmakuConfig;
use std::io::Read;

/// The largest danmaku file read, in bytes.
const LARGEST: u64 = 64 << 20;
/// How far the danmaku offset goes either way, in seconds.
const OFFSET_LIMIT: f64 = 600.0;
/// The composer's colors before plugins add any: white, black, yellow,
/// green, red and blue of the common sending palette, each with its dot.
const SEND_COLORS: [(i64, &str); 6] = [
    (0xFFFFFF, "⚪"),
    (0x000000, "⚫"),
    (0xFFFF00, "🟡"),
    (0x04CD00, "🟢"),
    (0xFE0502, "🔴"),
    (0x4266BE, "🔵"),
];
const WHITE: i64 = 0xFFFFFF;

pub fn initial() -> DanmakuSettings {
    let mut settings = DanmakuSettings {
        opacity: 0.8,
        opacity_percent: 0,
        supersampling: true,
        font_size: 0.45,
        font_points: String::new(),
        speed: 0.5,
        scroll_seconds: String::new(),
        delay: 0.0,
        delay_seconds: String::new(),
        merge: true,
        block_top: false,
        block_bottom: true,
        block_scroll: false,
        random_colors: false,
    };
    update_readouts(&mut settings);
    settings
}

/// The composer's colors: the six of [`SEND_COLORS`], then `extra` ones
/// they do not already hold, shown by their hexadecimal code.
pub fn send_colors(extra: &[i64]) -> Vec<DanmakuColorOption> {
    let mut colors: Vec<DanmakuColorOption> = SEND_COLORS
        .iter()
        .map(|&(rgb, label)| DanmakuColorOption {
            rgb,
            label: label.into(),
        })
        .collect();
    for &rgb in extra {
        if (0..=WHITE).contains(&rgb) && colors.iter().all(|color| color.rgb != rgb) {
            colors.push(DanmakuColorOption {
                rgb,
                label: format!("#{rgb:06X}"),
            });
        }
    }
    colors
}

/// `#RRGGBB` or `RRGGBB` as `0xRRGGBB`.
fn parse_hex(text: &str) -> Option<i64> {
    let digits = text.trim();
    let digits = digits.strip_prefix('#').unwrap_or(digits);
    if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    i64::from_str_radix(digits, 16).ok()
}

fn font_points(value: f64) -> f64 {
    20.0 + 20.0 * value
}
fn scroll_seconds(value: f64) -> f64 {
    14.0 - 8.0 * value
}

fn update_readouts(settings: &mut DanmakuSettings) {
    settings.opacity_percent = (settings.opacity * 100.0).round() as i64;
    settings.font_points = format!("{:.1}", font_points(settings.font_size));
    settings.scroll_seconds = format!("{:.1}", scroll_seconds(settings.speed));
    settings.delay_seconds = format!("{:.1}", settings.delay.abs());
}

/// The danmaku files next to a video file: its name with `.xml` or
/// `.json` in place of its extension.
fn beside(video: &MediaSource) -> Vec<MediaSource> {
    let url = &video.url;
    if !url.starts_with("file:") {
        return Vec::new();
    }
    let name_at = url.rfind('/').map_or(0, |slash| slash + 1);
    let stem = match url[name_at..].rfind('.') {
        Some(dot) if dot > 0 => &url[..name_at + dot],
        _ => url.as_str(),
    };
    ["xml", "json"]
        .iter()
        .map(|extension| MediaSource::new(format!("{stem}.{extension}")))
        .collect()
}

impl ErikaEngine {
    /// liberika's terms for the settings: the size slider from 20 to 40
    /// points (30 in the middle, liberika's own), the speed from 14 seconds
    /// across down to 6.
    pub(super) fn danmaku_config(&self) -> DanmakuConfig {
        let settings = &self.state.danmaku_settings;
        DanmakuConfig {
            enabled: self.state.features.danmaku && self.state.preferences.show_danmaku,
            supersampling: settings.supersampling,
            opacity: settings.opacity,
            font_size: font_points(settings.font_size),
            area: match self.state.preferences.danmaku_area {
                DanmakuArea::Quarter => 0.25,
                DanmakuArea::Half => 0.5,
                DanmakuArea::Full => 1.0,
            },
            scroll_seconds: scroll_seconds(settings.speed),
            delay: settings.delay,
            merge: settings.merge,
            block_top: settings.block_top,
            block_bottom: settings.block_bottom,
            block_scroll: settings.block_scroll,
            random_colors: settings.random_colors,
        }
    }

    /// Hand the settings to the kernel.
    pub(super) fn apply_danmaku(&self) {
        if let Some(kernel) = self.kernel.as_ref().filter(|kernel| kernel.ready()) {
            kernel.set_danmaku(&self.danmaku_config());
        }
    }

    pub(super) fn danmaku_number(&mut self, key: &str, value: f64) -> Result<(), CommandError> {
        if !value.is_finite() {
            return Err(CommandError::new("invalid_value", "value must be finite"));
        }
        let settings = &mut self.state.danmaku_settings;
        match key {
            "opacity" => settings.opacity = value.clamp(0.0, 1.0),
            "font_size" => settings.font_size = value.clamp(0.0, 1.0),
            "speed" => settings.speed = value.clamp(0.0, 1.0),
            "delay" => settings.delay = value.clamp(-OFFSET_LIMIT, OFFSET_LIMIT),
            _ => {
                return Err(CommandError::new(
                    "unknown_danmaku_option",
                    "unknown danmaku option",
                ));
            }
        }
        self.db
            .set_setting(&format!("danmaku.{key}"), Some(&value.to_string()));
        update_readouts(settings);
        self.apply_danmaku();
        Ok(())
    }

    pub(super) fn danmaku_switch(&mut self, key: &str, value: bool) -> Result<(), CommandError> {
        let settings = &mut self.state.danmaku_settings;
        match key {
            "supersampling" => {
                settings.supersampling = value;
                self.db.set_setting(
                    "danmaku_supersampling",
                    Some(if value { "true" } else { "false" }),
                );
            }
            "merge" => settings.merge = value,
            "block_top" => settings.block_top = value,
            "block_bottom" => settings.block_bottom = value,
            "block_scroll" => settings.block_scroll = value,
            "random_colors" => settings.random_colors = value,
            _ => {
                return Err(CommandError::new(
                    "unknown_danmaku_option",
                    "unknown danmaku option",
                ));
            }
        }
        self.db.set_setting(
            &format!("danmaku.{key}"),
            Some(if value { "true" } else { "false" }),
        );
        self.apply_danmaku();
        Ok(())
    }

    /// Show danmaku `seconds` later than their own time; negative is earlier.
    pub(super) fn set_offset(&mut self, seconds: f64) -> Result<(), CommandError> {
        self.danmaku_number("delay", seconds)
    }

    pub(super) fn shift_offset(&mut self, seconds: f64) -> Result<(), CommandError> {
        if !seconds.is_finite() {
            return Err(CommandError::new("invalid_value", "value must be finite"));
        }
        // Tenths, so repeated steps never show 4.999… on the readout.
        let offset = ((self.state.danmaku_settings.delay + seconds) * 10.0).round() / 10.0;
        self.set_offset(offset)
    }

    pub(super) fn set_send_color(&mut self, rgb: i64) -> Result<(), CommandError> {
        if !(0..=WHITE).contains(&rgb) {
            return Err(CommandError::new(
                "invalid_color",
                "颜色需要是 0x000000 到 0xFFFFFF 之间的 RGB 值",
            ));
        }
        self.state.player.danmaku_color = rgb;
        Ok(())
    }

    pub(super) fn set_send_color_hex(&mut self, hex: &str) -> Result<(), CommandError> {
        let rgb = parse_hex(hex).ok_or_else(|| {
            CommandError::new("invalid_color", "请输入六位十六进制颜色，例如 #FFCC00")
        })?;
        self.set_send_color(rgb)
    }

    /// For the plugin system: the composer offers its six colors and then
    /// `colors` (`0xRRGGBB`, those out of range or already offered are
    /// left out), and shows a hexadecimal color field when `hex_input`.
    /// Each call replaces what the previous one added, so
    /// `set_plugin_danmaku_colors(&[], false)` restores the composer; a
    /// chosen color that is no longer offered falls back to white.
    pub fn set_plugin_danmaku_colors(&mut self, colors: &[i64], hex_input: bool) {
        let player = &mut self.state.player;
        player.danmaku_colors = send_colors(colors);
        player.danmaku_hex_input = hex_input;
        if !hex_input
            && player
                .danmaku_colors
                .iter()
                .all(|color| color.rgb != player.danmaku_color)
        {
            player.danmaku_color = WHITE;
        }
    }

    /// Show the comments of `source` over what plays, and say so.
    pub(super) fn mount_danmaku_file(&mut self, source: &MediaSource) {
        let file = self.source_name(&source);
        let Some(kernel) = self
            .kernel
            .clone()
            .filter(|kernel| kernel.ready() && self.state.video)
        else {
            self.notify(Notice::DanmakuNeedsVideo);
            return;
        };
        if kernel.mount_danmaku_file(&source.url, &file) {
            return;
        }
        match read_text(source).and_then(|contents| self.load_danmaku(&kernel, &file, &contents)) {
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

    /// Show the danmaku files that sit beside a video file, quietly.
    pub(super) fn mount_danmaku_beside(&mut self, video: &MediaSource) {
        let Some(kernel) = self.kernel.clone().filter(|kernel| kernel.ready()) else {
            return;
        };
        for source in beside(video) {
            let Ok(contents) = read_text(&source) else {
                continue;
            };
            let file = self.source_name(&source);
            if let Ok(count) = self.load_danmaku(&kernel, &file, &contents) {
                self.add_danmaku_source(file, count);
            }
        }
    }

    pub(super) fn add_danmaku_source(&mut self, file: String, count: usize) {
        self.danmaku_sources.insert(file, count);
        self.state.player.danmaku_source = self
            .danmaku_sources
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join("、");
        self.state.player.danmaku_count = self.danmaku_sources.values().sum::<usize>() as i64;
    }

    /// Show the user's comment over what plays, in `rgb`, framed as theirs.
    pub(super) fn send_own_danmaku(&mut self, text: &str, rgb: i64) {
        let text: String = text.trim().chars().take(40).collect();
        if text.is_empty() || !self.state.video {
            return;
        }
        if let Some(kernel) = self.kernel.as_ref().filter(|kernel| kernel.ready()) {
            kernel.send_danmaku(&text, rgb.clamp(0, WHITE) as u32);
        }
    }
}

/// A danmaku file's text.
fn read_text(source: &MediaSource) -> Result<String, String> {
    let file = source.open().map_err(|error| error.message)?;
    let mut bytes = Vec::new();
    file.take(LARGEST)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn danmaku_beside_a_video_share_its_name() {
        let names: Vec<String> = beside(&MediaSource::new("file:///Videos/%E5%BD%B1.%201.mkv"))
            .into_iter()
            .map(|source| source.url)
            .collect();
        assert_eq!(
            names,
            [
                "file:///Videos/%E5%BD%B1.%201.xml",
                "file:///Videos/%E5%BD%B1.%201.json"
            ]
        );
        assert!(beside(&MediaSource::new("https://example.com/a.mp4")).is_empty());
    }

    #[test]
    fn hex_colors_read_with_or_without_the_hash() {
        assert_eq!(parse_hex("#ffcc00"), Some(0xFFCC00));
        assert_eq!(parse_hex(" 4266BE "), Some(0x4266BE));
        assert_eq!(parse_hex("#fff"), None);
        assert_eq!(parse_hex("#GGGGGG"), None);
        assert_eq!(parse_hex("+12345"), None);
    }

    #[test]
    fn plugins_add_colors_after_the_six() {
        let colors = send_colors(&[0xFFFFFF, 0x89D5FF, -1, 0x1000000]);
        let labels: Vec<_> = colors.iter().map(|color| color.label.as_str()).collect();
        assert_eq!(labels, ["⚪", "⚫", "🟡", "🟢", "🔴", "🔵", "#89D5FF"]);
    }
}
