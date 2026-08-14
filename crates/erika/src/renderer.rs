#[cfg(all(feature = "wgpu", target_os = "android"))]
pub(crate) mod android_vulkan;
#[doc(hidden)]
pub mod artcnn;
#[cfg(target_os = "windows")]
pub mod d3d11;
#[cfg(target_os = "windows")]
mod d3d11_artcnn;
mod frame;
pub mod metal;
#[cfg(all(feature = "wgpu", target_env = "ohos"))]
pub(crate) mod ohos_vulkan;
pub mod output;
pub mod pipeline;
pub(crate) mod presentation;
#[cfg(feature = "wgpu")]
pub mod wgpu;
#[cfg(feature = "wgpu")]
#[doc(hidden)]
pub mod wgpu_artcnn;

pub use frame::{VideoFrameDescriptor, VideoFramePayload};
