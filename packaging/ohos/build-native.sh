#!/usr/bin/env bash
# Builds the Rust half of the HarmonyOS app — the staticlib that CMake links into
# libwordcraft.so — for both ABIs.
#
#   packaging/ohos/build-native.sh                  release, both ABIs
#   packaging/ohos/build-native.sh --link-check     …and link the whole native library the way
#                                                   CMake will, to prove C++ and Rust fit together
#   WORDCRAFT_PROFILE=debug packaging/ohos/build-native.sh
#
# hvigor (DevEco or packaging/ohos/build-hap.sh) needs the staticlibs this produces.
set -euo pipefail

. "$(dirname "$0")/env.sh"

link_check=0
for arg in "$@"; do
  case "$arg" in
    --link-check) link_check=1 ;;
    -h | --help)
      sed -n '2,12p' "$0"
      exit 0
      ;;
    *)
      echo "error: unknown option $arg" >&2
      exit 2
      ;;
  esac
done

profile="${WORDCRAFT_PROFILE:-release}"
if [ "$profile" = "release" ]; then
  profile_flag=(--release)
  profile_dir=release
else
  profile_flag=()
  profile_dir=debug
fi

for i in "${!RUST_TARGETS[@]}"; do
  target="${RUST_TARGETS[$i]}"
  abi="${OHOS_ABIS[$i]}"
  echo "== WordCraft: cargo build ${profile} for ${target} (${abi})"
  # A staticlib needs no linker, so the NDK only matters for the C++ side (below) and hvigor.
  cargo build --manifest-path "$ROOT/Cargo.toml" -p wordcraft-ohos --target "$target" "${profile_flag[@]}"
  staticlib="$ROOT/target/${target}/${profile_dir}/libwordcraft_ohos.a"
  if [ ! -f "$staticlib" ]; then
    echo "error: expected $staticlib" >&2
    exit 1
  fi
  echo "   $staticlib ($(du -h "$staticlib" | cut -f1))"

  if [ "$link_check" = "1" ]; then
    triple="$(ohos_ndk_triple "$target")"
    out="$ROOT/target/ohos-link/$abi"
    mkdir -p "$out"
    echo "== WordCraft: link check for ${triple}"
    "$OHOS_CXX" --target="$triple" --sysroot="$OHOS_SYSROOT" -std=c++17 -fPIC -shared -O2 -static-libstdc++ \
      -I"$ROOT/ohos/entry/src/main/cpp" \
      "$ROOT/ohos/entry/src/main/cpp/napi_init.cpp" \
      "$ROOT/ohos/entry/src/main/cpp/present.cpp" \
      "$staticlib" \
      -Wl,--no-undefined \
      -lace_napi.z -lnative_window -lhilog_ndk.z \
      -o "$out/libwordcraft.so"
    # What the HAP ships: no debug info, no libc++_shared.so (see CMakeLists.txt).
    if [ -x "$OHOS_NDK/llvm/bin/llvm-strip" ]; then
      "$OHOS_NDK/llvm/bin/llvm-strip" --strip-unneeded "$out/libwordcraft.so"
    fi
    echo "   $out/libwordcraft.so ($(du -h "$out/libwordcraft.so" | cut -f1))"
  fi
done

echo "WordCraft: HarmonyOS native build done"
