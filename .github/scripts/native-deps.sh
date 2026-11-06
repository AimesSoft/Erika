#!/usr/bin/env bash
# Builds what the playback kernel links for the given Rust targets, in the
# checkout of liberika Cargo resolved. A cache may already hold the result.
set -euo pipefail

root="$(python3 web/dependencies.py)"
echo "liberika: $root"

# The generated hosts are workspaces of their own and do not see the manifest's
# patches, but they do read this file. liberika's wgpu renderer needs its patch.
patch_wgpu() {
  mkdir -p .cargo
  printf '[patch.crates-io]\nwgpu-hal = { path = "%s/third_party/wgpu-hal" }\n' "$root" > .cargo/config.toml
}

deps() {
  cargo run --manifest-path "$root/Cargo.toml" -p xtask -- \
    deps build --all --profile lgpl --target "$@"
}

for target in "$@"; do
  case "$target" in
    *-unknown-linux-gnu)
      # Linux takes FFmpeg from the system; only libass is liberika's own.
      [ -d "$root/third_party/dist/$target/system/libass" ] ||
        bash "$root/scripts/build_linux_libass.sh"
      patch_wgpu
      ;;
    *-pc-windows-msvc)
      # Bern's Windows apps link the C runtime statically.
      [ -d "$root/third_party/dist/$target/lgpl-crt-static" ] || deps "$target" --crt static
      ;;
    *-linux-android*)
      [ -d "$root/third_party/dist/$target/lgpl" ] || deps "$target"
      patch_wgpu
      ;;
    *)
      [ -d "$root/third_party/dist/$target/lgpl" ] || deps "$target"
      ;;
  esac
done
