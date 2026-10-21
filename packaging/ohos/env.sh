# shellcheck shell=bash
# Shared setup for the HarmonyOS packaging scripts. Source it: `. "$(dirname "$0")/env.sh"`.
#
# Exports:
#   ROOT            repository root
#   VERSION, DIST   from packaging/env.sh (so the HAP and the other installers share a version)
#   OHOS_NDK        the NDK folder (holding llvm/ and sysroot/)
#   OHOS_SYSROOT    <ndk>/sysroot
#   OHOS_CC/CXX     the NDK compilers
#   HVIGORW         the hvigor wrapper (DevEco's, or one on PATH)
#   RUST_TARGETS    Rust triples, in the same order as OHOS_ABIS
#   OHOS_ABIS       the ABI names hvigor uses
#
# The NDK is found like `cargo xtask ohos` finds it: OHOS_SDK_HOME, HOS_SDK_HOME or
# DEVECO_SDK_HOME, then the usual install folders; the newest version folder wins.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=../env.sh
. "$ROOT/packaging/env.sh"

# Rust target ↔ hvigor ABI, in the order the HAP bundles them.
RUST_TARGETS=(aarch64-unknown-linux-ohos x86_64-unknown-linux-ohos)
OHOS_ABIS=(arm64-v8a x86_64)

_has_ndk() {
  [ -d "$1/sysroot" ] || return 1
  [ -x "$1/llvm/bin/clang" ] || [ -x "$1/llvm/bin/clang.exe" ]
}

# The newest NDK under the usual places.
_find_ndk() {
  local roots=() root candidate best=""
  for var in OHOS_SDK_HOME HOS_SDK_HOME DEVECO_SDK_HOME; do
    if [ -n "${!var:-}" ]; then roots+=("${!var}"); fi
  done
  roots+=(
    "$ROOT/ohos-sdk"
    "/opt/ohos-sdk"
    "$HOME/ohos-sdk"
    "$HOME/ohos-sdk-home"
    "/Applications/DevEco-Studio.app/Contents/sdk"
    "/c/Program Files/Huawei/DevEco Studio/sdk"
    "C:/Program Files/Huawei/DevEco Studio/sdk"
  )
  for root in "${roots[@]}"; do
    [ -d "$root" ] || continue
    for candidate in "$root/native" "$root/default/openharmony/native" "$root"/*/native "$root"/*/openharmony/native \
      "$root"/*/default/openharmony/native; do
      [ -d "$candidate" ] || continue
      _has_ndk "$candidate" || continue
      if [ -z "$best" ] || [ "$(printf '%s\n%s\n' "$best" "$candidate" | sort -V | tail -1)" = "$candidate" ]; then
        best="$candidate"
      fi
    done
  done
  [ -n "$best" ] || return 1
  printf '%s\n' "$best"
}

if [ -z "${OHOS_NDK:-}" ]; then
  OHOS_NDK="$(_find_ndk)" || {
    echo "error: no OHOS NDK found. Set OHOS_SDK_HOME to the SDK folder (one holding" >&2
    echo "       <version>/native or default/openharmony/native)." >&2
    return 1 2>/dev/null || exit 1
  }
fi
export OHOS_NDK
export OHOS_SYSROOT="$OHOS_NDK/sysroot"

if [ -x "$OHOS_NDK/llvm/bin/clang.exe" ]; then
  export OHOS_CC="$OHOS_NDK/llvm/bin/clang.exe"
  export OHOS_CXX="$OHOS_NDK/llvm/bin/clang++.exe"
else
  export OHOS_CC="$OHOS_NDK/llvm/bin/clang"
  export OHOS_CXX="$OHOS_NDK/llvm/bin/clang++"
fi

if [ -z "${HVIGORW:-}" ]; then
  if command -v hvigorw >/dev/null 2>&1; then
    HVIGORW="$(command -v hvigorw)"
  elif [ -x "/c/Program Files/Huawei/DevEco Studio/tools/hvigor/bin/hvigorw" ]; then
    HVIGORW="/c/Program Files/Huawei/DevEco Studio/tools/hvigor/bin/hvigorw"
  elif [ -x "/Applications/DevEco-Studio.app/Contents/tools/hvigor/bin/hvigorw" ]; then
    HVIGORW="/Applications/DevEco-Studio.app/Contents/tools/hvigor/bin/hvigorw"
  else
    HVIGORW=""
  fi
fi
export HVIGORW

ohos_ndk_triple() {
  case "$1" in
    x86_64-*) printf 'x86_64-linux-ohos\n' ;;
    *) printf 'aarch64-linux-ohos\n' ;;
  esac
}
