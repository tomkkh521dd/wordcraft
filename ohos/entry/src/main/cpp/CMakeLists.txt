# Builds the HarmonyOS side of WordCraft: the C++ shell (NAPI + native window buffer) linked with
# the Rust app's staticlib into libwordcraft.so.
#
# hvigor (DevEco Studio or `hvigorw`) runs this once per ABI. The Rust half must be built first:
#   cargo build --release --target aarch64-unknown-linux-ohos -p wordcraft-ohos
# or simply
#   packaging/ohos/build-native.sh            (both ABIs, into target/)
#   packaging/ohos/build-hap.sh               (the above, then hvigor)
cmake_minimum_required(VERSION 3.5.0)
# The C++ runtime is linked statically (the module's boundary is C): no libc++_shared.so in the
# HAP. Must be set before project(), where hvigor's toolchain reads it.
set(OHOS_STL "c++_static")
project(wordcraft)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

set(NATIVERENDER_ROOT_PATH ${CMAKE_CURRENT_SOURCE_DIR})

# hvigor builds one ABI per pass; the Rust target triple follows it.
if(OHOS_ARCH STREQUAL "x86_64" OR CMAKE_ANDROID_ARCH_ABI STREQUAL "x86_64" OR CMAKE_SYSTEM_PROCESSOR STREQUAL "x86_64")
    set(WORDCRAFT_RUST_TARGET "x86_64-unknown-linux-ohos")
else()
    set(WORDCRAFT_RUST_TARGET "aarch64-unknown-linux-ohos")
endif()

# packaging/ohos/build-hap.sh exports the path; DevEco's Run button uses the workspace's target
# folder (five levels up from this file: cpp → main → src → entry → ohos → repository root).
if(NOT DEFINED WORDCRAFT_RUST_STATICLIB)
    if(DEFINED ENV{WORDCRAFT_RUST_STATICLIB})
        set(WORDCRAFT_RUST_STATICLIB "$ENV{WORDCRAFT_RUST_STATICLIB}")
    else()
        # packaging/env.sh lets CARGO_TARGET_DIR move the build; follow it.
        if(DEFINED ENV{WORDCRAFT_TARGET_DIR} AND NOT "$ENV{WORDCRAFT_TARGET_DIR}" STREQUAL "")
            set(WORDCRAFT_RUST_TARGET_DIR "$ENV{WORDCRAFT_TARGET_DIR}")
        else()
            set(WORDCRAFT_RUST_TARGET_DIR "${NATIVERENDER_ROOT_PATH}/../../../../../target")
        endif()
        set(WORDCRAFT_RUST_STATICLIB
            "${WORDCRAFT_RUST_TARGET_DIR}/${WORDCRAFT_RUST_TARGET}/release/libwordcraft_ohos.a")
    endif()
endif()
message(STATUS "WordCraft: Rust staticlib ${WORDCRAFT_RUST_STATICLIB}")
if(NOT EXISTS "${WORDCRAFT_RUST_STATICLIB}")
    message(FATAL_ERROR
        "The Rust app is not built for ${WORDCRAFT_RUST_TARGET}. Run\n"
        "  cargo build --release --target ${WORDCRAFT_RUST_TARGET} -p wordcraft-ohos\n"
        "or packaging/ohos/build-hap.sh, which does it for every ABI.\n"
        "Missing: ${WORDCRAFT_RUST_STATICLIB}")
endif()

include_directories(${NATIVERENDER_ROOT_PATH})

add_library(wordcraft SHARED
    napi_init.cpp
    present.cpp
)

# The NAPI runtime, the buffer queue of the XComponent's surface, and hilog.
target_link_libraries(wordcraft PUBLIC
    libace_napi.z.so
    libnative_window.so
    libhilog_ndk.z.so
)

# The Rust app (staticlib): the engine, the documents, and from step 3 the egui UI.
target_link_libraries(wordcraft PRIVATE "${WORDCRAFT_RUST_STATICLIB}")

# The C++ runtime goes in statically: the module's boundary is C, so there is no
# libc++_shared.so to ship in the HAP (and none to get out of step with the system's).
target_link_options(wordcraft PRIVATE -static-libstdc++)
