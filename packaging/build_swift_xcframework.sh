#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: build_swift_xcframework.sh <release-download-dir> <output-zip>" >&2
  exit 2
fi

SOURCE_DIR="$(cd "$1" && pwd)"
OUTPUT_DIR="$(cd "$(dirname "$2")" && pwd)"
OUTPUT_ZIP="$OUTPUT_DIR/$(basename "$2")"
WORK_DIR="$(mktemp -d "${RUNNER_TEMP:-/tmp}/erika-swift.XXXXXX")"

for archive in \
  erika-capi-macos-universal.zip \
  erika-capi-ios.zip \
  erika-capi-tvos.zip
do
  test -s "$SOURCE_DIR/$archive"
  platform="${archive#erika-capi-}"
  platform="${platform%.zip}"
  mkdir -p "$WORK_DIR/$platform"
  unzip -q "$SOURCE_DIR/$archive" -d "$WORK_DIR/$platform"
done

MACOS_LIBRARY="$(find "$WORK_DIR/macos-universal" -type f -name 'liberika_capi.a' -print -quit)"
IOS_FRAMEWORK="$(find "$WORK_DIR/ios" -type d -name 'erika_capi.xcframework' -print -quit)"
TVOS_FRAMEWORK="$(find "$WORK_DIR/tvos" -type d -name 'erika_capi.xcframework' -print -quit)"
IOS_DEVICE="$(find "$IOS_FRAMEWORK" -type f -name '*.a' -path '*ios-arm64/*' -print -quit)"
IOS_SIMULATOR="$(find "$IOS_FRAMEWORK" -type f -name '*.a' -path '*ios-*simulator/*' -print -quit)"
TVOS_DEVICE="$(find "$TVOS_FRAMEWORK" -type f -name '*.a' -path '*tvos-arm64/*' -print -quit)"
TVOS_SIMULATOR="$(find "$TVOS_FRAMEWORK" -type f -name '*.a' -path '*tvos-*simulator/*' -print -quit)"
HEADER="$(find "$WORK_DIR/macos-universal" -type f -path '*/include/erika.h' -print -quit)"

for path in \
  "$MACOS_LIBRARY" \
  "$IOS_DEVICE" \
  "$IOS_SIMULATOR" \
  "$TVOS_DEVICE" \
  "$TVOS_SIMULATOR" \
  "$HEADER"
do
  test -n "$path"
  test -s "$path"
done

HEADERS="$WORK_DIR/headers"
mkdir -p "$HEADERS"
cp "$HEADER" "$HEADERS/erika.h"
cat > "$HEADERS/module.modulemap" <<'MODULEMAP'
module CErika [system] {
  header "erika.h"
  export *
}
MODULEMAP

XCFRAMEWORK="$WORK_DIR/CErika.xcframework"
xcodebuild -create-xcframework \
  -library "$MACOS_LIBRARY" -headers "$HEADERS" \
  -library "$IOS_DEVICE" -headers "$HEADERS" \
  -library "$IOS_SIMULATOR" -headers "$HEADERS" \
  -library "$TVOS_DEVICE" -headers "$HEADERS" \
  -library "$TVOS_SIMULATOR" -headers "$HEADERS" \
  -output "$XCFRAMEWORK"

test ! -e "$OUTPUT_ZIP"
ditto -c -k --sequesterRsrc --keepParent "$XCFRAMEWORK" "$OUTPUT_ZIP"
unzip -tq "$OUTPUT_ZIP"

echo "SwiftPM checksum: $(swift package compute-checksum "$OUTPUT_ZIP")"
ls -lh "$OUTPUT_ZIP"
