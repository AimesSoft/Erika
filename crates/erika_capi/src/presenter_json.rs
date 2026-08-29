use std::ffi::{CStr, CString, c_char};
use std::ptr;

use serde_json::{Map, Value, json};

use super::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_invoke_json(
    handle: *mut ErikaPresenterHandle,
    method: *const c_char,
    arguments_json: *const c_char,
) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let method = read_c_string(method, "method")?;
        let arguments_json = read_c_string(arguments_json, "arguments_json")?;
        let arguments: Value = serde_json::from_str(&arguments_json)
            .map_err(|error| format!("invalid presenter JSON arguments: {error}"))?;
        unsafe { invoke(handle, &method, &arguments) }
    }));
    response_string(result)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_render_tick_json(
    handle: *mut ErikaPresenterHandle,
    time_seconds: f64,
) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut stats = ErikaPresenterStats::default();
        call_status(unsafe { erika_presenter_render_tick(handle, time_seconds, &mut stats) })?;
        Ok(stats_json(stats))
    }));
    response_string(result)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_poll_event_json(
    handle: *mut ErikaPresenterHandle,
) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<Option<Value>, String> {
        let mut event = ErikaEvent::default();
        match unsafe { erika_presenter_poll_event(handle, &mut event) } {
            ErikaStatus::NoEvent => return Ok(None),
            status => call_status(status)?,
        }
        let mut value = event_json(event);
        if let Value::Object(map) = &mut value {
            match event.kind {
                ErikaEventKind::Error => {
                    map.insert("error".to_string(), Value::String(last_error(event.status)));
                }
                ErikaEventKind::VideoDecoderChanged => {
                    let message = last_error(event.status);
                    if let Ok(decoder) = serde_json::from_str::<Value>(&message) {
                        map.insert("decoder".to_string(), decoder);
                    }
                    map.insert("message".to_string(), Value::String(message));
                }
                ErikaEventKind::AudioOutputChanged => {
                    let message = last_error(event.status);
                    if let Ok(audio) = serde_json::from_str::<Value>(&message) {
                        map.insert("audio".to_string(), audio);
                    }
                    map.insert("message".to_string(), Value::String(message));
                }
                _ => {}
            }
        }
        Ok(Some(value))
    }));
    match result {
        Ok(Ok(None)) => ptr::null_mut(),
        Ok(Ok(Some(value))) => owned_json(success_response(value)),
        Ok(Err(error)) => owned_json(error_response(error)),
        Err(_) => owned_json(error_response("panic in Erika presenter event polling")),
    }
}

unsafe fn invoke(
    handle: *mut ErikaPresenterHandle,
    method: &str,
    arguments: &Value,
) -> Result<Value, String> {
    let args = arguments
        .as_object()
        .ok_or_else(|| "arguments must be a JSON object".to_string())?;
    match method {
        "open" => {
            let uri = required_string(args, "uri")?;
            let uri = required_c_string(uri, "uri")?;
            let headers = HttpHeaders::from_json(args)?;
            let options = ErikaOpenOptions {
                headers: headers.headers.as_ptr(),
                header_count: headers.headers.len(),
                http_read_ahead_bytes: optional_read_ahead_bytes(args)?,
                reserved: [0; 3],
            };
            call_status(unsafe {
                erika_presenter_open_with_options(handle, uri.as_ptr(), &options)
            })?;
            Ok(Value::Null)
        }
        "play" => status_value(unsafe { erika_presenter_play(handle) }),
        "pause" => status_value(unsafe { erika_presenter_pause(handle) }),
        "stop" => status_value(unsafe { erika_presenter_stop(handle) }),
        "close" => status_value(unsafe { erika_presenter_close(handle) }),
        "seek" => status_value(unsafe {
            erika_presenter_seek(handle, required_u64(args, "positionMicros")?)
        }),
        "setPlaybackRate" => status_value(unsafe {
            erika_presenter_set_playback_rate(handle, required_f64(args, "rate")?)
        }),
        "setVolume" => status_value(unsafe {
            erika_presenter_set_volume(handle, required_f64(args, "volume")?)
        }),
        "setUpscaler" => status_value(unsafe {
            erika_presenter_set_upscaler(handle, required_i64(args, "mode")? as i32)
        }),
        "setSubtitleScale" => status_value(unsafe {
            erika_presenter_set_subtitle_scale(handle, required_f64(args, "scale")?)
        }),
        "selectSubtitleMemoryFonts" => {
            let ids = required_u64_array(args, "fontIds")?;
            status_value(unsafe {
                erika_presenter_select_subtitle_memory_fonts(handle, ids.as_ptr(), ids.len())
            })
        }
        "clearSubtitleMemoryFonts" => {
            status_value(unsafe { erika_presenter_clear_subtitle_memory_fonts(handle) })
        }
        "getSubtitleMemoryFontStatus" => unsafe { subtitle_memory_font_status_json(handle) },
        "getUpscalerStatus" => {
            let mut status = ErikaUpscalerStatus::default();
            call_status(unsafe { erika_presenter_get_upscaler_status(handle, &mut status) })?;
            Ok(upscaler_status_json(status))
        }
        "getOutputStatus" => {
            let mut status = ErikaOutputStatus::default();
            call_status(unsafe { erika_presenter_get_output_status(handle, &mut status) })?;
            Ok(output_status_json(status))
        }
        "getPresenterStats" => {
            let mut stats = ErikaPresenterStats::default();
            call_status(unsafe { erika_presenter_get_stats(handle, &mut stats) })?;
            Ok(stats_json(stats))
        }
        "getResourceStatus" => {
            let mut status = ErikaPresenterResourceStatus::default();
            call_status(unsafe { erika_presenter_get_resource_status(handle, &mut status) })?;
            Ok(resource_status_json(status))
        }
        "addExternalSubtitle" => {
            let uri = required_c_string(required_string(args, "uri")?, "uri")?;
            let mut track_id = -1;
            call_status(unsafe {
                erika_presenter_add_external_subtitle(handle, uri.as_ptr(), &mut track_id)
            })?;
            Ok(json!(track_id))
        }
        "removeSubtitleTrack" => status_value(unsafe {
            erika_presenter_remove_subtitle_track(handle, required_i64(args, "trackId")?)
        }),
        "selectAudioTrack" => status_value(unsafe {
            erika_presenter_select_audio_track(handle, optional_i64(args, "trackId").unwrap_or(-1))
        }),
        "selectSubtitleTrack" => status_value(unsafe {
            erika_presenter_select_subtitle_track(
                handle,
                optional_i64(args, "trackId").unwrap_or(-1),
            )
        }),
        "tracks" => unsafe { tracks_json(handle) },
        "loadDanmakuFile" => {
            let uri = required_c_string(required_string(args, "uri")?, "uri")?;
            status_value(unsafe { erika_presenter_load_danmaku_file(handle, uri.as_ptr()) })
        }
        "loadDanmakuJson" => {
            let source = required_c_string(required_string(args, "json")?, "json")?;
            status_value(unsafe { erika_presenter_load_danmaku_json(handle, source.as_ptr()) })
        }
        "addDanmakuTrackFile" => {
            let uri = required_c_string(required_string(args, "uri")?, "uri")?;
            let name = optional_c_string(args, "name")?;
            let mut track_id = 0;
            call_status(unsafe {
                erika_presenter_add_danmaku_track_file(
                    handle,
                    uri.as_ptr(),
                    optional_c_string_ptr(&name),
                    optional_i64(args, "offsetMicros").unwrap_or(0),
                    &mut track_id,
                )
            })?;
            Ok(json!(track_id))
        }
        "addDanmakuTrackJson" => {
            let source = required_c_string(required_string(args, "json")?, "json")?;
            let name = optional_c_string(args, "name")?;
            let mut track_id = 0;
            call_status(unsafe {
                erika_presenter_add_danmaku_track_json(
                    handle,
                    source.as_ptr(),
                    optional_c_string_ptr(&name),
                    optional_i64(args, "offsetMicros").unwrap_or(0),
                    &mut track_id,
                )
            })?;
            Ok(json!(track_id))
        }
        "removeDanmakuTrack" => status_value(unsafe {
            erika_presenter_remove_danmaku_track(handle, required_u64(args, "trackId")?)
        }),
        "setDanmakuTrackEnabled" => status_value(unsafe {
            erika_presenter_set_danmaku_track_enabled(
                handle,
                required_u64(args, "trackId")?,
                optional_bool(args, "enabled").unwrap_or(true),
            )
        }),
        "setDanmakuTrackOffset" => status_value(unsafe {
            erika_presenter_set_danmaku_track_offset(
                handle,
                required_u64(args, "trackId")?,
                optional_i64(args, "offsetMicros").unwrap_or(0),
            )
        }),
        "setDanmakuGlobalOffset" => status_value(unsafe {
            erika_presenter_set_danmaku_global_offset(
                handle,
                optional_i64(args, "offsetMicros").unwrap_or(0),
            )
        }),
        "danmakuTracks" => unsafe { danmaku_tracks_json(handle) },
        "clearDanmaku" => status_value(unsafe { erika_presenter_clear_danmaku(handle) }),
        "setDanmakuEnabled" => status_value(unsafe {
            erika_presenter_set_danmaku_enabled(
                handle,
                optional_bool(args, "enabled").unwrap_or(true),
            )
        }),
        "setDanmakuConfig" => unsafe { set_danmaku_config(handle, args) },
        method => Err(format!("unsupported Erika presenter method: {method}")),
    }
}

unsafe fn set_danmaku_config(
    handle: *mut ErikaPresenterHandle,
    args: &Map<String, Value>,
) -> Result<Value, String> {
    let mut config = ErikaDanmakuConfig::default();
    call_status(unsafe { erika_presenter_get_danmaku_config(handle, &mut config) })?;
    update_bool(args, "enabled", &mut config.enabled);
    update_f32(args, "fontSize", &mut config.font_size);
    update_f32(args, "opacity", &mut config.opacity);
    update_f32(args, "displayArea", &mut config.display_area);
    update_f32(
        args,
        "scrollDurationSeconds",
        &mut config.scroll_duration_seconds,
    );
    update_f32(args, "scrollSpeedFactor", &mut config.scroll_speed_factor);
    update_f32(args, "trackGapRatio", &mut config.track_gap_ratio);
    update_f32(args, "outlineWidth", &mut config.outline_width);
    update_f32(args, "shadowOffsetX", &mut config.shadow_offset_x);
    update_f32(args, "shadowOffsetY", &mut config.shadow_offset_y);
    update_bool(args, "mergeDuplicates", &mut config.merge_duplicates);
    update_bool(args, "allowStacking", &mut config.allow_stacking);
    update_bool(
        args,
        "allowScrollOverwrite",
        &mut config.allow_scroll_overwrite,
    );
    update_u32(args, "maxQuantity", &mut config.max_quantity);
    update_u32(args, "maxLinesPerMode", &mut config.max_lines_per_mode);
    update_bool(args, "blockTop", &mut config.block_top);
    update_bool(args, "blockBottom", &mut config.block_bottom);
    update_bool(args, "blockScroll", &mut config.block_scroll);
    if let Some(value) = args.get("shadowStyle").and_then(Value::as_i64) {
        config.shadow_style = value as i32;
    }
    call_status(unsafe { erika_presenter_set_danmaku_config(handle, config) })?;
    if args.contains_key("customFontFamily") || args.contains_key("customFontFilePath") {
        let family = optional_c_string(args, "customFontFamily")?;
        let file_path = optional_c_string(args, "customFontFilePath")?;
        call_status(unsafe {
            erika_presenter_set_danmaku_font(
                handle,
                optional_c_string_ptr(&family),
                optional_c_string_ptr(&file_path),
            )
        })?;
    }
    if let Some(words) = args.get("blockWordsJson").and_then(Value::as_str) {
        let words = required_c_string(words, "blockWordsJson")?;
        call_status(unsafe {
            erika_presenter_set_danmaku_block_words_json(handle, words.as_ptr())
        })?;
    }
    Ok(Value::Null)
}

unsafe fn tracks_json(handle: *mut ErikaPresenterHandle) -> Result<Value, String> {
    let mut len = 0;
    call_status(unsafe { erika_presenter_tracks(handle, ptr::null_mut(), 0, &mut len) })?;
    let mut tracks = vec![ErikaTrackInfo::default(); len];
    if len > 0 {
        call_status(unsafe {
            erika_presenter_tracks(handle, tracks.as_mut_ptr(), tracks.len(), &mut len)
        })?;
    }
    Ok(Value::Array(
        tracks
            .iter_mut()
            .take(len)
            .map(|track| {
                let value = json!({
                    "id": track.id, "kind": track.kind as i32,
                    "source": track.source as i32, "selected": track.selected,
                    "canRemove": track.can_remove,
                    "title": unsafe { borrowed_string(track.title) },
                    "language": unsafe { borrowed_string(track.language) },
                    "codec": unsafe { borrowed_string(track.codec) },
                    "width": track.width, "height": track.height,
                    "sampleRate": track.sample_rate, "channels": track.channels,
                    "pixelFormat": unsafe { borrowed_string(track.pixel_format) },
                    "sampleFormat": unsafe { borrowed_string(track.sample_format) },
                    "profile": unsafe { borrowed_string(track.profile) },
                    "level": track.level,
                });
                unsafe { erika_track_info_free(track) };
                value
            })
            .collect(),
    ))
}

unsafe fn danmaku_tracks_json(handle: *mut ErikaPresenterHandle) -> Result<Value, String> {
    let mut len = 0;
    call_status(unsafe { erika_presenter_danmaku_tracks(handle, ptr::null_mut(), 0, &mut len) })?;
    let mut tracks = vec![ErikaDanmakuTrackInfo::default(); len];
    if len > 0 {
        call_status(unsafe {
            erika_presenter_danmaku_tracks(handle, tracks.as_mut_ptr(), tracks.len(), &mut len)
        })?;
    }
    Ok(Value::Array(
        tracks
            .iter_mut()
            .take(len)
            .map(|track| {
                let value = json!({
                    "id": track.id, "enabled": track.enabled,
                    "offsetMicros": track.offset_micros, "itemCount": track.item_count,
                    "name": unsafe { borrowed_string(track.name) },
                    "source": unsafe { borrowed_string(track.source) },
                });
                unsafe { erika_danmaku_track_info_free(track) };
                value
            })
            .collect(),
    ))
}

unsafe fn subtitle_memory_font_status_json(
    handle: *mut ErikaPresenterHandle,
) -> Result<Value, String> {
    let mut status = ErikaSubtitleMemoryFontStatus::default();
    call_status(unsafe { erika_presenter_get_subtitle_memory_font_status(handle, &mut status) })?;
    let selected_ids = if status.selected_count == 0 || status.selected_ids.is_null() {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(status.selected_ids, status.selected_count) }.to_vec()
    };
    let value = json!({
        "registeredCount": status.registered_count,
        "registeredBytes": status.registered_bytes,
        "selectedCount": status.selected_count,
        "generation": status.generation,
        "selectedIds": selected_ids,
    });
    unsafe { erika_subtitle_memory_font_status_free(&mut status) };
    Ok(value)
}

struct HttpHeaders {
    _strings: Vec<CString>,
    headers: Vec<ErikaHttpHeader>,
}

impl HttpHeaders {
    fn from_json(args: &Map<String, Value>) -> Result<Self, String> {
        let Some(value) = args.get("httpHeaders") else {
            return Ok(Self {
                _strings: Vec::new(),
                headers: Vec::new(),
            });
        };
        let object = value
            .as_object()
            .ok_or_else(|| "httpHeaders must be a JSON object".to_string())?;
        let mut strings = Vec::with_capacity(object.len() * 2);
        for (name, value) in object {
            let value = value
                .as_str()
                .ok_or_else(|| format!("HTTP header {name} must be a string"))?;
            strings.push(required_c_string(name, "HTTP header name")?);
            strings.push(required_c_string(value, "HTTP header value")?);
        }
        let headers = strings
            .chunks_exact(2)
            .map(|pair| ErikaHttpHeader {
                name: pair[0].as_ptr(),
                value: pair[1].as_ptr(),
            })
            .collect();
        Ok(Self {
            _strings: strings,
            headers,
        })
    }
}

fn event_json(event: ErikaEvent) -> Value {
    json!({
        "kind": event.kind as i32, "status": event.status as i32,
        "state": event.state as i32, "durationMicros": event.duration_micros,
        "positionMicros": event.position_micros, "buffering": event.buffering,
        "video": { "width": event.video.width, "height": event.video.height,
            "primaries": event.video.primaries, "transfer": event.video.transfer },
        "tracks": { "video": event.tracks.video, "audio": event.tracks.audio,
            "subtitle": event.tracks.subtitle },
    })
}

fn stats_json(stats: ErikaPresenterStats) -> Value {
    json!({
        "decodedVideoFrames": stats.decoded_video_frames,
        "renderedVideoFrames": stats.rendered_video_frames,
        "renderedTestFrames": stats.rendered_test_frames,
        "pushedAudioFrames": stats.pushed_audio_frames,
        "overlayFrames": stats.overlay_frames, "danmakuFrames": stats.danmaku_frames,
        "danmakuItems": stats.danmaku_items, "importFailures": stats.import_failures,
        "renderFailures": stats.render_failures, "audioFailures": stats.audio_failures,
        "softwareVideoFrames": stats.software_video_frames,
        "hardwareVideoFrames": stats.hardware_video_frames,
        "zeroCopyVideoFrames": stats.zero_copy_video_frames,
        "cpuVideoFrameFallbacks": stats.cpu_video_frame_fallbacks,
        "lastRenderMicros": stats.last_render_micros,
        "lastRenderCurrentMicros": stats.last_render_current_micros,
        "audioClockReadFrames": stats.audio_clock_read_frames,
        "audioClockQueuedFrames": stats.audio_clock_queued_frames,
        "audioClockUnderflowFrames": stats.audio_clock_underflow_frames,
        "audioRecoveryState": stats.audio_recovery_state,
        "audioLastErrorCode": stats.audio_last_error_code,
        "audioRecoveryAttempts": stats.audio_recovery_attempts,
        "audioRecoveryCount": stats.audio_recovery_count,
        "audioRecoveryFailures": stats.audio_recovery_failures,
        "directZeroCopyVideoFrames": stats.direct_zero_copy_video_frames,
        "sharedHandleVideoFrames": stats.shared_handle_video_frames,
        "hdrSourceFrames": stats.hdr_source_frames,
        "hdr10OutputFrames": stats.hdr10_output_frames,
        "sdrTonemapFrames": stats.sdr_tonemap_frames,
        "hdr10MetadataUpdates": stats.hdr10_metadata_updates,
        "hdr10MetadataFailures": stats.hdr10_metadata_failures,
        "hdr10OutputFailures": stats.hdr10_output_failures,
        "hdr10OutputActive": stats.hdr10_output_active,
        "videoFrameBackpressureDrops": stats.video_frame_backpressure_drops,
    })
}

fn upscaler_status_json(status: ErikaUpscalerStatus) -> Value {
    json!({
        "requestedMode": status.requested_mode, "activeBackend": status.active_backend,
        "fallbackCount": status.fallback_count, "upscaledFrames": status.upscaled_frames,
        "lastEncodeMicros": status.last_encode_micros, "lastGpuMicros": status.last_gpu_micros,
    })
}

fn output_status_json(status: ErikaOutputStatus) -> Value {
    json!({
        "requestedMode": status.requested_mode, "activeEncoding": status.active_encoding,
        "surfaceFormat": status.surface_format, "nativeDataSpace": status.native_data_space,
        "requestedHeadroom": status.requested_headroom, "activeHeadroom": status.active_headroom,
        "activeHeadroomKnown": status.active_headroom_known,
        "extendedLinearActive": status.extended_linear_active,
        "fallbackReason": status.fallback_reason, "fallbackCount": status.fallback_count,
        "dataSpaceFailures": status.data_space_failures, "headroomUpdates": status.headroom_updates,
        "extendedLinearFrames": status.extended_linear_frames,
    })
}

fn resource_status_json(status: ErikaPresenterResourceStatus) -> Value {
    json!({
        "deviceCurrentAllocatedBytes": status.device_current_allocated_bytes,
        "deviceRecommendedWorkingSetBytes": status.device_recommended_working_set_bytes,
        "drawableEstimatedBytes": status.drawable_estimated_bytes,
        "videoFrameBytes": status.video_frame_bytes,
        "overlayAtlasBytes": status.overlay_atlas_bytes,
        "danmakuAtlasBytes": status.danmaku_atlas_bytes,
        "danmakuVertexBufferBytes": status.danmaku_vertex_buffer_bytes,
        "upscalerBytes": status.upscaler_bytes,
        "rendererTrackedBytes": status.renderer_tracked_bytes,
        "presenterCpuDanmakuAtlasBytes": status.presenter_cpu_danmaku_atlas_bytes,
        "drawableCount": status.drawable_count,
        "outputModeSwitches": status.output_mode_switches,
    })
}

fn response_string(result: std::thread::Result<Result<Value, String>>) -> *mut c_char {
    match result {
        Ok(Ok(value)) => owned_json(success_response(value)),
        Ok(Err(error)) => owned_json(error_response(error)),
        Err(_) => owned_json(error_response("panic in Erika presenter JSON bridge")),
    }
}

fn success_response(value: Value) -> Value {
    json!({ "ok": true, "status": ErikaStatus::Ok as i32, "value": value })
}

fn error_response(error: impl Into<String>) -> Value {
    json!({ "ok": false, "status": ErikaStatus::PlayerError as i32, "error": error.into() })
}

fn owned_json(value: Value) -> *mut c_char {
    CString::new(value.to_string())
        .expect("serialized JSON contains no NUL")
        .into_raw()
}

fn call_status(status: ErikaStatus) -> Result<(), String> {
    if status == ErikaStatus::Ok {
        Ok(())
    } else {
        Err(last_error(status))
    }
}

fn status_value(status: ErikaStatus) -> Result<Value, String> {
    call_status(status)?;
    Ok(Value::Null)
}

fn last_error(status: ErikaStatus) -> String {
    let value = erika_last_error_message();
    if value.is_null() {
        return format!("Erika C ABI returned {status:?}");
    }
    let message = unsafe { CStr::from_ptr(value) }
        .to_string_lossy()
        .into_owned();
    unsafe { erika_string_free(value) };
    message
}

fn read_c_string(value: *const c_char, name: &str) -> Result<String, String> {
    if value.is_null() {
        return Err(format!("{name} pointer is null"));
    }
    unsafe { CStr::from_ptr(value) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| format!("{name} is not valid UTF-8"))
}

fn required_string<'a>(args: &'a Map<String, Value>, name: &str) -> Result<&'a str, String> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{name} is required"))
}

fn required_i64(args: &Map<String, Value>, name: &str) -> Result<i64, String> {
    args.get(name)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("{name} is required"))
}

fn required_u64(args: &Map<String, Value>, name: &str) -> Result<u64, String> {
    args.get(name)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{name} must be non-negative"))
}

/// Parses the optional `httpReadAheadBytes` open argument. 0 / missing keeps
/// the default resolution (environment override, then 2 MiB); negative or
/// fractional values are rejected because they cannot be a byte count.
fn optional_read_ahead_bytes(args: &Map<String, Value>) -> Result<u64, String> {
    let Some(value) = args.get("httpReadAheadBytes") else {
        return Ok(0);
    };
    match value {
        Value::Number(number) => number
            .as_u64()
            .ok_or_else(|| "httpReadAheadBytes must be a non-negative integer".to_string()),
        Value::Null => Ok(0),
        _ => Err("httpReadAheadBytes must be a non-negative integer".to_string()),
    }
}

fn required_u64_array(args: &Map<String, Value>, name: &str) -> Result<Vec<u64>, String> {
    args.get(name)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{name} must be an array"))?
        .iter()
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| format!("{name} must contain only unsigned integers"))
        })
        .collect()
}

fn required_f64(args: &Map<String, Value>, name: &str) -> Result<f64, String> {
    args.get(name)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("{name} must be finite"))
}

fn optional_i64(args: &Map<String, Value>, name: &str) -> Option<i64> {
    args.get(name).and_then(Value::as_i64)
}

fn optional_bool(args: &Map<String, Value>, name: &str) -> Option<bool> {
    args.get(name).and_then(Value::as_bool)
}

fn required_c_string(value: &str, name: &str) -> Result<CString, String> {
    CString::new(value).map_err(|_| format!("{name} contains a NUL byte"))
}

fn optional_c_string(args: &Map<String, Value>, name: &str) -> Result<Option<CString>, String> {
    args.get(name)
        .and_then(Value::as_str)
        .map(|value| required_c_string(value, name))
        .transpose()
}

fn optional_c_string_ptr(value: &Option<CString>) -> *const c_char {
    value.as_ref().map_or(ptr::null(), |value| value.as_ptr())
}

unsafe fn borrowed_string(value: *const c_char) -> Option<String> {
    if value.is_null() {
        None
    } else {
        Some(
            unsafe { CStr::from_ptr(value) }
                .to_string_lossy()
                .into_owned(),
        )
    }
}

fn update_bool(args: &Map<String, Value>, name: &str, target: &mut bool) {
    if let Some(value) = args.get(name).and_then(Value::as_bool) {
        *target = value;
    }
}

fn update_f32(args: &Map<String, Value>, name: &str, target: &mut f32) {
    if let Some(value) = args.get(name).and_then(Value::as_f64) {
        *target = value as f32;
    }
}

fn update_u32(args: &Map<String, Value>, name: &str, target: &mut u32) {
    if let Some(value) = args.get(name).and_then(Value::as_u64) {
        *target = value.min(u32::MAX as u64) as u32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_bridge_parses_http_read_ahead_bytes() {
        for (value, expected) in [
            (Value::Null, 0),
            (json!(0), 0),
            (json!(2_097_152), 2_097_152),
        ] {
            let args = Map::from_iter([("httpReadAheadBytes".to_string(), value)]);
            assert_eq!(optional_read_ahead_bytes(&args), Ok(expected));
        }

        for value in [json!(-1), json!(1.5), json!("2097152")] {
            let args = Map::from_iter([("httpReadAheadBytes".to_string(), value)]);
            assert!(optional_read_ahead_bytes(&args).is_err());
        }
    }

    #[test]
    fn json_bridge_is_exported_on_supported_hosts() {
        let handle = erika_presenter_create();
        assert!(!handle.is_null());

        let response = unsafe {
            erika_presenter_invoke_json(handle, c"getPresenterStats".as_ptr(), c"{}".as_ptr())
        };
        assert!(!response.is_null());
        let value: Value = unsafe { CStr::from_ptr(response) }
            .to_str()
            .ok()
            .and_then(|response| serde_json::from_str(response).ok())
            .expect("JSON bridge returns valid UTF-8 JSON");
        unsafe { erika_string_free(response) };
        assert_eq!(value.get("ok"), Some(&Value::Bool(true)));
        assert!(value.get("value").is_some_and(Value::is_object));

        unsafe { erika_presenter_destroy(handle) };
    }

    #[test]
    fn json_bridge_exposes_resource_status() {
        let handle = erika_presenter_create();
        assert!(!handle.is_null());

        let response = unsafe {
            erika_presenter_invoke_json(handle, c"getResourceStatus".as_ptr(), c"{}".as_ptr())
        };
        assert!(!response.is_null());
        let value: Value = unsafe { CStr::from_ptr(response) }
            .to_str()
            .ok()
            .and_then(|response| serde_json::from_str(response).ok())
            .expect("JSON bridge returns valid UTF-8 JSON");
        unsafe { erika_string_free(response) };
        assert_eq!(value.get("ok"), Some(&Value::Bool(true)));
        let status = value
            .get("value")
            .and_then(Value::as_object)
            .expect("getResourceStatus returns an object");
        for field in [
            "deviceCurrentAllocatedBytes",
            "deviceRecommendedWorkingSetBytes",
            "drawableEstimatedBytes",
            "videoFrameBytes",
            "overlayAtlasBytes",
            "danmakuAtlasBytes",
            "danmakuVertexBufferBytes",
            "upscalerBytes",
            "rendererTrackedBytes",
            "presenterCpuDanmakuAtlasBytes",
            "drawableCount",
            "outputModeSwitches",
        ] {
            assert!(status.contains_key(field), "missing field: {field}");
        }

        unsafe { erika_presenter_destroy(handle) };
    }

    #[test]
    fn json_bridge_exposes_memory_font_selection_and_status() {
        let handle = erika_presenter_create();
        assert!(!handle.is_null());

        for (method, arguments) in [
            (c"selectSubtitleMemoryFonts", c"{\"fontIds\":[]}"),
            (c"getSubtitleMemoryFontStatus", c"{}"),
            (c"clearSubtitleMemoryFonts", c"{}"),
        ] {
            let response =
                unsafe { erika_presenter_invoke_json(handle, method.as_ptr(), arguments.as_ptr()) };
            assert!(!response.is_null());
            let value: Value = unsafe { CStr::from_ptr(response) }
                .to_str()
                .ok()
                .and_then(|response| serde_json::from_str(response).ok())
                .expect("memory font JSON method returns valid JSON");
            unsafe { erika_string_free(response) };
            assert_eq!(value.get("ok"), Some(&Value::Bool(true)));
        }

        unsafe { erika_presenter_destroy(handle) };
    }
}
