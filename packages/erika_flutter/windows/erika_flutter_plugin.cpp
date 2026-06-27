#include "erika_flutter_plugin.h"

#include <flutter/event_channel.h>
#include <flutter/method_channel.h>
#include <flutter/standard_method_codec.h>

#include <algorithm>
#include <chrono>
#include <cctype>
#include <cmath>
#include <cstdlib>
#include <filesystem>
#include <mutex>
#include <sstream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace erika_flutter {
namespace {

constexpr int64_t kWindowOverlayViewId = -1;
constexpr wchar_t kOverlayWindowClassName[] = L"ErikaFlutterVideoOverlay";
constexpr UINT kFrameTimerMinIntervalMs = 1;
constexpr UINT kFrameTimerDefaultIntervalMs = 16;

using flutter::EncodableList;
using flutter::EncodableMap;
using flutter::EncodableValue;

class PluginError : public std::runtime_error {
 public:
  explicit PluginError(const std::string& message) : std::runtime_error(message) {}
};

std::string LastErrorMessage() {
  const DWORD error = GetLastError();
  if (error == 0) {
    return {};
  }

  LPWSTR buffer = nullptr;
  const DWORD size = FormatMessageW(
      FORMAT_MESSAGE_ALLOCATE_BUFFER | FORMAT_MESSAGE_FROM_SYSTEM |
          FORMAT_MESSAGE_IGNORE_INSERTS,
      nullptr, error, MAKELANGID(LANG_NEUTRAL, SUBLANG_DEFAULT),
      reinterpret_cast<LPWSTR>(&buffer), 0, nullptr);
  if (size == 0 || buffer == nullptr) {
    return "Win32 error " + std::to_string(error);
  }

  const int utf8_size =
      WideCharToMultiByte(CP_UTF8, 0, buffer, static_cast<int>(size), nullptr, 0,
                          nullptr, nullptr);
  std::string result(static_cast<size_t>(std::max(0, utf8_size)), '\0');
  if (utf8_size > 0) {
    WideCharToMultiByte(CP_UTF8, 0, buffer, static_cast<int>(size),
                        result.data(), utf8_size, nullptr, nullptr);
  }
  LocalFree(buffer);
  while (!result.empty() &&
         (result.back() == '\n' || result.back() == '\r' || result.back() == ' ')) {
    result.pop_back();
  }
  return result.empty() ? "Win32 error " + std::to_string(error) : result;
}

std::wstring Utf8ToWide(const std::string& value) {
  if (value.empty()) {
    return {};
  }
  const int size =
      MultiByteToWideChar(CP_UTF8, 0, value.data(),
                          static_cast<int>(value.size()), nullptr, 0);
  if (size <= 0) {
    return {};
  }
  std::wstring result(static_cast<size_t>(size), L'\0');
  MultiByteToWideChar(CP_UTF8, 0, value.data(), static_cast<int>(value.size()),
                      result.data(), size);
  return result;
}

std::optional<std::filesystem::path> EnvironmentPath(const wchar_t* name) {
  const DWORD size = GetEnvironmentVariableW(name, nullptr, 0);
  if (size == 0) {
    return std::nullopt;
  }
  std::wstring value(size, L'\0');
  const DWORD written = GetEnvironmentVariableW(name, value.data(), size);
  if (written == 0) {
    return std::nullopt;
  }
  value.resize(written);
  if (value.empty()) {
    return std::nullopt;
  }
  return std::filesystem::path(value);
}

std::filesystem::path ExecutableDirectory() {
  std::wstring buffer(MAX_PATH, L'\0');
  DWORD size = GetModuleFileNameW(nullptr, buffer.data(),
                                 static_cast<DWORD>(buffer.size()));
  while (size == buffer.size()) {
    buffer.resize(buffer.size() * 2);
    size = GetModuleFileNameW(nullptr, buffer.data(),
                              static_cast<DWORD>(buffer.size()));
  }
  if (size == 0) {
    return {};
  }
  buffer.resize(size);
  return std::filesystem::path(buffer).parent_path();
}

std::filesystem::path SourceTreeRoot() {
#if defined(ERIKA_REPO_ROOT_PATH)
  return std::filesystem::path(ERIKA_REPO_ROOT_PATH);
#else
  std::filesystem::path source_file(__FILE__);
  return source_file.parent_path()  // windows
      .parent_path()                // erika_flutter
      .parent_path()                // packages
      .parent_path();               // repo root
#endif
}

void DebugLog(const std::string& message) {
  OutputDebugStringA(("ErikaFlutterPlugin: " + message + "\n").c_str());
}

double NowSeconds() {
  using clock = std::chrono::steady_clock;
  const auto now = clock::now().time_since_epoch();
  return std::chrono::duration<double>(now).count();
}

double ScaleForWindow(HWND hwnd) {
  if (hwnd == nullptr) {
    return 1.0;
  }
  const UINT dpi = GetDpiForWindow(hwnd);
  if (dpi == 0) {
    return 1.0;
  }
  return std::max(1.0, static_cast<double>(dpi) / 96.0);
}

UINT FrameTimerIntervalMs() {
  const auto env = EnvironmentPath(L"ERIKA_FLUTTER_TARGET_FPS");
  if (!env) {
    return kFrameTimerDefaultIntervalMs;
  }
  try {
    const double fps = std::stod(env->wstring());
    if (!std::isfinite(fps) || fps <= 0.0) {
      return kFrameTimerDefaultIntervalMs;
    }
    const double clamped = std::clamp(fps, 1.0, 1000.0);
    return std::max(kFrameTimerMinIntervalMs,
                    static_cast<UINT>(std::llround(1000.0 / clamped)));
  } catch (...) {
    return kFrameTimerDefaultIntervalMs;
  }
}

void Check(ErikaStatus status, const char* operation) {
  if (status == ErikaStatus_Ok) {
    return;
  }
  throw PluginError(std::string(operation) + " failed with ErikaStatus " +
                    std::to_string(static_cast<int>(status)));
}

const EncodableMap& DictionaryArgs(const EncodableValue* arguments) {
  if (arguments == nullptr) {
    throw PluginError("Arguments must be a dictionary.");
  }
  const auto* map = std::get_if<EncodableMap>(arguments);
  if (map == nullptr) {
    throw PluginError("Arguments must be a dictionary.");
  }
  return *map;
}

const EncodableValue* FindArg(const EncodableMap& args, const char* name) {
  const auto it = args.find(EncodableValue(std::string(name)));
  if (it == args.end()) {
    return nullptr;
  }
  return &it->second;
}

std::optional<int64_t> Int64Value(const EncodableValue* value) {
  if (value == nullptr || std::holds_alternative<std::monostate>(*value)) {
    return std::nullopt;
  }
  if (const auto* v = std::get_if<int32_t>(value)) {
    return static_cast<int64_t>(*v);
  }
  if (const auto* v = std::get_if<int64_t>(value)) {
    return *v;
  }
  if (const auto* v = std::get_if<double>(value)) {
    if (std::isfinite(*v)) {
      return static_cast<int64_t>(*v);
    }
  }
  if (const auto* v = std::get_if<std::string>(value)) {
    try {
      return std::stoll(*v);
    } catch (...) {
      return std::nullopt;
    }
  }
  return std::nullopt;
}

int64_t RequiredInt64(const EncodableMap& args, const char* name) {
  const auto value = Int64Value(FindArg(args, name));
  if (!value) {
    throw PluginError(std::string(name) + " is required.");
  }
  return *value;
}

std::optional<double> DoubleValue(const EncodableValue* value) {
  if (value == nullptr || std::holds_alternative<std::monostate>(*value)) {
    return std::nullopt;
  }
  if (const auto* v = std::get_if<double>(value)) {
    return std::isfinite(*v) ? std::optional<double>(*v) : std::nullopt;
  }
  if (const auto* v = std::get_if<int32_t>(value)) {
    return static_cast<double>(*v);
  }
  if (const auto* v = std::get_if<int64_t>(value)) {
    return static_cast<double>(*v);
  }
  if (const auto* v = std::get_if<std::string>(value)) {
    try {
      const double parsed = std::stod(*v);
      return std::isfinite(parsed) ? std::optional<double>(parsed)
                                   : std::nullopt;
    } catch (...) {
      return std::nullopt;
    }
  }
  return std::nullopt;
}

std::optional<bool> BoolValue(const EncodableValue* value) {
  if (value == nullptr || std::holds_alternative<std::monostate>(*value)) {
    return std::nullopt;
  }
  if (const auto* v = std::get_if<bool>(value)) {
    return *v;
  }
  if (const auto* v = std::get_if<int32_t>(value)) {
    return *v != 0;
  }
  if (const auto* v = std::get_if<int64_t>(value)) {
    return *v != 0;
  }
  if (const auto* v = std::get_if<std::string>(value)) {
    std::string lower = *v;
    std::transform(lower.begin(), lower.end(), lower.begin(),
                   [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    if (lower == "1" || lower == "true" || lower == "yes" || lower == "on") {
      return true;
    }
    if (lower == "0" || lower == "false" || lower == "no" ||
        lower == "off") {
      return false;
    }
  }
  return std::nullopt;
}

std::optional<std::string> StringValue(const EncodableValue* value) {
  if (value == nullptr || std::holds_alternative<std::monostate>(*value)) {
    return std::nullopt;
  }
  if (const auto* v = std::get_if<std::string>(value)) {
    return *v;
  }
  return std::nullopt;
}

std::string RequiredString(const EncodableMap& args, const char* name) {
  const auto value = StringValue(FindArg(args, name));
  if (!value) {
    throw PluginError(std::string(name) + " is required.");
  }
  return *value;
}

int64_t OptionalTrackId(const EncodableValue* value) {
  const auto track_id = Int64Value(value);
  if (!track_id || *track_id < 0) {
    return -1;
  }
  return *track_id;
}

ErikaDanmakuConfig DefaultDanmakuConfig() {
  ErikaDanmakuConfig config{};
  config.enabled = true;
  config.font_size = 30.0f;
  config.opacity = 1.0f;
  config.display_area = 1.0f;
  config.scroll_duration_seconds = 10.0f;
  config.scroll_speed_factor = 1.0f;
  config.track_gap_ratio = 0.15f;
  config.outline_width = 1.0f;
  config.shadow_offset_x = 1.0f;
  config.shadow_offset_y = 1.0f;
  config.allow_scroll_overwrite = true;
  config.shadow_style = 3;
  return config;
}

EncodableValue NullableString(char* value) {
  if (value == nullptr) {
    return EncodableValue();
  }
  return EncodableValue(std::string(value));
}

EncodableValue TrackSelectionToMap(const ErikaTrackSelection& selection) {
  return EncodableValue(EncodableMap{
      {EncodableValue("video"), EncodableValue(static_cast<int64_t>(selection.video))},
      {EncodableValue("audio"), EncodableValue(static_cast<int64_t>(selection.audio))},
      {EncodableValue("subtitle"),
       EncodableValue(static_cast<int64_t>(selection.subtitle))},
  });
}

EncodableValue UpscalerStatusToMap(const ErikaUpscalerStatus& status) {
  return EncodableValue(EncodableMap{
      {EncodableValue("requestedMode"),
       EncodableValue(static_cast<int32_t>(status.requested_mode))},
      {EncodableValue("activeBackend"),
       EncodableValue(static_cast<int32_t>(status.active_backend))},
      {EncodableValue("fallbackCount"),
       EncodableValue(static_cast<int64_t>(status.fallback_count))},
      {EncodableValue("upscaledFrames"),
       EncodableValue(static_cast<int64_t>(status.upscaled_frames))},
      {EncodableValue("lastEncodeMicros"),
       EncodableValue(static_cast<int64_t>(status.last_encode_micros))},
      {EncodableValue("lastGpuMicros"),
       EncodableValue(static_cast<int64_t>(status.last_gpu_micros))},
  });
}

}  // namespace

struct ErikaFlutterPlugin::ErikaNativeLibrary {
  using CreateFn = ErikaPresenterHandle* (*)();
  using CreateWithConfigFn = ErikaPresenterHandle* (*)(ErikaPresenterConfig);
  using CreateWithOutputModeFn = ErikaPresenterHandle* (*)(int32_t, float);
  using DestroyFn = void (*)(ErikaPresenterHandle*);
  using OpenFn = ErikaStatus (*)(ErikaPresenterHandle*, const char*);
  using CommandFn = ErikaStatus (*)(ErikaPresenterHandle*);
  using SeekFn = ErikaStatus (*)(ErikaPresenterHandle*, uint64_t);
  using SetPlaybackRateFn = ErikaStatus (*)(ErikaPresenterHandle*, double);
  using SetVolumeFn = ErikaStatus (*)(ErikaPresenterHandle*, double);
  using SetUpscalerFn = ErikaStatus (*)(ErikaPresenterHandle*, int32_t);
  using GetUpscalerStatusFn =
      ErikaStatus (*)(ErikaPresenterHandle*, ErikaUpscalerStatus*);
  using SelectTrackFn = ErikaStatus (*)(ErikaPresenterHandle*, int64_t);
  using AddExternalSubtitleFn =
      ErikaStatus (*)(ErikaPresenterHandle*, const char*, int64_t*);
  using RemoveSubtitleTrackFn = ErikaStatus (*)(ErikaPresenterHandle*, int64_t);
  using LoadDanmakuFn = ErikaStatus (*)(ErikaPresenterHandle*, const char*);
  using AddDanmakuTrackFn =
      ErikaStatus (*)(ErikaPresenterHandle*, const char*, const char*, int64_t,
                      uint64_t*);
  using RemoveDanmakuTrackFn = ErikaStatus (*)(ErikaPresenterHandle*, uint64_t);
  using SetDanmakuTrackEnabledFn =
      ErikaStatus (*)(ErikaPresenterHandle*, uint64_t, bool);
  using SetDanmakuTrackOffsetFn =
      ErikaStatus (*)(ErikaPresenterHandle*, uint64_t, int64_t);
  using SetDanmakuGlobalOffsetFn =
      ErikaStatus (*)(ErikaPresenterHandle*, int64_t);
  using DanmakuTracksFn =
      ErikaStatus (*)(ErikaPresenterHandle*, ErikaDanmakuTrackInfo*, uintptr_t,
                      uintptr_t*);
  using ClearDanmakuFn = ErikaStatus (*)(ErikaPresenterHandle*);
  using SetDanmakuEnabledFn = ErikaStatus (*)(ErikaPresenterHandle*, bool);
  using SetDanmakuConfigFn =
      ErikaStatus (*)(ErikaPresenterHandle*, const ErikaDanmakuConfig*);
  using GetDanmakuConfigFn =
      ErikaStatus (*)(ErikaPresenterHandle*, ErikaDanmakuConfig*);
  using SetDanmakuFontFn =
      ErikaStatus (*)(ErikaPresenterHandle*, const char*, const char*);
  using TrackSelectionFn =
      ErikaStatus (*)(ErikaPresenterHandle*, ErikaTrackSelection*);
  using TracksFn =
      ErikaStatus (*)(ErikaPresenterHandle*, ErikaTrackInfo*, uintptr_t,
                      uintptr_t*);
  using TrackInfoFreeFn = void (*)(ErikaTrackInfo*);
  using DanmakuTrackInfoFreeFn = void (*)(ErikaDanmakuTrackInfo*);
  using AttachWindowsHwndFn =
      ErikaStatus (*)(ErikaPresenterHandle*, uint64_t, uint64_t, uint32_t,
                      uint32_t, double);
  using ResizeSurfaceFn =
      ErikaStatus (*)(ErikaPresenterHandle*, uint32_t, uint32_t, double);
  using RenderTickFn =
      ErikaStatus (*)(ErikaPresenterHandle*, double, ErikaPresenterStats*);
  using PollEventFn = ErikaStatus (*)(ErikaPresenterHandle*, ErikaEvent*);

  static std::shared_ptr<ErikaNativeLibrary> Shared() {
    static std::mutex mutex;
    static std::weak_ptr<ErikaNativeLibrary> weak;
    std::lock_guard<std::mutex> lock(mutex);
    if (auto shared = weak.lock()) {
      return shared;
    }
    auto shared = std::shared_ptr<ErikaNativeLibrary>(new ErikaNativeLibrary());
    weak = shared;
    return shared;
  }

  ~ErikaNativeLibrary() {
    if (module != nullptr) {
      FreeLibrary(module);
      module = nullptr;
    }
  }

  ErikaPresenterHandle* CreatePresenter(ErikaPresenterConfig config) const {
    if (create_with_config != nullptr) {
      return create_with_config(config);
    }
    if (create_with_output_mode != nullptr) {
      return create_with_output_mode(config.output_mode, config.edr_headroom);
    }
    return create();
  }

  HMODULE module = nullptr;
  CreateFn create = nullptr;
  CreateWithConfigFn create_with_config = nullptr;
  CreateWithOutputModeFn create_with_output_mode = nullptr;
  DestroyFn destroy = nullptr;
  OpenFn open = nullptr;
  CommandFn play = nullptr;
  CommandFn pause = nullptr;
  CommandFn stop = nullptr;
  CommandFn close = nullptr;
  SeekFn seek = nullptr;
  SetPlaybackRateFn set_playback_rate = nullptr;
  SetVolumeFn set_volume = nullptr;
  SetUpscalerFn set_upscaler = nullptr;
  GetUpscalerStatusFn get_upscaler_status = nullptr;
  SelectTrackFn select_audio_track = nullptr;
  SelectTrackFn select_subtitle_track = nullptr;
  AddExternalSubtitleFn add_external_subtitle = nullptr;
  RemoveSubtitleTrackFn remove_subtitle_track = nullptr;
  LoadDanmakuFn load_danmaku_file = nullptr;
  LoadDanmakuFn load_danmaku_json = nullptr;
  AddDanmakuTrackFn add_danmaku_track_file = nullptr;
  AddDanmakuTrackFn add_danmaku_track_json = nullptr;
  RemoveDanmakuTrackFn remove_danmaku_track = nullptr;
  SetDanmakuTrackEnabledFn set_danmaku_track_enabled = nullptr;
  SetDanmakuTrackOffsetFn set_danmaku_track_offset = nullptr;
  SetDanmakuGlobalOffsetFn set_danmaku_global_offset = nullptr;
  DanmakuTracksFn danmaku_tracks = nullptr;
  ClearDanmakuFn clear_danmaku = nullptr;
  SetDanmakuEnabledFn set_danmaku_enabled = nullptr;
  SetDanmakuConfigFn set_danmaku_config = nullptr;
  GetDanmakuConfigFn get_danmaku_config = nullptr;
  SetDanmakuFontFn set_danmaku_font = nullptr;
  LoadDanmakuFn set_danmaku_block_words_json = nullptr;
  TrackSelectionFn track_selection = nullptr;
  TracksFn tracks = nullptr;
  TrackInfoFreeFn free_track_info = nullptr;
  DanmakuTrackInfoFreeFn free_danmaku_track_info = nullptr;
  AttachWindowsHwndFn attach_windows_hwnd = nullptr;
  ResizeSurfaceFn resize_surface = nullptr;
  CommandFn detach_surface = nullptr;
  RenderTickFn render_tick = nullptr;
  PollEventFn poll_event = nullptr;

 private:
  ErikaNativeLibrary() {
    const auto loaded = OpenLibrary();
    module = loaded.first;
    DebugLog("loaded Erika C API from " + loaded.second.string());

    create = LoadRequired<CreateFn>("erika_presenter_create");
    create_with_config =
        LoadOptional<CreateWithConfigFn>("erika_presenter_create_with_config");
    create_with_output_mode = LoadOptional<CreateWithOutputModeFn>(
        "erika_presenter_create_with_output_mode");
    destroy = LoadRequired<DestroyFn>("erika_presenter_destroy");
    open = LoadRequired<OpenFn>("erika_presenter_open");
    play = LoadRequired<CommandFn>("erika_presenter_play");
    pause = LoadRequired<CommandFn>("erika_presenter_pause");
    stop = LoadRequired<CommandFn>("erika_presenter_stop");
    close = LoadRequired<CommandFn>("erika_presenter_close");
    seek = LoadRequired<SeekFn>("erika_presenter_seek");
    set_playback_rate =
        LoadOptional<SetPlaybackRateFn>("erika_presenter_set_playback_rate");
    set_volume = LoadOptional<SetVolumeFn>("erika_presenter_set_volume");
    set_upscaler =
        LoadOptional<SetUpscalerFn>("erika_presenter_set_upscaler");
    get_upscaler_status = LoadOptional<GetUpscalerStatusFn>(
        "erika_presenter_get_upscaler_status");
    select_audio_track =
        LoadRequired<SelectTrackFn>("erika_presenter_select_audio_track");
    select_subtitle_track =
        LoadRequired<SelectTrackFn>("erika_presenter_select_subtitle_track");
    add_external_subtitle =
        LoadRequired<AddExternalSubtitleFn>("erika_presenter_add_external_subtitle");
    remove_subtitle_track =
        LoadRequired<RemoveSubtitleTrackFn>("erika_presenter_remove_subtitle_track");
    load_danmaku_file =
        LoadOptional<LoadDanmakuFn>("erika_presenter_load_danmaku_file");
    load_danmaku_json =
        LoadOptional<LoadDanmakuFn>("erika_presenter_load_danmaku_json");
    add_danmaku_track_file = LoadOptional<AddDanmakuTrackFn>(
        "erika_presenter_add_danmaku_track_file");
    add_danmaku_track_json = LoadOptional<AddDanmakuTrackFn>(
        "erika_presenter_add_danmaku_track_json");
    remove_danmaku_track = LoadOptional<RemoveDanmakuTrackFn>(
        "erika_presenter_remove_danmaku_track");
    set_danmaku_track_enabled = LoadOptional<SetDanmakuTrackEnabledFn>(
        "erika_presenter_set_danmaku_track_enabled");
    set_danmaku_track_offset = LoadOptional<SetDanmakuTrackOffsetFn>(
        "erika_presenter_set_danmaku_track_offset");
    set_danmaku_global_offset = LoadOptional<SetDanmakuGlobalOffsetFn>(
        "erika_presenter_set_danmaku_global_offset");
    danmaku_tracks =
        LoadOptional<DanmakuTracksFn>("erika_presenter_danmaku_tracks");
    clear_danmaku =
        LoadOptional<ClearDanmakuFn>("erika_presenter_clear_danmaku");
    set_danmaku_enabled = LoadOptional<SetDanmakuEnabledFn>(
        "erika_presenter_set_danmaku_enabled");
    set_danmaku_config = LoadOptional<SetDanmakuConfigFn>(
        "erika_presenter_set_danmaku_config_ptr");
    get_danmaku_config = LoadOptional<GetDanmakuConfigFn>(
        "erika_presenter_get_danmaku_config");
    set_danmaku_font =
        LoadOptional<SetDanmakuFontFn>("erika_presenter_set_danmaku_font");
    set_danmaku_block_words_json = LoadOptional<LoadDanmakuFn>(
        "erika_presenter_set_danmaku_block_words_json");
    track_selection =
        LoadRequired<TrackSelectionFn>("erika_presenter_track_selection");
    tracks = LoadRequired<TracksFn>("erika_presenter_tracks");
    free_track_info = LoadRequired<TrackInfoFreeFn>("erika_track_info_free");
    free_danmaku_track_info = LoadOptional<DanmakuTrackInfoFreeFn>(
        "erika_danmaku_track_info_free");
    attach_windows_hwnd =
        LoadRequired<AttachWindowsHwndFn>("erika_presenter_attach_windows_hwnd");
    resize_surface =
        LoadRequired<ResizeSurfaceFn>("erika_presenter_resize_surface");
    detach_surface =
        LoadRequired<CommandFn>("erika_presenter_detach_surface");
    render_tick = LoadRequired<RenderTickFn>("erika_presenter_render_tick");
    poll_event = LoadRequired<PollEventFn>("erika_presenter_poll_event");
  }

  static std::pair<HMODULE, std::filesystem::path> OpenLibrary() {
    std::vector<std::filesystem::path> candidates;
    if (auto value = EnvironmentPath(L"ERIKA_CAPI_DLL")) {
      candidates.push_back(*value);
    }
    if (auto value = EnvironmentPath(L"ERIKA_CAPI_DYLIB")) {
      candidates.push_back(*value);
    }
    const auto exe_dir = ExecutableDirectory();
    if (!exe_dir.empty()) {
      candidates.push_back(exe_dir / L"erika_capi.dll");
    }
    const auto repo_root = SourceTreeRoot();
    candidates.push_back(repo_root / L"target" / L"debug" / L"erika_capi.dll");
    candidates.push_back(repo_root / L"target" / L"release" / L"erika_capi.dll");
    candidates.push_back(L"erika_capi.dll");

    std::ostringstream failures;
    for (const auto& candidate : candidates) {
      SetLastError(0);
      if (HMODULE module = LoadLibraryW(candidate.c_str())) {
        return {module, candidate};
      }
      failures << candidate.string() << " (" << LastErrorMessage() << "); ";
    }
    throw PluginError("Unable to load erika_capi.dll. Tried: " +
                      failures.str());
  }

  template <typename T>
  T LoadRequired(const char* symbol) const {
    auto* raw = GetProcAddress(module, symbol);
    if (raw == nullptr) {
      throw PluginError(std::string("Missing Erika C ABI symbol: ") + symbol);
    }
    return reinterpret_cast<T>(raw);
  }

  template <typename T>
  T LoadOptional(const char* symbol) const {
    auto* raw = GetProcAddress(module, symbol);
    if (raw == nullptr) {
      return nullptr;
    }
    return reinterpret_cast<T>(raw);
  }
};

struct ErikaFlutterPlugin::ErikaOverlayWindow {
  explicit ErikaOverlayWindow(HWND parent_window)
      : parent(parent_window), scale(ScaleForWindow(parent_window)) {
    RegisterWindowClass();
    hwnd = CreateWindowExW(0, kOverlayWindowClassName, L"Erika Video Surface",
                           WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN, 0, 0,
                           1, 1, parent, nullptr, GetModuleHandleW(nullptr),
                           this);
    if (hwnd == nullptr) {
      throw PluginError("Unable to create Windows video overlay HWND: " +
                        LastErrorMessage());
    }
    ShowWindow(hwnd, SW_HIDE);
  }

  ~ErikaOverlayWindow() {
    if (hwnd != nullptr) {
      DestroyWindow(hwnd);
      hwnd = nullptr;
    }
  }

  void SetFrame(double x,
                double y,
                double width,
                double height,
                bool is_visible,
                std::optional<int64_t> generation,
                const std::optional<std::string>& debug_label) {
    if (generation) {
      active_generation = *generation;
    }
    logical_x = x;
    logical_y = y;
    logical_width = width;
    logical_height = height;
    visible = is_visible && width > 0.0 && height > 0.0;
    scale = ScaleForWindow(parent);

    if (debug_label) {
      SetWindowTextW(hwnd, Utf8ToWide(*debug_label).c_str());
    }

    if (!visible) {
      ShowWindow(hwnd, SW_HIDE);
      return;
    }

    const int px = static_cast<int>(std::llround(logical_x * scale));
    const int py = static_cast<int>(std::llround(logical_y * scale));
    const int pw = static_cast<int>(std::llround(logical_width * scale));
    const int ph = static_cast<int>(std::llround(logical_height * scale));
    SetWindowPos(hwnd, HWND_TOP, px, py, std::max(1, pw), std::max(1, ph),
                 SWP_NOACTIVATE | SWP_NOOWNERZORDER);
    ShowWindow(hwnd, SW_SHOWNOACTIVATE);
  }

  uint32_t PixelWidth() const {
    return static_cast<uint32_t>(
        std::max<int64_t>(1, std::llround(logical_width * scale)));
  }

  uint32_t PixelHeight() const {
    return static_cast<uint32_t>(
        std::max<int64_t>(1, std::llround(logical_height * scale)));
  }

  void RefreshScaleAndReposition() {
    SetFrame(logical_x, logical_y, logical_width, logical_height, visible,
             active_generation, std::nullopt);
  }

  static void RegisterWindowClass() {
    static bool registered = false;
    if (registered) {
      return;
    }
    WNDCLASSEXW window_class{};
    window_class.cbSize = sizeof(window_class);
    window_class.style = CS_HREDRAW | CS_VREDRAW | CS_OWNDC;
    window_class.lpfnWndProc = &ErikaOverlayWindow::WndProc;
    window_class.hInstance = GetModuleHandleW(nullptr);
    window_class.hCursor = LoadCursor(nullptr, IDC_ARROW);
    window_class.hbrBackground = static_cast<HBRUSH>(GetStockObject(BLACK_BRUSH));
    window_class.lpszClassName = kOverlayWindowClassName;
    if (!RegisterClassExW(&window_class) && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
      throw PluginError("Unable to register Windows video overlay class: " +
                        LastErrorMessage());
    }
    registered = true;
  }

  static LRESULT CALLBACK WndProc(HWND hwnd,
                                  UINT message,
                                  WPARAM wparam,
                                  LPARAM lparam) {
    if (message == WM_NCCREATE) {
      auto* create = reinterpret_cast<CREATESTRUCTW*>(lparam);
      SetWindowLongPtrW(hwnd, GWLP_USERDATA,
                        reinterpret_cast<LONG_PTR>(create->lpCreateParams));
    }

    switch (message) {
      case WM_ERASEBKGND:
        return 1;
      case WM_NCHITTEST:
        return HTTRANSPARENT;
      default:
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
  }

  HWND parent = nullptr;
  HWND hwnd = nullptr;
  double scale = 1.0;
  double logical_x = 0.0;
  double logical_y = 0.0;
  double logical_width = 1.0;
  double logical_height = 1.0;
  bool visible = false;
  int64_t active_generation = 0;
};

struct ErikaFlutterPlugin::PlayerHost {
  PlayerHost(int64_t player_id,
             std::shared_ptr<ErikaNativeLibrary> native_library,
             ErikaPresenterConfig config)
      : id(player_id), library(std::move(native_library)) {
    handle = library->CreatePresenter(config);
    if (handle == nullptr) {
      throw PluginError("erika_presenter_create returned null.");
    }
    RefreshDanmakuConfigSnapshot();
  }

  ~PlayerHost() {
    if (handle != nullptr) {
      library->detach_surface(handle);
      library->destroy(handle);
      handle = nullptr;
    }
  }

  void Open(const std::string& uri) {
    Check(library->open(handle, uri.c_str()), "open");
  }

  void Play() { Check(library->play(handle), "play"); }
  void Pause() { Check(library->pause(handle), "pause"); }
  void Stop() { Check(library->stop(handle), "stop"); }
  void Close() { Check(library->close(handle), "close"); }

  void Seek(uint64_t position_micros) {
    Check(library->seek(handle, position_micros), "seek");
  }

  void SetPlaybackRate(double rate) {
    if (library->set_playback_rate == nullptr) {
      throw PluginError("Missing Erika C ABI symbol: erika_presenter_set_playback_rate");
    }
    Check(library->set_playback_rate(handle, rate), "set_playback_rate");
  }

  void SetVolume(double volume) {
    if (library->set_volume == nullptr) {
      throw PluginError("Missing Erika C ABI symbol: erika_presenter_set_volume");
    }
    const double clamped = std::isfinite(volume) ? std::clamp(volume, 0.0, 1.0) : 1.0;
    Check(library->set_volume(handle, clamped), "set_volume");
  }

  void SetUpscaler(int32_t mode) {
    if (library->set_upscaler == nullptr) {
      throw PluginError("Missing Erika C ABI symbol: erika_presenter_set_upscaler");
    }
    Check(library->set_upscaler(handle, mode), "set_upscaler");
  }

  EncodableValue GetUpscalerStatus() {
    if (library->get_upscaler_status == nullptr) {
      throw PluginError("Missing Erika C ABI symbol: erika_presenter_get_upscaler_status");
    }
    ErikaUpscalerStatus status{};
    Check(library->get_upscaler_status(handle, &status), "get_upscaler_status");
    return UpscalerStatusToMap(status);
  }

  int64_t AddExternalSubtitle(const std::string& uri) {
    int64_t track_id = 0;
    Check(library->add_external_subtitle(handle, uri.c_str(), &track_id),
          "add_external_subtitle");
    return track_id;
  }

  void RemoveSubtitleTrack(int64_t track_id) {
    Check(library->remove_subtitle_track(handle, track_id),
          "remove_subtitle_track");
  }

  void SelectAudioTrack(int64_t track_id) {
    Check(library->select_audio_track(handle, track_id), "select_audio_track");
  }

  void SelectSubtitleTrack(int64_t track_id) {
    Check(library->select_subtitle_track(handle, track_id),
          "select_subtitle_track");
  }

  void LoadDanmakuFile(const std::string& uri) {
    if (library->load_danmaku_file == nullptr) {
      throw PluginError("Missing Erika C ABI symbol: erika_presenter_load_danmaku_file");
    }
    Check(library->load_danmaku_file(handle, uri.c_str()), "load_danmaku_file");
  }

  void LoadDanmakuJson(const std::string& json) {
    if (library->load_danmaku_json == nullptr) {
      throw PluginError("Missing Erika C ABI symbol: erika_presenter_load_danmaku_json");
    }
    Check(library->load_danmaku_json(handle, json.c_str()), "load_danmaku_json");
  }

  uint64_t AddDanmakuTrackFile(const std::string& uri,
                               const std::optional<std::string>& name,
                               int64_t offset_micros) {
    if (library->add_danmaku_track_file == nullptr) {
      throw PluginError("Missing Erika C ABI symbol: erika_presenter_add_danmaku_track_file");
    }
    uint64_t track_id = 0;
    Check(library->add_danmaku_track_file(
              handle, uri.c_str(), name ? name->c_str() : nullptr,
              offset_micros, &track_id),
          "add_danmaku_track_file");
    return track_id;
  }

  uint64_t AddDanmakuTrackJson(const std::string& json,
                               const std::optional<std::string>& name,
                               int64_t offset_micros) {
    if (library->add_danmaku_track_json == nullptr) {
      throw PluginError("Missing Erika C ABI symbol: erika_presenter_add_danmaku_track_json");
    }
    uint64_t track_id = 0;
    Check(library->add_danmaku_track_json(
              handle, json.c_str(), name ? name->c_str() : nullptr,
              offset_micros, &track_id),
          "add_danmaku_track_json");
    return track_id;
  }

  void RemoveDanmakuTrack(uint64_t track_id) {
    if (library->remove_danmaku_track == nullptr) {
      throw PluginError("Missing Erika C ABI symbol: erika_presenter_remove_danmaku_track");
    }
    Check(library->remove_danmaku_track(handle, track_id),
          "remove_danmaku_track");
  }

  void SetDanmakuTrackEnabled(uint64_t track_id, bool enabled) {
    if (library->set_danmaku_track_enabled == nullptr) {
      throw PluginError("Missing Erika C ABI symbol: erika_presenter_set_danmaku_track_enabled");
    }
    Check(library->set_danmaku_track_enabled(handle, track_id, enabled),
          "set_danmaku_track_enabled");
  }

  void SetDanmakuTrackOffset(uint64_t track_id, int64_t offset_micros) {
    if (library->set_danmaku_track_offset == nullptr) {
      throw PluginError("Missing Erika C ABI symbol: erika_presenter_set_danmaku_track_offset");
    }
    Check(library->set_danmaku_track_offset(handle, track_id, offset_micros),
          "set_danmaku_track_offset");
  }

  void SetDanmakuGlobalOffset(int64_t offset_micros) {
    if (library->set_danmaku_global_offset == nullptr) {
      throw PluginError("Missing Erika C ABI symbol: erika_presenter_set_danmaku_global_offset");
    }
    Check(library->set_danmaku_global_offset(handle, offset_micros),
          "set_danmaku_global_offset");
  }

  EncodableValue DanmakuTracks() {
    if (library->danmaku_tracks == nullptr) {
      throw PluginError("Missing Erika C ABI symbol: erika_presenter_danmaku_tracks");
    }
    uintptr_t len = 0;
    Check(library->danmaku_tracks(handle, nullptr, 0, &len), "danmaku_tracks");
    std::vector<ErikaDanmakuTrackInfo> tracks(len);
    if (len > 0) {
      Check(library->danmaku_tracks(handle, tracks.data(), len, &len),
            "danmaku_tracks");
    }
    EncodableList result;
    result.reserve(tracks.size());
    for (auto& track : tracks) {
      result.push_back(EncodableValue(EncodableMap{
          {EncodableValue("id"), EncodableValue(static_cast<int64_t>(track.id))},
          {EncodableValue("enabled"), EncodableValue(track.enabled)},
          {EncodableValue("offsetMicros"),
           EncodableValue(static_cast<int64_t>(track.offset_micros))},
          {EncodableValue("itemCount"),
           EncodableValue(static_cast<int64_t>(track.item_count))},
          {EncodableValue("name"), NullableString(track.name)},
          {EncodableValue("source"), NullableString(track.source)},
      }));
      if (library->free_danmaku_track_info != nullptr) {
        library->free_danmaku_track_info(&track);
      }
    }
    return EncodableValue(result);
  }

  void ClearDanmaku() {
    if (library->clear_danmaku == nullptr) {
      throw PluginError("Missing Erika C ABI symbol: erika_presenter_clear_danmaku");
    }
    Check(library->clear_danmaku(handle), "clear_danmaku");
  }

  void SetDanmakuEnabled(bool enabled) {
    if (library->set_danmaku_enabled == nullptr) {
      throw PluginError("Missing Erika C ABI symbol: erika_presenter_set_danmaku_enabled");
    }
    Check(library->set_danmaku_enabled(handle, enabled),
          "set_danmaku_enabled");
    current_danmaku_config.enabled = enabled;
  }

  void SetDanmakuConfig(const ErikaDanmakuConfig& config) {
    if (library->set_danmaku_config == nullptr) {
      throw PluginError("Missing Erika C ABI symbol: erika_presenter_set_danmaku_config_ptr");
    }
    ErikaDanmakuConfig copy = config;
    Check(library->set_danmaku_config(handle, &copy), "set_danmaku_config");
    current_danmaku_config = copy;
  }

  void SetDanmakuFont(const std::optional<std::string>& family,
                      const std::optional<std::string>& file_path) {
    if (library->set_danmaku_font == nullptr) {
      throw PluginError("Missing Erika C ABI symbol: erika_presenter_set_danmaku_font");
    }
    Check(library->set_danmaku_font(handle, family ? family->c_str() : nullptr,
                                    file_path ? file_path->c_str() : nullptr),
          "set_danmaku_font");
  }

  void SetDanmakuBlockWordsJson(const std::string& json) {
    if (library->set_danmaku_block_words_json == nullptr) {
      throw PluginError("Missing Erika C ABI symbol: erika_presenter_set_danmaku_block_words_json");
    }
    Check(library->set_danmaku_block_words_json(handle, json.c_str()),
          "set_danmaku_block_words_json");
  }

  EncodableValue Tracks() {
    uintptr_t len = 0;
    Check(library->tracks(handle, nullptr, 0, &len), "tracks");
    std::vector<ErikaTrackInfo> tracks(len);
    if (len > 0) {
      Check(library->tracks(handle, tracks.data(), len, &len), "tracks");
    }
    EncodableList result;
    result.reserve(tracks.size());
    for (auto& track : tracks) {
      result.push_back(EncodableValue(EncodableMap{
          {EncodableValue("id"), EncodableValue(static_cast<int64_t>(track.id))},
          {EncodableValue("kind"),
           EncodableValue(static_cast<int32_t>(track.kind))},
          {EncodableValue("source"),
           EncodableValue(static_cast<int32_t>(track.source))},
          {EncodableValue("selected"), EncodableValue(track.selected)},
          {EncodableValue("canRemove"), EncodableValue(track.can_remove)},
          {EncodableValue("title"), NullableString(track.title)},
          {EncodableValue("language"), NullableString(track.language)},
          {EncodableValue("codec"), NullableString(track.codec)},
      }));
      library->free_track_info(&track);
    }
    return EncodableValue(result);
  }

  EncodableValue TrackSelection() {
    ErikaTrackSelection selection{};
    Check(library->track_selection(handle, &selection), "track_selection");
    return TrackSelectionToMap(selection);
  }

  void AttachOverlay(ErikaOverlayWindow& overlay) {
    const uint32_t width = overlay.PixelWidth();
    const uint32_t height = overlay.PixelHeight();
    const double scale = overlay.scale;
    Check(library->attach_windows_hwnd(
              handle, reinterpret_cast<uint64_t>(overlay.hwnd),
              reinterpret_cast<uint64_t>(GetModuleHandleW(nullptr)), width,
              height, scale),
          "attach_windows_hwnd");
    attached_hwnd = overlay.hwnd;
    attached_view_id = kWindowOverlayViewId;
    surface_attached = true;
    start_time_seconds = NowSeconds();
  }

  void ResizeOverlay(ErikaOverlayWindow& overlay) {
    if (!surface_attached || attached_hwnd != overlay.hwnd) {
      return;
    }
    Check(library->resize_surface(handle, overlay.PixelWidth(),
                                  overlay.PixelHeight(), overlay.scale),
          "resize_surface");
  }

  void Detach(std::optional<int64_t> view_id) {
    if (view_id && attached_view_id != *view_id) {
      return;
    }
    attached_hwnd = nullptr;
    attached_view_id = 0;
    surface_attached = false;
    library->detach_surface(handle);
  }

  void RenderTick(flutter::EventSink<EncodableValue>* event_sink) {
    if (surface_attached) {
      ErikaPresenterStats stats{};
      const double time_seconds = NowSeconds() - start_time_seconds;
      const auto status = library->render_tick(handle, time_seconds, &stats);
      if (status != ErikaStatus_Ok) {
        DebugLog("render_tick failed with ErikaStatus " +
                 std::to_string(static_cast<int>(status)));
      }
    }
    PollEvents(event_sink);
  }

  void PollEvents(flutter::EventSink<EncodableValue>* event_sink) {
    if (event_sink == nullptr) {
      return;
    }
    while (true) {
      ErikaEvent event{};
      const auto status = library->poll_event(handle, &event);
      if (status == ErikaStatus_Ok) {
        event_sink->Success(EventToMap(event));
        continue;
      }
      if (status != ErikaStatus_NoEvent) {
        DebugLog("poll_event failed with ErikaStatus " +
                 std::to_string(static_cast<int>(status)));
      }
      break;
    }
  }

  ErikaDanmakuConfig DanmakuConfigFromArgs(const EncodableMap& args) const {
    ErikaDanmakuConfig config = current_danmaku_config;
    if (auto value = BoolValue(FindArg(args, "enabled"))) {
      config.enabled = *value;
    }
    if (auto value = DoubleValue(FindArg(args, "fontSize"))) {
      config.font_size = static_cast<float>(*value);
    }
    if (auto value = DoubleValue(FindArg(args, "opacity"))) {
      config.opacity = static_cast<float>(*value);
    }
    if (auto value = DoubleValue(FindArg(args, "displayArea"))) {
      config.display_area = static_cast<float>(*value);
    }
    if (auto value = DoubleValue(FindArg(args, "scrollDurationSeconds"))) {
      config.scroll_duration_seconds = static_cast<float>(*value);
    }
    if (auto value = DoubleValue(FindArg(args, "scrollSpeedFactor"))) {
      config.scroll_speed_factor = static_cast<float>(*value);
    }
    if (auto value = DoubleValue(FindArg(args, "trackGapRatio"))) {
      config.track_gap_ratio = static_cast<float>(*value);
    }
    if (auto value = DoubleValue(FindArg(args, "outlineWidth"))) {
      config.outline_width = static_cast<float>(*value);
    }
    if (auto value = DoubleValue(FindArg(args, "shadowOffsetX"))) {
      config.shadow_offset_x = static_cast<float>(*value);
    }
    if (auto value = DoubleValue(FindArg(args, "shadowOffsetY"))) {
      config.shadow_offset_y = static_cast<float>(*value);
    }
    if (auto value = BoolValue(FindArg(args, "mergeDuplicates"))) {
      config.merge_duplicates = *value;
    }
    if (auto value = BoolValue(FindArg(args, "allowStacking"))) {
      config.allow_stacking = *value;
    }
    if (auto value = BoolValue(FindArg(args, "allowScrollOverwrite"))) {
      config.allow_scroll_overwrite = *value;
    }
    if (auto value = Int64Value(FindArg(args, "maxQuantity")); value && *value > 0) {
      config.max_quantity = static_cast<uint32_t>(*value);
    }
    if (auto value = Int64Value(FindArg(args, "maxLinesPerMode"));
        value && *value > 0) {
      config.max_lines_per_mode = static_cast<uint32_t>(*value);
    }
    if (auto value = BoolValue(FindArg(args, "blockTop"))) {
      config.block_top = *value;
    }
    if (auto value = BoolValue(FindArg(args, "blockBottom"))) {
      config.block_bottom = *value;
    }
    if (auto value = BoolValue(FindArg(args, "blockScroll"))) {
      config.block_scroll = *value;
    }
    if (auto value = Int64Value(FindArg(args, "shadowStyle"))) {
      config.shadow_style = static_cast<int32_t>(*value);
    }
    return config;
  }

  EncodableValue EventToMap(const ErikaEvent& event) {
    EncodableMap map{
        {EncodableValue("playerId"), EncodableValue(id)},
        {EncodableValue("kind"), EncodableValue(static_cast<int32_t>(event.kind))},
        {EncodableValue("status"),
         EncodableValue(static_cast<int32_t>(event.status))},
        {EncodableValue("state"),
         EncodableValue(static_cast<int32_t>(event.state))},
        {EncodableValue("durationMicros"),
         EncodableValue(static_cast<int64_t>(event.duration_micros))},
        {EncodableValue("positionMicros"),
         EncodableValue(static_cast<int64_t>(event.position_micros))},
        {EncodableValue("buffering"), EncodableValue(event.buffering)},
        {EncodableValue("video"),
         EncodableValue(EncodableMap{
             {EncodableValue("width"),
              EncodableValue(static_cast<int32_t>(event.video.width))},
             {EncodableValue("height"),
              EncodableValue(static_cast<int32_t>(event.video.height))},
             {EncodableValue("primaries"),
              EncodableValue(static_cast<int32_t>(event.video.primaries))},
             {EncodableValue("transfer"),
              EncodableValue(static_cast<int32_t>(event.video.transfer))},
         })},
        {EncodableValue("tracks"),
         EncodableValue(EncodableMap{
             {EncodableValue("video"),
              EncodableValue(static_cast<int32_t>(event.tracks.video))},
             {EncodableValue("audio"),
              EncodableValue(static_cast<int32_t>(event.tracks.audio))},
             {EncodableValue("subtitle"),
              EncodableValue(static_cast<int32_t>(event.tracks.subtitle))},
         })},
    };
    if (event.kind == ErikaEventKind_TracksChanged ||
        event.kind == ErikaEventKind_TrackSelectionChanged) {
      try {
        map[EncodableValue("trackList")] = Tracks();
        map[EncodableValue("trackSelection")] = TrackSelection();
      } catch (const std::exception& error) {
        DebugLog(std::string("failed to add track details to event: ") +
                 error.what());
        map[EncodableValue("trackList")] = EncodableValue(EncodableList{});
        map[EncodableValue("trackSelection")] =
            TrackSelectionToMap(ErikaTrackSelection{-1, -1, -1});
      }
    }
    return EncodableValue(map);
  }

  void RefreshDanmakuConfigSnapshot() {
    current_danmaku_config = DefaultDanmakuConfig();
    if (library->get_danmaku_config == nullptr) {
      return;
    }
    ErikaDanmakuConfig config{};
    if (library->get_danmaku_config(handle, &config) == ErikaStatus_Ok) {
      current_danmaku_config = config;
    }
  }

  int64_t id = 0;
  std::shared_ptr<ErikaNativeLibrary> library;
  ErikaPresenterHandle* handle = nullptr;
  HWND attached_hwnd = nullptr;
  int64_t attached_view_id = 0;
  bool surface_attached = false;
  double start_time_seconds = NowSeconds();
  ErikaDanmakuConfig current_danmaku_config = DefaultDanmakuConfig();
};

ErikaEventStreamHandler::ErikaEventStreamHandler(ErikaFlutterPlugin* plugin)
    : plugin_(plugin) {}

std::unique_ptr<flutter::StreamHandlerError<EncodableValue>>
ErikaEventStreamHandler::OnListenInternal(
    const EncodableValue* arguments,
    std::unique_ptr<flutter::EventSink<EncodableValue>>&& events) {
  plugin_->SetEventSink(std::move(events));
  return nullptr;
}

std::unique_ptr<flutter::StreamHandlerError<EncodableValue>>
ErikaEventStreamHandler::OnCancelInternal(const EncodableValue* arguments) {
  plugin_->ClearEventSink();
  return nullptr;
}

void ErikaFlutterPlugin::RegisterWithRegistrar(
    flutter::PluginRegistrarWindows* registrar) {
  auto channel =
      std::make_unique<flutter::MethodChannel<EncodableValue>>(
          registrar->messenger(), "erika_flutter/player",
          &flutter::StandardMethodCodec::GetInstance());

  auto plugin = std::make_unique<ErikaFlutterPlugin>(registrar);

  channel->SetMethodCallHandler(
      [plugin_pointer = plugin.get()](const auto& call, auto result) {
        plugin_pointer->HandleMethodCall(call, std::move(result));
      });

  registrar->AddPlugin(std::move(plugin));
}

ErikaFlutterPlugin::ErikaFlutterPlugin(
    flutter::PluginRegistrarWindows* registrar)
    : registrar_(registrar) {
  event_channel_ = std::make_unique<flutter::EventChannel<EncodableValue>>(
      registrar_->messenger(), "erika_flutter/events",
      &flutter::StandardMethodCodec::GetInstance());
  event_channel_->SetStreamHandler(
      std::make_unique<ErikaEventStreamHandler>(this));

  window_proc_delegate_id_ = registrar_->RegisterTopLevelWindowProcDelegate(
      [this](HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam) {
        return OnTopLevelWindowProc(hwnd, message, wparam, lparam);
      });
  StartFrameTimer();
}

ErikaFlutterPlugin::~ErikaFlutterPlugin() {
  StopFrameTimer();
  if (window_proc_delegate_id_ != 0) {
    registrar_->UnregisterTopLevelWindowProcDelegate(window_proc_delegate_id_);
    window_proc_delegate_id_ = 0;
  }
  if (event_channel_) {
    event_channel_->SetStreamHandler(nullptr);
  }
  players_.clear();
  overlay_window_.reset();
}

void ErikaFlutterPlugin::SetEventSink(
    std::unique_ptr<flutter::EventSink<EncodableValue>> sink) {
  event_sink_ = std::move(sink);
}

void ErikaFlutterPlugin::ClearEventSink() {
  event_sink_.reset();
}

HWND ErikaFlutterPlugin::FlutterWindow() const {
  auto* view = registrar_->GetView();
  if (view == nullptr) {
    return nullptr;
  }
  return view->GetNativeWindow();
}

double ErikaFlutterPlugin::BackingScale() const {
  return ScaleForWindow(FlutterWindow());
}

void ErikaFlutterPlugin::StartFrameTimer() {
  if (frame_timer_id_ != 0) {
    return;
  }
  HWND hwnd = FlutterWindow();
  if (hwnd == nullptr) {
    return;
  }
  frame_timer_id_ = reinterpret_cast<UINT_PTR>(this);
  if (!SetTimer(hwnd, frame_timer_id_, FrameTimerIntervalMs(), nullptr)) {
    DebugLog("SetTimer failed: " + LastErrorMessage());
    frame_timer_id_ = 0;
  }
}

void ErikaFlutterPlugin::StopFrameTimer() {
  if (frame_timer_id_ == 0) {
    return;
  }
  if (HWND hwnd = FlutterWindow()) {
    KillTimer(hwnd, frame_timer_id_);
  }
  frame_timer_id_ = 0;
}

void ErikaFlutterPlugin::OnFrameTimer() {
  for (auto& entry : players_) {
    entry.second->RenderTick(event_sink_.get());
  }
}

std::optional<LRESULT> ErikaFlutterPlugin::OnTopLevelWindowProc(
    HWND hwnd,
    UINT message,
    WPARAM wparam,
    LPARAM lparam) {
  if (message == WM_TIMER && wparam == frame_timer_id_) {
    OnFrameTimer();
    return std::nullopt;
  }
  if (message == WM_SIZE || message == WM_DPICHANGED) {
    if (overlay_window_) {
      overlay_window_->RefreshScaleAndReposition();
      ResizeAttachedOverlay();
    }
  }
  if (message == WM_DESTROY) {
    for (auto& entry : players_) {
      entry.second->Detach(std::nullopt);
    }
    overlay_window_.reset();
  }
  return std::nullopt;
}

ErikaFlutterPlugin::ErikaOverlayWindow& ErikaFlutterPlugin::EnsureOverlayWindow() {
  HWND parent = FlutterWindow();
  if (parent == nullptr) {
    throw PluginError("No Flutter HWND is available for Erika overlay.");
  }
  if (!overlay_window_ || overlay_window_->parent != parent) {
    overlay_window_ = std::make_unique<ErikaOverlayWindow>(parent);
  }
  return *overlay_window_;
}

ErikaFlutterPlugin::PlayerHost& ErikaFlutterPlugin::PlayerFromArgs(
    const EncodableMap& args) {
  const int64_t player_id = RequiredInt64(args, "playerId");
  const auto it = players_.find(player_id);
  if (it == players_.end()) {
    throw PluginError("Erika player " + std::to_string(player_id) +
                      " was not found.");
  }
  return *it->second;
}

void ErikaFlutterPlugin::ResizeAttachedOverlay() {
  if (!overlay_window_) {
    return;
  }
  for (auto& entry : players_) {
    if (entry.second->attached_view_id == kWindowOverlayViewId) {
      try {
        entry.second->ResizeOverlay(*overlay_window_);
      } catch (const std::exception& error) {
        DebugLog(std::string("resize_surface failed: ") + error.what());
      }
    }
  }
}

int64_t ErikaFlutterPlugin::CreatePlayer(const EncodableValue* arguments) {
  ErikaPresenterConfig config{};
  config.output_mode = ErikaPresenterOutputMode_Sdr;
  config.edr_headroom = 1.0f;
  config.luma_upscaler = ErikaLumaUpscalerMode_Off;

  if (arguments != nullptr && std::holds_alternative<EncodableMap>(*arguments)) {
    const auto& args = std::get<EncodableMap>(*arguments);
    if (auto value = Int64Value(FindArg(args, "outputMode"))) {
      config.output_mode = static_cast<int32_t>(*value);
    }
    if (auto value = DoubleValue(FindArg(args, "edrHeadroom"))) {
      config.edr_headroom = static_cast<float>(std::max(1.0, *value));
    }
  }

  const int64_t id = next_player_id_++;
  players_[id] = std::make_unique<PlayerHost>(
      id, ErikaNativeLibrary::Shared(), config);
  StartFrameTimer();
  return id;
}

void ErikaFlutterPlugin::RemovePlayer(int64_t player_id) {
  players_.erase(player_id);
}

void ErikaFlutterPlugin::SendEvent(EncodableValue event) {
  if (event_sink_ != nullptr) {
    event_sink_->Success(event);
  }
}

void ErikaFlutterPlugin::HandleMethodCall(
    const flutter::MethodCall<EncodableValue>& method_call,
    std::unique_ptr<flutter::MethodResult<EncodableValue>> result) {
  const auto& method = method_call.method_name();
  try {
    if (method == "create") {
      result->Success(EncodableValue(CreatePlayer(method_call.arguments())));
      return;
    }

    const EncodableMap& args = DictionaryArgs(method_call.arguments());

    if (method == "dispose") {
      RemovePlayer(RequiredInt64(args, "playerId"));
      result->Success();
    } else if (method == "open") {
      PlayerFromArgs(args).Open(RequiredString(args, "uri"));
      result->Success();
    } else if (method == "play") {
      PlayerFromArgs(args).Play();
      result->Success();
    } else if (method == "pause") {
      PlayerFromArgs(args).Pause();
      result->Success();
    } else if (method == "stop") {
      PlayerFromArgs(args).Stop();
      result->Success();
    } else if (method == "close") {
      PlayerFromArgs(args).Close();
      result->Success();
    } else if (method == "seek") {
      PlayerFromArgs(args).Seek(
          static_cast<uint64_t>(std::max<int64_t>(
              0, RequiredInt64(args, "positionMicros"))));
      result->Success();
    } else if (method == "setPlaybackRate") {
      PlayerFromArgs(args).SetPlaybackRate(
          DoubleValue(FindArg(args, "rate")).value_or(1.0));
      result->Success();
    } else if (method == "setVolume") {
      PlayerFromArgs(args).SetVolume(
          DoubleValue(FindArg(args, "volume")).value_or(1.0));
      result->Success();
    } else if (method == "setUpscaler") {
      PlayerFromArgs(args).SetUpscaler(
          static_cast<int32_t>(RequiredInt64(args, "mode")));
      result->Success();
    } else if (method == "getUpscalerStatus") {
      result->Success(PlayerFromArgs(args).GetUpscalerStatus());
    } else if (method == "addExternalSubtitle") {
      result->Success(EncodableValue(
          PlayerFromArgs(args).AddExternalSubtitle(RequiredString(args, "uri"))));
    } else if (method == "removeSubtitleTrack") {
      PlayerFromArgs(args).RemoveSubtitleTrack(
          RequiredInt64(args, "trackId"));
      result->Success();
    } else if (method == "loadDanmakuFile") {
      PlayerFromArgs(args).LoadDanmakuFile(RequiredString(args, "uri"));
      result->Success();
    } else if (method == "loadDanmakuJson") {
      PlayerFromArgs(args).LoadDanmakuJson(RequiredString(args, "json"));
      result->Success();
    } else if (method == "addDanmakuTrackFile") {
      result->Success(EncodableValue(static_cast<int64_t>(
          PlayerFromArgs(args).AddDanmakuTrackFile(
              RequiredString(args, "uri"), StringValue(FindArg(args, "name")),
              Int64Value(FindArg(args, "offsetMicros")).value_or(0)))));
    } else if (method == "addDanmakuTrackJson") {
      result->Success(EncodableValue(static_cast<int64_t>(
          PlayerFromArgs(args).AddDanmakuTrackJson(
              RequiredString(args, "json"), StringValue(FindArg(args, "name")),
              Int64Value(FindArg(args, "offsetMicros")).value_or(0)))));
    } else if (method == "removeDanmakuTrack") {
      PlayerFromArgs(args).RemoveDanmakuTrack(
          static_cast<uint64_t>(RequiredInt64(args, "trackId")));
      result->Success();
    } else if (method == "setDanmakuTrackEnabled") {
      PlayerFromArgs(args).SetDanmakuTrackEnabled(
          static_cast<uint64_t>(RequiredInt64(args, "trackId")),
          BoolValue(FindArg(args, "enabled")).value_or(true));
      result->Success();
    } else if (method == "setDanmakuTrackOffset") {
      PlayerFromArgs(args).SetDanmakuTrackOffset(
          static_cast<uint64_t>(RequiredInt64(args, "trackId")),
          Int64Value(FindArg(args, "offsetMicros")).value_or(0));
      result->Success();
    } else if (method == "setDanmakuGlobalOffset") {
      PlayerFromArgs(args).SetDanmakuGlobalOffset(
          Int64Value(FindArg(args, "offsetMicros")).value_or(0));
      result->Success();
    } else if (method == "danmakuTracks") {
      result->Success(PlayerFromArgs(args).DanmakuTracks());
    } else if (method == "clearDanmaku") {
      PlayerFromArgs(args).ClearDanmaku();
      result->Success();
    } else if (method == "setDanmakuEnabled") {
      PlayerFromArgs(args).SetDanmakuEnabled(
          BoolValue(FindArg(args, "enabled")).value_or(true));
      result->Success();
    } else if (method == "setDanmakuConfig") {
      auto& host = PlayerFromArgs(args);
      host.SetDanmakuConfig(host.DanmakuConfigFromArgs(args));
      const bool has_font = FindArg(args, "customFontFamily") != nullptr ||
                            FindArg(args, "customFontFilePath") != nullptr;
      if (has_font) {
        host.SetDanmakuFont(StringValue(FindArg(args, "customFontFamily")),
                            StringValue(FindArg(args, "customFontFilePath")));
      }
      if (auto block_words = StringValue(FindArg(args, "blockWordsJson"))) {
        host.SetDanmakuBlockWordsJson(*block_words);
      }
      result->Success();
    } else if (method == "selectAudioTrack") {
      PlayerFromArgs(args).SelectAudioTrack(OptionalTrackId(FindArg(args, "trackId")));
      result->Success();
    } else if (method == "selectSubtitleTrack") {
      PlayerFromArgs(args).SelectSubtitleTrack(
          OptionalTrackId(FindArg(args, "trackId")));
      result->Success();
    } else if (method == "tracks") {
      result->Success(PlayerFromArgs(args).Tracks());
    } else if (method == "screenshot") {
      result->Success();
    } else if (method == "attachView") {
      auto& host = PlayerFromArgs(args);
      const int64_t view_id = RequiredInt64(args, "viewId");
      if (view_id != kWindowOverlayViewId) {
        throw PluginError("Erika video view " + std::to_string(view_id) +
                          " was not found.");
      }
      host.AttachOverlay(EnsureOverlayWindow());
      result->Success();
    } else if (method == "detachView") {
      PlayerFromArgs(args).Detach(RequiredInt64(args, "viewId"));
      result->Success();
    } else if (method == "attachOverlay") {
      auto& host = PlayerFromArgs(args);
      host.AttachOverlay(EnsureOverlayWindow());
      result->Success(EncodableValue(kWindowOverlayViewId));
    } else if (method == "detachOverlay") {
      auto& host = PlayerFromArgs(args);
      const auto generation = Int64Value(FindArg(args, "generation"));
      if (generation && overlay_window_ &&
          *generation != overlay_window_->active_generation) {
        result->Success();
        return;
      }
      host.Detach(kWindowOverlayViewId);
      if (overlay_window_) {
        overlay_window_->SetFrame(0.0, 0.0, 0.0, 0.0, false, generation,
                                  std::nullopt);
      }
      result->Success();
    } else if (method == "setOverlayFrame") {
      auto& overlay = EnsureOverlayWindow();
      overlay.SetFrame(DoubleValue(FindArg(args, "x")).value_or(0.0),
                       DoubleValue(FindArg(args, "y")).value_or(0.0),
                       DoubleValue(FindArg(args, "width")).value_or(0.0),
                       DoubleValue(FindArg(args, "height")).value_or(0.0),
                       BoolValue(FindArg(args, "visible")).value_or(true),
                       Int64Value(FindArg(args, "generation")),
                       StringValue(FindArg(args, "debugLabel")));
      ResizeAttachedOverlay();
      result->Success();
    } else {
      result->NotImplemented();
    }
  } catch (const std::exception& error) {
    result->Error("ERIKA_ERROR", error.what());
  }
}

}  // namespace erika_flutter
