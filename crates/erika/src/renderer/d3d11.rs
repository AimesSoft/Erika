use std::ffi::c_void;
use std::mem;
use std::ptr;

use ::windows::Win32::Foundation::{HANDLE, HMODULE, HWND};
use ::windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use ::windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_10_1,
    D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
    D3D_SRV_DIMENSION_TEXTURE2DARRAY, ID3DBlob,
};
use ::windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_SHADER_RESOURCE, D3D11_BLEND_DESC,
    D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ONE, D3D11_BLEND_OP_ADD, D3D11_BLEND_SRC_ALPHA,
    D3D11_BUFFER_DESC, D3D11_COLOR_WRITE_ENABLE_ALL, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_INPUT_ELEMENT_DESC, D3D11_INPUT_PER_VERTEX_DATA, D3D11_RENDER_TARGET_BLEND_DESC,
    D3D11_SAMPLER_DESC, D3D11_SDK_VERSION, D3D11_SHADER_RESOURCE_VIEW_DESC,
    D3D11_SHADER_RESOURCE_VIEW_DESC_0, D3D11_SUBRESOURCE_DATA, D3D11_TEX2D_ARRAY_SRV,
    D3D11_TEX2D_SRV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_VIEWPORT, D3D11CreateDevice,
    ID3D11BlendState, ID3D11Buffer, ID3D11Device, ID3D11DeviceContext, ID3D11InputLayout,
    ID3D11PixelShader, ID3D11RenderTargetView, ID3D11Resource, ID3D11SamplerState,
    ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
};
use ::windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709,
    DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020, DXGI_COLOR_SPACE_TYPE, DXGI_FORMAT,
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_FORMAT_P010, DXGI_FORMAT_R8_UNORM,
    DXGI_FORMAT_R8G8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R10G10B10A2_UNORM,
    DXGI_FORMAT_R16_UNORM, DXGI_FORMAT_R16G16_UNORM, DXGI_FORMAT_R32G32_FLOAT, DXGI_SAMPLE_DESC,
};
use ::windows::Win32::Graphics::Dxgi::{
    DXGI_HDR_METADATA_HDR10, DXGI_HDR_METADATA_TYPE_HDR10, DXGI_HDR_METADATA_TYPE_NONE,
    DXGI_PRESENT, DXGI_PRESENT_PARAMETERS, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_COLOR_SPACE_SUPPORT_FLAG_PRESENT, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter, IDXGIDevice,
    IDXGIFactory2, IDXGIResource, IDXGISwapChain1, IDXGISwapChain3, IDXGISwapChain4,
};
use ::windows::core::{Interface, PCSTR};

use crate::core::{
    ColorPrimaries, LumaUpscalerBackendStatus, PlatformSurface, PlayerError, PlayerVideoFrame,
    RenderFrameContext, RendererBackend, RendererRuntimeStats, Result, TransferFunction,
    WgpuSurfaceKind,
};
use crate::danmaku::{DanmakuGlyphAtlas, DanmakuGlyphInstance, DanmakuRenderPlan};
use crate::ffmpeg::Frame;
use crate::overlay::OverlayFrame;
use crate::renderer::metal::MetalRendererConfig;
use crate::renderer::pipeline::{
    Chromaticity, LumaUpscalerMode, PrimariesCoordinates, SourceColorState, TargetColorState,
    VideoRenderPipeline, VideoUniforms, primaries_coordinates,
};
use crate::subtitle::AssColor;

const SDR_SWAPCHAIN_FORMAT: DXGI_FORMAT = DXGI_FORMAT_B8G8R8A8_UNORM;
const HDR10_SWAPCHAIN_FORMAT: DXGI_FORMAT = DXGI_FORMAT_R10G10B10A2_UNORM;
const SHADER_SOURCE: &[u8] = br#"
struct VsIn {
    float2 position : POSITION;
    float2 texcoord : TEXCOORD0;
};

struct VsOut {
    float4 position : SV_Position;
    float2 texcoord : TEXCOORD0;
};

cbuffer VideoConstants : register(b0) {
    uint is_p010;
    uint full_range;
    uint source_transfer;
    uint target_transfer;
    uint tone_map;
    uint edr_output;
    uint reserved0;
    uint reserved1;
    float4 nits;
    float4 luma_coefficients;
    float4 gamut_matrix_rows[3];
};

Texture2D lumaTex : register(t0);
Texture2D chromaTex : register(t1);
SamplerState videoSampler : register(s0);

float source_peak_nits() {
    return max(nits.x, 1.0);
}

float target_peak_nits() {
    return max(nits.y, 1.0);
}

float source_reference_white_nits() {
    return max(nits.z, 1.0);
}

float target_reference_white_nits() {
    return max(nits.w, 1.0);
}

float pq_eotf(float encoded) {
    const float m1 = 0.1593017578125;
    const float m2 = 78.84375;
    const float c1 = 0.8359375;
    const float c2 = 18.8515625;
    const float c3 = 18.6875;
    float p = pow(max(encoded, 0.0), 1.0 / m2);
    float num = max(p - c1, 0.0);
    float den = max(c2 - c3 * p, 0.000001);
    return pow(num / den, 1.0 / m1);
}

float pq_inverse_eotf(float normalized_nits) {
    const float m1 = 0.1593017578125;
    const float m2 = 78.84375;
    const float c1 = 0.8359375;
    const float c2 = 18.8515625;
    const float c3 = 18.6875;
    float p = pow(clamp(normalized_nits, 0.0, 1.0), m1);
    return pow((c1 + c2 * p) / max(1.0 + c3 * p, 0.000001), m2);
}

float3 transfer_to_source_reference_linear(float3 rgb_in) {
    float3 rgb = max(rgb_in, float3(0.0, 0.0, 0.0));
    if (source_transfer == 3u) {
        const float pq_absolute_peak_nits = 10000.0;
        return float3(pq_eotf(rgb.r), pq_eotf(rgb.g), pq_eotf(rgb.b))
            * (pq_absolute_peak_nits / source_reference_white_nits());
    }
    if (source_transfer == 1u) {
        return pow(rgb, float3(2.2, 2.2, 2.2));
    }
    if (source_transfer == 2u) {
        return pow(rgb, float3(2.4, 2.4, 2.4));
    }
    return rgb;
}

float3 source_reference_to_nits(float3 rgb) {
    return max(rgb, float3(0.0, 0.0, 0.0)) * source_reference_white_nits();
}

float3 tone_map_nits(float3 input_nits) {
    float source_peak = source_peak_nits();
    float target_peak = target_peak_nits();
    float3 x = max(input_nits, float3(0.0, 0.0, 0.0)) / target_peak;
    float white = max(source_peak / target_peak, 1.0);
    if (tone_map == 1u) {
        float white2 = white * white;
        return target_peak * clamp((x * (float3(1.0, 1.0, 1.0) + x / white2)) / (float3(1.0, 1.0, 1.0) + x), float3(0.0, 0.0, 0.0), float3(1.0, 1.0, 1.0));
    }
    if (tone_map == 2u) {
        float knee = 0.75;
        float denom = max(white - knee, 0.0001);
        float3 knee3 = float3(knee, knee, knee);
        float3 t = clamp((x - knee3) / denom, float3(0.0, 0.0, 0.0), float3(1.0, 1.0, 1.0));
        float3 shoulder = knee3 + (1.0 - knee) * (float3(1.0, 1.0, 1.0) - pow(float3(1.0, 1.0, 1.0) - t, float3(2.0, 2.0, 2.0)));
        return target_peak * lerp(x, shoulder, step(knee3, x));
    }
    return target_peak * clamp(x, float3(0.0, 0.0, 0.0), float3(1.0, 1.0, 1.0));
}

float3 apply_gamut_map(float3 rgb) {
    return float3(
        dot(gamut_matrix_rows[0].xyz, rgb),
        dot(gamut_matrix_rows[1].xyz, rgb),
        dot(gamut_matrix_rows[2].xyz, rgb)
    );
}

float3 target_nits_to_reference_linear(float3 input_nits) {
    return max(input_nits, float3(0.0, 0.0, 0.0)) / target_reference_white_nits();
}

float3 target_reference_linear_to_output(float3 rgb) {
    if (target_transfer == 3u) {
        const float pq_absolute_peak_nits = 10000.0;
        float3 out_nits = max(rgb, float3(0.0, 0.0, 0.0)) * target_reference_white_nits();
        return float3(
            pq_inverse_eotf(out_nits.r / pq_absolute_peak_nits),
            pq_inverse_eotf(out_nits.g / pq_absolute_peak_nits),
            pq_inverse_eotf(out_nits.b / pq_absolute_peak_nits)
        );
    }
    if (edr_output != 0u) {
        return max(rgb, float3(0.0, 0.0, 0.0));
    }
    if (target_transfer == 1u) {
        return pow(max(rgb, float3(0.0, 0.0, 0.0)), float3(1.0 / 2.2, 1.0 / 2.2, 1.0 / 2.2));
    }
    if (target_transfer == 2u) {
        return pow(max(rgb, float3(0.0, 0.0, 0.0)), float3(1.0 / 2.4, 1.0 / 2.4, 1.0 / 2.4));
    }
    return rgb;
}

float4 final_output(float3 rgb) {
    if (target_transfer == 3u) {
        return float4(clamp(rgb, float3(0.0, 0.0, 0.0), float3(1.0, 1.0, 1.0)), 1.0);
    }
    if (edr_output != 0u) {
        float headroom = max(target_peak_nits() / target_reference_white_nits(), 1.0);
        return float4(clamp(rgb, float3(0.0, 0.0, 0.0), float3(headroom, headroom, headroom)), 1.0);
    }
    return float4(clamp(rgb, float3(0.0, 0.0, 0.0), float3(1.0, 1.0, 1.0)), 1.0);
}

void expand_ycbcr_range(float y_in, float2 cbcr_in, out float y, out float2 cbcr) {
    if (full_range != 0u) {
        y = y_in;
        cbcr = cbcr_in - float2(0.5, 0.5);
        return;
    }
    if (is_p010 != 0u) {
        y = (y_in - (64.0 / 1023.0)) * (1023.0 / 876.0);
        cbcr = (cbcr_in - float2(512.0 / 1023.0, 512.0 / 1023.0)) * (1023.0 / 896.0);
        return;
    }
    y = (y_in - (16.0 / 255.0)) * (255.0 / 219.0);
    cbcr = (cbcr_in - float2(128.0 / 255.0, 128.0 / 255.0)) * (255.0 / 224.0);
}

VsOut vs_main(VsIn input) {
    VsOut output;
    output.position = float4(input.position, 0.0, 1.0);
    output.texcoord = input.texcoord;
    return output;
}

float4 ps_main(VsOut input) : SV_Target {
    float y_sample = lumaTex.Sample(videoSampler, input.texcoord).r;
    float2 cbcr_sample = chromaTex.Sample(videoSampler, input.texcoord).rg;
    float y;
    float2 cbcr;
    expand_ycbcr_range(y_sample, cbcr_sample, y, cbcr);

    float kr = luma_coefficients.x;
    float kg = max(luma_coefficients.y, 0.000001);
    float kb = luma_coefficients.z;
    float3 rgb;
    rgb.r = y + 2.0 * (1.0 - kr) * cbcr.y;
    rgb.b = y + 2.0 * (1.0 - kb) * cbcr.x;
    rgb.g = (y - kr * rgb.r - kb * rgb.b) / kg;
    rgb = transfer_to_source_reference_linear(rgb);
    rgb = apply_gamut_map(rgb);
    rgb = source_reference_to_nits(rgb);
    rgb = tone_map_nits(rgb);
    rgb = target_nits_to_reference_linear(rgb);
    rgb = target_reference_linear_to_output(rgb);
    return final_output(rgb);
}
"#;

const OVERLAY_SHADER_SOURCE: &[u8] = br#"
struct VsIn {
    float2 position : POSITION;
    float2 texcoord : TEXCOORD0;
};

struct VsOut {
    float4 position : SV_Position;
    float2 texcoord : TEXCOORD0;
};

cbuffer OverlayConstants : register(b0) {
    float4 rect;
    float4 tex_rect;
    float2 viewport;
    uint overlay_mode;
    uint reserved0;
    float4 color;
};

Texture2D overlayTex : register(t0);
SamplerState overlaySampler : register(s0);

VsOut overlay_vs_main(VsIn input) {
    float2 pixel = rect.xy + input.texcoord * rect.zw;
    float2 safe_viewport = max(viewport, float2(1.0, 1.0));
    float2 ndc = float2(
        pixel.x / safe_viewport.x * 2.0 - 1.0,
        1.0 - pixel.y / safe_viewport.y * 2.0
    );
    VsOut output;
    output.position = float4(ndc, 0.0, 1.0);
    output.texcoord = tex_rect.xy + input.texcoord * tex_rect.zw;
    return output;
}

float4 overlay_ps_main(VsOut input) : SV_Target {
    float4 sampled = overlayTex.Sample(overlaySampler, input.texcoord);
    if (overlay_mode == 1u) {
        return float4(color.rgb, color.a * sampled.r);
    }
    return sampled;
}
"#;

#[repr(C)]
#[derive(Clone, Copy)]
struct VideoVertex {
    position: [f32; 2],
    texcoord: [f32; 2],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
struct OverlayUniforms {
    rect: [f32; 4],
    tex_rect: [f32; 4],
    viewport: [f32; 2],
    overlay_mode: u32,
    reserved0: u32,
    color: [f32; 4],
}

impl OverlayUniforms {
    fn rgba_plane(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        viewport_w: u32,
        viewport_h: u32,
    ) -> Self {
        Self {
            rect: [x as f32, y as f32, width as f32, height as f32],
            tex_rect: [0.0, 0.0, 1.0, 1.0],
            viewport: [viewport_w.max(1) as f32, viewport_h.max(1) as f32],
            overlay_mode: 0,
            reserved0: 0,
            color: [1.0, 1.0, 1.0, 1.0],
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn alpha_atlas(
        color_rgba: u32,
        place_x: i32,
        place_y: i32,
        place_w: u32,
        place_h: u32,
        atlas_x: u32,
        atlas_w: u32,
        atlas_h: u32,
        viewport_w: u32,
        viewport_h: u32,
    ) -> Self {
        let color = AssColor::from_libass_rgba(color_rgba);
        let aw = atlas_w.max(1) as f32;
        let ah = atlas_h.max(1) as f32;
        Self {
            rect: [
                place_x as f32,
                place_y as f32,
                place_w as f32,
                place_h as f32,
            ],
            tex_rect: [
                atlas_x as f32 / aw,
                0.0,
                place_w as f32 / aw,
                place_h as f32 / ah,
            ],
            viewport: [viewport_w.max(1) as f32, viewport_h.max(1) as f32],
            overlay_mode: 1,
            reserved0: 0,
            color: [
                f32::from(color.red) / 255.0,
                f32::from(color.green) / 255.0,
                f32::from(color.blue) / 255.0,
                f32::from(color.alpha) / 255.0,
            ],
        }
    }

    fn alpha_atlas_rect(
        color: [f32; 4],
        rect: [f32; 4],
        tex_rect: [f32; 4],
        viewport_w: u32,
        viewport_h: u32,
    ) -> Self {
        Self {
            rect,
            tex_rect,
            viewport: [viewport_w.max(1) as f32, viewport_h.max(1) as f32],
            overlay_mode: 1,
            reserved0: 0,
            color,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct D3d11RendererStats {
    pub surface_width: u32,
    pub surface_height: u32,
    pub rendered_frames: u64,
    pub hardware_video_frames: u64,
    pub zero_copy_video_frames: u64,
    pub direct_zero_copy_video_frames: u64,
    pub shared_handle_video_frames: u64,
    pub cpu_video_frame_fallbacks: u64,
    pub hdr_source_frames: u64,
    pub hdr10_output_frames: u64,
    pub sdr_tonemap_frames: u64,
    pub hdr10_metadata_updates: u64,
    pub hdr10_metadata_failures: u64,
    pub hdr10_output_failures: u64,
    pub hdr10_output_active: bool,
    pub import_failures: u64,
    pub prepared_overlay_frames: u64,
    pub prepared_overlay_subtitle_planes: u64,
    pub overlay_alpha_atlas_uploads: u64,
    pub overlay_alpha_atlas_reuses: u64,
    pub danmaku_passes: u64,
    pub danmaku_items: u64,
    pub attached: bool,
}

struct D3d11DeviceState {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    input_layout: ID3D11InputLayout,
    vertex_buffer: ID3D11Buffer,
    constants: ID3D11Buffer,
    sampler: ID3D11SamplerState,
    overlay_vertex_shader: ID3D11VertexShader,
    overlay_pixel_shader: ID3D11PixelShader,
    overlay_constants: ID3D11Buffer,
    overlay_sampler: ID3D11SamplerState,
    overlay_blend: ID3D11BlendState,
}

struct AttachedSurface {
    hwnd: HWND,
    width: u32,
    height: u32,
    scale: f64,
    output_mode: D3d11OutputMode,
    swapchain: Option<IDXGISwapChain1>,
    render_target: Option<ID3D11RenderTargetView>,
}

impl AttachedSurface {
    fn physical_size(&self) -> D3d11PhysicalSize {
        physical_size(self.width, self.height, self.scale)
    }
}

struct ImportedVideoFrame {
    _frame: Frame,
    _texture: ID3D11Texture2D,
    luma: ID3D11ShaderResourceView,
    chroma: ID3D11ShaderResourceView,
    width: u32,
    height: u32,
    tex_rect: D3d11TexRect,
    _array_index: u32,
    constants: VideoUniforms,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct D3d11PhysicalSize {
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Copy)]
struct D3d11DrawRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl D3d11DrawRect {
    fn full(width: u32, height: u32) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: width.max(1) as f32,
            height: height.max(1) as f32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct D3d11TexRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl D3d11TexRect {
    const FULL: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };

    fn visible_region(
        visible_width: u32,
        visible_height: u32,
        texture_width: u32,
        texture_height: u32,
    ) -> Self {
        let texture_width = texture_width.max(1);
        let texture_height = texture_height.max(1);
        Self {
            x: 0.0,
            y: 0.0,
            width: visible_width.max(1).min(texture_width) as f32 / texture_width as f32,
            height: visible_height.max(1).min(texture_height) as f32 / texture_height as f32,
        }
    }

    fn right(self) -> f32 {
        (self.x + self.width).min(1.0)
    }

    fn bottom(self) -> f32 {
        (self.y + self.height).min(1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum D3d11VideoImportMode {
    DirectDecoderDevice,
    SharedHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum D3d11OutputMode {
    #[default]
    Sdr,
    Hdr10,
}

impl D3d11OutputMode {
    fn swapchain_format(self) -> DXGI_FORMAT {
        match self {
            Self::Sdr => SDR_SWAPCHAIN_FORMAT,
            Self::Hdr10 => HDR10_SWAPCHAIN_FORMAT,
        }
    }

    fn color_space(self) -> DXGI_COLOR_SPACE_TYPE {
        match self {
            Self::Sdr => DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709,
            Self::Hdr10 => DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020,
        }
    }

    fn target_color_for_source(self, source: SourceColorState) -> TargetColorState {
        let _ = source;
        match self {
            Self::Sdr => TargetColorState::sdr(ColorPrimaries::Bt709),
            Self::Hdr10 => TargetColorState::hdr10(ColorPrimaries::Bt2020),
        }
    }
}

#[derive(Clone)]
struct D3d11OverlayTexture {
    _texture: ID3D11Texture2D,
    view: ID3D11ShaderResourceView,
}

struct D3d11OverlayDraw {
    texture: D3d11OverlayTexture,
    constants: OverlayUniforms,
}

struct D3d11DanmakuAtlasCache {
    version: u64,
    width: u32,
    height: u32,
    stride: usize,
    fill: D3d11OverlayTexture,
    outline: D3d11OverlayTexture,
}

impl D3d11DanmakuAtlasCache {
    fn can_reuse_for(&self, atlas: &DanmakuGlyphAtlas) -> bool {
        self.version == atlas.version
            && self.width == atlas.width
            && self.height == atlas.height
            && self.stride == atlas.stride
    }
}

pub struct D3d11Renderer {
    state: Option<D3d11DeviceState>,
    surface: Option<AttachedSurface>,
    current_video: Option<ImportedVideoFrame>,
    danmaku_atlas_cache: Option<D3d11DanmakuAtlasCache>,
    upscaler_mode: LumaUpscalerMode,
    hdr10_output_unavailable: bool,
    stats: D3d11RendererStats,
}

impl D3d11Renderer {
    pub fn new() -> Result<Self> {
        Self::with_config(MetalRendererConfig::default())
    }

    pub fn with_config(config: MetalRendererConfig) -> Result<Self> {
        Ok(Self {
            state: None,
            surface: None,
            current_video: None,
            danmaku_atlas_cache: None,
            upscaler_mode: config.luma_upscaler,
            hdr10_output_unavailable: false,
            stats: D3d11RendererStats::default(),
        })
    }

    pub fn stats(&self) -> D3d11RendererStats {
        self.stats
    }

    fn ensure_default_device(&mut self) -> Result<()> {
        if self.state.is_some() {
            return Ok(());
        }
        let (device, context) = create_default_device()?;
        self.set_device(device, context)
    }

    fn ensure_device_for_texture(&mut self, texture: &ID3D11Texture2D) -> Result<()> {
        let frame_device = unsafe { texture.GetDevice() }
            .map_err(|error| d3d_error("ID3D11Texture2D::GetDevice", error))?;
        if self
            .state
            .as_ref()
            .is_some_and(|state| state.device.as_raw() == frame_device.as_raw())
        {
            return Ok(());
        }
        let context = unsafe { frame_device.GetImmediateContext() }
            .map_err(|error| d3d_error("ID3D11Device::GetImmediateContext", error))?;
        self.current_video = None;
        self.danmaku_atlas_cache = None;
        self.set_device(frame_device, context)
    }

    fn set_device(&mut self, device: ID3D11Device, context: ID3D11DeviceContext) -> Result<()> {
        let state = D3d11DeviceState::new(device, context)?;
        self.state = Some(state);
        self.recreate_surface_targets()?;
        Ok(())
    }

    fn recreate_surface_targets(&mut self) -> Result<()> {
        let Some(surface) = self.surface.as_mut() else {
            return Ok(());
        };
        let Some(state) = self.state.as_ref() else {
            return Ok(());
        };
        trace("recreate_surface_targets: reset");
        surface.render_target = None;
        surface.swapchain = None;
        let output_mode = surface.output_mode;
        trace("recreate_surface_targets: create_swapchain");
        surface.swapchain = Some(create_swapchain(
            &state.device,
            surface.hwnd,
            surface.width,
            surface.height,
            surface.scale,
            output_mode.swapchain_format(),
        )?);
        configure_swapchain_color_space(
            surface.swapchain.as_ref().expect("swapchain just created"),
            output_mode,
        )?;
        if matches!(output_mode, D3d11OutputMode::Sdr) {
            let _ = clear_hdr_metadata(surface.swapchain.as_ref().expect("swapchain just created"));
        }
        trace("recreate_surface_targets: create_render_target");
        surface.render_target = Some(create_render_target(
            &state.device,
            surface.swapchain.as_ref().expect("swapchain just created"),
        )?);
        let physical = surface.physical_size();
        self.stats.surface_width = physical.width;
        self.stats.surface_height = physical.height;
        self.stats.hdr10_output_active = matches!(output_mode, D3d11OutputMode::Hdr10);
        Ok(())
    }

    fn set_output_mode(&mut self, output_mode: D3d11OutputMode) -> Result<()> {
        let Some(surface) = self.surface.as_mut() else {
            self.stats.hdr10_output_active = false;
            return Ok(());
        };
        if surface.output_mode == output_mode
            && surface.swapchain.is_some()
            && surface.render_target.is_some()
        {
            self.stats.hdr10_output_active = matches!(output_mode, D3d11OutputMode::Hdr10);
            return Ok(());
        }
        surface.output_mode = output_mode;
        self.current_video = None;
        self.recreate_surface_targets()
    }

    fn select_output_mode_for_source(
        &mut self,
        source: SourceColorState,
    ) -> Result<D3d11OutputMode> {
        let source_is_hdr = source.is_hdr();
        if source_is_hdr {
            self.stats.hdr_source_frames += 1;
        }
        if matches!(source.transfer, TransferFunction::Pq)
            && self.try_enable_hdr10_output(source)?
        {
            self.stats.hdr10_output_frames += 1;
            return Ok(D3d11OutputMode::Hdr10);
        }

        self.set_output_mode(D3d11OutputMode::Sdr)?;
        if source_is_hdr {
            self.stats.sdr_tonemap_frames += 1;
        }
        Ok(D3d11OutputMode::Sdr)
    }

    fn try_enable_hdr10_output(&mut self, source: SourceColorState) -> Result<bool> {
        if self.state.is_none() || self.surface.is_none() {
            return Ok(false);
        }
        if self.hdr10_output_unavailable {
            return Ok(false);
        }

        if let Err(error) = self.set_output_mode(D3d11OutputMode::Hdr10) {
            self.stats.hdr10_output_failures += 1;
            self.hdr10_output_unavailable = true;
            trace(&format!("hdr10 output unavailable: {error}"));
            self.set_output_mode(D3d11OutputMode::Sdr)?;
            return Ok(false);
        }
        if let Err(error) = self.update_hdr10_metadata(source) {
            self.stats.hdr10_metadata_failures += 1;
            self.hdr10_output_unavailable = true;
            trace(&format!("hdr10 metadata unavailable: {error}"));
            self.set_output_mode(D3d11OutputMode::Sdr)?;
            return Ok(false);
        }
        Ok(true)
    }

    fn update_hdr10_metadata(&mut self, source: SourceColorState) -> Result<()> {
        let swapchain = self
            .surface
            .as_ref()
            .and_then(|surface| surface.swapchain.as_ref())
            .ok_or_else(|| {
                PlayerError::Renderer("d3d11: no swapchain for HDR10 metadata".into())
            })?;
        set_hdr10_metadata(swapchain, dxgi_hdr10_metadata(source))?;
        self.stats.hdr10_metadata_updates += 1;
        Ok(())
    }

    fn import_d3d11va_frame(&mut self, frame: &PlayerVideoFrame) -> Result<()> {
        let Some(texture_ref) = frame.frame.d3d11va_texture() else {
            return Err(PlayerError::Renderer(
                "d3d11: hardware frame is not a D3D11VA texture".to_string(),
            ));
        };
        let retained_frame = frame.frame.try_clone_ref().map_err(|error| {
            PlayerError::Renderer(format!("d3d11: av_frame_ref failed: {error}"))
        })?;
        let source_texture = clone_d3d11_texture(texture_ref.raw_texture())?;
        let (texture, import_mode) = self.import_texture_for_current_device(&source_texture)?;

        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };
        let texture_format = D3d11VideoTextureFormat::from_dxgi(desc.Format).ok_or_else(|| {
            PlayerError::Renderer(format!(
                "d3d11: unsupported D3D11VA texture format {:?}",
                desc.Format
            ))
        })?;
        let array_index = texture_ref.array_index();
        if array_index >= desc.ArraySize {
            return Err(PlayerError::Renderer(format!(
                "d3d11: D3D11VA array index {array_index} out of bounds for {} slices",
                desc.ArraySize
            )));
        }
        let source_color = source_color_for_frame(frame);
        let output_mode = self.select_output_mode_for_source(source_color)?;
        let target_color = output_mode.target_color_for_source(source_color);
        let visible_width = texture_ref.width().max(1).min(desc.Width.max(1));
        let visible_height = texture_ref.height().max(1).min(desc.Height.max(1));

        let state = self.state.as_ref().expect("device ensured");
        let luma = create_plane_srv(state, &texture, array_index, texture_format.luma_srv())
            .map_err(|error| d3d11va_srv_error(error, &desc, array_index))?;
        let chroma = create_plane_srv(state, &texture, array_index, texture_format.chroma_srv())
            .map_err(|error| d3d11va_srv_error(error, &desc, array_index))?;
        self.stats.hardware_video_frames += 1;
        self.stats.zero_copy_video_frames += 1;
        match import_mode {
            D3d11VideoImportMode::DirectDecoderDevice => {
                self.stats.direct_zero_copy_video_frames += 1;
            }
            D3d11VideoImportMode::SharedHandle => {
                self.stats.shared_handle_video_frames += 1;
            }
        }
        self.current_video = Some(ImportedVideoFrame {
            _frame: retained_frame,
            _texture: texture,
            luma,
            chroma,
            width: visible_width,
            height: visible_height,
            tex_rect: D3d11TexRect::visible_region(
                visible_width,
                visible_height,
                desc.Width,
                desc.Height,
            ),
            _array_index: array_index,
            constants: constants_for_frame(source_color, texture_format, target_color),
        });
        Ok(())
    }

    fn import_texture_for_current_device(
        &mut self,
        texture: &ID3D11Texture2D,
    ) -> Result<(ID3D11Texture2D, D3d11VideoImportMode)> {
        if let Some(state) = self.state.as_ref() {
            let frame_device = unsafe { texture.GetDevice() }
                .map_err(|error| d3d_error("ID3D11Texture2D::GetDevice", error))?;
            if state.device.as_raw() == frame_device.as_raw() {
                return Ok((texture.clone(), D3d11VideoImportMode::DirectDecoderDevice));
            }
            return open_shared_texture_on_device(&state.device, texture)
                .map(|texture| (texture, D3d11VideoImportMode::SharedHandle));
        }

        self.ensure_device_for_texture(texture)?;
        Ok((texture.clone(), D3d11VideoImportMode::DirectDecoderDevice))
    }

    fn prepare_overlay_draws(
        &mut self,
        frame: Option<&OverlayFrame>,
    ) -> Result<Vec<D3d11OverlayDraw>> {
        let Some(frame) = frame else {
            return Ok(Vec::new());
        };
        if frame.is_empty() {
            return Ok(Vec::new());
        }
        self.ensure_default_device()?;
        self.stats.prepared_overlay_frames += 1;
        self.stats.prepared_overlay_subtitle_planes += frame
            .subtitle_planes
            .len()
            .saturating_add(frame.subtitle_alpha_planes.len())
            as u64;
        let viewport_w = frame.viewport.width;
        let viewport_h = frame.viewport.height;
        let mut draws = Vec::new();

        for plane in &frame.subtitle_planes {
            if plane.width == 0 || plane.height == 0 {
                continue;
            }
            let expected = plane.width as usize * plane.height as usize * 4;
            if plane.rgba.len() != expected {
                return Err(PlayerError::Renderer(format!(
                    "d3d11: overlay subtitle plane has {} bytes, expected {expected} for {}x{} RGBA",
                    plane.rgba.len(),
                    plane.width,
                    plane.height
                )));
            }
            let texture = {
                let state = self.state.as_ref().expect("device ensured");
                create_overlay_texture(
                    state,
                    plane.width,
                    plane.height,
                    DXGI_FORMAT_R8G8B8A8_UNORM,
                    &plane.rgba,
                    plane.width * 4,
                )?
            };
            let (x, y, width, height) = plane.scaled_rect(viewport_w, viewport_h);
            draws.push(D3d11OverlayDraw {
                texture,
                constants: OverlayUniforms::rgba_plane(
                    x,
                    y,
                    width,
                    height,
                    viewport_w,
                    viewport_h,
                ),
            });
        }

        self.append_alpha_atlas_draws(frame, viewport_w, viewport_h, &mut draws)?;
        Ok(draws)
    }

    fn prepare_danmaku_draws(
        &mut self,
        plan: Option<&DanmakuRenderPlan>,
    ) -> Result<Vec<D3d11OverlayDraw>> {
        let Some(plan) = plan else {
            return Ok(Vec::new());
        };
        if plan.is_empty() {
            return Ok(Vec::new());
        }
        self.ensure_default_device()?;
        let Some(atlas) = plan.atlas.as_ref() else {
            return Ok(Vec::new());
        };
        if !atlas.is_valid() {
            return Err(PlayerError::Renderer(format!(
                "d3d11: danmaku glyph atlas has fill={} outline={} bytes, expected at least {} for {}x{} stride {}",
                atlas.fill_alpha.len(),
                atlas.outline_alpha.len(),
                atlas.required_len(),
                atlas.width,
                atlas.height,
                atlas.stride
            )));
        }
        let viewport_w = plan.viewport.width;
        let viewport_h = plan.viewport.height;
        let mut draws = Vec::with_capacity(plan.items.len() * 3);
        let (fill, outline) = self.prepare_danmaku_atlas_textures(atlas)?;
        for item in &plan.items {
            self.append_danmaku_glyph_draws(
                item, &fill, &outline, viewport_w, viewport_h, &mut draws,
            );
        }
        Ok(draws)
    }

    fn prepare_danmaku_atlas_textures(
        &mut self,
        atlas: &DanmakuGlyphAtlas,
    ) -> Result<(D3d11OverlayTexture, D3d11OverlayTexture)> {
        if let Some(cache) = &self.danmaku_atlas_cache {
            if cache.can_reuse_for(atlas) {
                self.stats.overlay_alpha_atlas_reuses += 1;
                return Ok((cache.fill.clone(), cache.outline.clone()));
            }
        }

        let (fill, outline) = {
            let state = self.state.as_ref().expect("device ensured");
            (
                create_overlay_texture(
                    state,
                    atlas.width,
                    atlas.height,
                    DXGI_FORMAT_R8_UNORM,
                    &atlas.fill_alpha,
                    atlas.stride as u32,
                )?,
                create_overlay_texture(
                    state,
                    atlas.width,
                    atlas.height,
                    DXGI_FORMAT_R8_UNORM,
                    &atlas.outline_alpha,
                    atlas.stride as u32,
                )?,
            )
        };
        self.stats.overlay_alpha_atlas_uploads += 1;
        self.danmaku_atlas_cache = Some(D3d11DanmakuAtlasCache {
            version: atlas.version,
            width: atlas.width,
            height: atlas.height,
            stride: atlas.stride,
            fill: fill.clone(),
            outline: outline.clone(),
        });
        Ok((fill, outline))
    }

    fn append_danmaku_glyph_draws(
        &self,
        item: &DanmakuGlyphInstance,
        fill_texture: &D3d11OverlayTexture,
        outline_texture: &D3d11OverlayTexture,
        viewport_w: u32,
        viewport_h: u32,
        draws: &mut Vec<D3d11OverlayDraw>,
    ) {
        if item.shadow_rgba[3] > 0.0 {
            let mut rect = item.rect;
            rect[0] += item.shadow_offset[0];
            rect[1] += item.shadow_offset[1];
            draws.push(D3d11OverlayDraw {
                texture: outline_texture.clone(),
                constants: OverlayUniforms::alpha_atlas_rect(
                    item.shadow_rgba,
                    rect,
                    item.tex_rect,
                    viewport_w,
                    viewport_h,
                ),
            });
        }
        if item.outline_rgba[3] > 0.0 {
            draws.push(D3d11OverlayDraw {
                texture: outline_texture.clone(),
                constants: OverlayUniforms::alpha_atlas_rect(
                    item.outline_rgba,
                    item.rect,
                    item.tex_rect,
                    viewport_w,
                    viewport_h,
                ),
            });
        }
        draws.push(D3d11OverlayDraw {
            texture: fill_texture.clone(),
            constants: OverlayUniforms::alpha_atlas_rect(
                item.color_rgba,
                item.rect,
                item.tex_rect,
                viewport_w,
                viewport_h,
            ),
        });
    }

    fn append_alpha_atlas_draws(
        &mut self,
        frame: &OverlayFrame,
        viewport_w: u32,
        viewport_h: u32,
        draws: &mut Vec<D3d11OverlayDraw>,
    ) -> Result<()> {
        let bitmaps = &frame.subtitle_alpha_planes;
        let mut atlas_width = 0usize;
        let mut atlas_height = 0usize;
        for bitmap in bitmaps {
            if bitmap.placement.width == 0 || bitmap.placement.height == 0 {
                continue;
            }
            atlas_width += bitmap.placement.width as usize;
            atlas_height = atlas_height.max(bitmap.placement.height as usize);
        }
        if atlas_width == 0 || atlas_height == 0 {
            return Ok(());
        }

        let mut pixels = vec![0u8; atlas_width * atlas_height];
        let mut cursor_x = 0usize;
        let mut placements = Vec::new();
        for (index, bitmap) in bitmaps.iter().enumerate() {
            let bw = bitmap.placement.width as usize;
            let bh = bitmap.placement.height as usize;
            if bw == 0 || bh == 0 {
                continue;
            }
            if !bitmap.is_valid() {
                return Err(PlayerError::Renderer(format!(
                    "d3d11: overlay alpha bitmap has {} bytes, expected at least {} for {}x{} stride {}",
                    bitmap.alpha.len(),
                    bitmap.required_len(),
                    bitmap.placement.width,
                    bitmap.placement.height,
                    bitmap.stride
                )));
            }
            for row in 0..bh {
                let src = row * bitmap.stride;
                let dst = row * atlas_width + cursor_x;
                pixels[dst..dst + bw].copy_from_slice(&bitmap.alpha[src..src + bw]);
            }
            placements.push((index, cursor_x));
            cursor_x += bw;
        }

        let texture = {
            let state = self.state.as_ref().expect("device ensured");
            create_overlay_texture(
                state,
                atlas_width as u32,
                atlas_height as u32,
                DXGI_FORMAT_R8_UNORM,
                &pixels,
                atlas_width as u32,
            )?
        };
        self.stats.overlay_alpha_atlas_uploads += 1;
        for (index, atlas_x) in placements {
            let bitmap = &bitmaps[index];
            draws.push(D3d11OverlayDraw {
                texture: texture.clone(),
                constants: OverlayUniforms::alpha_atlas(
                    bitmap.color_rgba,
                    bitmap.placement.x,
                    bitmap.placement.y,
                    bitmap.placement.width,
                    bitmap.placement.height,
                    atlas_x as u32,
                    atlas_width as u32,
                    atlas_height as u32,
                    viewport_w,
                    viewport_h,
                ),
            });
        }
        Ok(())
    }

    fn render_video(&mut self, context: RenderFrameContext<'_>) -> Result<bool> {
        if self.current_video.is_none() {
            return Ok(false);
        }
        self.ensure_default_device()?;
        self.ensure_surface_ready()?;
        let overlay_draws = self.prepare_overlay_draws(context.overlay)?;
        let danmaku_draws = self.prepare_danmaku_draws(context.danmaku)?;
        let video = self.current_video.as_ref().expect("video checked");
        let state = self.state.as_ref().expect("device ensured");
        let surface = self.surface.as_ref().expect("surface ensured");
        let rtv = surface
            .render_target
            .as_ref()
            .ok_or_else(|| PlayerError::Renderer("d3d11: no render target attached".to_string()))?;
        let swapchain = surface
            .swapchain
            .as_ref()
            .ok_or_else(|| PlayerError::Renderer("d3d11: no swapchain attached".to_string()))?;
        let physical = surface.physical_size();
        let target_rect =
            aspect_fit_rect(video.width, video.height, physical.width, physical.height);
        unsafe {
            state
                .context
                .ClearRenderTargetView(rtv, &[0.0, 0.0, 0.0, 1.0]);
        }
        state.draw_video(video, rtv, target_rect)?;
        if !overlay_draws.is_empty() {
            state.draw_overlays(&overlay_draws, rtv, physical.width, physical.height)?;
        }
        if !danmaku_draws.is_empty() {
            state.draw_overlays(&danmaku_draws, rtv, physical.width, physical.height)?;
        }
        present_swapchain(swapchain, "IDXGISwapChain1::Present1")?;
        self.stats.rendered_frames += 1;
        if !danmaku_draws.is_empty() {
            self.stats.danmaku_passes += 1;
            self.stats.danmaku_items += danmaku_draws.len() as u64;
        }
        Ok(true)
    }

    fn ensure_surface_ready(&mut self) -> Result<()> {
        if self.surface.is_none() {
            return Err(PlayerError::Renderer(
                "d3d11: no HWND surface attached".to_string(),
            ));
        }
        if self
            .surface
            .as_ref()
            .is_some_and(|surface| surface.swapchain.is_none() || surface.render_target.is_none())
        {
            self.recreate_surface_targets()?;
        }
        Ok(())
    }

    fn render_clear(&mut self, time_seconds: f64) -> Result<()> {
        trace("render_clear: ensure_default_device");
        self.ensure_default_device()?;
        trace("render_clear: ensure_surface_ready");
        self.ensure_surface_ready()?;
        let state = self.state.as_ref().expect("device ensured");
        let surface = self.surface.as_ref().expect("surface ensured");
        let rtv = surface
            .render_target
            .as_ref()
            .ok_or_else(|| PlayerError::Renderer("d3d11: no render target attached".to_string()))?;
        let _ = time_seconds;
        let color = [0.0, 0.0, 0.0, 1.0];
        unsafe {
            trace("render_clear: clear");
            state.context.ClearRenderTargetView(rtv, &color);
            trace("render_clear: present");
            present_swapchain(
                surface.swapchain.as_ref().expect("swapchain ensured"),
                "IDXGISwapChain1::Present1(clear)",
            )?;
        }
        trace("render_clear: done");
        self.stats.rendered_frames += 1;
        Ok(())
    }
}

impl RendererBackend for D3d11Renderer {
    fn attach_surface(&mut self, surface: PlatformSurface) -> Result<()> {
        let PlatformSurface::Wgpu(handle) = surface else {
            return Err(PlayerError::Renderer(
                "d3d11: only Windows HWND surfaces are supported".to_string(),
            ));
        };
        if handle.kind != WgpuSurfaceKind::WindowsHwnd {
            return Err(PlayerError::Renderer(format!(
                "d3d11: surface kind {:?} is not supported",
                handle.kind
            )));
        }
        if handle.raw_window == 0 {
            return Err(PlayerError::Renderer(
                "d3d11: Windows HWND surface handle is null".to_string(),
            ));
        }
        self.surface = Some(AttachedSurface {
            hwnd: HWND(handle.raw_window as *mut c_void),
            width: handle.width.max(1),
            height: handle.height.max(1),
            scale: handle.scale,
            output_mode: D3d11OutputMode::Sdr,
            swapchain: None,
            render_target: None,
        });
        self.hdr10_output_unavailable = false;
        self.stats.attached = true;
        self.recreate_surface_targets()?;
        self.render_clear(0.0)
    }

    fn detach_surface(&mut self) -> Result<()> {
        self.surface = None;
        self.current_video = None;
        self.danmaku_atlas_cache = None;
        self.stats.attached = false;
        self.stats.surface_width = 0;
        self.stats.surface_height = 0;
        Ok(())
    }

    fn resize_surface(&mut self, width: u32, height: u32, scale: f64) -> Result<()> {
        let Some(surface) = self.surface.as_mut() else {
            return Err(PlayerError::Renderer(
                "d3d11: no HWND surface attached".to_string(),
            ));
        };
        surface.width = width.max(1);
        surface.height = height.max(1);
        surface.scale = scale;
        self.hdr10_output_unavailable = false;
        self.recreate_surface_targets()
    }

    fn render_test_frame(&mut self, time_seconds: f64) -> Result<()> {
        self.render_clear(time_seconds)
    }

    fn upload_player_frame(&mut self, frame: &PlayerVideoFrame) -> Result<()> {
        if frame.frame.d3d11va_texture().is_some() {
            return self.import_d3d11va_frame(frame);
        }
        if frame.frame.has_hw_frames_context() {
            self.stats.hardware_video_frames += 1;
            return Err(PlayerError::Renderer(
                "d3d11: hardware frame is not importable as D3D11VA".to_string(),
            ));
        }
        self.stats.cpu_video_frame_fallbacks += 1;
        Err(PlayerError::Renderer(
            "d3d11: software frames require WgpuFallback or a CPU upload path".to_string(),
        ))
    }

    fn clear_current_frame(&mut self) -> Result<()> {
        self.current_video = None;
        self.danmaku_atlas_cache = None;
        if self.surface.is_some() {
            self.render_clear(0.0)?;
        }
        Ok(())
    }

    fn render_current_frame(&mut self, context: RenderFrameContext<'_>) -> Result<bool> {
        self.render_video(context)
    }

    fn runtime_stats(&self) -> RendererRuntimeStats {
        RendererRuntimeStats {
            surface_width: self.stats.surface_width,
            surface_height: self.stats.surface_height,
            rendered_frames: self.stats.rendered_frames,
            attached: self.stats.attached,
            prepared_overlay_frames: self.stats.prepared_overlay_frames,
            prepared_overlay_subtitle_planes: self.stats.prepared_overlay_subtitle_planes,
            danmaku_passes: self.stats.danmaku_passes,
            danmaku_draw_items: self.stats.danmaku_items,
            overlay_alpha_atlas_uploads: self.stats.overlay_alpha_atlas_uploads,
            overlay_alpha_atlas_reuses: self.stats.overlay_alpha_atlas_reuses,
            software_video_frames: 0,
            hardware_video_frames: self.stats.hardware_video_frames,
            zero_copy_video_frames: self.stats.zero_copy_video_frames,
            direct_zero_copy_video_frames: self.stats.direct_zero_copy_video_frames,
            shared_handle_video_frames: self.stats.shared_handle_video_frames,
            cpu_video_frame_fallbacks: self.stats.cpu_video_frame_fallbacks,
            hdr_source_frames: self.stats.hdr_source_frames,
            hdr10_output_frames: self.stats.hdr10_output_frames,
            sdr_tonemap_frames: self.stats.sdr_tonemap_frames,
            hdr10_metadata_updates: self.stats.hdr10_metadata_updates,
            hdr10_metadata_failures: self.stats.hdr10_metadata_failures,
            hdr10_output_failures: self.stats.hdr10_output_failures,
            hdr10_output_active: self.stats.hdr10_output_active,
            upscaler_mode: self.upscaler_mode,
            upscaler_backend: if self.upscaler_mode.is_enabled() {
                LumaUpscalerBackendStatus::Inactive
            } else {
                LumaUpscalerBackendStatus::Off
            },
            ..Default::default()
        }
    }

    fn set_luma_upscaler(&mut self, mode: LumaUpscalerMode) {
        self.upscaler_mode = mode;
    }
}

impl D3d11DeviceState {
    fn new(device: ID3D11Device, context: ID3D11DeviceContext) -> Result<Self> {
        let vertex_blob = compile_shader("vs_main", "vs_4_0")?;
        let pixel_blob = compile_shader("ps_main", "ps_4_0")?;
        let overlay_vertex_blob =
            compile_shader_source(OVERLAY_SHADER_SOURCE, "overlay_vs_main", "vs_4_0")?;
        let overlay_pixel_blob =
            compile_shader_source(OVERLAY_SHADER_SOURCE, "overlay_ps_main", "ps_4_0")?;
        let vertex_shader = {
            let mut shader = None;
            unsafe {
                device
                    .CreateVertexShader(blob_bytes(&vertex_blob), None, Some(&mut shader))
                    .map_err(|error| d3d_error("ID3D11Device::CreateVertexShader", error))?;
            }
            shader.ok_or_else(|| {
                PlayerError::Renderer("d3d11: CreateVertexShader returned null".to_string())
            })?
        };
        let pixel_shader = {
            let mut shader = None;
            unsafe {
                device
                    .CreatePixelShader(blob_bytes(&pixel_blob), None, Some(&mut shader))
                    .map_err(|error| d3d_error("ID3D11Device::CreatePixelShader", error))?;
            }
            shader.ok_or_else(|| {
                PlayerError::Renderer("d3d11: CreatePixelShader returned null".to_string())
            })?
        };
        let overlay_vertex_shader = {
            let mut shader = None;
            unsafe {
                device
                    .CreateVertexShader(blob_bytes(&overlay_vertex_blob), None, Some(&mut shader))
                    .map_err(|error| {
                        d3d_error("ID3D11Device::CreateVertexShader(overlay)", error)
                    })?;
            }
            shader.ok_or_else(|| {
                PlayerError::Renderer(
                    "d3d11: CreateVertexShader(overlay) returned null".to_string(),
                )
            })?
        };
        let overlay_pixel_shader = {
            let mut shader = None;
            unsafe {
                device
                    .CreatePixelShader(blob_bytes(&overlay_pixel_blob), None, Some(&mut shader))
                    .map_err(|error| {
                        d3d_error("ID3D11Device::CreatePixelShader(overlay)", error)
                    })?;
            }
            shader.ok_or_else(|| {
                PlayerError::Renderer("d3d11: CreatePixelShader(overlay) returned null".to_string())
            })?
        };
        let input_layout = create_input_layout(&device, &vertex_blob)?;
        let vertex_buffer = create_vertex_buffer(&device)?;
        let constants = create_constants_buffer(&device)?;
        let sampler = create_sampler(&device)?;
        let overlay_constants = create_overlay_constants_buffer(&device)?;
        let overlay_sampler = create_sampler(&device)?;
        let overlay_blend = create_overlay_blend_state(&device)?;
        Ok(Self {
            device,
            context,
            vertex_shader,
            pixel_shader,
            input_layout,
            vertex_buffer,
            constants,
            sampler,
            overlay_vertex_shader,
            overlay_pixel_shader,
            overlay_constants,
            overlay_sampler,
            overlay_blend,
        })
    }

    fn draw_video(
        &self,
        video: &ImportedVideoFrame,
        render_target: &ID3D11RenderTargetView,
        target: D3d11DrawRect,
    ) -> Result<()> {
        let viewport = D3D11_VIEWPORT {
            TopLeftX: target.x,
            TopLeftY: target.y,
            Width: target.width.max(1.0),
            Height: target.height.max(1.0),
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        let stride = mem::size_of::<VideoVertex>() as u32;
        let offset = 0u32;
        let vertices = video_vertices(video.tex_rect);
        unsafe {
            self.context.UpdateSubresource(
                &self.vertex_buffer,
                0,
                None,
                vertices.as_ptr() as *const c_void,
                0,
                0,
            );
            self.context.UpdateSubresource(
                &self.constants,
                0,
                None,
                &video.constants as *const _ as *const c_void,
                0,
                0,
            );
            self.context.RSSetViewports(Some(&[viewport]));
            self.context
                .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.context.IASetInputLayout(&self.input_layout);
            self.context.IASetVertexBuffers(
                0,
                1,
                Some(&Some(self.vertex_buffer.clone())),
                Some(&stride),
                Some(&offset),
            );
            self.context.VSSetShader(&self.vertex_shader, None);
            self.context.PSSetShader(&self.pixel_shader, None);
            self.context
                .PSSetConstantBuffers(0, Some(&[Some(self.constants.clone())]));
            self.context.PSSetShaderResources(
                0,
                Some(&[Some(video.luma.clone()), Some(video.chroma.clone())]),
            );
            self.context
                .PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            self.context
                .OMSetRenderTargets(Some(&[Some(render_target.clone())]), None);
            self.context.OMSetBlendState(None, None, u32::MAX);
            self.context.Draw(6, 0);
            self.context.PSSetShaderResources(0, Some(&[None, None]));
        }
        Ok(())
    }

    fn draw_overlays(
        &self,
        draws: &[D3d11OverlayDraw],
        render_target: &ID3D11RenderTargetView,
        width: u32,
        height: u32,
    ) -> Result<()> {
        if draws.is_empty() {
            return Ok(());
        }
        let viewport = D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: width.max(1) as f32,
            Height: height.max(1) as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        let stride = mem::size_of::<VideoVertex>() as u32;
        let offset = 0u32;
        let blend_factor = [0.0f32; 4];
        let vertices = video_vertices(D3d11TexRect::FULL);
        unsafe {
            self.context.UpdateSubresource(
                &self.vertex_buffer,
                0,
                None,
                vertices.as_ptr() as *const c_void,
                0,
                0,
            );
            self.context.RSSetViewports(Some(&[viewport]));
            self.context
                .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.context.IASetInputLayout(&self.input_layout);
            self.context.IASetVertexBuffers(
                0,
                1,
                Some(&Some(self.vertex_buffer.clone())),
                Some(&stride),
                Some(&offset),
            );
            self.context.VSSetShader(&self.overlay_vertex_shader, None);
            self.context.PSSetShader(&self.overlay_pixel_shader, None);
            self.context
                .VSSetConstantBuffers(0, Some(&[Some(self.overlay_constants.clone())]));
            self.context
                .PSSetConstantBuffers(0, Some(&[Some(self.overlay_constants.clone())]));
            self.context
                .PSSetSamplers(0, Some(&[Some(self.overlay_sampler.clone())]));
            self.context
                .OMSetRenderTargets(Some(&[Some(render_target.clone())]), None);
            self.context
                .OMSetBlendState(&self.overlay_blend, Some(&blend_factor), u32::MAX);
            for draw in draws {
                self.context.UpdateSubresource(
                    &self.overlay_constants,
                    0,
                    None,
                    &draw.constants as *const _ as *const c_void,
                    0,
                    0,
                );
                self.context
                    .PSSetShaderResources(0, Some(&[Some(draw.texture.view.clone())]));
                self.context.Draw(6, 0);
            }
            self.context.PSSetShaderResources(0, Some(&[None]));
            self.context.OMSetBlendState(None, None, u32::MAX);
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum D3d11VideoTextureFormat {
    Nv12,
    P010,
}

impl D3d11VideoTextureFormat {
    fn from_dxgi(format: DXGI_FORMAT) -> Option<Self> {
        if format == DXGI_FORMAT_NV12 {
            Some(Self::Nv12)
        } else if format == DXGI_FORMAT_P010 {
            Some(Self::P010)
        } else {
            None
        }
    }

    fn luma_srv(self) -> DXGI_FORMAT {
        match self {
            Self::Nv12 => DXGI_FORMAT_R8_UNORM,
            Self::P010 => DXGI_FORMAT_R16_UNORM,
        }
    }

    fn chroma_srv(self) -> DXGI_FORMAT {
        match self {
            Self::Nv12 => DXGI_FORMAT_R8G8_UNORM,
            Self::P010 => DXGI_FORMAT_R16G16_UNORM,
        }
    }
}

fn aspect_fit_rect(
    source_width: u32,
    source_height: u32,
    target_width: u32,
    target_height: u32,
) -> D3d11DrawRect {
    if source_width == 0 || source_height == 0 || target_width == 0 || target_height == 0 {
        return D3d11DrawRect::full(target_width, target_height);
    }
    let target_w = target_width as f32;
    let target_h = target_height as f32;
    let source_aspect = source_width as f32 / source_height as f32;
    let target_aspect = target_w / target_h;
    if source_aspect > target_aspect {
        let height = target_w / source_aspect;
        D3d11DrawRect {
            x: 0.0,
            y: (target_h - height) * 0.5,
            width: target_w,
            height,
        }
    } else {
        let width = target_h * source_aspect;
        D3d11DrawRect {
            x: (target_w - width) * 0.5,
            y: 0.0,
            width,
            height: target_h,
        }
    }
}

fn create_default_device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    trace("create_default_device: D3D11CreateDevice");
    let feature_levels = [
        D3D_FEATURE_LEVEL_11_1,
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_1,
        D3D_FEATURE_LEVEL_10_0,
    ];
    let mut device = None;
    let mut context = None;
    let mut selected = D3D_FEATURE_LEVEL(0);
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&feature_levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut selected),
            Some(&mut context),
        )
        .map_err(|error| d3d_error("D3D11CreateDevice", error))?;
    }
    let device =
        device.ok_or_else(|| PlayerError::Renderer("d3d11: device was null".to_string()))?;
    let context =
        context.ok_or_else(|| PlayerError::Renderer("d3d11: context was null".to_string()))?;
    trace("create_default_device: done");
    Ok((device, context))
}

fn create_swapchain(
    device: &ID3D11Device,
    hwnd: HWND,
    width: u32,
    height: u32,
    scale: f64,
    format: DXGI_FORMAT,
) -> Result<IDXGISwapChain1> {
    trace("create_swapchain: cast IDXGIDevice");
    let dxgi_device: IDXGIDevice = device
        .cast()
        .map_err(|error| d3d_error("ID3D11Device::cast<IDXGIDevice>", error))?;
    trace("create_swapchain: get adapter");
    let adapter: IDXGIAdapter = unsafe { dxgi_device.GetAdapter() }
        .map_err(|error| d3d_error("IDXGIDevice::GetAdapter", error))?;
    trace("create_swapchain: get factory");
    let factory: IDXGIFactory2 = unsafe { adapter.GetParent() }
        .map_err(|error| d3d_error("IDXGIAdapter::GetParent<IDXGIFactory2>", error))?;
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: scaled(width, scale),
        Height: scaled(height, scale),
        Format: format,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
        Flags: 0,
    };
    trace("create_swapchain: CreateSwapChainForHwnd");
    let swapchain = unsafe { factory.CreateSwapChainForHwnd(device, hwnd, &desc, None, None) }
        .map_err(|error| d3d_error("IDXGIFactory2::CreateSwapChainForHwnd", error))?;
    trace("create_swapchain: done");
    Ok(swapchain)
}

fn configure_swapchain_color_space(
    swapchain: &IDXGISwapChain1,
    output_mode: D3d11OutputMode,
) -> Result<()> {
    let swapchain3: IDXGISwapChain3 = swapchain
        .cast()
        .map_err(|error| d3d_error("IDXGISwapChain1::cast<IDXGISwapChain3>", error))?;
    let color_space = output_mode.color_space();
    if matches!(output_mode, D3d11OutputMode::Hdr10) {
        let support = unsafe { swapchain3.CheckColorSpaceSupport(color_space) }
            .map_err(|error| d3d_error("IDXGISwapChain3::CheckColorSpaceSupport(HDR10)", error))?;
        if support & DXGI_SWAP_CHAIN_COLOR_SPACE_SUPPORT_FLAG_PRESENT.0 as u32 == 0 {
            return Err(PlayerError::Renderer(
                "d3d11: HDR10 swapchain color space is not presentable".to_string(),
            ));
        }
    }
    unsafe { swapchain3.SetColorSpace1(color_space) }
        .map_err(|error| d3d_error("IDXGISwapChain3::SetColorSpace1", error))
}

fn set_hdr10_metadata(
    swapchain: &IDXGISwapChain1,
    metadata: DXGI_HDR_METADATA_HDR10,
) -> Result<()> {
    let swapchain4: IDXGISwapChain4 = swapchain
        .cast()
        .map_err(|error| d3d_error("IDXGISwapChain1::cast<IDXGISwapChain4>", error))?;
    let bytes = unsafe {
        std::slice::from_raw_parts(
            (&metadata as *const DXGI_HDR_METADATA_HDR10).cast::<u8>(),
            mem::size_of::<DXGI_HDR_METADATA_HDR10>(),
        )
    };
    unsafe { swapchain4.SetHDRMetaData(DXGI_HDR_METADATA_TYPE_HDR10, Some(bytes)) }
        .map_err(|error| d3d_error("IDXGISwapChain4::SetHDRMetaData(HDR10)", error))
}

fn clear_hdr_metadata(swapchain: &IDXGISwapChain1) -> Result<()> {
    let swapchain4: IDXGISwapChain4 = swapchain
        .cast()
        .map_err(|error| d3d_error("IDXGISwapChain1::cast<IDXGISwapChain4>", error))?;
    unsafe { swapchain4.SetHDRMetaData(DXGI_HDR_METADATA_TYPE_NONE, None) }
        .map_err(|error| d3d_error("IDXGISwapChain4::SetHDRMetaData(None)", error))
}

fn create_render_target(
    device: &ID3D11Device,
    swapchain: &IDXGISwapChain1,
) -> Result<ID3D11RenderTargetView> {
    trace("create_render_target: GetBuffer");
    let back_buffer: ID3D11Texture2D = unsafe { swapchain.GetBuffer(0) }
        .map_err(|error| d3d_error("IDXGISwapChain1::GetBuffer", error))?;
    trace("create_render_target: cast resource");
    let resource: ID3D11Resource = back_buffer
        .cast()
        .map_err(|error| d3d_error("ID3D11Texture2D::cast<ID3D11Resource>", error))?;
    let mut view = None;
    unsafe {
        trace("create_render_target: CreateRenderTargetView");
        device
            .CreateRenderTargetView(&resource, None, Some(&mut view))
            .map_err(|error| d3d_error("ID3D11Device::CreateRenderTargetView", error))?;
    }
    let view =
        view.ok_or_else(|| PlayerError::Renderer("d3d11: render target was null".to_string()))?;
    trace("create_render_target: done");
    Ok(view)
}

fn present_swapchain(swapchain: &IDXGISwapChain1, operation: &'static str) -> Result<()> {
    let params = DXGI_PRESENT_PARAMETERS {
        DirtyRectsCount: 0,
        pDirtyRects: ptr::null_mut(),
        pScrollRect: ptr::null_mut(),
        pScrollOffset: ptr::null_mut(),
    };
    unsafe { swapchain.Present1(1, DXGI_PRESENT(0), &params) }
        .ok()
        .map_err(|error| d3d_error(operation, error))
}

fn create_plane_srv(
    state: &D3d11DeviceState,
    texture: &ID3D11Texture2D,
    array_index: u32,
    format: DXGI_FORMAT,
) -> Result<ID3D11ShaderResourceView> {
    let resource: ID3D11Resource = texture
        .cast()
        .map_err(|error| d3d_error("ID3D11Texture2D::cast<ID3D11Resource>", error))?;
    let desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: format,
        ViewDimension: D3D_SRV_DIMENSION_TEXTURE2DARRAY,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2DArray: D3D11_TEX2D_ARRAY_SRV {
                MostDetailedMip: 0,
                MipLevels: 1,
                FirstArraySlice: array_index,
                ArraySize: 1,
            },
        },
    };
    let mut view = None;
    unsafe {
        state
            .device
            .CreateShaderResourceView(&resource, Some(&desc), Some(&mut view))
            .map_err(|error| {
                d3d_error(
                    "ID3D11Device::CreateShaderResourceView(D3D11VA plane)",
                    error,
                )
            })?;
    }
    view.ok_or_else(|| PlayerError::Renderer("d3d11: shader resource view was null".to_string()))
}

fn open_shared_texture_on_device(
    device: &ID3D11Device,
    texture: &ID3D11Texture2D,
) -> Result<ID3D11Texture2D> {
    let shared_handle = d3d11_shared_handle(texture)?;
    let mut imported = None;
    unsafe {
        device
            .OpenSharedResource::<ID3D11Texture2D>(shared_handle, &mut imported)
            .map_err(|error| d3d_error("ID3D11Device::OpenSharedResource(D3D11VA)", error))?;
    }
    imported
        .ok_or_else(|| PlayerError::Renderer("d3d11: shared texture import was null".to_string()))
}

fn d3d11_shared_handle(texture: &ID3D11Texture2D) -> Result<HANDLE> {
    let resource: IDXGIResource = texture
        .cast()
        .map_err(|error| d3d_error("ID3D11Texture2D::cast<IDXGIResource>", error))?;
    let handle = unsafe { resource.GetSharedHandle() }
        .map_err(|error| d3d_error("IDXGIResource::GetSharedHandle(D3D11VA)", error))?;
    if handle.is_invalid() {
        return Err(PlayerError::Renderer(
            "d3d11: D3D11VA texture did not expose a valid shared handle".to_string(),
        ));
    }
    Ok(handle)
}

fn create_overlay_texture(
    state: &D3d11DeviceState,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    data: &[u8],
    bytes_per_row: u32,
) -> Result<D3d11OverlayTexture> {
    if width == 0 || height == 0 || bytes_per_row == 0 {
        return Err(PlayerError::Renderer(
            "d3d11: overlay texture dimensions must be non-zero".to_string(),
        ));
    }
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let subresource = D3D11_SUBRESOURCE_DATA {
        pSysMem: data.as_ptr() as *const c_void,
        SysMemPitch: bytes_per_row,
        SysMemSlicePitch: bytes_per_row.saturating_mul(height),
    };
    let mut texture = None;
    unsafe {
        state
            .device
            .CreateTexture2D(&desc, Some(&subresource), Some(&mut texture))
            .map_err(|error| d3d_error("ID3D11Device::CreateTexture2D(overlay)", error))?;
    }
    let texture = texture
        .ok_or_else(|| PlayerError::Renderer("d3d11: overlay texture was null".to_string()))?;
    let view = create_texture2d_srv(state, &texture, format)?;
    Ok(D3d11OverlayTexture {
        _texture: texture,
        view,
    })
}

fn create_texture2d_srv(
    state: &D3d11DeviceState,
    texture: &ID3D11Texture2D,
    format: DXGI_FORMAT,
) -> Result<ID3D11ShaderResourceView> {
    let resource: ID3D11Resource = texture
        .cast()
        .map_err(|error| d3d_error("ID3D11Texture2D::cast<ID3D11Resource>", error))?;
    let desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: format,
        ViewDimension: ::windows::Win32::Graphics::Direct3D::D3D_SRV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_SRV {
                MostDetailedMip: 0,
                MipLevels: 1,
            },
        },
    };
    let mut view = None;
    unsafe {
        state
            .device
            .CreateShaderResourceView(&resource, Some(&desc), Some(&mut view))
            .map_err(|error| d3d_error("ID3D11Device::CreateShaderResourceView(overlay)", error))?;
    }
    view.ok_or_else(|| PlayerError::Renderer("d3d11: overlay SRV was null".to_string()))
}

fn create_input_layout(device: &ID3D11Device, vertex_blob: &ID3DBlob) -> Result<ID3D11InputLayout> {
    const POSITION: &[u8] = b"POSITION\0";
    const TEXCOORD: &[u8] = b"TEXCOORD\0";
    let elements = [
        D3D11_INPUT_ELEMENT_DESC {
            SemanticName: PCSTR(POSITION.as_ptr()),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 0,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
        D3D11_INPUT_ELEMENT_DESC {
            SemanticName: PCSTR(TEXCOORD.as_ptr()),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 8,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
    ];
    let mut layout = None;
    unsafe {
        device
            .CreateInputLayout(&elements, blob_bytes(vertex_blob), Some(&mut layout))
            .map_err(|error| d3d_error("ID3D11Device::CreateInputLayout", error))?;
    }
    layout.ok_or_else(|| PlayerError::Renderer("d3d11: input layout was null".to_string()))
}

fn create_vertex_buffer(device: &ID3D11Device) -> Result<ID3D11Buffer> {
    let vertices = video_vertices(D3d11TexRect::FULL);
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: mem::size_of_val(&vertices) as u32,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: ::windows::Win32::Graphics::Direct3D11::D3D11_BIND_VERTEX_BUFFER.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let data = D3D11_SUBRESOURCE_DATA {
        pSysMem: vertices.as_ptr() as *const c_void,
        SysMemPitch: 0,
        SysMemSlicePitch: 0,
    };
    create_buffer(
        device,
        &desc,
        Some(&data),
        "ID3D11Device::CreateBuffer(vertex)",
    )
}

fn video_vertices(tex_rect: D3d11TexRect) -> [VideoVertex; 6] {
    let left = tex_rect.x;
    let right = tex_rect.right();
    let top = tex_rect.y;
    let bottom = tex_rect.bottom();
    [
        VideoVertex {
            position: [-1.0, -1.0],
            texcoord: [left, bottom],
        },
        VideoVertex {
            position: [-1.0, 1.0],
            texcoord: [left, top],
        },
        VideoVertex {
            position: [1.0, -1.0],
            texcoord: [right, bottom],
        },
        VideoVertex {
            position: [1.0, -1.0],
            texcoord: [right, bottom],
        },
        VideoVertex {
            position: [-1.0, 1.0],
            texcoord: [left, top],
        },
        VideoVertex {
            position: [1.0, 1.0],
            texcoord: [right, top],
        },
    ]
}

fn create_constants_buffer(device: &ID3D11Device) -> Result<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: mem::size_of::<VideoUniforms>() as u32,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    create_buffer(device, &desc, None, "ID3D11Device::CreateBuffer(constants)")
}

fn create_overlay_constants_buffer(device: &ID3D11Device) -> Result<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: mem::size_of::<OverlayUniforms>() as u32,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    create_buffer(
        device,
        &desc,
        None,
        "ID3D11Device::CreateBuffer(overlay constants)",
    )
}

fn create_overlay_blend_state(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    let target = D3D11_RENDER_TARGET_BLEND_DESC {
        BlendEnable: true.into(),
        SrcBlend: D3D11_BLEND_SRC_ALPHA,
        DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
        BlendOp: D3D11_BLEND_OP_ADD,
        SrcBlendAlpha: D3D11_BLEND_ONE,
        DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
        BlendOpAlpha: D3D11_BLEND_OP_ADD,
        RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
    };
    let mut render_targets = [D3D11_RENDER_TARGET_BLEND_DESC::default(); 8];
    render_targets[0] = target;
    let desc = D3D11_BLEND_DESC {
        AlphaToCoverageEnable: false.into(),
        IndependentBlendEnable: false.into(),
        RenderTarget: render_targets,
    };
    let mut state = None;
    unsafe {
        device
            .CreateBlendState(&desc, Some(&mut state))
            .map_err(|error| d3d_error("ID3D11Device::CreateBlendState(overlay)", error))?;
    }
    state.ok_or_else(|| PlayerError::Renderer("d3d11: overlay blend state was null".to_string()))
}

fn create_buffer(
    device: &ID3D11Device,
    desc: &D3D11_BUFFER_DESC,
    data: Option<&D3D11_SUBRESOURCE_DATA>,
    operation: &'static str,
) -> Result<ID3D11Buffer> {
    let mut buffer = None;
    unsafe {
        device
            .CreateBuffer(desc, data.map(|data| data as *const _), Some(&mut buffer))
            .map_err(|error| d3d_error(operation, error))?;
    }
    buffer.ok_or_else(|| PlayerError::Renderer(format!("d3d11: {operation} returned null")))
}

fn create_sampler(device: &ID3D11Device) -> Result<ID3D11SamplerState> {
    let desc = D3D11_SAMPLER_DESC {
        Filter: ::windows::Win32::Graphics::Direct3D11::D3D11_FILTER_MIN_MAG_MIP_LINEAR,
        AddressU: ::windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressV: ::windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressW: ::windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE_ADDRESS_CLAMP,
        MipLODBias: 0.0,
        MaxAnisotropy: 1,
        ComparisonFunc: ::windows::Win32::Graphics::Direct3D11::D3D11_COMPARISON_NEVER,
        BorderColor: [0.0; 4],
        MinLOD: 0.0,
        MaxLOD: f32::MAX,
    };
    let mut sampler = None;
    unsafe {
        device
            .CreateSamplerState(&desc, Some(&mut sampler))
            .map_err(|error| d3d_error("ID3D11Device::CreateSamplerState", error))?;
    }
    sampler.ok_or_else(|| PlayerError::Renderer("d3d11: sampler was null".to_string()))
}

fn compile_shader(entry: &'static str, target: &'static str) -> Result<ID3DBlob> {
    compile_shader_source(SHADER_SOURCE, entry, target)
}

fn compile_shader_source(
    source: &'static [u8],
    entry: &'static str,
    target: &'static str,
) -> Result<ID3DBlob> {
    let mut code = None;
    let mut errors = None;
    let entry = nul(entry);
    let target = nul(target);
    unsafe {
        let result = D3DCompile(
            source.as_ptr() as *const c_void,
            source.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR(entry.as_ptr()),
            PCSTR(target.as_ptr()),
            0,
            0,
            &mut code,
            Some(&mut errors),
        );
        if let Err(error) = result {
            let message = errors
                .as_ref()
                .map(blob_to_string)
                .unwrap_or_else(|| error.message());
            return Err(PlayerError::Renderer(format!(
                "d3d11: D3DCompile({}/{}) failed: {message}",
                String::from_utf8_lossy(&entry[..entry.len() - 1]),
                String::from_utf8_lossy(&target[..target.len() - 1])
            )));
        }
    }
    code.ok_or_else(|| PlayerError::Renderer("d3d11: D3DCompile returned null".to_string()))
}

fn clone_d3d11_texture(raw: *mut c_void) -> Result<ID3D11Texture2D> {
    if raw.is_null() {
        return Err(PlayerError::Renderer(
            "d3d11: D3D11VA texture pointer is null".to_string(),
        ));
    }
    let borrowed = unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.ok_or_else(|| {
        PlayerError::Renderer("d3d11: failed to borrow D3D11VA texture".to_string())
    })?;
    Ok(borrowed.clone())
}

fn source_color_for_frame(frame: &PlayerVideoFrame) -> SourceColorState {
    SourceColorState::new(
        frame.frame.color_primaries(),
        frame.frame.transfer_function(),
    )
    .range(frame.frame.color_range())
    .matrix(frame.frame.matrix_coefficients())
    .hdr_metadata(frame.frame.hdr_metadata())
}

fn constants_for_frame(
    source: SourceColorState,
    texture_format: D3d11VideoTextureFormat,
    target: TargetColorState,
) -> VideoUniforms {
    let pipeline = VideoRenderPipeline::new(source, target);
    VideoUniforms::from_pipeline(
        &pipeline,
        matches!(texture_format, D3d11VideoTextureFormat::P010),
        false,
    )
}

fn dxgi_hdr10_metadata(source: SourceColorState) -> DXGI_HDR_METADATA_HDR10 {
    let mastering = source
        .hdr_metadata
        .and_then(|metadata| metadata.mastering_display);
    let content_light = source
        .hdr_metadata
        .and_then(|metadata| metadata.content_light);
    let default_primaries = primaries_coordinates(ColorPrimaries::Bt2020);
    let primaries = mastering
        .and_then(|metadata| metadata.display_primaries)
        .map(|primaries| PrimariesCoordinates {
            red: primaries[0],
            green: primaries[1],
            blue: primaries[2],
            white: mastering
                .and_then(|metadata| metadata.white_point)
                .unwrap_or(default_primaries.white),
        })
        .unwrap_or(default_primaries);
    let max_mastering = mastering
        .and_then(|metadata| metadata.max_luminance_nits)
        .unwrap_or(source.nominal_peak_nits.max(1000.0));
    let min_mastering = mastering
        .and_then(|metadata| metadata.min_luminance_nits)
        .unwrap_or(0.005);
    let max_cll = content_light
        .map(|metadata| metadata.max_content_light_level_nits)
        .unwrap_or_else(|| max_mastering.round().max(1.0) as u32);
    let max_fall = content_light
        .map(|metadata| metadata.max_frame_average_light_level_nits)
        .unwrap_or_else(|| (max_mastering * 0.4).round().max(1.0) as u32);

    DXGI_HDR_METADATA_HDR10 {
        RedPrimary: dxgi_chromaticity(primaries.red),
        GreenPrimary: dxgi_chromaticity(primaries.green),
        BluePrimary: dxgi_chromaticity(primaries.blue),
        WhitePoint: dxgi_chromaticity(primaries.white),
        MaxMasteringLuminance: nits_u32(max_mastering),
        MinMasteringLuminance: min_mastering_luminance(min_mastering),
        MaxContentLightLevel: clamp_u16(max_cll),
        MaxFrameAverageLightLevel: clamp_u16(max_fall),
    }
}

fn dxgi_chromaticity(value: Chromaticity) -> [u16; 2] {
    [chromaticity_coord(value.x), chromaticity_coord(value.y)]
}

fn chromaticity_coord(value: f32) -> u16 {
    if value.is_finite() {
        (value.clamp(0.0, 1.0) * 50_000.0)
            .round()
            .clamp(0.0, u16::MAX as f32) as u16
    } else {
        0
    }
}

fn nits_u32(value: f32) -> u32 {
    if value.is_finite() {
        value.round().clamp(1.0, u32::MAX as f32) as u32
    } else {
        1000
    }
}

fn min_mastering_luminance(value: f32) -> u32 {
    if value.is_finite() {
        (value.max(0.0) * 10_000.0)
            .round()
            .clamp(0.0, u32::MAX as f32) as u32
    } else {
        50
    }
}

fn clamp_u16(value: u32) -> u16 {
    value.min(u16::MAX as u32) as u16
}

fn scaled(value: u32, scale: f64) -> u32 {
    let scale = if scale.is_finite() {
        scale.max(1.0)
    } else {
        1.0
    };
    ((value.max(1) as f64) * scale).round().min(u32::MAX as f64) as u32
}

fn physical_size(width: u32, height: u32, scale: f64) -> D3d11PhysicalSize {
    D3d11PhysicalSize {
        width: scaled(width, scale),
        height: scaled(height, scale),
    }
}

fn nul(value: &str) -> Vec<u8> {
    let mut bytes = value.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

fn blob_bytes(blob: &ID3DBlob) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())
    }
}

fn blob_to_string(blob: &ID3DBlob) -> String {
    String::from_utf8_lossy(blob_bytes(blob)).into_owned()
}

fn trace(message: &str) {
    if std::env::var_os("ERIKA_D3D11_TRACE").is_some() {
        eprintln!("erika d3d11: {message}");
    }
}

fn d3d11va_srv_error(
    error: PlayerError,
    desc: &D3D11_TEXTURE2D_DESC,
    array_index: u32,
) -> PlayerError {
    PlayerError::Renderer(format!(
        "{error}; texture desc format={:?} bind_flags=0x{:x} misc_flags=0x{:x} usage={:?} array_size={} mip_levels={} sample_count={} array_index={}",
        desc.Format,
        desc.BindFlags,
        desc.MiscFlags,
        desc.Usage,
        desc.ArraySize,
        desc.MipLevels,
        desc.SampleDesc.Count,
        array_index
    ))
}

fn d3d_error(operation: &'static str, error: ::windows::core::Error) -> PlayerError {
    PlayerError::Renderer(format!("d3d11: {operation} failed: {}", error.message()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.0001,
            "expected {actual} to be close to {expected}"
        );
    }

    #[test]
    fn video_texture_format_maps_plane_srv_formats() {
        assert_eq!(
            D3d11VideoTextureFormat::from_dxgi(DXGI_FORMAT_NV12)
                .unwrap()
                .luma_srv(),
            DXGI_FORMAT_R8_UNORM
        );
        assert_eq!(
            D3d11VideoTextureFormat::from_dxgi(DXGI_FORMAT_P010)
                .unwrap()
                .chroma_srv(),
            DXGI_FORMAT_R16G16_UNORM
        );
    }

    #[test]
    fn d3d11_tex_rect_crops_padded_decoder_texture() {
        let rect = D3d11TexRect::visible_region(1920, 1080, 2048, 1088);

        assert_close(rect.width, 1920.0 / 2048.0);
        assert_close(rect.height, 1080.0 / 1088.0);
        let vertices = video_vertices(rect);
        assert_close(vertices[0].texcoord[1], 1080.0 / 1088.0);
        assert_close(vertices[2].texcoord[0], 1920.0 / 2048.0);
    }

    #[test]
    fn d3d11_physical_size_applies_surface_scale() {
        assert_eq!(
            physical_size(640, 360, 1.5),
            D3d11PhysicalSize {
                width: 960,
                height: 540,
            }
        );
    }

    #[test]
    fn d3d11_renderer_config_initializes_upscaler_mode() {
        let renderer = D3d11Renderer::with_config(MetalRendererConfig {
            output_mode: crate::renderer::metal::MetalOutputMode::Sdr,
            luma_upscaler: LumaUpscalerMode::ArtCnnC4F16,
        })
        .unwrap();

        let stats = renderer.runtime_stats();
        assert_eq!(stats.upscaler_mode, LumaUpscalerMode::ArtCnnC4F16);
        assert_eq!(stats.upscaler_backend, LumaUpscalerBackendStatus::Inactive);
    }

    #[test]
    fn d3d11_video_shader_compiles() {
        compile_shader("vs_main", "vs_4_0").unwrap();
        compile_shader("ps_main", "ps_4_0").unwrap();
    }

    #[test]
    fn d3d11_overlay_shader_compiles() {
        compile_shader_source(OVERLAY_SHADER_SOURCE, "overlay_vs_main", "vs_4_0").unwrap();
        compile_shader_source(OVERLAY_SHADER_SOURCE, "overlay_ps_main", "ps_4_0").unwrap();
    }

    #[test]
    fn renderer_stats_distinguish_direct_and_shared_zero_copy() {
        let mut renderer = D3d11Renderer::new().unwrap();
        renderer.stats.hardware_video_frames = 3;
        renderer.stats.zero_copy_video_frames = 3;
        renderer.stats.direct_zero_copy_video_frames = 1;
        renderer.stats.shared_handle_video_frames = 2;
        renderer.stats.hdr_source_frames = 3;
        renderer.stats.hdr10_output_frames = 2;
        renderer.stats.sdr_tonemap_frames = 1;
        renderer.stats.hdr10_output_active = true;

        let stats = renderer.runtime_stats();
        assert_eq!(stats.hardware_video_frames, 3);
        assert_eq!(stats.zero_copy_video_frames, 3);
        assert_eq!(stats.direct_zero_copy_video_frames, 1);
        assert_eq!(stats.shared_handle_video_frames, 2);
        assert_eq!(stats.hdr_source_frames, 3);
        assert_eq!(stats.hdr10_output_frames, 2);
        assert_eq!(stats.sdr_tonemap_frames, 1);
        assert!(stats.hdr10_output_active);
    }

    #[test]
    fn hdr10_output_mode_targets_bt2020_pq() {
        let source = SourceColorState::new(ColorPrimaries::DisplayP3, TransferFunction::Pq);
        let target = D3d11OutputMode::Hdr10.target_color_for_source(source);

        assert_eq!(target.primaries, ColorPrimaries::Bt2020);
        assert_eq!(target.transfer, TransferFunction::Pq);
        assert_eq!(target.peak_nits, 10_000.0);
        assert_eq!(target.reference_white_nits, 203.0);
    }

    #[test]
    fn dxgi_hdr10_metadata_uses_source_mastering_values() {
        let source = SourceColorState::new(ColorPrimaries::Bt2020, TransferFunction::Pq)
            .hdr_metadata(Some(crate::renderer::pipeline::HdrMetadata::new(
                Some(crate::renderer::pipeline::MasteringDisplayMetadata {
                    display_primaries: Some([
                        Chromaticity::new(0.708, 0.292),
                        Chromaticity::new(0.170, 0.797),
                        Chromaticity::new(0.131, 0.046),
                    ]),
                    white_point: Some(Chromaticity::new(0.3127, 0.3290)),
                    min_luminance_nits: Some(0.005),
                    max_luminance_nits: Some(1000.0),
                }),
                Some(crate::renderer::pipeline::ContentLightMetadata {
                    max_content_light_level_nits: 4000,
                    max_frame_average_light_level_nits: 450,
                }),
            )));

        let metadata = dxgi_hdr10_metadata(source);

        assert_eq!(metadata.RedPrimary, [35400, 14600]);
        assert_eq!(metadata.GreenPrimary, [8500, 39850]);
        assert_eq!(metadata.BluePrimary, [6550, 2300]);
        assert_eq!(metadata.WhitePoint, [15635, 16450]);
        assert_eq!(metadata.MaxMasteringLuminance, 1000);
        assert_eq!(metadata.MinMasteringLuminance, 50);
        assert_eq!(metadata.MaxContentLightLevel, 4000);
        assert_eq!(metadata.MaxFrameAverageLightLevel, 450);
    }
}
