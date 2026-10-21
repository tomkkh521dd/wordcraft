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
The HarmonyOS project is the repository root (that is what DevEco Studio opens); its only module is
`ohos/entry`. DevEco can build and run the same project: its CMake pick-up reads the Rust staticlib
from `<repo>/target/<triple>/release/` (or `$WORDCRAFT_TARGET_DIR`).

## Installing

An unsigned HAP compiles, but installing needs a signature. A HarmonyOS device and emulator only
trust Huawei's chain, so one of these applies:

1. **DevEco Studio's automatic signing (one click, once per machine).** Open the repository root, sign in,
   *File › Project Structure › Signing Configs › Automatically generate signature*. DevEco writes
   the keystore, certificate and **provisioning profile** (bound to the connected devices' UDIDs)
   into `ohos/build-profile.json5`, and `hvigor` then signs `assembleHap` output by itself.
   That block is machine-local: **do not commit it** (the build script says so too).
2. **Sign a HAP you already built** with your own material:

   ```sh
   packaging/ohos/sign-debug.sh --material /path/to/signing-dir        dist/release/wordcraft-<version>-debug.hap dist/release/wordcraft-<version>-debug-signed.hap
   ```

   The directory needs `password.txt`, a `.p12` keystore, the app's `.cer` certificate **chain**
   (leaf, CA, root) and the `.p7b` provisioning profile for `karl.hmsoft.wordcraft`. A Huawei
   **root** CA certificate is not an app certificate, and a profile is per bundle, so material
   from another project cannot be reused.
3. **OpenHarmony devices only**: `packaging/ohos/sign-debug.sh --openharmony-test …` signs with the
   SDK's test chain. A HarmonyOS image rejects it with *install sign info inconsistent*.

Then (after a signature change, the old copy has to go first, or the install fails with
`9568332 install sign info inconsistent`):

```sh
hdc uninstall karl.hmsoft.wordcraft   # only when the signature changed
hdc install -r dist/release/wordcraft-<version>-debug.hap
hdc shell aa start -a EntryAbility -b karl.hmsoft.wordcraft
```

`hdc` ships with the SDK (`.../sdk/default/openharmony/toolchains/hdc`). For a debug profile, the
device's UDID is `hdc shell bm get --udid` (the emulator answers
`454D55057494E058383604C05D22BBCAD43ABB4167681896C32C700000000000` on this machine).

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
