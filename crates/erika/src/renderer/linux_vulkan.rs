//! FFmpeg/Vulkan plane transfer without host pixel staging. CUDA requires a
//! device-local copy; this path deliberately does not claim direct zero-copy.
use crate::ffmpeg::Frame;
use ash::{ext, khr, vk, vk::Handle};
use std::{
    ffi::{CStr, c_char, c_int, c_void},
    ptr::NonNull,
};

type Api = wgpu::hal::api::Vulkan;

unsafe extern "C" {
    fn erika_linux_vk_create(
        instance: u64,
        physical: u64,
        device: u64,
        family: u32,
        features: *const vk::PhysicalDeviceFeatures,
        dev_exts: *const *const c_char,
        dev_count: c_int,
        inst_exts: *const *const c_char,
        inst_count: c_int,
        error: *mut c_int,
    ) -> *mut c_void;
    fn erika_linux_vk_destroy(state: *mut c_void);
    fn erika_linux_vk_error(state: *mut c_void) -> *const c_char;
    fn erika_linux_vk_format(frame: *const erika_ffmpeg_sys::AVFrame) -> c_int;
    fn erika_linux_vk_copy(
        state: *mut c_void,
        frame: *const erika_ffmpeg_sys::AVFrame,
        luma: u64,
        chroma: u64,
    ) -> c_int;
}

/// Returns None when the adapter cannot safely share its device with FFmpeg.
pub(crate) fn request_device(
    adapter: &wgpu::Adapter,
    desc: &wgpu::DeviceDescriptor<'_>,
) -> Option<Result<(wgpu::Device, wgpu::Queue), String>> {
    // SAFETY: all borrowed HAL handles stay alive until device creation returns.
    let hal = unsafe { adapter.as_hal::<Api>() }?;
    let instance = hal.shared_instance().raw_instance();
    let physical = hal.raw_physical_device();
    let properties = unsafe { instance.get_physical_device_properties(physical) };
    if properties.api_version < vk::API_VERSION_1_2 {
        return None;
    }
    let mut sync2 = vk::PhysicalDeviceSynchronization2Features::default();
    let mut timeline = vk::PhysicalDeviceTimelineSemaphoreFeatures::default();
    let mut features = vk::PhysicalDeviceFeatures2::default()
        .push_next(&mut sync2)
        .push_next(&mut timeline);
    unsafe { instance.get_physical_device_features2(physical, &mut features) };
    if sync2.synchronization2 == 0 || timeline.timeline_semaphore == 0 {
        return None;
    }
    let caps = hal.physical_device_capabilities();
    // Requiring these advertised extensions also provides a reliable marker
    // that our callback enabled the features expected by the C bridge.
    let required = [
        khr::external_memory_fd::NAME,
        khr::external_semaphore_fd::NAME,
        khr::synchronization2::NAME,
        khr::timeline_semaphore::NAME,
    ];
    if required.iter().any(|name| !caps.supports_extension(name)) {
        return None;
    }
    let mut extensions = required.to_vec();
    for name in [
        ext::external_memory_dma_buf::NAME,
        ext::image_drm_format_modifier::NAME,
    ] {
        if caps.supports_extension(name) {
            extensions.push(name);
        }
    }
    let mut enable_sync2 =
        vk::PhysicalDeviceSynchronization2Features::default().synchronization2(true);
    // HAL already enables timeline semaphores for Vulkan 1.2+; adding a second
    // feature struct would violate Vulkan's pNext uniqueness requirement.
    let opened = unsafe {
        hal.open_with_callback(
            desc.required_features,
            &desc.required_limits,
            &desc.memory_hints,
            Some(Box::new(|args| {
                for name in extensions {
                    if !args.extensions.contains(&name) {
                        args.extensions.push(name);
                    }
                }
                enable_sync2.p_next = args.create_info.p_next.cast_mut();
                args.create_info.p_next =
                    (&enable_sync2 as *const vk::PhysicalDeviceSynchronization2Features<'_>).cast();
            })),
        )
    };
    Some(
        opened
            .map_err(|e| format!("Vulkan interop device: {e}"))
            .and_then(|opened| {
                unsafe { adapter.create_device_from_hal(opened, desc) }.map_err(|e| e.to_string())
            }),
    )
}

pub(crate) struct LinuxVulkanInterop {
    state: NonNull<c_void>,
    // Drop the FFmpeg wrapper before the VkDevice it borrows.
    device: wgpu::Device,
    renderable_planes: [bool; 2],
}

// Every access requires &mut self; the renderer serializes queue submissions.
unsafe impl Send for LinuxVulkanInterop {}

impl Drop for LinuxVulkanInterop {
    fn drop(&mut self) {
        unsafe { erika_linux_vk_destroy(self.state.as_ptr()) };
    }
}

impl LinuxVulkanInterop {
    pub(crate) fn new(adapter: &wgpu::Adapter, device: &wgpu::Device) -> Result<Self, String> {
        let hal = unsafe { device.as_hal::<Api>() }
            .ok_or("GPU frame sharing requires a Vulkan renderer")?;
        let extensions = hal.enabled_device_extensions();
        if !extensions.contains(&khr::synchronization2::NAME)
            || !extensions.contains(&khr::external_semaphore_fd::NAME)
        {
            return Err(
                "Vulkan device lacks enabled synchronization2/external semaphore support".into(),
            );
        }
        let adapter_hal = unsafe { adapter.as_hal::<Api>() }.ok_or("Vulkan adapter unavailable")?;
        let features = adapter_hal
            .physical_device_features(extensions, device.features())
            .get_core();
        let dev_exts: Vec<_> = extensions.iter().map(|e| e.as_ptr()).collect();
        let inst_exts: Vec<_> = hal
            .shared_instance()
            .extensions()
            .iter()
            .map(|e| e.as_ptr())
            .collect();
        let mut error = 0;
        let state = unsafe {
            erika_linux_vk_create(
                hal.shared_instance().raw_instance().handle().as_raw(),
                hal.raw_physical_device().as_raw(),
                hal.raw_device().handle().as_raw(),
                hal.queue_family_index(),
                &features,
                dev_exts.as_ptr(),
                dev_exts.len() as c_int,
                inst_exts.as_ptr(),
                inst_exts.len() as c_int,
                &mut error,
            )
        };
        Ok(Self {
            state: NonNull::new(state)
                .ok_or_else(|| format!("FFmpeg Vulkan device initialization failed ({error})"))?,
            device: device.clone(),
            renderable_planes: [
                [wgpu::TextureFormat::R8Unorm, wgpu::TextureFormat::Rg8Unorm],
                [
                    wgpu::TextureFormat::R16Unorm,
                    wgpu::TextureFormat::Rg16Unorm,
                ],
            ]
            .map(|formats| {
                formats.into_iter().all(|format| {
                    let capabilities = if device
                        .features()
                        .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
                    {
                        adapter.get_texture_format_features(format)
                    } else {
                        format.guaranteed_format_features(device.features())
                    };
                    capabilities.allowed_usages.contains(
                        wgpu::TextureUsages::TEXTURE_BINDING
                            | wgpu::TextureUsages::COPY_DST
                            | wgpu::TextureUsages::RENDER_ATTACHMENT,
                    )
                })
            }),
        })
    }

    pub(crate) fn copy_planes(
        &mut self,
        queue: &wgpu::Queue,
        frame: &Frame,
    ) -> Result<(wgpu::Texture, wgpu::Texture, bool), String> {
        let depth = unsafe { erika_linux_vk_format(frame.as_ptr()) };
        if !self.renderable_planes[usize::from(depth == 10)] {
            return Err("Vulkan device cannot render/copy/sample the decoded plane formats".into());
        }
        let formats = match depth {
            8 => [wgpu::TextureFormat::R8Unorm, wgpu::TextureFormat::Rg8Unorm],
            10 if self
                .device
                .features()
                .contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM) =>
            {
                [
                    wgpu::TextureFormat::R16Unorm,
                    wgpu::TextureFormat::Rg16Unorm,
                ]
            }
            _ => return Err("GPU import requires NV12 or supported P010 textures".into()),
        };
        let (width, height) = (frame.width(), frame.height());
        if width == 0
            || height == 0
            || width > self.device.limits().max_texture_dimension_2d
            || height > self.device.limits().max_texture_dimension_2d
        {
            return Err("Invalid hardware frame dimensions".into());
        }
        let textures: Vec<_> = formats
            .into_iter()
            .enumerate()
            .map(|(i, format)| {
                self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("erika-linux-gpu-plane"),
                    size: wgpu::Extent3d {
                        width: if i == 0 { width } else { width.div_ceil(2) },
                        height: if i == 0 { height } else { height.div_ceil(2) },
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_DST
                        | wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
            })
            .collect();
        // Establish layouts known to both wgpu's tracker and the external copy.
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for texture in &textures {
            let view = texture.create_view(&Default::default());
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("erika-linux-plane-layout"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        queue.submit([encoder.finish()]);
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(5)),
            })
            .map_err(|e| e.to_string())?;
        let luma = unsafe { textures[0].as_hal::<Api>() }.ok_or("Vulkan luma unavailable")?;
        let chroma = unsafe { textures[1].as_hal::<Api>() }.ok_or("Vulkan chroma unavailable")?;
        let result = unsafe {
            erika_linux_vk_copy(
                self.state.as_ptr(),
                frame.as_ptr(),
                luma.raw_handle().as_raw(),
                chroma.raw_handle().as_raw(),
            )
        };
        if result < 0 {
            return Err(
                unsafe { CStr::from_ptr(erika_linux_vk_error(self.state.as_ptr())) }
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        drop((luma, chroma));
        let mut textures = textures.into_iter();
        Ok((
            textures.next().unwrap(),
            textures.next().unwrap(),
            depth == 10,
        ))
    }
}
