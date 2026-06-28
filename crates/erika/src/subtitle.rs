#[cfg(feature = "libass")]
use std::ptr::NonNull;
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SubtitleError {
    #[error("invalid subtitle timestamp: {0}")]
    InvalidTimestamp(String),
    #[error("invalid subtitle cue")]
    InvalidCue,
    #[error("invalid subtitle bitmap: width={width} height={height} stride={stride} bytes={bytes}")]
    InvalidBitmap {
        width: u32,
        height: u32,
        stride: usize,
        bytes: usize,
    },
    #[error("subtitle bitmap pointer is null")]
    NullBitmap,
    #[error("subtitle bitmap list exceeded safety limit")]
    BitmapListTooLong,
    #[error("libass error: {0}")]
    Libass(String),
}

pub type Result<T> = std::result::Result<T, SubtitleError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubtitleTrackSource {
    Embedded { stream_index: i64 },
    External { uri: String },
}

impl SubtitleTrackSource {
    pub const fn embedded(stream_index: i64) -> Self {
        Self::Embedded { stream_index }
    }

    pub fn external(uri: impl Into<String>) -> Self {
        Self::External { uri: uri.into() }
    }

    pub const fn is_embedded(&self) -> bool {
        matches!(self, Self::Embedded { .. })
    }

    pub const fn is_external(&self) -> bool {
        matches!(self, Self::External { .. })
    }

    pub const fn can_remove(&self) -> bool {
        self.is_external()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleTrackConfig {
    pub id: i64,
    pub source: SubtitleTrackSource,
    pub language: Option<String>,
    pub title: Option<String>,
}

impl SubtitleTrackConfig {
    pub fn embedded(id: i64, stream_index: i64) -> Self {
        Self {
            id,
            source: SubtitleTrackSource::embedded(stream_index),
            language: None,
            title: None,
        }
    }

    pub fn external(id: i64, uri: impl Into<String>) -> Self {
        Self {
            id,
            source: SubtitleTrackSource::external(uri),
            language: None,
            title: None,
        }
    }

    pub const fn can_remove(&self) -> bool {
        self.source.can_remove()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleTextFormat {
    PlainText,
    Ass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleFileFormat {
    Srt,
    WebVtt,
    Ass,
}

impl SubtitleFileFormat {
    pub fn from_path(path: impl AsRef<str>) -> Option<Self> {
        let path = path.as_ref();
        let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
        match extension.as_str() {
            "srt" => Some(Self::Srt),
            "vtt" | "webvtt" => Some(Self::WebVtt),
            "ass" | "ssa" => Some(Self::Ass),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleTextSegment {
    pub format: SubtitleTextFormat,
    pub text: String,
    pub forced: bool,
}

impl SubtitleTextSegment {
    pub fn new(format: SubtitleTextFormat, text: impl Into<String>) -> Self {
        Self {
            format,
            text: text.into(),
            forced: false,
        }
    }

    pub fn with_forced(mut self, forced: bool) -> Self {
        self.forced = forced;
        self
    }

    pub fn display_text(&self) -> String {
        match self.format {
            SubtitleTextFormat::PlainText => self.text.trim().to_string(),
            SubtitleTextFormat::Ass => ass_segment_display_text(&self.text),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedSubtitleFrame {
    pub track_id: i64,
    pub start: Option<Duration>,
    pub end: Option<Duration>,
    pub text: Vec<SubtitleTextSegment>,
    pub bitmap: SubtitleFrame,
    pub forced: bool,
}

impl DecodedSubtitleFrame {
    pub fn new(track_id: i64, start: Option<Duration>, end: Option<Duration>) -> Self {
        Self {
            track_id,
            start,
            end,
            text: Vec::new(),
            bitmap: SubtitleFrame {
                pts: start.unwrap_or(Duration::ZERO),
                planes: Vec::new(),
            },
            forced: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.bitmap.planes.is_empty()
    }

    pub fn push_text(&mut self, segment: SubtitleTextSegment) {
        self.forced |= segment.forced;
        if !segment.text.is_empty() {
            self.text.push(segment);
        }
    }

    pub fn push_bitmap_plane(&mut self, plane: SubtitleBitmapPlane, forced: bool) {
        self.forced |= forced;
        self.bitmap.planes.push(plane);
    }

    pub fn with_track_id(mut self, track_id: i64) -> Self {
        self.track_id = track_id;
        self
    }

    pub fn has_text(&self) -> bool {
        self.text.iter().any(|segment| !segment.text.is_empty())
    }

    pub fn text_cues(&self, fallback_end: Duration) -> Vec<SubtitleCue> {
        let Some(start) = self.start else {
            return Vec::new();
        };
        let end = self.end.unwrap_or(fallback_end).max(start);
        self.text
            .iter()
            .filter_map(|segment| {
                let text = segment.display_text();
                (!text.is_empty()).then_some(SubtitleCue { start, end, text })
            })
            .collect()
    }

    pub fn to_ass_script(&self, fallback_end: Duration) -> Option<String> {
        decoded_subtitle_frames_to_ass_script([self], fallback_end)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleCue {
    pub start: Duration,
    pub end: Duration,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleBitmapPlane {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub rgba: Vec<u8>,
}

impl SubtitleBitmapPlane {
    pub fn new(x: i32, y: i32, width: u32, height: u32, rgba: Vec<u8>) -> Self {
        Self {
            x,
            y,
            width,
            height,
            canvas_width: 0,
            canvas_height: 0,
            rgba,
        }
    }

    pub fn with_canvas(mut self, width: u32, height: u32) -> Self {
        self.canvas_width = width;
        self.canvas_height = height;
        self
    }

    pub fn scaled_rect(&self, viewport_width: u32, viewport_height: u32) -> (i32, i32, u32, u32) {
        if self.canvas_width == 0
            || self.canvas_height == 0
            || (self.canvas_width == viewport_width && self.canvas_height == viewport_height)
        {
            return (self.x, self.y, self.width, self.height);
        }
        let viewport_width = viewport_width.max(1) as f64;
        let viewport_height = viewport_height.max(1) as f64;
        let canvas_width = self.canvas_width.max(1) as f64;
        let canvas_height = self.canvas_height.max(1) as f64;
        let scale = (viewport_width / canvas_width).max(viewport_height / canvas_height);
        let offset_x = (viewport_width - canvas_width * scale) * 0.5;
        let offset_y = (viewport_height - canvas_height * scale) * 0.5;
        (
            (offset_x + self.x as f64 * scale).round() as i32,
            (offset_y + self.y as f64 * scale).round() as i32,
            ((self.width as f64 * scale).round() as u32).max(1),
            ((self.height as f64 * scale).round() as u32).max(1),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubtitleBitmapPlacement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl SubtitleBitmapPlacement {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn clipped_to(self, frame_width: u32, frame_height: u32) -> Option<Self> {
        let left = self.x.max(0) as i64;
        let top = self.y.max(0) as i64;
        let right = (self.x as i64 + self.width as i64).min(frame_width as i64);
        let bottom = (self.y as i64 + self.height as i64).min(frame_height as i64);
        if right <= left || bottom <= top {
            return None;
        }
        Some(Self::new(
            left as i32,
            top as i32,
            (right - left) as u32,
            (bottom - top) as u32,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleBitmapColorSpace {
    Srgb,
    Video,
}

impl Default for SubtitleBitmapColorSpace {
    fn default() -> Self {
        Self::Srgb
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleAlphaBitmap {
    pub placement: SubtitleBitmapPlacement,
    pub stride: usize,
    pub color_rgba: u32,
    pub alpha: Vec<u8>,
}

impl SubtitleAlphaBitmap {
    pub fn new(
        placement: SubtitleBitmapPlacement,
        stride: usize,
        color_rgba: u32,
        alpha: Vec<u8>,
    ) -> Self {
        Self {
            placement,
            stride: stride.max(placement.width as usize),
            color_rgba,
            alpha,
        }
    }

    pub fn required_len(&self) -> usize {
        if self.placement.height == 0 || self.placement.width == 0 {
            return 0;
        }
        self.stride
            .saturating_mul(self.placement.height.saturating_sub(1) as usize)
            .saturating_add(self.placement.width as usize)
    }

    pub fn is_valid(&self) -> bool {
        self.alpha.len() >= self.required_len()
    }

    pub fn to_rgba_plane(&self) -> Option<SubtitleBitmapPlane> {
        if !self.is_valid() {
            return None;
        }
        let width = self.placement.width as usize;
        let height = self.placement.height as usize;
        if width == 0 || height == 0 {
            return None;
        }

        let color = AssColor::from_libass_rgba(self.color_rgba);
        let mut rgba = vec![0u8; width * height * 4];
        for y in 0..height {
            let row_start = y * self.stride;
            for x in 0..width {
                let coverage = self.alpha[row_start + x];
                let alpha = multiply_u8(color.alpha, coverage);
                let pixel = &mut rgba[(y * width + x) * 4..][..4];
                pixel.copy_from_slice(&[color.red, color.green, color.blue, alpha]);
            }
        }

        Some(SubtitleBitmapPlane::new(
            self.placement.x,
            self.placement.y,
            self.placement.width,
            self.placement.height,
            rgba,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleBitmapSet {
    pub pts: Duration,
    pub frame_width: u32,
    pub frame_height: u32,
    pub color_space: SubtitleBitmapColorSpace,
    pub parts: Vec<SubtitleAlphaBitmap>,
    pub changed: bool,
}

impl SubtitleBitmapSet {
    pub fn new(pts: Duration, frame_width: u32, frame_height: u32) -> Self {
        Self {
            pts,
            frame_width,
            frame_height,
            color_space: SubtitleBitmapColorSpace::default(),
            parts: Vec::new(),
            changed: true,
        }
    }

    pub fn with_color_space(mut self, color_space: SubtitleBitmapColorSpace) -> Self {
        self.color_space = color_space;
        self
    }

    pub fn with_changed(mut self, changed: bool) -> Self {
        self.changed = changed;
        self
    }

    pub fn push(&mut self, bitmap: SubtitleAlphaBitmap) {
        if bitmap.placement.width > 0 && bitmap.placement.height > 0 {
            self.parts.push(bitmap);
        }
    }

    pub fn to_frame(&self) -> SubtitleFrame {
        let planes = self
            .parts
            .iter()
            .filter_map(SubtitleAlphaBitmap::to_rgba_plane)
            .collect();
        SubtitleFrame {
            pts: self.pts,
            planes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleFrame {
    pub pts: Duration,
    pub planes: Vec<SubtitleBitmapPlane>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubtitleRenderViewport {
    pub width: u32,
    pub height: u32,
    pub storage_width: u32,
    pub storage_height: u32,
}

impl SubtitleRenderViewport {
    pub fn new(width: u32, height: u32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            width,
            height,
            storage_width: width,
            storage_height: height,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubtitleRenderRequest {
    pub pts: Duration,
    pub viewport: SubtitleRenderViewport,
}

impl SubtitleRenderRequest {
    pub fn new(pts: Duration, width: u32, height: u32) -> Self {
        Self {
            pts,
            viewport: SubtitleRenderViewport::new(width, height),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleRenderBackend {
    DebugTimeline,
    Libass,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubtitleRenderOutput {
    Rgba(SubtitleFrame),
    Alpha(SubtitleBitmapSet),
}

impl SubtitleRenderOutput {
    pub fn into_rgba_frame(self) -> SubtitleFrame {
        match self {
            Self::Rgba(frame) => frame,
            Self::Alpha(bitmaps) => bitmaps.to_frame(),
        }
    }
}

pub trait SubtitleRenderer {
    fn backend(&self) -> SubtitleRenderBackend;
    fn render(&mut self, request: SubtitleRenderRequest) -> Result<SubtitleRenderOutput>;
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RawAssImage {
    pub w: i32,
    pub h: i32,
    pub stride: i32,
    pub bitmap: *const u8,
    pub color: u32,
    pub dst_x: i32,
    pub dst_y: i32,
    pub next: *const RawAssImage,
    pub image_type: i32,
}

#[cfg(feature = "libass")]
mod libass_ffi {
    use libc::{c_char, c_int, c_longlong, c_void, size_t};

    pub type AssImageType = c_int;
    pub type AssLibrary = c_void;
    pub type AssRenderer = c_void;
    pub type AssTrack = c_void;

    #[repr(C)]
    pub struct AssImage {
        pub w: c_int,
        pub h: c_int,
        pub stride: c_int,
        pub bitmap: *mut u8,
        pub color: u32,
        pub dst_x: c_int,
        pub dst_y: c_int,
        pub next: *mut AssImage,
        pub image_type: AssImageType,
    }

    unsafe extern "C" {
        pub fn ass_library_init() -> *mut AssLibrary;
        pub fn ass_library_done(library: *mut AssLibrary);
        pub fn ass_renderer_init(library: *mut AssLibrary) -> *mut AssRenderer;
        pub fn ass_renderer_done(renderer: *mut AssRenderer);
        pub fn ass_set_frame_size(renderer: *mut AssRenderer, width: c_int, height: c_int);
        pub fn ass_set_storage_size(renderer: *mut AssRenderer, width: c_int, height: c_int);
        pub fn ass_set_fonts(
            renderer: *mut AssRenderer,
            default_font: *const c_char,
            default_family: *const c_char,
            default_font_provider: c_int,
            config: *const c_char,
            update: c_int,
        );
        pub fn ass_set_cache_limits(
            renderer: *mut AssRenderer,
            glyph_max: c_int,
            bitmap_max_size: c_int,
        );
        pub fn ass_read_memory(
            library: *mut AssLibrary,
            buffer: *mut c_char,
            buffer_size: size_t,
            codepage: *const c_char,
        ) -> *mut AssTrack;
        pub fn ass_free_track(track: *mut AssTrack);
        pub fn ass_render_frame(
            renderer: *mut AssRenderer,
            track: *mut AssTrack,
            now: c_longlong,
            detect_change: *mut c_int,
        ) -> *mut AssImage;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LibassRenderConfig {
    pub glyph_cache_limit: i32,
    pub bitmap_cache_limit_mb: i32,
}

impl Default for LibassRenderConfig {
    fn default() -> Self {
        Self {
            glyph_cache_limit: 0,
            bitmap_cache_limit_mb: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SubtitleAssStyle {
    pub font_scale: f64,
    pub play_res_width: u32,
    pub play_res_height: u32,
}

impl Default for SubtitleAssStyle {
    fn default() -> Self {
        Self {
            font_scale: 1.0,
            play_res_width: 1920,
            play_res_height: 1080,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibassRenderOperation {
    SetFrameSize { width: u32, height: u32 },
    SetStorageSize { width: u32, height: u32 },
    SetCacheLimits { glyphs: i32, bitmap_mb: i32 },
    RenderFrame { timestamp_ms: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibassRenderPlan {
    pub request: SubtitleRenderRequest,
    pub config: LibassRenderConfig,
    pub operations: Vec<LibassRenderOperation>,
}

impl LibassRenderPlan {
    pub fn new(request: SubtitleRenderRequest, config: LibassRenderConfig) -> Self {
        let viewport = request.viewport;
        Self {
            request,
            config,
            operations: vec![
                LibassRenderOperation::SetFrameSize {
                    width: viewport.width,
                    height: viewport.height,
                },
                LibassRenderOperation::SetStorageSize {
                    width: viewport.storage_width,
                    height: viewport.storage_height,
                },
                LibassRenderOperation::SetCacheLimits {
                    glyphs: config.glyph_cache_limit,
                    bitmap_mb: config.bitmap_cache_limit_mb,
                },
                LibassRenderOperation::RenderFrame {
                    timestamp_ms: duration_to_millis_i64(request.pts),
                },
            ],
        }
    }
}

#[cfg(feature = "libass")]
#[derive(Debug)]
pub struct LibassSubtitleRenderer {
    library: NonNull<libass_ffi::AssLibrary>,
    renderer: NonNull<libass_ffi::AssRenderer>,
    track: NonNull<libass_ffi::AssTrack>,
    config: LibassRenderConfig,
}

#[cfg(feature = "libass")]
impl LibassSubtitleRenderer {
    pub fn from_ass_script(script: impl AsRef<[u8]>, config: LibassRenderConfig) -> Result<Self> {
        let script = script.as_ref();
        if script.is_empty() {
            return Err(SubtitleError::Libass("ASS script is empty".to_string()));
        }

        let mut script = script.to_vec();
        unsafe {
            let library = NonNull::new(libass_ffi::ass_library_init()).ok_or_else(|| {
                SubtitleError::Libass("failed to initialize libass library".to_string())
            })?;

            let Some(renderer) = NonNull::new(libass_ffi::ass_renderer_init(library.as_ptr()))
            else {
                libass_ffi::ass_library_done(library.as_ptr());
                return Err(SubtitleError::Libass(
                    "failed to initialize libass renderer".to_string(),
                ));
            };

            libass_ffi::ass_set_fonts(
                renderer.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                std::ptr::null(),
                1,
            );
            libass_ffi::ass_set_cache_limits(
                renderer.as_ptr(),
                config.glyph_cache_limit,
                config.bitmap_cache_limit_mb,
            );

            let Some(track) = NonNull::new(libass_ffi::ass_read_memory(
                library.as_ptr(),
                script.as_mut_ptr().cast(),
                script.len(),
                std::ptr::null(),
            )) else {
                libass_ffi::ass_renderer_done(renderer.as_ptr());
                libass_ffi::ass_library_done(library.as_ptr());
                return Err(SubtitleError::Libass(
                    "failed to parse ASS script with libass".to_string(),
                ));
            };

            Ok(Self {
                library,
                renderer,
                track,
                config,
            })
        }
    }

    pub fn config(&self) -> LibassRenderConfig {
        self.config
    }

    pub fn render_plan(&self, request: SubtitleRenderRequest) -> LibassRenderPlan {
        LibassRenderPlan::new(request, self.config)
    }
}

#[cfg(feature = "libass")]
impl Drop for LibassSubtitleRenderer {
    fn drop(&mut self) {
        unsafe {
            libass_ffi::ass_free_track(self.track.as_ptr());
            libass_ffi::ass_renderer_done(self.renderer.as_ptr());
            libass_ffi::ass_library_done(self.library.as_ptr());
        }
    }
}

#[cfg(feature = "libass")]
impl SubtitleRenderer for LibassSubtitleRenderer {
    fn backend(&self) -> SubtitleRenderBackend {
        SubtitleRenderBackend::Libass
    }

    fn render(&mut self, request: SubtitleRenderRequest) -> Result<SubtitleRenderOutput> {
        let viewport = request.viewport;
        let frame_width = libass_dimension(viewport.width, "frame width")?;
        let frame_height = libass_dimension(viewport.height, "frame height")?;
        let storage_width = libass_dimension(viewport.storage_width, "storage width")?;
        let storage_height = libass_dimension(viewport.storage_height, "storage height")?;
        let timestamp_ms = duration_to_millis_i64(request.pts);

        unsafe {
            libass_ffi::ass_set_frame_size(self.renderer.as_ptr(), frame_width, frame_height);
            libass_ffi::ass_set_storage_size(self.renderer.as_ptr(), storage_width, storage_height);
            libass_ffi::ass_set_cache_limits(
                self.renderer.as_ptr(),
                self.config.glyph_cache_limit,
                self.config.bitmap_cache_limit_mb,
            );

            let mut changed = 0;
            let images = libass_ffi::ass_render_frame(
                self.renderer.as_ptr(),
                self.track.as_ptr(),
                timestamp_ms,
                &mut changed,
            );
            Ok(SubtitleRenderOutput::Alpha(import_libass_image_list(
                request.pts,
                viewport.width,
                viewport.height,
                images,
                changed != 0,
            )?))
        }
    }
}

pub struct LibassImageImporter;

impl LibassImageImporter {
    pub unsafe fn import_raw_list(
        pts: Duration,
        frame_width: u32,
        frame_height: u32,
        first: *const RawAssImage,
        changed: bool,
    ) -> Result<SubtitleBitmapSet> {
        let mut set = SubtitleBitmapSet::new(pts, frame_width, frame_height)
            .with_color_space(SubtitleBitmapColorSpace::Video)
            .with_changed(changed);
        let mut current = first;
        let mut count = 0usize;

        while !current.is_null() {
            if count >= RAW_ASS_IMAGE_LIST_LIMIT {
                return Err(SubtitleError::BitmapListTooLong);
            }
            let image = unsafe { &*current };
            if image.w > 0 && image.h > 0 {
                let bitmap = unsafe { raw_ass_image_to_alpha_bitmap(image)? };
                set.push(bitmap);
            }
            current = image.next;
            count += 1;
        }

        Ok(set)
    }
}

#[cfg(feature = "libass")]
fn libass_dimension(value: u32, label: &str) -> Result<libc::c_int> {
    i32::try_from(value)
        .map_err(|_| SubtitleError::Libass(format!("{label} exceeds libass integer range")))
}

#[cfg(feature = "libass")]
unsafe fn import_libass_image_list(
    pts: Duration,
    frame_width: u32,
    frame_height: u32,
    first: *mut libass_ffi::AssImage,
    changed: bool,
) -> Result<SubtitleBitmapSet> {
    let mut set = SubtitleBitmapSet::new(pts, frame_width, frame_height)
        .with_color_space(SubtitleBitmapColorSpace::Video)
        .with_changed(changed);
    let mut current = first;
    let mut count = 0usize;

    while !current.is_null() {
        if count >= RAW_ASS_IMAGE_LIST_LIMIT {
            return Err(SubtitleError::BitmapListTooLong);
        }
        let image = unsafe { &*current };
        if image.w > 0 && image.h > 0 {
            let raw = RawAssImage {
                w: image.w,
                h: image.h,
                stride: image.stride,
                bitmap: image.bitmap.cast_const(),
                color: image.color,
                dst_x: image.dst_x,
                dst_y: image.dst_y,
                next: std::ptr::null(),
                image_type: image.image_type,
            };
            set.push(unsafe { raw_ass_image_to_alpha_bitmap(&raw)? });
        }
        current = image.next;
        count += 1;
    }

    Ok(set)
}

pub type SubtitleViewport = SubtitleRenderViewport;
pub type SubtitleRendererBackend = SubtitleRenderBackend;

impl SubtitleFrame {
    pub fn from_ass_bitmaps<'a>(
        pts: Duration,
        bitmaps: impl IntoIterator<Item = &'a AssBitmapPlane>,
    ) -> Result<Self> {
        let mut set = SubtitleBitmapSet::new(pts, 1, 1);
        for bitmap in bitmaps {
            if let Some(part) = bitmap.as_alpha_bitmap()? {
                set.push(part);
            }
        }
        Ok(set.to_frame())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleFrameChange {
    Changed,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleRenderResult {
    pub backend: SubtitleRendererBackend,
    pub change: SubtitleFrameChange,
    pub frame: SubtitleFrame,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SubtitleFrameSignature {
    viewport: SubtitleViewport,
    active_cues: Vec<SubtitleCue>,
}

#[derive(Debug, Clone)]
pub struct SubtitleRendererCore {
    timeline: SubtitleTimeline,
    last_signature: Option<SubtitleFrameSignature>,
}

impl SubtitleRendererCore {
    pub fn new_debug(timeline: SubtitleTimeline) -> Self {
        Self {
            timeline,
            last_signature: None,
        }
    }

    pub fn timeline(&self) -> &SubtitleTimeline {
        &self.timeline
    }

    pub fn render(&mut self, pts: Duration, viewport: SubtitleViewport) -> SubtitleRenderResult {
        let active_cues = self
            .timeline
            .active_cues(pts)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let signature = SubtitleFrameSignature {
            viewport,
            active_cues,
        };
        let change = if self.last_signature.as_ref() == Some(&signature) {
            SubtitleFrameChange::Unchanged
        } else {
            SubtitleFrameChange::Changed
        };
        self.last_signature = Some(signature);
        SubtitleRenderResult {
            backend: SubtitleRenderBackend::DebugTimeline,
            change,
            frame: self
                .timeline
                .render_debug_frame(pts, viewport.width, viewport.height),
        }
    }

    pub fn render_ass_bitmaps<'a>(
        pts: Duration,
        bitmaps: impl IntoIterator<Item = &'a AssBitmapPlane>,
    ) -> Result<SubtitleRenderResult> {
        Ok(SubtitleRenderResult {
            backend: SubtitleRenderBackend::Libass,
            change: SubtitleFrameChange::Changed,
            frame: SubtitleFrame::from_ass_bitmaps(pts, bitmaps)?,
        })
    }
}

impl SubtitleRenderer for SubtitleRendererCore {
    fn backend(&self) -> SubtitleRenderBackend {
        SubtitleRenderBackend::DebugTimeline
    }

    fn render(&mut self, request: SubtitleRenderRequest) -> Result<SubtitleRenderOutput> {
        Ok(SubtitleRenderOutput::Rgba(
            self.render(request.pts, request.viewport).frame,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl AssColor {
    pub fn from_libass_rgba(color: u32) -> Self {
        Self {
            red: ((color >> 24) & 0xff) as u8,
            green: ((color >> 16) & 0xff) as u8,
            blue: ((color >> 8) & 0xff) as u8,
            alpha: (0xff - (color & 0xff)) as u8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssBitmapPlane {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub color: u32,
    pub alpha: Vec<u8>,
}

impl AssBitmapPlane {
    pub fn new(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        stride: usize,
        color: u32,
        alpha: Vec<u8>,
    ) -> Result<Self> {
        validate_ass_bitmap(width, height, stride, alpha.len())?;
        Ok(Self {
            x,
            y,
            width,
            height,
            stride,
            color,
            alpha,
        })
    }

    pub fn to_rgba_plane(&self) -> Result<SubtitleBitmapPlane> {
        self.as_alpha_bitmap()?
            .and_then(|bitmap| bitmap.to_rgba_plane())
            .ok_or_else(|| SubtitleError::InvalidBitmap {
                width: self.width,
                height: self.height,
                stride: self.stride,
                bytes: self.alpha.len(),
            })
    }

    pub fn as_alpha_bitmap(&self) -> Result<Option<SubtitleAlphaBitmap>> {
        validate_ass_bitmap(self.width, self.height, self.stride, self.alpha.len())?;
        if self.width == 0 || self.height == 0 {
            return Ok(None);
        }
        Ok(Some(SubtitleAlphaBitmap::new(
            SubtitleBitmapPlacement::new(self.x, self.y, self.width, self.height),
            self.stride,
            self.color,
            self.alpha.clone(),
        )))
    }
}

#[derive(Debug, Clone, Default)]
pub struct SubtitleTimeline {
    cues: Vec<SubtitleCue>,
}

impl SubtitleTimeline {
    pub fn new(cues: Vec<SubtitleCue>) -> Self {
        let mut timeline = Self { cues };
        timeline.cues.sort_by_key(|cue| cue.start);
        timeline
    }

    pub fn cues(&self) -> &[SubtitleCue] {
        &self.cues
    }

    pub fn active_cues(&self, pts: Duration) -> Vec<&SubtitleCue> {
        self.cues
            .iter()
            .filter(|cue| cue.start <= pts && pts < cue.end)
            .collect()
    }

    pub fn render_debug_frame(&self, pts: Duration, width: u32, height: u32) -> SubtitleFrame {
        let active = self.active_cues(pts);
        let mut planes = Vec::new();
        for (index, cue) in active.iter().enumerate() {
            let text_width = (cue.text.chars().count() as u32).saturating_mul(10).max(16);
            let plane_width = text_width.min(width.max(1));
            let plane_height = 28u32.min(height.max(1));
            let x = ((width.saturating_sub(plane_width)) / 2) as i32;
            let y = height
                .saturating_sub(plane_height.saturating_mul(index as u32 + 1))
                .saturating_sub(24) as i32;
            planes.push(SubtitleBitmapPlane::new(
                x,
                y,
                plane_width,
                plane_height,
                debug_rgba_plane(plane_width, plane_height),
            ));
        }
        SubtitleFrame { pts, planes }
    }
}

pub fn parse_srt(input: &str) -> Result<SubtitleTimeline> {
    let mut cues = Vec::new();
    for block in input.replace("\r\n", "\n").split("\n\n") {
        let lines = block
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>();
        if lines.is_empty() {
            continue;
        }
        let time_line_index = usize::from(!lines[0].contains("-->"));
        let Some(time_line) = lines.get(time_line_index) else {
            continue;
        };
        if !time_line.contains("-->") {
            continue;
        }
        let text = lines[time_line_index + 1..].join("\n");
        cues.push(parse_timed_text_cue(time_line, &text)?);
    }
    Ok(SubtitleTimeline::new(cues))
}

pub fn parse_webvtt(input: &str) -> Result<SubtitleTimeline> {
    let normalized = input.replace("\r\n", "\n");
    let without_header = normalized.strip_prefix("WEBVTT").unwrap_or(&normalized);
    parse_srt(without_header)
}

pub fn parse_ass_events(input: &str) -> Result<SubtitleTimeline> {
    let mut in_events = false;
    let mut format_fields = Vec::new();
    let mut cues = Vec::new();

    for line in input.lines() {
        let line = line.trim();
        if line.eq_ignore_ascii_case("[events]") {
            in_events = true;
            continue;
        }
        if !in_events {
            continue;
        }
        if let Some(format) = line.strip_prefix("Format:") {
            format_fields = format
                .split(',')
                .map(|field| field.trim().to_ascii_lowercase())
                .collect();
            continue;
        }
        if let Some(dialogue) = line.strip_prefix("Dialogue:") {
            let field_count = format_fields.len().max(10);
            let values = dialogue
                .splitn(field_count, ',')
                .map(str::trim)
                .collect::<Vec<_>>();
            let index = |name: &str, default: usize| {
                format_fields
                    .iter()
                    .position(|field| field == name)
                    .unwrap_or(default)
            };
            let start = values
                .get(index("start", 1))
                .ok_or(SubtitleError::InvalidCue)
                .and_then(|value| parse_timestamp(value))?;
            let end = values
                .get(index("end", 2))
                .ok_or(SubtitleError::InvalidCue)
                .and_then(|value| parse_timestamp(value))?;
            let text = values
                .get(index("text", 9))
                .map(|value| clean_ass_text(value))
                .unwrap_or_default();
            cues.push(SubtitleCue { start, end, text });
        }
    }

    Ok(SubtitleTimeline::new(cues))
}

pub fn parse_subtitle_text_file(
    format: SubtitleFileFormat,
    input: &str,
) -> Result<SubtitleTimeline> {
    match format {
        SubtitleFileFormat::Srt => parse_srt(input),
        SubtitleFileFormat::WebVtt => parse_webvtt(input),
        SubtitleFileFormat::Ass => parse_ass_events(input),
    }
}

pub fn decoded_subtitle_frames_to_timeline<'a>(
    frames: impl IntoIterator<Item = &'a DecodedSubtitleFrame>,
    fallback_end: Duration,
) -> SubtitleTimeline {
    let cues = frames
        .into_iter()
        .flat_map(|frame| frame.text_cues(fallback_end))
        .collect();
    SubtitleTimeline::new(cues)
}

pub fn decoded_subtitle_frames_to_ass_script<'a>(
    frames: impl IntoIterator<Item = &'a DecodedSubtitleFrame>,
    fallback_end: Duration,
) -> Option<String> {
    decoded_subtitle_frames_to_ass_script_with_style(
        frames,
        fallback_end,
        SubtitleAssStyle::default(),
    )
}

pub fn decoded_subtitle_frames_to_ass_script_with_style<'a>(
    frames: impl IntoIterator<Item = &'a DecodedSubtitleFrame>,
    fallback_end: Duration,
    style: SubtitleAssStyle,
) -> Option<String> {
    let mut events = String::new();
    for frame in frames {
        if !frame.has_text() {
            continue;
        }
        let start = frame.start.unwrap_or(Duration::ZERO);
        let end = frame.end.unwrap_or(fallback_end).max(start);
        for segment in &frame.text {
            if segment.text.trim().is_empty() {
                continue;
            }
            match segment.format {
                SubtitleTextFormat::Ass if is_ass_dialogue_line(&segment.text) => {
                    events.push_str(segment.text.trim());
                    events.push('\n');
                }
                SubtitleTextFormat::Ass => {
                    let text = ass_segment_display_text(&segment.text);
                    if !text.is_empty() {
                        push_ass_dialogue(&mut events, start, end, &text);
                    }
                }
                SubtitleTextFormat::PlainText => {
                    let text = segment.text.trim();
                    if !text.is_empty() {
                        push_ass_dialogue(&mut events, start, end, text);
                    }
                }
            }
        }
    }

    if events.is_empty() {
        return None;
    }

    let mut script = default_ass_script_header(style);
    script.push_str(&events);
    Some(script)
}

fn parse_timed_text_cue(time_line: &str, text: &str) -> Result<SubtitleCue> {
    let (start, end) = time_line
        .split_once("-->")
        .ok_or(SubtitleError::InvalidCue)?;
    let start = parse_timestamp(start.trim())?;
    let end_part = end.split_whitespace().next().unwrap_or(end).trim();
    let end = parse_timestamp(end_part)?;
    Ok(SubtitleCue {
        start,
        end,
        text: text.trim().to_string(),
    })
}

fn parse_timestamp(value: &str) -> Result<Duration> {
    let value = value.trim().replace(',', ".");
    let parts = value.split(':').collect::<Vec<_>>();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [minutes, seconds] => (0u64, parse_int(minutes)?, parse_seconds(seconds)?),
        [hours, minutes, seconds] => (
            parse_int(hours)?,
            parse_int(minutes)?,
            parse_seconds(seconds)?,
        ),
        _ => return Err(SubtitleError::InvalidTimestamp(value)),
    };
    Ok(Duration::from_secs(hours * 3600 + minutes * 60) + seconds)
}

fn parse_int(value: &str) -> Result<u64> {
    value
        .parse::<u64>()
        .map_err(|_| SubtitleError::InvalidTimestamp(value.to_string()))
}

fn parse_seconds(value: &str) -> Result<Duration> {
    let seconds = value
        .parse::<f64>()
        .map_err(|_| SubtitleError::InvalidTimestamp(value.to_string()))?;
    Ok(Duration::from_secs_f64(seconds))
}

fn clean_ass_text(value: &str) -> String {
    let mut output = String::new();
    let mut in_override = false;
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '{' => in_override = true,
            '}' => in_override = false,
            _ if in_override => {}
            '\\' => match chars.peek().copied() {
                Some('N') | Some('n') => {
                    let _ = chars.next();
                    output.push('\n');
                }
                _ => output.push(ch),
            },
            _ => output.push(ch),
        }
    }
    output.trim().to_string()
}

fn ass_segment_display_text(value: &str) -> String {
    let value = value.trim();
    if let Some(dialogue) = strip_ass_dialogue_prefix(value) {
        let fields = dialogue.splitn(10, ',').collect::<Vec<_>>();
        return fields
            .get(9)
            .map(|text| clean_ass_text(text))
            .unwrap_or_default();
    }
    let fields = value.splitn(9, ',').collect::<Vec<_>>();
    if fields.len() == 9 && fields[0].trim().parse::<i64>().is_ok() {
        return clean_ass_text(fields[8]);
    }
    clean_ass_text(value)
}

fn is_ass_dialogue_line(value: &str) -> bool {
    strip_ass_dialogue_prefix(value).is_some()
}

fn strip_ass_dialogue_prefix(value: &str) -> Option<&str> {
    let value = value.trim_start();
    let (prefix, rest) = value.split_once(':')?;
    prefix
        .eq_ignore_ascii_case("Dialogue")
        .then_some(rest.trim_start())
}

fn push_ass_dialogue(output: &mut String, start: Duration, end: Duration, text: &str) {
    output.push_str("Dialogue: 0,");
    output.push_str(&format_ass_timestamp(start));
    output.push(',');
    output.push_str(&format_ass_timestamp(end));
    output.push_str(",Default,,0,0,0,,");
    output.push_str(&escape_ass_text(text));
    output.push('\n');
}

fn format_ass_timestamp(value: Duration) -> String {
    let centiseconds = value.as_millis().saturating_add(5) / 10;
    let seconds_total = centiseconds / 100;
    let hours = seconds_total / 3600;
    let minutes = (seconds_total / 60) % 60;
    let seconds = seconds_total % 60;
    let centiseconds = centiseconds % 100;
    format!("{hours}:{minutes:02}:{seconds:02}.{centiseconds:02}")
}

fn escape_ass_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\n' => output.push_str("\\N"),
            '\\' => output.push_str("\\\\"),
            '{' => output.push_str("\\{"),
            '}' => output.push_str("\\}"),
            _ => output.push(ch),
        }
    }
    output
}

const DEFAULT_ASS_FONT_SIZE: f64 = 48.0;
const DEFAULT_ASS_OUTLINE: f64 = 2.0;

fn normalize_ass_font_scale(scale: f64) -> f64 {
    if scale.is_finite() {
        scale.clamp(0.25, 4.0)
    } else {
        1.0
    }
}

fn ass_number(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded.fract().abs() < 0.001 {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.2}")
    }
}

fn default_ass_script_header(style: SubtitleAssStyle) -> String {
    let scale = normalize_ass_font_scale(style.font_scale);
    let font_size = ass_number(DEFAULT_ASS_FONT_SIZE * scale);
    let outline = ass_number(DEFAULT_ASS_OUTLINE * scale);
    let play_res_width = style.play_res_width.max(1);
    let play_res_height = style.play_res_height.max(1);
    format!(
        r#"[Script Info]
ScriptType: v4.00+
PlayResX: {play_res_width}
PlayResY: {play_res_height}

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,{font_size},&H00FFFFFF,&H000000FF,&H80000000,&H80000000,0,0,0,0,100,100,0,0,1,{outline},0,2,48,48,54,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
"#
    )
}

fn debug_rgba_plane(width: u32, height: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[255, 255, 255, 220]);
    }
    rgba
}

fn validate_ass_bitmap(width: u32, height: u32, stride: usize, bytes: usize) -> Result<()> {
    let width = width as usize;
    let height = height as usize;
    if width == 0 || height == 0 {
        return Ok(());
    }
    let required = stride
        .checked_mul(height.saturating_sub(1))
        .and_then(|prefix| prefix.checked_add(width))
        .unwrap_or(usize::MAX);
    if stride < width || bytes < required {
        return Err(SubtitleError::InvalidBitmap {
            width: width.min(u32::MAX as usize) as u32,
            height: height.min(u32::MAX as usize) as u32,
            stride,
            bytes,
        });
    }
    Ok(())
}

fn multiply_u8(a: u8, b: u8) -> u8 {
    ((a as u16 * b as u16 + 127) / 255) as u8
}

const RAW_ASS_IMAGE_LIST_LIMIT: usize = 16_384;

unsafe fn raw_ass_image_to_alpha_bitmap(image: &RawAssImage) -> Result<SubtitleAlphaBitmap> {
    if image.bitmap.is_null() {
        return Err(SubtitleError::NullBitmap);
    }
    let width = image.w as u32;
    let height = image.h as u32;
    let stride = image.stride.max(0) as usize;
    let required = required_bitmap_len(width, height, stride)?;
    let alpha = unsafe { std::slice::from_raw_parts(image.bitmap, required) }.to_vec();
    Ok(SubtitleAlphaBitmap::new(
        SubtitleBitmapPlacement::new(image.dst_x, image.dst_y, width, height),
        stride,
        image.color,
        alpha,
    ))
}

fn required_bitmap_len(width: u32, height: u32, stride: usize) -> Result<usize> {
    validate_ass_bitmap(width, height, stride, usize::MAX)?;
    let height = height as usize;
    if width == 0 || height == 0 {
        return Ok(0);
    }
    Ok(stride * height.saturating_sub(1) + width as usize)
}

fn duration_to_millis_i64(value: Duration) -> i64 {
    value.as_millis().min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "libass")]
    const SIMPLE_ASS_SCRIPT: &str = r#"[Script Info]
ScriptType: v4.00+
PlayResX: 640
PlayResY: 360

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,32,&H00FFFFFF,&H000000FF,&H80000000,&H80000000,0,0,0,0,100,100,0,0,1,2,0,2,20,20,24,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,Hello libass
"#;

    #[test]
    fn parses_srt_and_finds_active_cue() {
        let srt =
            "1\n00:00:01,000 --> 00:00:03,500\nHello\n\n2\n00:00:04,000 --> 00:00:05,000\nWorld\n";
        let timeline = parse_srt(srt).unwrap();

        assert_eq!(timeline.cues().len(), 2);
        assert_eq!(
            timeline.active_cues(Duration::from_millis(1500))[0].text,
            "Hello"
        );
        assert!(timeline.active_cues(Duration::from_millis(3500)).is_empty());
    }

    #[test]
    fn subtitle_file_format_detects_text_subtitle_extensions() {
        assert_eq!(
            SubtitleFileFormat::from_path("/tmp/movie.en.srt"),
            Some(SubtitleFileFormat::Srt)
        );
        assert_eq!(
            SubtitleFileFormat::from_path("/tmp/movie.VTT"),
            Some(SubtitleFileFormat::WebVtt)
        );
        assert_eq!(
            SubtitleFileFormat::from_path("/tmp/movie.ass"),
            Some(SubtitleFileFormat::Ass)
        );
        assert_eq!(SubtitleFileFormat::from_path("/tmp/movie.sup"), None);
    }

    #[test]
    fn bitmap_plane_scales_from_subtitle_canvas_to_video_viewport() {
        let plane = SubtitleBitmapPlane::new(800, 905, 322, 60, Vec::new()).with_canvas(1920, 1080);

        assert_eq!(plane.scaled_rect(3840, 1816), (1600, 1638, 644, 120));
    }

    #[test]
    fn parse_subtitle_text_file_dispatches_by_format() {
        let srt = "1\n00:00:01,000 --> 00:00:02,000\nHello\n";
        let timeline = parse_subtitle_text_file(SubtitleFileFormat::Srt, srt).unwrap();

        assert_eq!(timeline.cues().len(), 1);
        assert_eq!(timeline.cues()[0].text, "Hello");
    }

    #[test]
    fn parses_ass_dialogue() {
        let ass = "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,{\\i1}Hi\\Nthere";
        let timeline = parse_ass_events(ass).unwrap();

        assert_eq!(timeline.cues().len(), 1);
        assert_eq!(timeline.cues()[0].text, "Hi\nthere");
    }

    #[test]
    fn decoded_plain_text_subtitle_can_become_ass_script() {
        let mut frame = DecodedSubtitleFrame::new(
            2,
            Some(Duration::from_millis(1250)),
            Some(Duration::from_millis(2500)),
        );
        frame.push_text(SubtitleTextSegment::new(
            SubtitleTextFormat::PlainText,
            "hello\nworld",
        ));

        let script = frame.to_ass_script(Duration::from_secs(5)).unwrap();

        assert!(script.contains("[Script Info]"));
        assert!(script.contains("Dialogue: 0,0:00:01.25,0:00:02.50"));
        assert!(script.contains("hello\\Nworld"));
    }

    #[test]
    fn decoded_plain_text_subtitle_ass_script_honors_font_scale() {
        let mut frame = DecodedSubtitleFrame::new(
            2,
            Some(Duration::from_millis(1250)),
            Some(Duration::from_millis(2500)),
        );
        frame.push_text(SubtitleTextSegment::new(
            SubtitleTextFormat::PlainText,
            "scaled",
        ));

        let script = decoded_subtitle_frames_to_ass_script_with_style(
            [&frame],
            Duration::from_secs(5),
            SubtitleAssStyle {
                font_scale: 1.5,
                ..SubtitleAssStyle::default()
            },
        )
        .unwrap();

        assert!(script.contains("Style: Default,Arial,72,"));
    }

    #[test]
    fn decoded_plain_text_subtitle_ass_script_uses_render_viewport_as_play_res() {
        let mut frame = DecodedSubtitleFrame::new(
            2,
            Some(Duration::from_millis(1250)),
            Some(Duration::from_millis(2500)),
        );
        frame.push_text(SubtitleTextSegment::new(
            SubtitleTextFormat::PlainText,
            "wide",
        ));

        let script = decoded_subtitle_frames_to_ass_script_with_style(
            [&frame],
            Duration::from_secs(5),
            SubtitleAssStyle {
                play_res_width: 1920,
                play_res_height: 816,
                ..SubtitleAssStyle::default()
            },
        )
        .unwrap();

        assert!(script.contains("PlayResX: 1920"));
        assert!(script.contains("PlayResY: 816"));
    }

    #[test]
    fn decoded_ass_dialogue_preserves_event_line_for_libass() {
        let mut frame = DecodedSubtitleFrame::new(
            2,
            Some(Duration::from_secs(1)),
            Some(Duration::from_secs(2)),
        );
        frame.push_text(SubtitleTextSegment::new(
            SubtitleTextFormat::Ass,
            "Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,{\\i1}Hi",
        ));

        let script = frame.to_ass_script(Duration::from_secs(5)).unwrap();

        assert!(script.contains("Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,{\\i1}Hi"));
        assert_eq!(frame.text[0].display_text(), "Hi");
    }

    #[test]
    fn decoded_lavc_ass_payload_uses_payload_text_field() {
        let segment = SubtitleTextSegment::new(
            SubtitleTextFormat::Ass,
            "0,0,Default,,0,0,0,,External subtitle",
        );

        assert_eq!(segment.display_text(), "External subtitle");
    }

    #[test]
    fn bitmap_plane_scales_to_matching_aspect_video_viewport() {
        let plane = SubtitleBitmapPlane::new(800, 905, 322, 60, Vec::new()).with_canvas(1920, 1080);

        assert_eq!(plane.scaled_rect(3840, 2160), (1600, 1810, 644, 120));
    }

    #[test]
    fn decoded_text_frames_can_become_debug_timeline() {
        let mut frame = DecodedSubtitleFrame::new(
            2,
            Some(Duration::from_secs(1)),
            Some(Duration::from_secs(3)),
        );
        frame.push_text(SubtitleTextSegment::new(
            SubtitleTextFormat::Ass,
            "{\\b1}Hello",
        ));

        let timeline = decoded_subtitle_frames_to_timeline([&frame], Duration::from_secs(5));

        assert_eq!(timeline.cues().len(), 1);
        assert_eq!(timeline.cues()[0].text, "Hello");
        assert_eq!(timeline.cues()[0].start, Duration::from_secs(1));
        assert_eq!(timeline.cues()[0].end, Duration::from_secs(3));
    }

    #[test]
    fn debug_frame_produces_rgba_planes() {
        let timeline = SubtitleTimeline::new(vec![SubtitleCue {
            start: Duration::from_secs(1),
            end: Duration::from_secs(3),
            text: "Hello".to_string(),
        }]);

        let frame = timeline.render_debug_frame(Duration::from_secs(2), 640, 360);

        assert_eq!(frame.planes.len(), 1);
        assert_eq!(
            frame.planes[0].rgba.len(),
            frame.planes[0].width as usize * frame.planes[0].height as usize * 4
        );
    }

    #[test]
    fn ass_color_decodes_libass_inverse_alpha() {
        assert_eq!(
            AssColor::from_libass_rgba(0x11223300),
            AssColor {
                red: 0x11,
                green: 0x22,
                blue: 0x33,
                alpha: 0xff,
            }
        );
        assert_eq!(AssColor::from_libass_rgba(0x112233ff).alpha, 0);
    }

    #[test]
    fn ass_bitmap_plane_expands_alpha_mask_to_straight_rgba() {
        let bitmap =
            AssBitmapPlane::new(3, 4, 2, 2, 4, 0x20406080, vec![0, 255, 9, 9, 128, 64]).unwrap();

        let plane = bitmap.to_rgba_plane().unwrap();

        assert_eq!(plane.x, 3);
        assert_eq!(plane.y, 4);
        assert_eq!(plane.width, 2);
        assert_eq!(plane.height, 2);
        assert_eq!(
            plane.rgba,
            vec![
                0x20, 0x40, 0x60, 0, 0x20, 0x40, 0x60, 127, 0x20, 0x40, 0x60, 64, 0x20, 0x40, 0x60,
                32,
            ]
        );
    }

    #[test]
    fn subtitle_alpha_bitmap_expands_libass_color_and_coverage() {
        let bitmap = SubtitleAlphaBitmap::new(
            SubtitleBitmapPlacement::new(2, 3, 2, 1),
            2,
            0x12345680,
            vec![255, 128],
        );

        let plane = bitmap.to_rgba_plane().unwrap();

        assert_eq!(plane.x, 2);
        assert_eq!(plane.y, 3);
        assert_eq!(
            plane.rgba,
            vec![0x12, 0x34, 0x56, 127, 0x12, 0x34, 0x56, 64]
        );
    }

    #[test]
    fn subtitle_bitmap_set_converts_alpha_parts_to_rgba_frame() {
        let mut set = SubtitleBitmapSet::new(Duration::from_secs(7), 640, 360)
            .with_color_space(SubtitleBitmapColorSpace::Video)
            .with_changed(false);
        set.push(SubtitleAlphaBitmap::new(
            SubtitleBitmapPlacement::new(0, 0, 1, 1),
            1,
            0x00ff0000,
            vec![255],
        ));

        let frame = SubtitleRenderOutput::Alpha(set).into_rgba_frame();

        assert_eq!(frame.pts, Duration::from_secs(7));
        assert_eq!(frame.planes.len(), 1);
        assert_eq!(frame.planes[0].rgba, vec![0, 255, 0, 255]);
    }

    #[test]
    fn subtitle_track_source_distinguishes_embedded_and_external_removal() {
        let embedded = SubtitleTrackConfig::embedded(2, 2);
        let external = SubtitleTrackConfig::external(100, "/tmp/subs.ass");

        assert!(embedded.source.is_embedded());
        assert!(!embedded.can_remove());
        assert!(external.source.is_external());
        assert!(external.can_remove());
    }

    #[test]
    fn ass_bitmap_validation_accepts_unpadded_last_row() {
        let bitmap = AssBitmapPlane::new(0, 0, 3, 2, 8, 0xffffff00, vec![0; 11]).unwrap();

        assert_eq!(bitmap.to_rgba_plane().unwrap().rgba.len(), 24);
    }

    #[test]
    fn ass_bitmap_validation_rejects_short_alpha_buffer() {
        let error = AssBitmapPlane::new(0, 0, 3, 2, 8, 0xffffff00, vec![0; 10]).unwrap_err();

        assert!(matches!(error, SubtitleError::InvalidBitmap { .. }));
    }

    #[test]
    fn subtitle_renderer_core_reports_unchanged_timeline_frame() {
        let timeline = SubtitleTimeline::new(vec![SubtitleCue {
            start: Duration::from_secs(1),
            end: Duration::from_secs(3),
            text: "Hello".to_string(),
        }]);
        let mut renderer = SubtitleRendererCore::new_debug(timeline);
        let viewport = SubtitleViewport::new(640, 360);

        let first = renderer.render(Duration::from_secs(2), viewport);
        let second = renderer.render(Duration::from_millis(2500), viewport);

        assert_eq!(first.change, SubtitleFrameChange::Changed);
        assert_eq!(second.change, SubtitleFrameChange::Unchanged);
        assert_eq!(second.frame.planes.len(), 1);
    }

    #[test]
    fn subtitle_renderer_core_converts_ass_bitmaps() {
        let bitmap = AssBitmapPlane::new(0, 0, 1, 1, 1, 0xff000000, vec![255]).unwrap();
        let result =
            SubtitleRendererCore::render_ass_bitmaps(Duration::from_secs(1), [&bitmap]).unwrap();

        assert_eq!(result.backend, SubtitleRendererBackend::Libass);
        assert_eq!(result.frame.planes.len(), 1);
        assert_eq!(result.frame.planes[0].rgba, vec![255, 0, 0, 255]);
    }

    #[test]
    fn libass_render_plan_keeps_renderer_operation_order() {
        let request = SubtitleRenderRequest {
            pts: Duration::from_millis(1234),
            viewport: SubtitleRenderViewport {
                width: 1920,
                height: 1080,
                storage_width: 3840,
                storage_height: 2160,
            },
        };
        let plan = LibassRenderPlan::new(
            request,
            LibassRenderConfig {
                glyph_cache_limit: 128,
                bitmap_cache_limit_mb: 32,
            },
        );

        assert_eq!(
            plan.operations,
            vec![
                LibassRenderOperation::SetFrameSize {
                    width: 1920,
                    height: 1080,
                },
                LibassRenderOperation::SetStorageSize {
                    width: 3840,
                    height: 2160,
                },
                LibassRenderOperation::SetCacheLimits {
                    glyphs: 128,
                    bitmap_mb: 32,
                },
                LibassRenderOperation::RenderFrame { timestamp_ms: 1234 },
            ]
        );
    }

    #[test]
    fn raw_ass_image_list_imports_alpha_bitmaps() {
        let alpha = [255u8, 128, 64, 0];
        let image = RawAssImage {
            w: 2,
            h: 2,
            stride: 2,
            bitmap: alpha.as_ptr(),
            color: 0x80402000,
            dst_x: 10,
            dst_y: 20,
            next: std::ptr::null(),
            image_type: 0,
        };

        let set = unsafe {
            LibassImageImporter::import_raw_list(Duration::from_secs(1), 1920, 1080, &image, true)
        }
        .unwrap();

        assert_eq!(set.parts.len(), 1);
        let plane = set.to_frame().planes.remove(0);
        assert_eq!(plane.x, 10);
        assert_eq!(plane.y, 20);
        assert_eq!(plane.rgba[0..4], [0x80, 0x40, 0x20, 255]);
        assert_eq!(plane.rgba[4..8], [0x80, 0x40, 0x20, 128]);
    }

    #[cfg(feature = "libass")]
    #[test]
    fn libass_renderer_rejects_empty_script() {
        let error =
            LibassSubtitleRenderer::from_ass_script("", LibassRenderConfig::default()).unwrap_err();

        assert!(matches!(error, SubtitleError::Libass(message) if message.contains("empty")));
    }

    #[cfg(feature = "libass")]
    #[test]
    fn libass_renderer_renders_ass_script_to_alpha_bitmaps() {
        let config = LibassRenderConfig {
            glyph_cache_limit: 64,
            bitmap_cache_limit_mb: 16,
        };
        let mut renderer =
            LibassSubtitleRenderer::from_ass_script(SIMPLE_ASS_SCRIPT, config).unwrap();
        let request = SubtitleRenderRequest::new(Duration::from_millis(500), 640, 360);

        assert_eq!(renderer.backend(), SubtitleRenderBackend::Libass);
        assert_eq!(renderer.config(), config);
        assert_eq!(
            renderer.render_plan(request).operations,
            LibassRenderPlan::new(request, config).operations
        );

        let output = renderer.render(request).unwrap();
        let SubtitleRenderOutput::Alpha(bitmaps) = output else {
            panic!("libass renderer should produce alpha bitmap output");
        };

        assert_eq!(bitmaps.pts, request.pts);
        assert_eq!(bitmaps.frame_width, 640);
        assert_eq!(bitmaps.frame_height, 360);
        assert_eq!(bitmaps.color_space, SubtitleBitmapColorSpace::Video);
        assert!(!bitmaps.parts.is_empty());
        assert!(bitmaps.parts.iter().all(SubtitleAlphaBitmap::is_valid));
    }
}
