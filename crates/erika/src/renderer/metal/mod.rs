use std::ffi::c_void;
use std::time::Duration;

use crate::core::{
    ColorPrimaries, LumaUpscalerBackendStatus, PlatformSurface, PlayerError, PlayerVideoFrame,
    RenderFrameContext, RendererBackend, RendererRuntimeStats, Result, TransferFunction,
};
use crate::danmaku::DanmakuRenderPlan;
use crate::ffmpeg::Frame;
use crate::overlay::OverlayFrame;
pub use crate::renderer::pipeline::LumaUpscalerMode;
use crate::renderer::pipeline::{
    ColorRange, HdrMetadata, MatrixCoefficients, SourceColorState, VideoRenderPipeline,
};

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple;
// Public for integration tests (numeric verification against ONNX
// references); not part of the stable API surface.
#[doc(hidden)]
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub mod upscaler;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClearColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

impl ClearColor {
    pub fn animated(time_seconds: f64) -> Self {
        Self {
            red: time_seconds.sin() * 0.5 + 0.5,
            green: (time_seconds * 0.73).sin() * 0.5 + 0.5,
            blue: (time_seconds * 1.37).cos() * 0.5 + 0.5,
            alpha: 1.0,
        }
    }
}

pub struct MetalRenderer {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    inner: apple::MetalRendererImpl,
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    _unsupported: (),
    current_frame: Option<ImportedVideoFrame>,
    current_media_time: Duration,
    current_generation: u64,
    upload_counter: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetalRendererConfig {
    pub output_mode: MetalOutputMode,
    pub luma_upscaler: LumaUpscalerMode,
}

impl Default for MetalRendererConfig {
    fn default() -> Self {
        Self {
            output_mode: MetalOutputMode::default(),
            luma_upscaler: LumaUpscalerMode::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MetalOutputMode {
    Sdr,
    AppleEdr { headroom: f32 },
}

impl MetalOutputMode {
    pub fn apple_edr(headroom: f32) -> Self {
        Self::AppleEdr {
            headroom: headroom.max(1.0),
        }
    }

    pub fn pixel_format(self) -> MetalDrawablePixelFormat {
        match self {
            Self::Sdr => MetalDrawablePixelFormat::Bgra8Unorm,
            Self::AppleEdr { .. } => MetalDrawablePixelFormat::Rgba16Float,
        }
    }

    pub fn is_edr(self) -> bool {
        matches!(self, Self::AppleEdr { .. })
    }

    pub fn target_color(self) -> crate::renderer::pipeline::TargetColorState {
        self.target_color_for_source(SourceColorState::default())
    }

    pub fn target_color_for_source(
        self,
        source: SourceColorState,
    ) -> crate::renderer::pipeline::TargetColorState {
        match self {
            Self::Sdr => crate::renderer::pipeline::TargetColorState::sdr(ColorPrimaries::Bt709),
            Self::AppleEdr { headroom } => {
                #[cfg(target_os = "ios")]
                {
                    let _ = source;
                    let headroom = headroom.max(1.0);
                    return crate::renderer::pipeline::TargetColorState {
                        primaries: ColorPrimaries::Bt709,
                        transfer: TransferFunction::Srgb,
                        peak_nits: 100.0 * headroom,
                        reference_white_nits: 100.0,
                        edr_headroom: headroom,
                    };
                }

                #[cfg(not(target_os = "ios"))]
                {
                    let primaries = match (source.transfer, source.primaries) {
                        (TransferFunction::Pq, ColorPrimaries::Unknown) => ColorPrimaries::Bt2020,
                        (TransferFunction::Pq, primaries) => primaries,
                        _ => ColorPrimaries::Bt709,
                    };
                    let mut target =
                        crate::renderer::pipeline::TargetColorState::apple_edr(primaries, headroom);
                    if matches!(source.transfer, TransferFunction::Pq) {
                        target.transfer = TransferFunction::Pq;
                        target.peak_nits = 10_000.0;
                        target.reference_white_nits = 203.0;
                    }
                    target
                }
            }
        }
    }
}

impl Default for MetalOutputMode {
    fn default() -> Self {
        Self::Sdr
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetalDrawablePixelFormat {
    Bgra8Unorm,
    Rgba16Float,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MetalRendererStats {
    pub drawable_width: u32,
    pub drawable_height: u32,
    pub rendered_frames: u64,
    pub prepared_overlay_frames: u64,
    pub prepared_overlay_subtitle_planes: u64,
    pub danmaku_passes: u64,
    pub danmaku_items: u64,
    pub overlay_alpha_atlas_uploads: u64,
    pub overlay_alpha_atlas_reuses: u64,
    pub last_danmaku_atlas_duration: Duration,
    pub last_danmaku_vertex_build_duration: Duration,
    pub last_danmaku_vertex_copy_duration: Duration,
    pub last_danmaku_encode_duration: Duration,
    pub last_danmaku_vertex_bytes: usize,
    pub last_danmaku_vertex_count: usize,
    pub upscaler_mode: LumaUpscalerMode,
    pub upscaler_backend: LumaUpscalerBackendStatus,
    pub upscaler_fallbacks: u64,
    pub upscaled_frames: u64,
    pub last_upscaler_encode_duration: Duration,
    pub last_gpu_duration: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoFrameTextureSource {
    pub raw_pixel_buffer: *mut c_void,
    pub width: u32,
    pub height: u32,
}

impl VideoFrameTextureSource {
    pub fn new(raw_pixel_buffer: *mut c_void, width: u32, height: u32) -> Self {
        Self {
            raw_pixel_buffer,
            width,
            height,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportedVideoFormat {
    Nv12,
    P010,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedVideoFrameInfo {
    pub width: usize,
    pub height: usize,
    pub pixel_format: u32,
    pub pixel_format_fourcc: String,
    pub format: ImportedVideoFormat,
    pub full_range: bool,
    pub color_range: ColorRange,
    pub planes: Vec<ImportedVideoPlaneInfo>,
}

pub struct ImportedVideoFrame {
    info: ImportedVideoFrameInfo,
    source_color: SourceColorState,
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    inner: Option<apple::ImportedVideoFrameTextures>,
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    _unsupported: (),
}

impl ImportedVideoFrame {
    pub fn info(&self) -> &ImportedVideoFrameInfo {
        &self.info
    }

    pub fn plane_count(&self) -> usize {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            self.inner.as_ref().map_or(0, |inner| inner.plane_count())
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            0
        }
    }

    pub fn source_color(&self) -> SourceColorState {
        self.source_color
    }

    pub fn set_source_color(&mut self, source: SourceColorState) {
        let import_range = self.info.color_range;
        let fallback = source.range.resolve(ColorRange::Limited);
        self.source_color = source.range(import_range.resolve(fallback));
    }

    pub fn set_source_color_metadata(
        &mut self,
        primaries: ColorPrimaries,
        transfer: TransferFunction,
        range: ColorRange,
        matrix: MatrixCoefficients,
        hdr_metadata: Option<HdrMetadata>,
    ) {
        self.set_source_color(
            SourceColorState::new(primaries, transfer)
                .range(range)
                .matrix(matrix)
                .hdr_metadata(hdr_metadata),
        );
    }
}

pub struct VideoRenderFrame<'a> {
    pub frame: &'a ImportedVideoFrame,
    pub pipeline: VideoRenderPipeline,
    /// Identifies the decoded frame across render ticks. The same token means
    /// the luma plane is unchanged, letting the upscaler reuse its cached
    /// output when a frame is presented for several vsync ticks.
    pub frame_token: Option<u64>,
}

impl<'a> VideoRenderFrame<'a> {
    pub fn new(frame: &'a ImportedVideoFrame) -> Self {
        Self {
            frame,
            pipeline: VideoRenderPipeline::new(frame.source_color(), Default::default()),
            frame_token: None,
        }
    }

    pub fn frame_token(mut self, token: u64) -> Self {
        self.frame_token = Some(token);
        self
    }

    pub fn full_range(mut self, full_range: bool) -> Self {
        self.pipeline.source.range = color_range_from_import(full_range);
        self
    }

    pub fn source_color(mut self, primaries: ColorPrimaries, transfer: TransferFunction) -> Self {
        let range = self.pipeline.source.range;
        let matrix = self.pipeline.source.matrix;
        let target = self.pipeline.target;
        self.pipeline = VideoRenderPipeline::new(
            SourceColorState::new(primaries, transfer)
                .range(range)
                .matrix(matrix),
            target,
        );
        self
    }

    pub fn pipeline(mut self, pipeline: VideoRenderPipeline) -> Self {
        self.pipeline = pipeline;
        self
    }
}

fn color_range_from_import(full_range: bool) -> ColorRange {
    if full_range {
        ColorRange::Full
    } else {
        ColorRange::Limited
    }
}

pub struct OverlayRenderFrame<'a> {
    pub frame: &'a OverlayFrame,
}

impl<'a> OverlayRenderFrame<'a> {
    pub fn new(frame: &'a OverlayFrame) -> Self {
        Self { frame }
    }
}

pub struct DanmakuRenderFrame<'a> {
    pub plan: &'a DanmakuRenderPlan,
}

impl<'a> DanmakuRenderFrame<'a> {
    pub fn new(plan: &'a DanmakuRenderPlan) -> Self {
        Self { plan }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreparedOverlayFrameInfo {
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub subtitle_planes: usize,
    pub subtitle_pixels: usize,
    pub subtitle_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedVideoPlaneInfo {
    pub index: usize,
    pub width: usize,
    pub height: usize,
    pub metal_pixel_format: &'static str,
}

impl MetalRenderer {
    pub fn new() -> Result<Self> {
        Self::with_config(MetalRendererConfig::default())
    }

    pub fn with_config(_config: MetalRendererConfig) -> Result<Self> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            Ok(Self {
                inner: apple::MetalRendererImpl::new(_config)?,
                current_frame: None,
                current_media_time: Duration::ZERO,
                current_generation: 1,
                upload_counter: 0,
            })
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            Err(PlayerError::Renderer(
                "Metal renderer is only available on Apple platforms for v0".to_string(),
            ))
        }
    }

    pub unsafe fn attach_raw_layer(
        &mut self,
        layer: *mut c_void,
        width: u32,
        height: u32,
        scale: f64,
    ) -> Result<()> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            unsafe { self.inner.attach_raw_layer(layer, width, height, scale) }
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            let _ = (layer, width, height, scale);
            Err(PlayerError::Renderer(
                "Metal renderer is only available on Apple platforms for v0".to_string(),
            ))
        }
    }

    pub unsafe fn import_video_frame_textures(
        &mut self,
        source: VideoFrameTextureSource,
    ) -> Result<ImportedVideoFrame> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            let imported = unsafe { self.inner.import_video_frame_textures(source) }?;
            let source_color =
                SourceColorState::new(ColorPrimaries::Unknown, TransferFunction::Unknown)
                    .range(imported.info.color_range.resolve(ColorRange::Limited));
            Ok(ImportedVideoFrame {
                info: imported.info,
                source_color,
                inner: Some(imported.textures),
            })
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            let _ = source;
            Err(PlayerError::Renderer(
                "Metal renderer is only available on Apple platforms for v0".to_string(),
            ))
        }
    }

    pub fn render_video_frame(&mut self, frame: VideoRenderFrame<'_>) -> Result<()> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            self.inner.render_video_frame(frame)
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            let _ = frame;
            Err(PlayerError::Renderer(
                "Metal renderer is only available on Apple platforms for v0".to_string(),
            ))
        }
    }

    pub fn render_video_frame_with_overlay(
        &mut self,
        frame: VideoRenderFrame<'_>,
        overlay: OverlayRenderFrame<'_>,
    ) -> Result<()> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            self.inner.render_video_frame_with_overlay(frame, overlay)
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            let _ = (frame, overlay);
            Err(PlayerError::Renderer(
                "Metal renderer is only available on Apple platforms for v0".to_string(),
            ))
        }
    }

    pub fn render_video_frame_with_context(
        &mut self,
        frame: VideoRenderFrame<'_>,
        overlay: Option<OverlayRenderFrame<'_>>,
        danmaku: Option<DanmakuRenderFrame<'_>>,
    ) -> Result<()> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            self.inner
                .render_video_frame_with_context(frame, overlay, danmaku)
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            let _ = (frame, overlay, danmaku);
            Err(PlayerError::Renderer(
                "Metal renderer is only available on Apple platforms for v0".to_string(),
            ))
        }
    }

    pub fn render_overlay_frame(&mut self, overlay: OverlayRenderFrame<'_>) -> Result<()> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            self.inner.render_overlay_frame(overlay)
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            let _ = overlay;
            Err(PlayerError::Renderer(
                "Metal renderer is only available on Apple platforms for v0".to_string(),
            ))
        }
    }

    pub fn prepare_overlay_frame(
        &mut self,
        frame: OverlayRenderFrame<'_>,
    ) -> Result<PreparedOverlayFrameInfo> {
        let info = inspect_overlay_frame(frame.frame)?;
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            self.inner.record_prepared_overlay_frame(info);
        }
        Ok(info)
    }

    fn import_player_frame(&mut self, frame: &Frame) -> Result<ImportedVideoFrame> {
        let pixel_buffer = frame.videotoolbox_pixel_buffer().ok_or_else(|| {
            PlayerError::Renderer(
                "decoded frame is not backed by VideoToolbox CVPixelBuffer".to_string(),
            )
        })?;
        let mut imported = unsafe {
            self.import_video_frame_textures(VideoFrameTextureSource::new(
                pixel_buffer.raw(),
                pixel_buffer.width(),
                pixel_buffer.height(),
            ))
        }?;
        imported.set_source_color_metadata(
            frame.color_primaries(),
            frame.transfer_function(),
            frame.color_range(),
            frame.matrix_coefficients(),
            frame.hdr_metadata(),
        );
        Ok(imported)
    }

    pub fn stats(&self) -> MetalRendererStats {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            self.inner.stats()
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            MetalRendererStats::default()
        }
    }
}

fn inspect_overlay_frame(frame: &OverlayFrame) -> Result<PreparedOverlayFrameInfo> {
    let mut subtitle_pixels = 0usize;
    let mut subtitle_bytes = 0usize;

    for plane in &frame.subtitle_planes {
        let pixels = plane.width as usize * plane.height as usize;
        let bytes = pixels * 4;
        if plane.rgba.len() != bytes {
            return Err(crate::core::PlayerError::Renderer(format!(
                "subtitle plane has {} bytes, expected {bytes} for {}x{} RGBA",
                plane.rgba.len(),
                plane.width,
                plane.height
            )));
        }
        subtitle_pixels += pixels;
        subtitle_bytes += bytes;
    }
    for bitmap in &frame.subtitle_alpha_planes {
        if !bitmap.is_valid() {
            return Err(crate::core::PlayerError::Renderer(format!(
                "subtitle alpha bitmap has {} bytes, expected at least {} for {}x{} stride {}",
                bitmap.alpha.len(),
                bitmap.required_len(),
                bitmap.placement.width,
                bitmap.placement.height,
                bitmap.stride
            )));
        }
        subtitle_pixels += bitmap.placement.width as usize * bitmap.placement.height as usize;
        subtitle_bytes += bitmap.required_len();
    }

    Ok(PreparedOverlayFrameInfo {
        viewport_width: frame.viewport.width,
        viewport_height: frame.viewport.height,
        subtitle_planes: frame.subtitle_planes.len() + frame.subtitle_alpha_planes.len(),
        subtitle_pixels,
        subtitle_bytes,
    })
}

pub fn fourcc_string(value: u32) -> String {
    let bytes = value.to_be_bytes();
    if bytes
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        format!("0x{value:08x}")
    }
}

impl RendererBackend for MetalRenderer {
    fn attach_surface(&mut self, surface: PlatformSurface) -> Result<()> {
        match surface {
            PlatformSurface::Metal(handle) => unsafe {
                self.attach_raw_layer(
                    handle.raw_layer as *mut c_void,
                    handle.width,
                    handle.height,
                    handle.scale,
                )
            },
            PlatformSurface::Wgpu(_) => Err(crate::core::PlayerError::Renderer(
                "wgpu surface cannot be attached to MetalRenderer".to_string(),
            )),
            PlatformSurface::FlutterTexture(_) => Err(crate::core::PlayerError::Renderer(
                "Flutter texture cannot be attached to MetalRenderer".to_string(),
            )),
        }
    }

    fn detach_surface(&mut self) -> Result<()> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            self.inner.detach_surface();
        }
        Ok(())
    }

    fn resize_surface(&mut self, width: u32, height: u32, scale: f64) -> Result<()> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            self.inner.resize_surface(width, height, scale);
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            let _ = (width, height, scale);
        }
        Ok(())
    }

    fn render_test_frame(&mut self, time_seconds: f64) -> Result<()> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            self.inner.render_clear(ClearColor::animated(time_seconds))
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            let _ = time_seconds;
            Err(PlayerError::Renderer(
                "Metal renderer is only available on Apple platforms for v0".to_string(),
            ))
        }
    }

    fn upload_player_frame(&mut self, frame: &PlayerVideoFrame) -> Result<()> {
        let imported = self.import_player_frame(&frame.frame)?;
        self.current_frame = Some(imported);
        self.current_media_time = frame.pts.unwrap_or(frame.media_time);
        self.current_generation = frame.generation.max(1);
        self.upload_counter = self.upload_counter.wrapping_add(1);
        Ok(())
    }

    fn render_current_frame(&mut self, context: RenderFrameContext<'_>) -> Result<bool> {
        let Some(frame) = self.current_frame.take() else {
            return Ok(false);
        };
        let danmaku = context.danmaku.filter(|plan| {
            plan.generation == context.generation
                && plan.media_time == context.media_time
                && (context.output_width == 0 || plan.viewport.width == context.output_width)
                && (context.output_height == 0 || plan.viewport.height == context.output_height)
        });
        let result = self.render_video_frame_with_context(
            VideoRenderFrame::new(&frame).frame_token(self.upload_counter),
            context.overlay.map(OverlayRenderFrame::new),
            danmaku.map(DanmakuRenderFrame::new),
        );
        self.current_frame = Some(frame);
        result.map(|()| true)
    }

    fn runtime_stats(&self) -> RendererRuntimeStats {
        let stats = self.stats();
        RendererRuntimeStats {
            surface_width: stats.drawable_width,
            surface_height: stats.drawable_height,
            rendered_frames: stats.rendered_frames,
            offscreen_frames: 0,
            prepared_overlay_frames: stats.prepared_overlay_frames,
            prepared_overlay_subtitle_planes: stats.prepared_overlay_subtitle_planes,
            danmaku_passes: stats.danmaku_passes,
            danmaku_draw_items: stats.danmaku_items,
            overlay_alpha_atlas_uploads: stats.overlay_alpha_atlas_uploads,
            overlay_alpha_atlas_reuses: stats.overlay_alpha_atlas_reuses,
            last_danmaku_atlas_duration: stats.last_danmaku_atlas_duration,
            last_danmaku_vertex_build_duration: stats.last_danmaku_vertex_build_duration,
            last_danmaku_vertex_copy_duration: stats.last_danmaku_vertex_copy_duration,
            last_danmaku_encode_duration: stats.last_danmaku_encode_duration,
            last_danmaku_vertex_bytes: stats.last_danmaku_vertex_bytes,
            last_danmaku_vertex_count: stats.last_danmaku_vertex_count,
            upscaler_mode: stats.upscaler_mode,
            upscaler_backend: stats.upscaler_backend,
            upscaler_fallbacks: stats.upscaler_fallbacks,
            upscaled_frames: stats.upscaled_frames,
            last_upscaler_encode_duration: stats.last_upscaler_encode_duration,
            last_gpu_duration: stats.last_gpu_duration,
            attached: stats.drawable_width > 0 && stats.drawable_height > 0,
            software_video_frames: 0,
            hardware_video_frames: self.upload_counter,
            zero_copy_video_frames: self.upload_counter,
            direct_zero_copy_video_frames: self.upload_counter,
            shared_handle_video_frames: 0,
            cpu_video_frame_fallbacks: 0,
        }
    }

    fn set_luma_upscaler(&mut self, mode: crate::renderer::pipeline::LumaUpscalerMode) {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            self.inner.set_luma_upscaler(mode);
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            let _ = mode;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::overlay::OverlayViewport;
    use crate::renderer::pipeline::{MatrixCoefficients, SourceColorState};
    use crate::subtitle::SubtitleBitmapPlane;

    fn test_imported_frame(
        import_range: ColorRange,
        source_color: SourceColorState,
    ) -> ImportedVideoFrame {
        ImportedVideoFrame {
            info: ImportedVideoFrameInfo {
                width: 1920,
                height: 1080,
                pixel_format: 0,
                pixel_format_fourcc: "test".to_string(),
                format: ImportedVideoFormat::P010,
                full_range: matches!(import_range, ColorRange::Full),
                color_range: import_range,
                planes: Vec::new(),
            },
            source_color,
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            inner: None,
            #[cfg(not(any(target_os = "macos", target_os = "ios")))]
            _unsupported: (),
        }
    }

    #[test]
    fn inspect_overlay_counts_subtitle_bytes() {
        let frame = OverlayFrame {
            pts: Duration::from_secs(1),
            viewport: OverlayViewport::new(640, 360),
            subtitle_planes: vec![SubtitleBitmapPlane {
                x: 0,
                y: 0,
                width: 10,
                height: 4,
                rgba: vec![255; 10 * 4 * 4],
            }],
            subtitle_alpha_planes: Vec::new(),
            subtitle_changed: true,
        };

        let info = inspect_overlay_frame(&frame).unwrap();

        assert_eq!(info.viewport_width, 640);
        assert_eq!(info.viewport_height, 360);
        assert_eq!(info.subtitle_planes, 1);
        assert_eq!(info.subtitle_pixels, 40);
        assert_eq!(info.subtitle_bytes, 160);
    }

    #[test]
    fn inspect_overlay_counts_alpha_bitmap_bytes() {
        let frame = OverlayFrame {
            pts: Duration::ZERO,
            viewport: OverlayViewport::new(640, 360),
            subtitle_planes: Vec::new(),
            subtitle_alpha_planes: vec![crate::subtitle::SubtitleAlphaBitmap::new(
                crate::subtitle::SubtitleBitmapPlacement::new(4, 8, 3, 2),
                5,
                0xff00ffff,
                vec![255; 8],
            )],
            subtitle_changed: true,
        };

        let info = inspect_overlay_frame(&frame).unwrap();

        assert_eq!(info.subtitle_planes, 1);
        assert_eq!(info.subtitle_pixels, 6);
        assert_eq!(info.subtitle_bytes, 8);
    }

    #[test]
    fn inspect_overlay_rejects_malformed_alpha_bitmap() {
        let frame = OverlayFrame {
            pts: Duration::ZERO,
            viewport: OverlayViewport::new(640, 360),
            subtitle_planes: Vec::new(),
            subtitle_alpha_planes: vec![crate::subtitle::SubtitleAlphaBitmap::new(
                crate::subtitle::SubtitleBitmapPlacement::new(4, 8, 3, 2),
                5,
                0xff00ffff,
                vec![255; 7],
            )],
            subtitle_changed: true,
        };

        assert!(inspect_overlay_frame(&frame).is_err());
    }

    #[test]
    fn inspect_overlay_rejects_malformed_rgba_plane() {
        let frame = OverlayFrame {
            pts: Duration::ZERO,
            viewport: OverlayViewport::new(1, 1),
            subtitle_planes: vec![SubtitleBitmapPlane {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
                rgba: vec![0; 15],
            }],
            subtitle_alpha_planes: Vec::new(),
            subtitle_changed: true,
        };

        assert!(inspect_overlay_frame(&frame).is_err());
    }

    #[test]
    fn metal_output_mode_maps_sdr_to_default_drawable_and_target() {
        let output = MetalOutputMode::default();

        assert_eq!(output.pixel_format(), MetalDrawablePixelFormat::Bgra8Unorm);
        assert!(!output.is_edr());

        let target = output.target_color();
        assert_eq!(target.primaries, ColorPrimaries::Bt709);
        assert_eq!(target.transfer, TransferFunction::Srgb);
        assert_eq!(target.peak_nits, 100.0);
        assert_eq!(target.edr_headroom, 1.0);
    }

    #[test]
    fn metal_output_mode_maps_apple_edr_to_float_drawable_and_headroom_target() {
        let output = MetalOutputMode::apple_edr(4.0);

        assert_eq!(output.pixel_format(), MetalDrawablePixelFormat::Rgba16Float);
        assert!(output.is_edr());

        let target = output.target_color();
        assert_eq!(target.primaries, ColorPrimaries::Bt709);
        assert_eq!(target.transfer, TransferFunction::Srgb);
        assert_eq!(target.peak_nits, 812.0);
        assert_eq!(target.reference_white_nits, 203.0);
        assert_eq!(target.edr_headroom, 4.0);
    }

    #[test]
    fn metal_output_mode_maps_pq_source_to_pq_edr_target() {
        let output = MetalOutputMode::apple_edr(4.0);
        let source = SourceColorState::new(ColorPrimaries::Bt2020, TransferFunction::Pq)
            .nominal_peak_nits(1200.0);

        let target = output.target_color_for_source(source);

        assert_eq!(target.primaries, ColorPrimaries::Bt2020);
        assert_eq!(target.transfer, TransferFunction::Pq);
        assert_eq!(target.peak_nits, 10_000.0);
        assert_eq!(target.reference_white_nits, 203.0);
        assert_eq!(target.edr_headroom, 4.0);
    }

    #[test]
    fn metal_output_mode_clamps_edr_headroom_to_one() {
        let target = MetalOutputMode::apple_edr(0.25).target_color();

        assert_eq!(target.peak_nits, 203.0);
        assert_eq!(target.edr_headroom, 1.0);
    }

    #[test]
    fn video_render_frame_uses_imported_source_color() {
        let source = SourceColorState::new(ColorPrimaries::Bt2020, TransferFunction::Pq)
            .range(ColorRange::Limited)
            .matrix(MatrixCoefficients::Bt2020NonConstantLuminance);
        let frame = test_imported_frame(ColorRange::Limited, source);

        let render_frame = VideoRenderFrame::new(&frame);

        assert_eq!(
            render_frame.pipeline.source.primaries,
            ColorPrimaries::Bt2020
        );
        assert_eq!(render_frame.pipeline.source.transfer, TransferFunction::Pq);
        assert_eq!(render_frame.pipeline.source.range, ColorRange::Limited);
        assert_eq!(
            render_frame.pipeline.source.matrix,
            MatrixCoefficients::Bt2020NonConstantLuminance
        );
    }

    #[test]
    fn imported_frame_prefers_pixel_buffer_range_over_metadata() {
        let mut frame = test_imported_frame(
            ColorRange::Full,
            SourceColorState::new(ColorPrimaries::Unknown, TransferFunction::Unknown)
                .range(ColorRange::Full),
        );

        frame.set_source_color_metadata(
            ColorPrimaries::Bt709,
            TransferFunction::Srgb,
            ColorRange::Limited,
            MatrixCoefficients::Bt709,
            None,
        );

        assert_eq!(frame.source_color().range, ColorRange::Full);
        assert_eq!(frame.source_color().matrix, MatrixCoefficients::Bt709);
    }

    #[test]
    fn imported_frame_applies_hdr_metadata_peak() {
        let mut frame = test_imported_frame(
            ColorRange::Limited,
            SourceColorState::new(ColorPrimaries::Unknown, TransferFunction::Unknown),
        );
        let metadata = HdrMetadata::new(
            None,
            Some(crate::renderer::pipeline::ContentLightMetadata {
                max_content_light_level_nits: 4000,
                max_frame_average_light_level_nits: 450,
            }),
        );

        frame.set_source_color_metadata(
            ColorPrimaries::Bt2020,
            TransferFunction::Pq,
            ColorRange::Limited,
            MatrixCoefficients::Bt2020NonConstantLuminance,
            Some(metadata),
        );

        assert_eq!(frame.source_color().hdr_metadata, Some(metadata));
        assert_eq!(frame.source_color().nominal_peak_nits, 4000.0);
    }

    #[test]
    fn imported_frame_uses_metadata_range_when_import_unspecified() {
        let mut frame = test_imported_frame(
            ColorRange::Unspecified,
            SourceColorState::new(ColorPrimaries::Unknown, TransferFunction::Unknown)
                .range(ColorRange::Unspecified),
        );

        frame.set_source_color(
            SourceColorState::new(ColorPrimaries::Bt709, TransferFunction::Srgb)
                .range(ColorRange::Full),
        );

        assert_eq!(frame.source_color().range, ColorRange::Full);
    }
}
