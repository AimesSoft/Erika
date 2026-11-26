//! Dynamic super-resolution and RGB post-process filter discovery,
//! grouping, localization, persistence migration, and kernel dispatch.
//!
//! Built-in upscaler modes come directly from liberika's hardware capability
//! probe (no hardcoded mode enum); custom neural models and post-processing
//! shaders (such as CRT filters) are supplied exclusively through installed,
//! enabled picture plugins.

use crate::ErikaEngine;
use crate::kernel::{
    FilterTarget, KernelShaderStatus, KernelUpscalerCapabilities, UpscalerTarget,
};
use erika_protocol::erika::{EnhancementGroup, EnhancementOption, EnhancementStatus};

pub const OFF_ID: &str = "off";

/// Maps legacy enum names saved by earlier Erika versions to stable liberika
/// mode identifiers.
pub fn migrate_saved_enhancement(raw: Option<&str>) -> String {
    match raw.map(str::trim) {
        None | Some("" | "Off" | "off") => OFF_ID.into(),
        Some("ArtCnn") => "artcnn_c4f16".into(),
        Some("ArtCnnQuality") => "artcnn_c4f32".into(),
        Some("ArtCnnDenoise") => "artcnn_c4f16_ds".into(),
        Some(other) => other.into(),
    }
}

pub fn normalize_id(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("off") {
        OFF_ID.into()
    } else {
        migrate_saved_enhancement(Some(trimmed))
    }
}

pub fn default_status() -> EnhancementStatus {
    EnhancementStatus {
        active: false,
        building: false,
        inactive: false,
        engine: String::new(),
        reason: String::new(),
        gpu_ms: String::new(),
        telemetry: String::new(),
    }
}

pub fn format_status(status: &KernelShaderStatus) -> EnhancementStatus {
    let gpu_ms = status
        .gpu_ms
        .filter(|ms| ms.is_finite() && *ms > 0.0)
        .map(|ms| format!("{ms:.2} ms"))
        .unwrap_or_default();
    let telemetry = match (status.engine.trim(), gpu_ms.as_str()) {
        ("", "") => String::new(),
        (engine, "") => engine.into(),
        ("", ms) => ms.into(),
        (engine, ms) => format!("{engine} · {ms}"),
    };
    EnhancementStatus {
        active: status.active,
        building: status.building,
        inactive: status.inactive,
        engine: status.engine.clone(),
        reason: localize_reason(&status.reason),
        gpu_ms,
        telemetry,
    }
}

fn localize_reason(reason: &str) -> String {
    let trimmed = reason.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.contains("not presented above its source size") {
        return "画面未放大，当前按原分辨率呈现".into();
    }
    if trimmed.contains("only process SDR video") {
        return "仅支持 SDR 视频，HDR 画面已保持原生输出".into();
    }
    if trimmed.contains("96x96 to 960x960") {
        return "VideoToolbox 仅支持 960×960 以内的片源".into();
    }
    if trimmed.contains("no custom upscaler model") {
        return "未加载自定义模型".into();
    }
    trimmed.into()
}

fn localize_group_name(group_id: &str, fallback: &str) -> String {
    match group_id {
        "artcnn" => "ArtCNN".into(),
        "anime4k" => "Anime4K".into(),
        "anime4k_denoise" => "Anime4K 降噪".into(),
        "spatial" => "空间缩放".into(),
        "platform" => "系统原生".into(),
        _ => fallback.into(),
    }
}

fn localize_mode_name(id: &str, fallback: &str) -> String {
    match id {
        "off" => "关闭".into(),
        "artcnn_c4f16" => "C4F16（轻量）".into(),
        "artcnn_c4f16_ds" => "C4F16 DS（降噪锐化）".into(),
        "artcnn_c4f16_dn" => "C4F16 DN（降噪）".into(),
        "artcnn_c4f32" => "C4F32（高质量）".into(),
        "artcnn_c4f32_ds" => "C4F32 DS（高质降噪锐化）".into(),
        "artcnn_c4f32_dn" => "C4F32 DN（高质降噪）".into(),
        "artcnn_r8f64" => "R8F64（旗舰）".into(),
        "anime4k_s" => "S（极速）".into(),
        "anime4k_m" => "M（均衡）".into(),
        "anime4k_l" => "L（清晰）".into(),
        "anime4k_vl" => "VL（高质）".into(),
        "anime4k_ul" => "UL（极致）".into(),
        "anime4k_denoise_s" => "S（极速降噪）".into(),
        "anime4k_denoise_m" => "M（均衡降噪）".into(),
        "anime4k_denoise_l" => "L（清晰降噪）".into(),
        "anime4k_denoise_vl" => "VL（高质降噪）".into(),
        "anime4k_denoise_ul" => "UL（极致降噪）".into(),
        "fsr1" => "AMD FSR 1.0".into(),
        "metalfx_spatial" => "MetalFX 空间超分".into(),
        _ => fallback.into(),
    }
}

/// Builds the flat option list (leading with `"off"`, used by sub-pages and
/// settings pickers) and the top-level group rows (auto-promoting single-mode
/// groups to direct selection and collapsing multi-mode groups into sub-pages).
fn build_catalog(
    page_prefix: &str,
    entries: Vec<(String, String, String, String, bool)>,
    selected_id: &str,
) -> (Vec<EnhancementOption>, Vec<EnhancementGroup>, String) {
    // Preserve insertion order of groups so built-in families stay stable.
    struct GroupBucket {
        id: String,
        page_key: String,
        name: String,
        is_plugin: bool,
        items: Vec<(String, String, bool)>,
    }
    let mut buckets: Vec<GroupBucket> = Vec::new();
    for (id, group_id, group_name, name, is_plugin) in entries {
        if let Some(bucket) = buckets.iter_mut().find(|b| b.id == group_id) {
            bucket.items.push((id, name, is_plugin));
        } else {
            buckets.push(GroupBucket {
                page_key: format!("{page_prefix}:{group_id}"),
                id: group_id,
                name: group_name,
                is_plugin,
                items: vec![(id, name, is_plugin)],
            });
        }
    }

    let mut options = Vec::new();
    options.push(EnhancementOption {
        id: OFF_ID.into(),
        page_key: String::new(),
        group_id: OFF_ID.into(),
        name: "关闭".into(),
        picker_name: "关闭".into(),
        is_plugin: false,
    });

    let mut groups = Vec::with_capacity(buckets.len());
    for bucket in buckets {
        let multi = bucket.items.len() > 1;
        for (item_id, item_name, item_is_plugin) in &bucket.items {
            let picker_name = if multi {
                format!("{} · {}", bucket.name, item_name)
            } else {
                item_name.clone()
            };
            options.push(EnhancementOption {
                id: item_id.clone(),
                page_key: bucket.page_key.clone(),
                group_id: bucket.id.clone(),
                name: item_name.clone(),
                picker_name,
                is_plugin: *item_is_plugin,
            });
        }

        if !multi {
            let (item_id, item_name, _) = &bucket.items[0];
            let selected = selected_id == item_id;
            groups.push(EnhancementGroup {
                id: bucket.id,
                page_key: bucket.page_key,
                name: item_name.clone(),
                direct_id: item_id.clone(),
                selected_name: String::new(),
                selected,
                is_plugin: bucket.is_plugin,
            });
        } else {
            let active = bucket
                .items
                .iter()
                .find(|(item_id, _, _)| item_id == selected_id);
            groups.push(EnhancementGroup {
                id: bucket.id,
                page_key: bucket.page_key,
                name: bucket.name,
                direct_id: String::new(),
                selected_name: active.map(|(_, name, _)| name.clone()).unwrap_or_default(),
                selected: active.is_some(),
                is_plugin: bucket.is_plugin,
            });
        }
    }

    let selected_name = options
        .iter()
        .find(|item| item.id == selected_id)
        .map(|item| item.picker_name.clone())
        .unwrap_or_else(|| "关闭".into());

    (options, groups, selected_name)
}

fn summarize_capabilities(caps: &KernelUpscalerCapabilities) -> String {
    let mut families: Vec<String> = Vec::new();
    for mode in &caps.modes {
        let label = match mode.group_id.as_str() {
            "platform" | "spatial" => localize_mode_name(&mode.id, &mode.name),
            other => localize_group_name(other, &mode.group_name),
        };
        if !families.contains(&label) {
            families.push(label);
        }
    }
    families.join(" · ")
}

impl ErikaEngine {
    pub(crate) fn init_enhancements(&mut self) {
        // Query the installed kernel directly (not `self.kernel()` which waits
        // for the frontend's MediaHub attachment) so cold-start hardware
        // probes populate Settings › Enhance before any video is opened.
        if let Some(kernel) = self.kernel.as_ref() {
            self.kernel_capabilities = kernel.upscaler_capabilities();
        }
        self.refresh_enhancements();
    }

    pub(crate) fn update_kernel_capabilities(&mut self, caps: KernelUpscalerCapabilities) {
        if self.kernel_capabilities != caps {
            self.kernel_capabilities = caps;
            self.refresh_enhancements();
            self.apply_enhancement();
            self.apply_filter();
        }
    }

    pub(crate) fn refresh_enhancements(&mut self) {
        let caps = self.kernel_capabilities.clone();
        let plugin_upscalers = self.plugins_upscalers();
        let plugin_filters = self.plugins_filters();
        let picture_plugin_count = self.enabled_picture_plugin_count() as i64;
        let kernel_known = self.kernel.is_some() || !caps.modes.is_empty();
        let plugins_known = self.plugins.started;

        let mut upscaler_entries: Vec<(String, String, String, String, bool)> = caps
            .modes
            .iter()
            .map(|desc| {
                (
                    desc.id.clone(),
                    desc.group_id.clone(),
                    localize_group_name(&desc.group_id, &desc.group_name),
                    localize_mode_name(&desc.id, &desc.name),
                    false,
                )
            })
            .collect();

        if caps.supports_custom_network {
            for item in plugin_upscalers {
                upscaler_entries.push((
                    item.full_id,
                    item.group_id,
                    item.group_name,
                    item.name,
                    true,
                ));
            }
        }

        // Only drop an unknown selection once its owning provider (kernel or
        // plugins) has initialized, so saved plugin choices survive startup.
        let current_upscaler = self.state.player.enhancement.as_str();
        let should_validate_upscaler = if current_upscaler.starts_with("plugin:") {
            plugins_known
        } else {
            kernel_known
        };
        if should_validate_upscaler
            && current_upscaler != OFF_ID
            && !upscaler_entries
                .iter()
                .any(|(id, ..)| id == current_upscaler)
        {
            self.state.player.enhancement = OFF_ID.into();
        }

        let (enhancements, enhancement_groups, enhancement_name) =
            build_catalog("u", upscaler_entries, &self.state.player.enhancement);

        let mut filter_entries: Vec<(String, String, String, String, bool)> = Vec::new();
        if caps.supports_post_process {
            for item in plugin_filters {
                filter_entries.push((
                    item.full_id,
                    item.group_id,
                    item.group_name,
                    item.name,
                    true,
                ));
            }
        }
        let current_filter = self.state.player.filter.as_str();
        if plugins_known
            && kernel_known
            && current_filter != OFF_ID
            && !filter_entries.iter().any(|(id, ..)| id == current_filter)
        {
            self.state.player.filter = OFF_ID.into();
        }

        let (filters, filter_groups, filter_name) =
            build_catalog("f", filter_entries, &self.state.player.filter);

        let capabilities_summary = summarize_capabilities(&caps);

        let player = &mut self.state.player;
        player.has_enhancements = !enhancement_groups.is_empty();
        player.enhancements = enhancements;
        player.enhancement_groups = enhancement_groups;
        player.enhancement_name = enhancement_name;
        player.enhancement_supports_custom = caps.supports_custom_network;
        player.enhancement_capabilities = capabilities_summary;
        player.has_filters = !filter_groups.is_empty();
        player.filters = filters;
        player.filter_groups = filter_groups;
        player.filter_name = filter_name;
        player.filter_supports_custom = caps.supports_post_process;
        player.picture_plugin_count = picture_plugin_count;
    }

    pub(crate) fn resolve_upscaler_target(&self) -> UpscalerTarget {
        if !self.state.features.enhance {
            return UpscalerTarget::Off;
        }
        let id = self.state.player.enhancement.as_str();
        if id.is_empty() || id == OFF_ID {
            return UpscalerTarget::Off;
        }
        if id.starts_with("plugin:") {
            return match self.read_plugin_upscaler(id) {
                Some((name, bytes)) => UpscalerTarget::Custom {
                    id: id.into(),
                    name,
                    bytes,
                },
                None => UpscalerTarget::Off,
            };
        }
        UpscalerTarget::Builtin(id.into())
    }

    pub(crate) fn resolve_filter_target(&self) -> FilterTarget {
        if !self.state.features.enhance {
            return FilterTarget::Off;
        }
        let id = self.state.player.filter.as_str();
        if id.is_empty() || id == OFF_ID {
            return FilterTarget::Off;
        }
        match self.read_plugin_filter(id) {
            Some((name, bytes)) => FilterTarget::Shader {
                id: id.into(),
                name,
                bytes,
            },
            None => FilterTarget::Off,
        }
    }

    pub(crate) fn apply_enhancement(&mut self) {
        let target = self.resolve_upscaler_target();
        if self.state.video
            && let Some(kernel) = self.kernel()
        {
            kernel.set_enhancement(target);
        }
    }

    pub(crate) fn apply_filter(&mut self) {
        let target = self.resolve_filter_target();
        if self.state.video
            && let Some(kernel) = self.kernel()
        {
            kernel.set_filter(target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_legacy_enhancement_names() {
        assert_eq!(migrate_saved_enhancement(None), "off");
        assert_eq!(migrate_saved_enhancement(Some("Off")), "off");
        assert_eq!(migrate_saved_enhancement(Some("ArtCnn")), "artcnn_c4f16");
        assert_eq!(
            migrate_saved_enhancement(Some("ArtCnnQuality")),
            "artcnn_c4f32"
        );
        assert_eq!(
            migrate_saved_enhancement(Some("ArtCnnDenoise")),
            "artcnn_c4f16_ds"
        );
        assert_eq!(migrate_saved_enhancement(Some("anime4k_m")), "anime4k_m");
    }

    #[test]
    fn single_mode_group_is_direct_and_multi_mode_group_opens_subpage() {
        let entries = vec![
            (
                "fsr1".into(),
                "spatial".into(),
                "空间缩放".into(),
                "AMD FSR 1.0".into(),
                false,
            ),
            (
                "artcnn_c4f16".into(),
                "artcnn".into(),
                "ArtCNN".into(),
                "C4F16（轻量）".into(),
                false,
            ),
            (
                "artcnn_c4f32".into(),
                "artcnn".into(),
                "ArtCNN".into(),
                "C4F32（高质量）".into(),
                false,
            ),
        ];
        let (options, groups, summary) = build_catalog("u", entries, "artcnn_c4f32");
        assert_eq!(options.len(), 4);
        assert_eq!(options[1].picker_name, "AMD FSR 1.0");
        assert_eq!(options[3].page_key, "u:artcnn");
        assert_eq!(options[3].name, "C4F32（高质量）");
        assert_eq!(options[3].picker_name, "ArtCNN · C4F32（高质量）");
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].direct_id, "fsr1");
        assert!(!groups[0].selected);
        assert!(groups[1].direct_id.is_empty());
        assert_eq!(groups[1].page_key, "u:artcnn");
        assert!(groups[1].selected);
        assert_eq!(groups[1].selected_name, "C4F32（高质量）");
        assert_eq!(summary, "ArtCNN · C4F32（高质量）");
    }

    #[test]
    fn cold_start_init_probes_capabilities_even_before_kernel_is_ready() {
        use crate::kernel::{Kernel, KernelUpscalerDescriptor};
        use std::sync::Arc;

        struct ColdKernel;
        impl Kernel for ColdKernel {
            fn ready(&self) -> bool {
                false
            }
            fn open(&self, _: &str, _: f64) {}
            fn play(&self) {}
            fn pause(&self) {}
            fn seek(&self, _: f64) {}
            fn set_volume(&self, _: f64) {}
            fn set_rate(&self, _: f64) {}
            fn select_audio(&self, _: i64) {}
            fn select_subtitle(&self, _: Option<i64>) {}
            fn add_subtitle(&self, _: &str) {}
            fn set_subtitle_scale(&self, _: f64) {}
            fn set_subtitle_supersampling(&self, _: bool) {}
            fn upscaler_capabilities(&self) -> KernelUpscalerCapabilities {
                KernelUpscalerCapabilities {
                    modes: vec![KernelUpscalerDescriptor {
                        id: "fsr1".into(),
                        name: "AMD FSR 1.0".into(),
                        group_id: "spatial".into(),
                        group_name: "Spatial".into(),
                    }],
                    supports_custom_network: true,
                    supports_post_process: true,
                }
            }
            fn set_enhancement(&self, _: UpscalerTarget) {}
            fn set_filter(&self, _: FilterTarget) {}
            fn set_danmaku(&self, _: &crate::kernel::DanmakuConfig) {}
            fn add_danmaku(&self, _: &str, _: &str) -> Result<usize, String> {
                Ok(0)
            }
            fn send_danmaku(&self, _: &str, _: u32) {}
            fn capture(&self, _: &std::path::Path, _: Option<u32>) {}
            fn close(&self) {}
            fn playback(&self) -> crate::kernel::Playback {
                Default::default()
            }
        }

        let mut engine = ErikaEngine::default();
        engine.kernel = Some(Arc::new(ColdKernel));
        engine.state.player.filter = "plugin:erika.crt_filter:standard".into();
        engine.init_enhancements();
        assert!(engine.state.player.has_enhancements);
        assert!(engine.state.player.enhancement_supports_custom);
        assert!(engine.state.player.filter_supports_custom);
        assert_eq!(engine.state.player.enhancement_capabilities, "AMD FSR 1.0");
        // Saved plugin filter must not be wiped before plugins finish starting.
        assert_eq!(
            engine.state.player.filter,
            "plugin:erika.crt_filter:standard"
        );
        // Once plugins have started without that plugin installed, it falls back cleanly.
        engine.plugins.started = true;
        engine.refresh_enhancements();
        assert_eq!(engine.state.player.filter, OFF_ID);
    }
}
