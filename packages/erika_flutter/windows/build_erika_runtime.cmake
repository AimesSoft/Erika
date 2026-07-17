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

# --- Optional: use a prebuilt erika_capi.dll from a GitHub Release ---
# Opt in with ERIKA_PREBUILT=1. ERIKA_PREBUILT_TAG selects the release tag
# (default v0.1.3). On any failure this falls through to the source build below,
# so enabling it never breaks a build. The Windows plugin loads the DLL
# dynamically, so only erika_capi.dll is required.
if("$ENV{ERIKA_PREBUILT}" STREQUAL "1")
  if(ERIKA_BUILD_CONFIG STREQUAL "Debug")
    set(_erika_cfg_dir "debug")
  else()
    set(_erika_cfg_dir "release")
  endif()
  set(_erika_tag "v0.1.3")
  if(NOT "$ENV{ERIKA_PREBUILT_TAG}" STREQUAL "")
    set(_erika_tag "$ENV{ERIKA_PREBUILT_TAG}")
  endif()
  set(_erika_dll_out "${ERIKA_REPO_ROOT}/target/${_erika_cfg_dir}/erika_capi.dll")
  set(_erika_tag_marker "${_erika_dll_out}.prebuilt-tag")
  if(EXISTS "${_erika_dll_out}" AND EXISTS "${_erika_tag_marker}")
    file(READ "${_erika_tag_marker}" _erika_cached_tag)
    string(STRIP "${_erika_cached_tag}" _erika_cached_tag)
    if("${_erika_cached_tag}" STREQUAL "${_erika_tag}")
      message(STATUS "Erika: reusing prebuilt ${_erika_tag} -> ${_erika_dll_out}")
      return()
    endif()
  endif()
  file(REMOVE
    "${_erika_dll_out}"
    "${ERIKA_REPO_ROOT}/target/${_erika_cfg_dir}/erika_capi.dll.lib"
    "${ERIKA_REPO_ROOT}/target/${_erika_cfg_dir}/erika_capi.lib"
    "${_erika_tag_marker}")
  set(_erika_url
    "https://github.com/AimesSoft/Erika/releases/download/${_erika_tag}/erika-capi-windows-x64.zip")
  set(_erika_work "${ERIKA_REPO_ROOT}/target/erika-prebuilt-windows")
  set(_erika_zip "${_erika_work}/bundle.zip")
  file(REMOVE_RECURSE "${_erika_work}")
  file(MAKE_DIRECTORY "${_erika_work}")
  message(STATUS "Erika: downloading prebuilt ${_erika_url}")
  file(DOWNLOAD "${_erika_url}" "${_erika_zip}" STATUS _erika_dl TIMEOUT 900)
  list(GET _erika_dl 0 _erika_dl_code)
  if(_erika_dl_code EQUAL 0)
    execute_process(
      COMMAND "${CMAKE_COMMAND}" -E tar xf "${_erika_zip}"
      WORKING_DIRECTORY "${_erika_work}"
      RESULT_VARIABLE _erika_unzip)
    file(GLOB_RECURSE _erika_found_dll "${_erika_work}/*/lib/erika_capi.dll")
    if(_erika_unzip EQUAL 0 AND _erika_found_dll)
      list(GET _erika_found_dll 0 _erika_src_dll)
      get_filename_component(_erika_src_lib_dir "${_erika_src_dll}" DIRECTORY)
      file(MAKE_DIRECTORY "${ERIKA_REPO_ROOT}/target/${_erika_cfg_dir}")
      file(COPY "${_erika_src_dll}"
        DESTINATION "${ERIKA_REPO_ROOT}/target/${_erika_cfg_dir}")
      foreach(_erika_extra erika_capi.dll.lib erika_capi.lib)
        if(EXISTS "${_erika_src_lib_dir}/${_erika_extra}")
          file(COPY "${_erika_src_lib_dir}/${_erika_extra}"
            DESTINATION "${ERIKA_REPO_ROOT}/target/${_erika_cfg_dir}")
        endif()
      endforeach()
      if(EXISTS "${_erika_dll_out}")
        file(WRITE "${_erika_tag_marker}" "${_erika_tag}\n")
        message(STATUS "Erika: installed prebuilt ${_erika_tag} -> ${_erika_dll_out}")
        return()
      endif()
    endif()
    message(WARNING "Erika: prebuilt extract failed; building from source")
  else()
    message(WARNING "Erika: prebuilt download failed (${_erika_dl}); building from source")
  endif()
  file(REMOVE "${_erika_tag_marker}")
else()
  # A source build replaces any DLL previously installed by the prebuilt path.
  # Drop its tag marker so a later prebuilt build cannot mistake the source DLL
  # for the cached release artifact.
  if(ERIKA_BUILD_CONFIG STREQUAL "Debug")
    set(_erika_source_cfg_dir "debug")
  else()
    set(_erika_source_cfg_dir "release")
  endif()
  file(REMOVE
    "${ERIKA_REPO_ROOT}/target/${_erika_source_cfg_dir}/erika_capi.dll.prebuilt-tag")
endif()

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
