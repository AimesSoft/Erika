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

set(erika_flutter_bundled_libraries
  ""
  PARENT_SCOPE
)
