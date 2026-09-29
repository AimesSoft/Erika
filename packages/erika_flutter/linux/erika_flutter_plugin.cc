#include "include/erika_flutter/erika_flutter_plugin.h"
#include "erika.h"
#include <epoxy/egl.h>
#include <epoxy/glx.h>
#include <algorithm>
#include <cmath>
#include <cstring>
#include <map>
#include <memory>
#include <mutex>
#include <stdexcept>
#include <string>
#include <vector>

// GTK/Flutter may leave a GLX or EGL context current on the platform thread.
// Mesa rejects binding Erika's EGL context while a GLX context is current.
// Restore the host context after every native call, including error paths.
class HostGraphicsContext {
 public:
  HostGraphicsContext()
      : egl_display_(eglGetCurrentDisplay()), egl_context_(eglGetCurrentContext()),
        egl_draw_(eglGetCurrentSurface(EGL_DRAW)), egl_read_(eglGetCurrentSurface(EGL_READ)),
        egl_api_(eglQueryAPI()), glx_display_(glXGetCurrentDisplay()),
        glx_context_(glXGetCurrentContext()), glx_draw_(glXGetCurrentDrawable()),
        glx_read_(glXGetCurrentReadDrawable()) {
    if (egl_context_ != EGL_NO_CONTEXT)
      eglMakeCurrent(egl_display_, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
    if (glx_context_) glXMakeContextCurrent(glx_display_, 0, 0, nullptr);
  }
  ~HostGraphicsContext() {
    EGLDisplay current = eglGetCurrentDisplay();
    if (current != EGL_NO_DISPLAY)
      eglMakeCurrent(current, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
    eglBindAPI(egl_api_);
    if (egl_context_ != EGL_NO_CONTEXT)
      eglMakeCurrent(egl_display_, egl_draw_, egl_read_, egl_context_);
    if (glx_context_)
      glXMakeContextCurrent(glx_display_, glx_draw_, glx_read_, glx_context_);
  }
 private:
  EGLDisplay egl_display_;
  EGLContext egl_context_;
  EGLSurface egl_draw_, egl_read_;
  EGLenum egl_api_;
  Display* glx_display_;
  GLXContext glx_context_;
  GLXDrawable glx_draw_, glx_read_;
};

// Main-thread producers never modify published pixels. Flutter's raster
// thread keeps the returned buffer alive until its next copy_pixels call.
struct Pixels { uint32_t width = 1, height = 1; std::vector<uint8_t> rgba{0,0,0,255}; };
struct PixelState { std::mutex mutex; std::shared_ptr<Pixels> pending, published = std::make_shared<Pixels>(); };
typedef struct { FlPixelBufferTexture parent; PixelState* pixels; } ErikaTexture;
typedef struct { FlPixelBufferTextureClass parent_class; } ErikaTextureClass;
G_DEFINE_TYPE(ErikaTexture, erika_texture, fl_pixel_buffer_texture_get_type())
static gboolean copy_pixels(FlPixelBufferTexture* texture, const uint8_t** buffer,
                            uint32_t* width, uint32_t* height, GError**) {
  auto* state = reinterpret_cast<ErikaTexture*>(texture)->pixels;
  std::lock_guard<std::mutex> lock(state->mutex);
  if (state->pending) state->published = std::move(state->pending);
  *buffer = state->published->rgba.data();
  *width = state->published->width; *height = state->published->height;
  return TRUE;
}
static void texture_finalize(GObject* object) {
  delete reinterpret_cast<ErikaTexture*>(object)->pixels;
  G_OBJECT_CLASS(erika_texture_parent_class)->finalize(object);
}
static void erika_texture_class_init(ErikaTextureClass* klass) {
  FL_PIXEL_BUFFER_TEXTURE_CLASS(klass)->copy_pixels = copy_pixels;
  G_OBJECT_CLASS(klass)->finalize = texture_finalize;
}
static void erika_texture_init(ErikaTexture* self) { self->pixels = new PixelState(); }

struct Player {
  ErikaPresenterHandle* handle = nullptr;
  int64_t texture = 0;
  ~Player() { if (handle) erika_presenter_destroy(handle); }
};
struct Texture { ErikaTexture* object; uint32_t width, height; double scale; };
struct Plugin {
  FlTextureRegistrar* registrar;
  FlEventChannel* events;
  guint timer = 0;
  bool listening = false;
  int64_t next_id = 1;
  std::map<int64_t, std::unique_ptr<Player>> players;
  std::map<int64_t, Texture> textures;
  ~Plugin() {
    HostGraphicsContext host_context;
    if (timer) g_source_remove(timer);
    players.clear();
    for (auto& item : textures) {
      fl_texture_registrar_unregister_texture(registrar, FL_TEXTURE(item.second.object));
      g_object_unref(item.second.object);
    }
    fl_event_channel_set_stream_handlers(events, nullptr, nullptr, nullptr, nullptr);
    g_object_unref(events); g_object_unref(registrar);
  }
};
static FlValue* arg(FlValue* args, const char* key) {
  return args && fl_value_get_type(args) == FL_VALUE_TYPE_MAP ? fl_value_lookup_string(args, key) : nullptr;
}
static double number(FlValue* args, const char* key, double fallback = 0) {
  auto* value = arg(args, key);
  if (!value) return fallback;
  if (fl_value_get_type(value) == FL_VALUE_TYPE_INT) return fl_value_get_int(value);
  if (fl_value_get_type(value) == FL_VALUE_TYPE_FLOAT) return fl_value_get_float(value);
  return fallback;
}
static const char* string_arg(FlValue* args, const char* key) {
  auto* value = arg(args, key);
  return value && fl_value_get_type(value) == FL_VALUE_TYPE_STRING ? fl_value_get_string(value) : nullptr;
}
static bool boolean(FlValue* args, const char* key) {
  auto* value = arg(args, key);
  return value && fl_value_get_type(value) == FL_VALUE_TYPE_BOOL && fl_value_get_bool(value);
}
static void check(ErikaStatus status) {
  if (status == ErikaStatus_Ok) return;
  char* raw = erika_last_error_message();
  std::string message = raw ? raw : "Erika operation failed";
  erika_string_free(raw);
  throw std::runtime_error(message);
}
static FlValue* decode_response(char* raw) {
  if (!raw) return nullptr;
  g_autoptr(FlJsonMessageCodec) codec = fl_json_message_codec_new();
  g_autoptr(GBytes) bytes = g_bytes_new(raw, strlen(raw));
  erika_string_free(raw);
  g_autoptr(GError) error = nullptr;
  g_autoptr(FlValue) envelope = fl_message_codec_decode_message(FL_MESSAGE_CODEC(codec), bytes, &error);
  if (!envelope) throw std::runtime_error(error ? error->message : "Invalid Erika JSON");
  if (!boolean(envelope, "ok")) {
    const char* message = string_arg(envelope, "error");
    throw std::runtime_error(message ? message : "Erika operation failed");
  }
  auto* result = arg(envelope, "value");
  return result ? fl_value_ref(result) : fl_value_new_null();
}
static void success(FlMethodCall* call, FlValue* value = nullptr) {
  fl_method_call_respond_success(call, value, nullptr);
}
static void send_event(Plugin* plugin, int64_t id, FlValue* event) {
  if (!plugin->listening || !event || fl_value_get_type(event) != FL_VALUE_TYPE_MAP) return;
  fl_value_set_string_take(event, "playerId", fl_value_new_int(id));
  fl_event_channel_send(plugin->events, event, nullptr, nullptr);
}
static gboolean tick(gpointer data) {
  HostGraphicsContext host_context;
  auto* plugin = static_cast<Plugin*>(data);
  for (auto& entry : plugin->players) {
    auto& player = *entry.second;
    try {
      ErikaPresenterStats stats{};
      check(erika_presenter_render_tick(player.handle, g_get_monotonic_time() / 1000000.0, &stats));
      auto texture = plugin->textures.find(player.texture);
      if (texture != plugin->textures.end()) {
        auto& target = texture->second;
        auto pixels = std::make_shared<Pixels>();
        pixels->rgba.resize(static_cast<size_t>(target.width) * target.height * 4);
        auto status = erika_presenter_copy_flutter_frame_rgba(player.handle, pixels->rgba.data(), pixels->rgba.size(), &pixels->width, &pixels->height);
        if (status == ErikaStatus_Ok) {
          {
            std::lock_guard<std::mutex> lock(target.object->pixels->mutex);
            target.object->pixels->pending = std::move(pixels);
          }
          fl_texture_registrar_mark_texture_frame_available(plugin->registrar, FL_TEXTURE(target.object));
        } else if (status != ErikaStatus_NoEvent) check(status);
      }
      for (int count = 0; count < 128; ++count) {
        g_autoptr(FlValue) event = decode_response(erika_presenter_poll_event_json(player.handle));
        if (!event) break;
        auto kind = static_cast<int>(number(event, "kind"));
        if (kind == ErikaEventKind_TracksChanged || kind == ErikaEventKind_TrackSelectionChanged) {
          g_autoptr(FlValue) tracks = decode_response(erika_presenter_invoke_json(player.handle, "tracks", "{}"));
          fl_value_set_string(event, "trackList", tracks);
        }
        send_event(plugin, entry.first, event);
      }
    } catch (const std::exception& error) {
      g_warning("Erika Linux tick: %s", error.what());
      g_autoptr(FlValue) event = fl_value_new_map();
      fl_value_set_string_take(event, "kind", fl_value_new_int(ErikaEventKind_Error));
      fl_value_set_string_take(event, "error", fl_value_new_string(error.what()));
      send_event(plugin, entry.first, event);
    }
  }
  return G_SOURCE_CONTINUE;
}
static void detach_texture(Plugin* plugin, int64_t id) {
  for (auto& item : plugin->players) if (item.second->texture == id) {
    check(erika_presenter_detach_surface(item.second->handle)); item.second->texture = 0;
  }
}
static void method_call(FlMethodChannel*, FlMethodCall* call, gpointer data) {
  HostGraphicsContext host_context;
  auto* plugin = static_cast<Plugin*>(data);
  std::string method = fl_method_call_get_name(call);
  FlValue* args = fl_method_call_get_args(call);
  try {
    if (method == "create") {
      auto player = std::make_unique<Player>();
      ErikaPresenterConfig config{};
      config.output_mode = static_cast<int32_t>(number(args, "outputMode"));
      config.edr_headroom = static_cast<float>(number(args, "edrHeadroom", 1));
      config.luma_upscaler = static_cast<int32_t>(number(args, "upscaler"));
      config.video_alpha_mode = static_cast<int32_t>(number(args, "videoAlphaMode"));
      player->handle = erika_presenter_create_with_config(config);
      if (!player->handle) check(ErikaStatus_PlayerError);
      if (g_strcmp0(g_getenv("ERIKA_DEBUG_HUD"), "1") == 0)
        check(erika_presenter_set_debug_hud_enabled(player->handle, true));
      auto id = plugin->next_id++;
      plugin->players.emplace(id, std::move(player));
      g_autoptr(FlValue) value = fl_value_new_int(id); success(call, value); return;
    }
    if (method == "createTexture") {
      uint32_t width = std::clamp(number(args, "width", 1), 1.0, 8192.0);
      uint32_t height = std::clamp(number(args, "height", 1), 1.0, 8192.0);
      auto* texture = reinterpret_cast<ErikaTexture*>(g_object_new(erika_texture_get_type(), nullptr));
      if (!fl_texture_registrar_register_texture(plugin->registrar, FL_TEXTURE(texture))) {
        g_object_unref(texture); throw std::runtime_error("Flutter texture registration failed");
      }
      auto id = fl_texture_get_id(FL_TEXTURE(texture));
      plugin->textures.emplace(id, Texture{texture, width, height, number(args, "scale", 1)});
      g_autoptr(FlValue) value = fl_value_new_int(id); success(call, value); return;
    }
    if (method == "resizeTexture" || method == "releaseTexture") {
      auto id = static_cast<int64_t>(number(args, "textureId"));
      auto found = plugin->textures.find(id);
      if (found == plugin->textures.end()) { success(call); return; }
      if (method == "releaseTexture") {
        detach_texture(plugin, id);
        fl_texture_registrar_unregister_texture(plugin->registrar, FL_TEXTURE(found->second.object));
        g_object_unref(found->second.object); plugin->textures.erase(found);
      } else {
        auto& target = found->second;
        target.width = std::clamp(number(args, "width", 1), 1.0, 8192.0);
        target.height = std::clamp(number(args, "height", 1), 1.0, 8192.0);
        target.scale = number(args, "scale", 1);
        for (auto& item : plugin->players) if (item.second->texture == id)
          check(erika_presenter_resize_surface(item.second->handle, target.width, target.height, target.scale));
      }
      success(call); return;
    }
    auto id = static_cast<int64_t>(number(args, "playerId"));
    auto found = plugin->players.find(id);
    if (found == plugin->players.end()) {
      if (method == "dispose" || method == "detachView") { success(call); return; }
      throw std::runtime_error("Unknown Erika player");
    }
    auto& player = *found->second;
    if (method == "dispose") { plugin->players.erase(found); success(call); return; }
    if (method == "attachView") {
      auto texture_id = static_cast<int64_t>(number(args, "viewId"));
      const auto& texture = plugin->textures.at(texture_id);
      detach_texture(plugin, texture_id);
      check(erika_presenter_attach_flutter_texture(player.handle, ErikaFlutterTextureKind_LinuxTextureRegistrar,
        texture_id, texture.width, texture.height, texture.scale));
      player.texture = texture_id; success(call); return;
    }
    if (method == "detachView") {
      if (player.texture == static_cast<int64_t>(number(args, "viewId"))) {
        check(erika_presenter_detach_surface(player.handle)); player.texture = 0;
      }
      success(call); return;
    }
    if (method == "setMediaMetadata" || method == "setSystemMediaNavigation") {
      // Desktop MPRIS is optional; these hints do not change playback.
      success(call); return;
    }
    if (method == "registerSubtitleMemoryFont") {
      auto* bytes = arg(args, "data"); uint64_t font_id = 0;
      if (!bytes || fl_value_get_type(bytes) != FL_VALUE_TYPE_UINT8_LIST) throw std::runtime_error("Expected font bytes");
      check(erika_presenter_register_subtitle_memory_font(player.handle, fl_value_get_uint8_list(bytes), fl_value_get_length(bytes), &font_id));
      g_autoptr(FlValue) value = fl_value_new_int(font_id); success(call, value); return;
    }
    if (method == "setSubtitleStyle") {
      ErikaSubtitleStyle style{};
      style.font_family = string_arg(args, "fontFamily"); style.font_file_path = string_arg(args, "fontFilePath");
      style.primary_color_rgba = number(args, "primaryColorRgba", 0xFFFFFFFF);
      style.outline_color_rgba = number(args, "outlineColorRgba", 0x0000007F);
      style.font_size = number(args, "fontSize", 48); style.outline_width = number(args, "outlineWidth", 2);
      style.bold = boolean(args, "bold"); style.italic = boolean(args, "italic");
      style.underline = boolean(args, "underline"); style.strike_out = boolean(args, "strikeOut");
      style.spacing = number(args, "spacing"); style.scale_x_percent = number(args, "scaleXPercent", 100);
      style.scale_y_percent = number(args, "scaleYPercent", 100); style.border_style = number(args, "borderStyle", 1);
      style.shadow_depth = number(args, "shadowDepth"); style.blur = number(args, "blur");
      style.alignment = number(args, "alignment", 2); style.margin_left = number(args, "marginLeft", 48);
      style.margin_right = number(args, "marginRight", 48); style.margin_vertical = number(args, "marginVertical", 54);
      style.override_mask = number(args, "overrideMask");
      check(erika_presenter_set_subtitle_style(player.handle, style)); success(call); return;
    }
    if (method == "screenshot") {
      uint32_t width = std::clamp(number(args, "width", 1280), 1.0, 8192.0);
      uint32_t height = std::clamp(number(args, "height", 720), 1.0, 8192.0);
      std::vector<uint8_t> rgba(static_cast<size_t>(width) * height * 4);
      check(erika_presenter_capture_frame_rgba(player.handle, width, height, rgba.data(), rgba.size()));
      // The Dart screenshot contract is tightly packed RGBA, not encoded PNG.
      g_autoptr(FlValue) value = fl_value_new_uint8_list(rgba.data(), rgba.size());
      success(call, value); return;
    }
    g_autoptr(FlJsonMessageCodec) codec = fl_json_message_codec_new();
    g_autoptr(GError) error = nullptr;
    g_autoptr(GBytes) bytes = fl_message_codec_encode_message(FL_MESSAGE_CODEC(codec), args, &error);
    if (!bytes) throw std::runtime_error(error->message);
    gsize size; const auto* encoded = static_cast<const char*>(g_bytes_get_data(bytes, &size));
    std::string json(encoded, size);
    g_autoptr(FlValue) result = decode_response(erika_presenter_invoke_json(player.handle, method.c_str(), json.c_str()));
    success(call, result);
  } catch (const std::exception& error) {
    fl_method_call_respond_error(call, "erika_linux", error.what(), nullptr, nullptr);
  }
}
static FlMethodErrorResponse* listen(FlEventChannel*, FlValue*, gpointer data) {
  static_cast<Plugin*>(data)->listening = true; return nullptr;
}
static FlMethodErrorResponse* cancel(FlEventChannel*, FlValue*, gpointer data) {
  static_cast<Plugin*>(data)->listening = false; return nullptr;
}
void erika_flutter_plugin_register_with_registrar(FlPluginRegistrar* registrar) {
  auto* plugin = new Plugin();
  plugin->registrar = FL_TEXTURE_REGISTRAR(g_object_ref(fl_plugin_registrar_get_texture_registrar(registrar)));
  auto* messenger = fl_plugin_registrar_get_messenger(registrar);
  g_autoptr(FlStandardMethodCodec) codec = fl_standard_method_codec_new();
  plugin->events = fl_event_channel_new(messenger, "erika_flutter/events", FL_METHOD_CODEC(codec));
  fl_event_channel_set_stream_handlers(plugin->events, listen, cancel, plugin, nullptr);
  g_autoptr(FlMethodChannel) channel = fl_method_channel_new(messenger, "erika_flutter/player", FL_METHOD_CODEC(codec));
  fl_method_channel_set_method_call_handler(channel, method_call, plugin, [](gpointer data) { delete static_cast<Plugin*>(data); });
  plugin->timer = g_timeout_add(16, tick, plugin);
}
