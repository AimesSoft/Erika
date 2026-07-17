use std::ffi::c_void;
use std::process;
use std::time::Duration;

use erika::overlay::{OverlayFrame, OverlayViewport};
use erika::renderer::metal::{MetalRenderer, OverlayRenderFrame};
use erika::subtitle::SubtitleBitmapPlane;
use erika::{MetalSurfaceHandle, PlatformSurface, RendererBackend};

unsafe extern "C" {
    fn erika_presenter_check_create_layer(width: f64, height: f64, scale: f64) -> *mut c_void;
    fn erika_presenter_check_release_layer(layer: *mut c_void);
}

fn main() {
    let layer = unsafe { erika_presenter_check_create_layer(640.0, 360.0, 2.0) };
    if layer.is_null() {
        eprintln!("failed to create CAMetalLayer");
        process::exit(1);
    }

    let result = run_check(layer);
    unsafe { erika_presenter_check_release_layer(layer) };
    if let Err(error) = result {
        eprintln!("metal overlay check failed: {error}");
        process::exit(1);
    }
}

fn run_check(layer: *mut c_void) -> Result<(), String> {
    let mut renderer = MetalRenderer::new().map_err(|error| error.to_string())?;
    renderer
        .attach_surface(PlatformSurface::Metal(MetalSurfaceHandle::new(
            layer as u64,
            1280,
            720,
            2.0,
        )))
        .map_err(|error| error.to_string())?;

    let overlay = OverlayFrame {
        pts: Duration::from_secs(1),
        viewport: OverlayViewport::new(640, 360),
        subtitle_planes: vec![SubtitleBitmapPlane {
            x: 80,
            y: 260,
            width: 240,
            height: 32,
            rgba: solid_rgba(240, 32, [255, 255, 255, 220]),
        }],
        subtitle_alpha_planes: Vec::new(),
        subtitle_changed: true,
    };
    let info = renderer
        .prepare_overlay_frame(OverlayRenderFrame::new(&overlay))
        .map_err(|error| error.to_string())?;
    renderer
        .render_overlay_frame(OverlayRenderFrame::new(&overlay))
        .map_err(|error| error.to_string())?;
    let stats = renderer.stats();
    println!(
        "metal overlay stats: drawable={}x{} rendered_frames={} prepared_overlays={} subtitle_planes={} bytes={}",
        stats.drawable_width,
        stats.drawable_height,
        stats.rendered_frames,
        stats.prepared_overlay_frames,
        stats.prepared_overlay_subtitle_planes,
        info.subtitle_bytes,
    );
    if stats.rendered_frames < 1 || stats.prepared_overlay_subtitle_planes < 1 {
        return Err(format!(
            "unexpected stats rendered={} subtitle_planes={}",
            stats.rendered_frames, stats.prepared_overlay_subtitle_planes
        ));
    }
    Ok(())
}

fn solid_rgba(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&rgba);
    }
    pixels
}
