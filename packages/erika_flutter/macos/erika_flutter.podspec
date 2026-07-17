Pod::Spec.new do |s|
  s.name             = 'erika_flutter'
  s.version          = '0.1.3'
  s.summary          = 'Flutter embedder glue for the Erika Rust media engine.'
  s.description      = <<-DESC
Flutter macOS plugin that hosts a CAMetalLayer and drives Erika through its C ABI.
                       DESC
  s.homepage         = 'https://github.com/AimesSoft/Erika'
  s.license          = { :type => 'MPL-2.0' }
  s.author           = { 'AimesSoft' => 'dev@aimesoft.com' }
  s.source           = { :path => '.' }
  s.source_files     = 'Classes/**/*'
  s.dependency 'FlutterMacOS'
  s.platform = :osx, '10.14'
  s.swift_version = '5.0'
  s.script_phase = {
    :name => 'Build Erika C ABI',
    :execution_position => :before_compile,
    :script => <<-SCRIPT
set -eu

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

PLUGIN_MACOS_DIR="$(cd "$PODS_TARGET_SRCROOT" && pwd -P)"
if [ -n "${ERIKA_ROOT:-}" ] && [ -f "$ERIKA_ROOT/Cargo.toml" ]; then
  ERIKA_ROOT="$(cd "$ERIKA_ROOT" && pwd -P)"
else
  ERIKA_ROOT="$(cd "$PLUGIN_MACOS_DIR/../../.." && pwd -P)"
fi
if [ ! -f "$ERIKA_ROOT/Cargo.toml" ]; then
  echo "error: cannot locate Erika source root (Cargo.toml) from $PLUGIN_MACOS_DIR" >&2
  exit 1
fi

ERIKA_NATIVE_PROFILE="${ERIKA_NATIVE_PROFILE:-lgpl}"
HOST_JOBS="$(sysctl -n hw.ncpu 2>/dev/null || echo 4)"

if [ -n "${ERIKA_MACOS_CAPI_PROFILE:-}" ]; then
  CARGO_PROFILE="$ERIKA_MACOS_CAPI_PROFILE"
elif [ "${CONFIGURATION:-Debug}" = "Release" ]; then
  CARGO_PROFILE="release"
else
  CARGO_PROFILE="debug"
fi
if [ "$CARGO_PROFILE" = "release" ]; then
  CARGO_ARGS="--release"
else
  CARGO_ARGS=""
fi

DEST_DIR="$BUILT_PRODUCTS_DIR/$FRAMEWORKS_FOLDER_PATH"
DEST_DYLIB="$DEST_DIR/liberika_capi.dylib"
mkdir -p "$DEST_DIR"

# Optional: use a prebuilt universal dylib from a GitHub Release (opt-in).
# Enable with ERIKA_PREBUILT=1; ERIKA_PREBUILT_TAG selects the tag (default
# v0.1.3). Any failure falls through to the source build. ERIKA_MACOS_CAPI_DYLIB
# takes precedence (explicit dylib path).
UNIVERSAL_DYLIB=""
if [ "${ERIKA_FORCE_SOURCE_BUILD:-0}" != "1" ] && [ "${ERIKA_PREBUILT:-0}" = "1" ] && [ -z "${ERIKA_MACOS_CAPI_DYLIB:-}" ]; then
  PREBUILT_TAG="${ERIKA_PREBUILT_TAG:-v0.1.3}"
  PREBUILT_WORK="$ERIKA_ROOT/target/erika-prebuilt-macos"
  PREBUILT_ZIP="$PREBUILT_WORK/erika-capi-macos-universal.zip"
  PREBUILT_URL="https://github.com/AimesSoft/Erika/releases/download/$PREBUILT_TAG/erika-capi-macos-universal.zip"
  rm -rf "$PREBUILT_WORK"
  mkdir -p "$PREBUILT_WORK"
  echo "Erika: downloading prebuilt $PREBUILT_URL"
  if curl -fSL --retry 3 -o "$PREBUILT_ZIP" "$PREBUILT_URL" && unzip -oq "$PREBUILT_ZIP" -d "$PREBUILT_WORK"; then
    CAND="$(find "$PREBUILT_WORK" -type f -name 'liberika_capi.dylib' | head -1)"
    if [ -n "$CAND" ]; then
      UNIVERSAL_DYLIB="$CAND"
      echo "Erika: using prebuilt $PREBUILT_TAG -> $CAND"
    fi
  fi
  [ -n "$UNIVERSAL_DYLIB" ] || echo "Erika: prebuilt unavailable; building from source"
fi

if [ -n "${ERIKA_MACOS_CAPI_DYLIB:-}" ]; then
  UNIVERSAL_DYLIB="$ERIKA_MACOS_CAPI_DYLIB"
elif [ -z "$UNIVERSAL_DYLIB" ]; then
  RUST_TARGETS="aarch64-apple-darwin x86_64-apple-darwin"
  if command -v rustup >/dev/null 2>&1; then
    rustup target add $RUST_TARGETS
  fi
  LIPO_INPUTS=""
  for RUST_TARGET in $RUST_TARGETS; do
    DIST="$ERIKA_ROOT/third_party/dist/$RUST_TARGET/$ERIKA_NATIVE_PROFILE"
    if [ ! -f "$DIST/ffmpeg/include/libavformat/avformat.h" ] || [ ! -f "$DIST/libass/lib/libass.a" ]; then
      echo "Building Erika native dependencies for $RUST_TARGET ($ERIKA_NATIVE_PROFILE, with libass)"
      (cd "$ERIKA_ROOT" && cargo run -p xtask -- deps build --all --profile "$ERIKA_NATIVE_PROFILE" --target "$RUST_TARGET" --jobs "$HOST_JOBS")
    fi
    echo "Building Erika C ABI dylib for $RUST_TARGET ($CARGO_PROFILE)"
    (cd "$ERIKA_ROOT" && ERIKA_NATIVE_PROFILE="$ERIKA_NATIVE_PROFILE" ERIKA_NATIVE_TARGET="$RUST_TARGET" ERIKA_FFMPEG_DIR="$DIST/ffmpeg" ERIKA_LIBASS_DIR="$DIST/libass" ERIKA_FREETYPE_DIR="$DIST/freetype" ERIKA_HARFBUZZ_DIR="$DIST/harfbuzz" ERIKA_FRIBIDI_DIR="$DIST/fribidi" cargo build -p erika_capi --target "$RUST_TARGET" --no-default-features --features libass $CARGO_ARGS)
    ARCH_DYLIB="$ERIKA_ROOT/target/$RUST_TARGET/$CARGO_PROFILE/liberika_capi.dylib"
    if [ ! -f "$ARCH_DYLIB" ]; then
      echo "error: $ARCH_DYLIB was not produced by the Erika build" >&2
      exit 1
    fi
    LIPO_INPUTS="$LIPO_INPUTS $ARCH_DYLIB"
  done
  UNIVERSAL_DYLIB="$ERIKA_ROOT/target/erika-macos-universal/liberika_capi.dylib"
  mkdir -p "$(dirname "$UNIVERSAL_DYLIB")"
  lipo -create $LIPO_INPUTS -output "$UNIVERSAL_DYLIB"
fi

if [ ! -f "$UNIVERSAL_DYLIB" ]; then
  echo "error: Erika C ABI dylib not found: $UNIVERSAL_DYLIB" >&2
  exit 1
fi

cp "$UNIVERSAL_DYLIB" "$DEST_DYLIB"
install_name_tool -id "@rpath/liberika_capi.dylib" "$DEST_DYLIB"
codesign --force --sign "${EXPANDED_CODE_SIGN_IDENTITY:--}" "$DEST_DYLIB"
    SCRIPT
  }
  s.pod_target_xcconfig = {
    'OTHER_LDFLAGS' => '$(inherited) -framework QuartzCore -framework Metal'
  }
end
