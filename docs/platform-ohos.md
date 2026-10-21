# HarmonyOS (鸿蒙) platform

> **Last reviewed:** 2026-10-21 · **Last updated:** 2026-10-21 · **Change:** major (first version: toolchain gate and the two host injection points; the app shell, services and HAP packaging are planned, see *Status*) · **Target:** HarmonyOS NEXT 5.1+ (API 24, built against API 26), PC/2in1 and tablet

WordCraft builds for HarmonyOS NEXT on `aarch64-unknown-linux-ohos` (devices) and
`x86_64-unknown-linux-ohos` (the DevEco emulators). This page says what works, what doesn't,
how to build it, and — importantly for following upstream releases — **exactly which upstream
files the port touches**.

## Status

| Part | State |
|---|---|
| Core (L0–L5) and the egui UI (L6) compile for both OHOS targets | ✅ done (`cargo xtask ohos`) |
| Host injection points: system font folders, settings folder | ✅ done |
| App shell (`apps/wordcraft-ohos`, `ohos/` HAP project) | ⏳ planned |
| Platform services (file picker, clipboard, IME, appearance, locale) | ⏳ planned |
| Packaging, signing, AppGallery | ⏳ planned |

The desktop shell (`apps/wordcraft`, eframe/winit) is **not** used on HarmonyOS: winit has no
OpenHarmony backend. The HarmonyOS shell draws the same `wordcraft-ui-egui` UI through
`egui::Context::run_ui`, into a native window buffer, and answers `Services` with OS calls.

## Why the port is thin

Both OHOS targets report `target_os = "linux"` with `target_env = "ohos"`. Everything WordCraft
needs from the OS is POSIX (`cfg(unix)` file work, threads, sockets), so the document engine,
layout, renderer (`vello_cpu`) and PDF writer build unchanged. Only the *shell* is platform code,
and it is already swappable: `wordcraft-ui-egui` has no eframe/winit dependency, and every OS
service the UI needs goes through `Services` (`crates/ui-egui/src/lib.rs`).

## Upstream footprint

Keeping the port easy to follow upstream is a rule, not a preference. **Four upstream files** are
touched, all additively, each marked `// OHOS:` where the change is a branch or a cfg:

| File | What | Why it can't be a host-side injection |
|---|---|---|
| `crates/fonts/src/fontdb.rs` | `system_font_dirs` gains an OHOS branch (`/system/fonts`) and `set_system_font_dirs()` | HarmonyOS reports its font folders through the UI framework |
| `crates/fonts/src/lib.rs` | re-export of `set_system_font_dirs` | — |
| `crates/control-key/src/lib.rs` | `set_settings_dir()`: the app sandbox folder | Only the UI framework knows the sandbox path |
| `xtask/src/main.rs` | one `match` arm; the command lives in the new `xtask/src/ohos.rs` | — |

Deliberately **not** changed, because the host can do it instead:

- **Interface language, region, `TMPDIR`, `HOME`**: the native shell sets the environment
  (`LANG`/`LC_ALL`/`TMPDIR`/`HOME`) from the values the UI framework reports, so
  `sys_locale`/`Region::from_locale_vars`/`std::env::temp_dir` keep working unchanged.
- **Read Aloud**: the shell offers no service hook, so `speech.rs`'s `spd-say` path never runs.
- **`zbus` (desktop theme)**: still compiled for OHOS (it builds fine) but never called; narrowing
  its cfg is not worth the merge surface yet.
- **Desktop shell** (`apps/wordcraft`): untouched.
- **Root `Cargo.toml`**: `members = ["crates/*", "apps/*", …]` already picks new crates up.

Everything else is new: `ohos/` (HAP project), `apps/wordcraft-ohos/` (Rust shell), `crates/ui-paint/`
(the egui software rasteriser), `packaging/ohos/`, this page.

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

The HAP is built by DevEco's own tooling (planned, see *Status*):

```sh
hvigorw assembleHap --mode module -p product=default -p buildMode=debug --no-daemon
```

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
  buffer. No GPU context is created, so a driver bug can't take the editor down.
- **`unsafe`.** The port keeps `unsafe_code = "forbid"`: every NAPI/NDK pointer stays in the small
  C++ layer (`ohos/entry/src/main/cpp`), and Rust only exposes `extern "C"` entry points.

## Known gaps (planned, tracked in [gaps.md](gaps.md))

- Printing: no `craft-print` port; the print UI hides itself (no `print_native` service).
- Read Aloud: no TTS hook yet.
- Zotero: needs the Zotero desktop app, which doesn't exist on HarmonyOS.
- Phone-sized layout: the ribbon collapses, but a phone-first layout is later work.
- No GPU renderer; very large windows on high-DPI screens are CPU-bound.

## Revision history

| Date | Kind | Change |
|---|---|---|
| 2026-10-21 | major | First version: `cargo xtask ohos` for both targets, font/settings injection points, upstream-footprint policy |
