cmake_minimum_required(VERSION 3.14)

if(NOT DEFINED ERIKA_REPO_ROOT OR ERIKA_REPO_ROOT STREQUAL "")
  message(FATAL_ERROR "ERIKA_REPO_ROOT is required")
endif()
if(NOT DEFINED CARGO_EXECUTABLE OR CARGO_EXECUTABLE STREQUAL "")
  set(CARGO_EXECUTABLE cargo)
endif()
if(NOT DEFINED ERIKA_NATIVE_TARGET OR ERIKA_NATIVE_TARGET STREQUAL "")
  set(ERIKA_NATIVE_TARGET "x86_64-pc-windows-msvc")
endif()
if(NOT DEFINED ERIKA_NATIVE_PROFILE OR ERIKA_NATIVE_PROFILE STREQUAL "")
  set(ERIKA_NATIVE_PROFILE "lgpl")
endif()
if(NOT DEFINED ERIKA_BUILD_CONFIG OR ERIKA_BUILD_CONFIG STREQUAL "")
  set(ERIKA_BUILD_CONFIG "Release")
endif()

set(ERIKA_NATIVE_DIST_DIR
  "${ERIKA_REPO_ROOT}/third_party/dist/${ERIKA_NATIVE_TARGET}/${ERIKA_NATIVE_PROFILE}")
set(ERIKA_FFMPEG_DIR "${ERIKA_NATIVE_DIST_DIR}/ffmpeg")
set(ERIKA_LIBASS_DIR "${ERIKA_NATIVE_DIST_DIR}/libass")
set(ERIKA_FREETYPE_DIR "${ERIKA_NATIVE_DIST_DIR}/freetype")
set(ERIKA_HARFBUZZ_DIR "${ERIKA_NATIVE_DIST_DIR}/harfbuzz")
set(ERIKA_FRIBIDI_DIR "${ERIKA_NATIVE_DIST_DIR}/fribidi")

function(erika_native_deps_ready output)
  set(ready TRUE)
  set(ffmpeg_version_header "${ERIKA_FFMPEG_DIR}/include/libavutil/version.h")
  if(NOT EXISTS "${ffmpeg_version_header}")
    set(ready FALSE)
  else()
    file(STRINGS "${ffmpeg_version_header}" ffmpeg_version_major
      REGEX "^#define[ \t]+LIBAVUTIL_VERSION_MAJOR[ \t]+[0-9]+")
    if(ffmpeg_version_major MATCHES "([0-9]+)$")
      set(ffmpeg_version_major "${CMAKE_MATCH_1}")
      if(ffmpeg_version_major LESS 59)
        set(ready FALSE)
      endif()
    else()
      set(ready FALSE)
    endif()
  endif()
  foreach(dep_dir
      "${ERIKA_LIBASS_DIR}"
      "${ERIKA_FREETYPE_DIR}"
      "${ERIKA_HARFBUZZ_DIR}"
      "${ERIKA_FRIBIDI_DIR}")
    if(NOT EXISTS "${dep_dir}/lib")
      set(ready FALSE)
    endif()
  endforeach()
  set(${output} "${ready}" PARENT_SCOPE)
endfunction()

erika_native_deps_ready(ERIKA_NATIVE_DEPS_READY)
if(NOT ERIKA_NATIVE_DEPS_READY)
  message(STATUS
    "Erika native dependency bundle missing; building ${ERIKA_NATIVE_TARGET}/${ERIKA_NATIVE_PROFILE}")
  execute_process(
    COMMAND "${CARGO_EXECUTABLE}" run -p xtask -- deps build
      --profile "${ERIKA_NATIVE_PROFILE}"
      --target "${ERIKA_NATIVE_TARGET}"
      --all
    WORKING_DIRECTORY "${ERIKA_REPO_ROOT}"
    RESULT_VARIABLE ERIKA_DEPS_RESULT
  )
  if(NOT ERIKA_DEPS_RESULT EQUAL 0)
    message(FATAL_ERROR
      "Failed to build Erika native dependencies with xtask (exit ${ERIKA_DEPS_RESULT})")
  endif()

  erika_native_deps_ready(ERIKA_NATIVE_DEPS_READY)
  if(NOT ERIKA_NATIVE_DEPS_READY)
    message(FATAL_ERROR
      "Erika native dependencies did not appear under ${ERIKA_NATIVE_DIST_DIR} after xtask")
  endif()
else()
  message(STATUS "Using Erika native dependencies from ${ERIKA_NATIVE_DIST_DIR}")
endif()

set(ERIKA_CARGO_ARGS build -p erika_capi)
if(NOT ERIKA_BUILD_CONFIG STREQUAL "Debug")
  list(APPEND ERIKA_CARGO_ARGS --release)
endif()

execute_process(
  COMMAND "${CMAKE_COMMAND}" -E env
    "ERIKA_NATIVE_TARGET=${ERIKA_NATIVE_TARGET}"
    "ERIKA_NATIVE_PROFILE=${ERIKA_NATIVE_PROFILE}"
    "ERIKA_FFMPEG_DIR=${ERIKA_FFMPEG_DIR}"
    "ERIKA_LIBASS_DIR=${ERIKA_LIBASS_DIR}"
    "ERIKA_FREETYPE_DIR=${ERIKA_FREETYPE_DIR}"
    "ERIKA_HARFBUZZ_DIR=${ERIKA_HARFBUZZ_DIR}"
    "ERIKA_FRIBIDI_DIR=${ERIKA_FRIBIDI_DIR}"
    "${CARGO_EXECUTABLE}" ${ERIKA_CARGO_ARGS}
  WORKING_DIRECTORY "${ERIKA_REPO_ROOT}"
  RESULT_VARIABLE ERIKA_CAPI_RESULT
)
if(NOT ERIKA_CAPI_RESULT EQUAL 0)
  message(FATAL_ERROR
    "Failed to build Erika C API runtime for ${ERIKA_BUILD_CONFIG} (exit ${ERIKA_CAPI_RESULT})")
endif()
