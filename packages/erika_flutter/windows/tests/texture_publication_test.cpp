#include "../erika_texture_publication.h"

#include <atomic>
#include <cstdlib>
#include <iostream>
#include <thread>
#include <vector>

using Microsoft::WRL::ComPtr;
using erika_flutter::ErikaTexturePublication;

void Require(bool condition) {
  if (!condition) {
    std::cerr << "texture publication assertion failed\n";
    std::abort();
  }
}

ComPtr<ID3D11Texture2D> Texture(ID3D11Device* device, UINT width) {
  D3D11_TEXTURE2D_DESC desc{};
  desc.Width = width;
  desc.Height = 4;
  desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
  desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
  desc.Usage = D3D11_USAGE_DEFAULT;
  desc.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
  desc.MiscFlags = D3D11_RESOURCE_MISC_SHARED;
  ComPtr<ID3D11Texture2D> texture;
  Require(SUCCEEDED(device->CreateTexture2D(&desc, nullptr, &texture)));
  return texture;
}

bool Publish(ErikaTexturePublication& publication, ID3D11Texture2D* texture) {
  texture->AddRef(); // Same owned-reference contract as the Rust C API.
  bool changed = false;
  Require(SUCCEEDED(publication.Update(texture, changed)));
  return changed;
}

ULONG RefCount(ID3D11Texture2D* texture) {
  texture->AddRef();
  return texture->Release();
}

int main() {
  ComPtr<ID3D11Device> device;
  Require(SUCCEEDED(D3D11CreateDevice(
      nullptr, D3D_DRIVER_TYPE_WARP, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
      nullptr, 0, D3D11_SDK_VERSION, &device, nullptr, nullptr)));
  ErikaTexturePublication publication;
  auto first = Texture(device.Get(), 4);
  const auto baseline = RefCount(first.Get());
  Require(Publish(publication, first.Get()));
  Require(!Publish(publication, first.Get())); // Paused: no new notification.
  const auto* descriptor = publication.AcquireDescriptor();
  Require(descriptor && descriptor->width == 4);
  const auto first_handle = descriptor->handle;

  // Flutter may still be reading the first descriptor while arbitrarily many
  // intermediate resize publications are replaced without ever being opened.
  std::vector<ComPtr<ID3D11Texture2D>> skipped;
  for (UINT i = 0; i < 100; ++i) {
    auto texture = Texture(device.Get(), 8 + i);
    Require(Publish(publication, texture.Get()));
    skipped.push_back(std::move(texture));
  }
  Require(descriptor->width == 4 && descriptor->handle == first_handle);
  for (size_t i = 0; i + 1 < skipped.size(); ++i) {
    Require(RefCount(skipped[i].Get()) == baseline);
  }
  Require(RefCount(first.Get()) > baseline);
  Require(publication.AcquireDescriptor()->width == 107);
  Require(RefCount(first.Get()) == baseline);
  publication.Clear();
  Require(publication.AcquireDescriptor() == nullptr);
  Require(RefCount(skipped.back().Get()) == baseline);

  auto a = Texture(device.Get(), 8);
  auto b = Texture(device.Get(), 16);
  std::atomic<bool> done{false};
  std::thread raster([&] {
    while (!done.load()) {
      const auto* frame = publication.AcquireDescriptor();
      if (frame) {
        const auto width = frame->width;
        std::this_thread::yield();
        Require((width == 8 || width == 16) && frame->width == width);
      }
    }
  });
  for (int i = 0; i < 10000; ++i) {
    Publish(publication, (i % 2 == 0 ? a : b).Get());
    if (i % 5 == 0) publication.Clear();
  }
  done.store(true);
  raster.join();
  publication.Clear();
  publication.AcquireDescriptor();
  Require(RefCount(a.Get()) == baseline && RefCount(b.Get()) == baseline);
  std::cout << "publication lifetime, skipped resize reclamation, idle and concurrent access passed\n";
}
