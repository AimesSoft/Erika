//! Erika danmaku outline profiles and raster quantization.
//!
//! Layout and rendering must use the same resolved width so collision bounds
//! match the glyphs drawn on screen. Keep every profile tuning constant here.

const BASE_WIDTH_RATIO: f32 = 0.06;
const BASE_WIDTH_MIN_PX: f32 = 1.0;
const BASE_WIDTH_MAX_PX: f32 = 2.6;
const FINE_PROFILE_SCALE: f32 = 0.8;
const THICK_PROFILE_SCALE: f32 = 1.5;
const NORMAL_PROFILE_THRESHOLD: f32 = 1.5;
const THICK_PROFILE_THRESHOLD: f32 = 2.5;
const MAX_RASTER_RADIUS_PX: f32 = 16.0;

/// Resolves the public outline profile to a physical-pixel width.
///
/// Profiles are 0 = off, 1 = fine, 2 = normal, and 3 = thick. Normal and
/// thick preserve Erika's former profile 1 and profile 2 respectively.
pub(super) fn resolve_width_px(font_size: f32, profile: f32) -> f32 {
    if !profile.is_finite() || profile <= 0.0 {
        return 0.0;
    }

    let base_width = (font_size * BASE_WIDTH_RATIO).clamp(BASE_WIDTH_MIN_PX, BASE_WIDTH_MAX_PX);
    if profile < NORMAL_PROFILE_THRESHOLD {
        base_width * FINE_PROFILE_SCALE
    } else if profile < THICK_PROFILE_THRESHOLD {
        base_width
    } else {
        base_width * THICK_PROFILE_SCALE
    }
}

/// Quantizes a resolved physical-pixel width for the alpha-mask dilator.
pub(super) fn raster_radius(width_px: f32) -> u16 {
    if !width_px.is_finite() {
        return 0;
    }
    width_px.round().clamp(0.0, MAX_RASTER_RADIUS_PX) as u16
}
