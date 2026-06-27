#ifndef FLUTTER_PLUGIN_ERIKA_FLUTTER_PLUGIN_H_
#define FLUTTER_PLUGIN_ERIKA_FLUTTER_PLUGIN_H_

#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include <flutter/encodable_value.h>
#include <flutter/event_channel.h>
#include <flutter/event_sink.h>
#include <flutter/event_stream_handler.h>
#include <flutter/method_channel.h>
#include <flutter/plugin_registrar_windows.h>

#include <memory>
#include <optional>
#include <unordered_map>

#include "erika.h"

namespace erika_flutter {

class ErikaFlutterPlugin;

class ErikaEventStreamHandler
    : public flutter::StreamHandler<flutter::EncodableValue> {
 public:
  explicit ErikaEventStreamHandler(ErikaFlutterPlugin* plugin);

 protected:
  std::unique_ptr<flutter::StreamHandlerError<flutter::EncodableValue>>
  OnListenInternal(
      const flutter::EncodableValue* arguments,
      std::unique_ptr<flutter::EventSink<flutter::EncodableValue>>&& events)
      override;

  std::unique_ptr<flutter::StreamHandlerError<flutter::EncodableValue>>
  OnCancelInternal(const flutter::EncodableValue* arguments) override;

 private:
  ErikaFlutterPlugin* plugin_;
};

class ErikaFlutterPlugin : public flutter::Plugin {
 public:
  static void RegisterWithRegistrar(flutter::PluginRegistrarWindows* registrar);

  explicit ErikaFlutterPlugin(flutter::PluginRegistrarWindows* registrar);
  ~ErikaFlutterPlugin() override;

  ErikaFlutterPlugin(const ErikaFlutterPlugin&) = delete;
  ErikaFlutterPlugin& operator=(const ErikaFlutterPlugin&) = delete;

  void HandleMethodCall(
      const flutter::MethodCall<flutter::EncodableValue>& method_call,
      std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>> result);

  void SetEventSink(
      std::unique_ptr<flutter::EventSink<flutter::EncodableValue>> sink);
  void ClearEventSink();

 private:
  struct ErikaNativeLibrary;
  struct ErikaOverlayWindow;
  struct PlayerHost;

  friend class ErikaEventStreamHandler;

  HWND FlutterWindow() const;
  double BackingScale() const;
  void StartFrameTimer();
  void StopFrameTimer();
  void OnFrameTimer();
  static void CALLBACK FrameTimerProc(HWND hwnd,
                                      UINT message,
                                      UINT_PTR timer_id,
                                      DWORD time);
  std::optional<LRESULT> OnTopLevelWindowProc(HWND hwnd,
                                              UINT message,
                                              WPARAM wparam,
                                              LPARAM lparam);

  ErikaOverlayWindow& EnsureOverlayWindow();
  PlayerHost& PlayerFromArgs(const flutter::EncodableMap& args);
  void ResizeAttachedOverlay();
  int64_t CreatePlayer(const flutter::EncodableValue* arguments);
  void RemovePlayer(int64_t player_id);
  void SendEvent(flutter::EncodableValue event);

  flutter::PluginRegistrarWindows* registrar_ = nullptr;
  std::unique_ptr<flutter::EventChannel<flutter::EncodableValue>>
      event_channel_;
  std::unique_ptr<flutter::EventSink<flutter::EncodableValue>> event_sink_;
  std::unordered_map<int64_t, std::unique_ptr<PlayerHost>> players_;
  std::unique_ptr<ErikaOverlayWindow> overlay_window_;
  int64_t next_player_id_ = 1;
  int window_proc_delegate_id_ = 0;
  UINT_PTR frame_timer_id_ = 0;
  bool in_frame_timer_ = false;
};

}  // namespace erika_flutter

#endif  // FLUTTER_PLUGIN_ERIKA_FLUTTER_PLUGIN_H_
