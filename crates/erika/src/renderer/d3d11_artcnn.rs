//! D3D11 compute implementation of the ArtCNN C4 luma doubler.
//!
//! The network is evaluated one bounded tile at a time. Three RGBA16F array
//! textures hold the feature slices and are reused for every tile; the final
//! DepthToSpace result is packed into a source-sized RGBA16F texture whose
//! channels contain the virtual 2x2 luma block in TL/TR/BL/BR order.

use std::ffi::c_void;
use std::mem;
use std::time::{Duration, Instant};

use ::windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use ::windows::Win32::Graphics::Direct3D::{
    D3D_FEATURE_LEVEL_11_0, D3D_SRV_DIMENSION_BUFFER, ID3DBlob,
};
use ::windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_UNORDERED_ACCESS,
    D3D11_BUFFER_DESC, D3D11_BUFFER_SRV, D3D11_BUFFER_SRV_0, D3D11_BUFFER_SRV_1,
    D3D11_SHADER_RESOURCE_VIEW_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC_0, D3D11_SUBRESOURCE_DATA,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Buffer, ID3D11ComputeShader, ID3D11Device,
    ID3D11DeviceContext, ID3D11Resource, ID3D11ShaderResourceView, ID3D11Texture2D,
    ID3D11UnorderedAccessView,
};
use ::windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_SAMPLE_DESC};
use ::windows::core::{Interface, PCSTR};

use crate::core::{LumaUpscalerBackendStatus, PlayerError, Result};
use crate::renderer::artcnn::{FrameTokenCache, LayerOffsets, MIDDLE_LAYER_COUNT, model_for_mode};
use crate::renderer::pipeline::LumaUpscalerMode;

const TILE_WIDTH: u32 = 512;
const TILE_HEIGHT: u32 = 256;
const NETWORK_RADIUS: u32 = 7;
const FEATURE_HALO: u32 = NETWORK_RADIUS - 1;
const WORKGROUP_X: u32 = 8;
const WORKGROUP_Y: u32 = 8;

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug)]
struct TileParams {
    image_core: [u32; 4],
    origins: [i32; 4],
    dispatch: [u32; 4],
    layer: [u32; 4],
    luma_coefficients: [f32; 4],
}

#[derive(Clone, Copy)]
enum DispatchKind {
    Conv0,
    Mid(usize),
    Conv6,
}

#[derive(Clone, Copy)]
struct DispatchRecord {
    kind: DispatchKind,
    params: TileParams,
}

struct ModelResources {
    slices: u32,
    offsets: LayerOffsets,
    _weights: ID3D11Buffer,
    weights_view: ID3D11ShaderResourceView,
    constants: ID3D11Buffer,
    conv0_shader: ID3D11ComputeShader,
    mid_shader: ID3D11ComputeShader,
    conv6_shader: ID3D11ComputeShader,
}

struct ComputeTexture {
    _texture: ID3D11Texture2D,
    srv: ID3D11ShaderResourceView,
    uav: ID3D11UnorderedAccessView,
}

struct TexturePool {
    width: u32,
    height: u32,
    features: [ComputeTexture; 3],
    output: ComputeTexture,
    frame_cache: FrameTokenCache,
}

#[derive(Default)]
pub(crate) struct D3d11ArtCnn {
    mode: LumaUpscalerMode,
    status: LumaUpscalerBackendStatus,
    resources: Option<ModelResources>,
    pool: Option<TexturePool>,
    fallback_count: u64,
    upscaled_frames: u64,
    last_encode_duration: Duration,
}

impl D3d11ArtCnn {
    pub(crate) fn status(&self) -> LumaUpscalerBackendStatus {
        self.status
    }

    pub(crate) fn fallback_count(&self) -> u64 {
        self.fallback_count
    }

    pub(crate) fn upscaled_frames(&self) -> u64 {
        self.upscaled_frames
    }

    pub(crate) fn last_encode_duration(&self) -> Duration {
        self.last_encode_duration
    }

    /// Drops resources from a previous decoder device and builds the selected
    /// model on the new D3D11 device.
    pub(crate) fn attach_device(
        &mut self,
        device: &ID3D11Device,
        mode: LumaUpscalerMode,
    ) -> Result<()> {
        self.resources = None;
        self.pool = None;
        self.mode = LumaUpscalerMode::Off;
        self.status = LumaUpscalerBackendStatus::Off;
        self.set_mode(device, mode)
    }

    pub(crate) fn set_mode(&mut self, device: &ID3D11Device, mode: LumaUpscalerMode) -> Result<()> {
        if self.mode == mode
            && (mode == LumaUpscalerMode::Off
                || (self.status == LumaUpscalerBackendStatus::Scalar && self.resources.is_some()))
        {
            return Ok(());
        }
        self.mode = mode;
        self.resources = None;
        self.pool = None;
        if mode == LumaUpscalerMode::Off {
            self.status = LumaUpscalerBackendStatus::Off;
            return Ok(());
        }

        self.status = LumaUpscalerBackendStatus::Building;
        if unsafe { device.GetFeatureLevel() }.0 < D3D_FEATURE_LEVEL_11_0.0 {
            return self.fail(PlayerError::Renderer(
                "d3d11 ArtCNN requires Direct3D feature level 11.0".to_string(),
            ));
        }
        match ModelResources::build(device, mode) {
            Ok(resources) => {
                self.resources = Some(resources);
                self.status = LumaUpscalerBackendStatus::Scalar;
                Ok(())
            }
            Err(error) => self.fail(error),
        }
    }

    pub(crate) fn encode(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        luma: &ID3D11ShaderResourceView,
        width: u32,
        height: u32,
        frame_token: Option<u64>,
    ) -> Result<Option<ID3D11ShaderResourceView>> {
        self.last_encode_duration = Duration::ZERO;
        if self.mode == LumaUpscalerMode::Off {
            return Ok(None);
        }
        if self.status != LumaUpscalerBackendStatus::Scalar {
            return Ok(None);
        }
        if width == 0 || height == 0 {
            return Err(PlayerError::Renderer(
                "d3d11 ArtCNN input dimensions must be non-zero".to_string(),
            ));
        }
        if self
            .pool
            .as_ref()
            .is_some_and(|pool| pool.frame_cache.matches(frame_token))
        {
            return Ok(self.pool.as_ref().map(|pool| pool.output.srv.clone()));
        }

        let resources = self
            .resources
            .as_ref()
            .expect("scalar ArtCNN status has resources");
        if self
            .pool
            .as_ref()
            .is_none_or(|pool| pool.width != width || pool.height != height)
        {
            self.pool = Some(TexturePool::build(device, resources.slices, width, height)?);
        }
        let pool = self.pool.as_ref().expect("ArtCNN pool built above");
        let records = build_dispatch_records(width, height, &resources.offsets, resources.slices);
        let started = Instant::now();

        unsafe {
            context.CSSetConstantBuffers(0, Some(&[Some(resources.constants.clone())]));
            for record in records {
                context.UpdateSubresource(
                    &resources.constants,
                    0,
                    None,
                    (&record.params as *const TileParams).cast::<c_void>(),
                    0,
                    0,
                );
                match record.kind {
                    DispatchKind::Conv0 => {
                        context.CSSetShader(&resources.conv0_shader, None);
                        context.CSSetShaderResources(
                            0,
                            Some(&[
                                Some(luma.clone()),
                                None,
                                None,
                                Some(resources.weights_view.clone()),
                            ]),
                        );
                        set_compute_uavs(context, Some(&pool.features[0].uav), None);
                    }
                    DispatchKind::Mid(layer) => {
                        const CHAIN: [(usize, usize); MIDDLE_LAYER_COUNT] =
                            [(0, 1), (1, 2), (2, 1), (1, 2), (2, 1)];
                        let (src, dst) = CHAIN[layer];
                        context.CSSetShader(&resources.mid_shader, None);
                        context.CSSetShaderResources(
                            0,
                            Some(&[
                                None,
                                Some(pool.features[src].srv.clone()),
                                Some(pool.features[0].srv.clone()),
                                Some(resources.weights_view.clone()),
                            ]),
                        );
                        set_compute_uavs(context, Some(&pool.features[dst].uav), None);
                    }
                    DispatchKind::Conv6 => {
                        context.CSSetShader(&resources.conv6_shader, None);
                        context.CSSetShaderResources(
                            0,
                            Some(&[
                                None,
                                Some(pool.features[1].srv.clone()),
                                None,
                                Some(resources.weights_view.clone()),
                            ]),
                        );
                        set_compute_uavs(context, None, Some(&pool.output.uav));
                    }
                }
                context.Dispatch(
                    record.params.dispatch[1].div_ceil(WORKGROUP_X),
                    record.params.dispatch[2].div_ceil(WORKGROUP_Y),
                    1,
                );
                // An explicit unbind provides the D3D11 read-after-write
                // ordering point before the same texture is used as an SRV.
                set_compute_uavs(context, None, None);
                context.CSSetShaderResources(0, Some(&[None, None, None, None]));
            }
            context.CSSetConstantBuffers(0, Some(&[None]));
            context.CSSetShader(None, None);
        }

        self.pool
            .as_mut()
            .expect("ArtCNN pool built above")
            .frame_cache
            .commit(frame_token);
        self.upscaled_frames = self.upscaled_frames.saturating_add(1);
        self.last_encode_duration = started.elapsed();
        Ok(self.pool.as_ref().map(|pool| pool.output.srv.clone()))
    }

    pub(crate) fn record_runtime_failure(&mut self, _error: &PlayerError) {
        self.status = LumaUpscalerBackendStatus::Inactive;
        self.resources = None;
        self.pool = None;
        self.fallback_count = self.fallback_count.saturating_add(1);
    }

    fn fail<T>(&mut self, error: PlayerError) -> Result<T> {
        self.record_runtime_failure(&error);
        Err(error)
    }
}

impl ModelResources {
    fn build(device: &ID3D11Device, mode: LumaUpscalerMode) -> Result<Self> {
        let model = model_for_mode(mode)?;
        let weights_desc = D3D11_BUFFER_DESC {
            ByteWidth: u32::try_from(model.payload.len()).map_err(|_| {
                PlayerError::Renderer("d3d11 ArtCNN weight buffer is too large".to_string())
            })?,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
            StructureByteStride: 0,
        };
        let weight_data = D3D11_SUBRESOURCE_DATA {
            pSysMem: model.payload.as_ptr().cast::<c_void>(),
            SysMemPitch: 0,
            SysMemSlicePitch: 0,
        };
        let weights = create_buffer(
            device,
            &weights_desc,
            Some(&weight_data),
            "ID3D11Device::CreateBuffer(ArtCNN weights)",
        )?;
        let weight_resource: ID3D11Resource = weights.cast().map_err(|error| {
            PlayerError::Renderer(format!("d3d11 ArtCNN weight resource cast failed: {error}"))
        })?;
        let weight_view_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            ViewDimension: D3D_SRV_DIMENSION_BUFFER,
            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                Buffer: D3D11_BUFFER_SRV {
                    Anonymous1: D3D11_BUFFER_SRV_0 { FirstElement: 0 },
                    Anonymous2: D3D11_BUFFER_SRV_1 {
                        NumElements: model.layout.payload_half4 as u32,
                    },
                },
            },
        };
        let mut weights_view = None;
        unsafe {
            device
                .CreateShaderResourceView(
                    &weight_resource,
                    Some(&weight_view_desc),
                    Some(&mut weights_view),
                )
                .map_err(|error| {
                    PlayerError::Renderer(format!(
                        "d3d11 CreateShaderResourceView(ArtCNN weights) failed: {error}"
                    ))
                })?;
        }
        let weights_view = weights_view
            .ok_or_else(|| PlayerError::Renderer("d3d11 ArtCNN weight SRV was null".to_string()))?;
        let constants_desc = D3D11_BUFFER_DESC {
            ByteWidth: mem::size_of::<TileParams>() as u32,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
            StructureByteStride: 0,
        };
        let constants = create_buffer(
            device,
            &constants_desc,
            None,
            "ID3D11Device::CreateBuffer(ArtCNN constants)",
        )?;
        let slices = model.layout.feature_slices;
        Ok(Self {
            slices,
            offsets: model.layout.layer_offsets,
            _weights: weights,
            weights_view,
            constants,
            conv0_shader: create_compute_shader(device, slices, "artcnn_conv0")?,
            mid_shader: create_compute_shader(device, slices, "artcnn_conv_mid")?,
            conv6_shader: create_compute_shader(device, slices, "artcnn_conv6")?,
        })
    }
}

impl TexturePool {
    fn build(device: &ID3D11Device, slices: u32, width: u32, height: u32) -> Result<Self> {
        let feature_width = TILE_WIDTH + FEATURE_HALO * 2;
        let feature_height = TILE_HEIGHT + FEATURE_HALO * 2;
        Ok(Self {
            width,
            height,
            features: [
                create_compute_texture(device, feature_width, feature_height, slices)?,
                create_compute_texture(device, feature_width, feature_height, slices)?,
                create_compute_texture(device, feature_width, feature_height, slices)?,
            ],
            output: create_compute_texture(device, width, height, 1)?,
            frame_cache: FrameTokenCache::default(),
        })
    }
}

fn create_compute_texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
    array_size: u32,
) -> Result<ComputeTexture> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width.max(1),
        Height: height.max(1),
        MipLevels: 1,
        ArraySize: array_size.max(1),
        Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32 | D3D11_BIND_UNORDERED_ACCESS.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe {
        device
            .CreateTexture2D(&desc, None, Some(&mut texture))
            .map_err(|error| {
                PlayerError::Renderer(format!(
                    "d3d11 CreateTexture2D(ArtCNN {width}x{height}x{array_size}) failed: {error}"
                ))
            })?;
    }
    let texture = texture.ok_or_else(|| {
        PlayerError::Renderer("d3d11 ArtCNN texture allocation returned null".to_string())
    })?;
    let resource: ID3D11Resource = texture.cast().map_err(|error| {
        PlayerError::Renderer(format!(
            "d3d11 ArtCNN texture resource cast failed: {error}"
        ))
    })?;
    let mut srv = None;
    let mut uav = None;
    unsafe {
        device
            .CreateShaderResourceView(&resource, None, Some(&mut srv))
            .map_err(|error| {
                PlayerError::Renderer(format!(
                    "d3d11 CreateShaderResourceView(ArtCNN texture) failed: {error}"
                ))
            })?;
        device
            .CreateUnorderedAccessView(&resource, None, Some(&mut uav))
            .map_err(|error| {
                PlayerError::Renderer(format!(
                    "d3d11 CreateUnorderedAccessView(ArtCNN texture) failed: {error}"
                ))
            })?;
    }
    Ok(ComputeTexture {
        _texture: texture,
        srv: srv.ok_or_else(|| {
            PlayerError::Renderer("d3d11 ArtCNN texture SRV was null".to_string())
        })?,
        uav: uav.ok_or_else(|| {
            PlayerError::Renderer("d3d11 ArtCNN texture UAV was null".to_string())
        })?,
    })
}

unsafe fn set_compute_uavs(
    context: &ID3D11DeviceContext,
    feature: Option<&ID3D11UnorderedAccessView>,
    output: Option<&ID3D11UnorderedAccessView>,
) {
    let views = [feature.cloned(), output.cloned()];
    unsafe {
        context.CSSetUnorderedAccessViews(0, views.len() as u32, Some(views.as_ptr()), None);
    }
}

fn build_dispatch_records(
    width: u32,
    height: u32,
    offsets: &LayerOffsets,
    slices: u32,
) -> Vec<DispatchRecord> {
    let tiles_x = width.div_ceil(TILE_WIDTH);
    let tiles_y = height.div_ceil(TILE_HEIGHT);
    let mut records = Vec::with_capacity((tiles_x * tiles_y * 7) as usize);
    for tile_y in 0..tiles_y {
        for tile_x in 0..tiles_x {
            let core_x = tile_x * TILE_WIDTH;
            let core_y = tile_y * TILE_HEIGHT;
            let core_width = (width - core_x).min(TILE_WIDTH);
            let core_height = (height - core_y).min(TILE_HEIGHT);
            let feature_origin_x = core_x as i32 - FEATURE_HALO as i32;
            let feature_origin_y = core_y as i32 - FEATURE_HALO as i32;
            let params = |inset: u32,
                          dispatch_width: u32,
                          dispatch_height: u32,
                          weight_offset: u32,
                          bias_offset: u32,
                          relu: bool,
                          add_residual: bool| TileParams {
                image_core: [width, height, core_width, core_height],
                origins: [
                    core_x as i32,
                    core_y as i32,
                    feature_origin_x,
                    feature_origin_y,
                ],
                dispatch: [inset, dispatch_width, dispatch_height, slices],
                layer: [
                    weight_offset,
                    bias_offset,
                    u32::from(relu),
                    u32::from(add_residual),
                ],
                luma_coefficients: [0.0; 4],
            };
            records.push(DispatchRecord {
                kind: DispatchKind::Conv0,
                params: params(
                    0,
                    core_width + FEATURE_HALO * 2,
                    core_height + FEATURE_HALO * 2,
                    offsets.conv0_w,
                    offsets.conv0_b,
                    false,
                    false,
                ),
            });
            for layer in 0..MIDDLE_LAYER_COUNT {
                let convolution = layer as u32 + 1;
                records.push(DispatchRecord {
                    kind: DispatchKind::Mid(layer),
                    params: params(
                        convolution,
                        core_width + (FEATURE_HALO - convolution) * 2,
                        core_height + (FEATURE_HALO - convolution) * 2,
                        offsets.mid_w[layer],
                        offsets.mid_b[layer],
                        layer != MIDDLE_LAYER_COUNT - 1,
                        layer == MIDDLE_LAYER_COUNT - 1,
                    ),
                });
            }
            records.push(DispatchRecord {
                kind: DispatchKind::Conv6,
                params: params(
                    FEATURE_HALO,
                    core_width,
                    core_height,
                    offsets.conv6_w,
                    offsets.conv6_b,
                    false,
                    false,
                ),
            });
        }
    }
    records
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
            .CreateBuffer(desc, data.map(|value| value as *const _), Some(&mut buffer))
            .map_err(|error| PlayerError::Renderer(format!("d3d11 {operation} failed: {error}")))?;
    }
    buffer.ok_or_else(|| PlayerError::Renderer(format!("d3d11 {operation} returned null")))
}

fn create_compute_shader(
    device: &ID3D11Device,
    slices: u32,
    entry: &'static str,
) -> Result<ID3D11ComputeShader> {
    let blob = compile_compute_shader(slices, entry)?;
    let mut shader = None;
    unsafe {
        device
            .CreateComputeShader(blob_bytes(&blob), None, Some(&mut shader))
            .map_err(|error| {
                PlayerError::Renderer(format!(
                    "d3d11 CreateComputeShader({entry}) failed: {error}"
                ))
            })?;
    }
    shader.ok_or_else(|| {
        PlayerError::Renderer(format!("d3d11 CreateComputeShader({entry}) returned null"))
    })
}

fn compile_compute_shader(slices: u32, entry: &'static str) -> Result<ID3DBlob> {
    let source = ART_CNN_SHADER_TEMPLATE.replace("{FEATURE_SLICES}", &slices.to_string());
    let entry_name = nul(entry);
    let target = nul("cs_5_0");
    let mut code = None;
    let mut errors = None;
    unsafe {
        let result = D3DCompile(
            source.as_ptr().cast::<c_void>(),
            source.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR(entry_name.as_ptr()),
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
                "d3d11 D3DCompile({entry}/cs_5_0) failed: {message}"
            )));
        }
    }
    code.ok_or_else(|| PlayerError::Renderer("d3d11 D3DCompile returned null".to_string()))
}

fn blob_bytes(blob: &ID3DBlob) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize())
    }
}

fn blob_to_string(blob: &ID3DBlob) -> String {
    String::from_utf8_lossy(blob_bytes(blob))
        .trim_end_matches('\0')
        .to_string()
}

fn nul(value: &str) -> Vec<u8> {
    let mut bytes = value.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

const ART_CNN_SHADER_TEMPLATE: &str = r#"
#define FEATURE_SLICES {FEATURE_SLICES}
#define TAP_COUNT 9

cbuffer TileParams : register(b0) {
    uint4 image_core;
    int4 origins;
    uint4 dispatch_info;
    uint4 layer_info;
    float4 luma_coefficients;
};

Texture2DArray<float> input_luma : register(t0);
Texture2DArray<float4> feature_src : register(t1);
Texture2DArray<float4> residual_src : register(t2);
Buffer<float4> weights : register(t3);
RWTexture2DArray<float4> feature_dst : register(u0);
RWTexture2D<float4> packed_output : register(u1);

bool in_image(int2 coord) {
    return coord.x >= 0 && coord.y >= 0
        && coord.x < int(image_core.x) && coord.y < int(image_core.y);
}

int2 local_coord(uint3 gid) {
    int inset = int(dispatch_info.x);
    return int2(gid.xy) + int2(inset, inset);
}

int2 global_coord(int2 local) {
    return origins.zw + local;
}

bool invocation_is_active(uint3 gid) {
    return gid.x < dispatch_info.y && gid.y < dispatch_info.z;
}

float luma_or_zero(int2 coord) {
    if (!in_image(coord)) {
        return 0.0;
    }
    return input_luma.Load(int4(coord, 0, 0)).r;
}

float4 apply_weight_matrix(uint index, float4 value) {
    return weights[index] * value.x
        + weights[index + 1] * value.y
        + weights[index + 2] * value.z
        + weights[index + 3] * value.w;
}

[numthreads(8, 8, 1)]
void artcnn_conv0(uint3 gid : SV_DispatchThreadID) {
    if (!invocation_is_active(gid)) {
        return;
    }
    int2 local = local_coord(gid);
    int2 global = global_coord(local);
    if (!in_image(global)) {
        for (uint slice = 0; slice < FEATURE_SLICES; ++slice) {
            feature_dst[int3(local, int(slice))] = 0.0;
        }
        return;
    }
    for (uint slice = 0; slice < FEATURE_SLICES; ++slice) {
        float4 acc = weights[layer_info.y + slice];
        for (uint tap = 0; tap < TAP_COUNT; ++tap) {
            int2 delta = int2(int(tap % 3) - 1, int(tap / 3) - 1);
            float value = luma_or_zero(global + delta);
            acc += weights[layer_info.x + slice * TAP_COUNT + tap] * value;
        }
        feature_dst[int3(local, int(slice))] = acc;
    }
}

[numthreads(8, 8, 1)]
void artcnn_conv_mid(uint3 gid : SV_DispatchThreadID) {
    if (!invocation_is_active(gid)) {
        return;
    }
    int2 local = local_coord(gid);
    int2 global = global_coord(local);
    if (!in_image(global)) {
        for (uint slice = 0; slice < FEATURE_SLICES; ++slice) {
            feature_dst[int3(local, int(slice))] = 0.0;
        }
        return;
    }

    float4 acc[FEATURE_SLICES];
    for (uint out_slice = 0; out_slice < FEATURE_SLICES; ++out_slice) {
        acc[out_slice] = weights[layer_info.y + out_slice];
    }
    for (uint in_slice = 0; in_slice < FEATURE_SLICES; ++in_slice) {
        for (uint tap = 0; tap < TAP_COUNT; ++tap) {
            int2 delta = int2(int(tap % 3) - 1, int(tap / 3) - 1);
            float4 value = feature_src.Load(int4(local + delta, int(in_slice), 0));
            for (uint out_slice = 0; out_slice < FEATURE_SLICES; ++out_slice) {
                uint matrix_index = layer_info.x
                    + (((out_slice * TAP_COUNT + tap) * FEATURE_SLICES + in_slice) * 4);
                acc[out_slice] += apply_weight_matrix(matrix_index, value);
            }
        }
    }
    for (uint write_slice = 0; write_slice < FEATURE_SLICES; ++write_slice) {
        float4 value = acc[write_slice];
        if (layer_info.w != 0) {
            value += residual_src.Load(int4(local, int(write_slice), 0));
        }
        if (layer_info.z != 0) {
            value = max(value, 0.0);
        }
        feature_dst[int3(local, int(write_slice))] = value;
    }
}

[numthreads(8, 8, 1)]
void artcnn_conv6(uint3 gid : SV_DispatchThreadID) {
    if (!invocation_is_active(gid)) {
        return;
    }
    int2 local = local_coord(gid);
    int2 global = global_coord(local);
    if (!in_image(global)) {
        return;
    }
    float4 acc = weights[layer_info.y];
    for (uint in_slice = 0; in_slice < FEATURE_SLICES; ++in_slice) {
        for (uint tap = 0; tap < TAP_COUNT; ++tap) {
            int2 delta = int2(int(tap % 3) - 1, int(tap / 3) - 1);
            float4 value = feature_src.Load(int4(local + delta, int(in_slice), 0));
            uint matrix_index = layer_info.x + ((tap * FEATURE_SLICES + in_slice) * 4);
            acc += apply_weight_matrix(matrix_index, value);
        }
    }
    packed_output[global] = clamp(acc, 0.0, 1.0);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use ::windows::Win32::Foundation::HMODULE;
    use ::windows::Win32::Graphics::Direct3D::{
        D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP,
        D3D_SRV_DIMENSION_TEXTURE2DARRAY,
    };
    use ::windows::Win32::Graphics::Direct3D11::{
        D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ,
        D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEX2D_ARRAY_SRV, D3D11_USAGE_STAGING,
        D3D11CreateDevice,
    };
    use ::windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32_FLOAT;

    const NUMERIC_WIDTH: u32 = 128;
    const NUMERIC_HEIGHT: u32 = 72;

    #[test]
    fn d3d11_artcnn_compute_shaders_compile_for_both_models() {
        for slices in [4, 8] {
            for entry in ["artcnn_conv0", "artcnn_conv_mid", "artcnn_conv6"] {
                compile_compute_shader(slices, entry).unwrap();
            }
        }
    }

    #[test]
    fn tiled_dispatch_has_seven_passes_per_tile_and_shrinks_halo() {
        let offsets = LayerOffsets::for_slices(4);
        let records = build_dispatch_records(513, 257, &offsets, 4);
        assert_eq!(records.len(), 4 * 7);
        assert_eq!(records[0].params.dispatch, [0, 524, 268, 4]);
        assert_eq!(records[1].params.dispatch, [1, 522, 266, 4]);
        assert_eq!(records[6].params.dispatch, [6, 512, 256, 4]);
        assert_eq!(records[7].params.image_core[2..4], [1, 256]);
    }

    #[test]
    fn d3d11_artcnn_matches_onnx_reference() {
        let Some((device, context)) = create_test_device() else {
            eprintln!("skipping D3D11 ArtCNN numeric test: no FL11 device");
            return;
        };
        check_model(
            &device,
            &context,
            LumaUpscalerMode::ArtCnnC4F16,
            include_bytes!("../../tests/data/artcnn/c4f16/input_128x72.f32"),
            include_bytes!("../../tests/data/artcnn/c4f16/output_256x144.f32"),
        );
        check_model(
            &device,
            &context,
            LumaUpscalerMode::ArtCnnC4F16Ds,
            include_bytes!("../../tests/data/artcnn/c4f16/input_128x72.f32"),
            include_bytes!("../../tests/data/artcnn/c4f16_ds/output_256x144.f32"),
        );
        check_model(
            &device,
            &context,
            LumaUpscalerMode::ArtCnnC4F32,
            include_bytes!("../../tests/data/artcnn/c4f32/input_128x72.f32"),
            include_bytes!("../../tests/data/artcnn/c4f32/output_256x144.f32"),
        );
    }

    fn create_test_device() -> Option<(ID3D11Device, ID3D11DeviceContext)> {
        for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
            if let Some(pair) = try_create_test_device(driver) {
                return Some(pair);
            }
        }
        None
    }

    fn try_create_test_device(
        driver: D3D_DRIVER_TYPE,
    ) -> Option<(ID3D11Device, ID3D11DeviceContext)> {
        let mut device = None;
        let mut context = None;
        let mut selected = D3D_FEATURE_LEVEL_11_0;
        let result = unsafe {
            D3D11CreateDevice(
                None,
                driver,
                HMODULE::default(),
                Default::default(),
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                Some(&mut selected),
                Some(&mut context),
            )
        };
        if result.is_err() || selected.0 < D3D_FEATURE_LEVEL_11_0.0 {
            return None;
        }
        Some((device?, context?))
    }

    fn check_model(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        mode: LumaUpscalerMode,
        input: &[u8],
        expected: &[u8],
    ) {
        let luma = upload_luma(device, input);
        let mut artcnn = D3d11ArtCnn::default();
        artcnn.set_mode(device, mode).expect("build D3D11 model");
        let output = artcnn
            .encode(
                device,
                context,
                &luma,
                NUMERIC_WIDTH,
                NUMERIC_HEIGHT,
                Some(1),
            )
            .expect("encode D3D11 ArtCNN")
            .expect("ArtCNN output");
        let actual = readback_packed(device, context, &output);
        let expected = load_f32(expected);
        assert_eq!(actual.len(), expected.len());
        let mut error_sum = 0.0f64;
        let mut max_error = 0.0f64;
        for (&actual, &expected) in actual.iter().zip(&expected) {
            let error = f64::from(actual - expected).abs();
            error_sum += error;
            max_error = max_error.max(error);
        }
        let mae = error_sum / actual.len() as f64;
        eprintln!("{mode:?}/D3D11 tiled scalar: mae={mae:.6} max={max_error:.6}");
        assert!(mae < 1.5e-3, "MAE too high: {mae}");
        assert!(max_error < 2.0e-2, "max error too high: {max_error}");

        let cached = artcnn
            .encode(
                device,
                context,
                &luma,
                NUMERIC_WIDTH,
                NUMERIC_HEIGHT,
                Some(1),
            )
            .expect("reuse cached D3D11 output");
        assert!(cached.is_some());
        assert_eq!(artcnn.upscaled_frames(), 1);
    }

    fn upload_luma(device: &ID3D11Device, bytes: &[u8]) -> ID3D11ShaderResourceView {
        assert_eq!(bytes.len(), (NUMERIC_WIDTH * NUMERIC_HEIGHT * 4) as usize);
        let texture_desc = D3D11_TEXTURE2D_DESC {
            Width: NUMERIC_WIDTH,
            Height: NUMERIC_HEIGHT,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let initial_data = D3D11_SUBRESOURCE_DATA {
            pSysMem: bytes.as_ptr().cast::<c_void>(),
            SysMemPitch: NUMERIC_WIDTH * 4,
            SysMemSlicePitch: NUMERIC_WIDTH * NUMERIC_HEIGHT * 4,
        };
        let mut texture = None;
        unsafe {
            device
                .CreateTexture2D(&texture_desc, Some(&initial_data), Some(&mut texture))
                .expect("create D3D11 numeric input");
        }
        let texture = texture.expect("D3D11 numeric input texture");
        let resource: ID3D11Resource = texture.cast().expect("cast D3D11 numeric input");
        let view_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R32_FLOAT,
            ViewDimension: D3D_SRV_DIMENSION_TEXTURE2DARRAY,
            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2DArray: D3D11_TEX2D_ARRAY_SRV {
                    MostDetailedMip: 0,
                    MipLevels: 1,
                    FirstArraySlice: 0,
                    ArraySize: 1,
                },
            },
        };
        let mut view = None;
        unsafe {
            device
                .CreateShaderResourceView(&resource, Some(&view_desc), Some(&mut view))
                .expect("create D3D11 numeric input SRV");
        }
        view.expect("D3D11 numeric input SRV")
    }

    fn readback_packed(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        output: &ID3D11ShaderResourceView,
    ) -> Vec<f32> {
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Width: NUMERIC_WIDTH,
            Height: NUMERIC_HEIGHT,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut staging = None;
        unsafe {
            device
                .CreateTexture2D(&staging_desc, None, Some(&mut staging))
                .expect("create D3D11 staging texture");
        }
        let staging = staging.expect("D3D11 staging texture");
        let staging_resource: ID3D11Resource = staging.cast().expect("cast D3D11 staging texture");
        let output_resource = unsafe { output.GetResource() }.expect("get D3D11 output resource");
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        let bytes = unsafe {
            context.CopyResource(&staging_resource, &output_resource);
            context
                .Map(&staging_resource, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .expect("map D3D11 staging texture");
            std::slice::from_raw_parts(
                mapped.pData.cast::<u8>(),
                mapped.RowPitch as usize * NUMERIC_HEIGHT as usize,
            )
            .to_vec()
        };
        unsafe {
            context.Unmap(&staging_resource, 0);
        }
        unpack_packed(&bytes, mapped.RowPitch)
    }

    fn unpack_packed(bytes: &[u8], row_pitch: u32) -> Vec<f32> {
        assert!(row_pitch >= NUMERIC_WIDTH * 8);
        let output_width = NUMERIC_WIDTH as usize * 2;
        let mut output = vec![0.0; output_width * NUMERIC_HEIGHT as usize * 2];
        for y in 0..NUMERIC_HEIGHT as usize {
            let row = y * row_pitch as usize;
            for x in 0..NUMERIC_WIDTH as usize {
                let pixel = row + x * 8;
                let values = [
                    half_to_f32(u16::from_le_bytes([bytes[pixel], bytes[pixel + 1]])),
                    half_to_f32(u16::from_le_bytes([bytes[pixel + 2], bytes[pixel + 3]])),
                    half_to_f32(u16::from_le_bytes([bytes[pixel + 4], bytes[pixel + 5]])),
                    half_to_f32(u16::from_le_bytes([bytes[pixel + 6], bytes[pixel + 7]])),
                ];
                let out_x = x * 2;
                let out_y = y * 2;
                output[out_y * output_width + out_x] = values[0];
                output[out_y * output_width + out_x + 1] = values[1];
                output[(out_y + 1) * output_width + out_x] = values[2];
                output[(out_y + 1) * output_width + out_x + 1] = values[3];
            }
        }
        output
    }

    fn load_f32(bytes: &[u8]) -> Vec<f32> {
        assert_eq!(bytes.len() % 4, 0);
        bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes(chunk.try_into().expect("four bytes")))
            .collect()
    }

    fn half_to_f32(bits: u16) -> f32 {
        let sign = u32::from(bits >> 15) << 31;
        let exponent = u32::from((bits >> 10) & 0x1f);
        let mantissa = u32::from(bits & 0x03ff);
        let value = match (exponent, mantissa) {
            (0, 0) => sign,
            (0, _) => {
                let shift = mantissa.leading_zeros() - 21;
                let exponent = 127 - 15 - shift;
                let mantissa = (mantissa << (shift + 1)) & 0x03ff;
                sign | (exponent << 23) | (mantissa << 13)
            }
            (0x1f, 0) => sign | 0x7f80_0000,
            (0x1f, _) => sign | 0x7f80_0000 | (mantissa << 13),
            _ => sign | ((exponent + (127 - 15)) << 23) | (mantissa << 13),
        };
        f32::from_bits(value)
    }
}
