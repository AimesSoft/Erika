use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use crossbeam_channel::Receiver;
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
use erika::LumaUpscalerBackendStatus;
use erika::danmaku::{
    DanmakuLayoutConfig, DanmakuShadowStyle, DanmakuTimeline, DanmakuTrackInfo, DanmakuTrackSource,
};
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
use erika::presenter::{PresenterConfig, PresenterRuntime, PresenterRuntimeSnapshot};
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
use erika::renderer::metal::{MetalOutputMode, MetalRendererConfig};
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
use erika::renderer::pipeline::LumaUpscalerMode;
use erika::{
    FlutterTextureHandle, FlutterTextureKind, MediaRequest, MetalSurfaceHandle, PlatformSurface,
    Player, PlayerConfig, PlayerEvent, PlayerState, RendererRuntimeStats, TrackInfo, TrackKind,
    TrackSelection, TrackSource, TransferFunction, WgpuSurfaceHandle, WgpuSurfaceKind,
};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErikaStatus {
    Ok = 0,
    NullPointer = 1,
    InvalidUtf8 = 2,
    PlayerError = 3,
    Panic = 4,
    NoEvent = 5,
}

thread_local! {
    static LAST_ERROR: RefCell<Option<String>> = RefCell::new(None);
}

fn clear_last_error() {
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

fn set_last_error(message: impl Into<String>) {
    let message = message.into().replace('\0', "\\0");
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = Some(message);
    });
}

fn ensure_last_error(status: ErikaStatus) {
    LAST_ERROR.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(format!("Erika C ABI returned {status:?}"));
        }
    });
}

fn finalize_status(status: ErikaStatus) -> ErikaStatus {
    match status {
        ErikaStatus::Ok => clear_last_error(),
        ErikaStatus::NoEvent => {}
        _ => ensure_last_error(status),
    }
    status
}

fn player_error(message: impl Into<String>) -> ErikaStatus {
    set_last_error(message);
    ErikaStatus::PlayerError
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErikaState {
    Idle = 0,
    Opening = 1,
    Ready = 2,
    Playing = 3,
    Paused = 4,
    Stopped = 5,
    Closed = 6,
    Error = 7,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErikaEventKind {
    None = 0,
    StateChanged = 1,
    DurationChanged = 2,
    PositionChanged = 3,
    TracksChanged = 4,
    BufferingChanged = 5,
    VideoParamsChanged = 6,
    SurfaceAttached = 7,
    SurfaceDetached = 8,
    Error = 9,
    TrackSelectionChanged = 10,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErikaTrackKind {
    Video = 0,
    Audio = 1,
    Subtitle = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErikaTrackSource {
    Embedded = 0,
    External = 1,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ErikaTrackSelection {
    pub video: i64,
    pub audio: i64,
    pub subtitle: i64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErikaTrackInfo {
    pub id: i64,
    pub kind: ErikaTrackKind,
    pub source: ErikaTrackSource,
    pub selected: bool,
    pub can_remove: bool,
    pub title: *mut c_char,
    pub language: *mut c_char,
    pub codec: *mut c_char,
}

impl Default for ErikaTrackInfo {
    fn default() -> Self {
        Self {
            id: -1,
            kind: ErikaTrackKind::Video,
            source: ErikaTrackSource::Embedded,
            selected: false,
            can_remove: false,
            title: std::ptr::null_mut(),
            language: std::ptr::null_mut(),
            codec: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErikaWgpuSurfaceKind {
    Unknown = 0,
    MacOsNsView = 1,
    MacOsCaMetalLayer = 2,
    IosUiView = 3,
    WindowsHwnd = 4,
    XlibWindow = 5,
    WaylandSurface = 6,
    AndroidNativeWindow = 7,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErikaFlutterTextureKind {
    Unknown = 0,
    MacOsTextureRegistrar = 1,
    IosTextureRegistrar = 2,
    AndroidSurfaceTexture = 3,
    WindowsTextureRegistrar = 4,
    LinuxTextureRegistrar = 5,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErikaPresenterOutputMode {
    Sdr = 0,
    AppleEdr = 1,
}

impl ErikaPresenterOutputMode {
    fn from_raw(value: i32) -> Self {
        match value {
            1 => Self::AppleEdr,
            _ => Self::Sdr,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErikaLumaUpscalerMode {
    Off = 0,
    ArtCnnC4F16 = 1,
    ArtCnnC4F32 = 2,
}

impl ErikaLumaUpscalerMode {
    fn from_raw(value: i32) -> Self {
        match value {
            1 => Self::ArtCnnC4F16,
            2 => Self::ArtCnnC4F32,
            _ => Self::Off,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErikaUpscalerBackendStatus {
    Off = 0,
    Inactive = 1,
    Building = 2,
    Scalar = 3,
    SimdgroupMatrix = 4,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ErikaPresenterConfig {
    pub output_mode: i32,
    pub edr_headroom: f32,
    pub luma_upscaler: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ErikaUpscalerStatus {
    pub requested_mode: i32,
    pub active_backend: i32,
    pub fallback_count: u64,
    pub upscaled_frames: u64,
    pub last_encode_micros: u64,
    pub last_gpu_micros: u64,
}

impl Default for ErikaPresenterConfig {
    fn default() -> Self {
        Self {
            output_mode: ErikaPresenterOutputMode::Sdr as i32,
            edr_headroom: 1.0,
            luma_upscaler: ErikaLumaUpscalerMode::Off as i32,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ErikaDanmakuConfig {
    pub enabled: bool,
    /// NipaPlay/Flutter logical danmaku font size. Erika uses the NipaPlay
    /// default danmaku font and multiplies by the surface scale for glyph pixels.
    pub font_size: f32,
    pub opacity: f32,
    pub display_area: f32,
    pub scroll_duration_seconds: f32,
    pub scroll_speed_factor: f32,
    pub track_gap_ratio: f32,
    pub outline_width: f32,
    pub shadow_offset_x: f32,
    pub shadow_offset_y: f32,
    pub merge_duplicates: bool,
    pub allow_stacking: bool,
    pub allow_scroll_overwrite: bool,
    pub max_quantity: u32,
    pub max_lines_per_mode: u32,
    pub block_top: bool,
    pub block_bottom: bool,
    pub block_scroll: bool,
    pub shadow_style: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErikaDanmakuTrackInfo {
    pub id: u64,
    pub enabled: bool,
    pub offset_micros: i64,
    pub item_count: usize,
    pub name: *mut c_char,
    pub source: *mut c_char,
}

impl Default for ErikaDanmakuTrackInfo {
    fn default() -> Self {
        Self {
            id: 0,
            enabled: false,
            offset_micros: 0,
            item_count: 0,
            name: std::ptr::null_mut(),
            source: std::ptr::null_mut(),
        }
    }
}

impl Default for ErikaDanmakuConfig {
    fn default() -> Self {
        let config = DanmakuLayoutConfig::default();
        Self {
            enabled: config.enabled,
            font_size: config.font_size,
            opacity: config.opacity,
            display_area: config.display_area,
            scroll_duration_seconds: config.scroll_duration_seconds,
            scroll_speed_factor: config.scroll_speed_factor,
            track_gap_ratio: config.track_gap_ratio,
            outline_width: config.outline_width,
            shadow_offset_x: config.shadow_offset[0],
            shadow_offset_y: config.shadow_offset[1],
            merge_duplicates: config.merge_duplicates,
            allow_stacking: config.allow_stacking,
            allow_scroll_overwrite: config.allow_scroll_overwrite,
            max_quantity: 0,
            max_lines_per_mode: 0,
            block_top: config.block_top,
            block_bottom: config.block_bottom,
            block_scroll: config.block_scroll,
            shadow_style: config.shadow_style.code(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ErikaVideoParams {
    pub width: u32,
    pub height: u32,
    pub primaries: u32,
    pub transfer: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ErikaTrackCounts {
    pub video: u32,
    pub audio: u32,
    pub subtitle: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErikaEvent {
    pub kind: ErikaEventKind,
    pub status: ErikaStatus,
    pub state: ErikaState,
    pub duration_micros: i64,
    pub position_micros: u64,
    pub buffering: bool,
    pub video: ErikaVideoParams,
    pub tracks: ErikaTrackCounts,
}

impl Default for ErikaEvent {
    fn default() -> Self {
        Self {
            kind: ErikaEventKind::None,
            status: ErikaStatus::Ok,
            state: ErikaState::Idle,
            duration_micros: -1,
            position_micros: 0,
            buffering: false,
            video: ErikaVideoParams::default(),
            tracks: ErikaTrackCounts::default(),
        }
    }
}

pub struct ErikaHandle {
    player: Player,
    events: Receiver<PlayerEvent>,
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
pub struct ErikaPresenterHandle {
    presenter: PresenterRuntime,
    events: Receiver<PlayerEvent>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ErikaPresenterStats {
    pub decoded_video_frames: u64,
    pub rendered_video_frames: u64,
    pub rendered_test_frames: u64,
    pub pushed_audio_frames: u64,
    pub overlay_frames: u64,
    pub danmaku_frames: u64,
    pub danmaku_items: u64,
    pub import_failures: u64,
    pub render_failures: u64,
    pub audio_failures: u64,
    pub software_video_frames: u64,
    pub hardware_video_frames: u64,
    pub zero_copy_video_frames: u64,
    pub cpu_video_frame_fallbacks: u64,
    pub last_render_micros: u64,
    pub last_render_current_micros: u64,
    pub audio_clock_read_frames: u64,
    pub audio_clock_queued_frames: u64,
    pub audio_clock_underflow_frames: u64,
    pub direct_zero_copy_video_frames: u64,
    pub shared_handle_video_frames: u64,
    pub hdr_source_frames: u64,
    pub hdr10_output_frames: u64,
    pub sdr_tonemap_frames: u64,
    pub hdr10_metadata_updates: u64,
    pub hdr10_metadata_failures: u64,
    pub hdr10_output_failures: u64,
    pub hdr10_output_active: bool,
}

#[unsafe(no_mangle)]
pub extern "C" fn erika_create() -> *mut ErikaHandle {
    let player = Player::new(PlayerConfig::default());
    let events = player.subscribe();
    Box::into_raw(Box::new(ErikaHandle { player, events }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_destroy(handle: *mut ErikaHandle) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn erika_last_error_message() -> *mut c_char {
    LAST_ERROR.with(|slot| option_string_to_c(slot.borrow().as_deref()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_string_free(value: *mut c_char) {
    if value.is_null() {
        return;
    }
    unsafe { drop(CString::from_raw(value)) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_open(handle: *mut ErikaHandle, uri: *const c_char) -> ErikaStatus {
    with_handle_mut(handle, |handle| {
        let uri = match c_string(uri) {
            Ok(uri) => uri,
            Err(status) => return status,
        };
        status_from_player_result(handle.player.open(MediaRequest::new(uri)))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_play(handle: *mut ErikaHandle) -> ErikaStatus {
    with_handle_mut(handle, |handle| {
        status_from_player_result(handle.player.play())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_pause(handle: *mut ErikaHandle) -> ErikaStatus {
    with_handle_mut(handle, |handle| {
        status_from_player_result(handle.player.pause())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_stop(handle: *mut ErikaHandle) -> ErikaStatus {
    with_handle_mut(handle, |handle| {
        status_from_player_result(handle.player.stop())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_close(handle: *mut ErikaHandle) -> ErikaStatus {
    with_handle_mut(handle, |handle| {
        status_from_player_result(handle.player.close())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_seek(handle: *mut ErikaHandle, position_micros: u64) -> ErikaStatus {
    with_handle_mut(handle, |handle| {
        status_from_player_result(handle.player.seek(Duration::from_micros(position_micros)))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_add_external_subtitle(
    handle: *mut ErikaHandle,
    uri: *const c_char,
    out_track_id: *mut i64,
) -> ErikaStatus {
    if out_track_id.is_null() {
        return ErikaStatus::NullPointer;
    }
    with_handle_mut(handle, |handle| {
        let uri = match c_string(uri) {
            Ok(uri) => uri,
            Err(status) => return status,
        };
        match handle.player.add_external_subtitle(uri) {
            Ok(track) => {
                unsafe { *out_track_id = track.id };
                ErikaStatus::Ok
            }
            Err(_) => ErikaStatus::PlayerError,
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_remove_subtitle_track(
    handle: *mut ErikaHandle,
    track_id: i64,
) -> ErikaStatus {
    with_handle_mut(handle, |handle| {
        status_from_player_result(handle.player.remove_subtitle_track(track_id))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_select_audio_track(
    handle: *mut ErikaHandle,
    track_id: i64,
) -> ErikaStatus {
    with_handle_mut(handle, |handle| {
        status_from_player_result(handle.player.select_audio_track(track_id_option(track_id)))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_select_subtitle_track(
    handle: *mut ErikaHandle,
    track_id: i64,
) -> ErikaStatus {
    with_handle_mut(handle, |handle| {
        status_from_player_result(
            handle
                .player
                .select_subtitle_track(track_id_option(track_id)),
        )
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_track_selection(
    handle: *mut ErikaHandle,
    out_selection: *mut ErikaTrackSelection,
) -> ErikaStatus {
    if out_selection.is_null() {
        return ErikaStatus::NullPointer;
    }
    with_handle_mut(handle, |handle| {
        unsafe { *out_selection = track_selection_to_c(handle.player.track_selection()) };
        ErikaStatus::Ok
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_tracks(
    handle: *mut ErikaHandle,
    out_tracks: *mut ErikaTrackInfo,
    capacity: usize,
    out_len: *mut usize,
) -> ErikaStatus {
    if out_len.is_null() || (capacity > 0 && out_tracks.is_null()) {
        return ErikaStatus::NullPointer;
    }
    with_handle_mut(handle, |handle| {
        write_tracks_to_c(&handle.player.tracks(), out_tracks, capacity, out_len)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_track_info_free(track: *mut ErikaTrackInfo) {
    if track.is_null() {
        return;
    }
    let track = unsafe { &mut *track };
    free_c_string(&mut track.title);
    free_c_string(&mut track.language);
    free_c_string(&mut track.codec);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_danmaku_track_info_free(track: *mut ErikaDanmakuTrackInfo) {
    if track.is_null() {
        return;
    }
    let track = unsafe { &mut *track };
    free_c_string(&mut track.name);
    free_c_string(&mut track.source);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_attach_metal_layer(
    handle: *mut ErikaHandle,
    raw_layer: u64,
    width: u32,
    height: u32,
    scale: f64,
) -> ErikaStatus {
    if raw_layer == 0 {
        set_last_error("metal layer pointer is null");
        return ErikaStatus::NullPointer;
    }
    with_handle_mut(handle, |handle| {
        status_from_player_result(handle.player.attach_surface(PlatformSurface::Metal(
            MetalSurfaceHandle::new(raw_layer, width, height, scale),
        )))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_attach_wgpu_surface(
    handle: *mut ErikaHandle,
    kind: ErikaWgpuSurfaceKind,
    raw_window: u64,
    raw_display: u64,
    width: u32,
    height: u32,
    scale: f64,
) -> ErikaStatus {
    if raw_window == 0 {
        set_last_error("surface window pointer is null");
        return ErikaStatus::NullPointer;
    }
    with_handle_mut(handle, |handle| {
        status_from_player_result(handle.player.attach_surface(PlatformSurface::Wgpu(
            WgpuSurfaceHandle::new(
                wgpu_surface_kind_from_c(kind),
                raw_window,
                raw_display,
                width,
                height,
                scale,
            ),
        )))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_attach_flutter_texture(
    handle: *mut ErikaHandle,
    kind: ErikaFlutterTextureKind,
    texture_id: i64,
    width: u32,
    height: u32,
    scale: f64,
) -> ErikaStatus {
    if texture_id < 0 {
        return ErikaStatus::NullPointer;
    }
    with_handle_mut(handle, |handle| {
        status_from_player_result(
            handle
                .player
                .attach_surface(PlatformSurface::FlutterTexture(FlutterTextureHandle::new(
                    flutter_texture_kind_from_c(kind),
                    texture_id,
                    width,
                    height,
                    scale,
                ))),
        )
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_detach_surface(handle: *mut ErikaHandle) -> ErikaStatus {
    with_handle_mut(handle, |handle| {
        status_from_player_result(handle.player.detach_surface())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_state(
    handle: *mut ErikaHandle,
    out_state: *mut ErikaState,
) -> ErikaStatus {
    if out_state.is_null() {
        return ErikaStatus::NullPointer;
    }
    with_handle_mut(handle, |handle| {
        unsafe { *out_state = state_to_c(handle.player.state()) };
        ErikaStatus::Ok
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_poll_event(
    handle: *mut ErikaHandle,
    out_event: *mut ErikaEvent,
) -> ErikaStatus {
    if out_event.is_null() {
        return ErikaStatus::NullPointer;
    }
    with_handle_mut(handle, |handle| match handle.events.try_recv() {
        Ok(event) => {
            unsafe { *out_event = event_to_c(event) };
            ErikaStatus::Ok
        }
        Err(crossbeam_channel::TryRecvError::Empty) => ErikaStatus::NoEvent,
        Err(crossbeam_channel::TryRecvError::Disconnected) => ErikaStatus::PlayerError,
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub extern "C" fn erika_presenter_create() -> *mut ErikaPresenterHandle {
    create_presenter_handle(PresenterConfig::default())
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub extern "C" fn erika_presenter_create() -> *mut std::ffi::c_void {
    std::ptr::null_mut()
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub extern "C" fn erika_presenter_create_with_config(
    config: ErikaPresenterConfig,
) -> *mut ErikaPresenterHandle {
    create_presenter_handle(presenter_config_from_c(config))
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub extern "C" fn erika_presenter_create_with_output_mode(
    output_mode: i32,
    edr_headroom: f32,
) -> *mut ErikaPresenterHandle {
    create_presenter_handle(presenter_config_from_c(ErikaPresenterConfig {
        output_mode,
        edr_headroom,
        ..ErikaPresenterConfig::default()
    }))
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub extern "C" fn erika_presenter_create_with_config(
    _config: ErikaPresenterConfig,
) -> *mut std::ffi::c_void {
    std::ptr::null_mut()
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub extern "C" fn erika_presenter_create_with_output_mode(
    _output_mode: i32,
    _edr_headroom: f32,
) -> *mut std::ffi::c_void {
    std::ptr::null_mut()
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
fn create_presenter_handle(config: PresenterConfig) -> *mut ErikaPresenterHandle {
    match PresenterRuntime::new(config) {
        Ok(presenter) => {
            clear_last_error();
            let events = presenter.player().subscribe();
            Box::into_raw(Box::new(ErikaPresenterHandle { presenter, events }))
        }
        Err(error) => {
            set_last_error(format!("presenter create failed: {error}"));
            std::ptr::null_mut()
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
fn presenter_config_from_c(config: ErikaPresenterConfig) -> PresenterConfig {
    let output_mode = match ErikaPresenterOutputMode::from_raw(config.output_mode) {
        ErikaPresenterOutputMode::AppleEdr => {
            let headroom = if config.edr_headroom.is_finite() {
                config.edr_headroom
            } else {
                1.0
            };
            MetalOutputMode::apple_edr(headroom)
        }
        ErikaPresenterOutputMode::Sdr => MetalOutputMode::Sdr,
    };

    PresenterConfig {
        renderer: MetalRendererConfig {
            output_mode,
            luma_upscaler: luma_upscaler_mode_from_c(config.luma_upscaler),
            ..MetalRendererConfig::default()
        },
        ..PresenterConfig::default()
    }
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
fn luma_upscaler_mode_from_c(mode: i32) -> LumaUpscalerMode {
    match ErikaLumaUpscalerMode::from_raw(mode) {
        ErikaLumaUpscalerMode::Off => LumaUpscalerMode::Off,
        ErikaLumaUpscalerMode::ArtCnnC4F16 => LumaUpscalerMode::ArtCnnC4F16,
        ErikaLumaUpscalerMode::ArtCnnC4F32 => LumaUpscalerMode::ArtCnnC4F32,
    }
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
fn luma_upscaler_mode_to_c(mode: LumaUpscalerMode) -> i32 {
    match mode {
        LumaUpscalerMode::Off => ErikaLumaUpscalerMode::Off as i32,
        LumaUpscalerMode::ArtCnnC4F16 => ErikaLumaUpscalerMode::ArtCnnC4F16 as i32,
        LumaUpscalerMode::ArtCnnC4F32 => ErikaLumaUpscalerMode::ArtCnnC4F32 as i32,
    }
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
fn upscaler_backend_status_to_c(status: LumaUpscalerBackendStatus) -> i32 {
    match status {
        LumaUpscalerBackendStatus::Off => ErikaUpscalerBackendStatus::Off as i32,
        LumaUpscalerBackendStatus::Inactive => ErikaUpscalerBackendStatus::Inactive as i32,
        LumaUpscalerBackendStatus::Building => ErikaUpscalerBackendStatus::Building as i32,
        LumaUpscalerBackendStatus::Scalar => ErikaUpscalerBackendStatus::Scalar as i32,
        LumaUpscalerBackendStatus::SimdgroupMatrix => {
            ErikaUpscalerBackendStatus::SimdgroupMatrix as i32
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
fn upscaler_status_to_c(stats: RendererRuntimeStats) -> ErikaUpscalerStatus {
    ErikaUpscalerStatus {
        requested_mode: luma_upscaler_mode_to_c(stats.upscaler_mode),
        active_backend: upscaler_backend_status_to_c(stats.upscaler_backend),
        fallback_count: stats.upscaler_fallbacks,
        upscaled_frames: stats.upscaled_frames,
        last_encode_micros: duration_micros_u64(stats.last_upscaler_encode_duration),
        last_gpu_micros: duration_micros_u64(stats.last_gpu_duration),
    }
}

fn danmaku_config_from_c(
    config: ErikaDanmakuConfig,
    base: &DanmakuLayoutConfig,
) -> DanmakuLayoutConfig {
    DanmakuLayoutConfig {
        enabled: config.enabled,
        font_size: config.font_size,
        opacity: config.opacity,
        display_area: config.display_area,
        scroll_duration_seconds: config.scroll_duration_seconds,
        scroll_speed_factor: config.scroll_speed_factor,
        track_gap_ratio: config.track_gap_ratio,
        outline_width: config.outline_width,
        shadow_offset: [config.shadow_offset_x, config.shadow_offset_y],
        merge_duplicates: config.merge_duplicates,
        allow_stacking: config.allow_stacking,
        allow_scroll_overwrite: config.allow_scroll_overwrite,
        max_quantity: (config.max_quantity > 0).then_some(config.max_quantity),
        max_lines_per_mode: (config.max_lines_per_mode > 0).then_some(config.max_lines_per_mode),
        block_top: config.block_top,
        block_bottom: config.block_bottom,
        block_scroll: config.block_scroll,
        block_words: base.block_words.clone(),
        shadow_style: DanmakuShadowStyle::from_code(config.shadow_style),
        custom_font_family: base.custom_font_family.clone(),
        custom_font_file_path: base.custom_font_file_path.clone(),
    }
}

fn danmaku_config_to_c(config: &DanmakuLayoutConfig) -> ErikaDanmakuConfig {
    ErikaDanmakuConfig {
        enabled: config.enabled,
        font_size: config.font_size,
        opacity: config.opacity,
        display_area: config.display_area,
        scroll_duration_seconds: config.scroll_duration_seconds,
        scroll_speed_factor: config.scroll_speed_factor,
        track_gap_ratio: config.track_gap_ratio,
        outline_width: config.outline_width,
        shadow_offset_x: config.shadow_offset[0],
        shadow_offset_y: config.shadow_offset[1],
        merge_duplicates: config.merge_duplicates,
        allow_stacking: config.allow_stacking,
        allow_scroll_overwrite: config.allow_scroll_overwrite,
        max_quantity: config.max_quantity.unwrap_or(0),
        max_lines_per_mode: config.max_lines_per_mode.unwrap_or(0),
        block_top: config.block_top,
        block_bottom: config.block_bottom,
        block_scroll: config.block_scroll,
        shadow_style: config.shadow_style.code(),
    }
}

fn danmaku_block_words_from_json(json: &str) -> Result<Vec<String>, ErikaStatus> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|_| ErikaStatus::PlayerError)?;
    match value {
        serde_json::Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                serde_json::Value::String(value) => Ok(value),
                _ => Err(ErikaStatus::PlayerError),
            })
            .collect(),
        serde_json::Value::String(value) => Ok(vec![value]),
        _ => Err(ErikaStatus::PlayerError),
    }
}

#[cfg(all(
    any(target_os = "macos", target_os = "ios", target_os = "windows"),
    test
))]
fn metal_output_mode_from_c(config: ErikaPresenterConfig) -> MetalOutputMode {
    presenter_config_from_c(config).renderer.output_mode
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_destroy(handle: *mut ErikaPresenterHandle) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_destroy(_handle: *mut std::ffi::c_void) {}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_open(
    handle: *mut ErikaPresenterHandle,
    uri: *const c_char,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        let uri = match c_string(uri) {
            Ok(uri) => uri,
            Err(status) => return status,
        };
        status_from_player_result(handle.presenter.open(MediaRequest::new(uri)))
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_play(handle: *mut ErikaPresenterHandle) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        status_from_player_result(handle.presenter.play())
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_pause(handle: *mut ErikaPresenterHandle) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        status_from_player_result(handle.presenter.pause())
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_stop(handle: *mut ErikaPresenterHandle) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        status_from_player_result(handle.presenter.stop())
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_close(handle: *mut ErikaPresenterHandle) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        status_from_player_result(handle.presenter.close())
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_seek(
    handle: *mut ErikaPresenterHandle,
    position_micros: u64,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        status_from_player_result(
            handle
                .presenter
                .seek(Duration::from_micros(position_micros)),
        )
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_playback_rate(
    handle: *mut ErikaPresenterHandle,
    rate: f64,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        status_from_player_result(handle.presenter.set_playback_rate(rate))
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_volume(
    handle: *mut ErikaPresenterHandle,
    volume: f64,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        handle.presenter.set_volume(volume);
        ErikaStatus::Ok
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_upscaler(
    handle: *mut ErikaPresenterHandle,
    mode: i32,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        handle
            .presenter
            .set_luma_upscaler(luma_upscaler_mode_from_c(mode));
        ErikaStatus::Ok
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_subtitle_scale(
    handle: *mut ErikaPresenterHandle,
    scale: f64,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        handle.presenter.set_subtitle_scale(scale);
        ErikaStatus::Ok
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_get_upscaler_status(
    handle: *mut ErikaPresenterHandle,
    out_status: *mut ErikaUpscalerStatus,
) -> ErikaStatus {
    if out_status.is_null() {
        return ErikaStatus::NullPointer;
    }
    with_presenter_mut(handle, |handle| {
        let status = upscaler_status_to_c(handle.presenter.runtime_snapshot().renderer);
        unsafe { *out_status = status };
        ErikaStatus::Ok
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_add_external_subtitle(
    handle: *mut ErikaPresenterHandle,
    uri: *const c_char,
    out_track_id: *mut i64,
) -> ErikaStatus {
    if out_track_id.is_null() {
        return ErikaStatus::NullPointer;
    }
    with_presenter_mut(handle, |handle| {
        let uri = match c_string(uri) {
            Ok(uri) => uri,
            Err(status) => return status,
        };
        match handle.presenter.add_external_subtitle(uri) {
            Ok(track) => {
                unsafe { *out_track_id = track.id };
                ErikaStatus::Ok
            }
            Err(_) => ErikaStatus::PlayerError,
        }
    })
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_add_external_subtitle(
    _handle: *mut std::ffi::c_void,
    _uri: *const c_char,
    _out_track_id: *mut i64,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_remove_subtitle_track(
    handle: *mut ErikaPresenterHandle,
    track_id: i64,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        status_from_player_result(handle.presenter.remove_subtitle_track(track_id))
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_select_audio_track(
    handle: *mut ErikaPresenterHandle,
    track_id: i64,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        status_from_player_result(
            handle
                .presenter
                .select_audio_track(track_id_option(track_id)),
        )
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_select_subtitle_track(
    handle: *mut ErikaPresenterHandle,
    track_id: i64,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        status_from_player_result(
            handle
                .presenter
                .select_subtitle_track(track_id_option(track_id)),
        )
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_load_danmaku_file(
    handle: *mut ErikaPresenterHandle,
    uri: *const c_char,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        let uri = match c_string(uri) {
            Ok(uri) => uri,
            Err(status) => return status,
        };
        match DanmakuTimeline::from_file(uri) {
            Ok(timeline) => {
                handle.presenter.set_danmaku_timeline(timeline);
                ErikaStatus::Ok
            }
            Err(_) => ErikaStatus::PlayerError,
        }
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_load_danmaku_json(
    handle: *mut ErikaPresenterHandle,
    json: *const c_char,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        let json = match c_string(json) {
            Ok(json) => json,
            Err(status) => return status,
        };
        match DanmakuTimeline::parse_auto(&json) {
            Ok(timeline) => {
                handle.presenter.set_danmaku_timeline(timeline);
                ErikaStatus::Ok
            }
            Err(_) => ErikaStatus::PlayerError,
        }
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_add_danmaku_track_file(
    handle: *mut ErikaPresenterHandle,
    uri: *const c_char,
    name: *const c_char,
    offset_micros: i64,
    out_track_id: *mut u64,
) -> ErikaStatus {
    if out_track_id.is_null() {
        return ErikaStatus::NullPointer;
    }
    with_presenter_mut(handle, |handle| {
        let uri = match c_string(uri) {
            Ok(uri) => uri,
            Err(status) => return status,
        };
        let name = optional_c_string(name).unwrap_or_else(|| danmaku_track_name_from_uri(&uri));
        match DanmakuTimeline::from_file(&uri) {
            Ok(timeline) => {
                let track_id = handle.presenter.add_danmaku_track(
                    timeline,
                    name,
                    DanmakuTrackSource::File(std::path::PathBuf::from(uri)),
                    offset_micros,
                );
                unsafe { *out_track_id = track_id };
                ErikaStatus::Ok
            }
            Err(_) => ErikaStatus::PlayerError,
        }
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_add_danmaku_track_json(
    handle: *mut ErikaPresenterHandle,
    json: *const c_char,
    name: *const c_char,
    offset_micros: i64,
    out_track_id: *mut u64,
) -> ErikaStatus {
    if out_track_id.is_null() {
        return ErikaStatus::NullPointer;
    }
    with_presenter_mut(handle, |handle| {
        let json = match c_string(json) {
            Ok(json) => json,
            Err(status) => return status,
        };
        let name = optional_c_string(name).unwrap_or_else(|| "danmaku".to_string());
        match DanmakuTimeline::parse_auto(&json) {
            Ok(timeline) => {
                let track_id = handle.presenter.add_danmaku_track(
                    timeline,
                    name,
                    DanmakuTrackSource::Json,
                    offset_micros,
                );
                unsafe { *out_track_id = track_id };
                ErikaStatus::Ok
            }
            Err(_) => ErikaStatus::PlayerError,
        }
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_remove_danmaku_track(
    handle: *mut ErikaPresenterHandle,
    track_id: u64,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        if handle.presenter.remove_danmaku_track(track_id) {
            ErikaStatus::Ok
        } else {
            ErikaStatus::PlayerError
        }
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_danmaku_track_enabled(
    handle: *mut ErikaPresenterHandle,
    track_id: u64,
    enabled: bool,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        if handle
            .presenter
            .set_danmaku_track_enabled(track_id, enabled)
        {
            ErikaStatus::Ok
        } else {
            ErikaStatus::PlayerError
        }
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_danmaku_track_offset(
    handle: *mut ErikaPresenterHandle,
    track_id: u64,
    offset_micros: i64,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        if handle
            .presenter
            .set_danmaku_track_offset(track_id, offset_micros)
        {
            ErikaStatus::Ok
        } else {
            ErikaStatus::PlayerError
        }
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_danmaku_global_offset(
    handle: *mut ErikaPresenterHandle,
    offset_micros: i64,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        handle.presenter.set_danmaku_global_offset(offset_micros);
        ErikaStatus::Ok
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_danmaku_tracks(
    handle: *mut ErikaPresenterHandle,
    out_tracks: *mut ErikaDanmakuTrackInfo,
    capacity: usize,
    out_len: *mut usize,
) -> ErikaStatus {
    if out_len.is_null() || (capacity > 0 && out_tracks.is_null()) {
        return ErikaStatus::NullPointer;
    }
    with_presenter_mut(handle, |handle| {
        write_danmaku_tracks_to_c(
            &handle.presenter.danmaku_tracks(),
            out_tracks,
            capacity,
            out_len,
        )
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_clear_danmaku(
    handle: *mut ErikaPresenterHandle,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        handle.presenter.clear_danmaku();
        ErikaStatus::Ok
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_danmaku_enabled(
    handle: *mut ErikaPresenterHandle,
    enabled: bool,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        handle.presenter.set_danmaku_enabled(enabled);
        ErikaStatus::Ok
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_danmaku_config(
    handle: *mut ErikaPresenterHandle,
    config: ErikaDanmakuConfig,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        let base = handle
            .presenter
            .danmaku_config()
            .cloned()
            .unwrap_or_default();
        handle
            .presenter
            .set_danmaku_config(danmaku_config_from_c(config, &base));
        ErikaStatus::Ok
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_danmaku_config_ptr(
    handle: *mut ErikaPresenterHandle,
    config: *const ErikaDanmakuConfig,
) -> ErikaStatus {
    if config.is_null() {
        return ErikaStatus::NullPointer;
    }
    unsafe { erika_presenter_set_danmaku_config(handle, *config) }
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_get_danmaku_config(
    handle: *mut ErikaPresenterHandle,
    out_config: *mut ErikaDanmakuConfig,
) -> ErikaStatus {
    if out_config.is_null() {
        return ErikaStatus::NullPointer;
    }
    with_presenter_mut(handle, |handle| {
        let config = handle
            .presenter
            .danmaku_config()
            .map(danmaku_config_to_c)
            .unwrap_or_default();
        unsafe {
            *out_config = config;
        }
        ErikaStatus::Ok
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_danmaku_font(
    handle: *mut ErikaPresenterHandle,
    family: *const c_char,
    file_path: *const c_char,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        let family = optional_c_string(family).unwrap_or_default();
        let file_path = optional_c_string(file_path).unwrap_or_default();
        handle.presenter.set_danmaku_font(family, file_path);
        ErikaStatus::Ok
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_danmaku_block_words_json(
    handle: *mut ErikaPresenterHandle,
    json: *const c_char,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        let json = match c_string(json) {
            Ok(json) => json,
            Err(status) => return status,
        };
        let block_words = match danmaku_block_words_from_json(&json) {
            Ok(words) => words,
            Err(status) => return status,
        };
        let mut config = handle
            .presenter
            .danmaku_config()
            .cloned()
            .unwrap_or_default();
        config.block_words = block_words;
        handle.presenter.set_danmaku_config(config);
        ErikaStatus::Ok
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_track_selection(
    handle: *mut ErikaPresenterHandle,
    out_selection: *mut ErikaTrackSelection,
) -> ErikaStatus {
    if out_selection.is_null() {
        return ErikaStatus::NullPointer;
    }
    with_presenter_mut(handle, |handle| {
        unsafe { *out_selection = track_selection_to_c(handle.presenter.track_selection()) };
        ErikaStatus::Ok
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_tracks(
    handle: *mut ErikaPresenterHandle,
    out_tracks: *mut ErikaTrackInfo,
    capacity: usize,
    out_len: *mut usize,
) -> ErikaStatus {
    if out_len.is_null() || (capacity > 0 && out_tracks.is_null()) {
        return ErikaStatus::NullPointer;
    }
    with_presenter_mut(handle, |handle| {
        write_tracks_to_c(&handle.presenter.tracks(), out_tracks, capacity, out_len)
    })
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_remove_subtitle_track(
    _handle: *mut std::ffi::c_void,
    _track_id: i64,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_select_audio_track(
    _handle: *mut std::ffi::c_void,
    _track_id: i64,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_select_subtitle_track(
    _handle: *mut std::ffi::c_void,
    _track_id: i64,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_track_selection(
    _handle: *mut std::ffi::c_void,
    _out_selection: *mut ErikaTrackSelection,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_tracks(
    _handle: *mut std::ffi::c_void,
    _out_tracks: *mut ErikaTrackInfo,
    _capacity: usize,
    _out_len: *mut usize,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_playback_rate(
    _handle: *mut std::ffi::c_void,
    _rate: f64,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_volume(
    _handle: *mut std::ffi::c_void,
    _volume: f64,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_upscaler(
    _handle: *mut std::ffi::c_void,
    _mode: i32,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_subtitle_scale(
    _handle: *mut std::ffi::c_void,
    _scale: f64,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_get_upscaler_status(
    _handle: *mut std::ffi::c_void,
    out_status: *mut ErikaUpscalerStatus,
) -> ErikaStatus {
    if out_status.is_null() {
        return ErikaStatus::NullPointer;
    }
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_load_danmaku_file(
    _handle: *mut std::ffi::c_void,
    _uri: *const c_char,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_load_danmaku_json(
    _handle: *mut std::ffi::c_void,
    _json: *const c_char,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_add_danmaku_track_file(
    _handle: *mut std::ffi::c_void,
    uri: *const c_char,
    _name: *const c_char,
    _offset_micros: i64,
    out_track_id: *mut u64,
) -> ErikaStatus {
    if uri.is_null() || out_track_id.is_null() {
        return ErikaStatus::NullPointer;
    }
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_add_danmaku_track_json(
    _handle: *mut std::ffi::c_void,
    json: *const c_char,
    _name: *const c_char,
    _offset_micros: i64,
    out_track_id: *mut u64,
) -> ErikaStatus {
    if json.is_null() || out_track_id.is_null() {
        return ErikaStatus::NullPointer;
    }
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_remove_danmaku_track(
    _handle: *mut std::ffi::c_void,
    _track_id: u64,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_danmaku_track_enabled(
    _handle: *mut std::ffi::c_void,
    _track_id: u64,
    _enabled: bool,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_danmaku_track_offset(
    _handle: *mut std::ffi::c_void,
    _track_id: u64,
    _offset_micros: i64,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_danmaku_global_offset(
    _handle: *mut std::ffi::c_void,
    _offset_micros: i64,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_danmaku_tracks(
    _handle: *mut std::ffi::c_void,
    out_tracks: *mut ErikaDanmakuTrackInfo,
    capacity: usize,
    out_len: *mut usize,
) -> ErikaStatus {
    if out_len.is_null() || (capacity > 0 && out_tracks.is_null()) {
        return ErikaStatus::NullPointer;
    }
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_clear_danmaku(
    _handle: *mut std::ffi::c_void,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_danmaku_enabled(
    _handle: *mut std::ffi::c_void,
    _enabled: bool,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_danmaku_config(
    _handle: *mut std::ffi::c_void,
    _config: ErikaDanmakuConfig,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_danmaku_config_ptr(
    _handle: *mut std::ffi::c_void,
    config: *const ErikaDanmakuConfig,
) -> ErikaStatus {
    if config.is_null() {
        return ErikaStatus::NullPointer;
    }
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_get_danmaku_config(
    _handle: *mut std::ffi::c_void,
    out_config: *mut ErikaDanmakuConfig,
) -> ErikaStatus {
    if out_config.is_null() {
        return ErikaStatus::NullPointer;
    }
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_danmaku_font(
    _handle: *mut std::ffi::c_void,
    _family: *const c_char,
    _file_path: *const c_char,
) -> ErikaStatus {
    ErikaStatus::PlayerError
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_set_danmaku_block_words_json(
    _handle: *mut std::ffi::c_void,
    json: *const c_char,
) -> ErikaStatus {
    if json.is_null() {
        return ErikaStatus::NullPointer;
    }
    ErikaStatus::PlayerError
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_attach_metal_layer(
    handle: *mut ErikaPresenterHandle,
    raw_layer: u64,
    width: u32,
    height: u32,
    scale: f64,
) -> ErikaStatus {
    if raw_layer == 0 {
        return ErikaStatus::NullPointer;
    }
    with_presenter_mut(handle, |handle| {
        status_from_player_result(handle.presenter.attach_surface(PlatformSurface::Metal(
            MetalSurfaceHandle::new(raw_layer, width, height, scale),
        )))
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_attach_wgpu_surface(
    handle: *mut ErikaPresenterHandle,
    kind: ErikaWgpuSurfaceKind,
    raw_window: u64,
    raw_display: u64,
    width: u32,
    height: u32,
    scale: f64,
) -> ErikaStatus {
    if raw_window == 0 {
        return ErikaStatus::NullPointer;
    }
    with_presenter_mut(handle, |handle| {
        status_from_player_result(handle.presenter.attach_surface(PlatformSurface::Wgpu(
            WgpuSurfaceHandle::new(
                wgpu_surface_kind_from_c(kind),
                raw_window,
                raw_display,
                width,
                height,
                scale,
            ),
        )))
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_attach_windows_hwnd(
    handle: *mut ErikaPresenterHandle,
    hwnd: u64,
    hinstance: u64,
    width: u32,
    height: u32,
    scale: f64,
) -> ErikaStatus {
    unsafe {
        erika_presenter_attach_wgpu_surface(
            handle,
            ErikaWgpuSurfaceKind::WindowsHwnd,
            hwnd,
            hinstance,
            width,
            height,
            scale,
        )
    }
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_resize_surface(
    handle: *mut ErikaPresenterHandle,
    width: u32,
    height: u32,
    scale: f64,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        status_from_player_result(handle.presenter.resize_surface(width, height, scale))
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_detach_surface(
    handle: *mut ErikaPresenterHandle,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        status_from_player_result(handle.presenter.detach_surface())
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_render_tick(
    handle: *mut ErikaPresenterHandle,
    time_seconds: f64,
    out_stats: *mut ErikaPresenterStats,
) -> ErikaStatus {
    with_presenter_mut(handle, |handle| {
        match handle.presenter.render_tick(time_seconds) {
            Ok(_stats) => {
                if !out_stats.is_null() {
                    let snapshot = handle.presenter.runtime_snapshot();
                    unsafe { *out_stats = presenter_stats_to_c(snapshot) };
                }
                clear_last_error();
                ErikaStatus::Ok
            }
            Err(error) => player_error(format!("render_tick failed: {error}")),
        }
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_capture_frame_rgba(
    handle: *mut ErikaPresenterHandle,
    width: u32,
    height: u32,
    out_rgba: *mut u8,
    out_capacity: usize,
) -> ErikaStatus {
    let Some(expected_len) = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
    else {
        return ErikaStatus::PlayerError;
    };
    if expected_len == 0 || out_rgba.is_null() || out_capacity < expected_len {
        return ErikaStatus::NullPointer;
    }
    with_presenter_mut(handle, |handle| {
        match handle.presenter.capture_frame_rgba(width, height) {
            Ok(Some(rgba)) if rgba.len() == expected_len => {
                unsafe {
                    std::ptr::copy_nonoverlapping(rgba.as_ptr(), out_rgba, expected_len);
                }
                ErikaStatus::Ok
            }
            Ok(_) => player_error("capture_frame_rgba returned an unexpected frame size"),
            Err(error) => player_error(format!("capture_frame_rgba failed: {error}")),
        }
    })
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn erika_presenter_poll_event(
    handle: *mut ErikaPresenterHandle,
    out_event: *mut ErikaEvent,
) -> ErikaStatus {
    if out_event.is_null() {
        set_last_error("event output pointer is null");
        return ErikaStatus::NullPointer;
    }
    if handle.is_null() {
        set_last_error("Erika presenter handle pointer is null");
        return ErikaStatus::NullPointer;
    }
    match catch_unwind(AssertUnwindSafe(|| {
        let handle = unsafe { &mut *handle };
        match handle.events.try_recv() {
            Ok(event) => {
                let event = event_to_c(event);
                let is_error = event.kind == ErikaEventKind::Error;
                unsafe { *out_event = event };
                if !is_error {
                    clear_last_error();
                }
                ErikaStatus::Ok
            }
            Err(crossbeam_channel::TryRecvError::Empty) => ErikaStatus::NoEvent,
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                player_error("presenter event channel disconnected")
            }
        }
    })) {
        Ok(status) => status,
        Err(_) => {
            set_last_error("panic while polling Erika presenter event");
            ErikaStatus::Panic
        }
    }
}

fn with_handle_mut(
    handle: *mut ErikaHandle,
    f: impl FnOnce(&mut ErikaHandle) -> ErikaStatus,
) -> ErikaStatus {
    if handle.is_null() {
        set_last_error("Erika handle pointer is null");
        return ErikaStatus::NullPointer;
    }
    match catch_unwind(AssertUnwindSafe(|| f(unsafe { &mut *handle }))) {
        Ok(status) => finalize_status(status),
        Err(_) => {
            set_last_error("panic while handling Erika C ABI call");
            ErikaStatus::Panic
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
fn with_presenter_mut(
    handle: *mut ErikaPresenterHandle,
    f: impl FnOnce(&mut ErikaPresenterHandle) -> ErikaStatus,
) -> ErikaStatus {
    if handle.is_null() {
        set_last_error("Erika presenter handle pointer is null");
        return ErikaStatus::NullPointer;
    }
    match catch_unwind(AssertUnwindSafe(|| f(unsafe { &mut *handle }))) {
        Ok(status) => finalize_status(status),
        Err(_) => {
            set_last_error("panic while handling Erika presenter C ABI call");
            ErikaStatus::Panic
        }
    }
}

fn c_string(ptr: *const c_char) -> Result<String, ErikaStatus> {
    if ptr.is_null() {
        set_last_error("required C string pointer is null");
        return Err(ErikaStatus::NullPointer);
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map(str::to_string)
        .map_err(|_| {
            set_last_error("required C string is not valid UTF-8");
            ErikaStatus::InvalidUtf8
        })
}

fn optional_c_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let value = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()?
        .trim()
        .to_string();
    (!value.is_empty()).then_some(value)
}

fn status_from_player_result(result: erika::Result<()>) -> ErikaStatus {
    match result {
        Ok(()) => {
            clear_last_error();
            ErikaStatus::Ok
        }
        Err(error) => player_error(error.to_string()),
    }
}

fn track_id_option(track_id: i64) -> Option<i64> {
    (track_id >= 0).then_some(track_id)
}

fn write_tracks_to_c(
    tracks: &[TrackInfo],
    out_tracks: *mut ErikaTrackInfo,
    capacity: usize,
    out_len: *mut usize,
) -> ErikaStatus {
    unsafe { *out_len = tracks.len() };
    if capacity == 0 {
        return ErikaStatus::Ok;
    }
    let count = tracks.len().min(capacity);
    for (index, track) in tracks.iter().take(count).enumerate() {
        unsafe { *out_tracks.add(index) = track_info_to_c(track) };
    }
    ErikaStatus::Ok
}

fn write_danmaku_tracks_to_c(
    tracks: &[DanmakuTrackInfo],
    out_tracks: *mut ErikaDanmakuTrackInfo,
    capacity: usize,
    out_len: *mut usize,
) -> ErikaStatus {
    unsafe { *out_len = tracks.len() };
    if capacity == 0 {
        return ErikaStatus::Ok;
    }
    let count = tracks.len().min(capacity);
    for (index, track) in tracks.iter().take(count).enumerate() {
        unsafe { *out_tracks.add(index) = danmaku_track_info_to_c(track) };
    }
    ErikaStatus::Ok
}

fn danmaku_track_info_to_c(track: &DanmakuTrackInfo) -> ErikaDanmakuTrackInfo {
    ErikaDanmakuTrackInfo {
        id: track.id,
        enabled: track.enabled,
        offset_micros: track.offset_micros,
        item_count: track.item_count,
        name: option_string_to_c(Some(&track.name)),
        source: option_string_to_c(Some(&track.source)),
    }
}

fn danmaku_track_name_from_uri(uri: &str) -> String {
    std::path::Path::new(uri)
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("danmaku")
        .to_string()
}

fn track_info_to_c(track: &TrackInfo) -> ErikaTrackInfo {
    ErikaTrackInfo {
        id: track.id,
        kind: track_kind_to_c(track.kind),
        source: track_source_to_c(track.source),
        selected: track.selected,
        can_remove: track.can_remove,
        title: option_string_to_c(track.title.as_deref()),
        language: option_string_to_c(track.language.as_deref()),
        codec: option_string_to_c(track.codec.as_deref()),
    }
}

fn track_kind_to_c(kind: TrackKind) -> ErikaTrackKind {
    match kind {
        TrackKind::Video => ErikaTrackKind::Video,
        TrackKind::Audio => ErikaTrackKind::Audio,
        TrackKind::Subtitle => ErikaTrackKind::Subtitle,
    }
}

fn track_source_to_c(source: TrackSource) -> ErikaTrackSource {
    match source {
        TrackSource::Embedded => ErikaTrackSource::Embedded,
        TrackSource::External => ErikaTrackSource::External,
    }
}

fn track_selection_to_c(selection: TrackSelection) -> ErikaTrackSelection {
    ErikaTrackSelection {
        video: selection.video.unwrap_or(-1),
        audio: selection.audio.unwrap_or(-1),
        subtitle: selection.subtitle.unwrap_or(-1),
    }
}

fn option_string_to_c(value: Option<&str>) -> *mut c_char {
    let Some(value) = value else {
        return std::ptr::null_mut();
    };
    match CString::new(value) {
        Ok(value) => value.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

fn free_c_string(ptr: &mut *mut c_char) {
    if ptr.is_null() {
        return;
    }
    let raw = *ptr;
    *ptr = std::ptr::null_mut();
    unsafe { drop(CString::from_raw(raw)) };
}

fn event_to_c(event: PlayerEvent) -> ErikaEvent {
    match event {
        PlayerEvent::StateChanged(state) => ErikaEvent {
            kind: ErikaEventKind::StateChanged,
            state: state_to_c(state),
            ..ErikaEvent::default()
        },
        PlayerEvent::DurationChanged(duration) => ErikaEvent {
            kind: ErikaEventKind::DurationChanged,
            duration_micros: duration.map(duration_micros_i64).unwrap_or(-1),
            ..ErikaEvent::default()
        },
        PlayerEvent::PositionChanged(position) => ErikaEvent {
            kind: ErikaEventKind::PositionChanged,
            position_micros: duration_micros_u64(position),
            ..ErikaEvent::default()
        },
        PlayerEvent::TracksChanged(tracks) => {
            let mut counts = ErikaTrackCounts::default();
            for track in tracks {
                match track.kind {
                    TrackKind::Video => counts.video = counts.video.saturating_add(1),
                    TrackKind::Audio => counts.audio = counts.audio.saturating_add(1),
                    TrackKind::Subtitle => counts.subtitle = counts.subtitle.saturating_add(1),
                }
            }
            ErikaEvent {
                kind: ErikaEventKind::TracksChanged,
                tracks: counts,
                ..ErikaEvent::default()
            }
        }
        PlayerEvent::TrackSelectionChanged(_) => ErikaEvent {
            kind: ErikaEventKind::TrackSelectionChanged,
            ..ErikaEvent::default()
        },
        PlayerEvent::BufferingChanged(buffering) => ErikaEvent {
            kind: ErikaEventKind::BufferingChanged,
            buffering,
            ..ErikaEvent::default()
        },
        PlayerEvent::VideoParamsChanged(params) => ErikaEvent {
            kind: ErikaEventKind::VideoParamsChanged,
            video: ErikaVideoParams {
                width: params.width,
                height: params.height,
                primaries: params.primaries as u32,
                transfer: transfer_to_c(params.transfer),
            },
            ..ErikaEvent::default()
        },
        PlayerEvent::SurfaceAttached(_) => ErikaEvent {
            kind: ErikaEventKind::SurfaceAttached,
            ..ErikaEvent::default()
        },
        PlayerEvent::SurfaceDetached => ErikaEvent {
            kind: ErikaEventKind::SurfaceDetached,
            ..ErikaEvent::default()
        },
        PlayerEvent::Error(error) => {
            set_last_error(error.to_string());
            ErikaEvent {
                kind: ErikaEventKind::Error,
                status: ErikaStatus::PlayerError,
                ..ErikaEvent::default()
            }
        }
    }
}

fn state_to_c(state: PlayerState) -> ErikaState {
    match state {
        PlayerState::Idle => ErikaState::Idle,
        PlayerState::Opening => ErikaState::Opening,
        PlayerState::Ready => ErikaState::Ready,
        PlayerState::Playing => ErikaState::Playing,
        PlayerState::Paused => ErikaState::Paused,
        PlayerState::Stopped => ErikaState::Stopped,
        PlayerState::Closed => ErikaState::Closed,
        PlayerState::Error => ErikaState::Error,
    }
}

fn transfer_to_c(transfer: TransferFunction) -> u32 {
    match transfer {
        TransferFunction::Unknown => 0,
        TransferFunction::Srgb => 1,
        TransferFunction::Bt1886 => 2,
        TransferFunction::Pq => 3,
        TransferFunction::Hlg => 4,
    }
}

fn wgpu_surface_kind_from_c(kind: ErikaWgpuSurfaceKind) -> WgpuSurfaceKind {
    match kind {
        ErikaWgpuSurfaceKind::Unknown => WgpuSurfaceKind::Unknown,
        ErikaWgpuSurfaceKind::MacOsNsView => WgpuSurfaceKind::MacOsNsView,
        ErikaWgpuSurfaceKind::MacOsCaMetalLayer => WgpuSurfaceKind::MacOsCaMetalLayer,
        ErikaWgpuSurfaceKind::IosUiView => WgpuSurfaceKind::IosUiView,
        ErikaWgpuSurfaceKind::WindowsHwnd => WgpuSurfaceKind::WindowsHwnd,
        ErikaWgpuSurfaceKind::XlibWindow => WgpuSurfaceKind::XlibWindow,
        ErikaWgpuSurfaceKind::WaylandSurface => WgpuSurfaceKind::WaylandSurface,
        ErikaWgpuSurfaceKind::AndroidNativeWindow => WgpuSurfaceKind::AndroidNativeWindow,
    }
}

fn flutter_texture_kind_from_c(kind: ErikaFlutterTextureKind) -> FlutterTextureKind {
    match kind {
        ErikaFlutterTextureKind::Unknown => FlutterTextureKind::Unknown,
        ErikaFlutterTextureKind::MacOsTextureRegistrar => FlutterTextureKind::MacOsTextureRegistrar,
        ErikaFlutterTextureKind::IosTextureRegistrar => FlutterTextureKind::IosTextureRegistrar,
        ErikaFlutterTextureKind::AndroidSurfaceTexture => FlutterTextureKind::AndroidSurfaceTexture,
        ErikaFlutterTextureKind::WindowsTextureRegistrar => {
            FlutterTextureKind::WindowsTextureRegistrar
        }
        ErikaFlutterTextureKind::LinuxTextureRegistrar => FlutterTextureKind::LinuxTextureRegistrar,
    }
}

fn duration_micros_i64(duration: Duration) -> i64 {
    duration.as_micros().min(i64::MAX as u128) as i64
}

fn duration_micros_u64(duration: Duration) -> u64 {
    duration.as_micros().min(u64::MAX as u128) as u64
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
fn presenter_stats_to_c(snapshot: PresenterRuntimeSnapshot) -> ErikaPresenterStats {
    let stats = snapshot.stats;
    let renderer = snapshot.renderer;
    ErikaPresenterStats {
        decoded_video_frames: stats.decoded_video_frames,
        rendered_video_frames: stats.rendered_video_frames,
        rendered_test_frames: stats.rendered_test_frames,
        pushed_audio_frames: stats.pushed_audio_frames,
        overlay_frames: stats.overlay_frames,
        danmaku_frames: stats.danmaku_frames,
        danmaku_items: stats.danmaku_items,
        import_failures: stats.import_failures,
        render_failures: stats.render_failures,
        audio_failures: stats.audio_failures,
        software_video_frames: renderer.software_video_frames,
        hardware_video_frames: renderer.hardware_video_frames,
        zero_copy_video_frames: renderer.zero_copy_video_frames,
        cpu_video_frame_fallbacks: renderer.cpu_video_frame_fallbacks,
        last_render_micros: duration_micros_u64(snapshot.last_render_duration),
        last_render_current_micros: duration_micros_u64(snapshot.last_render_current_duration),
        audio_clock_read_frames: snapshot.audio_output_read_frames,
        audio_clock_queued_frames: snapshot.audio_output_queued_frames.min(u64::MAX as usize)
            as u64,
        audio_clock_underflow_frames: snapshot.audio_output_underflow_frames,
        direct_zero_copy_video_frames: renderer.direct_zero_copy_video_frames,
        shared_handle_video_frames: renderer.shared_handle_video_frames,
        hdr_source_frames: renderer.hdr_source_frames,
        hdr10_output_frames: renderer.hdr10_output_frames,
        sdr_tonemap_frames: renderer.sdr_tonemap_frames,
        hdr10_metadata_updates: renderer.hdr10_metadata_updates,
        hdr10_metadata_failures: renderer.hdr10_metadata_failures,
        hdr10_output_failures: renderer.hdr10_output_failures,
        hdr10_output_active: renderer.hdr10_output_active,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_event_counts_tracks() {
        let event = event_to_c(PlayerEvent::TracksChanged(vec![
            erika::TrackInfo::embedded(0, TrackKind::Video),
            erika::TrackInfo::embedded(1, TrackKind::Audio),
        ]));

        assert_eq!(event.kind, ErikaEventKind::TracksChanged);
        assert_eq!(event.tracks.video, 1);
        assert_eq!(event.tracks.audio, 1);
        assert_eq!(event.tracks.subtitle, 0);
    }

    #[test]
    fn c_event_reports_track_selection_changed() {
        let event = event_to_c(PlayerEvent::TrackSelectionChanged(erika::TrackSelection {
            video: Some(0),
            audio: Some(1),
            subtitle: None,
        }));

        assert_eq!(event.kind, ErikaEventKind::TrackSelectionChanged);
    }

    #[test]
    fn c_track_info_maps_source_selection_and_strings() {
        let mut track = erika::TrackInfo::external(1_000_001, TrackKind::Subtitle);
        track.selected = true;
        track.title = Some("Signs".to_string());
        track.language = Some("jpn".to_string());
        track.codec = Some("ass".to_string());

        let mut c_track = track_info_to_c(&track);

        assert_eq!(c_track.id, 1_000_001);
        assert_eq!(c_track.kind, ErikaTrackKind::Subtitle);
        assert_eq!(c_track.source, ErikaTrackSource::External);
        assert!(c_track.selected);
        assert!(c_track.can_remove);
        assert_eq!(
            unsafe { CStr::from_ptr(c_track.title).to_str().unwrap() },
            "Signs"
        );
        assert_eq!(
            unsafe { CStr::from_ptr(c_track.language).to_str().unwrap() },
            "jpn"
        );
        assert_eq!(
            unsafe { CStr::from_ptr(c_track.codec).to_str().unwrap() },
            "ass"
        );

        unsafe { erika_track_info_free(&mut c_track) };
        assert!(c_track.title.is_null());
        assert!(c_track.language.is_null());
        assert!(c_track.codec.is_null());
    }

    #[test]
    fn c_track_selection_uses_negative_one_for_disabled_tracks() {
        let selection = track_selection_to_c(erika::TrackSelection {
            video: Some(0),
            audio: None,
            subtitle: Some(2),
        });

        assert_eq!(selection.video, 0);
        assert_eq!(selection.audio, -1);
        assert_eq!(selection.subtitle, 2);
    }

    #[test]
    fn null_handle_is_rejected() {
        assert_eq!(
            unsafe { erika_play(std::ptr::null_mut()) },
            ErikaStatus::NullPointer
        );
    }

    #[test]
    fn c_external_subtitle_api_rejects_null_pointers() {
        let subtitle_uri = std::ffi::CString::new("/tmp/subs.srt").unwrap();
        let handle = erika_create();
        assert!(!handle.is_null());

        let status = unsafe {
            erika_add_external_subtitle(handle, subtitle_uri.as_ptr(), std::ptr::null_mut())
        };
        assert_eq!(status, ErikaStatus::NullPointer);

        let mut track_id = 0;
        let status = unsafe {
            erika_add_external_subtitle(std::ptr::null_mut(), subtitle_uri.as_ptr(), &mut track_id)
        };
        assert_eq!(status, ErikaStatus::NullPointer);

        let status = unsafe { erika_remove_subtitle_track(std::ptr::null_mut(), 1_000_001) };
        assert_eq!(status, ErikaStatus::NullPointer);

        unsafe { erika_destroy(handle) };
    }

    #[test]
    fn c_surface_attach_emits_events() {
        let handle = erika_create();
        assert!(!handle.is_null());

        let status = unsafe { erika_attach_metal_layer(handle, 42, 1920, 1080, 2.0) };
        assert_eq!(status, ErikaStatus::Ok);

        let mut event = ErikaEvent::default();
        let status = unsafe { erika_poll_event(handle, &mut event) };
        assert_eq!(status, ErikaStatus::Ok);
        assert_eq!(event.kind, ErikaEventKind::SurfaceAttached);

        let status = unsafe { erika_detach_surface(handle) };
        assert_eq!(status, ErikaStatus::Ok);
        let status = unsafe { erika_poll_event(handle, &mut event) };
        assert_eq!(status, ErikaStatus::Ok);
        assert_eq!(event.kind, ErikaEventKind::SurfaceDetached);

        unsafe { erika_destroy(handle) };
    }

    #[test]
    fn c_wgpu_surface_attach_emits_event() {
        let handle = erika_create();
        assert!(!handle.is_null());

        let status = unsafe {
            erika_attach_wgpu_surface(
                handle,
                ErikaWgpuSurfaceKind::MacOsCaMetalLayer,
                42,
                0,
                1920,
                1080,
                2.0,
            )
        };
        assert_eq!(status, ErikaStatus::Ok);

        let mut event = ErikaEvent::default();
        let status = unsafe { erika_poll_event(handle, &mut event) };
        assert_eq!(status, ErikaStatus::Ok);
        assert_eq!(event.kind, ErikaEventKind::SurfaceAttached);

        unsafe { erika_destroy(handle) };
    }

    #[test]
    fn c_flutter_texture_attach_emits_event() {
        let handle = erika_create();
        assert!(!handle.is_null());

        let status = unsafe {
            erika_attach_flutter_texture(
                handle,
                ErikaFlutterTextureKind::MacOsTextureRegistrar,
                7,
                1280,
                720,
                2.0,
            )
        };
        assert_eq!(status, ErikaStatus::Ok);

        let mut event = ErikaEvent::default();
        let status = unsafe { erika_poll_event(handle, &mut event) };
        assert_eq!(status, ErikaStatus::Ok);
        assert_eq!(event.kind, ErikaEventKind::SurfaceAttached);

        unsafe { erika_destroy(handle) };
    }

    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
    #[test]
    fn c_presenter_lifecycle_rejects_null_and_can_be_destroyed() {
        assert_eq!(
            unsafe { erika_presenter_play(std::ptr::null_mut()) },
            ErikaStatus::NullPointer
        );
        let handle = erika_presenter_create();
        assert!(!handle.is_null());
        unsafe { erika_presenter_destroy(handle) };
    }

    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
    #[test]
    fn c_presenter_set_volume_accepts_valid_handle() {
        assert_eq!(
            unsafe { erika_presenter_set_volume(std::ptr::null_mut(), 0.5) },
            ErikaStatus::NullPointer
        );

        let handle = erika_presenter_create();
        assert!(!handle.is_null());
        assert_eq!(
            unsafe { erika_presenter_set_volume(handle, 0.5) },
            ErikaStatus::Ok
        );
        assert_eq!(
            unsafe { erika_presenter_set_volume(handle, f64::NAN) },
            ErikaStatus::Ok
        );
        unsafe { erika_presenter_destroy(handle) };
    }

    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
    #[test]
    fn c_presenter_set_upscaler_accepts_valid_handle() {
        assert_eq!(
            unsafe {
                erika_presenter_set_upscaler(
                    std::ptr::null_mut(),
                    ErikaLumaUpscalerMode::ArtCnnC4F16 as i32,
                )
            },
            ErikaStatus::NullPointer
        );

        let handle = erika_presenter_create();
        assert!(!handle.is_null());
        assert_eq!(
            unsafe {
                erika_presenter_set_upscaler(handle, ErikaLumaUpscalerMode::ArtCnnC4F16 as i32)
            },
            ErikaStatus::Ok
        );
        assert_eq!(
            unsafe { erika_presenter_set_upscaler(handle, ErikaLumaUpscalerMode::Off as i32) },
            ErikaStatus::Ok
        );
        assert_eq!(
            unsafe { erika_presenter_set_upscaler(handle, 999) },
            ErikaStatus::Ok
        );
        unsafe { erika_presenter_destroy(handle) };
    }

    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
    #[test]
    fn c_presenter_reports_upscaler_status() {
        assert_eq!(
            unsafe {
                erika_presenter_get_upscaler_status(std::ptr::null_mut(), std::ptr::null_mut())
            },
            ErikaStatus::NullPointer
        );

        let handle = erika_presenter_create();
        assert!(!handle.is_null());
        let mut status = ErikaUpscalerStatus::default();
        assert_eq!(
            unsafe { erika_presenter_get_upscaler_status(handle, &mut status) },
            ErikaStatus::Ok
        );
        assert_eq!(status.requested_mode, ErikaLumaUpscalerMode::Off as i32);
        assert_eq!(
            status.active_backend,
            ErikaUpscalerBackendStatus::Off as i32
        );

        assert_eq!(
            unsafe {
                erika_presenter_set_upscaler(handle, ErikaLumaUpscalerMode::ArtCnnC4F16 as i32)
            },
            ErikaStatus::Ok
        );
        assert_eq!(
            unsafe { erika_presenter_get_upscaler_status(handle, &mut status) },
            ErikaStatus::Ok
        );
        assert_eq!(
            status.requested_mode,
            ErikaLumaUpscalerMode::ArtCnnC4F16 as i32
        );
        assert_eq!(
            status.active_backend,
            ErikaUpscalerBackendStatus::Inactive as i32
        );
        assert_eq!(status.fallback_count, 0);
        unsafe { erika_presenter_destroy(handle) };
    }

    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
    #[test]
    fn c_presenter_can_be_created_with_edr_config() {
        let handle = erika_presenter_create_with_config(ErikaPresenterConfig {
            output_mode: ErikaPresenterOutputMode::AppleEdr as i32,
            edr_headroom: 4.0,
            ..ErikaPresenterConfig::default()
        });
        assert!(!handle.is_null());
        unsafe { erika_presenter_destroy(handle) };
    }

    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
    #[test]
    fn c_presenter_danmaku_api_loads_configures_and_clears() {
        let handle = erika_presenter_create();
        assert!(!handle.is_null());

        let json = CString::new(
            r##"{"comments":[{"time":1.0,"content":"c api danmaku","type":"scroll","color":"#ffffff"}]}"##,
        )
        .unwrap();
        assert_eq!(
            unsafe { erika_presenter_load_danmaku_json(handle, json.as_ptr()) },
            ErikaStatus::Ok
        );
        assert_eq!(
            unsafe { erika_presenter_set_danmaku_enabled(handle, true) },
            ErikaStatus::Ok
        );
        assert_eq!(
            unsafe { erika_presenter_set_danmaku_config(handle, ErikaDanmakuConfig::default()) },
            ErikaStatus::Ok
        );
        assert_eq!(
            unsafe { erika_presenter_clear_danmaku(handle) },
            ErikaStatus::Ok
        );

        unsafe { erika_presenter_destroy(handle) };
    }

    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
    #[test]
    fn c_presenter_config_maps_output_modes() {
        assert_eq!(
            metal_output_mode_from_c(ErikaPresenterConfig::default()),
            MetalOutputMode::Sdr
        );
        assert_eq!(
            metal_output_mode_from_c(ErikaPresenterConfig {
                output_mode: ErikaPresenterOutputMode::AppleEdr as i32,
                edr_headroom: 4.0,
                ..ErikaPresenterConfig::default()
            }),
            MetalOutputMode::apple_edr(4.0)
        );
        assert_eq!(
            metal_output_mode_from_c(ErikaPresenterConfig {
                output_mode: 999,
                edr_headroom: 4.0,
                ..ErikaPresenterConfig::default()
            }),
            MetalOutputMode::Sdr
        );
        assert_eq!(
            metal_output_mode_from_c(ErikaPresenterConfig {
                output_mode: ErikaPresenterOutputMode::AppleEdr as i32,
                edr_headroom: f32::NAN,
                ..ErikaPresenterConfig::default()
            }),
            MetalOutputMode::apple_edr(1.0)
        );
    }

    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "windows"))]
    #[test]
    fn c_presenter_config_maps_upscaler_modes() {
        assert_eq!(
            presenter_config_from_c(ErikaPresenterConfig::default())
                .renderer
                .luma_upscaler,
            LumaUpscalerMode::Off
        );
        assert_eq!(
            presenter_config_from_c(ErikaPresenterConfig {
                luma_upscaler: ErikaLumaUpscalerMode::ArtCnnC4F16 as i32,
                ..ErikaPresenterConfig::default()
            })
            .renderer
            .luma_upscaler,
            LumaUpscalerMode::ArtCnnC4F16
        );
        assert_eq!(
            presenter_config_from_c(ErikaPresenterConfig {
                luma_upscaler: ErikaLumaUpscalerMode::ArtCnnC4F32 as i32,
                ..ErikaPresenterConfig::default()
            })
            .renderer
            .luma_upscaler,
            LumaUpscalerMode::ArtCnnC4F32
        );
        assert_eq!(
            presenter_config_from_c(ErikaPresenterConfig {
                luma_upscaler: 999,
                ..ErikaPresenterConfig::default()
            })
            .renderer
            .luma_upscaler,
            LumaUpscalerMode::Off
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn c_presenter_config_keeps_windows_native_decode_path() {
        let config = presenter_config_from_c(ErikaPresenterConfig::default());
        assert_eq!(
            config.player.renderer,
            erika::RendererBackendPreference::Auto
        );
        assert_eq!(
            config.player.playback.video_decode,
            erika::playback::VideoDecodePreference::D3d11va
        );
    }
}
