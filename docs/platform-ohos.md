# HarmonyOS (鸿蒙) platform

> **Last reviewed:** 2026-10-21 · **Last updated:** 2026-10-21 · **Change:** minor (first frame verified on the emulator: format, stride, density, input, engine; `mmap` for the buffer, and the uninstall a changed signature needs; previously trivial: signing; previously minor: step 2, HAP project, C++ shell and test pattern; previously major: first version) · **Target:** HarmonyOS NEXT 5.1+ (API 24, built against API 26), PC/2in1 and tablet

WordCraft builds for HarmonyOS NEXT on `aarch64-unknown-linux-ohos` (devices) and
`x86_64-unknown-linux-ohos` (the DevEco emulators). This page says what works, what doesn't,
how to build it, and — importantly for following upstream releases — **exactly which upstream
files the port touches**.

## Status

| Part | State |
|---|---|
| Core (L0–L5) and the egui UI (L6) compile for both OHOS targets | ✅ done (`cargo xtask ohos`) |
| Host injection points: system font folders, settings folder | ✅ done |
| HAP project, C++ shell (NAPI + native window buffer), Rust app | ✅ builds; **first frame verified on the emulator** (see *Verified on the emulator*) |
| The document UI (egui) on the surface | ⏳ next |
| Platform services (file picker, clipboard, IME, appearance, locale) | ⏳ planned |
| Packaging (signed HAP), AppGallery | ⏳ planned |

The desktop shell (`apps/wordcraft`, eframe/winit) is **not** used on HarmonyOS: winit has no
OpenHarmony backend. The HarmonyOS shell draws the same `wordcraft-ui-egui` UI through
`egui::Context::run_ui`, into a native window buffer, and answers `Services` with OS calls.

## Why the port is thin

Both OHOS targets report `target_os = "linux"` with `target_env = "ohos"`. Everything WordCraft
needs from the OS is POSIX (`cfg(unix)` file work, threads, sockets), so the document engine,
layout, renderer (`vello_cpu`) and PDF writer build unchanged. Only the *shell* is platform code,
and it is already swappable: `wordcraft-ui-egui` has no eframe/winit dependency, and every OS
service the UI needs goes through `Services` (`crates/ui-egui/src/lib.rs`).

## The project

```
ohos/                        HarmonyOS project (DevEco Studio opens this folder)
  AppScope/app.json5         bundleName karl.hmsoft.wordcraft, version, icon
  entry/src/main/ets/        EntryAbility, the page with the XComponent, input, system info
  entry/src/main/cpp/        NAPI module, the surface's buffer queue, hilog (no unsafe in Rust)
  entry/src/main/cpp/types/  the module's .d.ts, so ArkTS sees typed functions
apps/wordcraft-ohos/         the C ABI the C++ calls (wc_*), the only `#[unsafe(no_mangle)]` in the port
crates/ohos-app/             the app: configuration, input, the document session, the frame
packaging/ohos/              env discovery, build-native.sh, build-hap.sh
```

ArkTS owns what belongs to the UI framework — the XComponent and its surface lifecycle, input
events, the display density, the system's fonts, language and colour mode — and passes plain
values to C++, which turns them into environment variables and `wc_*` calls. Configuration crosses
the boundary as environment variables so no pointer ever enters Rust; frames leave as a
`(pointer, length)` pair that C++ copies into the window buffer.

## Upstream footprint

Keeping the port easy to follow upstream is a rule, not a preference. **Five upstream files** are
touched, all additively, each marked `// OHOS:` where the change is a branch or a cfg:

| File | What | Why it can't be a host-side injection |
|---|---|---|
| `crates/fonts/src/fontdb.rs` | `system_font_dirs` gains an OHOS branch (`/system/fonts`) and `set_system_font_dirs()` | HarmonyOS reports its font folders through the UI framework |
| `crates/fonts/src/lib.rs` | re-export of `set_system_font_dirs` | — |
| `crates/control-key/src/lib.rs` | `set_settings_dir()`: the app sandbox folder | Only the UI framework knows the sandbox path |
| `xtask/src/main.rs` | one `match` arm; the command lives in the new `xtask/src/ohos.rs` | — |
| `xtask/src/layers.rs` | two table entries: `ohos-app` at L6, `ohos` exempt | The layering check requires every crate to be classified |

Deliberately **not** changed, because the host can do it instead:

- **Interface language, region, `TMPDIR`, `HOME`**: the native shell sets the environment
  (`LANG`/`LC_ALL`/`TMPDIR`/`HOME`) from the values the UI framework reports, so
  `sys_locale`/`Region::from_locale_vars`/`std::env::temp_dir` keep working unchanged.
- **Read Aloud**: the shell offers no service hook, so `speech.rs`'s `spd-say` path never runs.
- **`zbus` (desktop theme)**: still compiled for OHOS (it builds fine) but never called; narrowing
  its cfg is not worth the merge surface yet.
- **Desktop shell** (`apps/wordcraft`): untouched.
- **Root `Cargo.toml`**: `members = ["crates/*", "apps/*", …]` already picks new crates up.

Everything else is new: `ohos/` (the HAP project), `apps/wordcraft-ohos/` (the C ABI crate),
`crates/ohos-app/` (the app), `packaging/ohos/`, this page — and, next, `crates/ui-paint/` (the
egui software rasteriser).

## Building

```sh
rustup target add aarch64-unknown-linux-ohos x86_64-unknown-linux-ohos

cargo xtask ohos            # check every crate the app links, both targets
cargo xtask ohos --build    # link them too
cargo xtask ohos --target aarch64
cargo xtask ohos --build --target x86_64
```

`cargo xtask ohos` finds the NDK itself: `OHOS_SDK_HOME`, `HOS_SDK_HOME`, `DEVECO_SDK_HOME`, then
the usual install folders (DevEco Studio's bundled SDK included), newest version first. It needs
`<ndk>/llvm/bin/clang` and `<ndk>/sysroot`, and writes small compiler wrappers under
`target/ohos-toolchain/` so paths with spaces work. No per-machine cargo config is needed.

The app itself (hvigor, DevEco's build tool, packages ArkTS, resources and `libwordcraft.so`):

```sh
packaging/ohos/build-native.sh              # the Rust staticlib for both ABIs
packaging/ohos/build-hap.sh                 # …then hvigor packages the HAP into dist/release/
packaging/ohos/build-hap.sh release         # a release HAP
```

Installing needs a signature from Huawei's chain: DevEco's *Automatically generate signature*
(once per machine; it also creates the per-bundle provisioning profile that names the connected
devices' UDIDs), or your own keystore + app certificate chain + profile through
`packaging/ohos/sign-debug.sh --material`. The SDK's OpenHarmony test chain is accepted only by
OpenHarmony images — a HarmonyOS device or emulator answers *install sign info inconsistent*.
`hdc install -r dist/release/wordcraft-<version>-debug.hap` then puts it on the device; see
[packaging/ohos/README.md](../packaging/ohos/README.md) for the details and the log commands.

### Verified on the emulator

HarmonyOS 6.1.1 (API 24, x86_64) tablet emulator, `karl.hmsoft.wordcraft`, 1920×2799 at 1.5×:

| What | Result |
|---|---|
| Buffer format | stripes read red-green-blue-white: the buffer is RGBA, not BGRA |
| Stride | the checkerboard's squares stay square |
| Density | the 10-vp ruler draws ten 15-px squares; a touch at 700,1100 px puts the crosshair there exactly |
| Input | touch down/move reach the app (`events` climbs), and the pointer is drawn where it was put |
| Engine | the sample document loads and lays out (41 paragraphs) |
| System fonts | one folder reported by `font.getUIFontConfig()` |
| Frames | presented every display refresh (~24 fps for the 5.4 MP test pattern, which repaints every pixel per frame; the real UI will paint rows, not pixels) |

Two things only a device shows, both handled:

- **`BufferHandle.virAddr` is null** on this emulator. The documented CPU path is to `mmap` the
  buffer's file descriptor for the frame and `munmap` it before `FlushBuffer`; that is what
  `present.cpp` does now.
- **A different signature cannot replace an installed app.** Installing over a build signed with
  another certificate fails with `9568332 install sign info inconsistent`; `hdc uninstall
  karl.hmsoft.wordcraft` first. (DevEco's *Run* installed one such build; the CLI install then
  needed the uninstall.)

### What the first frame shows

Until the UI lands (next step), the app draws a test pattern
(`crates/ohos-app/src/test_pattern.rs`) that makes a wrong buffer or input path obvious:

- four stripes, **red, green, blue, white** — blue where red belongs means the buffer is BGRA;
- a checkerboard — a wrong stride skews the squares into a rhombus;
- the very top line turns green with focus, grey without;
- an orange bar that grows with the events received (input reaches the app);
- one square per paragraph in the sample document (the engine runs on the device) and one cyan
  square per system font folder the shell reported;
- a yellow line sweeping across (frames advance), a crosshair at the pointer, and ten 10-vp
  squares at the bottom left (density).

## Platform notes

- **Fonts.** Bundled OFL families (`assets/fonts/`) always load. System fonts come from
  `/system/fonts` plus the folders the UI framework reports
  (`font.getUIFontConfig().fontDir`), passed to `wordcraft_fonts::set_system_font_dirs` at
  start-up. Word's font substitution (`crates/fonts/src/word.rs`) maps Calibri/Aptos and friends
  to what the device has.
- **Settings and logs.** `wordcraft_control_key::set_settings_dir(context.filesDir + "/settings")`
  at start-up; `WORDCRAFT_PROFILE_DIR` still overrides it for isolated runs.
- **Files.** User documents are picked with `DocumentViewPicker`; the picked URI's temporary
  grant covers the physical path the app receives, and persistent access for Recent files uses
  `fileshare.persistPermission`. The engine reads and writes ordinary paths, so saving, autosave
  and crash recovery (`crates/engine/src/recovery/desktop.rs`) work as on Linux.
- **Rendering.** Pages are rasterised on the CPU by `vello_cpu` (`crates/render`); the shell
  composites egui's triangles on the CPU as well and hands finished frames to the native window
  buffer (`OH_NativeWindow_NativeWindowRequestBuffer` → copy → `FlushBuffer`). No GPU context is
  created, so a driver bug can't take the editor down.
- **`unsafe`.** Every NAPI and NDK pointer stays in the small C++ layer
  (`ohos/entry/src/main/cpp`). Rust has one exception: exporting a symbol C can find needs
  `#[unsafe(no_mangle)]`, so `apps/wordcraft-ohos` sets `unsafe_code = "deny"` and only its
  `src/abi.rs` opts out; nothing there dereferences a pointer. `crates/ohos-app` (all the logic)
  keeps `unsafe_code = "forbid"`.

## Known gaps (planned, tracked in [gaps.md](gaps.md))

- Printing: no `craft-print` port; the print UI hides itself (no `print_native` service).
- Read Aloud: no TTS hook yet.
- Zotero: needs the Zotero desktop app, which doesn't exist on HarmonyOS.
- Phone-sized layout: the ribbon collapses, but a phone-first layout is later work.
- No GPU renderer; very large windows on high-DPI screens are CPU-bound.

## Revision history

| Date | Kind | Change |
|---|---|---|
| 2026-10-21 | minor | First frame verified on the emulator: RGBA order, stride, density, input and the engine all correct; `mmap` needed because `virAddr` is null; a changed signature needs an uninstall first |
| 2026-10-21 | trivial | Signing: `sign-debug.sh` (own material, or the SDK's OpenHarmony test chain), what a HarmonyOS image refuses, and the UDID a debug profile is bound to |
| 2026-10-21 | minor | Step 2: HAP project, C++ shell (NAPI, native window buffer, hilog), Rust app with configuration and input, test pattern; unsigned HAP builds for arm64 and x86_64 |
| 2026-10-21 | major | First version: `cargo xtask ohos` for both targets, font/settings injection points, upstream-footprint policy |
