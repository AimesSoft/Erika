# The Flutter tooling requires that developers have a version of Visual Studio
# installed that includes CMake 3.14 or later. Do not increase this version.
cmake_minimum_required(VERSION 3.14)

set(PROJECT_NAME "erika_flutter")
project(${PROJECT_NAME} LANGUAGES CXX)

cmake_policy(VERSION 3.14...3.25)

set(PLUGIN_NAME "erika_flutter_plugin")

get_filename_component(ERIKA_FLUTTER_PACKAGE_DIR
  "${CMAKE_CURRENT_SOURCE_DIR}/.."
  REALPATH)
if(IS_SYMLINK "${CMAKE_CURRENT_SOURCE_DIR}/..")
  file(READ_SYMLINK "${CMAKE_CURRENT_SOURCE_DIR}/.."
    ERIKA_FLUTTER_PACKAGE_LINK_TARGET)
  file(TO_CMAKE_PATH "${ERIKA_FLUTTER_PACKAGE_LINK_TARGET}"
    ERIKA_FLUTTER_PACKAGE_LINK_TARGET)
  string(REGEX REPLACE "^/\\?\\?/" ""
    ERIKA_FLUTTER_PACKAGE_LINK_TARGET "${ERIKA_FLUTTER_PACKAGE_LINK_TARGET}")
  get_filename_component(ERIKA_FLUTTER_PACKAGE_DIR
    "${ERIKA_FLUTTER_PACKAGE_LINK_TARGET}"
    REALPATH)
endif()
get_filename_component(ERIKA_REPO_ROOT
  "${ERIKA_FLUTTER_PACKAGE_DIR}/../.."
  REALPATH)
if(DEFINED ENV{ERIKA_REPO_ROOT} AND
    EXISTS "$ENV{ERIKA_REPO_ROOT}/crates/erika_capi/include/erika.h")
  file(TO_CMAKE_PATH "$ENV{ERIKA_REPO_ROOT}" ERIKA_REPO_ROOT)
endif()
if(NOT EXISTS "${ERIKA_REPO_ROOT}/crates/erika_capi/include/erika.h")
  set(_ERIKA_SEARCH_DIR "${CMAKE_CURRENT_SOURCE_DIR}")
  foreach(_ERIKA_SEARCH_DEPTH RANGE 0 12)
    if(EXISTS "${_ERIKA_SEARCH_DIR}/crates/erika_capi/include/erika.h")
      get_filename_component(ERIKA_REPO_ROOT "${_ERIKA_SEARCH_DIR}" REALPATH)
      break()
    endif()
    get_filename_component(_ERIKA_PARENT_DIR "${_ERIKA_SEARCH_DIR}/.." REALPATH)
    if(_ERIKA_PARENT_DIR STREQUAL _ERIKA_SEARCH_DIR)
      break()
    endif()
    set(_ERIKA_SEARCH_DIR "${_ERIKA_PARENT_DIR}")
  endforeach()
endif()
if(NOT EXISTS "${ERIKA_REPO_ROOT}/crates/erika_capi/include/erika.h")
  message(FATAL_ERROR
    "Unable to locate Erika repo root from ${CMAKE_CURRENT_SOURCE_DIR}. "
    "Set ERIKA_REPO_ROOT to the Erika checkout.")
endif()

list(APPEND PLUGIN_SOURCES
  "erika_flutter_plugin.cpp"
  "erika_flutter_plugin.h"
)

add_library(${PLUGIN_NAME} SHARED
  "include/erika_flutter/erika_flutter_plugin_c_api.h"
  "erika_flutter_plugin_c_api.cpp"
  ${PLUGIN_SOURCES}
)

apply_standard_settings(${PLUGIN_NAME})

set_target_properties(${PLUGIN_NAME} PROPERTIES
  CXX_VISIBILITY_PRESET hidden)
target_compile_definitions(${PLUGIN_NAME} PRIVATE FLUTTER_PLUGIN_IMPL)

target_include_directories(${PLUGIN_NAME} INTERFACE
  "${CMAKE_CURRENT_SOURCE_DIR}/include")
target_include_directories(${PLUGIN_NAME} PRIVATE
  "${ERIKA_REPO_ROOT}/crates/erika_capi/include")
target_compile_definitions(${PLUGIN_NAME} PRIVATE
  ERIKA_REPO_ROOT_PATH="${ERIKA_REPO_ROOT}")
target_link_libraries(${PLUGIN_NAME} PRIVATE flutter flutter_wrapper_plugin)

find_program(CARGO_EXECUTABLE cargo REQUIRED)
set(ERIKA_CAPI_DLL_PATH
  "${ERIKA_REPO_ROOT}/target/$<IF:$<CONFIG:Debug>,debug,release>/erika_capi.dll")
add_custom_target(erika_capi_runtime
  COMMAND "${CARGO_EXECUTABLE}" build -p erika_capi
    $<$<NOT:$<CONFIG:Debug>>:--release>
  WORKING_DIRECTORY "${ERIKA_REPO_ROOT}"
  COMMENT "Building Erika C API runtime for $<CONFIG>"
  VERBATIM
)
add_dependencies(${PLUGIN_NAME} erika_capi_runtime)

set(erika_flutter_bundled_libraries
  "${ERIKA_CAPI_DLL_PATH}"
  PARENT_SCOPE
)
