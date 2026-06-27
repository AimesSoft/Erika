#ifndef ERIKA_H
#define ERIKA_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct ErikaHandle ErikaHandle;
typedef struct ErikaPresenterHandle ErikaPresenterHandle;

typedef enum ErikaStatus {
  ErikaStatus_Ok = 0,
  ErikaStatus_NullPointer = 1,
  ErikaStatus_InvalidUtf8 = 2,
  ErikaStatus_PlayerError = 3,
  ErikaStatus_Panic = 4,
  ErikaStatus_NoEvent = 5,
} ErikaStatus;

typedef enum ErikaState {
  ErikaState_Idle = 0,
  ErikaState_Opening = 1,
  ErikaState_Ready = 2,
  ErikaState_Playing = 3,
  ErikaState_Paused = 4,
  ErikaState_Stopped = 5,
  ErikaState_Closed = 6,
  ErikaState_Error = 7,
} ErikaState;

typedef enum ErikaEventKind {
  ErikaEventKind_None = 0,
  ErikaEventKind_StateChanged = 1,
  ErikaEventKind_DurationChanged = 2,
  ErikaEventKind_PositionChanged = 3,
  ErikaEventKind_TracksChanged = 4,
  ErikaEventKind_BufferingChanged = 5,
  ErikaEventKind_VideoParamsChanged = 6,
  ErikaEventKind_SurfaceAttached = 7,
  ErikaEventKind_SurfaceDetached = 8,
  ErikaEventKind_Error = 9,
  ErikaEventKind_TrackSelectionChanged = 10,
} ErikaEventKind;

typedef enum ErikaTrackKind {
  ErikaTrackKind_Video = 0,
  ErikaTrackKind_Audio = 1,
  ErikaTrackKind_Subtitle = 2,
} ErikaTrackKind;

typedef enum ErikaTrackSource {
  ErikaTrackSource_Embedded = 0,
  ErikaTrackSource_External = 1,
} ErikaTrackSource;

typedef enum ErikaWgpuSurfaceKind {
  ErikaWgpuSurfaceKind_Unknown = 0,
  ErikaWgpuSurfaceKind_MacOsNsView = 1,
  ErikaWgpuSurfaceKind_MacOsCaMetalLayer = 2,
  ErikaWgpuSurfaceKind_IosUiView = 3,
  ErikaWgpuSurfaceKind_WindowsHwnd = 4,
  ErikaWgpuSurfaceKind_XlibWindow = 5,
  ErikaWgpuSurfaceKind_WaylandSurface = 6,
  ErikaWgpuSurfaceKind_AndroidNativeWindow = 7,
} ErikaWgpuSurfaceKind;

typedef enum ErikaFlutterTextureKind {
  ErikaFlutterTextureKind_Unknown = 0,
  ErikaFlutterTextureKind_MacOsTextureRegistrar = 1,
  ErikaFlutterTextureKind_IosTextureRegistrar = 2,
  ErikaFlutterTextureKind_AndroidSurfaceTexture = 3,
  ErikaFlutterTextureKind_WindowsTextureRegistrar = 4,
  ErikaFlutterTextureKind_LinuxTextureRegistrar = 5,
} ErikaFlutterTextureKind;

typedef enum ErikaPresenterOutputMode {
  ErikaPresenterOutputMode_Sdr = 0,
  ErikaPresenterOutputMode_AppleEdr = 1,
} ErikaPresenterOutputMode;

typedef enum ErikaLumaUpscalerMode {
  ErikaLumaUpscalerMode_Off = 0,
  ErikaLumaUpscalerMode_ArtCnnC4F16 = 1,
  ErikaLumaUpscalerMode_ArtCnnC4F32 = 2,
} ErikaLumaUpscalerMode;

typedef enum ErikaUpscalerBackendStatus {
  ErikaUpscalerBackendStatus_Off = 0,
  ErikaUpscalerBackendStatus_Inactive = 1,
  ErikaUpscalerBackendStatus_Building = 2,
  ErikaUpscalerBackendStatus_Scalar = 3,
  ErikaUpscalerBackendStatus_SimdgroupMatrix = 4,
} ErikaUpscalerBackendStatus;

typedef struct ErikaPresenterConfig {
  int32_t output_mode;
  float edr_headroom;
  int32_t luma_upscaler;
} ErikaPresenterConfig;

typedef struct ErikaUpscalerStatus {
  int32_t requested_mode;
  int32_t active_backend;
  uint64_t fallback_count;
  uint64_t upscaled_frames;
  uint64_t last_encode_micros;
  uint64_t last_gpu_micros;
} ErikaUpscalerStatus;

typedef struct ErikaDanmakuConfig {
  bool enabled;
  /* NipaPlay/Flutter logical danmaku font size. Erika uses the NipaPlay
   * default danmaku font and multiplies by the surface scale for glyph pixels. */
  float font_size;
  float opacity;
  float display_area;
  float scroll_duration_seconds;
  float scroll_speed_factor;
  float track_gap_ratio;
  float outline_width;
  float shadow_offset_x;
  float shadow_offset_y;
  bool merge_duplicates;
  bool allow_stacking;
  bool allow_scroll_overwrite;
  uint32_t max_quantity;
  uint32_t max_lines_per_mode;
  bool block_top;
  bool block_bottom;
  bool block_scroll;
  int32_t shadow_style;
} ErikaDanmakuConfig;

typedef struct ErikaDanmakuTrackInfo {
  uint64_t id;
  bool enabled;
  int64_t offset_micros;
  uintptr_t item_count;
  char *name;
  char *source;
} ErikaDanmakuTrackInfo;

typedef struct ErikaVideoParams {
  uint32_t width;
  uint32_t height;
  uint32_t primaries;
  uint32_t transfer;
} ErikaVideoParams;

typedef struct ErikaTrackCounts {
  uint32_t video;
  uint32_t audio;
  uint32_t subtitle;
} ErikaTrackCounts;

typedef struct ErikaTrackSelection {
  int64_t video;
  int64_t audio;
  int64_t subtitle;
} ErikaTrackSelection;

typedef struct ErikaTrackInfo {
  int64_t id;
  ErikaTrackKind kind;
  ErikaTrackSource source;
  bool selected;
  bool can_remove;
  char *title;
  char *language;
  char *codec;
} ErikaTrackInfo;

typedef struct ErikaEvent {
  ErikaEventKind kind;
  ErikaStatus status;
  ErikaState state;
  int64_t duration_micros;
  uint64_t position_micros;
  bool buffering;
  ErikaVideoParams video;
  ErikaTrackCounts tracks;
} ErikaEvent;

typedef struct ErikaPresenterStats {
  uint64_t decoded_video_frames;
  uint64_t rendered_video_frames;
  uint64_t rendered_test_frames;
  uint64_t pushed_audio_frames;
  uint64_t overlay_frames;
  uint64_t danmaku_frames;
  uint64_t danmaku_items;
  uint64_t import_failures;
  uint64_t render_failures;
  uint64_t audio_failures;
  uint64_t software_video_frames;
  uint64_t hardware_video_frames;
  uint64_t zero_copy_video_frames;
  uint64_t cpu_video_frame_fallbacks;
  uint64_t last_render_micros;
  uint64_t last_render_current_micros;
  uint64_t audio_clock_read_frames;
  uint64_t audio_clock_queued_frames;
  uint64_t audio_clock_underflow_frames;
  uint64_t direct_zero_copy_video_frames;
  uint64_t shared_handle_video_frames;
  uint64_t hdr_source_frames;
  uint64_t hdr10_output_frames;
  uint64_t sdr_tonemap_frames;
  uint64_t hdr10_metadata_updates;
  uint64_t hdr10_metadata_failures;
  uint64_t hdr10_output_failures;
  bool hdr10_output_active;
} ErikaPresenterStats;

ErikaHandle *erika_create(void);
void erika_destroy(ErikaHandle *handle);

ErikaStatus erika_open(ErikaHandle *handle, const char *uri);
ErikaStatus erika_play(ErikaHandle *handle);
ErikaStatus erika_pause(ErikaHandle *handle);
ErikaStatus erika_stop(ErikaHandle *handle);
ErikaStatus erika_close(ErikaHandle *handle);
ErikaStatus erika_seek(ErikaHandle *handle, uint64_t position_micros);
ErikaStatus erika_add_external_subtitle(
    ErikaHandle *handle,
    const char *uri,
    int64_t *out_track_id);
ErikaStatus erika_remove_subtitle_track(ErikaHandle *handle, int64_t track_id);
ErikaStatus erika_select_audio_track(ErikaHandle *handle, int64_t track_id);
ErikaStatus erika_select_subtitle_track(ErikaHandle *handle, int64_t track_id);
ErikaStatus erika_track_selection(
    ErikaHandle *handle,
    ErikaTrackSelection *out_selection);
ErikaStatus erika_tracks(
    ErikaHandle *handle,
    ErikaTrackInfo *out_tracks,
    uintptr_t capacity,
    uintptr_t *out_len);
void erika_track_info_free(ErikaTrackInfo *track);
void erika_danmaku_track_info_free(ErikaDanmakuTrackInfo *track);
ErikaStatus erika_state(ErikaHandle *handle, ErikaState *out_state);
ErikaStatus erika_poll_event(ErikaHandle *handle, ErikaEvent *out_event);

ErikaStatus erika_attach_metal_layer(
    ErikaHandle *handle,
    uint64_t raw_layer,
    uint32_t width,
    uint32_t height,
    double scale);

ErikaStatus erika_attach_wgpu_surface(
    ErikaHandle *handle,
    ErikaWgpuSurfaceKind kind,
    uint64_t raw_window,
    uint64_t raw_display,
    uint32_t width,
    uint32_t height,
    double scale);

ErikaStatus erika_attach_flutter_texture(
    ErikaHandle *handle,
    ErikaFlutterTextureKind kind,
    int64_t texture_id,
    uint32_t width,
    uint32_t height,
    double scale);

ErikaStatus erika_detach_surface(ErikaHandle *handle);

ErikaPresenterHandle *erika_presenter_create(void);
ErikaPresenterHandle *erika_presenter_create_with_config(ErikaPresenterConfig config);
ErikaPresenterHandle *erika_presenter_create_with_output_mode(
    int32_t output_mode,
    float edr_headroom);
void erika_presenter_destroy(ErikaPresenterHandle *handle);

ErikaStatus erika_presenter_open(ErikaPresenterHandle *handle, const char *uri);
ErikaStatus erika_presenter_play(ErikaPresenterHandle *handle);
ErikaStatus erika_presenter_pause(ErikaPresenterHandle *handle);
ErikaStatus erika_presenter_stop(ErikaPresenterHandle *handle);
ErikaStatus erika_presenter_close(ErikaPresenterHandle *handle);
ErikaStatus erika_presenter_seek(ErikaPresenterHandle *handle, uint64_t position_micros);
ErikaStatus erika_presenter_set_playback_rate(ErikaPresenterHandle *handle, double rate);
ErikaStatus erika_presenter_set_volume(ErikaPresenterHandle *handle, double volume);
ErikaStatus erika_presenter_set_upscaler(ErikaPresenterHandle *handle, int32_t mode);
ErikaStatus erika_presenter_get_upscaler_status(
    ErikaPresenterHandle *handle,
    ErikaUpscalerStatus *out_status);
ErikaStatus erika_presenter_add_external_subtitle(
    ErikaPresenterHandle *handle,
    const char *uri,
    int64_t *out_track_id);
ErikaStatus erika_presenter_remove_subtitle_track(
    ErikaPresenterHandle *handle,
    int64_t track_id);
ErikaStatus erika_presenter_select_audio_track(
    ErikaPresenterHandle *handle,
    int64_t track_id);
ErikaStatus erika_presenter_select_subtitle_track(
    ErikaPresenterHandle *handle,
    int64_t track_id);
ErikaStatus erika_presenter_load_danmaku_file(
    ErikaPresenterHandle *handle,
    const char *uri);
ErikaStatus erika_presenter_load_danmaku_json(
    ErikaPresenterHandle *handle,
    const char *json);
ErikaStatus erika_presenter_add_danmaku_track_file(
    ErikaPresenterHandle *handle,
    const char *uri,
    const char *name,
    int64_t offset_micros,
    uint64_t *out_track_id);
ErikaStatus erika_presenter_add_danmaku_track_json(
    ErikaPresenterHandle *handle,
    const char *json,
    const char *name,
    int64_t offset_micros,
    uint64_t *out_track_id);
ErikaStatus erika_presenter_remove_danmaku_track(
    ErikaPresenterHandle *handle,
    uint64_t track_id);
ErikaStatus erika_presenter_set_danmaku_track_enabled(
    ErikaPresenterHandle *handle,
    uint64_t track_id,
    bool enabled);
ErikaStatus erika_presenter_set_danmaku_track_offset(
    ErikaPresenterHandle *handle,
    uint64_t track_id,
    int64_t offset_micros);
ErikaStatus erika_presenter_set_danmaku_global_offset(
    ErikaPresenterHandle *handle,
    int64_t offset_micros);
ErikaStatus erika_presenter_danmaku_tracks(
    ErikaPresenterHandle *handle,
    ErikaDanmakuTrackInfo *out_tracks,
    uintptr_t capacity,
    uintptr_t *out_len);
ErikaStatus erika_presenter_clear_danmaku(ErikaPresenterHandle *handle);
ErikaStatus erika_presenter_set_danmaku_enabled(
    ErikaPresenterHandle *handle,
    bool enabled);
ErikaStatus erika_presenter_set_danmaku_config(
    ErikaPresenterHandle *handle,
    ErikaDanmakuConfig config);
ErikaStatus erika_presenter_set_danmaku_config_ptr(
    ErikaPresenterHandle *handle,
    const ErikaDanmakuConfig *config);
ErikaStatus erika_presenter_get_danmaku_config(
    ErikaPresenterHandle *handle,
    ErikaDanmakuConfig *out_config);
ErikaStatus erika_presenter_set_danmaku_font(
    ErikaPresenterHandle *handle,
    const char *family,
    const char *file_path);
ErikaStatus erika_presenter_set_danmaku_block_words_json(
    ErikaPresenterHandle *handle,
    const char *json);
ErikaStatus erika_presenter_track_selection(
    ErikaPresenterHandle *handle,
    ErikaTrackSelection *out_selection);
ErikaStatus erika_presenter_tracks(
    ErikaPresenterHandle *handle,
    ErikaTrackInfo *out_tracks,
    uintptr_t capacity,
    uintptr_t *out_len);

ErikaStatus erika_presenter_attach_metal_layer(
    ErikaPresenterHandle *handle,
    uint64_t raw_layer,
    uint32_t width,
    uint32_t height,
    double scale);

ErikaStatus erika_presenter_attach_wgpu_surface(
    ErikaPresenterHandle *handle,
    ErikaWgpuSurfaceKind kind,
    uint64_t raw_window,
    uint64_t raw_display,
    uint32_t width,
    uint32_t height,
    double scale);

ErikaStatus erika_presenter_attach_windows_hwnd(
    ErikaPresenterHandle *handle,
    uint64_t hwnd,
    uint64_t hinstance,
    uint32_t width,
    uint32_t height,
    double scale);

ErikaStatus erika_presenter_resize_surface(
    ErikaPresenterHandle *handle,
    uint32_t width,
    uint32_t height,
    double scale);

ErikaStatus erika_presenter_detach_surface(ErikaPresenterHandle *handle);
ErikaStatus erika_presenter_render_tick(
    ErikaPresenterHandle *handle,
    double time_seconds,
    ErikaPresenterStats *out_stats);
ErikaStatus erika_presenter_poll_event(ErikaPresenterHandle *handle, ErikaEvent *out_event);

#ifdef __cplusplus
}
#endif

#endif
