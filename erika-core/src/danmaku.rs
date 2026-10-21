//! Danmaku: liberika draws the comments of the files the user mounts, and
//! the user's own, over the video; the settings say how.
use super::*;
use crate::kernel::DanmakuConfig;
use std::io::Read;

/// The largest danmaku file read, in bytes.
const LARGEST: u64 = 64 << 20;

pub fn initial() -> DanmakuSettings {
    DanmakuSettings {
        opacity: 0.8,
        font_size: 0.45,
        speed: 0.5,
        delay: 0.0,
        merge: true,
        block_top: false,
        block_bottom: true,
        block_scroll: false,
    }
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
    ["xml", "json"].iter().map(|extension| MediaSource::new(format!("{stem}.{extension}"))).collect()
}

impl ErikaEngine {
    /// liberika's terms for the settings: the size slider from 20 to 40
    /// points (30 in the middle, liberika's own), the speed from 14 seconds
    /// across down to 6.
    pub(super) fn danmaku_config(&self) -> DanmakuConfig {
        let settings = &self.state.danmaku_settings;
        DanmakuConfig {
            enabled: self.state.features.danmaku && self.state.preferences.show_danmaku,
            opacity: settings.opacity,
            font_size: 20.0 + 20.0 * settings.font_size,
            area: match self.state.preferences.danmaku_area {
                DanmakuArea::Quarter => 0.25,
                DanmakuArea::Half => 0.5,
                DanmakuArea::Full => 1.0,
            },
            scroll_seconds: 14.0 - 8.0 * settings.speed,
            delay: settings.delay,
            merge: settings.merge,
            block_top: settings.block_top,
            block_bottom: settings.block_bottom,
            block_scroll: settings.block_scroll,
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
            "delay" => settings.delay = value.clamp(-10.0, 10.0),
            _ => return Err(CommandError::new("unknown_danmaku_option", "unknown danmaku option")),
        }
        self.apply_danmaku();
        Ok(())
    }

    pub(super) fn danmaku_switch(&mut self, key: &str, value: bool) -> Result<(), CommandError> {
        let settings = &mut self.state.danmaku_settings;
        match key {
            "merge" => settings.merge = value,
            "block_top" => settings.block_top = value,
            "block_bottom" => settings.block_bottom = value,
            "block_scroll" => settings.block_scroll = value,
            _ => return Err(CommandError::new("unknown_danmaku_option", "unknown danmaku option")),
        }
        self.apply_danmaku();
        Ok(())
    }

    /// Show the comments of `source` over what plays, and say so.
    pub(super) fn mount_danmaku_file(&mut self, source: &MediaSource) {
        let file = source.file_name().unwrap_or_else(|| source.url.clone());
        let Some(kernel) = self.kernel.clone().filter(|kernel| kernel.ready() && self.state.video) else {
            self.notify(Notice::DanmakuNeedsVideo);
            return;
        };
        match read_text(source).and_then(|contents| kernel.add_danmaku(&file, &contents)) {
            Ok(count) => {
                self.add_danmaku_source(file.clone(), count);
                self.notify(Notice::DanmakuMounted { file, count: count as i64 });
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
            let Ok(contents) = read_text(&source) else { continue };
            let file = source.file_name().unwrap_or_else(|| source.url.clone());
            if let Ok(count) = kernel.add_danmaku(&file, &contents) {
                self.add_danmaku_source(file, count);
            }
        }
    }

    fn add_danmaku_source(&mut self, file: String, count: usize) {
        let player = &mut self.state.player;
        if !player.danmaku_source.is_empty() {
            player.danmaku_source.push('、');
        }
        player.danmaku_source.push_str(&file);
        player.danmaku_count += count as i64;
    }

    /// Show the user's comment over what plays.
    pub(super) fn send_own_danmaku(&mut self, text: &str) {
        let text: String = text.trim().chars().take(40).collect();
        if text.is_empty() || !self.state.video {
            return;
        }
        if let Some(kernel) = self.kernel.as_ref().filter(|kernel| kernel.ready()) {
            kernel.send_danmaku(&text);
        }
    }
}

/// A danmaku file's text.
fn read_text(source: &MediaSource) -> Result<String, String> {
    let file = source.open().map_err(|error| error.message)?;
    let mut bytes = Vec::new();
    file.take(LARGEST).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
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
        assert_eq!(names, ["file:///Videos/%E5%BD%B1.%201.xml", "file:///Videos/%E5%BD%B1.%201.json"]);
        assert!(beside(&MediaSource::new("https://example.com/a.mp4")).is_empty());
    }
}
