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

  static void Run() {
    auto library = ErikaFlutterPlugin::ErikaNativeLibrary::Shared();
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
