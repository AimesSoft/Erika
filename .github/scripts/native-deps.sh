#!/usr/bin/env bash
# Builds what the playback kernel links for one Rust target, in the checkout of
# liberika Cargo resolved. A cache may already hold the result.
set -euo pipefail

target="$1"
root="$(python3 web/dependencies.py)"
echo "liberika: $root"

case "$target" in
  *-unknown-linux-gnu)
    # Linux takes FFmpeg from the system; only libass is liberika's own.
    if [ ! -d "$root/third_party/dist/$target/system/libass" ]; then
      bash "$root/scripts/build_linux_libass.sh"
    fi
    # The generated hosts are workspaces of their own and do not see the
    # manifest's patches, but they do read this file.
    mkdir -p .cargo
    printf '[patch.crates-io]\nwgpu-hal = { path = "%s/third_party/wgpu-hal" }\n' "$root" > .cargo/config.toml
    ;;
  *-pc-windows-msvc)
    # Bern's Windows apps link the C runtime statically.
    if [ ! -d "$root/third_party/dist/$target/lgpl-crt-static" ]; then
      cargo run --manifest-path "$root/Cargo.toml" -p xtask -- \
        deps build --all --profile lgpl --target "$target" --crt static
    fi
    ;;
  *)
    if [ ! -d "$root/third_party/dist/$target/lgpl" ]; then
      cargo run --manifest-path "$root/Cargo.toml" -p xtask -- \
        deps build --all --profile lgpl --target "$target"
    fi
    ;;
esac
