# HarmonyOS packaging

Builds the HarmonyOS NEXT app (HAP) from this repository. The platform itself — what works, what
doesn't, and why the port is thin — is documented in [docs/platform-ohos.md](../../docs/platform-ohos.md).

## What you need

- **Rust targets**: `rustup target add aarch64-unknown-linux-ohos x86_64-unknown-linux-ohos`
- **The OHOS SDK** (API 26, compatible 24): DevEco Studio's bundled one is enough. Found through
  `OHOS_SDK_HOME`, `HOS_SDK_HOME` or `DEVECO_SDK_HOME`, else the usual install folders; the newest
  version wins. `env.sh` does the discovery for everyone else.
- **hvigor**: DevEco Studio's `tools/hvigor/bin/hvigorw`, or `hvigorw` on `PATH`, with `node`
  available (`NODE_HOME` or `PATH`).
- **A signing material** to install on a device or emulator (see *Installing*).

## Build

```sh
packaging/ohos/build-native.sh               # the Rust app, both ABIs (the staticlib hvigor links)
packaging/ohos/build-native.sh --link-check  # …and link libwordcraft.so the way CMake will
packaging/ohos/build-hap.sh                  # debug HAP → dist/release/wordcraft-<version>-debug.hap
packaging/ohos/build-hap.sh release
```

`build-hap.sh` runs hvigor with `assembleHap --mode module -p product=default -p buildMode=<mode>`.
DevEco Studio can build and run the same project by opening `ohos/`: its CMake pick-up reads the
Rust staticlib from `<repo>/target/<triple>/release/` (or `$WORDCRAFT_TARGET_DIR`).

## Installing

An unsigned HAP compiles, but installing needs a signature:

1. **DevEco Studio (easiest)**: open `ohos/`, sign in, *File › Project Structure › Signing
   Configs › Automatically generate signature*. It writes the certificate, profile and keystore
   into `build-profile.json5` for this machine only.
2. **Command line**: `hdc install -r dist/release/wordcraft-<version>-debug.hap`.

`hdc` ships with the SDK (`.../sdk/default/openharmony/toolchains/hdc`). Start the emulator from
DevEco's Device Manager (the `MateBook Pro` 2in1 image is the primary target).

## Looking at a run

```sh
hdc shell hilog -T WordCraft      # the shell's and the app's log lines
hdc shell hilog -r -T WordCraft   # follow
```

Step 2 of the port draws a test pattern (see `crates/ohos-app/src/test_pattern.rs`): channel
stripes, a checkerboard, the engine's paragraph count, the pointer, and a sweeping line. If the
stripes read red-green-blue-white and the squares stay square, the buffer format, stride and
density are right.

## Sizes

Each ABI carries the whole engine, so the unsigned debug HAP is ~97 MB with both ABIs
(~46 MB arm64, ~51 MB x86_64, stripped, C++ runtime linked statically). A store build ships one
ABI per device, and trimming the binary is tracked in `docs/platform-ohos.md`.
