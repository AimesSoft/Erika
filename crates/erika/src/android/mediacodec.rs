use std::ffi::c_void;
use std::io;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use thiserror::Error;

const AIMAGE_FORMAT_PRIVATE: i32 = 0x22;
const AHARDWAREBUFFER_USAGE_GPU_SAMPLED_IMAGE: u64 = 1 << 8;
const AHARDWAREBUFFER_USAGE_PROTECTED_CONTENT: u64 = 1 << 14;
const MAX_ACQUIRED_IMAGES: i32 = 8;
// Surface-backed decoders can take several vsyncs to publish their first
// ImageReader buffer while the codec and gralloc queues warm up. This remains
// a bounded one-frame probe: a broken Surface route must still step down to
// MediaCodec byte-buffer mode instead of stalling playback indefinitely.
const IMAGE_ACQUIRE_TIMEOUT: Duration = Duration::from_millis(500);
const SLOW_IMAGE_ACQUIRE_LOG_THRESHOLD: Duration = Duration::from_millis(5);
const AMEDIA_OK: i32 = 0;
const AMEDIA_IMGREADER_NO_BUFFER_AVAILABLE: i32 = -30_001;
const AMEDIA_IMGREADER_MAX_IMAGES_ACQUIRED: i32 = -30_002;

#[repr(C)]
struct AImageReader {
    _private: [u8; 0],
}

#[repr(C)]
struct AImage {
    _private: [u8; 0],
}

#[repr(C)]
struct AHardwareBuffer {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AndroidImageCrop {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AndroidHardwareBufferDescription {
    pub width: u32,
    pub height: u32,
    pub layers: u32,
    pub format: u32,
    pub usage: u64,
    pub stride: u32,
    pub reserved0: u32,
    pub reserved1: u64,
}

#[derive(Debug, Error)]
pub(crate) enum AndroidMediaCodecError {
    #[error("{operation} failed with Android media status {status}")]
    MediaStatus {
        operation: &'static str,
        status: i32,
    },
    #[error("{operation} returned a null Android object")]
    NullObject { operation: &'static str },
    #[error("MediaCodec Surface delivery lock was poisoned")]
    DeliveryLockPoisoned,
    #[error(
        "timed out after {elapsed_ms} ms waiting for the next MediaCodec ImageReader buffer ({no_buffer_polls} no-buffer polls)"
    )]
    ImageTimeout {
        elapsed_ms: u64,
        no_buffer_polls: u64,
    },
    #[error(
        "MediaCodec ImageReader acquisition is temporarily backpressured because the {max_images}-image client limit is already acquired"
    )]
    ImageBackpressure { max_images: i32 },
    #[error(
        "timed out after {elapsed_ms} ms waiting for MediaCodec ImageReader capacity ({max_images_polls} max-images polls, limit {max_images})"
    )]
    ImageBackpressureTimeout {
        elapsed_ms: u64,
        max_images_polls: u64,
        max_images: i32,
    },
    #[error("MediaCodec acquire fence {fd} wait failed: {reason}")]
    AcquireFence { fd: i32, reason: String },
    #[error(
        "invalid MediaCodec image geometry: image={image_width}x{image_height}, buffer={buffer_width}x{buffer_height}, crop=({left},{top})-({right},{bottom}), layers={layers}"
    )]
    InvalidGeometry {
        image_width: i32,
        image_height: i32,
        buffer_width: u32,
        buffer_height: u32,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        layers: u32,
    },
    #[error("protected MediaCodec AHardwareBuffer content cannot be imported")]
    ProtectedContent,
}

pub(crate) struct AndroidMediaCodecFrameSource {
    reader: NonNull<AImageReader>,
    native_window: NonNull<c_void>,
    delivery_lock: Mutex<()>,
    acquired_image_count: AtomicU64,
    live_image_count: AtomicU64,
    width: u32,
    height: u32,
}

unsafe impl Send for AndroidMediaCodecFrameSource {}
unsafe impl Sync for AndroidMediaCodecFrameSource {}

impl AndroidMediaCodecFrameSource {
    pub(crate) fn new(width: u32, height: u32) -> Result<Self, AndroidMediaCodecError> {
        if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
            return Err(AndroidMediaCodecError::InvalidGeometry {
                image_width: width.min(i32::MAX as u32) as i32,
                image_height: height.min(i32::MAX as u32) as i32,
                buffer_width: width,
                buffer_height: height,
                left: 0,
                top: 0,
                right: width.min(i32::MAX as u32) as i32,
                bottom: height.min(i32::MAX as u32) as i32,
                layers: 1,
            });
        }
        let mut reader = ptr::null_mut();
        check_media_status(
            unsafe {
                AImageReader_newWithUsage(
                    width as i32,
                    height as i32,
                    AIMAGE_FORMAT_PRIVATE,
                    AHARDWAREBUFFER_USAGE_GPU_SAMPLED_IMAGE,
                    MAX_ACQUIRED_IMAGES,
                    &mut reader,
                )
            },
            "AImageReader_newWithUsage",
        )?;
        let reader = NonNull::new(reader).ok_or(AndroidMediaCodecError::NullObject {
            operation: "AImageReader_newWithUsage",
        })?;
        let mut native_window = ptr::null_mut();
        if let Err(error) = check_media_status(
            unsafe { AImageReader_getWindow(reader.as_ptr(), &mut native_window) },
            "AImageReader_getWindow",
        ) {
            unsafe { AImageReader_delete(reader.as_ptr()) };
            return Err(error);
        }
        let Some(native_window) = NonNull::new(native_window.cast::<c_void>()) else {
            unsafe { AImageReader_delete(reader.as_ptr()) };
            return Err(AndroidMediaCodecError::NullObject {
                operation: "AImageReader_getWindow",
            });
        };
        crate::trace::diagnostic(
            serde_json::json!({
                "event": "android_mediacodec_surface",
                "stage": "image_reader_created",
                "width": width,
                "height": height,
                "format": "PRIVATE",
                "usage": "GPU_SAMPLED_IMAGE",
                "maxImages": MAX_ACQUIRED_IMAGES,
            })
            .to_string(),
        );
        Ok(Self {
            reader,
            native_window,
            delivery_lock: Mutex::new(()),
            acquired_image_count: AtomicU64::new(0),
            live_image_count: AtomicU64::new(0),
            width,
            height,
        })
    }

    pub(crate) fn native_window(&self) -> NonNull<c_void> {
        self.native_window
    }

    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    pub(crate) fn lock_delivery(&self) -> Result<MutexGuard<'_, ()>, AndroidMediaCodecError> {
        self.delivery_lock
            .lock()
            .map_err(|_| AndroidMediaCodecError::DeliveryLockPoisoned)
    }

    pub(crate) fn ensure_image_capacity(
        &self,
        expected_media_timestamp_ns: Option<i64>,
    ) -> Result<(), AndroidMediaCodecError> {
        let live_images = self.live_image_count.load(Ordering::Acquire);
        if live_images < MAX_ACQUIRED_IMAGES as u64 {
            return Ok(());
        }
        crate::trace::diagnostic(
            serde_json::json!({
                "event": "android_mediacodec_surface",
                "stage": "image_capacity_backpressure",
                "renderMode": "immediate",
                "expectedMediaTimestampNs": expected_media_timestamp_ns,
                "liveImages": live_images,
                "maxImages": MAX_ACQUIRED_IMAGES,
                "action": "retry_before_releasing_codec_buffer",
            })
            .to_string(),
        );
        Err(AndroidMediaCodecError::ImageBackpressure {
            max_images: MAX_ACQUIRED_IMAGES,
        })
    }

    pub(crate) fn acquire_next_rendered_image(
        self: &std::sync::Arc<Self>,
        expected_media_timestamp_ns: Option<i64>,
    ) -> Result<std::sync::Arc<AndroidHardwareBufferImage>, AndroidMediaCodecError> {
        let started = Instant::now();
        let deadline = started + IMAGE_ACQUIRE_TIMEOUT;
        let mut no_buffer_polls = 0u64;
        let mut max_images_polls = 0u64;
        loop {
            let mut image = ptr::null_mut();
            let mut acquire_fence_fd = -1;
            let status = unsafe {
                AImageReader_acquireNextImageAsync(
                    self.reader.as_ptr(),
                    &mut image,
                    &mut acquire_fence_fd,
                )
            };
            if status == AMEDIA_IMGREADER_NO_BUFFER_AVAILABLE {
                no_buffer_polls = no_buffer_polls.saturating_add(1);
                if !image.is_null() {
                    unsafe { AImage_delete(image) };
                }
                if acquire_fence_fd >= 0 {
                    unsafe { libc::close(acquire_fence_fd) };
                }
                if Instant::now() >= deadline {
                    let elapsed_ms = duration_millis_u64(started.elapsed());
                    crate::trace::diagnostic(
                        serde_json::json!({
                            "event": "android_mediacodec_surface",
                            "stage": "image_acquire_timeout",
                            "renderMode": "immediate",
                            "association": "release_then_acquire_next",
                            "expectedMediaTimestampNs": expected_media_timestamp_ns,
                            "elapsedMs": elapsed_ms,
                            "noBufferPolls": no_buffer_polls,
                            "maxImagesPolls": max_images_polls,
                        })
                        .to_string(),
                    );
                    return Err(AndroidMediaCodecError::ImageTimeout {
                        elapsed_ms,
                        no_buffer_polls,
                    });
                }
                std::thread::sleep(Duration::from_micros(250));
                continue;
            }
            if status == AMEDIA_IMGREADER_MAX_IMAGES_ACQUIRED {
                max_images_polls = max_images_polls.saturating_add(1);
                if !image.is_null() {
                    unsafe { AImage_delete(image) };
                }
                if acquire_fence_fd >= 0 {
                    unsafe { libc::close(acquire_fence_fd) };
                }
                if Instant::now() >= deadline {
                    let elapsed_ms = duration_millis_u64(started.elapsed());
                    crate::trace::diagnostic(
                        serde_json::json!({
                            "event": "android_mediacodec_surface",
                            "stage": "image_acquire_backpressure_timeout",
                            "renderMode": "immediate",
                            "association": "release_then_acquire_next",
                            "expectedMediaTimestampNs": expected_media_timestamp_ns,
                            "elapsedMs": elapsed_ms,
                            "status": status,
                            "maxImagesPolls": max_images_polls,
                            "maxImages": MAX_ACQUIRED_IMAGES,
                            "action": "surface_route_failure_after_bounded_transient_retries",
                        })
                        .to_string(),
                    );
                    return Err(AndroidMediaCodecError::ImageBackpressureTimeout {
                        elapsed_ms,
                        max_images_polls,
                        max_images: MAX_ACQUIRED_IMAGES,
                    });
                }
                std::thread::sleep(Duration::from_micros(250));
                continue;
            }
            if status != AMEDIA_OK {
                if !image.is_null() {
                    unsafe { AImage_delete(image) };
                }
                if acquire_fence_fd >= 0 {
                    unsafe { libc::close(acquire_fence_fd) };
                }
                return Err(AndroidMediaCodecError::MediaStatus {
                    operation: "AImageReader_acquireNextImageAsync",
                    status,
                });
            }
            let Some(image) = NonNull::new(image) else {
                if acquire_fence_fd >= 0 {
                    unsafe { libc::close(acquire_fence_fd) };
                }
                return Err(AndroidMediaCodecError::NullObject {
                    operation: "AImageReader_acquireNextImageAsync",
                });
            };
            if let Err(error) = wait_and_close_acquire_fence(acquire_fence_fd, deadline) {
                unsafe { AImage_delete(image.as_ptr()) };
                return Err(error);
            }

            let mut timestamp_ns = 0i64;
            if let Err(error) = check_media_status(
                unsafe { AImage_getTimestamp(image.as_ptr(), &mut timestamp_ns) },
                "AImage_getTimestamp",
            ) {
                unsafe { AImage_delete(image.as_ptr()) };
                return Err(error);
            }

            // Only one codec output buffer is released before this acquire, so
            // queue order is the authoritative association. ImageReader's
            // timestamp is media presentation time on many codecs, not the
            // CLOCK_MONOTONIC render deadline accepted by
            // releaseOutputBufferAtTime; rejecting it against a synthetic
            // render timestamp discards valid frames.
            let elapsed = started.elapsed();
            let image_sequence = self
                .acquired_image_count
                .fetch_add(1, Ordering::AcqRel)
                .saturating_add(1);
            if image_sequence == 1 || elapsed >= SLOW_IMAGE_ACQUIRE_LOG_THRESHOLD {
                crate::trace::diagnostic(
                    serde_json::json!({
                        "event": "android_mediacodec_surface",
                        "stage": if image_sequence == 1 {
                            "first_image_acquired"
                        } else {
                            "slow_image_acquired"
                        },
                        "renderMode": "immediate",
                        "association": "release_then_acquire_next",
                        "imageSequence": image_sequence,
                        "expectedMediaTimestampNs": expected_media_timestamp_ns,
                        "actualImageTimestampNs": timestamp_ns,
                        "timestampDeltaNs": expected_media_timestamp_ns
                            .map(|expected| timestamp_ns.saturating_sub(expected)),
                        "elapsedUs": duration_micros_u64(elapsed),
                        "noBufferPolls": no_buffer_polls,
                        "maxImagesPolls": max_images_polls,
                    })
                    .to_string(),
                );
            }

            return AndroidHardwareBufferImage::from_raw(self.clone(), image, timestamp_ns)
                .map(std::sync::Arc::new);
        }
    }
}

impl Drop for AndroidMediaCodecFrameSource {
    fn drop(&mut self) {
        unsafe { AImageReader_delete(self.reader.as_ptr()) };
    }
}

pub(crate) struct AndroidHardwareBufferImage {
    image: NonNull<AImage>,
    hardware_buffer: NonNull<c_void>,
    crop: AndroidImageCrop,
    description: AndroidHardwareBufferDescription,
    timestamp_ns: i64,
    _source: std::sync::Arc<AndroidMediaCodecFrameSource>,
}

unsafe impl Send for AndroidHardwareBufferImage {}
unsafe impl Sync for AndroidHardwareBufferImage {}

impl AndroidHardwareBufferImage {
    fn from_raw(
        source: std::sync::Arc<AndroidMediaCodecFrameSource>,
        image: NonNull<AImage>,
        timestamp_ns: i64,
    ) -> Result<Self, AndroidMediaCodecError> {
        let mut image_width = 0i32;
        let mut image_height = 0i32;
        let mut crop = AndroidImageCrop::default();
        let mut hardware_buffer = ptr::null_mut();
        for (status, operation) in [
            (
                unsafe { AImage_getWidth(image.as_ptr(), &mut image_width) },
                "AImage_getWidth",
            ),
            (
                unsafe { AImage_getHeight(image.as_ptr(), &mut image_height) },
                "AImage_getHeight",
            ),
            (
                unsafe { AImage_getCropRect(image.as_ptr(), &mut crop) },
                "AImage_getCropRect",
            ),
            (
                unsafe { AImage_getHardwareBuffer(image.as_ptr(), &mut hardware_buffer) },
                "AImage_getHardwareBuffer",
            ),
        ] {
            if let Err(error) = check_media_status(status, operation) {
                unsafe { AImage_delete(image.as_ptr()) };
                return Err(error);
            }
        }
        let Some(hardware_buffer) = NonNull::new(hardware_buffer.cast::<c_void>()) else {
            unsafe { AImage_delete(image.as_ptr()) };
            return Err(AndroidMediaCodecError::NullObject {
                operation: "AImage_getHardwareBuffer",
            });
        };
        let description = describe_hardware_buffer(hardware_buffer);
        let geometry_valid = image_width > 0
            && image_height > 0
            && description.width > 0
            && description.height > 0
            && description.layers == 1
            && crop.left >= 0
            && crop.top >= 0
            && crop.right > crop.left
            && crop.bottom > crop.top
            && crop.right <= description.width as i32
            && crop.bottom <= description.height as i32;
        if !geometry_valid {
            unsafe { AImage_delete(image.as_ptr()) };
            return Err(AndroidMediaCodecError::InvalidGeometry {
                image_width,
                image_height,
                buffer_width: description.width,
                buffer_height: description.height,
                left: crop.left,
                top: crop.top,
                right: crop.right,
                bottom: crop.bottom,
                layers: description.layers,
            });
        }
        if description.usage & AHARDWAREBUFFER_USAGE_PROTECTED_CONTENT != 0 {
            unsafe { AImage_delete(image.as_ptr()) };
            return Err(AndroidMediaCodecError::ProtectedContent);
        }
        source.live_image_count.fetch_add(1, Ordering::AcqRel);
        Ok(Self {
            image,
            hardware_buffer,
            crop,
            description,
            timestamp_ns,
            _source: source,
        })
    }

    pub(crate) fn hardware_buffer(&self) -> NonNull<c_void> {
        self.hardware_buffer
    }

    pub(crate) fn crop(&self) -> AndroidImageCrop {
        self.crop
    }

    pub(crate) fn description(&self) -> AndroidHardwareBufferDescription {
        self.description
    }

    pub(crate) fn timestamp_ns(&self) -> i64 {
        self.timestamp_ns
    }
}

pub(crate) fn describe_hardware_buffer(
    hardware_buffer: NonNull<c_void>,
) -> AndroidHardwareBufferDescription {
    let mut description = AndroidHardwareBufferDescription::default();
    unsafe { AHardwareBuffer_describe(hardware_buffer.as_ptr().cast_const(), &mut description) };
    description
}

impl Drop for AndroidHardwareBufferImage {
    fn drop(&mut self) {
        unsafe { AImage_delete(self.image.as_ptr()) };
        self._source.live_image_count.fetch_sub(1, Ordering::AcqRel);
    }
}

fn duration_millis_u64(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

fn duration_micros_u64(duration: Duration) -> u64 {
    duration.as_micros().min(u64::MAX as u128) as u64
}

fn wait_and_close_acquire_fence(fd: i32, deadline: Instant) -> Result<(), AndroidMediaCodecError> {
    if fd < 0 {
        return Ok(());
    }
    struct OwnedFenceFd(i32);

    impl Drop for OwnedFenceFd {
        fn drop(&mut self) {
            unsafe { libc::close(self.0) };
        }
    }

    let fence = OwnedFenceFd(fd);
    let mut descriptor = libc::pollfd {
        fd: fence.0,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let timeout_ms = remaining.as_millis().min(i32::MAX as u128) as i32;
        descriptor.revents = 0;
        let poll_result = unsafe { libc::poll(&mut descriptor, 1, timeout_ms) };
        if poll_result < 0 {
            let poll_error = io::Error::last_os_error();
            if poll_error.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(AndroidMediaCodecError::AcquireFence {
                fd,
                reason: poll_error.to_string(),
            });
        }
        if poll_result == 0 {
            return Err(AndroidMediaCodecError::AcquireFence {
                fd,
                reason: "timeout".to_string(),
            });
        }
        break;
    }
    if descriptor.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
        return Err(AndroidMediaCodecError::AcquireFence {
            fd,
            reason: format!("poll revents=0x{:x}", descriptor.revents),
        });
    }
    Ok(())
}

fn check_media_status(status: i32, operation: &'static str) -> Result<(), AndroidMediaCodecError> {
    if status == AMEDIA_OK {
        Ok(())
    } else {
        Err(AndroidMediaCodecError::MediaStatus { operation, status })
    }
}

#[link(name = "mediandk")]
unsafe extern "C" {
    fn AImageReader_newWithUsage(
        width: i32,
        height: i32,
        format: i32,
        usage: u64,
        max_images: i32,
        reader: *mut *mut AImageReader,
    ) -> i32;
    fn AImageReader_getWindow(reader: *mut AImageReader, window: *mut *mut c_void) -> i32;
    fn AImageReader_acquireNextImageAsync(
        reader: *mut AImageReader,
        image: *mut *mut AImage,
        acquire_fence_fd: *mut i32,
    ) -> i32;
    fn AImageReader_delete(reader: *mut AImageReader);
    fn AImage_delete(image: *mut AImage);
    fn AImage_getWidth(image: *const AImage, width: *mut i32) -> i32;
    fn AImage_getHeight(image: *const AImage, height: *mut i32) -> i32;
    fn AImage_getCropRect(image: *const AImage, crop: *mut AndroidImageCrop) -> i32;
    fn AImage_getTimestamp(image: *const AImage, timestamp_ns: *mut i64) -> i32;
    fn AImage_getHardwareBuffer(
        image: *const AImage,
        hardware_buffer: *mut *mut AHardwareBuffer,
    ) -> i32;
}

#[link(name = "android")]
unsafe extern "C" {
    fn AHardwareBuffer_describe(
        buffer: *const c_void,
        description: *mut AndroidHardwareBufferDescription,
    );
}
