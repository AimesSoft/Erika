use crate::core::{ColorPrimaries, TransferFunction};
use crate::renderer::pipeline::TargetColorState;

/// Platform-neutral output request selected by the embedder.
///
/// `ExtendedLinear` means a floating-point, linear-light presentation path
/// whose values above `1.0` carry display headroom. Each backend must still
/// negotiate a matching native surface/color space; otherwise it must render
/// through the SDR description and report the fallback explicitly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputMode {
    Sdr,
    /// Legacy Apple API spelling retained for ABI compatibility.
    AppleEdr {
        headroom: f32,
    },
    ExtendedLinear {
        headroom: f32,
    },
}

impl OutputMode {
    pub fn extended_linear(headroom: f32) -> Self {
        Self::ExtendedLinear {
            headroom: normalized_headroom(headroom),
        }
    }

    /// Backwards-compatible spelling for the original Apple-only C/Dart API.
    pub fn apple_edr(headroom: f32) -> Self {
        Self::AppleEdr {
            headroom: normalized_headroom(headroom),
        }
    }

    pub fn is_edr(self) -> bool {
        matches!(self, Self::AppleEdr { .. } | Self::ExtendedLinear { .. })
    }

    pub fn is_android_extended_linear(self) -> bool {
        matches!(self, Self::ExtendedLinear { .. })
    }

    pub fn headroom(self) -> f32 {
        match self {
            Self::Sdr => 1.0,
            Self::AppleEdr { headroom } | Self::ExtendedLinear { headroom } => {
                normalized_headroom(headroom)
            }
        }
    }
}

impl Default for OutputMode {
    fn default() -> Self {
        Self::Sdr
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputColorSpace {
    Srgb,
    ExtendedSrgbLinear,
    Bt2020Pq,
}

impl OutputColorSpace {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Srgb => "srgb",
            Self::ExtendedSrgbLinear => "extended-srgb-linear",
            Self::Bt2020Pq => "bt2020-pq",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputSurfaceFormat {
    EightBitUnorm,
    TenBitUnorm,
    SixteenBitFloat,
}

impl Default for OutputSurfaceFormat {
    fn default() -> Self {
        Self::EightBitUnorm
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActiveOutputEncoding {
    #[default]
    SdrSrgb,
    AppleEdr,
    AndroidExtendedLinearScRgb,
    Hdr10Pq,
}

impl ActiveOutputEncoding {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SdrSrgb => "sdr-srgb",
            Self::AppleEdr => "apple-edr",
            Self::AndroidExtendedLinearScRgb => "android-extended-linear-scrgb",
            Self::Hdr10Pq => "hdr10-pq",
        }
    }
}

/// Stable reason code explaining why a requested output mode is not active.
///
/// These values are part of the C/Dart ABI. Add new reasons at the end and do
/// not renumber existing variants.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFallbackReason {
    #[default]
    None = 0,
    DisplayHdrUnsupported = 1,
    HybridCompositionRequired = 2,
    WgpuBackendNotVulkan = 3,
    Rgba16FloatSurfaceFormatUnavailable = 4,
    NativeWindowDataSpaceApiUnavailable = 5,
    ScrgbDataSpaceVerificationFailed = 6,
    SurfaceConfigureFailed = 7,
    LegacyAppleEdrUnsupported = 8,
}

impl OutputFallbackReason {
    pub const fn from_raw(value: i32) -> Self {
        match value {
            1 => Self::DisplayHdrUnsupported,
            2 => Self::HybridCompositionRequired,
            3 => Self::WgpuBackendNotVulkan,
            4 => Self::Rgba16FloatSurfaceFormatUnavailable,
            5 => Self::NativeWindowDataSpaceApiUnavailable,
            6 => Self::ScrgbDataSpaceVerificationFailed,
            7 => Self::SurfaceConfigureFailed,
            8 => Self::LegacyAppleEdrUnsupported,
            _ => Self::None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::DisplayHdrUnsupported => "display_hdr_unsupported",
            Self::HybridCompositionRequired => "hybrid_composition_required",
            Self::WgpuBackendNotVulkan => "wgpu_backend_not_vulkan",
            Self::Rgba16FloatSurfaceFormatUnavailable => "rgba16float_surface_format_unavailable",
            Self::NativeWindowDataSpaceApiUnavailable => "native_window_dataspace_api_unavailable",
            Self::ScrgbDataSpaceVerificationFailed => "scrgb_dataspace_verification_failed",
            Self::SurfaceConfigureFailed => "surface_configure_failed",
            Self::LegacyAppleEdrUnsupported => "legacy_apple_edr_unsupported",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputRuntimeStatus {
    pub requested_mode: OutputMode,
    pub active_encoding: ActiveOutputEncoding,
    pub surface_format: OutputSurfaceFormat,
    pub native_data_space: i32,
    pub requested_headroom: f32,
    pub active_headroom: f32,
    pub active_headroom_known: bool,
    pub extended_linear_active: bool,
    pub fallback_reason: OutputFallbackReason,
    pub fallback_count: u64,
    pub data_space_failures: u64,
    pub headroom_updates: u64,
    pub extended_linear_frames: u64,
}

impl OutputRuntimeStatus {
    pub fn requested(mode: OutputMode) -> Self {
        Self {
            requested_mode: mode,
            requested_headroom: mode.headroom(),
            ..Self::default()
        }
    }
}

impl Default for OutputRuntimeStatus {
    fn default() -> Self {
        Self {
            requested_mode: OutputMode::Sdr,
            active_encoding: ActiveOutputEncoding::SdrSrgb,
            surface_format: OutputSurfaceFormat::EightBitUnorm,
            native_data_space: -1,
            requested_headroom: 1.0,
            active_headroom: 1.0,
            active_headroom_known: true,
            extended_linear_active: false,
            fallback_reason: OutputFallbackReason::None,
            fallback_count: 0,
            data_space_failures: 0,
            headroom_updates: 0,
            extended_linear_frames: 0,
        }
    }
}

impl OutputSurfaceFormat {
    pub const fn label(self) -> &'static str {
        match self {
            Self::EightBitUnorm => "8bit-unorm",
            Self::TenBitUnorm => "10bit-unorm",
            Self::SixteenBitFloat => "16bit-float",
        }
    }
}

/// Actual signal contract used by a renderer after native surface negotiation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputDescription {
    pub color_space: OutputColorSpace,
    pub surface_format: OutputSurfaceFormat,
    pub target: TargetColorState,
    pub extended_linear: bool,
}

impl OutputDescription {
    pub fn sdr() -> Self {
        Self {
            color_space: OutputColorSpace::Srgb,
            surface_format: OutputSurfaceFormat::EightBitUnorm,
            target: TargetColorState::sdr(ColorPrimaries::Bt709),
            extended_linear: false,
        }
    }

    pub fn extended_linear(headroom: f32) -> Self {
        let headroom = normalized_headroom(headroom);
        Self {
            color_space: OutputColorSpace::ExtendedSrgbLinear,
            surface_format: OutputSurfaceFormat::SixteenBitFloat,
            // Android/Vulkan EXTENDED_SRGB_LINEAR follows the scRGB convention:
            // a linear component value of 1.0 represents 80 cd/m².
            target: TargetColorState::extended_linear(ColorPrimaries::Bt709, 80.0, headroom),
            extended_linear: true,
        }
    }

    pub fn apple_edr(headroom: f32) -> Self {
        let headroom = normalized_headroom(headroom);
        Self {
            color_space: OutputColorSpace::ExtendedSrgbLinear,
            surface_format: OutputSurfaceFormat::SixteenBitFloat,
            target: TargetColorState::apple_edr(ColorPrimaries::Bt709, headroom),
            extended_linear: true,
        }
    }

    pub fn hdr10() -> Self {
        Self {
            color_space: OutputColorSpace::Bt2020Pq,
            surface_format: OutputSurfaceFormat::TenBitUnorm,
            target: TargetColorState::hdr10(ColorPrimaries::Bt2020),
            extended_linear: false,
        }
    }

    pub fn requested(mode: OutputMode) -> Self {
        match mode {
            OutputMode::Sdr => Self::sdr(),
            OutputMode::AppleEdr { headroom } => Self::apple_edr(headroom),
            OutputMode::ExtendedLinear { headroom } => Self::extended_linear(headroom),
        }
    }

    pub fn target_transfer(self) -> TransferFunction {
        self.target.transfer
    }
}

fn normalized_headroom(headroom: f32) -> f32 {
    if headroom.is_finite() {
        headroom.clamp(1.0, 10_000.0)
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_linear_description_is_float_sc_rgb() {
        let description = OutputDescription::requested(OutputMode::extended_linear(4.0));

        assert_eq!(
            description.color_space,
            OutputColorSpace::ExtendedSrgbLinear
        );
        assert_eq!(
            description.surface_format,
            OutputSurfaceFormat::SixteenBitFloat
        );
        assert_eq!(description.target.primaries, ColorPrimaries::Bt709);
        assert_eq!(description.target.transfer, TransferFunction::Srgb);
        assert_eq!(description.target.edr_headroom, 4.0);
        assert!(description.extended_linear);
    }

    #[test]
    fn output_mode_rejects_invalid_headroom() {
        assert_eq!(OutputMode::extended_linear(f32::NAN).headroom(), 1.0);
        assert_eq!(OutputMode::extended_linear(0.25).headroom(), 1.0);
        assert_eq!(OutputMode::extended_linear(20_000.0).headroom(), 10_000.0);
    }

    #[test]
    fn apple_and_android_extended_linear_keep_distinct_reference_white() {
        let apple = OutputDescription::requested(OutputMode::apple_edr(4.0));
        let android = OutputDescription::requested(OutputMode::extended_linear(4.0));

        assert_eq!(apple.target.reference_white_nits, 203.0);
        assert_eq!(android.target.reference_white_nits, 80.0);
        assert_eq!(apple.target.edr_headroom, 4.0);
        assert_eq!(android.target.edr_headroom, 4.0);
    }

    #[test]
    fn fallback_reason_codes_and_labels_are_stable() {
        let expected = [
            (OutputFallbackReason::None, 0, "none"),
            (
                OutputFallbackReason::DisplayHdrUnsupported,
                1,
                "display_hdr_unsupported",
            ),
            (
                OutputFallbackReason::HybridCompositionRequired,
                2,
                "hybrid_composition_required",
            ),
            (
                OutputFallbackReason::WgpuBackendNotVulkan,
                3,
                "wgpu_backend_not_vulkan",
            ),
            (
                OutputFallbackReason::Rgba16FloatSurfaceFormatUnavailable,
                4,
                "rgba16float_surface_format_unavailable",
            ),
            (
                OutputFallbackReason::NativeWindowDataSpaceApiUnavailable,
                5,
                "native_window_dataspace_api_unavailable",
            ),
            (
                OutputFallbackReason::ScrgbDataSpaceVerificationFailed,
                6,
                "scrgb_dataspace_verification_failed",
            ),
            (
                OutputFallbackReason::SurfaceConfigureFailed,
                7,
                "surface_configure_failed",
            ),
            (
                OutputFallbackReason::LegacyAppleEdrUnsupported,
                8,
                "legacy_apple_edr_unsupported",
            ),
        ];

        for (reason, code, label) in expected {
            assert_eq!(reason as i32, code);
            assert_eq!(OutputFallbackReason::from_raw(code), reason);
            assert_eq!(reason.label(), label);
        }
    }

    #[test]
    fn hdr10_description_remains_distinct_from_extended_linear() {
        let description = OutputDescription::hdr10();

        assert_eq!(description.color_space, OutputColorSpace::Bt2020Pq);
        assert_eq!(description.surface_format, OutputSurfaceFormat::TenBitUnorm);
        assert_eq!(description.target.transfer, TransferFunction::Pq);
        assert!(!description.extended_linear);
    }
}
