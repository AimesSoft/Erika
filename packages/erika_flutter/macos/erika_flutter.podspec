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
  s.swift_version = '5.7'
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

MACOS_ARCHS="${ERIKA_MACOS_ARCHS:-universal}"
MACOS_ARCHS="$(printf '%s' "$MACOS_ARCHS" | tr ',' ' ')"
RUST_TARGETS=""
PREBUILT_ARCH=""
for MACOS_ARCH in $MACOS_ARCHS; do
  case "$MACOS_ARCH" in
    universal)
      RUST_TARGETS="aarch64-apple-darwin x86_64-apple-darwin"
      PREBUILT_ARCH="universal"
      break
      ;;
    arm64|aarch64|aarch64-apple-darwin)
      case " $RUST_TARGETS " in
        *" aarch64-apple-darwin "*) ;;
        *) RUST_TARGETS="$RUST_TARGETS aarch64-apple-darwin" ;;
      esac
      ;;
    x86_64|x64|x86_64-apple-darwin)
      case " $RUST_TARGETS " in
        *" x86_64-apple-darwin "*) ;;
        *) RUST_TARGETS="$RUST_TARGETS x86_64-apple-darwin" ;;
      esac
      ;;
    *)
      echo "error: unsupported ERIKA_MACOS_ARCHS value: $MACOS_ARCH" >&2
      exit 1
      ;;
  esac
done
RUST_TARGETS="$(printf '%s' "$RUST_TARGETS" | xargs)"
if [ -z "$RUST_TARGETS" ]; then
  echo "error: ERIKA_MACOS_ARCHS did not select an architecture" >&2
  exit 1
fi
if [ -z "$PREBUILT_ARCH" ]; then
  case "$RUST_TARGETS" in
    "aarch64-apple-darwin") PREBUILT_ARCH="arm64" ;;
    "x86_64-apple-darwin") PREBUILT_ARCH="x64" ;;
    *) PREBUILT_ARCH="universal" ;;
  esac
fi

OUTPUT_DYLIB=""
if [ "${ERIKA_FORCE_SOURCE_BUILD:-0}" != "1" ] && [ "${ERIKA_PREBUILT:-0}" = "1" ] && [ -z "${ERIKA_MACOS_CAPI_DYLIB:-}" ]; then
  PREBUILT_TAG="${ERIKA_PREBUILT_TAG:-v0.1.3}"
  PREBUILT_WORK="$ERIKA_ROOT/target/erika-prebuilt-macos-$PREBUILT_ARCH"
  PREBUILT_ZIP="$PREBUILT_WORK/erika-capi-macos-$PREBUILT_ARCH.zip"
  PREBUILT_URL="https://github.com/AimesSoft/Erika/releases/download/$PREBUILT_TAG/erika-capi-macos-$PREBUILT_ARCH.zip"
  rm -rf "$PREBUILT_WORK"
  mkdir -p "$PREBUILT_WORK"
  echo "Erika: downloading prebuilt $PREBUILT_URL"
  if curl -fSL --retry 3 -o "$PREBUILT_ZIP" "$PREBUILT_URL" && unzip -oq "$PREBUILT_ZIP" -d "$PREBUILT_WORK"; then
    CAND="$(find "$PREBUILT_WORK" -type f -name 'liberika_capi.dylib' | head -1)"
    if [ -n "$CAND" ]; then
      OUTPUT_DYLIB="$CAND"
      echo "Erika: using prebuilt $PREBUILT_TAG -> $CAND"
    fi
  fi
  [ -n "$OUTPUT_DYLIB" ] || echo "Erika: prebuilt unavailable; building from source"
fi

if [ -n "${ERIKA_MACOS_CAPI_DYLIB:-}" ]; then
  OUTPUT_DYLIB="$ERIKA_MACOS_CAPI_DYLIB"
elif [ -z "$OUTPUT_DYLIB" ]; then
  if command -v rustup >/dev/null 2>&1; then
    rustup target add $RUST_TARGETS
  fi
  LIPO_INPUTS=""
  for RUST_TARGET in $RUST_TARGETS; do
    DIST="$ERIKA_ROOT/third_party/dist/$RUST_TARGET/$ERIKA_NATIVE_PROFILE"
    DAV1D_DIR="$DIST/dav1d"
    DAV1D_MARKER="$ERIKA_ROOT/third_party/build/$RUST_TARGET/$ERIKA_NATIVE_PROFILE/dav1d/dav1d-built.txt"
    if [ ! -f "$DIST/ffmpeg/include/libavformat/avformat.h" ] || [ ! -f "$DAV1D_DIR/include/dav1d/dav1d.h" ] || [ ! -f "$DAV1D_DIR/lib/libdav1d.a" ] || [ ! -f "$DAV1D_MARKER" ] || ! grep -qx 'dav1d=1.5.1' "$DAV1D_MARKER" || [ ! -f "$DIST/libass/lib/libass.a" ]; then
      echo "Building Erika native dependencies for $RUST_TARGET ($ERIKA_NATIVE_PROFILE, with libass)"
      (cd "$ERIKA_ROOT" && cargo run -p xtask -- deps build --all --profile "$ERIKA_NATIVE_PROFILE" --target "$RUST_TARGET" --jobs "$HOST_JOBS")
    fi
    echo "Building Erika C ABI dylib for $RUST_TARGET ($CARGO_PROFILE)"
    (cd "$ERIKA_ROOT" && ERIKA_NATIVE_PROFILE="$ERIKA_NATIVE_PROFILE" ERIKA_NATIVE_TARGET="$RUST_TARGET" ERIKA_FFMPEG_DIR="$DIST/ffmpeg" ERIKA_DAV1D_DIR="$DAV1D_DIR" ERIKA_LIBASS_DIR="$DIST/libass" ERIKA_FREETYPE_DIR="$DIST/freetype" ERIKA_HARFBUZZ_DIR="$DIST/harfbuzz" ERIKA_FRIBIDI_DIR="$DIST/fribidi" cargo build -p erika_capi --target "$RUST_TARGET" --no-default-features --features libass $CARGO_ARGS)
    ARCH_DYLIB="$ERIKA_ROOT/target/$RUST_TARGET/$CARGO_PROFILE/liberika_capi.dylib"
    if [ ! -f "$ARCH_DYLIB" ]; then
      echo "error: $ARCH_DYLIB was not produced by the Erika build" >&2
      exit 1
    fi
    LIPO_INPUTS="$LIPO_INPUTS $ARCH_DYLIB"
  done
  if [ "$(printf '%s\n' $RUST_TARGETS | wc -l | xargs)" = "1" ]; then
    OUTPUT_DYLIB="$ARCH_DYLIB"
  else
    OUTPUT_DYLIB="$ERIKA_ROOT/target/erika-macos-universal/liberika_capi.dylib"
    mkdir -p "$(dirname "$OUTPUT_DYLIB")"
    lipo -create $LIPO_INPUTS -output "$OUTPUT_DYLIB"
  fi
fi

if [ ! -f "$OUTPUT_DYLIB" ]; then
  echo "error: Erika C ABI dylib not found: $OUTPUT_DYLIB" >&2
  exit 1
fi

cp "$OUTPUT_DYLIB" "$DEST_DYLIB"
install_name_tool -id "@rpath/liberika_capi.dylib" "$DEST_DYLIB"
codesign --force --sign "${EXPANDED_CODE_SIGN_IDENTITY:--}" "$DEST_DYLIB"
    SCRIPT
  }
  s.pod_target_xcconfig = {
    'OTHER_LDFLAGS' => '$(inherited) -framework QuartzCore -framework Metal -framework MediaPlayer'
  }
end
