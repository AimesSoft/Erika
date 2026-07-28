//! Native Erika danmaku pipeline.
//!
//! The DFM-style track retainer and filters are adapted from the MIT-licensed
//! NipaPlay-Reload DFM+ Rust implementation:
//! Copyright (c) 2025 MCDFsteve.

mod dfm;
pub mod dfm_core;
mod outline;
mod typography;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ab_glyph::{Font, FontArc, FontVec, Glyph, GlyphId, ScaleFont};
use serde_json::Value;
use thiserror::Error;

use crate::NIPAPLAY_FALLBACK_FONT;
use crate::text::TextShaper;
use outline::{raster_radius, resolve_width_px};
use typography::{
    DEFAULT_CONFIG_FONT_SIZE, DEFAULT_NATIVE_FONT_SIZE, DEFAULT_SOURCE_FONT_SIZE,
    effective_config_font_size, effective_font_size, px_scale_for_em,
};

const DEFAULT_SCROLL_DURATION: Duration = Duration::from_millis(9000);
const DEFAULT_STATIC_DURATION: Duration = Duration::from_millis(3800);
const DEFAULT_GENERATION: u64 = 1;
const GLYPH_ATLAS_WIDTH: u32 = 2048;
const GLYPH_ATLAS_INITIAL_HEIGHT: u32 = 256;
const SCROLL_DURATION_REFERENCE_LOGICAL_WIDTH: f32 = 1280.0;
const SCROLL_DURATION_MIN_WIDTH_SCALE: f32 = 0.9;
const SCROLL_DURATION_MAX_WIDTH_SCALE: f32 = 1.3;
const DEFAULT_DANMAKU_TRACK_ID: u64 = 1;
const TRACK_ID_SHIFT: u64 = 48;
const ITEM_ID_MASK: u64 = (1u64 << TRACK_ID_SHIFT) - 1;
pub const DANMAKU_DEBUG_BUCKETS: usize = 16;

#[derive(Debug, Error)]
pub enum DanmakuError {
    #[error("invalid danmaku field: {0}")]
    InvalidField(String),
    #[error("missing danmaku text")]
    MissingText,
    #[error("danmaku parse error: {0}")]
    Parse(String),
    #[error("danmaku io error: {0}")]
    Io(String),
}

pub type Result<T> = std::result::Result<T, DanmakuError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DanmakuMode {
    Scroll,
    ScrollReverse,
    Top,
    Bottom,
    Special,
}

impl DanmakuMode {
    pub fn from_bilibili_mode(value: u32) -> Self {
        match value {
            6 => Self::ScrollReverse,
            5 => Self::Top,
            4 => Self::Bottom,
            7 => Self::Special,
            _ => Self::Scroll,
        }
    }

    fn from_text(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "top" | "fix_top" | "fixed_top" | "5" => Self::Top,
            "bottom" | "fix_bottom" | "fixed_bottom" | "4" => Self::Bottom,
            "reverse" | "scroll_reverse" | "left" | "l2r" | "6" => Self::ScrollReverse,
            "special" | "7" => Self::Special,
            _ => Self::Scroll,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DanmakuColor {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    pub alpha: f32,
}

impl DanmakuColor {
    pub const WHITE: Self = Self::rgb_u8(255, 255, 255);

    pub const fn rgb_u8(red: u8, green: u8, blue: u8) -> Self {
        Self {
            red: red as f32 / 255.0,
            green: green as f32 / 255.0,
            blue: blue as f32 / 255.0,
            alpha: 1.0,
        }
    }

    pub fn rgba(self, opacity: f32) -> [f32; 4] {
        [
            self.red.clamp(0.0, 1.0),
            self.green.clamp(0.0, 1.0),
            self.blue.clamp(0.0, 1.0),
            (self.alpha * opacity).clamp(0.0, 1.0),
        ]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DanmakuItem {
    pub id: u64,
    pub pts: Duration,
    pub text: String,
    pub mode: DanmakuMode,
    pub font_size: f32,
    pub color: DanmakuColor,
    pub opacity: f32,
    pub is_self: bool,
}

impl DanmakuItem {
    fn normalized(mut self, fallback_id: u64) -> Result<Self> {
        if self.text.trim().is_empty() {
            return Err(DanmakuError::MissingText);
        }
        if self.id == 0 {
            self.id = fallback_id;
        }
        self.font_size = sanitize_f32(self.font_size, DEFAULT_SOURCE_FONT_SIZE).max(1.0);
        self.opacity = sanitize_f32(self.opacity, 1.0).clamp(0.0, 1.0);
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DanmakuViewport {
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
}

impl DanmakuViewport {
    pub fn new(width: u32, height: u32) -> Self {
        Self::with_scale(width, height, 1.0)
    }

    pub fn with_scale(width: u32, height: u32, scale_factor: f32) -> Self {
        let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        Self {
            width: width.max(1),
            height: height.max(1),
            scale_factor,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DanmakuLayoutConfig {
    pub font_size: f32,
    pub opacity: f32,
    pub display_area: f32,
    pub scroll_duration_seconds: f32,
    pub scroll_speed_factor: f32,
    pub track_gap_ratio: f32,
    pub outline_width: f32,
    pub shadow_offset: [f32; 2],
    pub shadow_style: DanmakuShadowStyle,
    pub custom_font_family: String,
    pub custom_font_file_path: String,
    pub merge_duplicates: bool,
    pub allow_stacking: bool,
    pub allow_scroll_overwrite: bool,
    pub max_quantity: Option<u32>,
    pub max_lines_per_mode: Option<u32>,
    pub block_top: bool,
    pub block_bottom: bool,
    pub block_scroll: bool,
    pub block_words: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DanmakuShadowStyle {
    None,
    Soft,
    Medium,
    #[default]
    Strong,
}

impl DanmakuShadowStyle {
    pub fn from_code(value: i32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Soft,
            2 => Self::Medium,
            3 => Self::Strong,
            _ => Self::Strong,
        }
    }

    pub fn code(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Soft => 1,
            Self::Medium => 2,
            Self::Strong => 3,
        }
    }
}

impl Default for DanmakuLayoutConfig {
    fn default() -> Self {
        Self {
            font_size: DEFAULT_CONFIG_FONT_SIZE,
            opacity: 1.0,
            display_area: 1.0,
            scroll_duration_seconds: 10.0,
            scroll_speed_factor: 1.0,
            track_gap_ratio: 0.15,
            outline_width: 1.0,
            shadow_offset: [1.0, 1.0],
            shadow_style: DanmakuShadowStyle::Strong,
            custom_font_family: String::new(),
            custom_font_file_path: String::new(),
            merge_duplicates: false,
            allow_stacking: false,
            allow_scroll_overwrite: true,
            max_quantity: None,
            max_lines_per_mode: None,
            block_top: false,
            block_bottom: false,
            block_scroll: false,
            block_words: Vec::new(),
            enabled: true,
        }
    }
}

impl DanmakuLayoutConfig {
    fn sanitized(&self) -> Self {
        let mut config = self.clone();
        config.font_size = sanitize_f32(config.font_size, DEFAULT_CONFIG_FONT_SIZE).max(1.0);
        config.opacity = sanitize_f32(config.opacity, 1.0).clamp(0.0, 1.0);
        config.display_area = sanitize_f32(config.display_area, 1.0).clamp(0.1, 1.0);
        config.scroll_duration_seconds =
            sanitize_f32(config.scroll_duration_seconds, 10.0).clamp(1.0, 60.0);
        config.scroll_speed_factor = sanitize_f32(config.scroll_speed_factor, 1.0).clamp(0.2, 4.0);
        config.track_gap_ratio = sanitize_f32(config.track_gap_ratio, 0.15).clamp(0.0, 2.0);
        config.outline_width = sanitize_f32(config.outline_width, 1.0).clamp(0.0, 4.0);
        config.shadow_offset = [
            sanitize_f32(config.shadow_offset[0], 1.0),
            sanitize_f32(config.shadow_offset[1], 1.0),
        ];
        config.custom_font_family = config.custom_font_family.trim().to_string();
        config.custom_font_file_path = config.custom_font_file_path.trim().to_string();
        config
    }

    fn layout_equivalent(&self, other: &Self) -> bool {
        self.font_size == other.font_size
            && self.display_area == other.display_area
            && self.scroll_duration_seconds == other.scroll_duration_seconds
            && self.scroll_speed_factor == other.scroll_speed_factor
            && self.track_gap_ratio == other.track_gap_ratio
            && self.outline_width == other.outline_width
            && self.custom_font_family == other.custom_font_family
            && self.custom_font_file_path == other.custom_font_file_path
            && self.merge_duplicates == other.merge_duplicates
            && self.allow_stacking == other.allow_stacking
            && self.allow_scroll_overwrite == other.allow_scroll_overwrite
            && self.max_quantity == other.max_quantity
            && self.max_lines_per_mode == other.max_lines_per_mode
            && self.block_top == other.block_top
            && self.block_bottom == other.block_bottom
            && self.block_scroll == other.block_scroll
            && self.block_words == other.block_words
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DanmakuConfigChange {
    Unchanged,
    PaintOnly,
    Layout,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DanmakuTimeline {
    items: Vec<DanmakuItem>,
}

impl DanmakuTimeline {
    pub fn new(items: Vec<DanmakuItem>) -> Result<Self> {
        let mut timeline = Self { items: Vec::new() };
        timeline.extend(items)?;
        Ok(timeline)
    }

    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let input =
            fs::read_to_string(path).map_err(|error| DanmakuError::Io(error.to_string()))?;
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
        {
            Some(ext) if ext == "xml" => Self::from_bilibili_xml(&input),
            Some(ext) if ext == "jsonl" || ext == "ndjson" => Self::from_json_lines(&input),
            Some(ext) if ext == "json" => Self::from_json(&input),
            _ => Self::parse_auto(&input),
        }
    }

    pub fn parse_auto(input: &str) -> Result<Self> {
        let trimmed = input.trim_start();
        if trimmed.starts_with('<') {
            Self::from_bilibili_xml(input)
        } else if trimmed.starts_with('{') {
            Self::from_json(input).or_else(|_| Self::from_json_lines(input))
        } else if trimmed.starts_with('[') {
            Self::from_json(input)
        } else {
            Self::from_json_lines(input)
        }
    }

    pub fn from_json(input: &str) -> Result<Self> {
        let value: Value =
            serde_json::from_str(input).map_err(|error| DanmakuError::Parse(error.to_string()))?;
        let comments = match value {
            Value::Array(items) => items,
            Value::Object(mut object) => object
                .remove("comments")
                .or_else(|| object.remove("danmaku"))
                .or_else(|| object.remove("items"))
                .and_then(|value| value.as_array().cloned())
                .ok_or_else(|| DanmakuError::InvalidField("comments".to_string()))?,
            _ => return Err(DanmakuError::InvalidField("json root".to_string())),
        };
        let mut items = Vec::new();
        for (index, value) in comments.iter().enumerate() {
            match item_from_json_value(value, index as u64 + 1) {
                Ok(item) => items.push(item),
                Err(error) if should_skip_item_error(&error) => continue,
                Err(error) => return Err(error),
            }
        }
        Self::new(items)
    }

    pub fn from_json_lines(input: &str) -> Result<Self> {
        let mut items = Vec::new();
        for (line_index, line) in input.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(line).map_err(|error| {
                DanmakuError::Parse(format!("line {}: {error}", line_index + 1))
            })?;
            match item_from_json_value(&value, items.len() as u64 + 1) {
                Ok(item) => items.push(item),
                Err(error) if should_skip_item_error(&error) => continue,
                Err(error) => return Err(error),
            }
        }
        Self::new(items)
    }

    pub fn from_bilibili_xml(input: &str) -> Result<Self> {
        Self::new(parse_bilibili_xml(input)?)
    }

    pub fn push(&mut self, item: DanmakuItem) -> Result<()> {
        let fallback = self.items.len() as u64 + 1;
        self.items.push(item.normalized(fallback)?);
        self.items.sort_by_key(|item| item.pts);
        Ok(())
    }

    pub fn extend<I>(&mut self, items: I) -> Result<()>
    where
        I: IntoIterator<Item = DanmakuItem>,
    {
        for item in items {
            let fallback = self.items.len() as u64 + 1;
            match item.normalized(fallback) {
                Ok(item) => self.items.push(item),
                Err(DanmakuError::MissingText) => continue,
                Err(error) => return Err(error),
            }
        }
        self.items.sort_by_key(|item| item.pts);
        Ok(())
    }

    pub fn items(&self) -> &[DanmakuItem] {
        &self.items
    }

    pub fn window(&self, start: Duration, end: Duration) -> Self {
        if end < start || self.items.is_empty() {
            return Self::default();
        }
        let items = self
            .items
            .iter()
            .filter(|item| item.pts >= start && item.pts <= end)
            .cloned()
            .collect();
        Self { items }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DanmakuTrackSource {
    Unknown,
    File(PathBuf),
    Json,
    Remote(String),
    Manual,
}

impl DanmakuTrackSource {
    pub fn label(&self) -> String {
        match self {
            Self::Unknown => String::new(),
            Self::File(path) => path.to_string_lossy().into_owned(),
            Self::Json => "json".to_string(),
            Self::Remote(value) => value.clone(),
            Self::Manual => "manual".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DanmakuTrack {
    id: u64,
    name: String,
    source: DanmakuTrackSource,
    timeline: DanmakuTimeline,
    enabled: bool,
    offset: i64,
}

impl DanmakuTrack {
    pub fn new(
        id: u64,
        name: impl Into<String>,
        source: DanmakuTrackSource,
        timeline: DanmakuTimeline,
    ) -> Self {
        Self {
            id: id.max(DEFAULT_DANMAKU_TRACK_ID),
            name: name.into(),
            source,
            timeline,
            enabled: true,
            offset: 0,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn source(&self) -> &DanmakuTrackSource {
        &self.source
    }

    pub fn timeline(&self) -> &DanmakuTimeline {
        &self.timeline
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn offset(&self) -> i64 {
        self.offset
    }

    pub fn offset_duration(&self) -> Duration {
        Duration::from_micros(self.offset.max(0) as u64)
    }

    pub fn item_count(&self) -> usize {
        self.timeline.len()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DanmakuTrackInfo {
    pub id: u64,
    pub name: String,
    pub source: String,
    pub enabled: bool,
    pub offset_micros: i64,
    pub item_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DanmakuSession {
    tracks: Vec<DanmakuTrack>,
    active_timeline: DanmakuTimeline,
    next_track_id: u64,
    global_offset: i64,
    version: u64,
    dirty: bool,
}

impl Default for DanmakuSession {
    fn default() -> Self {
        Self::new()
    }
}

impl DanmakuSession {
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            active_timeline: DanmakuTimeline::default(),
            next_track_id: DEFAULT_DANMAKU_TRACK_ID,
            global_offset: 0,
            version: 1,
            dirty: false,
        }
    }

    pub fn from_timeline(timeline: DanmakuTimeline) -> Self {
        let mut session = Self::new();
        session.replace_default_track(timeline, "default", DanmakuTrackSource::Unknown);
        session
    }

    pub fn add_track(
        &mut self,
        timeline: DanmakuTimeline,
        name: impl Into<String>,
        source: DanmakuTrackSource,
    ) -> u64 {
        let id = self.allocate_track_id();
        self.add_track_with_id(id, timeline, name, source)
    }

    pub fn add_track_with_offset(
        &mut self,
        timeline: DanmakuTimeline,
        name: impl Into<String>,
        source: DanmakuTrackSource,
        offset_micros: i64,
    ) -> u64 {
        let id = self.add_track(timeline, name, source);
        let _ = self.set_track_offset(id, offset_micros);
        id
    }

    pub fn replace_default_track(
        &mut self,
        timeline: DanmakuTimeline,
        name: impl Into<String>,
        source: DanmakuTrackSource,
    ) -> u64 {
        self.tracks.clear();
        self.next_track_id = DEFAULT_DANMAKU_TRACK_ID + 1;
        self.add_track_with_id(DEFAULT_DANMAKU_TRACK_ID, timeline, name, source)
    }

    pub fn clear(&mut self) {
        if self.tracks.is_empty() && self.active_timeline.is_empty() {
            return;
        }
        self.tracks.clear();
        self.active_timeline = DanmakuTimeline::default();
        self.next_track_id = DEFAULT_DANMAKU_TRACK_ID;
        self.mark_changed();
    }

    pub fn remove_track(&mut self, track_id: u64) -> bool {
        let before = self.tracks.len();
        self.tracks.retain(|track| track.id != track_id);
        let removed = self.tracks.len() != before;
        if removed {
            self.mark_dirty();
        }
        removed
    }

    pub fn set_track_enabled(&mut self, track_id: u64, enabled: bool) -> bool {
        let Some(track) = self.tracks.iter_mut().find(|track| track.id == track_id) else {
            return false;
        };
        if track.enabled != enabled {
            track.enabled = enabled;
            self.mark_dirty();
        }
        true
    }

    pub fn set_track_offset(&mut self, track_id: u64, offset_micros: i64) -> bool {
        let Some(track) = self.tracks.iter_mut().find(|track| track.id == track_id) else {
            return false;
        };
        if track.offset != offset_micros {
            track.offset = offset_micros;
            self.mark_dirty();
        }
        true
    }

    pub fn set_global_offset(&mut self, offset_micros: i64) {
        if self.global_offset != offset_micros {
            self.global_offset = offset_micros;
            self.mark_dirty();
        }
    }

    pub fn global_offset(&self) -> i64 {
        self.global_offset
    }

    pub fn tracks(&self) -> &[DanmakuTrack] {
        &self.tracks
    }

    pub fn track_infos(&self) -> Vec<DanmakuTrackInfo> {
        self.tracks
            .iter()
            .map(|track| DanmakuTrackInfo {
                id: track.id,
                name: track.name.clone(),
                source: track.source.label(),
                enabled: track.enabled,
                offset_micros: track.offset,
                item_count: track.item_count(),
            })
            .collect()
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn active_timeline(&mut self) -> &DanmakuTimeline {
        self.rebuild_if_dirty();
        &self.active_timeline
    }

    pub fn active_timeline_clone(&mut self) -> DanmakuTimeline {
        self.active_timeline().clone()
    }

    pub fn is_empty(&mut self) -> bool {
        self.active_timeline().is_empty()
    }

    fn add_track_with_id(
        &mut self,
        id: u64,
        timeline: DanmakuTimeline,
        name: impl Into<String>,
        source: DanmakuTrackSource,
    ) -> u64 {
        let track = DanmakuTrack::new(id, name, source, timeline);
        self.tracks.push(track);
        self.next_track_id = self.next_track_id.max(id.saturating_add(1));
        self.mark_dirty();
        id
    }

    fn allocate_track_id(&mut self) -> u64 {
        let id = self.next_track_id.max(DEFAULT_DANMAKU_TRACK_ID);
        self.next_track_id = id.saturating_add(1).max(DEFAULT_DANMAKU_TRACK_ID + 1);
        id
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.mark_changed();
    }

    fn mark_changed(&mut self) {
        self.version = self.version.saturating_add(1).max(1);
    }

    fn rebuild_if_dirty(&mut self) {
        if !self.dirty {
            return;
        }
        let mut items = Vec::new();
        for track in &self.tracks {
            if !track.enabled {
                continue;
            }
            for item in track.timeline.items() {
                let offset = track.offset.saturating_add(self.global_offset);
                let Some(pts) = apply_track_offset(item.pts, offset) else {
                    continue;
                };
                let mut item = item.clone();
                item.id = compose_track_item_id(track.id, item.id);
                item.pts = pts;
                items.push(item);
            }
        }
        items.sort_by_key(|item| item.pts);
        self.active_timeline = DanmakuTimeline { items };
        self.dirty = false;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedDanmakuItem {
    pub id: u64,
    pub time: Duration,
    pub text: Arc<str>,
    pub mode: DanmakuMode,
    pub color: DanmakuColor,
    pub opacity: f32,
    pub font_size: f32,
    pub width: f32,
    pub height: f32,
    pub y: f32,
    pub track_index: usize,
    pub scroll_speed: f32,
    pub duration: Duration,
    pub duplicate_count: u32,
    text_layout: Arc<PreparedTextLayout>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DfmPreparedLayout {
    viewport: DanmakuViewport,
    config: DanmakuLayoutConfig,
    dfm_layout: dfm::PreparedLayout,
    stats: DanmakuPreparedStats,
    scroll_duration: Duration,
    static_duration: Duration,
    item_times: Vec<f64>,
    items: Vec<PreparedDanmakuItem>,
}

impl DfmPreparedLayout {
    pub fn items(&self) -> &[PreparedDanmakuItem] {
        &self.items
    }

    pub fn stats(&self) -> DanmakuPreparedStats {
        self.stats
    }

    pub(crate) fn apply_paint_config(&mut self, config: &DanmakuLayoutConfig) {
        let config = config.sanitized();
        debug_assert!(self.config.layout_equivalent(&config));
        self.config.opacity = config.opacity;
        self.config.shadow_offset = config.shadow_offset;
        self.config.shadow_style = config.shadow_style;
        self.config.enabled = config.enabled;
    }

    pub fn frame_layout(&self, media_time: Duration, generation: u64) -> DanmakuFrameLayout {
        if !self.config.enabled {
            return DanmakuFrameLayout::empty(media_time, generation, self.viewport);
        }
        let current = media_time.as_secs_f64();
        let mut items = Vec::with_capacity(dfm::frame_candidate_count(&self.dfm_layout, current));
        dfm::for_each_frame_item(&self.dfm_layout, current, |frame_item| {
            let Some(item) = self.items.get(frame_item.item_index) else {
                return;
            };
            if item.mode == DanmakuMode::Special {
                return;
            }
            items.push(DanmakuPlacedItem {
                item_id: item.id,
                text: Arc::clone(&item.text),
                mode: item.mode,
                track_index: frame_item.track_index.max(0) as usize,
                x: frame_item.x as f32,
                y: frame_item.y as f32,
                width: item.width,
                height: item.height,
                font_size: item.font_size,
                color: item.color,
                opacity: item.opacity * self.config.opacity,
                outline_width: resolve_width_px(item.font_size, self.config.outline_width),
                shadow_offset: scale_offset(self.config.shadow_offset, self.viewport.scale_factor),
                shadow_alpha: shadow_style_alpha(self.config.shadow_style),
                duplicate_count: item.duplicate_count,
                text_layout: Arc::clone(&item.text_layout),
            });
        });
        DanmakuFrameLayout {
            media_time,
            generation,
            viewport: self.viewport,
            prepared_stats: self.stats,
            items,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DanmakuFrameLayout {
    pub media_time: Duration,
    pub generation: u64,
    pub viewport: DanmakuViewport,
    pub prepared_stats: DanmakuPreparedStats,
    pub items: Vec<DanmakuPlacedItem>,
}

impl DanmakuFrameLayout {
    pub fn empty(media_time: Duration, generation: u64, viewport: DanmakuViewport) -> Self {
        Self {
            media_time,
            generation,
            viewport,
            prepared_stats: DanmakuPreparedStats::default(),
            items: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DanmakuPlacedItem {
    pub item_id: u64,
    pub text: Arc<str>,
    pub mode: DanmakuMode,
    pub track_index: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub font_size: f32,
    pub color: DanmakuColor,
    pub opacity: f32,
    pub outline_width: f32,
    pub shadow_offset: [f32; 2],
    pub shadow_alpha: f32,
    pub duplicate_count: u32,
    text_layout: Arc<PreparedTextLayout>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DanmakuRenderPlan {
    pub media_time: Duration,
    pub generation: u64,
    pub viewport: DanmakuViewport,
    pub atlas: Option<Arc<DanmakuGlyphAtlas>>,
    pub items: Vec<DanmakuGlyphInstance>,
    pub frame_stats: DanmakuFrameStats,
}

impl DanmakuRenderPlan {
    pub fn empty(media_time: Duration, generation: u64, viewport: DanmakuViewport) -> Self {
        Self {
            media_time,
            generation,
            viewport,
            atlas: None,
            items: Vec::new(),
            frame_stats: DanmakuFrameStats::default(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DanmakuFrameStats {
    pub prepared: DanmakuPreparedStats,
    pub placed_items: usize,
    pub scroll_items: usize,
    pub top_items: usize,
    pub bottom_items: usize,
    pub scroll_rows: usize,
    pub scroll_track_min: usize,
    pub scroll_track_max: usize,
    pub scroll_min_y: f32,
    pub scroll_max_y: f32,
    pub scroll_bucket_count: usize,
    pub scroll_buckets: [DanmakuDebugBucket; DANMAKU_DEBUG_BUCKETS],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DanmakuDebugBucket {
    pub key: i32,
    pub count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DanmakuPreparedStats {
    pub source_items: usize,
    pub supported_items: usize,
    pub prepared_items: usize,
    pub filtered_items: usize,
    pub prepared_scroll_items: usize,
    pub prepared_top_items: usize,
    pub prepared_bottom_items: usize,
    pub prepared_scroll_rows: usize,
    pub prepared_scroll_min_y: f32,
    pub prepared_scroll_max_y: f32,
    pub expected_scroll_tracks: usize,
    pub dfm_track_count: usize,
    pub display_area_height: f32,
    pub scroll_area_height: f32,
    pub track_height: f32,
    pub scroll_bucket_count: usize,
    pub scroll_buckets: [DanmakuDebugBucket; DANMAKU_DEBUG_BUCKETS],
}

impl DanmakuFrameStats {
    fn from_layout(layout: &DanmakuFrameLayout) -> Self {
        let mut stats = Self::default();
        stats.prepared = layout.prepared_stats;
        let mut scroll_rows = BTreeSet::new();
        let mut scroll_track_min = usize::MAX;
        let mut scroll_track_max = 0usize;
        let mut bucket_counts = BTreeMap::<i32, u32>::new();
        let mut scroll_min_y = f32::MAX;
        let mut scroll_max_y = f32::MIN;
        for item in &layout.items {
            stats.placed_items += 1;
            match item.mode {
                DanmakuMode::Scroll | DanmakuMode::ScrollReverse => {
                    stats.scroll_items += 1;
                    let row_key = item.y.round() as i32;
                    scroll_rows.insert(row_key);
                    *bucket_counts.entry(row_key).or_default() += 1;
                    scroll_track_min = scroll_track_min.min(item.track_index);
                    scroll_track_max = scroll_track_max.max(item.track_index);
                    scroll_min_y = scroll_min_y.min(item.y);
                    scroll_max_y = scroll_max_y.max(item.y + item.height);
                }
                DanmakuMode::Top => stats.top_items += 1,
                DanmakuMode::Bottom => stats.bottom_items += 1,
                DanmakuMode::Special => {}
            }
        }
        stats.scroll_rows = scroll_rows.len();
        if stats.scroll_items > 0 {
            stats.scroll_track_min = scroll_track_min;
            stats.scroll_track_max = scroll_track_max;
            stats.scroll_min_y = scroll_min_y;
            stats.scroll_max_y = scroll_max_y;
        }
        stats.scroll_bucket_count = fill_debug_buckets(&bucket_counts, &mut stats.scroll_buckets);
        stats
    }
}

impl DanmakuPreparedStats {
    fn from_items(
        source_items: usize,
        supported_items: usize,
        prepared: &[PreparedDanmakuItem],
        viewport: DanmakuViewport,
        config: &DanmakuLayoutConfig,
        dfm_track_count: usize,
    ) -> Self {
        let mut stats = Self {
            source_items,
            supported_items,
            prepared_items: prepared.len(),
            filtered_items: supported_items.saturating_sub(prepared.len()),
            dfm_track_count,
            display_area_height: viewport.height as f32 * config.display_area.clamp(0.1, 1.0),
            scroll_area_height: viewport.height as f32 * config.display_area.clamp(0.1, 1.0),
            ..Self::default()
        };
        let mut scroll_rows = BTreeSet::new();
        let mut bucket_counts = BTreeMap::<i32, u32>::new();
        let mut scroll_min_y = f32::MAX;
        let mut scroll_max_y = f32::MIN;
        let mut first_scroll_height = 0.0f32;
        for item in prepared {
            match item.mode {
                DanmakuMode::Scroll | DanmakuMode::ScrollReverse => {
                    stats.prepared_scroll_items += 1;
                    let row_key = item.y.round() as i32;
                    scroll_rows.insert(row_key);
                    *bucket_counts.entry(row_key).or_default() += 1;
                    scroll_min_y = scroll_min_y.min(item.y);
                    scroll_max_y = scroll_max_y.max(item.y + item.height);
                    if first_scroll_height <= 0.0 {
                        first_scroll_height = item.height;
                    }
                }
                DanmakuMode::Top => stats.prepared_top_items += 1,
                DanmakuMode::Bottom => stats.prepared_bottom_items += 1,
                DanmakuMode::Special => {}
            }
        }
        stats.prepared_scroll_rows = scroll_rows.len();
        if stats.prepared_scroll_items > 0 {
            stats.prepared_scroll_min_y = scroll_min_y;
            stats.prepared_scroll_max_y = scroll_max_y;
        }
        let track_height =
            (first_scroll_height + first_scroll_height * config.track_gap_ratio).max(1.0);
        stats.track_height = track_height;
        stats.expected_scroll_tracks =
            (stats.scroll_area_height / track_height).floor().max(1.0) as usize;
        stats.scroll_bucket_count = fill_debug_buckets(&bucket_counts, &mut stats.scroll_buckets);
        stats
    }
}

fn fill_debug_buckets(
    source: &BTreeMap<i32, u32>,
    target: &mut [DanmakuDebugBucket; DANMAKU_DEBUG_BUCKETS],
) -> usize {
    for bucket in target.iter_mut() {
        *bucket = DanmakuDebugBucket::default();
    }
    for (slot, (&key, &count)) in target.iter_mut().zip(source.iter()) {
        *slot = DanmakuDebugBucket { key, count };
    }
    source.len()
}

#[derive(Debug, Clone, PartialEq)]
pub struct DanmakuGlyphInstance {
    pub item_id: u64,
    pub rect: [f32; 4],
    pub tex_rect: [f32; 4],
    pub color_rgba: [f32; 4],
    pub outline_rgba: [f32; 4],
    pub shadow_rgba: [f32; 4],
    pub shadow_offset: [f32; 2],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DanmakuAtlasUpdate {
    pub from_version: u64,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DanmakuGlyphAtlas {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub fill_alpha: Vec<u8>,
    pub outline_alpha: Vec<u8>,
    pub version: u64,
    pub update: Option<DanmakuAtlasUpdate>,
}

impl DanmakuGlyphAtlas {
    pub fn is_valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.stride >= self.width as usize
            && self.fill_alpha.len() >= self.required_len()
            && self.outline_alpha.len() >= self.required_len()
    }

    pub fn required_len(&self) -> usize {
        self.stride.saturating_mul(self.height as usize)
    }

    pub fn incremental_update_from(
        &self,
        version: u64,
        width: u32,
        height: u32,
        stride: usize,
    ) -> Option<&DanmakuAtlasUpdate> {
        let update = self.update.as_ref()?;
        (update.from_version == version
            && self.width == width
            && self.height == height
            && self.stride == stride
            && update.width > 0
            && update.height > 0
            && update.x.saturating_add(update.width) <= self.width
            && update.y.saturating_add(update.height) <= self.height)
            .then_some(update)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct PreparedTextLayout {
    metrics: TextMeasure,
    glyphs: Vec<PreparedGlyphPlacement>,
}

#[derive(Debug, Clone, PartialEq)]
struct PreparedGlyphPlacement {
    glyph: Arc<RasterizedGlyph>,
    packed: PackedGlyph,
    pen_x: f32,
}

#[derive(Debug, Clone)]
struct DanmakuFontFace {
    id: u32,
    font: Arc<FontArc>,
}

impl DanmakuFontFace {
    fn new(id: u32, font: FontArc) -> Self {
        Self {
            id,
            font: Arc::new(font),
        }
    }

    fn glyph_id(&self, ch: char) -> GlyphId {
        self.font.glyph_id(ch)
    }

    fn has_glyph(&self, ch: char) -> bool {
        ch.is_whitespace() || self.glyph_id(ch).0 != 0
    }
}

#[derive(Debug)]
struct SystemFontFallback {
    db: fontdb::Database,
    faces: Vec<fontdb::ID>,
    loaded: HashMap<fontdb::ID, Option<DanmakuFontFace>>,
    char_cache: HashMap<char, Option<u32>>,
    next_font_id: u32,
}

impl SystemFontFallback {
    fn new(next_font_id: u32) -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let preferred = [
            "PingFang SC",
            "Hiragino Sans GB",
            "Hiragino Sans",
            "Apple SD Gothic Neo",
            "Songti SC",
            "STHeiti",
            "Microsoft YaHei",
            "SimHei",
            "Noto Sans CJK SC",
            "Noto Sans CJK JP",
            "Noto Sans CJK",
            "Source Han Sans SC",
            "Source Han Sans",
            "Arial Unicode MS",
            "Arial Unicode",
            "Segoe UI Symbol",
        ];
        let mut faces = Vec::new();
        for family in preferred {
            let query = fontdb::Query {
                families: &[fontdb::Family::Name(family)],
                weight: fontdb::Weight::NORMAL,
                stretch: fontdb::Stretch::Normal,
                style: fontdb::Style::Normal,
            };
            if let Some(id) = db.query(&query) {
                push_unique_face(&mut faces, id);
            }
        }
        for family in [fontdb::Family::SansSerif, fontdb::Family::Serif] {
            let query = fontdb::Query {
                families: &[family],
                weight: fontdb::Weight::NORMAL,
                stretch: fontdb::Stretch::Normal,
                style: fontdb::Style::Normal,
            };
            if let Some(id) = db.query(&query) {
                push_unique_face(&mut faces, id);
            }
        }
        let face_infos = db.faces().map(|face| face.id).collect::<Vec<_>>();
        for id in face_infos {
            push_unique_face(&mut faces, id);
        }
        Self {
            db,
            faces,
            loaded: HashMap::new(),
            char_cache: HashMap::new(),
            next_font_id,
        }
    }

    fn resolve(&mut self, ch: char) -> Option<DanmakuFontFace> {
        if let Some(font_id) = self.char_cache.get(&ch).copied() {
            return font_id.and_then(|font_id| self.loaded_font_by_id(font_id));
        }
        let faces = self.faces.clone();
        for face_id in faces {
            let Some(face) = self.load_face(face_id) else {
                continue;
            };
            if face.has_glyph(ch) {
                self.char_cache.insert(ch, Some(face.id));
                return Some(face);
            }
        }
        self.char_cache.insert(ch, None);
        None
    }

    fn loaded_font_by_id(&self, font_id: u32) -> Option<DanmakuFontFace> {
        self.loaded
            .values()
            .filter_map(|face| face.as_ref())
            .find(|face| face.id == font_id)
            .cloned()
    }

    fn load_face(&mut self, id: fontdb::ID) -> Option<DanmakuFontFace> {
        if let Some(face) = self.loaded.get(&id) {
            return face.clone();
        }
        let font = self
            .db
            .with_face_data(id, |data, face_index| {
                FontVec::try_from_vec_and_index(data.to_vec(), face_index)
                    .map(FontArc::new)
                    .ok()
            })
            .flatten();
        let face = font.map(|font| {
            let id = self.next_font_id;
            self.next_font_id = self.next_font_id.saturating_add(1).max(1);
            DanmakuFontFace::new(id, font)
        });
        self.loaded.insert(id, face.clone());
        face
    }
}

fn push_unique_face(faces: &mut Vec<fontdb::ID>, id: fontdb::ID) {
    if !faces.contains(&id) {
        faces.push(id);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TextLayoutCacheKey {
    text: Arc<str>,
    font_size_milli: u32,
    outline_radius: u16,
}

impl TextLayoutCacheKey {
    fn new(text: Arc<str>, font_size: f32, outline_width: f32) -> Self {
        Self {
            text,
            font_size_milli: (sanitize_f32(font_size, DEFAULT_NATIVE_FONT_SIZE).max(1.0) * 1000.0)
                .round() as u32,
            outline_radius: raster_radius(outline_width),
        }
    }

    fn font_size(&self) -> f32 {
        self.font_size_milli as f32 / 1000.0
    }

    fn outline_width(&self) -> f32 {
        f32::from(self.outline_radius)
    }
}

#[derive(Debug, Clone)]
pub struct DanmakuTextRasterizer {
    shaper: TextShaper,
    primary_font: Option<DanmakuFontFace>,
    fallback_fonts: Arc<Mutex<SystemFontFallback>>,
    glyph_cache: Arc<Mutex<HashMap<GlyphCacheKey, Arc<RasterizedGlyph>>>>,
    text_layout_cache: Arc<Mutex<HashMap<TextLayoutCacheKey, Arc<PreparedTextLayout>>>>,
    glyph_atlas: Arc<Mutex<PersistentGlyphAtlas>>,
}

impl Default for DanmakuTextRasterizer {
    fn default() -> Self {
        Self::new(TextShaper::default())
    }
}

impl DanmakuTextRasterizer {
    pub fn new(shaper: TextShaper) -> Self {
        Self {
            shaper,
            primary_font: load_default_font().map(|font| DanmakuFontFace::new(0, font)),
            fallback_fonts: Arc::new(Mutex::new(SystemFontFallback::new(1))),
            glyph_cache: Arc::new(Mutex::new(HashMap::new())),
            text_layout_cache: Arc::new(Mutex::new(HashMap::new())),
            glyph_atlas: Arc::new(Mutex::new(PersistentGlyphAtlas::new())),
        }
    }

    pub fn for_config(config: &DanmakuLayoutConfig) -> Self {
        let shaper = TextShaper::default();
        Self {
            shaper,
            primary_font: load_configured_font(
                &config.custom_font_family,
                &config.custom_font_file_path,
            )
            .map(|font| DanmakuFontFace::new(0, font)),
            fallback_fonts: Arc::new(Mutex::new(SystemFontFallback::new(1))),
            glyph_cache: Arc::new(Mutex::new(HashMap::new())),
            text_layout_cache: Arc::new(Mutex::new(HashMap::new())),
            glyph_atlas: Arc::new(Mutex::new(PersistentGlyphAtlas::new())),
        }
    }

    pub fn measure(&self, text: &str, font_size: f32) -> TextMeasure {
        if self.primary_font.is_none() {
            let metrics = self.shaper.measure(text, font_size);
            return TextMeasure {
                width: metrics.width.max(1.0),
                height: metrics.height.max(1.0),
                ascent: metrics.ascent,
                descent: metrics.descent,
            };
        }
        self.measure_with_fallback(text, font_size)
    }

    fn measure_with_fallback(&self, text: &str, font_size: f32) -> TextMeasure {
        let mut width = 0.0f32;
        let mut max_ascent = 0.0f32;
        let mut max_descent = 0.0f32;
        let mut max_line_gap = 0.0f32;
        let mut previous: Option<(u32, GlyphId)> = None;
        for ch in text.chars() {
            let Some(face) = self.resolve_font(ch) else {
                let metrics = self.shaper.measure(&ch.to_string(), font_size);
                width += metrics.width.max(font_size * 0.5);
                max_ascent = max_ascent.max(metrics.ascent);
                max_descent = max_descent.max(metrics.descent);
                previous = None;
                continue;
            };
            let scale = px_scale_for_em(face.font.as_ref(), font_size);
            let scaled = face.font.as_scaled(scale);
            let glyph_id = scaled.glyph_id(ch);
            if let Some((previous_font_id, previous_glyph_id)) = previous {
                if previous_font_id == face.id {
                    width += scaled.kern(previous_glyph_id, glyph_id);
                }
            }
            width += scaled.h_advance(glyph_id).max(0.0);
            max_ascent = max_ascent.max(scaled.ascent());
            max_descent = max_descent.max(scaled.descent().abs());
            max_line_gap = max_line_gap.max(scaled.line_gap());
            previous = Some((face.id, glyph_id));
        }
        let ascent = max_ascent.max(font_size * 0.8);
        let descent = max_descent.max(font_size * 0.2);
        let height = (ascent + descent + max_line_gap).max(font_size * 1.2);
        TextMeasure {
            width: width.max(1.0),
            height,
            ascent,
            descent,
        }
    }

    fn prepare_text_layout_with_metrics(
        &self,
        text: Arc<str>,
        font_size: f32,
        outline_width: f32,
        metrics: TextMeasure,
    ) -> Arc<PreparedTextLayout> {
        let key = TextLayoutCacheKey::new(text, font_size, outline_width);
        if let Some(layout) = self
            .text_layout_cache
            .lock()
            .expect("danmaku text layout cache lock")
            .get(&key)
            .cloned()
        {
            return layout;
        }
        let layout = Arc::new(self.build_text_layout(&key, metrics));
        self.text_layout_cache
            .lock()
            .expect("danmaku text layout cache lock")
            .insert(key, Arc::clone(&layout));
        layout
    }

    fn build_text_layout(
        &self,
        key: &TextLayoutCacheKey,
        metrics: TextMeasure,
    ) -> PreparedTextLayout {
        let font_size = key.font_size();
        let outline_width = key.outline_width();
        let mut glyphs = Vec::new();
        let mut pen_x = 0.0f32;
        let mut previous: Option<(u32, GlyphId)> = None;
        for ch in key.text.chars() {
            let face = self.resolve_font(ch);
            let glyph_id = face.as_ref().map(|face| face.glyph_id(ch));
            if let (Some(face), Some((previous_font_id, previous_glyph_id)), Some(current)) =
                (face.as_ref(), previous, glyph_id)
            {
                if previous_font_id == face.id {
                    pen_x += face
                        .font
                        .as_scaled(px_scale_for_em(face.font.as_ref(), font_size))
                        .kern(previous_glyph_id, current);
                }
            }
            let glyph_key = GlyphCacheKey::new(
                face.as_ref().map(|face| face.id).unwrap_or(u32::MAX),
                ch,
                font_size,
                outline_width,
            );
            let glyph = self.cached_glyph_by_key(glyph_key);
            let packed = if glyph.has_bitmap() {
                self.pack_glyph(&glyph)
            } else {
                PackedGlyph::default()
            };
            let advance = glyph.advance;
            glyphs.push(PreparedGlyphPlacement {
                glyph,
                packed,
                pen_x,
            });
            pen_x += advance;
            previous = face
                .as_ref()
                .and_then(|face| glyph_id.map(|glyph_id| (face.id, glyph_id)));
        }
        PreparedTextLayout { metrics, glyphs }
    }

    fn pack_glyph(&self, glyph: &RasterizedGlyph) -> PackedGlyph {
        self.glyph_atlas
            .lock()
            .expect("danmaku glyph atlas lock")
            .pack(glyph)
    }

    fn resolve_font(&self, ch: char) -> Option<DanmakuFontFace> {
        if let Some(font) = &self.primary_font {
            if font.has_glyph(ch) {
                return Some(font.clone());
            }
        }
        self.fallback_fonts
            .lock()
            .expect("danmaku system font fallback lock")
            .resolve(ch)
    }

    pub fn render_plan(&self, layout: &DanmakuFrameLayout) -> DanmakuRenderPlan {
        let frame_stats = DanmakuFrameStats::from_layout(layout);
        if layout.items.is_empty() {
            return DanmakuRenderPlan::empty(layout.media_time, layout.generation, layout.viewport);
        }
        let Some(atlas_snapshot) = self
            .glyph_atlas
            .lock()
            .expect("danmaku glyph atlas lock")
            .snapshot()
        else {
            return DanmakuRenderPlan::empty(layout.media_time, layout.generation, layout.viewport);
        };
        let atlas_w = atlas_snapshot.width.max(1) as f32;
        let atlas_h = atlas_snapshot.height.max(1) as f32;
        let capacity = layout
            .items
            .iter()
            .map(|item| item.text_layout.glyphs.len())
            .sum();
        let mut items = Vec::with_capacity(capacity);
        for item in &layout.items {
            let color = item.color.rgba(item.opacity);
            if color[3] <= 0.0 {
                continue;
            }
            self.append_item_glyphs(item, color, atlas_w, atlas_h, &mut items);
        }
        DanmakuRenderPlan {
            media_time: layout.media_time,
            generation: layout.generation,
            viewport: layout.viewport,
            atlas: Some(atlas_snapshot),
            items,
            frame_stats,
        }
    }

    fn append_item_glyphs(
        &self,
        item: &DanmakuPlacedItem,
        color: [f32; 4],
        atlas_w: f32,
        atlas_h: f32,
        items: &mut Vec<DanmakuGlyphInstance>,
    ) {
        let baseline = item.y + item.text_layout.metrics.ascent;
        for placement in &item.text_layout.glyphs {
            let glyph = &placement.glyph;
            if glyph.has_bitmap() {
                let packed = placement.packed;
                items.push(DanmakuGlyphInstance {
                    item_id: item.item_id,
                    rect: [
                        item.x + placement.pen_x + glyph.offset_x,
                        baseline + glyph.offset_y,
                        glyph.width as f32,
                        glyph.height as f32,
                    ],
                    tex_rect: [
                        packed.x as f32 / atlas_w,
                        packed.y as f32 / atlas_h,
                        packed.width as f32 / atlas_w,
                        packed.height as f32 / atlas_h,
                    ],
                    color_rgba: color,
                    // Keep the outline as opaque as the fill. Capping it at
                    // 0.75 made an otherwise opaque danmaku look washed out.
                    outline_rgba: [0.0, 0.0, 0.0, color[3]],
                    shadow_rgba: [0.0, 0.0, 0.0, (color[3] * item.shadow_alpha).min(1.0)],
                    shadow_offset: item.shadow_offset,
                });
            }
        }
    }

    fn cached_glyphs<I>(&self, keys: I) -> Vec<Arc<RasterizedGlyph>>
    where
        I: IntoIterator<Item = GlyphCacheKey>,
    {
        let keys = keys.into_iter().collect::<Vec<_>>();
        if keys.is_empty() {
            return Vec::new();
        }
        let mut results = Vec::with_capacity(keys.len());
        let mut missing = Vec::new();
        {
            let cache = self.glyph_cache.lock().expect("danmaku glyph cache lock");
            for key in keys {
                if let Some(glyph) = cache.get(&key).cloned() {
                    results.push(glyph);
                } else {
                    results.push(Arc::new(RasterizedGlyph::placeholder(key)));
                    missing.push((results.len() - 1, key));
                }
            }
        }
        if missing.is_empty() {
            return results;
        }
        for (index, key) in missing {
            let glyph = Arc::new(self.rasterize_glyph(key));
            let cached = self
                .glyph_cache
                .lock()
                .expect("danmaku glyph cache lock")
                .entry(key)
                .or_insert_with(|| Arc::clone(&glyph))
                .clone();
            results[index] = cached;
        }
        results
    }

    fn cached_glyph_by_key(&self, key: GlyphCacheKey) -> Arc<RasterizedGlyph> {
        self.cached_glyphs([key])
            .into_iter()
            .next()
            .expect("one glyph returned")
    }

    fn rasterize_glyph(&self, key: GlyphCacheKey) -> RasterizedGlyph {
        if let Some(font) = self.resolve_font(key.ch) {
            if font.id == key.font_id {
                return rasterize_font_glyph(&font.font, key);
            }
        }
        rasterize_fallback_glyph(&self.shaper, key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphCacheKey {
    font_id: u32,
    ch: char,
    font_size_milli: u32,
    outline_radius: u16,
}

impl GlyphCacheKey {
    fn new(font_id: u32, ch: char, font_size: f32, outline_width: f32) -> Self {
        Self {
            font_id,
            ch,
            font_size_milli: (sanitize_f32(font_size, DEFAULT_NATIVE_FONT_SIZE).max(1.0) * 1000.0)
                .round() as u32,
            outline_radius: raster_radius(outline_width),
        }
    }

    fn font_size(self) -> f32 {
        self.font_size_milli as f32 / 1000.0
    }
}

#[derive(Debug, Clone, PartialEq)]
struct RasterizedGlyph {
    key: GlyphCacheKey,
    width: u32,
    height: u32,
    stride: usize,
    fill_alpha: Vec<u8>,
    outline_alpha: Vec<u8>,
    offset_x: f32,
    offset_y: f32,
    advance: f32,
}

impl RasterizedGlyph {
    fn placeholder(key: GlyphCacheKey) -> Self {
        Self::empty(key, 0.0)
    }

    fn empty(key: GlyphCacheKey, advance: f32) -> Self {
        Self {
            key,
            width: 0,
            height: 0,
            stride: 0,
            fill_alpha: Vec::new(),
            outline_alpha: Vec::new(),
            offset_x: 0.0,
            offset_y: 0.0,
            advance,
        }
    }

    fn has_bitmap(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.stride >= self.width as usize
            && self.fill_alpha.len() >= self.required_len()
            && self.outline_alpha.len() >= self.required_len()
    }

    fn required_len(&self) -> usize {
        self.stride.saturating_mul(self.height as usize)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct PackedGlyph {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Debug)]
struct PersistentGlyphAtlas {
    width: u32,
    height: u32,
    stride: usize,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    fill_alpha: Vec<u8>,
    outline_alpha: Vec<u8>,
    packed: HashMap<GlyphCacheKey, PackedGlyph>,
    version: u64,
    dirty: bool,
    dirty_bounds: Option<[u32; 4]>,
    snapshot: Option<Arc<DanmakuGlyphAtlas>>,
}

impl PersistentGlyphAtlas {
    fn new() -> Self {
        let width = GLYPH_ATLAS_WIDTH;
        let height = GLYPH_ATLAS_INITIAL_HEIGHT;
        let stride = width as usize;
        Self {
            width,
            height,
            stride,
            cursor_x: 0,
            cursor_y: 0,
            row_height: 0,
            fill_alpha: vec![0; stride * height as usize],
            outline_alpha: vec![0; stride * height as usize],
            packed: HashMap::new(),
            version: 1,
            dirty: true,
            dirty_bounds: None,
            snapshot: None,
        }
    }

    fn pack(&mut self, glyph: &RasterizedGlyph) -> PackedGlyph {
        if let Some(packed) = self.packed.get(&glyph.key).copied() {
            return packed;
        }
        let glyph_width = glyph.width.min(self.width);
        let glyph_height = glyph.height;
        if self.cursor_x + glyph_width > self.width {
            self.cursor_x = 0;
            self.cursor_y += self.row_height;
            self.row_height = 0;
        }
        self.ensure_height(self.cursor_y + glyph_height);
        let packed = PackedGlyph {
            x: self.cursor_x,
            y: self.cursor_y,
            width: glyph_width,
            height: glyph_height,
        };
        for row in 0..glyph_height as usize {
            let src = row * glyph.stride;
            let dst = (packed.y as usize + row) * self.stride + packed.x as usize;
            let len = glyph_width as usize;
            self.fill_alpha[dst..dst + len].copy_from_slice(&glyph.fill_alpha[src..src + len]);
            self.outline_alpha[dst..dst + len]
                .copy_from_slice(&glyph.outline_alpha[src..src + len]);
        }
        self.cursor_x += glyph_width;
        self.row_height = self.row_height.max(glyph_height);
        self.packed.insert(glyph.key, packed);
        self.version = self.version.saturating_add(1).max(1);
        self.dirty = true;
        if packed.width > 0 && packed.height > 0 {
            let right = packed.x.saturating_add(packed.width);
            let bottom = packed.y.saturating_add(packed.height);
            self.dirty_bounds = Some(match self.dirty_bounds {
                Some([left, top, old_right, old_bottom]) => [
                    left.min(packed.x),
                    top.min(packed.y),
                    old_right.max(right),
                    old_bottom.max(bottom),
                ],
                None => [packed.x, packed.y, right, bottom],
            });
        }
        packed
    }

    fn ensure_height(&mut self, required: u32) {
        if required <= self.height {
            return;
        }
        while required > self.height {
            self.height *= 2;
        }
        let new_len = self.stride * self.height as usize;
        self.fill_alpha.resize(new_len, 0);
        self.outline_alpha.resize(new_len, 0);
        self.dirty = true;
    }

    fn snapshot(&mut self) -> Option<Arc<DanmakuGlyphAtlas>> {
        if self.packed.is_empty() {
            return None;
        }
        if self.dirty || self.snapshot.is_none() {
            let update = self.snapshot.as_ref().and_then(|snapshot| {
                self.dirty_bounds
                    .map(|[left, top, right, bottom]| DanmakuAtlasUpdate {
                        from_version: snapshot.version,
                        x: left,
                        y: top,
                        width: right.saturating_sub(left),
                        height: bottom.saturating_sub(top),
                    })
            });
            self.snapshot = Some(Arc::new(DanmakuGlyphAtlas {
                width: self.width,
                height: self.height,
                stride: self.stride,
                fill_alpha: self.fill_alpha.clone(),
                outline_alpha: self.outline_alpha.clone(),
                version: self.version,
                update,
            }));
            self.dirty = false;
            self.dirty_bounds = None;
        }
        self.snapshot.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextMeasure {
    pub width: f32,
    pub height: f32,
    pub ascent: f32,
    pub descent: f32,
}

#[derive(Debug, Clone)]
pub struct DfmLayoutEngine {
    timeline: DanmakuTimeline,
    config: DanmakuLayoutConfig,
    rasterizer: DanmakuTextRasterizer,
    prepared: Option<DfmPreparedLayout>,
    stable_tracks: HashMap<u64, usize>,
    stable_viewport: Option<DanmakuViewport>,
}

impl DfmLayoutEngine {
    pub fn new(timeline: DanmakuTimeline, config: DanmakuLayoutConfig) -> Self {
        let config = config.sanitized();
        let rasterizer = DanmakuTextRasterizer::for_config(&config);
        Self {
            timeline,
            config,
            rasterizer,
            prepared: None,
            stable_tracks: HashMap::new(),
            stable_viewport: None,
        }
    }

    pub fn set_timeline(&mut self, timeline: DanmakuTimeline) {
        self.timeline = timeline;
        self.prepared = None;
        self.invalidate_stable_tracks();
    }

    pub fn sync_timeline(&mut self, timeline: &DanmakuTimeline) {
        if self.timeline != *timeline {
            self.timeline = timeline.clone();
            self.prepared = None;
        }
    }

    pub fn clear_timeline(&mut self) {
        self.timeline = DanmakuTimeline::default();
        self.prepared = None;
        self.invalidate_stable_tracks();
    }

    pub fn set_config(&mut self, config: DanmakuLayoutConfig) -> bool {
        self.apply_config(config) != DanmakuConfigChange::Unchanged
    }

    pub(crate) fn apply_config(&mut self, config: DanmakuLayoutConfig) -> DanmakuConfigChange {
        let config = config.sanitized();
        if self.config == config {
            return DanmakuConfigChange::Unchanged;
        }
        let layout_changed = !self.config.layout_equivalent(&config);
        let font_changed = self.config.custom_font_family != config.custom_font_family
            || self.config.custom_font_file_path != config.custom_font_file_path;
        self.config = config;
        if font_changed {
            self.rasterizer = DanmakuTextRasterizer::for_config(&self.config);
        }
        if layout_changed {
            self.prepared = None;
            self.invalidate_stable_tracks();
            DanmakuConfigChange::Layout
        } else {
            if let Some(prepared) = &mut self.prepared {
                prepared.apply_paint_config(&self.config);
            }
            DanmakuConfigChange::PaintOnly
        }
    }

    pub fn config(&self) -> &DanmakuLayoutConfig {
        &self.config
    }

    pub(crate) fn rasterizer_clone(&self) -> DanmakuTextRasterizer {
        self.rasterizer.clone()
    }

    pub(crate) fn set_config_with_rasterizer(
        &mut self,
        config: DanmakuLayoutConfig,
        rasterizer: DanmakuTextRasterizer,
    ) {
        if self.apply_config(config) == DanmakuConfigChange::Layout {
            // The async planner must pack glyphs into the same atlas that the
            // presenter later snapshots. This is especially important after a
            // custom-font change creates a fresh rasterizer generation.
            self.rasterizer = rasterizer;
        }
    }

    pub fn prepare(&mut self, viewport: DanmakuViewport, _generation: u64) -> DfmPreparedLayout {
        if self.stable_viewport != Some(viewport) {
            self.invalidate_stable_tracks();
            self.stable_viewport = Some(viewport);
        }
        let config = self.config.sanitized();
        let prepared = prepare_layout(
            &self.timeline,
            viewport,
            &config,
            &self.rasterizer,
            &self.stable_tracks,
        );
        let active_ids = prepared
            .items()
            .iter()
            .map(|item| item.id)
            .collect::<BTreeSet<_>>();
        self.stable_tracks
            .retain(|item_id, _| active_ids.contains(item_id));
        for item in prepared.items() {
            self.stable_tracks.insert(item.id, item.track_index);
        }
        self.prepared = Some(prepared.clone());
        prepared
    }

    pub fn invalidate_stable_tracks(&mut self) {
        self.stable_tracks.clear();
        self.stable_viewport = None;
    }

    pub fn frame_layout(
        &mut self,
        media_time: Duration,
        viewport: DanmakuViewport,
        generation: u64,
    ) -> DanmakuFrameLayout {
        let generation = generation.max(DEFAULT_GENERATION);
        let needs_prepare = self.prepared.as_ref().is_none_or(|prepared| {
            prepared.viewport != viewport || prepared.config != self.config.sanitized()
        });
        if needs_prepare {
            self.prepare(viewport, generation);
        }
        self.prepared
            .as_ref()
            .expect("prepared layout exists")
            .frame_layout(media_time, generation)
    }

    pub fn render_prepared_plan(
        &self,
        prepared: &DfmPreparedLayout,
        media_time: Duration,
        generation: u64,
    ) -> DanmakuRenderPlan {
        let layout = prepared.frame_layout(media_time, generation);
        self.rasterizer.render_plan(&layout)
    }

    pub fn render_plan(
        &mut self,
        media_time: Duration,
        viewport: DanmakuViewport,
        generation: u64,
    ) -> DanmakuRenderPlan {
        let layout = self.frame_layout(media_time, viewport, generation);
        self.rasterizer.render_plan(&layout)
    }
}

fn prepare_layout(
    timeline: &DanmakuTimeline,
    viewport: DanmakuViewport,
    config: &DanmakuLayoutConfig,
    rasterizer: &DanmakuTextRasterizer,
    stable_tracks: &HashMap<u64, usize>,
) -> DfmPreparedLayout {
    if timeline.is_empty() || !config.enabled {
        let dfm_layout = dfm::PreparedLayout {
            width: viewport.width as f64,
            height: viewport.height as f64,
            scroll_duration_seconds: config.scroll_duration_seconds.max(1.0) as f64,
            static_duration_seconds: DEFAULT_STATIC_DURATION.as_secs_f64(),
            items: Vec::new(),
            item_times: Vec::new(),
            track_count: 0,
        };
        return DfmPreparedLayout {
            viewport,
            config: config.clone(),
            dfm_layout,
            stats: DanmakuPreparedStats::default(),
            scroll_duration: DEFAULT_SCROLL_DURATION,
            static_duration: DEFAULT_STATIC_DURATION,
            item_times: Vec::new(),
            items: Vec::new(),
        };
    }
    let scroll_duration = scroll_duration_for_viewport(config, viewport);
    let base_font_size = effective_config_font_size(config.font_size, viewport.scale_factor);
    let source_count = timeline.items().len();
    let mut source_items = Vec::new();
    for source in timeline.items() {
        if source.mode == DanmakuMode::Special {
            continue;
        }
        let font_size =
            effective_font_size(source.font_size, config.font_size, viewport.scale_factor);
        let measure = rasterizer.measure(&source.text, font_size);
        source_items.push(dfm::PrepareItem {
            source_id: source.id,
            time_seconds: source.pts.as_secs_f64(),
            text: source.text.clone(),
            type_code: mode_to_dfm_type_code(source.mode),
            color_argb: color_to_argb(source.color, source.opacity),
            opacity: source.opacity,
            is_me: source.is_self,
            font_size,
            paint_width: measure.width as f64,
            paint_height: measure.height.max(font_size * 1.2) as f64,
        });
    }
    let supported_count = source_items.len();

    let dfm_layout = dfm::prepare_layout(dfm::PrepareRequest {
        items: source_items,
        width: viewport.width as f64,
        height: viewport.height as f64,
        font_size: base_font_size as f64,
        display_area: config.display_area as f64,
        scroll_duration_seconds: scroll_duration.as_secs_f64(),
        allow_stacking: config.allow_stacking,
        allow_scroll_overwrite: config.allow_scroll_overwrite,
        merge_danmaku: config.merge_duplicates,
        max_quantity: config.max_quantity,
        max_lines_per_type: config.max_lines_per_mode,
        track_gap_ratio: config.track_gap_ratio as f64,
        outline_width: config.outline_width as f64,
        preferred_tracks: stable_tracks,
        block_words: config.block_words.clone(),
        block_top: config.block_top,
        block_bottom: config.block_bottom,
        block_scroll: config.block_scroll,
    })
    .unwrap_or_else(|_| dfm::PreparedLayout {
        width: viewport.width as f64,
        height: viewport.height as f64,
        scroll_duration_seconds: scroll_duration.as_secs_f64(),
        static_duration_seconds: DEFAULT_STATIC_DURATION.as_secs_f64(),
        items: Vec::new(),
        item_times: Vec::new(),
        track_count: 0,
    });

    let prepared = dfm_layout
        .items
        .iter()
        .map(|item| {
            let text: Arc<str> = Arc::from(item.text.as_str());
            let font_size = item.font_size as f32;
            let metrics = rasterizer.measure(&item.text, font_size);
            let outline_width = resolve_width_px(font_size, config.outline_width);
            let text_layout = rasterizer.prepare_text_layout_with_metrics(
                Arc::clone(&text),
                font_size,
                outline_width,
                metrics,
            );
            PreparedDanmakuItem {
                id: item.source_id,
                time: Duration::from_secs_f64(item.time_seconds.max(0.0)),
                text,
                mode: dfm_type_code_to_mode(item.type_code),
                color: DanmakuColor::from_argb(item.color_argb as u32),
                opacity: item.opacity,
                font_size,
                width: item.width as f32,
                height: item.height as f32,
                y: item.y_position as f32,
                track_index: item.track_index.max(0) as usize,
                scroll_speed: item.scroll_speed as f32,
                duration: Duration::from_secs_f64(item.duration_seconds.max(0.0)),
                duplicate_count: item.duplicate_count,
                text_layout,
            }
        })
        .collect::<Vec<_>>();

    let item_times = prepared
        .iter()
        .map(|item| item.time.as_secs_f64())
        .collect();
    let stats = DanmakuPreparedStats::from_items(
        source_count,
        supported_count,
        &prepared,
        viewport,
        config,
        dfm_layout.track_count.max(0) as usize,
    );
    DfmPreparedLayout {
        viewport,
        config: config.clone(),
        dfm_layout,
        stats,
        scroll_duration,
        static_duration: DEFAULT_STATIC_DURATION,
        item_times,
        items: prepared,
    }
}

pub fn parse_bilibili_xml(input: &str) -> Result<Vec<DanmakuItem>> {
    let mut items = Vec::new();
    for (index, segment) in input.split("<d ").skip(1).enumerate() {
        let Some(p_start) = segment.find("p=\"") else {
            continue;
        };
        let p_rest = &segment[p_start + 3..];
        let Some(p_end) = p_rest.find('"') else {
            continue;
        };
        let fields = p_rest[..p_end].split(',').collect::<Vec<_>>();
        let Some(text_start) = segment.find('>') else {
            continue;
        };
        let Some(text_end) = segment[text_start + 1..].find("</d>") else {
            continue;
        };
        let text = decode_xml_entities(&segment[text_start + 1..text_start + 1 + text_end]);
        if text.trim().is_empty() {
            continue;
        }
        let mode_code = fields
            .get(1)
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(1);
        let mode = DanmakuMode::from_bilibili_mode(mode_code);
        if mode == DanmakuMode::Special {
            continue;
        }
        let Ok(pts) = parse_float_field(fields.first(), "pts") else {
            continue;
        };
        items.push(DanmakuItem {
            id: fields
                .get(7)
                .and_then(|value| value.parse().ok())
                .unwrap_or(index as u64 + 1),
            pts: Duration::from_secs_f64(pts.max(0.0)),
            mode,
            font_size: fields
                .get(2)
                .and_then(|value| value.parse().ok())
                .unwrap_or(DEFAULT_SOURCE_FONT_SIZE),
            color: DanmakuColor::from_decimal(
                fields
                    .get(3)
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0xffffff),
            ),
            opacity: 1.0,
            is_self: false,
            text,
        });
    }
    Ok(items)
}

fn should_skip_item_error(error: &DanmakuError) -> bool {
    matches!(
        error,
        DanmakuError::MissingText | DanmakuError::InvalidField(_)
    )
}

impl DanmakuColor {
    fn from_decimal(value: u32) -> Self {
        Self::rgb_u8(
            ((value >> 16) & 0xff) as u8,
            ((value >> 8) & 0xff) as u8,
            (value & 0xff) as u8,
        )
    }

    fn from_argb(value: u32) -> Self {
        Self {
            red: ((value >> 16) & 0xff) as f32 / 255.0,
            green: ((value >> 8) & 0xff) as f32 / 255.0,
            blue: (value & 0xff) as f32 / 255.0,
            alpha: ((value >> 24) & 0xff) as f32 / 255.0,
        }
    }
}

fn mode_to_dfm_type_code(mode: DanmakuMode) -> i32 {
    match mode {
        DanmakuMode::Scroll => 1,
        DanmakuMode::ScrollReverse => 6,
        DanmakuMode::Top => 5,
        DanmakuMode::Bottom => 4,
        DanmakuMode::Special => 7,
    }
}

fn dfm_type_code_to_mode(type_code: i32) -> DanmakuMode {
    DanmakuMode::from_bilibili_mode(type_code.max(0) as u32)
}

fn color_to_argb(color: DanmakuColor, opacity: f32) -> i32 {
    let alpha = (color.alpha * opacity)
        .clamp(0.0, 1.0)
        .mul_add(255.0, 0.0)
        .round() as u32;
    let red = (color.red.clamp(0.0, 1.0) * 255.0).round() as u32;
    let green = (color.green.clamp(0.0, 1.0) * 255.0).round() as u32;
    let blue = (color.blue.clamp(0.0, 1.0) * 255.0).round() as u32;
    ((alpha << 24) | (red << 16) | (green << 8) | blue) as i32
}

fn item_from_json_value(value: &Value, fallback_id: u64) -> Result<DanmakuItem> {
    let object = value
        .as_object()
        .ok_or_else(|| DanmakuError::InvalidField("comment".to_string()))?;
    let text = string_field(object, &["content", "text", "c"]).ok_or(DanmakuError::MissingText)?;
    let mode = string_field(object, &["type", "mode", "y"])
        .map(|value| DanmakuMode::from_text(&value))
        .or_else(|| {
            numeric_field(object, &["type_code", "mode_code"])
                .map(|v| DanmakuMode::from_bilibili_mode(v as u32))
        })
        .unwrap_or(DanmakuMode::Scroll);
    let color = object
        .get("color")
        .or_else(|| object.get("r"))
        .and_then(parse_color_value)
        .unwrap_or(DanmakuColor::WHITE);
    Ok(DanmakuItem {
        id: numeric_field(object, &["id"])
            .map(|value| value as u64)
            .unwrap_or(fallback_id),
        pts: Duration::from_secs_f64(
            numeric_field(object, &["time", "t"])
                .unwrap_or(0.0)
                .max(0.0),
        ),
        text,
        mode,
        font_size: numeric_field(object, &["font_size", "fontSize", "size", "s"])
            .unwrap_or(DEFAULT_SOURCE_FONT_SIZE as f64) as f32,
        color,
        opacity: numeric_field(object, &["opacity", "alpha", "a"]).unwrap_or(1.0) as f32,
        is_self: bool_field(object, &["is_me", "isMe", "self", "mine"]).unwrap_or(false),
    })
}

fn string_field(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| object.get(*key))
        .and_then(|value| match value {
            Value::String(value) => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
}

fn numeric_field(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| object.get(*key))
        .and_then(|value| match value {
            Value::Number(value) => value.as_f64(),
            Value::String(value) => value.parse().ok(),
            _ => None,
        })
}

fn bool_field(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| object.get(*key))
        .and_then(|value| match value {
            Value::Bool(value) => Some(*value),
            Value::Number(value) => Some(value.as_u64().unwrap_or(0) != 0),
            Value::String(value) => matches!(value.as_str(), "true" | "1" | "yes").then_some(true),
            _ => None,
        })
}

fn parse_color_value(value: &Value) -> Option<DanmakuColor> {
    match value {
        Value::Number(value) => value
            .as_u64()
            .map(|value| DanmakuColor::from_decimal(value as u32)),
        Value::String(value) => parse_color_string(value),
        _ => None,
    }
}

fn parse_color_string(value: &str) -> Option<DanmakuColor> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        let hex = hex.strip_prefix("0x").unwrap_or(hex);
        let rgb = u32::from_str_radix(hex, 16).ok()?;
        return Some(DanmakuColor::from_decimal(rgb));
    }
    if let Some(inner) = value.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let parts = inner
            .split(',')
            .map(|part| part.trim().parse::<u8>().ok())
            .collect::<Option<Vec<_>>>()?;
        if parts.len() >= 3 {
            return Some(DanmakuColor::rgb_u8(parts[0], parts[1], parts[2]));
        }
    }
    value.parse::<u32>().ok().map(DanmakuColor::from_decimal)
}

fn parse_float_field(field: Option<&&str>, name: &'static str) -> Result<f64> {
    field
        .ok_or_else(|| DanmakuError::InvalidField(name.to_string()))?
        .parse::<f64>()
        .map_err(|_| DanmakuError::InvalidField(name.to_string()))
}

fn decode_xml_entities(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

pub(crate) fn scroll_duration_for_viewport(
    config: &DanmakuLayoutConfig,
    viewport: DanmakuViewport,
) -> Duration {
    let duration_seconds = sanitize_f32(config.scroll_duration_seconds, 10.0).max(1.0);
    let speed_factor = sanitize_f32(config.scroll_speed_factor, 1.0).max(0.001);
    let logical_width = viewport.width as f32 / viewport.scale_factor.max(0.001);
    let width_scale = (logical_width / SCROLL_DURATION_REFERENCE_LOGICAL_WIDTH).clamp(
        SCROLL_DURATION_MIN_WIDTH_SCALE,
        SCROLL_DURATION_MAX_WIDTH_SCALE,
    );
    let millis = duration_seconds * width_scale * 1000.0 / speed_factor;
    Duration::from_millis(millis.max(1.0).round() as u64)
}

fn scale_offset(offset: [f32; 2], scale_factor: f32) -> [f32; 2] {
    let scale_factor = sanitize_f32(scale_factor, 1.0).max(0.001);
    [offset[0] * scale_factor, offset[1] * scale_factor]
}

fn shadow_style_alpha(style: DanmakuShadowStyle) -> f32 {
    match style {
        DanmakuShadowStyle::None => 0.0,
        DanmakuShadowStyle::Soft => 0.34,
        DanmakuShadowStyle::Medium => 0.44,
        DanmakuShadowStyle::Strong => 0.55,
    }
}

fn apply_track_offset(pts: Duration, offset_micros: i64) -> Option<Duration> {
    if offset_micros >= 0 {
        return Some(pts.saturating_add(Duration::from_micros(offset_micros as u64)));
    }
    pts.checked_sub(Duration::from_micros(offset_micros.unsigned_abs()))
}

fn compose_track_item_id(track_id: u64, item_id: u64) -> u64 {
    let track_part = track_id.min((1u64 << (64 - TRACK_ID_SHIFT)) - 1) << TRACK_ID_SHIFT;
    track_part | (item_id & ITEM_ID_MASK)
}

fn sanitize_f32(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

fn load_configured_font(family: &str, file_path: &str) -> Option<FontArc> {
    let file_path = file_path.trim();
    if !file_path.is_empty() {
        if let Some(font) = load_font_from_path(Path::new(file_path)) {
            return Some(font);
        }
    }

    let family = family.trim();
    if !family.is_empty() {
        if let Some(font) = load_font_family(family) {
            return Some(font);
        }
    }

    load_default_font()
}

fn load_font_from_path(path: &Path) -> Option<FontArc> {
    if let Ok(bytes) = fs::read(path) {
        if let Ok(font) = FontArc::try_from_vec(bytes) {
            return Some(font);
        }
    }

    let mut db = fontdb::Database::new();
    db.load_font_file(path).ok()?;
    db.faces()
        .next()
        .and_then(|face| {
            db.with_face_data(face.id, |data, _| FontArc::try_from_vec(data.to_vec()).ok())
        })
        .flatten()
}

fn load_font_family(family: &str) -> Option<FontArc> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let query = fontdb::Query {
        families: &[fontdb::Family::Name(family)],
        weight: fontdb::Weight::NORMAL,
        stretch: fontdb::Stretch::Normal,
        style: fontdb::Style::Normal,
    };
    db.query(&query)
        .and_then(|id| db.with_face_data(id, |data, _| FontArc::try_from_vec(data.to_vec()).ok()))
        .flatten()
}

fn load_default_font() -> Option<FontArc> {
    if let Ok(font) = FontArc::try_from_slice(NIPAPLAY_FALLBACK_FONT) {
        return Some(font);
    }

    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let families = [
        fontdb::Family::Name("PingFang SC"),
        fontdb::Family::Name("Hiragino Sans GB"),
        fontdb::Family::Name("Microsoft YaHei"),
        fontdb::Family::Name("Noto Sans CJK SC"),
        fontdb::Family::Name("Noto Sans CJK"),
        fontdb::Family::Name("Source Han Sans SC"),
        fontdb::Family::SansSerif,
    ];
    for family in families {
        let query = fontdb::Query {
            families: &[family],
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        };
        if let Some(font) = db
            .query(&query)
            .and_then(|id| {
                db.with_face_data(id, |data, _| FontArc::try_from_vec(data.to_vec()).ok())
            })
            .flatten()
        {
            return Some(font);
        }
    }
    const CANDIDATES: &[&str] = &[
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/Library/Fonts/Arial Unicode.ttf",
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/System/Library/Fonts/SFNS.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\simhei.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
    ];
    for path in CANDIDATES {
        let Ok(bytes) = fs::read(path) else {
            continue;
        };
        if let Ok(font) = FontArc::try_from_vec(bytes) {
            return Some(font);
        }
    }
    None
}

fn rasterize_font_glyph(font: &FontArc, key: GlyphCacheKey) -> RasterizedGlyph {
    let font_size = key.font_size();
    let scale = px_scale_for_em(font, font_size);
    let scaled = font.as_scaled(scale);
    let glyph_id = scaled.glyph_id(key.ch);
    let advance = scaled.h_advance(glyph_id).max(0.0);
    if key.ch.is_whitespace() {
        return RasterizedGlyph::empty(key, advance.max(font_size * 0.35));
    }
    let glyph = Glyph {
        id: glyph_id,
        scale,
        position: ab_glyph::point(0.0, 0.0),
    };
    let Some(outlined) = font.outline_glyph(glyph) else {
        return RasterizedGlyph::empty(key, advance.max(font_size * 0.5));
    };
    let bounds = outlined.px_bounds();
    let pad = i32::from(key.outline_radius) + 2;
    let width = (bounds.width().ceil() as i32 + pad * 2).max(1) as u32;
    let height = (bounds.height().ceil() as i32 + pad * 2).max(1) as u32;
    let stride = width as usize;
    let mut fill_alpha = vec![0u8; stride * height as usize];
    outlined.draw(|x, y, coverage| {
        let px = x as i32 + pad;
        let py = y as i32 + pad;
        if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
            return;
        }
        let index = py as usize * stride + px as usize;
        let value = (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
        fill_alpha[index] = fill_alpha[index].max(value);
    });
    let outline_alpha = if key.outline_radius > 0 {
        dilate_alpha(
            &fill_alpha,
            width,
            height,
            stride,
            i32::from(key.outline_radius),
        )
    } else {
        fill_alpha.clone()
    };
    RasterizedGlyph {
        key,
        width,
        height,
        stride,
        fill_alpha,
        outline_alpha,
        offset_x: bounds.min.x.floor() - pad as f32,
        offset_y: bounds.min.y.floor() - pad as f32,
        advance,
    }
}

fn rasterize_fallback_glyph(shaper: &TextShaper, key: GlyphCacheKey) -> RasterizedGlyph {
    let font_size = key.font_size();
    if key.ch.is_whitespace() {
        return RasterizedGlyph::empty(key, font_size * 0.35);
    }
    let text = key.ch.to_string();
    let metrics = shaper.measure(&text, font_size);
    let pad = u32::from(key.outline_radius) + 2;
    let width = metrics.width.ceil().max(1.0) as u32 + pad * 2;
    let height = metrics.height.ceil().max(1.0) as u32 + pad * 2;
    let stride = width as usize;
    let mut fill_alpha = vec![0u8; stride * height as usize];
    for y in pad..height.saturating_sub(pad) {
        for x in pad..width.saturating_sub(pad) {
            let border = x == pad
                || y == pad
                || x + 1 == width.saturating_sub(pad)
                || y + 1 == height.saturating_sub(pad);
            fill_alpha[y as usize * stride + x as usize] = if border { 180 } else { 230 };
        }
    }
    let outline_alpha = if key.outline_radius > 0 {
        dilate_alpha(
            &fill_alpha,
            width,
            height,
            stride,
            i32::from(key.outline_radius),
        )
    } else {
        fill_alpha.clone()
    };
    RasterizedGlyph {
        key,
        width,
        height,
        stride,
        fill_alpha,
        outline_alpha,
        offset_x: -(pad as f32),
        offset_y: -metrics.ascent - pad as f32,
        advance: metrics.width.max(font_size * 0.5),
    }
}

fn dilate_alpha(input: &[u8], width: u32, height: u32, stride: usize, radius: i32) -> Vec<u8> {
    if radius <= 0 {
        return input.to_vec();
    }
    let mut output = input.to_vec();
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let mut max_value = input[y as usize * stride + x as usize];
            for oy in -radius..=radius {
                for ox in -radius..=radius {
                    let nx = x + ox;
                    let ny = y + oy;
                    if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                        continue;
                    }
                    max_value = max_value.max(input[ny as usize * stride + nx as usize]);
                }
            }
            output[y as usize * stride + x as usize] = max_value;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(time: f64, text: &str, mode: DanmakuMode) -> DanmakuItem {
        DanmakuItem {
            id: 0,
            pts: Duration::from_secs_f64(time),
            text: text.to_string(),
            mode,
            font_size: 24.0,
            color: DanmakuColor::WHITE,
            opacity: 1.0,
            is_self: false,
        }
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.001,
            "expected {actual} to be close to {expected}"
        );
    }

    #[test]
    fn parses_json_long_and_short_fields() {
        let input = r##"{
            "comments": [
                {"time": 1.0, "content": "hello", "type": "scroll", "color": "#ff0000"},
                {"t": 2.0, "c": "top", "y": "top", "r": "rgb(0,255,0)"}
            ]
        }"##;
        let timeline = DanmakuTimeline::from_json(input).unwrap();

        assert_eq!(timeline.len(), 2);
        assert_eq!(timeline.items()[0].mode, DanmakuMode::Scroll);
        assert_eq!(timeline.items()[1].mode, DanmakuMode::Top);
        assert_eq!(timeline.items()[0].color.red, 1.0);
        assert_eq!(timeline.items()[1].color.green, 1.0);
    }

    #[test]
    fn parses_nipaplay_camel_case_item_fields() {
        let input = r#"[{"t":1,"c":"phone","y":"scroll","r":16777215,"fontSize":30,"isMe":true}]"#;
        let timeline = DanmakuTimeline::from_json(input).unwrap();

        assert_eq!(timeline.items()[0].font_size, 30.0);
        assert!(timeline.items()[0].is_self);
    }

    #[test]
    fn parses_json_lines_and_xml() {
        let jsonl = r#"{"t":1,"c":"a","y":"bottom","r":16777215}
{"t":2,"c":"b","y":"scroll"}"#;
        let timeline = DanmakuTimeline::from_json_lines(jsonl).unwrap();
        assert_eq!(timeline.items()[0].mode, DanmakuMode::Bottom);

        let xml = r#"<i><d p="1.0,1,25,16777215,0,0,0,42">滚动&amp;测试</d><d p="2.0,5,25,16711680,0,0,0,0">顶部</d></i>"#;
        let timeline = DanmakuTimeline::from_bilibili_xml(xml).unwrap();
        assert_eq!(timeline.len(), 2);
        assert_eq!(timeline.items()[0].text, "滚动&测试");
        assert_eq!(timeline.items()[1].mode, DanmakuMode::Top);
    }

    #[test]
    fn dfm_layout_avoids_same_time_scroll_collision() {
        let timeline = DanmakuTimeline::new(vec![
            item(1.0, "wide comment", DanmakuMode::Scroll),
            item(1.0, "another comment", DanmakuMode::Scroll),
        ])
        .unwrap();
        let mut engine = DfmLayoutEngine::new(timeline, DanmakuLayoutConfig::default());
        let prepared = engine.prepare(DanmakuViewport::new(640, 360), 7);

        assert_eq!(prepared.items().len(), 2);
        assert_ne!(prepared.items()[0].y, prepared.items()[1].y);
    }

    #[test]
    fn default_scroll_layout_uses_full_display_area_when_configured() {
        let timeline = DanmakuTimeline::new(
            (0..18)
                .map(|index| item(1.0, &format!("line {index}"), DanmakuMode::Scroll))
                .collect(),
        )
        .unwrap();
        let config = DanmakuLayoutConfig {
            track_gap_ratio: 0.0,
            merge_duplicates: false,
            allow_scroll_overwrite: false,
            ..DanmakuLayoutConfig::default()
        };
        let mut engine = DfmLayoutEngine::new(timeline, config);
        let prepared = engine.prepare(DanmakuViewport::new(640, 360), 7);
        let max_y = prepared
            .items()
            .iter()
            .map(|item| item.y)
            .fold(0.0, f32::max);

        assert!(
            max_y > 270.0,
            "display_area=1.0 should allow scroll rows into the lower viewport, got max y {max_y}"
        );
        assert!(
            max_y < 360.0,
            "scroll rows should still stay inside the viewport, got max y {max_y}"
        );
    }

    #[test]
    fn frame_layout_uses_generation_and_media_time() {
        let timeline = DanmakuTimeline::new(vec![item(1.0, "sync", DanmakuMode::Scroll)]).unwrap();
        let mut engine = DfmLayoutEngine::new(timeline, DanmakuLayoutConfig::default());
        let viewport = DanmakuViewport::new(640, 360);
        let layout = engine.frame_layout(Duration::from_secs_f64(1.5), viewport, 10);
        let stale = engine.frame_layout(Duration::from_secs_f64(1.5), viewport, 11);

        assert_eq!(layout.generation, 10);
        assert_eq!(layout.items.len(), 1);
        assert_eq!(stale.generation, 11);
    }

    #[test]
    fn generation_change_reuses_prepared_layout() {
        let timeline = DanmakuTimeline::new(vec![item(1.0, "seek", DanmakuMode::Scroll)]).unwrap();
        let mut engine = DfmLayoutEngine::new(timeline, DanmakuLayoutConfig::default());
        let viewport = DanmakuViewport::new(640, 360);

        let first = engine.prepare(viewport, 1);
        let first_text = Arc::clone(&first.items()[0].text);
        let first_text_layout = Arc::clone(&first.items()[0].text_layout);
        let layout = engine.frame_layout(Duration::from_secs_f64(1.5), viewport, 2);
        let prepared = engine.prepared.as_ref().expect("prepared layout exists");

        assert_eq!(layout.generation, 2);
        assert!(Arc::ptr_eq(&prepared.items()[0].text, &first_text));
        assert!(Arc::ptr_eq(
            &prepared.items()[0].text_layout,
            &first_text_layout
        ));
    }

    #[test]
    fn sliding_timeline_window_preserves_tracks_for_overlapping_items() {
        let timeline = DanmakuTimeline::new(
            (0..80)
                .map(|index| {
                    let mut entry = item(
                        index as f64 * 0.25,
                        &format!("scroll {index}"),
                        DanmakuMode::Scroll,
                    );
                    entry.id = index + 1;
                    entry
                })
                .collect(),
        )
        .unwrap();
        let config = DanmakuLayoutConfig {
            allow_stacking: true,
            merge_duplicates: false,
            ..DanmakuLayoutConfig::default()
        };
        let viewport = DanmakuViewport::new(640, 360);
        let first_window = timeline.window(Duration::ZERO, Duration::from_secs(12));
        let second_window = timeline.window(Duration::from_secs(4), Duration::from_secs(16));
        let mut engine = DfmLayoutEngine::new(first_window, config);

        let first = engine.prepare(viewport, 1);
        let first_tracks = first
            .items()
            .iter()
            .map(|item| (item.id, item.track_index))
            .collect::<HashMap<_, _>>();
        engine.sync_timeline(&second_window);
        let second = engine.prepare(viewport, 1);

        let overlap = second
            .items()
            .iter()
            .filter_map(|item| {
                first_tracks
                    .get(&item.id)
                    .map(|track| (*track, item.track_index))
            })
            .collect::<Vec<_>>();
        assert!(!overlap.is_empty());
        assert!(overlap.iter().all(|(before, after)| before == after));
        assert_eq!(engine.stable_tracks.len(), second.items().len());
        assert!(
            engine
                .stable_tracks
                .keys()
                .all(|item_id| second.items().iter().any(|item| item.id == *item_id))
        );
    }

    #[test]
    fn render_plan_contains_glyph_atlas() {
        let timeline = DanmakuTimeline::new(vec![item(0.0, "GPU", DanmakuMode::Top)]).unwrap();
        let mut engine = DfmLayoutEngine::new(timeline, DanmakuLayoutConfig::default());
        let plan = engine.render_plan(
            Duration::from_millis(500),
            DanmakuViewport::new(640, 360),
            3,
        );

        assert_eq!(plan.generation, 3);
        assert!(!plan.items.is_empty());
        let atlas = plan.atlas.as_ref().expect("glyph atlas exists");
        assert!(atlas.is_valid());
        assert!(plan.items.iter().all(|item| item.tex_rect[2] > 0.0));
    }

    #[test]
    fn render_plan_reuses_glyph_atlas_snapshot_for_stable_glyph_set() {
        let timeline = DanmakuTimeline::new(vec![item(0.0, "GPU", DanmakuMode::Scroll)]).unwrap();
        let mut engine = DfmLayoutEngine::new(timeline, DanmakuLayoutConfig::default());
        let viewport = DanmakuViewport::new(640, 360);

        let first = engine.render_plan(Duration::from_millis(500), viewport, 3);
        let first_atlas = first.atlas.as_ref().expect("glyph atlas exists").clone();
        let first_x = first.items[0].rect[0];
        let second = engine.render_plan(Duration::from_millis(600), viewport, 4);
        let second_atlas = second.atlas.as_ref().expect("glyph atlas exists");

        assert_eq!(second.generation, 4);
        assert_ne!(first_x, second.items[0].rect[0]);
        assert_eq!(first_atlas.version, second_atlas.version);
        assert!(Arc::ptr_eq(&first_atlas, second_atlas));
    }

    #[test]
    fn glyph_atlas_snapshot_describes_only_the_new_dirty_region() {
        let first_timeline = DanmakuTimeline::new(vec![item(0.0, "A", DanmakuMode::Top)]).unwrap();
        let mut engine = DfmLayoutEngine::new(first_timeline, DanmakuLayoutConfig::default());
        let viewport = DanmakuViewport::new(640, 360);
        let first = engine.render_plan(Duration::from_millis(500), viewport, 1);
        let first_atlas = first.atlas.as_ref().expect("first glyph atlas exists");
        assert!(first_atlas.update.is_none());

        engine.set_timeline(DanmakuTimeline::new(vec![item(0.0, "AB", DanmakuMode::Top)]).unwrap());
        let second = engine.render_plan(Duration::from_millis(500), viewport, 2);
        let second_atlas = second.atlas.as_ref().expect("second glyph atlas exists");
        let update = second_atlas
            .update
            .as_ref()
            .expect("new glyph produces an incremental update");

        assert_eq!(update.from_version, first_atlas.version);
        assert!(update.width > 0 && update.height > 0);
        assert!(update.width < second_atlas.width);
        assert!(
            second_atlas
                .incremental_update_from(
                    first_atlas.version,
                    first_atlas.width,
                    first_atlas.height,
                    first_atlas.stride,
                )
                .is_some()
        );
    }

    #[test]
    fn paint_only_config_change_reuses_prepared_layout() {
        let timeline = DanmakuTimeline::new(vec![item(0.0, "paint", DanmakuMode::Top)]).unwrap();
        let mut engine = DfmLayoutEngine::new(timeline, DanmakuLayoutConfig::default());
        let viewport = DanmakuViewport::new(640, 360);
        let first = engine.prepare(viewport, 1);
        let first_text_layout = Arc::clone(&first.items()[0].text_layout);

        let mut config = engine.config().clone();
        config.opacity = 0.4;
        config.shadow_style = DanmakuShadowStyle::None;
        assert_eq!(engine.apply_config(config), DanmakuConfigChange::PaintOnly);

        let prepared = engine.prepared.as_ref().expect("prepared layout retained");
        assert!(Arc::ptr_eq(
            &prepared.items()[0].text_layout,
            &first_text_layout
        ));
        let frame = prepared.frame_layout(Duration::from_millis(500), 2);
        assert_close(frame.items[0].opacity, 0.4);
        assert_close(frame.items[0].shadow_alpha, 0.0);
    }

    #[test]
    fn scroll_duration_uses_logical_width_and_retina_scale() {
        let config = DanmakuLayoutConfig::default();
        let narrow = scroll_duration_for_viewport(&config, DanmakuViewport::new(640, 360));
        let reference = scroll_duration_for_viewport(&config, DanmakuViewport::new(1280, 720));
        let wide = scroll_duration_for_viewport(&config, DanmakuViewport::new(1920, 1080));
        let retina_reference =
            scroll_duration_for_viewport(&config, DanmakuViewport::with_scale(2560, 1440, 2.0));

        assert_eq!(narrow, Duration::from_secs(9));
        assert_eq!(reference, Duration::from_secs(10));
        assert_eq!(wide, Duration::from_secs(13));
        assert_eq!(retina_reference, reference);
    }

    #[test]
    fn layout_font_size_uses_nipaplay_logical_units_across_resize() {
        let timeline = DanmakuTimeline::new(vec![DanmakuItem {
            font_size: DEFAULT_SOURCE_FONT_SIZE,
            ..item(0.0, "scale", DanmakuMode::Scroll)
        }])
        .unwrap();
        let config = DanmakuLayoutConfig {
            font_size: 32.0,
            ..DanmakuLayoutConfig::default()
        };
        let mut engine = DfmLayoutEngine::new(timeline, config);

        let small = engine.prepare(DanmakuViewport::new(640, 360), 1);
        let large = engine.prepare(DanmakuViewport::new(1920, 1080), 2);
        let expected = 32.0;

        assert_close(small.items()[0].font_size, expected);
        assert_close(large.items()[0].font_size, expected);
        assert!(large.items()[0].scroll_speed > small.items()[0].scroll_speed);
    }

    #[test]
    fn layout_font_size_uses_surface_scale_for_physical_pixels() {
        let timeline = DanmakuTimeline::new(vec![DanmakuItem {
            font_size: DEFAULT_SOURCE_FONT_SIZE,
            ..item(0.0, "scale", DanmakuMode::Top)
        }])
        .unwrap();
        let config = DanmakuLayoutConfig {
            font_size: 32.0,
            ..DanmakuLayoutConfig::default()
        };
        let mut engine = DfmLayoutEngine::new(timeline, config);

        let one_x = engine.prepare(DanmakuViewport::with_scale(800, 450, 1.0), 1);
        let two_x = engine.prepare(DanmakuViewport::with_scale(1600, 900, 2.0), 2);
        let expected = 32.0;

        assert_close(one_x.items()[0].font_size, expected);
        assert_close(two_x.items()[0].font_size, expected * 2.0);
    }

    #[test]
    fn nipaplay_phone_font_size_uses_fractional_android_density() {
        let timeline = DanmakuTimeline::new(vec![DanmakuItem {
            font_size: DEFAULT_SOURCE_FONT_SIZE,
            ..item(0.0, "android density", DanmakuMode::Top)
        }])
        .unwrap();
        let config = DanmakuLayoutConfig {
            font_size: 20.0,
            ..DanmakuLayoutConfig::default()
        };
        let mut engine = DfmLayoutEngine::new(timeline, config);

        let prepared = engine.prepare(DanmakuViewport::with_scale(1080, 2400, 2.625), 1);

        assert_close(prepared.items()[0].font_size, 52.5);
    }

    #[test]
    fn source_font_size_is_relative_to_configured_reference_size() {
        let timeline = DanmakuTimeline::new(vec![DanmakuItem {
            font_size: 50.0,
            ..item(0.0, "large", DanmakuMode::Top)
        }])
        .unwrap();
        let config = DanmakuLayoutConfig {
            font_size: 30.0,
            ..DanmakuLayoutConfig::default()
        };
        let mut engine = DfmLayoutEngine::new(timeline, config);
        let prepared = engine.prepare(DanmakuViewport::new(1920, 1080), 1);

        assert_close(prepared.items()[0].font_size, 60.0);
    }

    #[test]
    fn render_plan_keeps_fill_and_outline_masks_separate() {
        let timeline = DanmakuTimeline::new(vec![item(0.0, "Outline", DanmakuMode::Top)]).unwrap();
        let config = DanmakuLayoutConfig {
            outline_width: 2.0,
            ..DanmakuLayoutConfig::default()
        };
        let mut engine = DfmLayoutEngine::new(timeline, config);
        let plan = engine.render_plan(
            Duration::from_millis(500),
            DanmakuViewport::new(640, 360),
            3,
        );
        let atlas = plan.atlas.as_ref().expect("glyph atlas exists");
        let fill_sum = atlas.fill_alpha.iter().map(|&v| u64::from(v)).sum::<u64>();
        let outline_sum = atlas
            .outline_alpha
            .iter()
            .map(|&v| u64::from(v))
            .sum::<u64>();

        assert!(outline_sum > fill_sum);
        assert_ne!(atlas.fill_alpha, atlas.outline_alpha);
    }

    #[test]
    fn keyword_filter_blocks_items() {
        let timeline = DanmakuTimeline::new(vec![
            item(0.0, "visible", DanmakuMode::Scroll),
            item(0.1, "blocked bad", DanmakuMode::Scroll),
        ])
        .unwrap();
        let config = DanmakuLayoutConfig {
            block_words: vec!["bad".to_string()],
            ..DanmakuLayoutConfig::default()
        };
        let mut engine = DfmLayoutEngine::new(timeline, config);
        let prepared = engine.prepare(DanmakuViewport::new(640, 360), 1);

        assert_eq!(prepared.items().len(), 1);
        assert_eq!(prepared.items()[0].text.as_ref(), "visible");
    }

    #[test]
    fn duplicate_merge_matches_dfm_prepare_scope() {
        let timeline = DanmakuTimeline::new(vec![
            item(0.0, "same", DanmakuMode::Top),
            item(0.1, "same", DanmakuMode::Top),
            item(11.0, "same", DanmakuMode::Top),
        ])
        .unwrap();
        let config = DanmakuLayoutConfig {
            merge_duplicates: true,
            ..DanmakuLayoutConfig::default()
        };
        let mut engine = DfmLayoutEngine::new(timeline, config);
        let prepared = engine.prepare(DanmakuViewport::new(640, 360), 1);

        assert_eq!(prepared.items().len(), 1);
        assert_eq!(prepared.items()[0].duplicate_count, 3);
        assert_eq!(prepared.items()[0].text.as_ref(), "same x3");
    }

    #[test]
    fn parser_skips_bad_individual_items() {
        let input = r#"{"comments":[
            {"time":1,"content":"ok"},
            {"time":2,"content":""},
            {"bad":"shape"},
            {"time":3,"content":"still ok"}
        ]}"#;
        let timeline = DanmakuTimeline::from_json(input).unwrap();

        assert_eq!(timeline.len(), 2);
        assert_eq!(timeline.items()[0].text, "ok");
        assert_eq!(timeline.items()[1].text, "still ok");
    }

    #[test]
    fn dfm_overwrite_marks_displaced_scroll_items_filtered() {
        let mut first = item(0.0, "first wide", DanmakuMode::Scroll);
        first.font_size = 24.0;
        let mut second = item(0.0, "second wide", DanmakuMode::Scroll);
        second.font_size = 24.0;
        let timeline = DanmakuTimeline::new(vec![first, second]).unwrap();
        let config = DanmakuLayoutConfig {
            display_area: 0.1,
            track_gap_ratio: 0.0,
            allow_scroll_overwrite: true,
            merge_duplicates: false,
            ..DanmakuLayoutConfig::default()
        };
        let mut engine = DfmLayoutEngine::new(timeline, config);
        let prepared = engine.prepare(DanmakuViewport::new(320, 24), 1);

        assert_eq!(prepared.items().len(), 1);
        assert_eq!(prepared.items()[0].text.as_ref(), "second wide");
    }

    #[test]
    fn max_lines_caps_each_danmaku_mode_tracks() {
        let timeline = DanmakuTimeline::new(vec![
            item(0.0, "a", DanmakuMode::Top),
            item(0.0, "b", DanmakuMode::Top),
            item(0.0, "c", DanmakuMode::Bottom),
            item(0.0, "d", DanmakuMode::Bottom),
        ])
        .unwrap();
        let config = DanmakuLayoutConfig {
            max_lines_per_mode: Some(1),
            merge_duplicates: false,
            ..DanmakuLayoutConfig::default()
        };
        let mut engine = DfmLayoutEngine::new(timeline, config);
        let prepared = engine.prepare(DanmakuViewport::new(640, 360), 1);

        let top_count = prepared
            .items()
            .iter()
            .filter(|item| item.mode == DanmakuMode::Top)
            .count();
        let bottom_count = prepared
            .items()
            .iter()
            .filter(|item| item.mode == DanmakuMode::Bottom)
            .count();

        assert!(!prepared.items().is_empty());
        assert!(top_count <= 1);
        assert!(bottom_count <= 1);
        assert!(prepared.items().iter().all(|item| item.track_index == 0));
    }

    #[test]
    fn regex_filter_blocks_matching_items() {
        let timeline = DanmakuTimeline::new(vec![
            item(0.0, "visible", DanmakuMode::Scroll),
            item(0.1, "episode 123 spoiler", DanmakuMode::Scroll),
        ])
        .unwrap();
        let config = DanmakuLayoutConfig {
            block_words: vec!["spoiler/[0-9]+ spoiler/".to_string()],
            merge_duplicates: false,
            ..DanmakuLayoutConfig::default()
        };
        let mut engine = DfmLayoutEngine::new(timeline, config);
        let prepared = engine.prepare(DanmakuViewport::new(640, 360), 1);

        assert_eq!(prepared.items().len(), 1);
        assert_eq!(prepared.items()[0].text.as_ref(), "visible");
    }

    #[test]
    fn json_special_danmaku_is_filtered_from_layout() {
        let input = r#"{"comments":[{"time":1,"content":"path","type":"special"}]}"#;
        let timeline = DanmakuTimeline::from_json(input).unwrap();
        let mut engine = DfmLayoutEngine::new(timeline, DanmakuLayoutConfig::default());
        let prepared = engine.prepare(DanmakuViewport::new(640, 360), 1);

        assert_eq!(prepared.items().len(), 0);
    }
}
