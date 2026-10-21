#!/usr/bin/env bash
# Builds the HAP: the Rust app for both ABIs, then hvigor (DevEco's build tool) packages ArkTS,
# resources and libwordcraft.so into entry-default.hap and copies it to dist/release.
#
#   packaging/ohos/build-hap.sh            debug (the default while porting)
#   packaging/ohos/build-hap.sh release
#
# An unsigned HAP builds without any signing setup; installing it needs one, see
# docs/platform-ohos.md ("Installing"): DevEco's automatic signing, or packaging/ohos/sign-debug.sh.
set -euo pipefail

. "$(dirname "$0")/env.sh"

mode="${1:-debug}"
case "$mode" in
  debug | release) ;;
  -h | --help)
    sed -n '2,10p' "$0"
    exit 0
    ;;
  *)
    echo "error: expected debug or release, got '$mode'" >&2
    exit 2
    ;;
esac

if [ -z "${HVIGORW:-}" ] || [ ! -x "$HVIGORW" ]; then
  echo "error: hvigorw not found. It ships with DevEco Studio (tools/hvigor/bin);" >&2
  echo "       set HVIGORW=/path/to/hvigorw, or build from DevEco Studio instead." >&2
  exit 1
fi

"$(dirname "$0")/build-native.sh"

# CMake finds the Rust staticlib through this (see entry/src/main/cpp/CMakeLists.txt), so a moved
# CARGO_TARGET_DIR keeps working.
export WORDCRAFT_TARGET_DIR="$CARGO_TARGET_DIR"

# Signing material is machine-local: DevEco's "Automatically generate signature" writes it into
# build-profile.json5, which is tracked. Building with it is normal; committing it is not.
if ! git -C "$ROOT" diff --quiet -- ohos/build-profile.json5 2>/dev/null; then
  echo "note: ohos/build-profile.json5 has local changes (signing material) — do not commit them." >&2
fi

cd "$ROOT/ohos"
echo "== WordCraft: hvigor assembleHap ($mode)"
"$HVIGORW" clean --no-daemon >/dev/null 2>&1 || true
"$HVIGORW" assembleHap --mode module -p product=default -p buildMode="$mode" --no-daemon

hap="$(find "$ROOT/ohos/entry/build" -name '*.hap' -type f -print0 2>/dev/null | xargs -0 ls -t 2>/dev/null | head -1 || true)"
if [ -z "$hap" ]; then
  echo "error: hvigor finished but no .hap was found under ohos/entry/build" >&2
  exit 1
fi
mkdir -p "$DIST"
name="wordcraft-${VERSION}-${mode}.hap"
cp -f "$hap" "$DIST/$name"
echo "WordCraft: $DIST/$name ($(du -h "$DIST/$name" | cut -f1))"
echo "Install it with: hdc install -r \"$DIST/$name\""
