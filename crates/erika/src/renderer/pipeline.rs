use crate::core::{ColorPrimaries, TransferFunction};

pub const VIDEO_INPUT_MODE_MASK: u32 = 0xff;
pub const VIDEO_INPUT_PACKED_ALPHA_RIGHT: u32 = 1 << 8;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HdrMetadata {
    pub mastering_display: Option<MasteringDisplayMetadata>,
    pub content_light: Option<ContentLightMetadata>,
}

impl HdrMetadata {
    pub fn new(
        mastering_display: Option<MasteringDisplayMetadata>,
        content_light: Option<ContentLightMetadata>,
    ) -> Self {
        Self {
            mastering_display,
            content_light,
        }
    }

    pub fn nominal_peak_nits(self) -> Option<f32> {
        self.mastering_display
            .and_then(|metadata| metadata.max_luminance_nits())
            .or_else(|| {
                self.content_light
                    .and_then(|metadata| metadata.max_content_light_level_nits())
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MasteringDisplayMetadata {
    pub display_primaries: Option<[Chromaticity; 3]>,
    pub white_point: Option<Chromaticity>,
    pub min_luminance_nits: Option<f32>,
    pub max_luminance_nits: Option<f32>,
}

impl MasteringDisplayMetadata {
    pub fn max_luminance_nits(self) -> Option<f32> {
        self.max_luminance_nits
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContentLightMetadata {
    pub max_content_light_level_nits: u32,
    pub max_frame_average_light_level_nits: u32,
}

impl ContentLightMetadata {
    pub fn max_content_light_level_nits(self) -> Option<f32> {
        if self.max_content_light_level_nits == 0 {
            None
        } else {
            Some(self.max_content_light_level_nits as f32)
        }
    }
}

/// Maximum number of reshaping pieces per component in a Dolby Vision RPU
/// (`AV_DOVI_MAX_PIECES` in FFmpeg, `num_pivots - 1` segments).
pub const DOVI_MAX_PIECES: usize = 8;
/// Maximum number of MMR orders per piece (FFmpeg allows 1..=3).
pub const DOVI_MAX_MMR_ORDER: usize = 3;
/// Number of coefficients per MMR order: 3 linear terms plus the 4 cross
/// products (x·y, x·z, y·z, x·y·z).
pub const DOVI_MMR_COEFFS: usize = 7;

/// One component's reshaping curve, converted from the RPU's fixed-point
/// representation into shader-ready floats. Pivots are normalized to the
/// base-layer signal range [0, 1] and coefficients by `2^-coef_log2_denom`,
/// exactly like libplacebo's `pl_map_dovi_metadata`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DoviComponentCurve {
    /// 0 when this component carries no reshaping, otherwise 2..=9.
    pub num_pivots: u8,
    /// Sorted ascending, normalized to [0, 1]. Only the first `num_pivots`
    /// entries are meaningful.
    pub pivots: [f32; DOVI_MAX_PIECES + 1],
    /// Polynomial coefficients per segment (x^0, x^1, x^2). Segments above
    /// `poly_order` are zero-filled.
    pub poly_coeffs: [[f32; 3]; DOVI_MAX_PIECES],
    /// Per segment: 0 selects the polynomial, 1..=3 selects MMR of that order.
    pub mmr_orders: [u8; DOVI_MAX_PIECES],
    pub mmr_constants: [f32; DOVI_MAX_PIECES],
    pub mmr_coeffs: [[[f32; DOVI_MMR_COEFFS]; DOVI_MAX_MMR_ORDER]; DOVI_MAX_PIECES],
}

impl Default for DoviComponentCurve {
    fn default() -> Self {
        Self {
            num_pivots: 0,
            pivots: [0.0; DOVI_MAX_PIECES + 1],
            poly_coeffs: [[0.0; 3]; DOVI_MAX_PIECES],
            mmr_orders: [0; DOVI_MAX_PIECES],
            mmr_constants: [0.0; DOVI_MAX_PIECES],
            mmr_coeffs: [[[0.0; DOVI_MMR_COEFFS]; DOVI_MAX_MMR_ORDER]; DOVI_MAX_PIECES],
        }
    }
}

/// Per-frame Dolby Vision RPU payload copied out of the decoder's
/// `AV_FRAME_DATA_DOVI_METADATA` side data before the frame is retired.
///
/// The `nonlinear_matrix` is the RPU's "ycc_to_rgb" transform applied to the
/// reshaped (still PQ-encoded) signal; `rgb_to_lms` is the RPU's mastering
/// transform whose inverse converts PQ-linearized LMS back to BT.2020 RGB.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DoviSourceMetadata {
    /// Per-component reshaping curves (luma, Cb, Cr).
    pub reshaping: [DoviComponentCurve; 3],
    /// RPU's ycc_to_rgb matrix applied to reshaped nonlinear signal.
    pub nonlinear_matrix: RgbMatrix,
    /// RPU signal offsets applied before the nonlinear matrix.
    pub nonlinear_offset: [f32; 3],
    /// RPU's rgb_to_lms mastering transform (inverted after PQ linearization).
    pub rgb_to_lms: RgbMatrix,
    /// 12-bit PQ code of the mastering display's black level (typically 0-100).
    pub source_min_pq: u16,
    /// 12-bit PQ code of the mastering display's peak level (typically 2000-4000 nits).
    pub source_max_pq: u16,
}

/// Decodes a 12-bit PQ code value into absolute nits, matching the PQ EOTF
/// used by the video shaders.
pub fn pq_code_to_nits(code: u16) -> f32 {
    if code == 0 {
        return 0.0;
    }
    let encoded = f32::from(code.min(4095)) / 4095.0;
    let m1 = 0.1593017578125_f32;
    let m2 = 78.84375_f32;
    let c1 = 0.8359375_f32;
    let c2 = 18.8515625_f32;
    let c3 = 18.6875_f32;
    let p = encoded.max(0.0).powf(1.0 / m2);
    let num = (p - c1).max(0.0);
    let den = (c2 - c3 * p).max(0.000001);
    10000.0 * (num / den).powf(1.0 / m1)
}

/// Inverse of the no-crosstalk BT.2020-referred HPE RGB->LMS transform that
/// the RPU's `rgb_to_lms` output is fed into (hard-coded by libplacebo as
/// `dovi_lms2rgb`).
const DOVI_HPE_LMS_TO_RGB: RgbMatrix = RgbMatrix::new([
    [3.06441879, -2.16597676, 0.10155818],
    [-0.65612108, 1.78554118, -0.12943749],
    [0.01736321, -0.04725154, 1.03004253],
]);

/// The composite LMS->RGB matrix applied after PQ linearization of a reshaped
/// Dolby Vision signal: the fixed HPE inverse multiplied by the RPU's
/// `rgb_to_lms` matrix, matching libplacebo's `dovi_lms2rgb` composition.
pub fn dovi_lms_to_rgb_matrix(rgb_to_lms: RgbMatrix) -> RgbMatrix {
    DOVI_HPE_LMS_TO_RGB.mul(rgb_to_lms)
}

/// Shader uniform block for Dolby Vision reshaping. All values are
/// vec4-aligned so the block can be appended to the shared video uniform
/// buffer across the WGSL, Metal and HLSL backends without packing tricks.
///
/// **Size**: ~3KB total (144 vec4s for MMR + overhead). Modern GPUs support
/// this easily, but older mobile devices may have uniform buffer limits around
/// 16KB - this uses ~20% of that budget.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "wgpu", derive(bytemuck::Pod, bytemuck::Zeroable))]
pub struct DoviUniforms {
    /// x = 1.0 when the source is RPU-mapped; y/z/w = per-component segment
    /// counts (`num_pivots - 1`, 0 when the component carries no curve).
    pub flags: [f32; 4],
    /// Interior pivots per component (two rows each, segments - 1 values,
    /// padded with a quasi-infinite sentinel like libplacebo).
    pub pivots: [[f32; 4]; 6],
    /// Per-component `[first_pivot, last_pivot]` output clamp.
    pub bounds: [[f32; 4]; 3],
    /// Per-segment payload `[c0, c1, c2, kind]`: kind == 0 is the polynomial
    /// `(c2·s + c1)·s + c0`, kind 1..=3 is MMR of that order with constant
    /// `c0` and packed rows starting at offset `c1`.
    pub coefficients: [[f32; 4]; 3 * DOVI_MAX_PIECES],
    /// Packed MMR rows per component (two vec4 rows per order), addressed by
    /// the segment's `c1` offset.
    pub mmr: [[f32; 4]; 3 * 2 * DOVI_MAX_MMR_ORDER * DOVI_MAX_PIECES],
    /// RPU "ycc_to_rgb" rows applied to the reshaped nonlinear signal.
    pub nonlinear_matrix: [[f32; 4]; 3],
    /// RPU signal offsets, pre-scaled so the shader's normalized 8-bit or
    /// P010 samples subtract exactly (libplacebo folds 2^bits/(2^bits-1)).
    pub nonlinear_offset: [f32; 4],
    /// Composite LMS->RGB rows applied after PQ linearization.
    pub lms_matrix: [[f32; 4]; 3],
}

const DOVI_PIVOT_SENTINEL: f32 = 1e9;
/// RPU offsets are rational /1024-style values while shader samples are
/// normalized by n/(2^bits - 1). `DoviUniforms::of_for_representation`
/// applies the matching
/// 2^bits/(2^bits-1) correction for the actual uploaded representation.
impl DoviUniforms {
    pub const fn disabled() -> Self {
        Self {
            flags: [0.0; 4],
            pivots: [[0.0; 4]; 6],
            bounds: [[0.0; 4]; 3],
            coefficients: [[0.0; 4]; 3 * DOVI_MAX_PIECES],
            mmr: [[0.0; 4]; 3 * 2 * DOVI_MAX_MMR_ORDER * DOVI_MAX_PIECES],
            nonlinear_matrix: [[0.0; 4]; 3],
            nonlinear_offset: [0.0; 4],
            lms_matrix: [[0.0; 4]; 3],
        }
    }

    /// Builds uniforms using the historical 10-bit DV signal representation.
    /// New callers with an explicit uploaded format should use
    /// [`Self::of_for_representation`].
    pub fn of(source: &SourceColorState) -> Self {
        Self::of_for_representation(source, true)
    }

    /// Builds uniforms for the concrete plane representation used by the
    /// renderer (`P010` when `is_p010` is true, otherwise 8-bit `NV12`).
    pub fn of_for_representation(source: &SourceColorState, is_p010: bool) -> Self {
        let Some(dovi) = &source.dovi else {
            return Self::disabled();
        };
        let mut uniforms = Self::disabled();
        uniforms.flags[0] = 1.0;
        for (component, curve) in dovi.reshaping.iter().enumerate() {
            if curve.num_pivots < 2 {
                continue;
            }
            let segments = (curve.num_pivots - 1) as usize;
            uniforms.flags[1 + component] = segments as f32;
            let mut interior = [DOVI_PIVOT_SENTINEL; DOVI_MAX_PIECES];
            interior[..segments - 1].copy_from_slice(&curve.pivots[1..segments]);
            uniforms.pivots[2 * component] = [interior[0], interior[1], interior[2], interior[3]];
            uniforms.pivots[2 * component + 1] =
                [interior[4], interior[5], interior[6], interior[7]];
            uniforms.bounds[component] = [
                curve.pivots[0].min(curve.pivots[segments]),
                curve.pivots[0].max(curve.pivots[segments]),
                0.0,
                0.0,
            ];

            let mut mmr_row = 0usize;
            for (segment, &kind) in curve.mmr_orders[..segments].iter().enumerate() {
                let slot = DOVI_MAX_PIECES * component + segment;
                if kind == 0 {
                    uniforms.coefficients[slot] = [
                        curve.poly_coeffs[segment][0],
                        curve.poly_coeffs[segment][1],
                        curve.poly_coeffs[segment][2],
                        0.0,
                    ];
                    continue;
                }
                let order = (kind as usize).min(DOVI_MAX_MMR_ORDER);
                let orders = &curve.mmr_coeffs[segment][..order];
                for (index, coefficients) in orders.iter().enumerate() {
                    let row =
                        DOVI_MAX_PIECES * 2 * DOVI_MAX_MMR_ORDER * component + mmr_row + 2 * index;
                    uniforms.mmr[row] = [coefficients[0], coefficients[1], coefficients[2], 0.0];
                    uniforms.mmr[row + 1] = [
                        coefficients[3],
                        coefficients[4],
                        coefficients[5],
                        coefficients[6],
                    ];
                }
                uniforms.coefficients[slot] = [
                    curve.mmr_constants[segment],
                    mmr_row as f32,
                    0.0,
                    kind as f32,
                ];
                mmr_row += 2 * order;
            }
        }
        uniforms.nonlinear_matrix = dovi.nonlinear_matrix.row4s();
        let signal_bits = if is_p010 { 10 } else { 8 };
        let signal_max = ((1_u32 << signal_bits) - 1) as f32;
        let signal_offset_scale = (1_u32 << signal_bits) as f32 / signal_max;
        uniforms.nonlinear_offset = [
            dovi.nonlinear_offset[0] * signal_offset_scale,
            dovi.nonlinear_offset[1] * signal_offset_scale,
            dovi.nonlinear_offset[2] * signal_offset_scale,
            0.0,
        ];
        uniforms.lms_matrix = dovi_lms_to_rgb_matrix(dovi.rgb_to_lms).row4s();
        uniforms
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorRange {
    Unspecified,
    Limited,
    Full,
}

impl Default for ColorRange {
    fn default() -> Self {
        Self::Unspecified
    }
}

impl ColorRange {
    pub fn resolve(self, fallback: Self) -> Self {
        match self {
            Self::Unspecified => fallback,
            _ => self,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatrixCoefficients {
    Unspecified,
    Identity,
    Bt601,
    Bt709,
    Bt2020NonConstantLuminance,
}

impl Default for MatrixCoefficients {
    fn default() -> Self {
        Self::Unspecified
    }
}

impl MatrixCoefficients {
    pub fn resolve(self, primaries: ColorPrimaries) -> Self {
        if self != Self::Unspecified {
            return self;
        }
        match primaries {
            ColorPrimaries::Bt2020 => Self::Bt2020NonConstantLuminance,
            ColorPrimaries::Bt709 | ColorPrimaries::DisplayP3 => Self::Bt709,
            ColorPrimaries::Unknown => Self::Bt709,
        }
    }

    pub fn luma_coefficients(self, primaries: ColorPrimaries) -> LumaCoefficients {
        match self.resolve(primaries) {
            Self::Bt601 => LumaCoefficients::new(0.2990, 0.5870, 0.1140),
            Self::Bt2020NonConstantLuminance => LumaCoefficients::new(0.2627, 0.6780, 0.0593),
            Self::Identity | Self::Bt709 | Self::Unspecified => {
                LumaCoefficients::new(0.2126, 0.7152, 0.0722)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LumaCoefficients {
    pub kr: f32,
    pub kg: f32,
    pub kb: f32,
}

impl LumaCoefficients {
    pub const fn new(kr: f32, kg: f32, kb: f32) -> Self {
        Self { kr, kg, kb }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chromaticity {
    pub x: f32,
    pub y: f32,
}

impl Chromaticity {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrimariesCoordinates {
    pub red: Chromaticity,
    pub green: Chromaticity,
    pub blue: Chromaticity,
    pub white: Chromaticity,
}

impl PrimariesCoordinates {
    pub const fn new(
        red: Chromaticity,
        green: Chromaticity,
        blue: Chromaticity,
        white: Chromaticity,
    ) -> Self {
        Self {
            red,
            green,
            blue,
            white,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RgbMatrix {
    rows: [[f32; 3]; 3],
}

impl RgbMatrix {
    pub const fn new(rows: [[f32; 3]; 3]) -> Self {
        Self { rows }
    }

    pub const fn identity() -> Self {
        Self::new([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
    }

    pub fn rows(self) -> [[f32; 3]; 3] {
        self.rows
    }

    pub fn row4s(self) -> [[f32; 4]; 3] {
        [
            [self.rows[0][0], self.rows[0][1], self.rows[0][2], 0.0],
            [self.rows[1][0], self.rows[1][1], self.rows[1][2], 0.0],
            [self.rows[2][0], self.rows[2][1], self.rows[2][2], 0.0],
        ]
    }

    fn mul(self, rhs: Self) -> Self {
        let mut rows = [[0.0; 3]; 3];
        for (row_index, row) in rows.iter_mut().enumerate() {
            for (col_index, value) in row.iter_mut().enumerate() {
                *value = self.rows[row_index][0] * rhs.rows[0][col_index]
                    + self.rows[row_index][1] * rhs.rows[1][col_index]
                    + self.rows[row_index][2] * rhs.rows[2][col_index];
            }
        }
        Self::new(rows)
    }

    fn mul_vec(self, value: [f32; 3]) -> [f32; 3] {
        [
            self.rows[0][0] * value[0] + self.rows[0][1] * value[1] + self.rows[0][2] * value[2],
            self.rows[1][0] * value[0] + self.rows[1][1] * value[1] + self.rows[1][2] * value[2],
            self.rows[2][0] * value[0] + self.rows[2][1] * value[1] + self.rows[2][2] * value[2],
        ]
    }

    fn inverse(self) -> Self {
        let m = self.rows;
        let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
        let inv_det = 1.0 / det;
        Self::new([
            [
                (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_det,
                (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
                (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
            ],
            [
                (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det,
                (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
                (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
            ],
            [
                (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_det,
                (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
                (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
            ],
        ])
    }
}

pub fn primaries_coordinates(primaries: ColorPrimaries) -> PrimariesCoordinates {
    match resolve_primaries(primaries) {
        ColorPrimaries::Bt2020 => PrimariesCoordinates::new(
            Chromaticity::new(0.708, 0.292),
            Chromaticity::new(0.170, 0.797),
            Chromaticity::new(0.131, 0.046),
            D65_WHITE,
        ),
        ColorPrimaries::DisplayP3 => PrimariesCoordinates::new(
            Chromaticity::new(0.680, 0.320),
            Chromaticity::new(0.265, 0.690),
            Chromaticity::new(0.150, 0.060),
            D65_WHITE,
        ),
        ColorPrimaries::Bt709 | ColorPrimaries::Unknown => PrimariesCoordinates::new(
            Chromaticity::new(0.640, 0.330),
            Chromaticity::new(0.300, 0.600),
            Chromaticity::new(0.150, 0.060),
            D65_WHITE,
        ),
    }
}

pub fn rgb_to_xyz_matrix(primaries: ColorPrimaries) -> RgbMatrix {
    let coords = primaries_coordinates(primaries);
    let red = xy_to_xyz(coords.red);
    let green = xy_to_xyz(coords.green);
    let blue = xy_to_xyz(coords.blue);
    let white = xy_to_xyz(coords.white);
    let unscaled = RgbMatrix::new([
        [red[0], green[0], blue[0]],
        [red[1], green[1], blue[1]],
        [red[2], green[2], blue[2]],
    ]);
    let scale = unscaled.inverse().mul_vec(white);
    RgbMatrix::new([
        [red[0] * scale[0], green[0] * scale[1], blue[0] * scale[2]],
        [red[1] * scale[0], green[1] * scale[1], blue[1] * scale[2]],
        [red[2] * scale[0], green[2] * scale[1], blue[2] * scale[2]],
    ])
}

pub fn xyz_to_rgb_matrix(primaries: ColorPrimaries) -> RgbMatrix {
    rgb_to_xyz_matrix(primaries).inverse()
}

pub fn source_to_target_rgb_matrix(source: ColorPrimaries, target: ColorPrimaries) -> RgbMatrix {
    let source = resolve_primaries(source);
    let target = resolve_primaries(target);
    if source == target {
        return RgbMatrix::identity();
    }
    xyz_to_rgb_matrix(target).mul(rgb_to_xyz_matrix(source))
}

const D65_WHITE: Chromaticity = Chromaticity::new(0.3127, 0.3290);

fn resolve_primaries(primaries: ColorPrimaries) -> ColorPrimaries {
    match primaries {
        ColorPrimaries::Unknown => ColorPrimaries::Bt709,
        _ => primaries,
    }
}

fn xy_to_xyz(value: Chromaticity) -> [f32; 3] {
    [value.x / value.y, 1.0, (1.0 - value.x - value.y) / value.y]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToneMapOperator {
    Clip,
    Reinhard,
    Mobius,
    /// ITU-R BT.2390 EETF evaluated in the PQ domain. The default: it keeps
    /// midtones closer to the reference look of libplacebo/mpv than the
    /// legacy operators, whose linear-region passthrough reads as washed out.
    Bt2390,
}

impl Default for ToneMapOperator {
    fn default() -> Self {
        Self::Bt2390
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalerKernel {
    Nearest,
    Bilinear,
    Bicubic,
    Lanczos3,
}

/// Neural 2x luma upscaler applied to the decoded Y plane before plane
/// sampling. Chroma keeps its source resolution and is reconstructed by the
/// regular scaler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LumaUpscalerMode {
    #[default]
    Off,
    /// ArtCNN C4F16 (~12K parameters), lightweight real-time anime doubler.
    ArtCnnC4F16,
    /// ArtCNN C4F16 DS, denoising and sharpening for degraded anime sources.
    ArtCnnC4F16Ds,
    /// ArtCNN C4F32 (~48K parameters), higher quality real-time doubler.
    ArtCnnC4F32,
}

impl LumaUpscalerMode {
    pub fn is_enabled(self) -> bool {
        self != Self::Off
    }
}

impl Default for ScalerKernel {
    fn default() -> Self {
        Self::Bilinear
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SourceColorState {
    pub primaries: ColorPrimaries,
    pub transfer: TransferFunction,
    pub matrix: MatrixCoefficients,
    pub range: ColorRange,
    pub hdr_metadata: Option<HdrMetadata>,
    pub dovi: Option<DoviSourceMetadata>,
    pub nominal_peak_nits: f32,
    pub reference_white_nits: f32,
}

impl SourceColorState {
    pub fn new(primaries: ColorPrimaries, transfer: TransferFunction) -> Self {
        Self {
            primaries,
            transfer,
            matrix: MatrixCoefficients::default(),
            range: ColorRange::default(),
            hdr_metadata: None,
            dovi: None,
            nominal_peak_nits: nominal_peak_for_transfer(transfer),
            reference_white_nits: reference_white_for_transfer(transfer),
        }
    }

    pub fn range(mut self, range: ColorRange) -> Self {
        self.range = range;
        self
    }

    pub fn matrix(mut self, matrix: MatrixCoefficients) -> Self {
        self.matrix = matrix;
        self
    }

    pub fn nominal_peak_nits(mut self, peak: f32) -> Self {
        self.nominal_peak_nits = peak.max(1.0);
        self
    }

    pub fn hdr_metadata(mut self, metadata: Option<HdrMetadata>) -> Self {
        if let Some(peak) = metadata.and_then(HdrMetadata::nominal_peak_nits) {
            self.nominal_peak_nits = peak.max(1.0);
        }
        self.hdr_metadata = metadata;
        self
    }

    /// Attaches per-frame Dolby Vision RPU metadata. The RPU describes an
    /// IPT/LMS signal referred to BT.2020 with a PQ transfer, so the primaries
    /// and transfer are forced to BT.2020/PQ (matching libplacebo's
    /// `pl_map_avdovi_metadata`). The RPU's `source_min_pq`/`source_max_pq`
    /// replace static mastering luminance when present, while ordinary display
    /// primaries and content-light metadata are retained. Forcing the transfer
    /// also repairs streams whose VUI tags are missing entirely.
    pub fn dovi(mut self, metadata: Option<DoviSourceMetadata>) -> Self {
        if let Some(dovi) = metadata {
            self.primaries = ColorPrimaries::Bt2020;
            self.transfer = TransferFunction::Pq;
            self.reference_white_nits = reference_white_for_transfer(self.transfer).max(1.0);
            let min_luminance = (dovi.source_min_pq != 0)
                .then(|| pq_code_to_nits(dovi.source_min_pq))
                .filter(|value| value.is_finite() && *value >= 0.0);
            let peak = pq_code_to_nits(dovi.source_max_pq);
            if peak > 0.0 {
                self.nominal_peak_nits = peak.max(1.0);
            } else if self.nominal_peak_nits <= self.reference_white_nits {
                self.nominal_peak_nits = nominal_peak_for_transfer(self.transfer);
            }
            // Keep ordinary mastering primaries/content-light metadata, but
            // prefer the RPU's source luminance bounds when present. This lets
            // native HDR10 outputs carry Dolby Vision black-level metadata too.
            if min_luminance.is_some()
                || (peak.is_finite() && peak > 0.0)
                || self.hdr_metadata.is_some()
            {
                let mut hdr = self
                    .hdr_metadata
                    .unwrap_or_else(|| HdrMetadata::new(None, None));
                let mut mastering = hdr.mastering_display.unwrap_or(MasteringDisplayMetadata {
                    display_primaries: None,
                    white_point: None,
                    min_luminance_nits: None,
                    max_luminance_nits: None,
                });
                if let Some(min_luminance) = min_luminance {
                    mastering.min_luminance_nits = Some(min_luminance);
                }
                if peak.is_finite() && peak > 0.0 {
                    mastering.max_luminance_nits = Some(peak);
                }
                hdr.mastering_display = Some(mastering);
                self.hdr_metadata = Some(hdr);
            }
            self.dovi = Some(dovi);
        } else {
            self.dovi = None;
        }
        self
    }

    pub fn reference_white_nits(mut self, white: f32) -> Self {
        self.reference_white_nits = white.max(1.0);
        self
    }

    pub fn is_hdr(&self) -> bool {
        matches!(self.transfer, TransferFunction::Pq | TransferFunction::Hlg)
            || self.nominal_peak_nits > self.reference_white_nits * 1.5
    }
}

impl Default for SourceColorState {
    fn default() -> Self {
        Self::new(ColorPrimaries::Unknown, TransferFunction::Unknown)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetColorState {
    pub primaries: ColorPrimaries,
    pub transfer: TransferFunction,
    pub peak_nits: f32,
    pub reference_white_nits: f32,
    pub edr_headroom: f32,
}

impl TargetColorState {
    pub fn sdr(primaries: ColorPrimaries) -> Self {
        Self {
            primaries,
            transfer: TransferFunction::Srgb,
            peak_nits: 100.0,
            reference_white_nits: 100.0,
            edr_headroom: 1.0,
        }
    }

    pub fn apple_edr(primaries: ColorPrimaries, headroom: f32) -> Self {
        Self::extended_linear(primaries, 203.0, headroom)
    }

    pub fn extended_linear(
        primaries: ColorPrimaries,
        reference_white_nits: f32,
        headroom: f32,
    ) -> Self {
        let reference_white_nits = reference_white_nits.max(1.0);
        let headroom = headroom.max(1.0);
        Self {
            primaries,
            transfer: TransferFunction::Srgb,
            peak_nits: reference_white_nits * headroom,
            reference_white_nits,
            edr_headroom: headroom,
        }
    }

    pub fn hdr10(primaries: ColorPrimaries) -> Self {
        Self {
            primaries,
            transfer: TransferFunction::Pq,
            peak_nits: 10_000.0,
            reference_white_nits: 203.0,
            edr_headroom: 1.0,
        }
    }
}

impl Default for TargetColorState {
    fn default() -> Self {
        Self::sdr(ColorPrimaries::Bt709)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToneMapConfig {
    pub operator: ToneMapOperator,
    pub knee_start: f32,
    pub desaturate: f32,
}

impl Default for ToneMapConfig {
    fn default() -> Self {
        Self {
            // Follow the operator enum's default so a default-operator change
            // actually reaches the pipeline (a hardcoded value here silently
            // overrides it).
            operator: ToneMapOperator::default(),
            knee_start: 0.75,
            desaturate: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScalerConfig {
    pub kernel: ScalerKernel,
    pub radius: f32,
}

impl Default for ScalerConfig {
    fn default() -> Self {
        Self {
            kernel: ScalerKernel::Bilinear,
            radius: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderPassKind {
    ImportFrame,
    NeuralUpscale,
    PlaneSampling,
    ChromaReconstruction,
    DoviReshape,
    TransferDecode,
    GamutMap,
    ToneMap,
    Scale,
    OverlayComposite,
    Dither,
    OutputTransform,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderPass {
    pub kind: RenderPassKind,
    pub label: &'static str,
}

impl RenderPass {
    pub const fn new(kind: RenderPassKind, label: &'static str) -> Self {
        Self { kind, label }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RenderGraph {
    passes: Vec<RenderPass>,
}

impl RenderGraph {
    pub fn new() -> Self {
        Self { passes: Vec::new() }
    }

    pub fn push(&mut self, pass: RenderPass) {
        self.passes.push(pass);
    }

    pub fn passes(&self) -> &[RenderPass] {
        &self.passes
    }

    pub fn contains(&self, kind: RenderPassKind) -> bool {
        self.passes.iter().any(|pass| pass.kind == kind)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoRenderPipeline {
    pub source: SourceColorState,
    pub target: TargetColorState,
    pub tone_map: ToneMapConfig,
    pub scaler: ScalerConfig,
    pub luma_upscaler: LumaUpscalerMode,
    pub graph: RenderGraph,
}

impl VideoRenderPipeline {
    pub fn new(source: SourceColorState, target: TargetColorState) -> Self {
        let tone_map = ToneMapConfig::default();
        let scaler = ScalerConfig::default();
        let luma_upscaler = LumaUpscalerMode::default();
        let graph = build_graph(source, target, scaler, luma_upscaler);
        Self {
            source,
            target,
            tone_map,
            scaler,
            luma_upscaler,
            graph,
        }
    }

    pub fn sdr_default() -> Self {
        Self::new(SourceColorState::default(), TargetColorState::default())
    }

    pub fn with_target(mut self, target: TargetColorState) -> Self {
        self.target = target;
        self.graph = build_graph(self.source, self.target, self.scaler, self.luma_upscaler);
        self
    }

    pub fn with_luma_upscaler(mut self, mode: LumaUpscalerMode) -> Self {
        self.luma_upscaler = mode;
        self.graph = build_graph(self.source, self.target, self.scaler, self.luma_upscaler);
        self
    }

    pub fn requires_tone_mapping(&self) -> bool {
        requires_tone_mapping(self.source, self.target)
    }

    pub fn luma_coefficients(&self) -> LumaCoefficients {
        self.source.matrix.luma_coefficients(self.source.primaries)
    }

    pub fn requires_gamut_mapping(&self) -> bool {
        requires_gamut_mapping(self.source, self.target)
    }

    pub fn gamut_matrix(&self) -> RgbMatrix {
        source_to_target_rgb_matrix(self.source.primaries, self.target.primaries)
    }
}

impl Default for VideoRenderPipeline {
    fn default() -> Self {
        Self::sdr_default()
    }
}

/// Fragment-shader uniforms shared by the video sampling shaders.
///
/// Backends may wrap this with presentation-only fields (for example Metal's
/// target rect), but the color/HDR/gamut payload is generated here so platform
/// renderers do not each invent their own color contract.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "wgpu", derive(bytemuck::Pod, bytemuck::Zeroable))]
pub struct VideoUniforms {
    pub is_p010: u32,
    pub full_range: u32,
    pub source_transfer: u32,
    pub target_transfer: u32,
    pub tone_map: u32,
    pub edr_output: u32,
    /// 0 samples Y + interleaved CbCr planes; 1 samples an already converted
    /// nonlinear RGB texture; 2 samples ArtCNN's packed 2x luma output plus a
    /// normal CbCr plane; 3 applies that packed luma as detail to an already
    /// converted nonlinear RGB texture. Packed RGBA texels store the top-left,
    /// top-right, bottom-left and bottom-right luma subpixels. All modes retain
    /// the common transfer/gamut/tone-map handling.
    pub input_mode: u32,
    /// Leaves the shader output in target-reference-linear space so a backend
    /// can composite overlays before applying the output transfer function.
    pub scene_linear: u32,
    pub nits: [f32; 4],
    pub luma_coefficients: [f32; 4],
    pub gamut_matrix_rows: [[f32; 4]; 3],
    /// Dolby Vision reshaping payload; inert unless `flags[0]` is set.
    pub dovi: DoviUniforms,
}

impl VideoUniforms {
    pub fn from_pipeline(pipeline: &VideoRenderPipeline, is_p010: bool, edr_output: bool) -> Self {
        let luma = pipeline.luma_coefficients();
        Self {
            is_p010: u32::from(is_p010),
            full_range: u32::from(matches!(pipeline.source.range, ColorRange::Full)),
            source_transfer: transfer_code(pipeline.source.transfer),
            target_transfer: transfer_code(pipeline.target.transfer),
            tone_map: tone_map_code(pipeline.tone_map.operator),
            edr_output: u32::from(edr_output),
            input_mode: 0,
            scene_linear: 0,
            nits: [
                pipeline.source.nominal_peak_nits,
                pipeline.target.peak_nits,
                pipeline.source.reference_white_nits,
                pipeline.target.reference_white_nits,
            ],
            luma_coefficients: [luma.kr, luma.kg, luma.kb, 0.0],
            gamut_matrix_rows: pipeline.gamut_matrix().row4s(),
            dovi: DoviUniforms::of_for_representation(&pipeline.source, is_p010),
        }
    }

    pub fn rgb_texture_input(mut self) -> Self {
        self.input_mode = (self.input_mode & !VIDEO_INPUT_MODE_MASK) | 1;
        self
    }

    /// Updates the decoded sample representation while keeping Dolby Vision
    /// signal offsets in the same normalized domain as the texture samples.
    /// This matters when a 10-bit P010 frame is down-converted to 8-bit NV12.
    pub fn with_p010_representation(mut self, is_p010: bool) -> Self {
        let old_bits = if self.is_p010 != 0 { 10 } else { 8 };
        let new_bits = if is_p010 { 10 } else { 8 };
        if old_bits != new_bits {
            let old_scale = (1_u32 << old_bits) as f32 / ((1_u32 << old_bits) - 1) as f32;
            let new_scale = (1_u32 << new_bits) as f32 / ((1_u32 << new_bits) - 1) as f32;
            let ratio = new_scale / old_scale;
            for offset in &mut self.dovi.nonlinear_offset[..3] {
                *offset *= ratio;
            }
        }
        self.is_p010 = u32::from(is_p010);
        self
    }

    pub fn packed_d2s_luma_input(mut self) -> Self {
        self.input_mode = (self.input_mode & !VIDEO_INPUT_MODE_MASK) | 2;
        self
    }

    pub fn packed_d2s_rgb_detail_input(mut self) -> Self {
        self.input_mode = (self.input_mode & !VIDEO_INPUT_MODE_MASK) | 3;
        self
    }

    pub fn packed_alpha_right(mut self, enabled: bool) -> Self {
        if enabled {
            self.input_mode |= VIDEO_INPUT_PACKED_ALPHA_RIGHT;
        } else {
            self.input_mode &= !VIDEO_INPUT_PACKED_ALPHA_RIGHT;
        }
        self
    }

    pub fn has_packed_alpha_right(self) -> bool {
        self.input_mode & VIDEO_INPUT_PACKED_ALPHA_RIGHT != 0
    }

    pub fn scene_linear_output(mut self) -> Self {
        self.scene_linear = 1;
        self
    }
}

fn transfer_code(transfer: TransferFunction) -> u32 {
    match transfer {
        TransferFunction::Srgb => 1,
        TransferFunction::Bt1886 => 2,
        TransferFunction::Pq => 3,
        TransferFunction::Hlg => 4,
        TransferFunction::Unknown => 1,
    }
}

fn tone_map_code(operator: ToneMapOperator) -> u32 {
    match operator {
        ToneMapOperator::Clip => 0,
        ToneMapOperator::Reinhard => 1,
        ToneMapOperator::Mobius => 2,
        ToneMapOperator::Bt2390 => 3,
    }
}

fn build_graph(
    source: SourceColorState,
    target: TargetColorState,
    scaler: ScalerConfig,
    luma_upscaler: LumaUpscalerMode,
) -> RenderGraph {
    let mut graph = RenderGraph::new();
    graph.push(RenderPass::new(RenderPassKind::ImportFrame, "import frame"));
    if luma_upscaler.is_enabled() {
        graph.push(RenderPass::new(
            RenderPassKind::NeuralUpscale,
            "neural luma upscale",
        ));
    }
    graph.push(RenderPass::new(
        RenderPassKind::PlaneSampling,
        "sample YCbCr planes",
    ));
    graph.push(RenderPass::new(
        RenderPassKind::ChromaReconstruction,
        "reconstruct chroma",
    ));
    if source.dovi.is_some() {
        graph.push(RenderPass::new(
            RenderPassKind::DoviReshape,
            "reshape dolby vision signal",
        ));
    }
    graph.push(RenderPass::new(
        RenderPassKind::TransferDecode,
        "decode transfer function",
    ));
    if requires_gamut_mapping(source, target) {
        graph.push(RenderPass::new(RenderPassKind::GamutMap, "map gamut"));
    }
    if requires_tone_mapping(source, target) {
        graph.push(RenderPass::new(RenderPassKind::ToneMap, "tone map"));
    }
    if scaler.kernel != ScalerKernel::Nearest {
        graph.push(RenderPass::new(RenderPassKind::Scale, "scale"));
    }
    graph.push(RenderPass::new(
        RenderPassKind::OverlayComposite,
        "composite overlays",
    ));
    graph.push(RenderPass::new(RenderPassKind::Dither, "dither"));
    graph.push(RenderPass::new(
        RenderPassKind::OutputTransform,
        "output transform",
    ));
    graph
}

fn requires_tone_mapping(source: SourceColorState, target: TargetColorState) -> bool {
    if !source.is_hdr() {
        return false;
    }
    source.nominal_peak_nits > target.peak_nits * 1.05
}

fn requires_gamut_mapping(source: SourceColorState, target: TargetColorState) -> bool {
    resolve_primaries(source.primaries) != resolve_primaries(target.primaries)
}

fn nominal_peak_for_transfer(transfer: TransferFunction) -> f32 {
    match transfer {
        TransferFunction::Pq => 1000.0,
        TransferFunction::Hlg => 1000.0,
        TransferFunction::Srgb | TransferFunction::Bt1886 => 100.0,
        TransferFunction::Unknown => 100.0,
    }
}

fn reference_white_for_transfer(transfer: TransferFunction) -> f32 {
    match transfer {
        TransferFunction::Pq | TransferFunction::Hlg => 203.0,
        TransferFunction::Srgb | TransferFunction::Bt1886 | TransferFunction::Unknown => 100.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference implementation of the BT.2100 HLG inverse OETF used by the
    /// video shaders' `transfer_to_source_reference_linear` (code 4). Nonlinear
    /// signal E' maps to scene linear light in [0, 1]. Constants are spelled
    /// exactly as in BT.2100 (and the shaders), hence the precision allow.
    #[allow(clippy::excessive_precision)]
    fn hlg_inverse_oetf(encoded: f32) -> f32 {
        let a = 0.17883277_f32;
        let b = 0.28466892_f32;
        let c = 0.55991073_f32;
        let e = encoded.max(0.0);
        if e <= 0.5 {
            e * e / 3.0
        } else {
            (((e - c) / a).exp() + b) / 12.0
        }
    }

    /// Reference implementation of the shaders' full HLG decode: inverse OETF
    /// to scene light, then the BT.2100 OOTF (system gamma 1.2 at the 1000 nit
    /// nominal peak), normalized to source reference white exactly like the PQ
    /// branch of the same shader function.
    fn hlg_encoded_to_source_reference_linear(
        encoded: [f32; 3],
        luma: LumaCoefficients,
        reference_white_nits: f32,
    ) -> [f32; 3] {
        let hlg_nominal_peak_nits = 1000.0_f32;
        let hlg_system_gamma = 1.2_f32;
        let scene = [
            hlg_inverse_oetf(encoded[0]),
            hlg_inverse_oetf(encoded[1]),
            hlg_inverse_oetf(encoded[2]),
        ];
        let scene_luma = (luma.kr * scene[0] + luma.kg * scene[1] + luma.kb * scene[2]).max(1e-6);
        let scale = hlg_nominal_peak_nits * scene_luma.powf(hlg_system_gamma - 1.0)
            / reference_white_nits.max(1.0);
        [scene[0] * scale, scene[1] * scale, scene[2] * scale]
    }

    #[test]
    #[allow(clippy::excessive_precision)]
    fn hlg_inverse_oetf_matches_bt2100_anchors() {
        assert!(hlg_inverse_oetf(0.0).abs() < 1e-7);
        // E' = 0.5 sits exactly at the 1/12 scene light scale.
        assert!((hlg_inverse_oetf(0.5) - 1.0 / 12.0).abs() < 1e-6);
        // The quadratic and exponential branches are continuous at E' = 0.5.
        let upper_at_half = (((0.5_f32 - 0.55991073) / 0.17883277).exp() + 0.28466892) / 12.0;
        assert!((hlg_inverse_oetf(0.5) - upper_at_half).abs() < 1e-5);
        // Full-scale signal decodes to unit scene light.
        assert!((hlg_inverse_oetf(1.0) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn hlg_full_scale_white_reaches_nominal_peak_over_reference_white() {
        let luma = MatrixCoefficients::Bt2020NonConstantLuminance
            .luma_coefficients(ColorPrimaries::Bt2020);

        let rgb = hlg_encoded_to_source_reference_linear([1.0, 1.0, 1.0], luma, 203.0);

        for channel in rgb {
            assert!(
                (channel - 1000.0 / 203.0).abs() < 0.01,
                "expected {channel} to be near the 1000 nit peak over 203 nit reference white"
            );
        }
    }

    #[test]
    fn hlg_reference_white_signal_lands_at_reference_white() {
        // BT.2408: the 75% HLG signal displays at roughly 203 nits on the
        // 1000 nit nominal display, i.e. 1.0 in source-reference-linear terms.
        let luma = MatrixCoefficients::Bt2020NonConstantLuminance
            .luma_coefficients(ColorPrimaries::Bt2020);

        let rgb = hlg_encoded_to_source_reference_linear([0.75, 0.75, 0.75], luma, 203.0);

        for channel in rgb {
            assert!(
                (channel - 1.0).abs() < 0.005,
                "expected {channel} to be near 1.0 (203 nits over 203 nit reference white)"
            );
        }
    }

    #[test]
    fn hlg_source_uniforms_use_code_4_and_thousand_nit_peak() {
        let source = SourceColorState::new(ColorPrimaries::Bt2020, TransferFunction::Hlg);
        let target = TargetColorState::sdr(ColorPrimaries::Bt709);
        let pipeline = VideoRenderPipeline::new(source, target);

        let uniforms = VideoUniforms::from_pipeline(&pipeline, false, false);

        assert!(source.is_hdr());
        assert_eq!(uniforms.source_transfer, 4);
        assert_eq!(uniforms.nits[0], 1000.0);
        assert_eq!(uniforms.nits[2], 203.0);
        assert!(pipeline.requires_tone_mapping());
    }

    #[test]
    fn hlg_decode_formula_is_identical_across_video_shaders() {
        let shaders = [
            include_str!("wgpu_video.wgsl"),
            include_str!("metal/apple.rs"),
            include_str!("d3d11.rs"),
        ];
        for shader in shaders {
            assert!(shader.contains("hlg_inverse_oetf"));
            assert!(shader.contains("0.17883277"));
            assert!(shader.contains("0.28466892"));
            assert!(shader.contains("0.55991073"));
            assert!(shader.contains("e * e / 3.0"));
            assert!(shader.contains("(exp((e - c) / a) + b) / 12.0"));
            assert!(shader.contains("source_transfer == 4"));
            assert!(shader.contains("hlg_nominal_peak_nits = 1000.0"));
            assert!(shader.contains("hlg_system_gamma = 1.2"));
            assert!(shader.contains("pow(scene_luma, hlg_system_gamma - 1.0)"));
        }
    }

    /// Reference implementation of the shaders' `bt2390_eetf` (tone_map code
    /// 3): the ITU-R BT.2390 EETF evaluated in the PQ domain, ported from
    /// libplacebo's `pl_tone_map_bt2390` with the default knee offset (1.0)
    /// and a 0 target black level.
    fn bt2390_eetf(nits: f32, source_peak_nits: f32, target_peak_nits: f32) -> f32 {
        let pq_inverse_eotf = |normalized_nits: f32| -> f32 {
            let m1 = 0.1593017578125_f32;
            let m2 = 78.84375_f32;
            let c1 = 0.8359375_f32;
            let c2 = 18.8515625_f32;
            let c3 = 18.6875_f32;
            let p = normalized_nits.clamp(0.0, 1.0).powf(m1);
            ((c1 + c2 * p) / (1.0 + c3 * p).max(0.000_001)).powf(m2)
        };
        let pq_eotf = |encoded: f32| -> f32 {
            let m1 = 0.1593017578125_f32;
            let m2 = 78.84375_f32;
            let c1 = 0.8359375_f32;
            let c2 = 18.8515625_f32;
            let c3 = 18.6875_f32;
            let p = encoded.max(0.0).powf(1.0 / m2);
            let num = (p - c1).max(0.0);
            let den = (c2 - c3 * p).max(0.000_001);
            (num / den).powf(1.0 / m1)
        };
        let src_peak_pq = pq_inverse_eotf(source_peak_nits / 10000.0).max(0.000_001);
        let dst_peak_pq = pq_inverse_eotf(target_peak_nits / 10000.0).max(0.000_001);
        let max_lum = (dst_peak_pq / src_peak_pq).clamp(0.0, 1.0);
        let x = (pq_inverse_eotf(nits.clamp(0.0, 10000.0) / 10000.0) / src_peak_pq).clamp(0.0, 1.0);
        let ks = 2.0 * max_lum - 1.0;
        let mut u = x;
        if ks < 1.0 && x > ks {
            let tb = (x - ks) / (1.0 - ks);
            let tb2 = tb * tb;
            let tb3 = tb2 * tb;
            u = (2.0 * tb3 - 3.0 * tb2 + 1.0) * ks
                + (tb3 - 2.0 * tb2 + tb) * (1.0 - ks)
                + (-2.0 * tb3 + 3.0 * tb2) * max_lum;
        }
        10000.0 * pq_eotf(u * src_peak_pq)
    }

    #[test]
    fn bt2390_eetf_anchors_and_monotonicity() {
        let (source_peak, target_peak) = (1000.0_f32, 100.0_f32);

        // Black stays black; the source peak maps exactly onto the target
        // peak (the spline's endpoint is maxLum by construction).
        assert!(bt2390_eetf(0.0, source_peak, target_peak).abs() < 1e-3);
        let peak = bt2390_eetf(source_peak, source_peak, target_peak);
        assert!((peak - target_peak).abs() < 0.05, "peak = {peak}");

        // Monotonically increasing and bounded by the target peak.
        let mut previous = -1.0_f32;
        for step in 0..=100 {
            let nits = source_peak * step as f32 / 100.0;
            let mapped = bt2390_eetf(nits, source_peak, target_peak);
            assert!(mapped >= previous);
            assert!(mapped <= target_peak + 1e-3);
            previous = mapped;
        }

        // 10:1 compression: the PQ-domain spline lands 100-nit diffuse white
        // at roughly half its mastered level while compressing the source
        // peak onto the target — the reference BT.2390 E+ response.
        let diffuse = bt2390_eetf(100.0, source_peak, target_peak);
        assert!(diffuse > 45.0 && diffuse < 65.0, "diffuse = {diffuse}");
    }

    #[test]
    fn bt2390_formula_is_present_across_video_shaders() {
        let shaders = [
            include_str!("wgpu_video.wgsl"),
            include_str!("metal/apple.rs"),
            include_str!("d3d11.rs"),
        ];
        for shader in shaders {
            assert!(shader.contains("bt2390_eetf"));
            assert!(shader.contains("2.0 * max_lum - 1.0"));
            assert!(shader.contains("tone_map == 3"));
        }
    }

    /// Reference implementation of the shaders' `gamut_desaturate`: out-of-gamut
    /// (negative) components after the linear gamut matrix are blended towards
    /// their BT.709 luma just enough to fit the gamut, preserving hue where
    /// hard-clipping would shift it.
    fn gamut_desaturate(rgb: [f32; 3]) -> [f32; 3] {
        let minc = rgb[0].min(rgb[1]).min(rgb[2]);
        if minc >= 0.0 {
            return rgb;
        }
        let luma = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
        let t = (minc / (minc - luma)).clamp(0.0, 1.0);
        [
            rgb[0] + (luma - rgb[0]) * t,
            rgb[1] + (luma - rgb[1]) * t,
            rgb[2] + (luma - rgb[2]) * t,
        ]
    }

    #[test]
    fn gamut_desaturate_keeps_hue_where_clipping_shifts_it() {
        // In-gamut colors pass through untouched.
        assert_eq!(gamut_desaturate([0.2, 0.7, 0.3]), [0.2, 0.7, 0.3]);
        // A saturated BT.2020 teal-green that the gamut matrix pushes out of
        // gamut (negative red/blue): after desaturation no component is
        // negative and the channel ordering (hue) is preserved — hard
        // clipping would have zeroed red/blue and turned it neon.
        let out_of_gamut = [-0.08_f32, 0.9, -0.04];
        let mapped = gamut_desaturate(out_of_gamut);
        assert!(mapped[0] >= 0.0 && mapped[2] >= 0.0);
        assert!(mapped[1] > mapped[2] && mapped[2] > mapped[0]);
        // The blend only ever reduces saturation, never brightness below the
        // original luma.
        let luma = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        assert!((luma(mapped) - luma(out_of_gamut)).abs() < 1e-4);
    }

    #[test]
    fn gamut_desaturate_formula_is_present_across_video_shaders() {
        let shaders = [
            include_str!("wgpu_video.wgsl"),
            include_str!("metal/apple.rs"),
            include_str!("d3d11.rs"),
        ];
        for shader in shaders {
            assert!(shader.contains("gamut_desaturate"));
            assert!(shader.contains("0.2126, 0.7152, 0.0722"));
            assert!(shader.contains("rgb = gamut_desaturate(rgb)"));
        }
    }

    #[test]
    fn overlay_shaders_handle_sdr_ui_for_hdr_targets() {
        let metal = include_str!("metal/apple.rs");
        let d3d11 = include_str!("d3d11.rs");
        assert!(metal.contains("float3 sdr_ui_color_to_target_output"));
        assert!(metal.contains("pq_inverse_eotf(nits.r / pq_absolute_peak_nits)"));

        // D3D11 composites into an FP16 reference-linear target, then applies
        // PQ once in a full-screen encode pass after alpha blending.
        assert!(d3d11.contains("if (ui_nits.y > 0.0)"));
        assert!(d3d11.contains("max(ui_nits.x, 1.0) / max(ui_nits.y, 1.0)"));
        assert!(d3d11.contains("float4 encode_ps_main"));
        assert!(d3d11.contains("if (scene_linear != 0u)"));
        assert!(d3d11.contains("PSSetShaderResources(0, Some(&[None]))"));
    }

    #[test]
    fn hdr_pq_to_sdr_builds_tone_mapping_graph() {
        let source = SourceColorState::new(ColorPrimaries::Bt2020, TransferFunction::Pq);
        let target = TargetColorState::sdr(ColorPrimaries::Bt709);
        let pipeline = VideoRenderPipeline::new(source, target);

        assert!(pipeline.requires_tone_mapping());
        assert!(pipeline.graph.contains(RenderPassKind::TransferDecode));
        assert!(pipeline.graph.contains(RenderPassKind::GamutMap));
        assert!(pipeline.graph.contains(RenderPassKind::ToneMap));
        assert!(pipeline.graph.contains(RenderPassKind::OutputTransform));
    }

    #[test]
    fn hdr_pq_to_hdr10_keeps_absolute_pq_without_tone_mapping() {
        let source = SourceColorState::new(ColorPrimaries::Bt2020, TransferFunction::Pq);
        let target = TargetColorState::hdr10(ColorPrimaries::Bt2020);
        let pipeline = VideoRenderPipeline::new(source, target);

        assert_eq!(pipeline.target.transfer, TransferFunction::Pq);
        assert_eq!(pipeline.target.reference_white_nits, 203.0);
        assert!(!pipeline.requires_tone_mapping());
        assert!(!pipeline.graph.contains(RenderPassKind::GamutMap));
        assert!(!pipeline.graph.contains(RenderPassKind::ToneMap));
    }

    #[test]
    fn sdr_bt709_to_sdr_skips_tone_mapping() {
        let source = SourceColorState::new(ColorPrimaries::Bt709, TransferFunction::Srgb);
        let target = TargetColorState::sdr(ColorPrimaries::Bt709);
        let pipeline = VideoRenderPipeline::new(source, target);

        assert!(!pipeline.requires_tone_mapping());
        assert!(!pipeline.graph.contains(RenderPassKind::ToneMap));
    }

    #[test]
    fn unknown_sdr_source_uses_sdr_reference_white() {
        let source = SourceColorState::new(ColorPrimaries::Unknown, TransferFunction::Unknown);

        assert_eq!(source.nominal_peak_nits, 100.0);
        assert_eq!(source.reference_white_nits, 100.0);
        assert!(!source.is_hdr());
    }

    #[test]
    fn retargeting_pipeline_preserves_render_options_and_rebuilds_graph() {
        let source = SourceColorState::new(ColorPrimaries::Bt2020, TransferFunction::Pq);
        let mut pipeline =
            VideoRenderPipeline::new(source, TargetColorState::sdr(ColorPrimaries::Bt709));
        pipeline.tone_map.operator = ToneMapOperator::Clip;
        pipeline.scaler.kernel = ScalerKernel::Nearest;

        let pipeline =
            pipeline.with_target(TargetColorState::apple_edr(ColorPrimaries::Bt709, 4.0));

        assert_eq!(pipeline.target.edr_headroom, 4.0);
        assert_eq!(pipeline.tone_map.operator, ToneMapOperator::Clip);
        assert_eq!(pipeline.scaler.kernel, ScalerKernel::Nearest);
        assert!(!pipeline.graph.contains(RenderPassKind::Scale));
    }

    #[test]
    fn luma_upscaler_adds_neural_upscale_pass() {
        let pipeline = VideoRenderPipeline::sdr_default();
        assert!(!pipeline.graph.contains(RenderPassKind::NeuralUpscale));

        let pipeline = pipeline.with_luma_upscaler(LumaUpscalerMode::ArtCnnC4F16);

        assert_eq!(pipeline.luma_upscaler, LumaUpscalerMode::ArtCnnC4F16);
        assert!(pipeline.graph.contains(RenderPassKind::NeuralUpscale));
    }

    #[test]
    fn matrix_defaults_follow_primaries() {
        let source = SourceColorState::new(ColorPrimaries::Bt2020, TransferFunction::Pq);
        let coeffs = source.matrix.luma_coefficients(source.primaries);

        assert!((coeffs.kr - 0.2627).abs() < 0.0001);
        assert!((coeffs.kg - 0.6780).abs() < 0.0001);
        assert!((coeffs.kb - 0.0593).abs() < 0.0001);
    }

    #[test]
    fn color_range_resolves_unspecified_to_fallback() {
        assert_eq!(
            ColorRange::Unspecified.resolve(ColorRange::Limited),
            ColorRange::Limited
        );
        assert_eq!(
            ColorRange::Full.resolve(ColorRange::Limited),
            ColorRange::Full
        );
    }

    #[test]
    fn hdr_metadata_prefers_mastering_display_peak() {
        let metadata = HdrMetadata::new(
            Some(MasteringDisplayMetadata {
                display_primaries: None,
                white_point: None,
                min_luminance_nits: Some(0.005),
                max_luminance_nits: Some(1000.0),
            }),
            Some(ContentLightMetadata {
                max_content_light_level_nits: 4000,
                max_frame_average_light_level_nits: 450,
            }),
        );

        let source = SourceColorState::new(ColorPrimaries::Bt2020, TransferFunction::Pq)
            .hdr_metadata(Some(metadata));

        assert_eq!(metadata.nominal_peak_nits(), Some(1000.0));
        assert_eq!(source.nominal_peak_nits, 1000.0);
        assert_eq!(source.hdr_metadata, Some(metadata));
    }

    #[test]
    fn hdr_metadata_falls_back_to_max_cll_when_mastering_peak_is_missing() {
        let metadata = HdrMetadata::new(
            Some(MasteringDisplayMetadata {
                display_primaries: None,
                white_point: None,
                min_luminance_nits: Some(0.005),
                max_luminance_nits: None,
            }),
            Some(ContentLightMetadata {
                max_content_light_level_nits: 4000,
                max_frame_average_light_level_nits: 450,
            }),
        );

        let source = SourceColorState::new(ColorPrimaries::Bt2020, TransferFunction::Pq)
            .hdr_metadata(Some(metadata));

        assert_eq!(metadata.nominal_peak_nits(), Some(4000.0));
        assert_eq!(source.nominal_peak_nits, 4000.0);
    }

    #[test]
    fn hdr_metadata_falls_back_to_mastering_peak_when_max_cll_is_missing() {
        let metadata = HdrMetadata::new(
            Some(MasteringDisplayMetadata {
                display_primaries: None,
                white_point: None,
                min_luminance_nits: Some(0.005),
                max_luminance_nits: Some(1000.0),
            }),
            Some(ContentLightMetadata {
                max_content_light_level_nits: 0,
                max_frame_average_light_level_nits: 450,
            }),
        );

        let source = SourceColorState::new(ColorPrimaries::Bt2020, TransferFunction::Pq)
            .hdr_metadata(Some(metadata));

        assert_eq!(metadata.nominal_peak_nits(), Some(1000.0));
        assert_eq!(source.nominal_peak_nits, 1000.0);
    }

    #[test]
    fn bt709_to_bt709_gamut_matrix_is_identity() {
        let matrix = source_to_target_rgb_matrix(ColorPrimaries::Bt709, ColorPrimaries::Bt709);
        assert_matrix_close(matrix.rows(), RgbMatrix::identity().rows(), 0.00001);
    }

    #[test]
    fn unknown_primaries_fall_back_to_bt709_for_gamut_matrix() {
        let matrix = source_to_target_rgb_matrix(ColorPrimaries::Unknown, ColorPrimaries::Bt709);
        assert_matrix_close(matrix.rows(), RgbMatrix::identity().rows(), 0.00001);
    }

    #[test]
    fn bt2020_to_bt709_gamut_matrix_is_stable() {
        let matrix = source_to_target_rgb_matrix(ColorPrimaries::Bt2020, ColorPrimaries::Bt709);

        assert_matrix_close(
            matrix.rows(),
            [
                [1.66049, -0.58764, -0.07285],
                [-0.12455, 1.13290, -0.00835],
                [-0.01815, -0.10058, 1.11873],
            ],
            0.0002,
        );
    }

    #[test]
    fn display_p3_to_bt709_gamut_matrix_is_stable() {
        let matrix = source_to_target_rgb_matrix(ColorPrimaries::DisplayP3, ColorPrimaries::Bt709);

        assert_matrix_close(
            matrix.rows(),
            [
                [1.22494, -0.22494, 0.0],
                [-0.04206, 1.04206, 0.0],
                [-0.01964, -0.07864, 1.09827],
            ],
            0.0002,
        );
    }

    #[test]
    fn pipeline_reports_gamut_mapping_when_primaries_differ() {
        let pipeline = VideoRenderPipeline::new(
            SourceColorState::new(ColorPrimaries::Bt2020, TransferFunction::Pq),
            TargetColorState::sdr(ColorPrimaries::Bt709),
        );

        assert!(pipeline.requires_gamut_mapping());
        assert!(pipeline.graph.contains(RenderPassKind::GamutMap));
    }

    #[test]
    fn packed_alpha_flag_survives_source_texture_mode_changes() {
        let pipeline = VideoRenderPipeline::sdr_default();
        let uniforms = VideoUniforms::from_pipeline(&pipeline, false, false)
            .packed_alpha_right(true)
            .rgb_texture_input();

        assert!(uniforms.has_packed_alpha_right());
        assert_eq!(uniforms.input_mode & VIDEO_INPUT_MODE_MASK, 1);
        assert_eq!(
            uniforms.input_mode & VIDEO_INPUT_PACKED_ALPHA_RIGHT,
            VIDEO_INPUT_PACKED_ALPHA_RIGHT,
        );
        assert!(!uniforms.packed_alpha_right(false).has_packed_alpha_right());
    }

    /// A curve shaped like the RPU's default luma mapping: two polynomial
    /// segments split at pivot 0.25, then one MMR segment of order 2.
    fn sample_dovi_metadata() -> DoviSourceMetadata {
        let mut reshaping = [DoviComponentCurve::default(); 3];
        reshaping[0].num_pivots = 4;
        reshaping[0].pivots = [0.0, 0.25, 0.5, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        reshaping[0].poly_coeffs[0] = [0.0, 0.5, 0.0];
        reshaping[0].poly_coeffs[1] = [1.0, 1.0, 0.5];
        reshaping[0].mmr_orders[2] = 2;
        reshaping[0].mmr_constants[2] = 0.25;
        reshaping[0].mmr_coeffs[2][0] = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7];
        reshaping[0].mmr_coeffs[2][1] = [0.8, 0.9, 1.0, 1.1, 1.2, 1.3, 1.4];
        DoviSourceMetadata {
            reshaping,
            nonlinear_matrix: RgbMatrix::new([
                [1.0, 0.5, 0.25],
                [0.75, 1.0, 0.125],
                [0.0625, 0.03125, 1.0],
            ]),
            nonlinear_offset: [0.25, 0.5, 0.5],
            rgb_to_lms: RgbMatrix::new([
                [0.356742, 0.592257, 0.051081],
                [0.156705, 0.747860, 0.095435],
                [0.0, 0.041455, 0.958545],
            ]),
            source_min_pq: 62,
            source_max_pq: 3079,
        }
    }

    #[test]
    fn dovi_uniforms_are_disabled_without_metadata() {
        let source = SourceColorState::new(ColorPrimaries::Bt2020, TransferFunction::Pq);
        assert_eq!(
            DoviUniforms::of_for_representation(&source, false),
            DoviUniforms::disabled()
        );
        assert_eq!(DoviUniforms::disabled().flags[0], 0.0);

        let target = TargetColorState::sdr(ColorPrimaries::Bt709);
        let pipeline = VideoRenderPipeline::new(source, target);
        let uniforms = VideoUniforms::from_pipeline(&pipeline, false, false).dovi;

        assert_eq!(uniforms, DoviUniforms::disabled());
        assert_eq!(uniforms.flags[0], 0.0);
    }

    #[test]
    fn dovi_uniforms_pack_pivots_poly_and_mmr() {
        let metadata = sample_dovi_metadata();
        let source = SourceColorState::new(ColorPrimaries::Bt2020, TransferFunction::Pq)
            .dovi(Some(metadata));
        let uniforms = DoviUniforms::of_for_representation(&source, true);

        assert_eq!(uniforms.flags, [1.0, 3.0, 0.0, 0.0]);
        // Interior pivots skip the endpoints; padding gets the sentinel.
        assert_eq!(
            uniforms.pivots[0],
            [0.25, 0.5, DOVI_PIVOT_SENTINEL, DOVI_PIVOT_SENTINEL]
        );
        assert_eq!(uniforms.pivots[1], [DOVI_PIVOT_SENTINEL; 4]);
        assert_eq!(uniforms.bounds[0], [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(uniforms.coefficients[0], [0.0, 0.5, 0.0, 0.0]);
        assert_eq!(uniforms.coefficients[1], [1.0, 1.0, 0.5, 0.0]);
        // MMR rows start after two polynomial segments; the order rides in w.
        assert_eq!(uniforms.coefficients[2], [0.25, 0.0, 0.0, 2.0]);
        assert_eq!(uniforms.mmr[0], [0.1, 0.2, 0.3, 0.0]);
        assert_eq!(uniforms.mmr[1], [0.4, 0.5, 0.6, 0.7]);
        assert_eq!(uniforms.mmr[2], [0.8, 0.9, 1.0, 0.0]);
        assert_eq!(uniforms.mmr[3], [1.1, 1.2, 1.3, 1.4]);
        assert_eq!(uniforms.mmr[4], [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn dovi_uniforms_apply_signal_offset_correction() {
        let metadata = sample_dovi_metadata();
        let source = SourceColorState::new(ColorPrimaries::Bt2020, TransferFunction::Pq)
            .dovi(Some(metadata));
        let uniforms = DoviUniforms::of_for_representation(&source, true);
        let correction = 1024.0_f32 / 1023.0;

        assert!((uniforms.nonlinear_offset[0] - 0.25 * correction).abs() < 1e-6);
        assert!((uniforms.nonlinear_offset[1] - 0.5 * correction).abs() < 1e-6);
        assert_eq!(uniforms.nonlinear_matrix[0], [1.0, 0.5, 0.25, 0.0]);
    }

    #[test]
    fn dovi_uniforms_use_the_uploaded_sample_depth_for_offsets() {
        let metadata = sample_dovi_metadata();
        let source = SourceColorState::new(ColorPrimaries::Bt2020, TransferFunction::Pq)
            .dovi(Some(metadata));
        let p010 = DoviUniforms::of_for_representation(&source, true);
        let nv12 = DoviUniforms::of_for_representation(&source, false);
        assert!((p010.nonlinear_offset[0] - 0.25 * 1024.0 / 1023.0).abs() < 1e-6);
        assert!((nv12.nonlinear_offset[0] - 0.25 * 256.0 / 255.0).abs() < 1e-6);

        let uniforms = VideoUniforms::from_pipeline(
            &VideoRenderPipeline::new(source, TargetColorState::sdr(ColorPrimaries::Bt709)),
            true,
            false,
        );
        let converted = uniforms.with_p010_representation(false);
        assert_eq!(converted.is_p010, 0);
        assert!((converted.dovi.nonlinear_offset[0] - nv12.nonlinear_offset[0]).abs() < 1e-6);
    }

    #[test]
    fn dovi_lms_matrix_composite_matches_libplacebo_default() {
        // libplacebo composites its hard-coded HPE LMS->RGB matrix with the
        // RPU's rgb_to_lms rows; for the RPU default matrix the product is
        // this near-diagonal, white-preserving transform.
        let matrix = dovi_lms_to_rgb_matrix(RgbMatrix::new([
            [5845.0 / 16384.0, 9702.0 / 16384.0, 837.0 / 16384.0],
            [2568.0 / 16384.0, 12256.0 / 16384.0, 1561.0 / 16384.0],
            [0.0, 679.0 / 16384.0, 15705.0 / 16384.0],
        ]));

        let expected = [
            [0.753741425, 0.198592403, 0.047534181],
            [0.045791140, 0.941773555, 0.012526896],
            [-0.001211792, 0.017623405, 0.983739703],
        ];
        for (row, expected_row) in matrix.rows().iter().zip(expected) {
            for (value, expected_value) in row.iter().zip(expected_row) {
                assert!((value - expected_value).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn dovi_source_uses_rpu_peak_and_bt2020_primaries() {
        let metadata = sample_dovi_metadata();
        let source = SourceColorState::new(ColorPrimaries::DisplayP3, TransferFunction::Pq)
            .hdr_metadata(Some(HdrMetadata::new(
                Some(MasteringDisplayMetadata {
                    display_primaries: None,
                    white_point: None,
                    min_luminance_nits: Some(0.005),
                    max_luminance_nits: Some(4000.0),
                }),
                None,
            )))
            .dovi(Some(metadata));

        // PQ code 3079 is the 12-bit encoding of ~1000 nits.
        assert!((source.nominal_peak_nits - 1000.0).abs() < 5.0);
        assert!(
            (source
                .hdr_metadata
                .unwrap()
                .mastering_display
                .unwrap()
                .min_luminance_nits
                .unwrap()
                - 0.005)
                .abs()
                < 0.0001
        );
        assert_eq!(source.primaries, ColorPrimaries::Bt2020);
        assert!(source.is_hdr());
        assert_eq!(pq_code_to_nits(0), 0.0);
    }

    #[test]
    fn dovi_source_peak_falls_back_to_pq_default_when_rpu_max_pq_is_zero() {
        let mut metadata = sample_dovi_metadata();
        metadata.source_max_pq = 0;
        let source = SourceColorState::new(ColorPrimaries::Unknown, TransferFunction::Unknown)
            .dovi(Some(metadata));

        assert_eq!(source.transfer, TransferFunction::Pq);
        assert_eq!(source.reference_white_nits, 203.0);
        assert_eq!(source.nominal_peak_nits, 1000.0);
        assert!(source.nominal_peak_nits > source.reference_white_nits);
    }

    #[test]
    fn dovi_source_forces_pq_when_stream_tags_are_missing() {
        // libplacebo forces BT.2020/PQ from the RPU because P5/P8 VUI tags are
        // unreliable; without this an unspecified trc would decode the
        // reshaped PQ signal with an sRGB gamma.
        let source = SourceColorState::new(ColorPrimaries::Unknown, TransferFunction::Unknown)
            .dovi(Some(sample_dovi_metadata()));

        assert_eq!(source.transfer, TransferFunction::Pq);
        assert_eq!(source.primaries, ColorPrimaries::Bt2020);
        assert_eq!(source.reference_white_nits, 203.0);
        assert_eq!(transfer_code(source.transfer), 3);
        assert!(source.is_hdr());
    }

    #[test]
    fn dovi_source_adds_reshape_pass_and_tone_maps_to_sdr() {
        let source = SourceColorState::new(ColorPrimaries::Bt2020, TransferFunction::Pq)
            .dovi(Some(sample_dovi_metadata()));
        let pipeline =
            VideoRenderPipeline::new(source, TargetColorState::sdr(ColorPrimaries::Bt709));

        assert!(pipeline.graph.contains(RenderPassKind::DoviReshape));
        assert!(pipeline.requires_tone_mapping());
        assert!(pipeline.requires_gamut_mapping());

        let pipeline =
            VideoRenderPipeline::new(source, TargetColorState::hdr10(ColorPrimaries::Bt2020));
        assert!(!pipeline.requires_tone_mapping());
    }

    #[test]
    fn dovi_formulas_are_present_across_video_shaders() {
        let shaders = [
            include_str!("wgpu_video.wgsl"),
            include_str!("metal/apple.rs"),
            include_str!("d3d11.rs"),
        ];
        for shader in shaders {
            assert!(shader.contains("dovi_flags"));
            assert!(shader.contains("dovi_pivots"));
            assert!(shader.contains("dovi_bounds"));
            assert!(shader.contains("dovi_coefficients"));
            assert!(shader.contains("dovi_mmr"));
            assert!(shader.contains("dovi_nonlinear_matrix"));
            assert!(shader.contains("dovi_nonlinear_offset"));
            assert!(shader.contains("dovi_lms_matrix"));
            assert!(shader.contains("dovi_reshaped_signal"));
            assert!(shader.contains("dovi_signal_to_pq_rgb"));
            assert!(shader.contains("dovi_lms_to_rgb"));
        }
    }

    fn assert_matrix_close(actual: [[f32; 3]; 3], expected: [[f32; 3]; 3], epsilon: f32) {
        for row in 0..3 {
            for col in 0..3 {
                assert!(
                    (actual[row][col] - expected[row][col]).abs() <= epsilon,
                    "matrix[{row}][{col}] expected {}, got {}",
                    expected[row][col],
                    actual[row][col]
                );
            }
        }
    }
}
