#pragma once
#include <flutter_linux/flutter_linux.h>
#include "erika.h"
#include <cstdint>
#include <memory>

// A Wayland subsurface below the Flutter surface. Video never enters Flutter's
// RGBA8 texture compositor. All calls must run on GTK's platform thread.
class NativeVideoSurface {
 public:
  NativeVideoSurface(FlView* view, ErikaPresenterHandle* presenter);
  ~NativeVideoSurface();
  NativeVideoSurface(const NativeVideoSurface&) = delete;
  NativeVideoSurface& operator=(const NativeVideoSurface&) = delete;
  void update(double x, double y, double width, double height, bool visible);
  bool visible() const;
 private:
  struct Impl;
  std::unique_ptr<Impl> impl_;
};
