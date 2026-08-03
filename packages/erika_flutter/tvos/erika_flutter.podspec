Pod::Spec.new do |s|
  erika_cabi_symbols = %w[
    erika_danmaku_track_info_free
    erika_presenter_add_danmaku_track_file
    erika_presenter_add_danmaku_track_json
    erika_presenter_add_external_subtitle
    erika_presenter_attach_metal_layer
    erika_presenter_clear_danmaku
    erika_presenter_close
    erika_presenter_create
    erika_presenter_create_with_output_mode
    erika_presenter_danmaku_tracks
    erika_presenter_destroy
    erika_presenter_detach_surface
    erika_presenter_get_danmaku_config
    erika_presenter_get_upscaler_status
    erika_presenter_load_danmaku_file
    erika_presenter_load_danmaku_json
    erika_presenter_open
    erika_presenter_open_with_headers
    erika_presenter_pause
    erika_presenter_play
    erika_presenter_poll_event
    erika_presenter_remove_danmaku_track
    erika_presenter_remove_subtitle_track
    erika_presenter_render_tick
    erika_presenter_resize_surface
    erika_presenter_seek
    erika_presenter_select_audio_track
    erika_presenter_select_subtitle_track
    erika_presenter_set_danmaku_block_words_json
    erika_presenter_set_danmaku_config_ptr
    erika_presenter_set_danmaku_enabled
    erika_presenter_set_danmaku_font
    erika_presenter_set_danmaku_global_offset
    erika_presenter_set_danmaku_track_enabled
    erika_presenter_set_danmaku_track_offset
    erika_presenter_set_playback_rate
    erika_presenter_set_subtitle_scale
    erika_presenter_set_upscaler
    erika_presenter_set_volume
    erika_presenter_stop
    erika_presenter_track_selection
    erika_presenter_tracks
    erika_track_info_free
  ]
  erika_cabi_undefined_flags = erika_cabi_symbols
    .map { |symbol| "-Wl,-u,_#{symbol}" }
    .join(' ')

  s.name             = 'erika_flutter'
  s.version          = '0.1.4'
  s.summary          = 'Flutter embedder glue for the Erika Rust media engine.'
  s.description      = <<-DESC
Flutter tvOS plugin that hosts a CAMetalLayer and drives Erika through its C ABI.
                       DESC
  s.homepage         = 'https://github.com/AimesSoft/Erika'
  s.license          = { :type => 'MPL-2.0' }
  s.author           = { 'AimesSoft' => 'dev@aimesoft.com' }
  s.source           = { :path => '.' }
  s.source_files     = 'Classes/**/*'
  # Flutter.framework is supplied by the flutter-tvos host app.
  s.platform = :tvos, '13.0'
  s.swift_version = '5.0'
  s.xcconfig = {
    'FRAMEWORK_SEARCH_PATHS' => '"${PODS_ROOT}/../Flutter"',
    'OTHER_SWIFT_FLAGS' => '$(inherited) -DTARGET_OS_TV',
  }
  s.script_phase = {
    :name => 'Build Erika C ABI',
    :execution_position => :before_compile,
    :input_files => ['${BUILT_PRODUCTS_DIR}/erika_capi_phony'],
    :output_files => ['${PODS_TARGET_SRCROOT}/native/liberika_capi.a'],
    :script => <<-SCRIPT
set -eu

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

PLUGIN_TVOS_DIR="$(cd "$PODS_TARGET_SRCROOT" && pwd -P)"
ERIKA_ROOT="$(cd "$PLUGIN_TVOS_DIR/../../.." && pwd -P)"
ERIKA_NATIVE_PROFILE="${ERIKA_NATIVE_PROFILE:-lgpl}"
HOST_JOBS="$(sysctl -n hw.ncpu 2>/dev/null || echo 4)"
ARCH="${CURRENT_ARCH:-}"
if [ -z "$ARCH" ] || [ "$ARCH" = "undefined_arch" ]; then
  ARCH="${ARCHS%% *}"
fi

case "${PLATFORM_NAME:-appletvos}" in
  appletvos)
    RUST_TARGET="aarch64-apple-tvos"
    BINDGEN_CLANG_TARGET="arm64-apple-tvos"
    BINDGEN_SDK="appletvos"
    ;;
  appletvsimulator)
    if [ "$ARCH" = "x86_64" ]; then
      RUST_TARGET="x86_64-apple-tvos"
      BINDGEN_CLANG_TARGET="x86_64-apple-tvos-simulator"
    else
      RUST_TARGET="aarch64-apple-tvos-sim"
      BINDGEN_CLANG_TARGET="arm64-apple-tvos-simulator"
    fi
    BINDGEN_SDK="appletvsimulator"
    ;;
  *)
    echo "error: unsupported Erika tvOS platform: ${PLATFORM_NAME:-unknown}" >&2
    exit 1
    ;;
esac

if [ -n "${ERIKA_TVOS_CAPI_PROFILE:-}" ]; then
  CARGO_PROFILE="$ERIKA_TVOS_CAPI_PROFILE"
elif [ "${CONFIGURATION:-Debug}" = "Release" ]; then
  CARGO_PROFILE="release"
else
  CARGO_PROFILE="debug"
fi

if [ "$CARGO_PROFILE" = "release" ]; then
  CARGO_ARGS="--release"
elif [ "$CARGO_PROFILE" = "debug" ]; then
  CARGO_ARGS=""
else
  echo "error: unsupported ERIKA_TVOS_CAPI_PROFILE=$CARGO_PROFILE" >&2
  exit 1
fi

if ! command -v rustup >/dev/null 2>&1; then
  echo "error: rustup is required to build Erika for tvOS" >&2
  exit 1
fi
if ! rustup run nightly rustc --version >/dev/null 2>&1; then
  rustup toolchain install nightly --profile minimal --component rust-src
elif ! rustup component list --toolchain nightly --installed | grep -q '^rust-src'; then
  rustup component add rust-src --toolchain nightly
fi
export TVOS_DEPLOYMENT_TARGET="${TVOS_DEPLOYMENT_TARGET:-13.0}"

BINDGEN_SDKROOT="$(xcrun --sdk "$BINDGEN_SDK" --show-sdk-path)"
BINDGEN_TARGET_ENV="$(echo "$RUST_TARGET" | tr '-' '_')"
export "BINDGEN_EXTRA_CLANG_ARGS_$BINDGEN_TARGET_ENV=--target=$BINDGEN_CLANG_TARGET -isysroot $BINDGEN_SDKROOT"

if [ -z "${ERIKA_FFMPEG_DIR:-}" ]; then
  ERIKA_FFMPEG_DIR="$ERIKA_ROOT/third_party/dist/$RUST_TARGET/$ERIKA_NATIVE_PROFILE/ffmpeg"
fi
ERIKA_TARGET_DIST="$ERIKA_ROOT/third_party/dist/$RUST_TARGET/$ERIKA_NATIVE_PROFILE"
ERIKA_DAV1D_DIR="${ERIKA_DAV1D_DIR:-$ERIKA_TARGET_DIST/dav1d}"
ERIKA_LIBASS_DIR="${ERIKA_LIBASS_DIR:-$ERIKA_TARGET_DIST/libass}"
ERIKA_FREETYPE_DIR="${ERIKA_FREETYPE_DIR:-$ERIKA_TARGET_DIST/freetype}"
ERIKA_HARFBUZZ_DIR="${ERIKA_HARFBUZZ_DIR:-$ERIKA_TARGET_DIST/harfbuzz}"
ERIKA_FRIBIDI_DIR="${ERIKA_FRIBIDI_DIR:-$ERIKA_TARGET_DIST/fribidi}"
ERIKA_DAV1D_MARKER="$ERIKA_ROOT/third_party/build/$RUST_TARGET/$ERIKA_NATIVE_PROFILE/dav1d/dav1d-built.txt"

# Optional: use a prebuilt static lib from a GitHub Release (opt-in).
# Enable with ERIKA_PREBUILT=1; ERIKA_PREBUILT_TAG selects the tag (default
# v0.1.4). Any failure falls through to the source build below, so enabling it
# never breaks a build. ERIKA_TVOS_CAPI_STATICLIB still takes precedence.
PREBUILT_LIB=""
if [ "${ERIKA_FORCE_SOURCE_BUILD:-0}" != "1" ] && [ "${ERIKA_PREBUILT:-0}" = "1" ] && [ -z "${ERIKA_TVOS_CAPI_STATICLIB:-}" ]; then
  PREBUILT_TAG="${ERIKA_PREBUILT_TAG:-v0.1.4}"
  PREBUILT_WORK="$ERIKA_ROOT/target/erika-prebuilt-tvos"
  PREBUILT_ZIP="$PREBUILT_WORK/erika-capi-tvos.zip"
  PREBUILT_URL="https://github.com/AimesSoft/Erika/releases/download/$PREBUILT_TAG/erika-capi-tvos.zip"
  rm -rf "$PREBUILT_WORK"
  mkdir -p "$PREBUILT_WORK"
  echo "Erika: downloading prebuilt $PREBUILT_URL"
  if curl -fSL --retry 3 -o "$PREBUILT_ZIP" "$PREBUILT_URL" && unzip -oq "$PREBUILT_ZIP" -d "$PREBUILT_WORK"; then
    XCF="$(find "$PREBUILT_WORK" -type d -name 'erika_capi.xcframework' | head -1)"
    if [ -n "$XCF" ]; then
      case "${PLATFORM_NAME:-appletvos}" in
        appletvsimulator) SLICE="$(find "$XCF" -maxdepth 1 -type d -name '*simulator*' | head -1)" ;;
        *) SLICE="$(find "$XCF" -maxdepth 1 -type d -name 'tvos-*' ! -name '*simulator*' | head -1)" ;;
      esac
      if [ -n "${SLICE:-}" ] && [ -f "$SLICE/liberika_capi.a" ]; then
        PREBUILT_LIB="$SLICE/liberika_capi.a"
        echo "Erika: using prebuilt $PREBUILT_TAG -> $PREBUILT_LIB"
      fi
    fi
  fi
  [ -n "$PREBUILT_LIB" ] || echo "Erika: prebuilt unavailable; building from source"
fi

if [ -n "${ERIKA_TVOS_CAPI_STATICLIB:-}" ]; then
  LIB_SOURCE="$ERIKA_TVOS_CAPI_STATICLIB"
elif [ -n "$PREBUILT_LIB" ]; then
  LIB_SOURCE="$PREBUILT_LIB"
else
  if [ ! -f "$ERIKA_FFMPEG_DIR/include/libavformat/avformat.h" ] || [ ! -f "$ERIKA_DAV1D_DIR/include/dav1d/dav1d.h" ] || [ ! -f "$ERIKA_DAV1D_DIR/lib/libdav1d.a" ] || [ ! -f "$ERIKA_DAV1D_MARKER" ] || ! grep -qx 'dav1d=1.5.1' "$ERIKA_DAV1D_MARKER" || [ ! -f "$ERIKA_LIBASS_DIR/lib/libass.a" ]; then
    echo "Building Erika native dependencies for $RUST_TARGET ($ERIKA_NATIVE_PROFILE, with libass)"
    (cd "$ERIKA_ROOT" && cargo run -p xtask -- deps build --all --profile "$ERIKA_NATIVE_PROFILE" --target "$RUST_TARGET" --jobs "$HOST_JOBS")
  fi
  LIB_SOURCE="$ERIKA_ROOT/target/$RUST_TARGET/$CARGO_PROFILE/liberika_capi.a"
  echo "Building Erika C ABI staticlib for $RUST_TARGET ($CARGO_PROFILE)"
  (cd "$ERIKA_ROOT" && ERIKA_NATIVE_PROFILE="$ERIKA_NATIVE_PROFILE" ERIKA_NATIVE_TARGET="$RUST_TARGET" ERIKA_FFMPEG_DIR="$ERIKA_FFMPEG_DIR" ERIKA_DAV1D_DIR="$ERIKA_DAV1D_DIR" ERIKA_LIBASS_DIR="$ERIKA_LIBASS_DIR" ERIKA_FREETYPE_DIR="$ERIKA_FREETYPE_DIR" ERIKA_HARFBUZZ_DIR="$ERIKA_HARFBUZZ_DIR" ERIKA_FRIBIDI_DIR="$ERIKA_FRIBIDI_DIR" cargo +nightly rustc -Z build-std=std,panic_abort -p erika_capi --target "$RUST_TARGET" --no-default-features --features libass $CARGO_ARGS --lib --crate-type staticlib)
fi

if [ ! -f "$LIB_SOURCE" ]; then
  echo "error: Erika C ABI static library not found: $LIB_SOURCE" >&2
  echo "       Build it with: cargo +nightly rustc -Z build-std=std,panic_abort -p erika_capi --target $RUST_TARGET $CARGO_ARGS --lib --crate-type staticlib" >&2
  exit 1
fi

mkdir -p "$PODS_TARGET_SRCROOT/native"
cp "$LIB_SOURCE" "$PODS_TARGET_SRCROOT/native/liberika_capi.a"
if [ -f "$OBJROOT/XCBuildData/build.db" ]; then
  ln -fs "$OBJROOT/XCBuildData/build.db" "$BUILT_PRODUCTS_DIR/erika_capi_phony"
fi
    SCRIPT
  }
  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'EXCLUDED_ARCHS[sdk=appletvsimulator*]' => 'i386',
    'OTHER_LDFLAGS' => "$(inherited) \"$(PODS_TARGET_SRCROOT)/native/liberika_capi.a\" #{erika_cabi_undefined_flags} -framework AVFoundation -framework AudioToolbox -framework QuartzCore -framework Metal -framework CoreVideo -framework CoreMedia -framework VideoToolbox -framework CoreText -framework CoreFoundation -framework CoreGraphics -framework Foundation -liconv -lbz2 -lz",
  }
end
