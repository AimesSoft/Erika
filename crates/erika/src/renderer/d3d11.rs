use std::ffi::c_void;
use std::mem;
use std::ptr;

use ::windows::Win32::Foundation::{HMODULE, HWND};
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
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12,
    DXGI_FORMAT_P010, DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_R8G8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM,
    DXGI_FORMAT_R16_UNORM, DXGI_FORMAT_R16G16_UNORM, DXGI_FORMAT_R32G32_FLOAT, DXGI_SAMPLE_DESC,
};
use ::windows::Win32::Graphics::Dxgi::{
    DXGI_PRESENT, DXGI_PRESENT_PARAMETERS, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter, IDXGIDevice,
    IDXGIFactory2, IDXGISwapChain1,
};
use ::windows::core::{Interface, PCSTR};

use crate::core::{
    ColorPrimaries, LumaUpscalerBackendStatus, PlatformSurface, PlayerError, PlayerVideoFrame,
    RenderFrameContext, RendererBackend, RendererRuntimeStats, Result, WgpuSurfaceKind,
};
use crate::danmaku::{DanmakuGlyphAtlas, DanmakuGlyphInstance, DanmakuRenderPlan};
use crate::ffmpeg::Frame;
use crate::overlay::OverlayFrame;
use crate::renderer::pipeline::{
    LumaUpscalerMode, SourceColorState, TargetColorState, VideoRenderPipeline, VideoUniforms,
};
use crate::subtitle::AssColor;

const SWAPCHAIN_FORMAT: DXGI_FORMAT = DXGI_FORMAT_B8G8R8A8_UNORM;
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
    pub cpu_video_frame_fallbacks: u64,
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
    swapchain: Option<IDXGISwapChain1>,
    render_target: Option<ID3D11RenderTargetView>,
}

struct ImportedVideoFrame {
    _frame: Frame,
    _texture: ID3D11Texture2D,
    luma: ID3D11ShaderResourceView,
    chroma: ID3D11ShaderResourceView,
    _width: u32,
    _height: u32,
    _array_index: u32,
    constants: VideoUniforms,
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
    stats: D3d11RendererStats,
}

impl D3d11Renderer {
    pub fn new() -> Result<Self> {
        Ok(Self {
            state: None,
            surface: None,
            current_video: None,
            danmaku_atlas_cache: None,
            upscaler_mode: LumaUpscalerMode::Off,
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
        trace("recreate_surface_targets: create_swapchain");
        surface.swapchain = Some(create_swapchain(
            &state.device,
            surface.hwnd,
            surface.width,
            surface.height,
            surface.scale,
        )?);
        trace("recreate_surface_targets: create_render_target");
        surface.render_target = Some(create_render_target(
            &state.device,
            surface.swapchain.as_ref().expect("swapchain just created"),
        )?);
        self.stats.surface_width = scaled(surface.width, surface.scale);
        self.stats.surface_height = scaled(surface.height, surface.scale);
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
        let texture = clone_d3d11_texture(texture_ref.raw_texture())?;
        self.ensure_device_for_texture(&texture)?;

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

        let state = self.state.as_ref().expect("device ensured");
        let luma = create_plane_srv(state, &texture, array_index, texture_format.luma_srv())
            .map_err(|error| d3d11va_srv_error(error, &desc, array_index))?;
        let chroma = create_plane_srv(state, &texture, array_index, texture_format.chroma_srv())
            .map_err(|error| d3d11va_srv_error(error, &desc, array_index))?;
        self.stats.hardware_video_frames += 1;
        self.stats.zero_copy_video_frames += 1;
        self.current_video = Some(ImportedVideoFrame {
            _frame: retained_frame,
            _texture: texture,
            luma,
            chroma,
            _width: texture_ref.width().max(1),
            _height: texture_ref.height().max(1),
            _array_index: array_index,
            constants: constants_for_frame(frame, texture_format),
        });
        Ok(())
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
            draws.push(D3d11OverlayDraw {
                texture,
                constants: OverlayUniforms::rgba_plane(
                    plane.x,
                    plane.y,
                    plane.width,
                    plane.height,
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
        state.draw_video(video, rtv, surface.width, surface.height)?;
        if !overlay_draws.is_empty() {
            state.draw_overlays(&overlay_draws, rtv, surface.width, surface.height)?;
        }
        if !danmaku_draws.is_empty() {
            state.draw_overlays(&danmaku_draws, rtv, surface.width, surface.height)?;
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
        let color = [
            (time_seconds.sin() * 0.5 + 0.5) as f32,
            ((time_seconds * 0.73).sin() * 0.5 + 0.5) as f32,
            ((time_seconds * 1.37).cos() * 0.5 + 0.5) as f32,
            1.0,
        ];
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
            swapchain: None,
            render_target: None,
        });
        self.stats.attached = true;
        self.recreate_surface_targets()
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
            cpu_video_frame_fallbacks: self.stats.cpu_video_frame_fallbacks,
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
        width: u32,
        height: u32,
    ) -> Result<()> {
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
        unsafe {
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
        unsafe {
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
        Format: SWAPCHAIN_FORMAT,
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
    let vertices = [
        VideoVertex {
            position: [-1.0, -1.0],
            texcoord: [0.0, 1.0],
        },
        VideoVertex {
            position: [-1.0, 1.0],
            texcoord: [0.0, 0.0],
        },
        VideoVertex {
            position: [1.0, -1.0],
            texcoord: [1.0, 1.0],
        },
        VideoVertex {
            position: [1.0, -1.0],
            texcoord: [1.0, 1.0],
        },
        VideoVertex {
            position: [-1.0, 1.0],
            texcoord: [0.0, 0.0],
        },
        VideoVertex {
            position: [1.0, 1.0],
            texcoord: [1.0, 0.0],
        },
    ];
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

fn constants_for_frame(
    frame: &PlayerVideoFrame,
    texture_format: D3d11VideoTextureFormat,
) -> VideoUniforms {
    let source = SourceColorState::new(
        frame.frame.color_primaries(),
        frame.frame.transfer_function(),
    )
    .range(frame.frame.color_range())
    .matrix(frame.frame.matrix_coefficients())
    .hdr_metadata(frame.frame.hdr_metadata());
    let pipeline = VideoRenderPipeline::new(source, TargetColorState::sdr(ColorPrimaries::Bt709));
    VideoUniforms::from_pipeline(
        &pipeline,
        matches!(texture_format, D3d11VideoTextureFormat::P010),
        false,
    )
}

fn scaled(value: u32, scale: f64) -> u32 {
    let scale = if scale.is_finite() {
        scale.max(1.0)
    } else {
        1.0
    };
    ((value.max(1) as f64) * scale).round().min(u32::MAX as f64) as u32
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
    fn d3d11_video_shader_compiles() {
        compile_shader("vs_main", "vs_4_0").unwrap();
        compile_shader("ps_main", "ps_4_0").unwrap();
    }

    #[test]
    fn d3d11_overlay_shader_compiles() {
        compile_shader_source(OVERLAY_SHADER_SOURCE, "overlay_vs_main", "vs_4_0").unwrap();
        compile_shader_source(OVERLAY_SHADER_SOURCE, "overlay_ps_main", "ps_4_0").unwrap();
    }
}
