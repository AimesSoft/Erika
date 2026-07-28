cmake_minimum_required(VERSION 3.17)
project(erika_flutter LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(PLUGIN_NAME erika_flutter_plugin)

get_filename_component(
  ERIKA_ROOT
  "${CMAKE_CURRENT_SOURCE_DIR}/../../../../../.."
  REALPATH
)
set(ERIKA_TARGET aarch64-unknown-linux-ohos)
set(ERIKA_RUNTIME
  "${ERIKA_ROOT}/target/${ERIKA_TARGET}/release/liberika_capi.so"
)

if(NOT DEFINED OHOS_SDK_NATIVE)
  message(FATAL_ERROR "OHOS_SDK_NATIVE is required to build Erika for OpenHarmony")
endif()

find_program(CARGO_EXECUTABLE cargo REQUIRED)
set(OHOS_LLVM_BIN "${OHOS_SDK_NATIVE}/llvm/bin")
set(OHOS_CXX_INCLUDE
  "${OHOS_SDK_NATIVE}/llvm/include/libcxx-ohos/include/c++/v1"
)
if(NOT EXISTS "${OHOS_CXX_INCLUDE}/vector")
  set(OHOS_CXX_INCLUDE "${OHOS_SDK_NATIVE}/llvm/include/c++/v1")
endif()
set(OHOS_BINDGEN_ARGS
  "--target=${ERIKA_TARGET} --sysroot=${OHOS_SDK_NATIVE}/sysroot -isystem ${OHOS_CXX_INCLUDE}"
)
file(GLOB_RECURSE ERIKA_RUST_SOURCES CONFIGURE_DEPENDS
  "${ERIKA_ROOT}/crates/*.rs"
  "${ERIKA_ROOT}/xtask/*.rs"
)
list(APPEND ERIKA_RUST_SOURCES
  "${ERIKA_ROOT}/Cargo.toml"
  "${ERIKA_ROOT}/Cargo.lock"
)

add_custom_command(
  OUTPUT "${ERIKA_RUNTIME}"
  COMMAND
    "${CMAKE_COMMAND}" -E env
      "OHOS_NDK_HOME=${OHOS_SDK_NATIVE}"
      "${CARGO_EXECUTABLE}" +1.93.0 run -q -p xtask --
      deps build --profile lgpl --target "${ERIKA_TARGET}"
  COMMAND
    "${CMAKE_COMMAND}" -E env
      "ERIKA_NATIVE_PROFILE=lgpl"
      "ERIKA_NATIVE_TARGET=${ERIKA_TARGET}"
      "OHOS_NDK_HOME=${OHOS_SDK_NATIVE}"
      "LIBCLANG_PATH=${OHOS_SDK_NATIVE}/llvm/lib"
      "BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_linux_ohos=${OHOS_BINDGEN_ARGS}"
      "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER=${OHOS_LLVM_BIN}/aarch64-unknown-linux-ohos-clang"
      "CC_aarch64_unknown_linux_ohos=${OHOS_LLVM_BIN}/aarch64-unknown-linux-ohos-clang"
      "CXX_aarch64_unknown_linux_ohos=${OHOS_LLVM_BIN}/aarch64-unknown-linux-ohos-clang++"
      "AR_aarch64_unknown_linux_ohos=${OHOS_LLVM_BIN}/llvm-ar"
      "${CARGO_EXECUTABLE}" +1.93.0 build -p erika_capi
      --target "${ERIKA_TARGET}" --release
      --no-default-features --features wgpu
  WORKING_DIRECTORY "${ERIKA_ROOT}"
  DEPENDS ${ERIKA_RUST_SOURCES}
  COMMENT "Building Erika OpenHarmony runtime"
  VERBATIM
)
add_custom_target(erika_ohos_runtime DEPENDS "${ERIKA_RUNTIME}")

add_library(erika_capi SHARED IMPORTED GLOBAL)
set_target_properties(erika_capi PROPERTIES IMPORTED_LOCATION "${ERIKA_RUNTIME}")
add_dependencies(erika_capi erika_ohos_runtime)

add_library(${PLUGIN_NAME} SHARED erika_flutter_plugin.cpp)
add_dependencies(${PLUGIN_NAME} erika_ohos_runtime)
set_target_properties(${PLUGIN_NAME} PROPERTIES
  CXX_VISIBILITY_PRESET hidden
  OUTPUT_NAME "erika_flutter"
)
target_include_directories(${PLUGIN_NAME} PRIVATE
  "${CMAKE_CURRENT_SOURCE_DIR}/include"
  "${ERIKA_ROOT}/crates/erika_capi/include"
)
target_compile_options(${PLUGIN_NAME} PRIVATE
  -fno-exceptions
  -ffunction-sections
  -fdata-sections
)
target_link_options(${PLUGIN_NAME} PRIVATE -Wl,--gc-sections)
target_link_libraries(${PLUGIN_NAME} PRIVATE
  erika_capi
  libace_napi.z.so
  libnative_window.so
  libhilog_ndk.z.so
)
