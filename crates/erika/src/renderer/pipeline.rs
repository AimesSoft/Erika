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
}

impl Default for ToneMapOperator {
    fn default() -> Self {
        Self::Mobius
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
            operator: ToneMapOperator::Mobius,
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
        }
    }

    pub fn rgb_texture_input(mut self) -> Self {
        self.input_mode = (self.input_mode & !VIDEO_INPUT_MODE_MASK) | 1;
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

    #[test]
    fn p010_unorm_prescale_is_present_across_video_shaders() {
        // P010 packs 10-bit codes as code << 6, so the UNORM sampler reads
        // 64/65472 for full black instead of the nominal 64/1023. Every
        // backend must undo this before range expansion, or limited-range
        // P010 renders slightly lifted and slightly dull.
        let shaders = [
            include_str!("wgpu_video.wgsl"),
            include_str!("metal/apple.rs"),
            include_str!("d3d11.rs"),
        ];
        for shader in shaders {
            assert!(shader.contains("65535.0 / 65472.0"));
            assert!(shader.contains("P010 stores 10-bit codes as code << 6"));
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
