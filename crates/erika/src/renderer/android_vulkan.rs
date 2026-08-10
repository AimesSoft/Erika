use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, c_void};
use std::fmt;
use std::io::Cursor;
use std::mem;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ash::vk;
use ash::vk::Handle as _;
use wgpu::hal::Instance as _;

use crate::renderer::pipeline::{ColorRange, MatrixCoefficients};

const ANDROID_HARDWARE_BUFFER_USAGE_GPU_SAMPLED_IMAGE: u64 = 1 << 8;
const ANDROID_HARDWARE_BUFFER_USAGE_PROTECTED_CONTENT: u64 = 1 << 14;
const MAX_PENDING_AHB_CONVERSIONS: usize = 3;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub(crate) enum AndroidAhbIntermediateFormat {
    Rgb10a2Unorm,
    Rgba16Float,
}

impl AndroidAhbIntermediateFormat {
    fn wgpu(self) -> wgpu::TextureFormat {
        match self {
            Self::Rgb10a2Unorm => wgpu::TextureFormat::Rgb10a2Unorm,
            Self::Rgba16Float => wgpu::TextureFormat::Rgba16Float,
        }
    }

    fn vk(self) -> vk::Format {
        match self {
            Self::Rgb10a2Unorm => vk::Format::A2B10G10R10_UNORM_PACK32,
            Self::Rgba16Float => vk::Format::R16G16B16A16_SFLOAT,
        }
    }

    pub(crate) fn diagnostic_name(self) -> &'static str {
        match self {
            Self::Rgb10a2Unorm => "rgb10a2unorm",
            Self::Rgba16Float => "rgba16float",
        }
    }

    pub(crate) fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::Rgb10a2Unorm => 4,
            Self::Rgba16Float => 8,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct YcbcrDescriptorBudgetPolicy {
    limit: u32,
    source: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AndroidAhbConversionError {
    Backpressure { pending: usize, max: usize },
    Interop(String),
}

impl fmt::Display for AndroidAhbConversionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backpressure { pending, max } => write!(
                formatter,
                "stage=reserve_ahb_conversion reason=retirement_capacity_exhausted pending={pending} max={max}"
            ),
            Self::Interop(message) => formatter.write_str(message),
        }
    }
}

impl From<String> for AndroidAhbConversionError {
    fn from(message: String) -> Self {
        Self::Interop(message)
    }
}

/// Pixel crop in the allocation's coordinate system. `right` and `bottom` are exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AndroidAhbCrop {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

/// A decoded AHardwareBuffer that is ready for Vulkan access.
///
/// The ImageReader acquire fence must have been waited before this reaches the converter. The
/// opaque owner normally contains the `AImage` plus the decoder frame source and is transferred to
/// the returned pending bundle so neither can be released while Vulkan still uses the buffer.
pub(crate) struct AndroidAhbFrameDescription {
    pub hardware_buffer: NonNull<vk::AHardwareBuffer>,
    pub buffer_width: u32,
    pub buffer_height: u32,
    pub crop: AndroidAhbCrop,
    /// Size of the intermediate RGB texture. This may be smaller than the decoded crop when the
    /// presentation surface is smaller than the video.
    pub output_width: u32,
    pub output_height: u32,
    pub color_range: ColorRange,
    pub matrix_coefficients: MatrixCoefficients,
    pub output_format: AndroidAhbIntermediateFormat,
    pub owner: Arc<dyn Any + Send + Sync>,
}

pub(crate) struct AndroidAhbConversion {
    pub texture: wgpu::Texture,
    pub width: u32,
    pub height: u32,
    pub output_format: AndroidAhbIntermediateFormat,
    /// Must be retained until the queue submission containing the conversion has completed.
    pub pending: PendingAndroidAhbConversion,
}

/// Owns every native object referenced by the recorded conversion commands.
///
/// Dropping this object before the corresponding queue submission completes violates Vulkan object
/// lifetime rules. Callers should move it into their normal submitted-work retirement queue.
pub(crate) struct PendingAndroidAhbConversion {
    device: ash::Device,
    imported_image: vk::Image,
    imported_memory: vk::DeviceMemory,
    imported_view: vk::ImageView,
    output_view: vk::ImageView,
    descriptor_pool: vk::DescriptorPool,
    framebuffer: vk::Framebuffer,
    retirement_counter: Arc<AtomicUsize>,
    _cached: Arc<CachedAndroidAhbConversion>,
    owner: Option<Arc<dyn Any + Send + Sync>>,
}

impl PendingAndroidAhbConversion {
    fn new(
        interop: &AndroidVulkanInterop,
        owner: Arc<dyn Any + Send + Sync>,
        cached: Arc<CachedAndroidAhbConversion>,
        reservation: PendingConversionReservation,
    ) -> Self {
        Self {
            device: interop.device.clone(),
            imported_image: vk::Image::null(),
            imported_memory: vk::DeviceMemory::null(),
            imported_view: vk::ImageView::null(),
            output_view: vk::ImageView::null(),
            descriptor_pool: vk::DescriptorPool::null(),
            framebuffer: vk::Framebuffer::null(),
            retirement_counter: reservation.commit(),
            _cached: cached,
            owner: Some(owner),
        }
    }
}

struct PendingConversionReservation {
    counter: Arc<AtomicUsize>,
    active: bool,
}

impl PendingConversionReservation {
    fn commit(mut self) -> Arc<AtomicUsize> {
        self.active = false;
        Arc::clone(&self.counter)
    }
}

impl Drop for PendingConversionReservation {
    fn drop(&mut self) {
        if self.active {
            self.counter.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

impl Drop for PendingAndroidAhbConversion {
    fn drop(&mut self) {
        unsafe {
            if !self.descriptor_pool.is_null() {
                self.device
                    .destroy_descriptor_pool(self.descriptor_pool, None);
            }
            if !self.framebuffer.is_null() {
                self.device.destroy_framebuffer(self.framebuffer, None);
            }
            if !self.imported_view.is_null() {
                self.device.destroy_image_view(self.imported_view, None);
            }
            if !self.output_view.is_null() {
                self.device.destroy_image_view(self.output_view, None);
            }
            if !self.imported_image.is_null() {
                self.device.destroy_image(self.imported_image, None);
            }
            if !self.imported_memory.is_null() {
                self.device.free_memory(self.imported_memory, None);
            }
        }
        drop(self.owner.take());
        self.retirement_counter.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Registers native frame resources for destruction after all previously submitted work finishes.
/// Call this immediately after submitting the command buffer that contains the conversion.
pub(crate) fn retire_ahb_conversion_after_submission(
    queue: &wgpu::Queue,
    pending: PendingAndroidAhbConversion,
) {
    queue.on_submitted_work_done(move || drop(pending));
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct ConversionCacheKey {
    format: i32,
    external_format: u64,
    component_r: i32,
    component_g: i32,
    component_b: i32,
    component_a: i32,
    model: i32,
    range: i32,
    x_chroma_offset: i32,
    y_chroma_offset: i32,
    chroma_filter: i32,
    output_format: i32,
}

impl ConversionCacheKey {
    fn new(
        properties: &AhbProperties,
        parameters: ConversionParameters,
        output_format: AndroidAhbIntermediateFormat,
    ) -> Self {
        Self {
            format: properties.import_format().as_raw(),
            external_format: properties.external_format,
            component_r: properties.components.r.as_raw(),
            component_g: properties.components.g.as_raw(),
            component_b: properties.components.b.as_raw(),
            component_a: properties.components.a.as_raw(),
            model: parameters.model.as_raw(),
            range: parameters.range.as_raw(),
            x_chroma_offset: properties.suggested_x_chroma_offset.as_raw(),
            y_chroma_offset: properties.suggested_y_chroma_offset.as_raw(),
            chroma_filter: parameters.chroma_filter.as_raw(),
            output_format: output_format.vk().as_raw(),
        }
    }
}

struct CachedAndroidAhbConversion {
    instance: ash::Instance,
    device: ash::Device,
    ycbcr_via_khr: bool,
    ycbcr_conversion: vk::SamplerYcbcrConversion,
    sampler: vk::Sampler,
    descriptor_set_layout: vk::DescriptorSetLayout,
    pipeline_layout: vk::PipelineLayout,
    render_pass: vk::RenderPass,
    vertex_shader: vk::ShaderModule,
    fragment_shader: vk::ShaderModule,
    pipeline: vk::Pipeline,
    _device_guard: wgpu::Device,
}

impl CachedAndroidAhbConversion {
    fn empty(
        interop: &AndroidVulkanInterop,
        device_guard: &wgpu::Device,
        ycbcr_via_khr: bool,
    ) -> Self {
        Self {
            instance: interop.instance.clone(),
            device: interop.device.clone(),
            ycbcr_via_khr,
            ycbcr_conversion: vk::SamplerYcbcrConversion::null(),
            sampler: vk::Sampler::null(),
            descriptor_set_layout: vk::DescriptorSetLayout::null(),
            pipeline_layout: vk::PipelineLayout::null(),
            render_pass: vk::RenderPass::null(),
            vertex_shader: vk::ShaderModule::null(),
            fragment_shader: vk::ShaderModule::null(),
            pipeline: vk::Pipeline::null(),
            _device_guard: device_guard.clone(),
        }
    }
}

impl Drop for CachedAndroidAhbConversion {
    fn drop(&mut self) {
        unsafe {
            if !self.pipeline.is_null() {
                self.device.destroy_pipeline(self.pipeline, None);
            }
            if !self.pipeline_layout.is_null() {
                self.device
                    .destroy_pipeline_layout(self.pipeline_layout, None);
            }
            if !self.descriptor_set_layout.is_null() {
                self.device
                    .destroy_descriptor_set_layout(self.descriptor_set_layout, None);
            }
            if !self.render_pass.is_null() {
                self.device.destroy_render_pass(self.render_pass, None);
            }
            if !self.vertex_shader.is_null() {
                self.device.destroy_shader_module(self.vertex_shader, None);
            }
            if !self.fragment_shader.is_null() {
                self.device
                    .destroy_shader_module(self.fragment_shader, None);
            }
            if !self.sampler.is_null() {
                self.device.destroy_sampler(self.sampler, None);
            }
            if !self.ycbcr_conversion.is_null() {
                if self.ycbcr_via_khr {
                    ash::khr::sampler_ycbcr_conversion::Device::new(&self.instance, &self.device)
                        .destroy_sampler_ycbcr_conversion(self.ycbcr_conversion, None);
                } else {
                    self.device
                        .destroy_sampler_ycbcr_conversion(self.ycbcr_conversion, None);
                }
            }
        }
    }
}

pub(crate) struct AndroidVulkanDeviceContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub supports_16bit_norm: bool,
    pub interop: AndroidVulkanInterop,
}

pub(crate) struct AndroidVulkanInterop {
    pub(crate) instance: ash::Instance,
    pub(crate) device: ash::Device,
    pub(crate) physical_device: vk::PhysicalDevice,
    pub(crate) queue_family_index: u32,
    pub(crate) ahardware_buffer: ash::android::external_memory_android_hardware_buffer::Device,
    supports_rgb10a2_intermediate: bool,
    conversion_cache: Mutex<HashMap<ConversionCacheKey, Arc<CachedAndroidAhbConversion>>>,
    pending_conversions: Arc<AtomicUsize>,
    descriptor_budget_limit: u32,
    descriptor_budget_source: &'static str,
    descriptor_budget_hint: AtomicU32,
}

impl AndroidVulkanInterop {
    pub(crate) fn supports_rgb10a2_intermediate(&self) -> bool {
        self.supports_rgb10a2_intermediate
    }

    /// Imports and converts one decoded AHardwareBuffer into an ordinary wgpu RGB texture.
    ///
    /// The function first uses `state_encoder` for a wgpu clear pass that establishes the output
    /// texture's tracked state, then records the native Vulkan YCbCr conversion through
    /// `conversion_encoder::as_hal_mut`. wgpu deliberately forbids mixing its encoding API and the
    /// raw encoding API on one command encoder, so callers must submit the two finished command
    /// buffers in that order. The native pass leaves the output in `COLOR_ATTACHMENT_OPTIMAL`,
    /// matching wgpu's tracked state, so later wgpu sampling transitions are both valid and
    /// visibility-complete.
    ///
    /// # Safety
    ///
    /// `frame.hardware_buffer` must point to a live AHardwareBuffer whose acquire fence has already
    /// signalled. The returned `pending` bundle must not be dropped until the queue submission that
    /// contains both encoders has completed.
    pub(crate) unsafe fn convert_ahardware_buffer(
        &self,
        wgpu_device: &wgpu::Device,
        state_encoder: &mut wgpu::CommandEncoder,
        conversion_encoder: &mut wgpu::CommandEncoder,
        frame: AndroidAhbFrameDescription,
    ) -> Result<AndroidAhbConversion, AndroidAhbConversionError> {
        self.validate_wgpu_device(wgpu_device)?;
        let (visible_width, visible_height, mut output_width, mut output_height) =
            validate_frame_description(&frame, wgpu_device)?;
        // Native wgpu callbacks are driven by device polling. Service completed
        // submissions before enforcing the bounded AHB retirement queue so a
        // completed conversion never leaves the zero-copy path permanently full.
        wgpu_device.poll(wgpu::PollType::Poll).map_err(|error| {
            AndroidAhbConversionError::Interop(format!("stage=poll_ahb_retirements reason={error}"))
        })?;
        let reservation = self.reserve_pending_conversion()?;

        let AndroidAhbFrameDescription {
            hardware_buffer,
            buffer_width,
            buffer_height,
            crop,
            output_width: _,
            output_height: _,
            color_range,
            matrix_coefficients,
            output_format,
            owner,
        } = frame;

        let properties = unsafe { self.query_ahb_properties(hardware_buffer)? };
        validate_ahb_format(&properties)?;
        let conversion = conversion_parameters(&properties, color_range, matrix_coefficients)?;
        // Minifying with nearest sampling is visibly worse than the existing full-resolution
        // conversion. Only apply the surface-sized optimization when the imported YCbCr format
        // advertises linear filtering.
        if conversion.chroma_filter != vk::Filter::LINEAR {
            output_width = visible_width;
            output_height = visible_height;
        }
        let cached =
            unsafe { self.cached_conversion(wgpu_device, &properties, conversion, output_format)? };
        let mut pending =
            PendingAndroidAhbConversion::new(self, owner, Arc::clone(&cached), reservation);

        pending.imported_image = unsafe {
            self.create_imported_image(
                buffer_width,
                buffer_height,
                properties.import_format(),
                properties.external_format,
            )?
        };
        pending.imported_memory = unsafe {
            self.allocate_imported_memory(
                pending.imported_image,
                hardware_buffer,
                properties.allocation_size,
                properties.memory_type_bits,
            )?
        };
        unsafe {
            self.device
                .bind_image_memory(pending.imported_image, pending.imported_memory, 0)
                .map_err(|error| vk_stage_error("bind_imported_ahb_memory", error))?;
        }

        pending.imported_view = unsafe {
            create_ycbcr_image_view(
                &self.device,
                pending.imported_image,
                properties.import_format(),
                cached.ycbcr_conversion,
            )?
        };

        let output = wgpu_device.create_texture(&wgpu::TextureDescriptor {
            label: Some("erika-android-ahb-rgb"),
            size: wgpu::Extent3d {
                width: output_width,
                height: output_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: output_format.wgpu(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        establish_wgpu_output_state(state_encoder, &output);
        let output_image = unsafe {
            let texture = output
                .as_hal::<wgpu::hal::vulkan::Api>()
                .ok_or_else(|| "stage=access_output_texture reason=not_vulkan".to_string())?;
            texture.raw_handle()
        };
        pending.output_view =
            unsafe { create_output_image_view(&self.device, output_image, output_format.vk())? };
        pending.framebuffer = unsafe {
            create_output_framebuffer(
                &self.device,
                cached.render_pass,
                pending.output_view,
                output_width,
                output_height,
            )?
        };
        let (descriptor_pool, descriptor_set) = unsafe {
            self.allocate_and_write_descriptor_set(
                cached.descriptor_set_layout,
                pending.imported_view,
                properties.external_format,
            )?
        };
        pending.descriptor_pool = descriptor_pool;

        let crop_transform = [
            crop.left as f32 / buffer_width as f32,
            crop.top as f32 / buffer_height as f32,
            visible_width as f32 / buffer_width as f32,
            visible_height as f32 / buffer_height as f32,
        ];
        let record_result = unsafe {
            conversion_encoder.as_hal_mut::<wgpu::hal::vulkan::Api, _, _>(|hal_encoder| {
                let hal_encoder = hal_encoder.ok_or_else(|| {
                    "stage=record_android_ahb_conversion reason=not_vulkan".to_string()
                })?;
                let command_buffer = hal_encoder.raw_handle();
                if command_buffer.is_null() {
                    return Err(
                        "stage=record_android_ahb_conversion reason=no_active_command_buffer"
                            .to_string(),
                    );
                }
                record_conversion_commands(
                    &self.device,
                    command_buffer,
                    self.queue_family_index,
                    pending.imported_image,
                    cached.render_pass,
                    pending.framebuffer,
                    cached.pipeline,
                    cached.pipeline_layout,
                    descriptor_set,
                    output_width,
                    output_height,
                    crop_transform,
                );
                Ok(())
            })
        };
        record_result?;

        Ok(AndroidAhbConversion {
            texture: output,
            width: output_width,
            height: output_height,
            output_format,
            pending,
        })
    }

    fn validate_wgpu_device(&self, device: &wgpu::Device) -> Result<(), String> {
        let hal_device = unsafe { device.as_hal::<wgpu::hal::vulkan::Api>() }
            .ok_or_else(|| "stage=validate_android_vulkan_device reason=not_vulkan".to_string())?;
        if hal_device.raw_device().handle() != self.device.handle()
            || hal_device.raw_physical_device() != self.physical_device
            || hal_device.queue_family_index() != self.queue_family_index
        {
            return Err(
                "stage=validate_android_vulkan_device reason=interop_device_mismatch".to_string(),
            );
        }
        Ok(())
    }

    fn reserve_pending_conversion(
        &self,
    ) -> Result<PendingConversionReservation, AndroidAhbConversionError> {
        let mut current = self.pending_conversions.load(Ordering::Acquire);
        loop {
            if current >= MAX_PENDING_AHB_CONVERSIONS {
                return Err(AndroidAhbConversionError::Backpressure {
                    pending: current,
                    max: MAX_PENDING_AHB_CONVERSIONS,
                });
            }
            match self.pending_conversions.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(PendingConversionReservation {
                        counter: Arc::clone(&self.pending_conversions),
                        active: true,
                    });
                }
                Err(observed) => current = observed,
            }
        }
    }

    unsafe fn allocate_and_write_descriptor_set(
        &self,
        descriptor_set_layout: vk::DescriptorSetLayout,
        imported_view: vk::ImageView,
        external_format: u64,
    ) -> Result<(vk::DescriptorPool, vk::DescriptorSet), String> {
        let limit = self.descriptor_budget_limit.max(1);
        let mut budget = self
            .descriptor_budget_hint
            .load(Ordering::Acquire)
            .clamp(1, limit);

        loop {
            let descriptor_pool = unsafe { create_descriptor_pool(&self.device, budget) }.map_err(
                |error| {
                    format!(
                        "stage=create_ahb_descriptor_pool reason=vulkan_error code={error:?} budget={budget} limit={limit} source={} externalFormat={external_format}",
                        self.descriptor_budget_source
                    )
                },
            )?;
            match unsafe {
                allocate_descriptor_set(&self.device, descriptor_pool, descriptor_set_layout)
            } {
                Ok(descriptor_set) => {
                    unsafe {
                        write_descriptor_set(&self.device, descriptor_set, imported_view);
                    }
                    let previous = self
                        .descriptor_budget_hint
                        .fetch_max(budget, Ordering::AcqRel);
                    if budget > previous {
                        crate::trace::diagnostic(
                            serde_json::json!({
                                "event": "android_ahb_descriptor_budget",
                                "stage": "active",
                                "budget": budget,
                                "previousBudget": previous,
                                "limit": limit,
                                "source": self.descriptor_budget_source,
                                "externalFormat": external_format,
                            })
                            .to_string(),
                        );
                    }
                    return Ok((descriptor_pool, descriptor_set));
                }
                Err(vk::Result::ERROR_OUT_OF_POOL_MEMORY) if budget < limit => {
                    unsafe {
                        self.device.destroy_descriptor_pool(descriptor_pool, None);
                    }
                    let next = next_descriptor_budget(budget, limit);
                    crate::trace::diagnostic(
                        serde_json::json!({
                            "event": "android_ahb_descriptor_budget",
                            "stage": "retry",
                            "failedBudget": budget,
                            "nextBudget": next,
                            "limit": limit,
                            "source": self.descriptor_budget_source,
                            "externalFormat": external_format,
                            "reason": "VK_ERROR_OUT_OF_POOL_MEMORY",
                        })
                        .to_string(),
                    );
                    budget = next;
                }
                Err(error) => {
                    unsafe {
                        self.device.destroy_descriptor_pool(descriptor_pool, None);
                    }
                    return Err(format!(
                        "stage=allocate_ahb_descriptor_set reason=vulkan_error code={error:?} budget={budget} limit={limit} source={} externalFormat={external_format}",
                        self.descriptor_budget_source
                    ));
                }
            }
        }
    }

    unsafe fn cached_conversion(
        &self,
        wgpu_device: &wgpu::Device,
        properties: &AhbProperties,
        parameters: ConversionParameters,
        output_format: AndroidAhbIntermediateFormat,
    ) -> Result<Arc<CachedAndroidAhbConversion>, String> {
        let key = ConversionCacheKey::new(properties, parameters, output_format);
        let mut cache = self
            .conversion_cache
            .lock()
            .map_err(|_| "stage=lock_ahb_conversion_cache reason=mutex_poisoned".to_string())?;
        if let Some(cached) = cache.get(&key) {
            return Ok(Arc::clone(cached));
        }

        let api_version = unsafe {
            self.instance
                .get_physical_device_properties(self.physical_device)
                .api_version
        };
        let mut cached =
            CachedAndroidAhbConversion::empty(self, wgpu_device, api_version < vk::API_VERSION_1_1);
        cached.ycbcr_conversion =
            unsafe { self.create_ycbcr_conversion(properties, parameters, cached.ycbcr_via_khr)? };
        cached.sampler = unsafe {
            create_ycbcr_sampler(
                &self.device,
                cached.ycbcr_conversion,
                parameters.chroma_filter,
            )?
        };
        cached.descriptor_set_layout =
            unsafe { create_descriptor_set_layout(&self.device, cached.sampler)? };
        cached.pipeline_layout =
            unsafe { create_pipeline_layout(&self.device, cached.descriptor_set_layout)? };
        cached.render_pass =
            unsafe { create_output_render_pass(&self.device, output_format.vk())? };
        cached.vertex_shader = unsafe {
            create_shader_module(
                &self.device,
                include_bytes!(concat!(env!("OUT_DIR"), "/android_ahb.vert.spv")),
                "create_android_ahb_vertex_shader",
            )?
        };
        cached.fragment_shader = unsafe {
            create_shader_module(
                &self.device,
                include_bytes!(concat!(env!("OUT_DIR"), "/android_ahb.frag.spv")),
                "create_android_ahb_fragment_shader",
            )?
        };
        cached.pipeline = unsafe {
            create_conversion_pipeline(
                &self.device,
                cached.render_pass,
                cached.pipeline_layout,
                cached.vertex_shader,
                cached.fragment_shader,
            )?
        };

        let cached = Arc::new(cached);
        cache.insert(key, Arc::clone(&cached));
        Ok(cached)
    }

    unsafe fn query_ahb_properties(
        &self,
        hardware_buffer: NonNull<vk::AHardwareBuffer>,
    ) -> Result<AhbProperties, String> {
        let mut format = vk::AndroidHardwareBufferFormatPropertiesANDROID::default();
        let mut properties =
            vk::AndroidHardwareBufferPropertiesANDROID::default().push_next(&mut format);
        unsafe {
            self.ahardware_buffer
                .get_android_hardware_buffer_properties(
                    hardware_buffer.as_ptr().cast_const(),
                    &mut properties,
                )
                .map_err(|error| vk_stage_error("query_ahb_properties", error))?;
        }
        let allocation_size = properties.allocation_size;
        let memory_type_bits = properties.memory_type_bits;
        Ok(AhbProperties {
            allocation_size,
            memory_type_bits,
            format: format.format,
            external_format: format.external_format,
            format_features: format.format_features,
            components: format.sampler_ycbcr_conversion_components,
            suggested_model: format.suggested_ycbcr_model,
            suggested_range: format.suggested_ycbcr_range,
            suggested_x_chroma_offset: format.suggested_x_chroma_offset,
            suggested_y_chroma_offset: format.suggested_y_chroma_offset,
        })
    }

    unsafe fn create_imported_image(
        &self,
        width: u32,
        height: u32,
        format: vk::Format,
        external_format: u64,
    ) -> Result<vk::Image, String> {
        let mut external_memory = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::ANDROID_HARDWARE_BUFFER_ANDROID);
        let mut external = vk::ExternalFormatANDROID::default().external_format(external_format);
        let mut create_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push_next(&mut external_memory);
        if external_format != 0 {
            create_info = create_info.push_next(&mut external);
        }
        unsafe { self.device.create_image(&create_info, None) }
            .map_err(|error| vk_stage_error("create_imported_ahb_image", error))
    }

    unsafe fn allocate_imported_memory(
        &self,
        image: vk::Image,
        hardware_buffer: NonNull<vk::AHardwareBuffer>,
        allocation_size: vk::DeviceSize,
        memory_type_bits: u32,
    ) -> Result<vk::DeviceMemory, String> {
        let memory_type_index = choose_memory_type(
            unsafe {
                self.instance
                    .get_physical_device_memory_properties(self.physical_device)
            },
            memory_type_bits,
        )?;
        let mut import =
            vk::ImportAndroidHardwareBufferInfoANDROID::default().buffer(hardware_buffer.as_ptr());
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
        let allocate_info = vk::MemoryAllocateInfo::default()
            .allocation_size(allocation_size)
            .memory_type_index(memory_type_index)
            .push_next(&mut dedicated)
            .push_next(&mut import);
        unsafe { self.device.allocate_memory(&allocate_info, None) }
            .map_err(|error| vk_stage_error("allocate_imported_ahb_memory", error))
    }

    unsafe fn create_ycbcr_conversion(
        &self,
        properties: &AhbProperties,
        parameters: ConversionParameters,
        via_khr: bool,
    ) -> Result<vk::SamplerYcbcrConversion, String> {
        let mut external =
            vk::ExternalFormatANDROID::default().external_format(properties.external_format);
        let mut create_info = vk::SamplerYcbcrConversionCreateInfo::default()
            .format(properties.import_format())
            .ycbcr_model(parameters.model)
            .ycbcr_range(parameters.range)
            .components(properties.components)
            .x_chroma_offset(properties.suggested_x_chroma_offset)
            .y_chroma_offset(properties.suggested_y_chroma_offset)
            .chroma_filter(parameters.chroma_filter)
            .force_explicit_reconstruction(false);
        if properties.external_format != 0 {
            create_info = create_info.push_next(&mut external);
        }
        if via_khr {
            unsafe {
                ash::khr::sampler_ycbcr_conversion::Device::new(&self.instance, &self.device)
                    .create_sampler_ycbcr_conversion(&create_info, None)
            }
        } else {
            unsafe {
                self.device
                    .create_sampler_ycbcr_conversion(&create_info, None)
            }
        }
        .map_err(|error| vk_stage_error("create_ahb_ycbcr_conversion", error))
    }
}

#[derive(Clone, Copy)]
struct AhbProperties {
    allocation_size: vk::DeviceSize,
    memory_type_bits: u32,
    format: vk::Format,
    external_format: u64,
    format_features: vk::FormatFeatureFlags,
    components: vk::ComponentMapping,
    suggested_model: vk::SamplerYcbcrModelConversion,
    suggested_range: vk::SamplerYcbcrRange,
    suggested_x_chroma_offset: vk::ChromaLocation,
    suggested_y_chroma_offset: vk::ChromaLocation,
}

impl AhbProperties {
    fn import_format(&self) -> vk::Format {
        if self.external_format != 0 {
            vk::Format::UNDEFINED
        } else {
            self.format
        }
    }
}

#[derive(Clone, Copy)]
struct ConversionParameters {
    model: vk::SamplerYcbcrModelConversion,
    range: vk::SamplerYcbcrRange,
    chroma_filter: vk::Filter,
}

fn validate_frame_description(
    frame: &AndroidAhbFrameDescription,
    device: &wgpu::Device,
) -> Result<(u32, u32, u32, u32), String> {
    if frame.buffer_width == 0 || frame.buffer_height == 0 {
        return Err(format!(
            "stage=validate_ahb_frame reason=zero_buffer_extent width={} height={}",
            frame.buffer_width, frame.buffer_height
        ));
    }
    let descriptor = crate::android::mediacodec::describe_hardware_buffer(
        frame.hardware_buffer.cast::<c_void>(),
    );
    if descriptor.width != frame.buffer_width || descriptor.height != frame.buffer_height {
        return Err(format!(
            "stage=validate_ahb_frame reason=descriptor_extent_mismatch expected={}x{} actual={}x{}",
            frame.buffer_width, frame.buffer_height, descriptor.width, descriptor.height
        ));
    }
    if descriptor.layers != 1 {
        return Err(format!(
            "stage=validate_ahb_frame reason=unsupported_layer_count layers={}",
            descriptor.layers
        ));
    }
    if descriptor.usage & ANDROID_HARDWARE_BUFFER_USAGE_PROTECTED_CONTENT != 0 {
        return Err("stage=validate_ahb_frame reason=protected_content_not_supported".to_string());
    }
    if descriptor.usage & ANDROID_HARDWARE_BUFFER_USAGE_GPU_SAMPLED_IMAGE == 0 {
        return Err(format!(
            "stage=validate_ahb_frame reason=missing_gpu_sampled_usage usage=0x{:x}",
            descriptor.usage
        ));
    }
    let crop = frame.crop;
    if crop.left >= crop.right
        || crop.top >= crop.bottom
        || crop.right > frame.buffer_width
        || crop.bottom > frame.buffer_height
    {
        return Err(format!(
            "stage=validate_ahb_frame reason=invalid_crop crop={},{},{},{} buffer={}x{}",
            crop.left, crop.top, crop.right, crop.bottom, frame.buffer_width, frame.buffer_height
        ));
    }
    let visible_width = crop.right - crop.left;
    let visible_height = crop.bottom - crop.top;
    if frame.output_width == 0 || frame.output_height == 0 {
        return Err(format!(
            "stage=validate_ahb_frame reason=zero_output_extent width={} height={}",
            frame.output_width, frame.output_height
        ));
    }
    if frame.output_width > visible_width || frame.output_height > visible_height {
        return Err(format!(
            "stage=validate_ahb_frame reason=output_extent_upscales_source output={}x{} visible={}x{}",
            frame.output_width, frame.output_height, visible_width, visible_height
        ));
    }
    let max_dimension = device.limits().max_texture_dimension_2d;
    if frame.output_width > max_dimension || frame.output_height > max_dimension {
        return Err(format!(
            "stage=validate_ahb_frame reason=output_extent_exceeds_device_limit output={}x{} max={max_dimension}",
            frame.output_width, frame.output_height
        ));
    }
    Ok((
        visible_width,
        visible_height,
        frame.output_width,
        frame.output_height,
    ))
}

fn validate_ahb_format(properties: &AhbProperties) -> Result<(), String> {
    if properties.allocation_size == 0 {
        return Err("stage=validate_ahb_format reason=zero_allocation_size".to_string());
    }
    if properties.memory_type_bits == 0 {
        return Err("stage=validate_ahb_format reason=no_memory_types".to_string());
    }
    if properties.external_format == 0 {
        return Err(format!(
            "stage=validate_ahb_format reason=missing_external_format vkFormat={}",
            properties.format.as_raw()
        ));
    }
    if !properties
        .format_features
        .contains(vk::FormatFeatureFlags::SAMPLED_IMAGE)
    {
        return Err(format!(
            "stage=validate_ahb_format reason=not_sampleable features=0x{:x}",
            properties.format_features.as_raw()
        ));
    }
    validate_chroma_location(
        "x",
        properties.suggested_x_chroma_offset,
        properties.format_features,
    )?;
    validate_chroma_location(
        "y",
        properties.suggested_y_chroma_offset,
        properties.format_features,
    )?;
    Ok(())
}

fn validate_chroma_location(
    axis: &str,
    location: vk::ChromaLocation,
    features: vk::FormatFeatureFlags,
) -> Result<(), String> {
    let supported = if location == vk::ChromaLocation::COSITED_EVEN {
        features.contains(vk::FormatFeatureFlags::COSITED_CHROMA_SAMPLES)
    } else if location == vk::ChromaLocation::MIDPOINT {
        features.contains(vk::FormatFeatureFlags::MIDPOINT_CHROMA_SAMPLES)
    } else {
        false
    };
    if supported {
        Ok(())
    } else {
        Err(format!(
            "stage=validate_ahb_format reason=unsupported_chroma_offset axis={axis} location={} features=0x{:x}",
            location.as_raw(),
            features.as_raw()
        ))
    }
}

fn conversion_parameters(
    properties: &AhbProperties,
    color_range: ColorRange,
    matrix_coefficients: MatrixCoefficients,
) -> Result<ConversionParameters, String> {
    let model = match matrix_coefficients {
        MatrixCoefficients::Unspecified => properties.suggested_model,
        MatrixCoefficients::Identity => vk::SamplerYcbcrModelConversion::RGB_IDENTITY,
        MatrixCoefficients::Bt601 => vk::SamplerYcbcrModelConversion::YCBCR_601,
        MatrixCoefficients::Bt709 => vk::SamplerYcbcrModelConversion::YCBCR_709,
        MatrixCoefficients::Bt2020NonConstantLuminance => {
            vk::SamplerYcbcrModelConversion::YCBCR_2020
        }
    };
    if ![
        vk::SamplerYcbcrModelConversion::RGB_IDENTITY,
        vk::SamplerYcbcrModelConversion::YCBCR_IDENTITY,
        vk::SamplerYcbcrModelConversion::YCBCR_601,
        vk::SamplerYcbcrModelConversion::YCBCR_709,
        vk::SamplerYcbcrModelConversion::YCBCR_2020,
    ]
    .contains(&model)
    {
        return Err(format!(
            "stage=select_ahb_ycbcr_conversion reason=unsupported_model model={}",
            model.as_raw()
        ));
    }
    let range = match color_range {
        ColorRange::Unspecified => properties.suggested_range,
        ColorRange::Limited => vk::SamplerYcbcrRange::ITU_NARROW,
        ColorRange::Full => vk::SamplerYcbcrRange::ITU_FULL,
    };
    if range != vk::SamplerYcbcrRange::ITU_NARROW && range != vk::SamplerYcbcrRange::ITU_FULL {
        return Err(format!(
            "stage=select_ahb_ycbcr_conversion reason=unsupported_range range={}",
            range.as_raw()
        ));
    }
    let chroma_filter = if properties
        .format_features
        .contains(vk::FormatFeatureFlags::SAMPLED_IMAGE_YCBCR_CONVERSION_LINEAR_FILTER)
    {
        vk::Filter::LINEAR
    } else {
        vk::Filter::NEAREST
    };
    Ok(ConversionParameters {
        model,
        range,
        chroma_filter,
    })
}

fn choose_memory_type(
    properties: vk::PhysicalDeviceMemoryProperties,
    memory_type_bits: u32,
) -> Result<u32, String> {
    let count = properties.memory_type_count.min(32);
    let device_local = (0..count).find(|index| {
        memory_type_bits & (1 << index) != 0
            && properties.memory_types[*index as usize]
                .property_flags
                .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
    });
    device_local
        .or_else(|| (0..count).find(|index| memory_type_bits & (1 << index) != 0))
        .ok_or_else(|| {
            format!(
                "stage=select_ahb_memory_type reason=no_compatible_type bits=0x{memory_type_bits:x} count={count}"
            )
        })
}

unsafe fn create_ycbcr_sampler(
    device: &ash::Device,
    conversion: vk::SamplerYcbcrConversion,
    filter: vk::Filter,
) -> Result<vk::Sampler, String> {
    let mut conversion_info = vk::SamplerYcbcrConversionInfo::default().conversion(conversion);
    let create_info = vk::SamplerCreateInfo::default()
        .mag_filter(filter)
        .min_filter(filter)
        .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
        .mip_lod_bias(0.0)
        .anisotropy_enable(false)
        .compare_enable(false)
        .min_lod(0.0)
        .max_lod(0.0)
        .border_color(vk::BorderColor::FLOAT_TRANSPARENT_BLACK)
        .unnormalized_coordinates(false)
        .push_next(&mut conversion_info);
    unsafe { device.create_sampler(&create_info, None) }
        .map_err(|error| vk_stage_error("create_ahb_ycbcr_sampler", error))
}

unsafe fn create_ycbcr_image_view(
    device: &ash::Device,
    image: vk::Image,
    format: vk::Format,
    conversion: vk::SamplerYcbcrConversion,
) -> Result<vk::ImageView, String> {
    let mut conversion_info = vk::SamplerYcbcrConversionInfo::default().conversion(conversion);
    let create_info = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(format)
        .components(vk::ComponentMapping::default())
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        })
        .push_next(&mut conversion_info);
    unsafe { device.create_image_view(&create_info, None) }
        .map_err(|error| vk_stage_error("create_imported_ahb_view", error))
}

unsafe fn create_descriptor_set_layout(
    device: &ash::Device,
    immutable_sampler: vk::Sampler,
) -> Result<vk::DescriptorSetLayout, String> {
    let immutable_samplers = [immutable_sampler];
    let bindings = [vk::DescriptorSetLayoutBinding::default()
        .binding(0)
        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
        .descriptor_count(1)
        .stage_flags(vk::ShaderStageFlags::FRAGMENT)
        .immutable_samplers(&immutable_samplers)];
    let create_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
    unsafe { device.create_descriptor_set_layout(&create_info, None) }
        .map_err(|error| vk_stage_error("create_ahb_descriptor_set_layout", error))
}

unsafe fn create_pipeline_layout(
    device: &ash::Device,
    descriptor_set_layout: vk::DescriptorSetLayout,
) -> Result<vk::PipelineLayout, String> {
    let layouts = [descriptor_set_layout];
    let push_constants = [vk::PushConstantRange::default()
        .stage_flags(vk::ShaderStageFlags::FRAGMENT)
        .offset(0)
        .size((mem::size_of::<[f32; 4]>()) as u32)];
    let create_info = vk::PipelineLayoutCreateInfo::default()
        .set_layouts(&layouts)
        .push_constant_ranges(&push_constants);
    unsafe { device.create_pipeline_layout(&create_info, None) }
        .map_err(|error| vk_stage_error("create_ahb_pipeline_layout", error))
}

unsafe fn create_output_render_pass(
    device: &ash::Device,
    output_format: vk::Format,
) -> Result<vk::RenderPass, String> {
    let attachments = [vk::AttachmentDescription::default()
        .format(output_format)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::DONT_CARE)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
        .final_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
    let color_attachments = [vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
    let subpasses = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color_attachments)];
    let dependencies = [vk::SubpassDependency::default()
        .src_subpass(vk::SUBPASS_EXTERNAL)
        .dst_subpass(0)
        .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
        .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
        .dependency_flags(vk::DependencyFlags::BY_REGION)];
    let create_info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(&subpasses)
        .dependencies(&dependencies);
    unsafe { device.create_render_pass(&create_info, None) }
        .map_err(|error| vk_stage_error("create_ahb_output_render_pass", error))
}

unsafe fn create_shader_module(
    device: &ash::Device,
    bytes: &[u8],
    stage: &str,
) -> Result<vk::ShaderModule, String> {
    let words = ash::util::read_spv(&mut Cursor::new(bytes))
        .map_err(|error| format!("stage={stage} reason=invalid_spirv message={error}"))?;
    let create_info = vk::ShaderModuleCreateInfo::default().code(&words);
    unsafe { device.create_shader_module(&create_info, None) }
        .map_err(|error| vk_stage_error(stage, error))
}

unsafe fn create_conversion_pipeline(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    pipeline_layout: vk::PipelineLayout,
    vertex_shader: vk::ShaderModule,
    fragment_shader: vk::ShaderModule,
) -> Result<vk::Pipeline, String> {
    let main = CStr::from_bytes_with_nul(b"main\0").expect("main is nul-terminated");
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vertex_shader)
            .name(main),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(fragment_shader)
            .name(main),
    ];
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
        .primitive_restart_enable(false);
    let viewport = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
        .depth_clamp_enable(false)
        .rasterizer_discard_enable(false)
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .depth_bias_enable(false)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1)
        .sample_shading_enable(false);
    let color_attachments = [vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(false)
        .color_write_mask(
            vk::ColorComponentFlags::R
                | vk::ColorComponentFlags::G
                | vk::ColorComponentFlags::B
                | vk::ColorComponentFlags::A,
        )];
    let color_blend =
        vk::PipelineColorBlendStateCreateInfo::default().attachments(&color_attachments);
    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
    let create_info = [vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport)
        .rasterization_state(&rasterization)
        .multisample_state(&multisample)
        .color_blend_state(&color_blend)
        .dynamic_state(&dynamic)
        .layout(pipeline_layout)
        .render_pass(render_pass)
        .subpass(0)];
    match unsafe { device.create_graphics_pipelines(vk::PipelineCache::null(), &create_info, None) }
    {
        Ok(mut pipelines) => Ok(pipelines.remove(0)),
        Err((pipelines, error)) => {
            for pipeline in pipelines {
                unsafe { device.destroy_pipeline(pipeline, None) };
            }
            Err(vk_stage_error("create_ahb_conversion_pipeline", error))
        }
    }
}

fn establish_wgpu_output_state(encoder: &mut wgpu::CommandEncoder, texture: &wgpu::Texture) {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("erika-android-ahb-output-state"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                // Keep this store even though the following native Vulkan pass overwrites the
                // texture. Discard produced black frames on the tested Adreno 830 because the raw
                // conversion pass sits outside wgpu's resource tracking.
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
}

unsafe fn create_output_image_view(
    device: &ash::Device,
    image: vk::Image,
    output_format: vk::Format,
) -> Result<vk::ImageView, String> {
    let create_info = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(output_format)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        });
    unsafe { device.create_image_view(&create_info, None) }
        .map_err(|error| vk_stage_error("create_ahb_output_view", error))
}

unsafe fn create_output_framebuffer(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    output_view: vk::ImageView,
    width: u32,
    height: u32,
) -> Result<vk::Framebuffer, String> {
    let attachments = [output_view];
    let create_info = vk::FramebufferCreateInfo::default()
        .render_pass(render_pass)
        .attachments(&attachments)
        .width(width)
        .height(height)
        .layers(1);
    unsafe { device.create_framebuffer(&create_info, None) }
        .map_err(|error| vk_stage_error("create_ahb_output_framebuffer", error))
}

fn next_descriptor_budget(current: u32, limit: u32) -> u32 {
    current.saturating_mul(2).min(limit).max(1)
}

unsafe fn create_descriptor_pool(
    device: &ash::Device,
    descriptor_count: u32,
) -> Result<vk::DescriptorPool, vk::Result> {
    let pool_sizes = [vk::DescriptorPoolSize::default()
        .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
        .descriptor_count(descriptor_count)];
    let create_info = vk::DescriptorPoolCreateInfo::default()
        .max_sets(1)
        .pool_sizes(&pool_sizes);
    unsafe { device.create_descriptor_pool(&create_info, None) }
}

unsafe fn allocate_descriptor_set(
    device: &ash::Device,
    descriptor_pool: vk::DescriptorPool,
    descriptor_set_layout: vk::DescriptorSetLayout,
) -> Result<vk::DescriptorSet, vk::Result> {
    let layouts = [descriptor_set_layout];
    let allocate_info = vk::DescriptorSetAllocateInfo::default()
        .descriptor_pool(descriptor_pool)
        .set_layouts(&layouts);
    let descriptor_set = unsafe { device.allocate_descriptor_sets(&allocate_info) }?
        .into_iter()
        .next()
        .ok_or(vk::Result::ERROR_UNKNOWN)?;
    Ok(descriptor_set)
}

unsafe fn write_descriptor_set(
    device: &ash::Device,
    descriptor_set: vk::DescriptorSet,
    imported_view: vk::ImageView,
) {
    let image_info = [vk::DescriptorImageInfo::default()
        .image_view(imported_view)
        .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
    let writes = [vk::WriteDescriptorSet::default()
        .dst_set(descriptor_set)
        .dst_binding(0)
        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
        .image_info(&image_info)];
    unsafe { device.update_descriptor_sets(&writes, &[]) };
}

#[allow(clippy::too_many_arguments)]
unsafe fn record_conversion_commands(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    queue_family_index: u32,
    imported_image: vk::Image,
    render_pass: vk::RenderPass,
    framebuffer: vk::Framebuffer,
    pipeline: vk::Pipeline,
    pipeline_layout: vk::PipelineLayout,
    descriptor_set: vk::DescriptorSet,
    width: u32,
    height: u32,
    crop_transform: [f32; 4],
) {
    let subresource_range = vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    };
    let acquire = [vk::ImageMemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::empty())
        .dst_access_mask(vk::AccessFlags::SHADER_READ)
        .old_layout(vk::ImageLayout::GENERAL)
        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
        .dst_queue_family_index(queue_family_index)
        .image(imported_image)
        .subresource_range(subresource_range)];
    unsafe {
        device.cmd_pipeline_barrier(
            command_buffer,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &acquire,
        );
    }

    let begin_info = vk::RenderPassBeginInfo::default()
        .render_pass(render_pass)
        .framebuffer(framebuffer)
        .render_area(vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D { width, height },
        });
    unsafe {
        device.cmd_begin_render_pass(command_buffer, &begin_info, vk::SubpassContents::INLINE);
        device.cmd_set_viewport(
            command_buffer,
            0,
            &[vk::Viewport {
                x: 0.0,
                y: 0.0,
                width: width as f32,
                height: height as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            }],
        );
        device.cmd_set_scissor(
            command_buffer,
            0,
            &[vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: vk::Extent2D { width, height },
            }],
        );
        device.cmd_bind_pipeline(command_buffer, vk::PipelineBindPoint::GRAPHICS, pipeline);
        device.cmd_bind_descriptor_sets(
            command_buffer,
            vk::PipelineBindPoint::GRAPHICS,
            pipeline_layout,
            0,
            &[descriptor_set],
            &[],
        );
        let push_constants = std::slice::from_raw_parts(
            crop_transform.as_ptr().cast::<u8>(),
            mem::size_of_val(&crop_transform),
        );
        device.cmd_push_constants(
            command_buffer,
            pipeline_layout,
            vk::ShaderStageFlags::FRAGMENT,
            0,
            push_constants,
        );
        device.cmd_draw(command_buffer, 3, 1, 0, 0);
        device.cmd_end_render_pass(command_buffer);
    }

    let release = [vk::ImageMemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::SHADER_READ)
        .dst_access_mask(vk::AccessFlags::empty())
        .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        .new_layout(vk::ImageLayout::GENERAL)
        .src_queue_family_index(queue_family_index)
        .dst_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
        .image(imported_image)
        .subresource_range(subresource_range)];
    unsafe {
        device.cmd_pipeline_barrier(
            command_buffer,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &release,
        );
    }
}

fn vk_stage_error(stage: &str, error: vk::Result) -> String {
    format!("stage={stage} reason=vulkan_error code={error:?}")
}

fn android_custom_vulkan_instance_flags(mut flags: wgpu::InstanceFlags) -> wgpu::InstanceFlags {
    // `wgpu::Instance::from_hal` currently reconstructs the wgpu-core
    // instance with `InstanceFlags::default()`, so DISCARD_HAL_LABELS from the
    // HAL descriptor does not reach wgpu-core. If VK_EXT_debug_utils remains
    // enabled, those regenerated labels still call into the vendor object's
    // naming hook. Android Emulator's ranchu driver dereferences a null object
    // there during device creation. Keep validation and indirect validation,
    // but do not enable the debug-utils extension on this custom Vulkan path.
    flags.remove(wgpu::InstanceFlags::DEBUG);
    flags.insert(wgpu::InstanceFlags::DISCARD_HAL_LABELS);
    flags
}

pub(crate) fn create_device() -> Result<AndroidVulkanDeviceContext, String> {
    let requested_instance_flags = wgpu::InstanceFlags::from_build_config().with_env();
    let instance_flags = android_custom_vulkan_instance_flags(requested_instance_flags);
    if requested_instance_flags.contains(wgpu::InstanceFlags::DEBUG) {
        crate::trace::diagnostic(
            serde_json::json!({
                "event": "android_vulkan",
                "stage": "debug_utils_disabled",
                "backendCandidate": "vulkan-ahb",
                "validationEnabled": instance_flags.intersects(
                    wgpu::InstanceFlags::VALIDATION
                        | wgpu::InstanceFlags::GPU_BASED_VALIDATION
                ),
                "indirectValidationEnabled": instance_flags
                    .contains(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL),
                "objectLabelsEnabled": false,
                "reason": "wgpu_from_hal_does_not_preserve_discard_hal_labels",
                "action": "disable_vk_ext_debug_utils_keep_validation",
            })
            .to_string(),
        );
    }
    let hal_descriptor = wgpu::hal::InstanceDescriptor {
        name: "erika-android-vulkan",
        flags: instance_flags,
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        backend_options: wgpu::BackendOptions::default(),
        telemetry: None,
        display: None,
    };
    let hal_instance = unsafe {
        wgpu::hal::vulkan::Instance::init_with_callback(
            &hal_descriptor,
            Some(Box::new(|args| {
                let api_version = args
                    .entry
                    .try_enumerate_instance_version()
                    .ok()
                    .flatten()
                    .unwrap_or(vk::API_VERSION_1_0);
                if api_version >= vk::API_VERSION_1_1 {
                    return;
                }
                let available = args
                    .entry
                    .enumerate_instance_extension_properties(None)
                    .unwrap_or_default();
                for required in [
                    ash::khr::get_physical_device_properties2::NAME,
                    ash::khr::external_memory_capabilities::NAME,
                ] {
                    if available
                        .iter()
                        .any(|extension| extension.extension_name_as_c_str().ok() == Some(required))
                        && !args.extensions.contains(&required)
                    {
                        args.extensions.push(required);
                    }
                }
            })),
        )
    }
    .map_err(|error| format!("custom Vulkan instance creation failed: {error}"))?;

    let instance_api_version = hal_instance.shared_instance().instance_api_version();
    if instance_api_version < vk::API_VERSION_1_1 {
        for required in [
            ash::khr::get_physical_device_properties2::NAME,
            ash::khr::external_memory_capabilities::NAME,
        ] {
            if !hal_instance
                .shared_instance()
                .extensions()
                .contains(&required)
            {
                return Err(format!(
                    "Vulkan 1.0 adapter lacks required instance extension {}",
                    required.to_string_lossy()
                ));
            }
        }
    }

    let mut exposed = unsafe { hal_instance.enumerate_adapters(None) };
    if exposed.is_empty() {
        return Err("custom Vulkan instance found no adapters".to_string());
    }
    exposed.sort_by_key(|adapter| adapter_rank(adapter.info.device_type));
    let exposed = exposed.remove(0);

    let instance = unsafe { wgpu::Instance::from_hal::<wgpu::hal::vulkan::Api>(hal_instance) };
    let adapter = unsafe { instance.create_adapter_from_hal::<wgpu::hal::vulkan::Api>(exposed) };
    let adapter_limits = adapter.limits();
    let supports_16bit_norm = adapter
        .features()
        .contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM);
    let rgb10a2_features = adapter.get_texture_format_features(wgpu::TextureFormat::Rgb10a2Unorm);
    let supports_rgb10a2_intermediate = rgb10a2_features
        .allowed_usages
        .contains(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING)
        && rgb10a2_features
            .flags
            .contains(wgpu::TextureFormatFeatureFlags::FILTERABLE);
    let required_features = if supports_16bit_norm {
        wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
    } else {
        wgpu::Features::empty()
    };
    let required_limits = wgpu::Limits::downlevel_defaults()
        .using_resolution(adapter_limits.clone())
        .using_alignment(adapter_limits);
    let memory_hints = wgpu::MemoryHints::default();

    let hal_adapter = unsafe {
        adapter
            .as_hal::<wgpu::hal::vulkan::Api>()
            .ok_or_else(|| "wgpu Vulkan adapter handle is unavailable".to_string())?
    };
    let physical_device = hal_adapter.raw_physical_device();
    let shared_instance = hal_adapter.shared_instance();
    let raw_instance = shared_instance.raw_instance();
    let physical_properties =
        unsafe { raw_instance.get_physical_device_properties(physical_device) };
    let physical_api_version = physical_properties.api_version;
    let available_extensions =
        unsafe { raw_instance.enumerate_device_extension_properties(physical_device) }
            .map_err(|error| format!("Vulkan device extension enumeration failed: {error:?}"))?;
    let available_extension_names = available_extensions
        .iter()
        .filter_map(|extension| extension.extension_name_as_c_str().ok())
        .collect::<HashSet<_>>();
    let required_extensions = required_device_extensions(physical_api_version);
    let missing_extensions = required_extensions
        .iter()
        .copied()
        .filter(|extension| !available_extension_names.contains(extension))
        .map(|extension| extension.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if !missing_extensions.is_empty() {
        return Err(format!(
            "Vulkan adapter lacks Android shared-frame extensions: {}",
            missing_extensions.join(", ")
        ));
    }
    if !sampler_ycbcr_conversion_supported(shared_instance, physical_device, physical_api_version) {
        return Err("Vulkan samplerYcbcrConversion feature is unavailable".to_string());
    }
    let descriptor_budget_policy = ycbcr_descriptor_budget_policy(
        shared_instance,
        physical_device,
        &physical_properties,
        &available_extension_names,
    );
    crate::trace::diagnostic(
        serde_json::json!({
            "event": "android_ahb_descriptor_budget",
            "stage": "configured",
            "limit": descriptor_budget_policy.limit,
            "source": descriptor_budget_policy.source,
            "maxPerStageDescriptorSamplers": physical_properties.limits.max_per_stage_descriptor_samplers,
            "maxPerStageDescriptorSampledImages": physical_properties.limits.max_per_stage_descriptor_sampled_images,
            "maxDescriptorSetSamplers": physical_properties.limits.max_descriptor_set_samplers,
            "maxDescriptorSetSampledImages": physical_properties.limits.max_descriptor_set_sampled_images,
        })
        .to_string(),
    );

    let mut enable_ycbcr =
        vk::PhysicalDeviceSamplerYcbcrConversionFeatures::default().sampler_ycbcr_conversion(true);
    let callback_extensions = required_extensions.clone();
    let hal_open_device = unsafe {
        hal_adapter.open_with_callback(
            required_features,
            &required_limits,
            &memory_hints,
            Some(Box::new(|args| {
                for extension in callback_extensions {
                    if !args.extensions.contains(&extension) {
                        args.extensions.push(extension);
                    }
                }
                enable_ycbcr.p_next = args.create_info.p_next.cast_mut();
                args.create_info.p_next = std::ptr::from_ref(&enable_ycbcr).cast();
            })),
        )
    }
    .map_err(|error| format!("custom Vulkan device creation failed: {error:?}"))?;
    drop(hal_adapter);

    let descriptor = wgpu::DeviceDescriptor {
        label: Some("erika-wgpu-device"),
        required_features,
        required_limits,
        memory_hints,
        experimental_features: wgpu::ExperimentalFeatures::default(),
        trace: wgpu::Trace::Off,
    };
    let (device, queue) = unsafe {
        adapter.create_device_from_hal::<wgpu::hal::vulkan::Api>(hal_open_device, &descriptor)
    }
    .map_err(|error| format!("wgpu wrapping custom Vulkan device failed: {error}"))?;

    let interop = unsafe {
        let hal_device = device
            .as_hal::<wgpu::hal::vulkan::Api>()
            .ok_or_else(|| "wgpu Vulkan device handle is unavailable".to_string())?;
        let raw_instance = hal_device.shared_instance().raw_instance().clone();
        let raw_device = hal_device.raw_device().clone();
        AndroidVulkanInterop {
            ahardware_buffer: ash::android::external_memory_android_hardware_buffer::Device::new(
                &raw_instance,
                &raw_device,
            ),
            instance: raw_instance,
            device: raw_device,
            physical_device: hal_device.raw_physical_device(),
            queue_family_index: hal_device.queue_family_index(),
            supports_rgb10a2_intermediate,
            conversion_cache: Mutex::new(HashMap::new()),
            pending_conversions: Arc::new(AtomicUsize::new(0)),
            descriptor_budget_limit: descriptor_budget_policy.limit,
            descriptor_budget_source: descriptor_budget_policy.source,
            descriptor_budget_hint: AtomicU32::new(1),
        }
    };

    Ok(AndroidVulkanDeviceContext {
        instance,
        adapter,
        device,
        queue,
        supports_16bit_norm,
        interop,
    })
}

fn adapter_rank(device_type: wgpu::DeviceType) -> u8 {
    match device_type {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::VirtualGpu => 2,
        wgpu::DeviceType::Other => 3,
        wgpu::DeviceType::Cpu => 4,
    }
}

fn required_device_extensions(api_version: u32) -> Vec<&'static CStr> {
    let mut extensions = vec![
        ash::android::external_memory_android_hardware_buffer::NAME,
        ash::ext::queue_family_foreign::NAME,
    ];
    if api_version < vk::API_VERSION_1_1 {
        extensions.extend([
            ash::khr::maintenance1::NAME,
            ash::khr::bind_memory2::NAME,
            ash::khr::get_memory_requirements2::NAME,
            ash::khr::sampler_ycbcr_conversion::NAME,
            ash::khr::external_memory::NAME,
            ash::khr::dedicated_allocation::NAME,
        ]);
    }
    extensions
}

fn sampler_ycbcr_conversion_supported(
    instance: &wgpu::hal::vulkan::InstanceShared,
    physical_device: vk::PhysicalDevice,
    api_version: u32,
) -> bool {
    let mut ycbcr = vk::PhysicalDeviceSamplerYcbcrConversionFeatures::default();
    let mut features = vk::PhysicalDeviceFeatures2::default().push_next(&mut ycbcr);
    unsafe {
        if instance.instance_api_version() >= vk::API_VERSION_1_1
            && api_version >= vk::API_VERSION_1_1
        {
            instance
                .raw_instance()
                .get_physical_device_features2(physical_device, &mut features);
        } else {
            let loader = ash::khr::get_physical_device_properties2::Instance::new(
                instance.entry(),
                instance.raw_instance(),
            );
            loader.get_physical_device_features2(physical_device, &mut features);
        }
    }
    ycbcr.sampler_ycbcr_conversion == vk::TRUE
}

fn ycbcr_descriptor_budget_policy(
    instance: &wgpu::hal::vulkan::InstanceShared,
    physical_device: vk::PhysicalDevice,
    physical_properties: &vk::PhysicalDeviceProperties,
    available_extension_names: &HashSet<&CStr>,
) -> YcbcrDescriptorBudgetPolicy {
    // wgpu-hal 29 creates its Vulkan instance with API 1.3 at most. Its
    // `instance_api_version()` reports the loader version, not the negotiated
    // VkApplicationInfo version, so a 1.4 loader must not be treated as enabling
    // maintenance6 core structures on this instance. Query the KHR property only
    // when the physical device explicitly enumerates the extension; once wgpu
    // negotiates Vulkan 1.4 this gate can grow a genuine core branch.
    let maintenance6_source = if available_extension_names.contains(ash::khr::maintenance6::NAME) {
        Some("VK_KHR_maintenance6")
    } else {
        None
    };

    if let Some(source) = maintenance6_source {
        let mut maintenance6 = vk::PhysicalDeviceMaintenance6PropertiesKHR::default();
        let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut maintenance6);
        unsafe {
            if instance.instance_api_version() >= vk::API_VERSION_1_1 {
                instance
                    .raw_instance()
                    .get_physical_device_properties2(physical_device, &mut properties);
            } else {
                let loader = ash::khr::get_physical_device_properties2::Instance::new(
                    instance.entry(),
                    instance.raw_instance(),
                );
                loader.get_physical_device_properties2(physical_device, &mut properties);
            }
        }
        if maintenance6.max_combined_image_sampler_descriptor_count > 0 {
            return YcbcrDescriptorBudgetPolicy {
                limit: maintenance6.max_combined_image_sampler_descriptor_count,
                source,
            };
        }
        crate::trace::diagnostic(
            serde_json::json!({
                "event": "android_ahb_descriptor_budget",
                "stage": "maintenance6_invalid",
                "source": source,
                "reportedLimit": maintenance6.max_combined_image_sampler_descriptor_count,
                "action": "fall_back_to_descriptor_layout_limits",
            })
            .to_string(),
        );
    }

    let limits = physical_properties.limits;
    let limit = [
        limits.max_per_stage_descriptor_samplers,
        limits.max_per_stage_descriptor_sampled_images,
        limits.max_descriptor_set_samplers,
        limits.max_descriptor_set_sampled_images,
    ]
    .into_iter()
    .min()
    .unwrap_or(1)
    .max(1);
    YcbcrDescriptorBudgetPolicy {
        limit,
        source: "descriptor layout limits",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AndroidAhbIntermediateFormat, android_custom_vulkan_instance_flags, next_descriptor_budget,
    };

    #[test]
    fn ahb_intermediate_formats_match_wgpu_vulkan_layouts() {
        assert_eq!(
            AndroidAhbIntermediateFormat::Rgb10a2Unorm.wgpu(),
            wgpu::TextureFormat::Rgb10a2Unorm
        );
        assert_eq!(
            AndroidAhbIntermediateFormat::Rgb10a2Unorm.vk(),
            ash::vk::Format::A2B10G10R10_UNORM_PACK32
        );
        assert_eq!(
            AndroidAhbIntermediateFormat::Rgb10a2Unorm.bytes_per_pixel(),
            4
        );
        assert_eq!(
            AndroidAhbIntermediateFormat::Rgba16Float.bytes_per_pixel(),
            8
        );
    }

    #[test]
    fn custom_vulkan_instance_disables_debug_utils_but_keeps_validation() {
        let requested = wgpu::InstanceFlags::DEBUG
            | wgpu::InstanceFlags::VALIDATION
            | wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL
            | wgpu::InstanceFlags::ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER;

        let actual = android_custom_vulkan_instance_flags(requested);

        assert!(!actual.contains(wgpu::InstanceFlags::DEBUG));
        assert!(actual.contains(wgpu::InstanceFlags::DISCARD_HAL_LABELS));
        assert!(actual.contains(wgpu::InstanceFlags::VALIDATION));
        assert!(actual.contains(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL));
        assert!(actual.contains(wgpu::InstanceFlags::ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER));
    }

    #[test]
    fn descriptor_budget_growth_is_bounded_and_reaches_non_power_of_two_limit() {
        assert_eq!(next_descriptor_budget(1, 7), 2);
        assert_eq!(next_descriptor_budget(2, 7), 4);
        assert_eq!(next_descriptor_budget(4, 7), 7);
        assert_eq!(next_descriptor_budget(7, 7), 7);
    }
}
