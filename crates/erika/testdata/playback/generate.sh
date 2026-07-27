#!/usr/bin/env bash
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly FIXTURE_NAME="playback-fixture.mkv"
readonly FIXTURE_PATH="${SCRIPT_DIR}/${FIXTURE_NAME}"
readonly CHECKSUM_PATH="${SCRIPT_DIR}/SHA256SUMS"
readonly REQUIRED_VERSION="8.1.2"

usage() {
  cat <<'EOF'
Usage: ./generate.sh [--check | --update]

  --check   Rebuild twice, validate, and compare with the committed fixture.
            This is the default.
  --update  Rebuild twice, validate, replace the fixture, and refresh hashes.
EOF
}

fail() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

mode="check"
case "${1:-}" in
  "" | --check)
    ;;
  --update)
    mode="update"
    ;;
  -h | --help)
    usage
    exit 0
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac

for tool in ffmpeg ffprobe shasum cmp awk; do
  command -v "${tool}" >/dev/null 2>&1 || fail "required tool not found: ${tool}"
done

check_version() {
  local tool="$1"
  local first_line
  first_line="$("${tool}" -version | awk 'NR == 1 { print; exit }')"
  case "${first_line}" in
    "${tool} version ${REQUIRED_VERSION}" | "${tool} version ${REQUIRED_VERSION} "*)
      ;;
    *)
      fail "${tool} ${REQUIRED_VERSION} is required; found: ${first_line}"
      ;;
  esac
}

check_version ffmpeg
check_version ffprobe

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/erika-playback-fixture.XXXXXX")"
trap 'rm -rf "${tmp_dir}"' EXIT

generate_fixture() {
  local output="$1"

  ffmpeg \
    -hide_banner \
    -loglevel error \
    -nostdin \
    -filter_threads 1 \
    -filter_complex_threads 1 \
    -f lavfi \
    -i "testsrc2=size=160x90:rate=30:duration=8" \
    -f lavfi \
    -i "aevalsrc=0.25*sin(2*PI*880*t)*lt(mod(t\,1)\,0.1):sample_rate=48000:duration=8:channel_layout=mono" \
    -f lavfi \
    -i "aevalsrc=0.25*sin(2*PI*1320*t)*lt(mod(t\,1)\,0.1):sample_rate=48000:duration=8:channel_layout=mono" \
    -f srt \
    -i "${SCRIPT_DIR}/track-a.srt" \
    -f srt \
    -i "${SCRIPT_DIR}/track-b.srt" \
    -map 0:v:0 \
    -map 1:a:0 \
    -map 2:a:0 \
    -map 3:s:0 \
    -map 4:s:0 \
    -map_metadata -1 \
    -fflags +bitexact \
    -c:v mpeg4 \
    -pix_fmt yuv420p \
    -r:v 30 \
    -g:v 30 \
    -bf:v 0 \
    -sc_threshold:v 0 \
    -qscale:v 5 \
    -threads:v 1 \
    -flags:v +bitexact \
    -c:a flac \
    -sample_fmt:a s16 \
    -ar:a 48000 \
    -ac:a 1 \
    -compression_level:a 5 \
    -threads:a 1 \
    -flags:a +bitexact \
    -c:s copy \
    -metadata title="Erika deterministic playback fixture" \
    -metadata:s:v:0 title="deterministic-testsrc2" \
    -metadata:s:a:0 language=eng \
    -metadata:s:a:0 title="pulse-880-hz" \
    -metadata:s:a:1 language=jpn \
    -metadata:s:a:1 title="pulse-1320-hz" \
    -metadata:s:s:0 language=eng \
    -metadata:s:s:0 title="track-a" \
    -metadata:s:s:1 language=jpn \
    -metadata:s:s:1 title="track-b" \
    -disposition:v:0 default \
    -disposition:a:0 default \
    -disposition:a:1 0 \
    -disposition:s:0 default \
    -disposition:s:1 0 \
    -t 8 \
    -bitexact \
    -f matroska \
    -y "${output}"
}

probe_stream_field() {
  local file="$1"
  local selector="$2"
  local field="$3"

  ffprobe \
    -v error \
    -select_streams "${selector}" \
    -show_entries "stream=${field}" \
    -of default=noprint_wrappers=1:nokey=1 \
    "${file}"
}

expect_stream_field() {
  local file="$1"
  local selector="$2"
  local field="$3"
  local expected="$4"
  local actual

  actual="$(probe_stream_field "${file}" "${selector}" "${field}")"
  [[ "${actual}" == "${expected}" ]] ||
    fail "${selector} ${field}: expected '${expected}', got '${actual}'"
}

validate_fixture() {
  local file="$1"
  local stream_count
  local duration
  local frame_count
  local keyframes
  local expected_keyframes

  stream_count="$(
    ffprobe \
      -v error \
      -show_entries stream=index \
      -of csv=p=0 \
      "${file}" | awk 'END { print NR }'
  )"
  [[ "${stream_count}" == "5" ]] || fail "expected 5 streams, got ${stream_count}"

  # Absolute indices make the expected Matroska stream order explicit.
  expect_stream_field "${file}" v:0 index 0
  expect_stream_field "${file}" v:0 codec_name mpeg4
  expect_stream_field "${file}" v:0 width 160
  expect_stream_field "${file}" v:0 height 90
  expect_stream_field "${file}" v:0 r_frame_rate 30/1
  expect_stream_field "${file}" v:0 has_b_frames 0

  expect_stream_field "${file}" a:0 index 1
  expect_stream_field "${file}" a:0 codec_name flac
  expect_stream_field "${file}" a:0 sample_rate 48000
  expect_stream_field "${file}" a:0 channels 1

  expect_stream_field "${file}" a:1 index 2
  expect_stream_field "${file}" a:1 codec_name flac
  expect_stream_field "${file}" a:1 sample_rate 48000
  expect_stream_field "${file}" a:1 channels 1

  expect_stream_field "${file}" s:0 index 3
  expect_stream_field "${file}" s:0 codec_name subrip
  expect_stream_field "${file}" s:1 index 4
  expect_stream_field "${file}" s:1 codec_name subrip

  duration="$(
    ffprobe \
      -v error \
      -show_entries format=duration \
      -of default=noprint_wrappers=1:nokey=1 \
      "${file}"
  )"
  awk -v duration="${duration}" \
    'BEGIN { exit !(duration >= 7.999 && duration <= 8.001) }' ||
    fail "expected 8.000 seconds, got ${duration}"

  frame_count="$(
    ffprobe \
      -v error \
      -count_frames \
      -select_streams v:0 \
      -show_entries stream=nb_read_frames \
      -of default=noprint_wrappers=1:nokey=1 \
      "${file}"
  )"
  [[ "${frame_count}" == "240" ]] || fail "expected 240 video frames, got ${frame_count}"

  keyframes="$(
    ffprobe \
      -v error \
      -select_streams v:0 \
      -show_entries packet=pts_time,flags \
      -of csv=p=0 \
      "${file}" |
      awk -F, '$2 ~ /K/ { printf "%.6f\n", $1 }'
  )"
  expected_keyframes="$(cat <<'EOF'
0.000000
1.000000
2.000000
3.000000
4.000000
5.000000
6.000000
7.000000
EOF
  )"
  [[ "${keyframes}" == "${expected_keyframes}" ]] ||
    fail "unexpected keyframe timestamps:\n${keyframes}"
}

first_build="${tmp_dir}/first.mkv"
second_build="${tmp_dir}/second.mkv"

generate_fixture "${first_build}"
generate_fixture "${second_build}"
cmp -s "${first_build}" "${second_build}" ||
  fail "two clean builds are not byte-for-byte identical"
validate_fixture "${first_build}"

if [[ "${mode}" == "update" ]]; then
  cp "${first_build}" "${FIXTURE_PATH}"
  (
    cd "${SCRIPT_DIR}"
    shasum -a 256 "${FIXTURE_NAME}" track-a.srt track-b.srt >"${CHECKSUM_PATH}"
  )
else
  [[ -f "${FIXTURE_PATH}" ]] || fail "fixture is missing: ${FIXTURE_PATH}"
  [[ -f "${CHECKSUM_PATH}" ]] || fail "checksum file is missing: ${CHECKSUM_PATH}"
  cmp -s "${first_build}" "${FIXTURE_PATH}" ||
    fail "committed fixture differs from a clean build; run ./generate.sh --update intentionally"
fi

(
  cd "${SCRIPT_DIR}"
  shasum -a 256 -c SHA256SUMS
)

printf 'ok: %s is deterministic and structurally valid\n' "${FIXTURE_NAME}"
