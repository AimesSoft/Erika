// Include the implementation so the test peer exercises the actual internal
// PlayerHost and native-library loader, without starting a Flutter/DWM window.
#include "../erika_flutter_plugin.cpp"

#include <cstdlib>
#include <iostream>

namespace erika_flutter {

class ErikaFlutterPluginTestPeer {
 public:
  static void Require(bool value) { if (!value) std::abort(); }

  class Registrar : public flutter::TextureRegistrar {
   public:
    int marked_frames = 0;
    int64_t RegisterTexture(flutter::TextureVariant*) override { return 13; }
    bool MarkTextureFrameAvailable(int64_t) override {
      ++marked_frames;
      return true;
    }
    void UnregisterTexture(int64_t, std::function<void()> callback) override {
      if (callback) callback();
    }
    bool UnregisterTexture(int64_t) override { return true; }
  };

  inline static std::vector<ErikaPresenterHandle*> hwnd_attachments;
  inline static bool fail_hwnd_attach = false;

  static void RunOverlayOwnership(
      const std::shared_ptr<ErikaFlutterPlugin::ErikaNativeLibrary>& library) {
    const auto attach = library->attach_windows_hwnd;
    const auto detach = library->detach_surface;
    library->attach_windows_hwnd = [](ErikaPresenterHandle* player,
        uint64_t, uint64_t, uint32_t, uint32_t, double) {
      hwnd_attachments.push_back(player);
      return fail_hwnd_attach ? ErikaStatus_PlayerError : ErikaStatus_Ok;
    };
    library->detach_surface = [](ErikaPresenterHandle*) { return ErikaStatus_Ok; };
    // Hidden Win32 parents are sufficient to test actual overlay ownership
    // logic. Only the native surface attachment is stubbed, not the owner map.
    struct HiddenWindow {
      HWND hwnd = CreateWindowExW(0, L"STATIC", L"Erika ownership test",
          WS_POPUP, 0, 0, 1, 1, nullptr, nullptr, GetModuleHandleW(nullptr), nullptr);
      ~HiddenWindow() { if (hwnd) DestroyWindow(hwnd); }
    };
    {
      HiddenWindow first_parent, second_parent;
      Require(first_parent.hwnd && second_parent.hwnd);
      auto overlay = std::make_unique<ErikaFlutterPlugin::ErikaOverlayWindow>(
          first_parent.hwnd);
      ErikaFlutterPlugin::PlayerMap players;
      for (int64_t id : {11, 22, 33}) {
        players.emplace(id, std::make_unique<ErikaFlutterPlugin::PlayerHost>(
            id, library, ErikaPresenterConfig{}));
      }
      auto& original = *players.begin()->second; // Deliberately not the last entry.
      for (const auto& entry : players) {
        entry.second->attached_view_id = kWindowOverlayViewId;
        entry.second->surface_attached = true;
        entry.second->attached_hwnd = overlay->hwnd;
      }
      overlay->owner_player_id = original.id;
      hwnd_attachments.clear();
      ErikaFlutterPlugin::RecreateOverlayWindow(players, overlay, second_parent.hwnd);
      Require(overlay->owner_player_id == original.id);
      Require(hwnd_attachments.size() == 1 && hwnd_attachments[0] == original.handle);
      Require(original.attached_hwnd == overlay->hwnd);
      ErikaFlutterPlugin::PlayerHost* replacement = nullptr;
      for (const auto& entry : players) {
        if (entry.second.get() != &original) {
          Require(!entry.second->surface_attached && entry.second->attached_view_id == 0);
          replacement = entry.second.get();
        }
      }
      Require(replacement != nullptr);
      ErikaFlutterPlugin::AttachOverlayPlayer(players, *replacement, *overlay);
      Require(!original.surface_attached && replacement->surface_attached);
      Require(overlay->owner_player_id == replacement->id);
      overlay->visible = true; // Test state without showing any window.
      overlay->active_generation = 77;
      Require(ErikaFlutterPlugin::DetachOverlayPlayer(original, overlay.get(), std::nullopt));
      Require(overlay->visible && overlay->owner_player_id == replacement->id);
      Require(!ErikaFlutterPlugin::DetachOverlayPlayer(*replacement, overlay.get(), 76));
      Require(replacement->surface_attached && overlay->visible);
      Require(ErikaFlutterPlugin::DetachOverlayPlayer(*replacement, overlay.get(), 77));
      Require(!replacement->surface_attached && !overlay->visible && overlay->owner_player_id == 0);

      // Hidden/unowned overlays must not acquire an arbitrary stale producer.
      original.attached_view_id = kWindowOverlayViewId;
      original.surface_attached = true;
      hwnd_attachments.clear();
      ErikaFlutterPlugin::RecreateOverlayWindow(players, overlay, first_parent.hwnd);
      Require(hwnd_attachments.empty() && overlay->owner_player_id == 0);
      Require(!original.surface_attached);
      overlay->owner_player_id = original.id; // Stale ID without an attachment.
      ErikaFlutterPlugin::RecreateOverlayWindow(players, overlay, second_parent.hwnd);
      Require(hwnd_attachments.empty() && overlay->owner_player_id == 0);

      // A failed takeover must not retain the previous player's ownership.
      ErikaFlutterPlugin::AttachOverlayPlayer(players, original, *overlay);
      fail_hwnd_attach = true;
      bool rejected = false;
      try {
        ErikaFlutterPlugin::AttachOverlayPlayer(players, *replacement, *overlay);
      } catch (const std::exception&) { rejected = true; }
      fail_hwnd_attach = false;
      Require(rejected && overlay->owner_player_id == 0);
      Require(!original.surface_attached && !replacement->surface_attached);
    }
    library->attach_windows_hwnd = attach;
    library->detach_surface = detach;
    std::cout << "overlay owner preservation, exclusive takeover and stale detach protection passed\n";
  }

  static void Run() {
    auto library = ErikaFlutterPlugin::ErikaNativeLibrary::Shared();
    RunOverlayOwnership(library);
    ErikaPresenterConfig config{};
    Registrar registrar;
    ErikaFlutterPlugin::ErikaFlutterTexture texture(&registrar, 4, 4, 1.0);
    if (library->attach_flutter_texture && library->windows_flutter_texture) {
      // When run against a source-built DLL, exercise the actual bridge all
      // the way to D3D11 too; no replacement function pointers in this block.
      {
        ErikaFlutterPlugin::PlayerHost player(4, library, config);
        player.AttachFlutterTexture(texture);
        player.RenderTick(nullptr);
        Require(registrar.marked_frames == 1);
        const auto* first = texture.publication.AcquireDescriptor();
        Require(first && first->width == 4 && first->height == 4);
        for (int i = 0; i < 10; ++i) player.RenderTick(nullptr);
        Require(registrar.marked_frames == 1);
        player.ResizeFlutterTexture(texture, 8, 6, 1.0);
        player.RenderTick(nullptr);
        Require(registrar.marked_frames == 2);
        Require(first->width == 4 && first->height == 4);
        const auto* resized = texture.publication.AcquireDescriptor();
        Require(resized && resized->width == 8 && resized->height == 6);
      }
      Require(texture.owner_player_id == 0);
      Require(texture.publication.AcquireDescriptor() == nullptr);
      std::cout << "source runtime texture attach, idle, resize and disposal passed\n";
    }
    // Load the bundled runtime (including v0.1.7 without texture symbols).
    // Also exercise absence when a future bundled runtime provides them.
    library->attach_flutter_texture = nullptr;
    library->windows_flutter_texture = nullptr;
    {
      ErikaFlutterPlugin::PlayerHost player(1, library, config);
      Require(player.handle != nullptr);
      player.surface_attached = true;
      bool rejected = false;
      try {
        player.AttachFlutterTexture(texture);
      } catch (const std::exception&) { rejected = true; }
      Require(rejected && player.surface_attached); // Failed opt-in is atomic.
      player.surface_attached = false;
    }

    // Supply only the new attachment capability; player creation/destruction
    // still use the real released DLL. This isolates the plugin ownership bug.
    library->attach_flutter_texture = [](ErikaPresenterHandle*, ErikaFlutterTextureKind,
        int64_t, uint32_t, uint32_t, double) { return ErikaStatus_Ok; };
    library->windows_flutter_texture = [](ErikaPresenterHandle*, void**) {
      return ErikaStatus_PlayerError;
    };
    {
      ErikaFlutterPlugin::PlayerHost player(2, library, config);
      player.AttachFlutterTexture(texture);
      Require(texture.owner_player_id == 2);
    }
    Require(texture.owner_player_id == 0);
    {
      ErikaFlutterPlugin::PlayerHost replacement(3, library, config);
      replacement.AttachFlutterTexture(texture);
      Require(texture.owner_player_id == 3);
    }
    Require(texture.owner_player_id == 0);
  }
};
}  // namespace erika_flutter

int wmain(int argc, wchar_t** argv) {
  if (argc != 2 && argc != 3) return 1;
  const bool keep_loaded = argc == 3 && wcscmp(argv[2], L"--keep-runtime-loaded") == 0;
  if (argc == 3 && !keep_loaded) return 1;
  if (!SetEnvironmentVariableW(L"ERIKA_CAPI_DLL", argv[1])) return 1;
  // Compatibility-only fixture for released binaries with the pre-existing
  // unjoined danmaku worker. Source-runtime tests MUST exercise real unloading.
  if (keep_loaded && !LoadLibraryW(argv[1])) return 1;
  erika_flutter::ErikaFlutterPluginTestPeer::Run();
  if (!keep_loaded) {
    erika_flutter::ErikaFlutterPluginTestPeer::Require(
        GetModuleHandleW(argv[1]) == nullptr);
    std::cout << "source runtime DLL unload passed\n";
  }
  std::cout << "runtime create/dispose, optional capability guard and texture owner cleanup passed\n";
}
