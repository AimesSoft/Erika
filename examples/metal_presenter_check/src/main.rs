use std::ffi::c_void;
use std::process;

use erika::renderer::metal::MetalRenderer;
use erika::{MetalSurfaceHandle, PlatformSurface, RendererBackend, SurfaceMetrics};

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
        eprintln!("metal presenter check failed: {error}");
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
    renderer
        .render_test_frame(0.25)
        .map_err(|error| error.to_string())?;
    renderer
        .resize_surface(SurfaceMetrics::new(640, 360, 2.0))
        .map_err(|error| error.to_string())?;
    renderer
        .render_test_frame(0.5)
        .map_err(|error| error.to_string())?;

    let stats = renderer.stats();
    println!(
        "metal presenter stats: drawable={}x{} rendered_frames={}",
        stats.drawable_width, stats.drawable_height, stats.rendered_frames
    );
    if stats.drawable_width != 640 || stats.drawable_height != 360 {
        return Err(format!(
            "unexpected drawable size {}x{}",
            stats.drawable_width, stats.drawable_height
        ));
    }
    if stats.rendered_frames < 2 {
        return Err(format!(
            "expected at least 2 rendered frames, got {}",
            stats.rendered_frames
        ));
    }
    Ok(())
}
