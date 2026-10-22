# HarmonyOS (鸿蒙) platform

> **Last reviewed:** 2026-10-22 · **Last updated:** 2026-10-22 · **Change:** minor (a real HarmonyOS 6.1 tablet: the UI at 2.25×, the picker and the clipboard verified there too, including a paste of another app's copy without the restricted clipboard permission; only keys that can type carry a character, since injected keys bring garbage `keyText`; the working copy takes the name the user typed; previously minor: the real UI runs on the device, the CJK interface face and the icon glyphs are fixed, and the frame rate is the open problem; previously minor: first frame verified on the emulator: format, stride, density, input, engine; `mmap` for the buffer, and the uninstall a changed signature needs; previously trivial: signing; previously minor: step 2, HAP project, C++ shell and test pattern; previously major: first version) · **Target:** HarmonyOS NEXT 5.1+ (API 24, built against API 26), PC/2in1 and tablet

WordCraft builds for HarmonyOS NEXT on `aarch64-unknown-linux-ohos` (devices) and
`x86_64-unknown-linux-ohos` (the DevEco emulators). This page says what works, what doesn't,
how to build it, and — importantly for following upstream releases — **exactly which upstream
files the port touches**.

## Status

| Part | State |
|---|---|
| Core (L0–L5) and the egui UI (L6) compile for both OHOS targets | ✅ done (`cargo xtask ohos`) |
| Host injection points: system font folders, settings folder, language, colour mode | ✅ done |
| HAP project, C++ shell (NAPI + native window buffer), Rust app | ✅ builds and runs on the emulator |
| The document UI (egui) on the surface | ✅ done: ribbon, ruler, pages, tables, lists, status bar |
| Platform services: system font folders, locale, dark mode | ✅ done |
| Platform services: file picker (open, save), clipboard (text, HTML, pictures) | ✅ done and verified on the emulator (see *Platform services*) |
| Input method (soft keyboard, Chinese IME, candidate window) | ⏳ next (OHOS.5) |
| Packaging (signed HAP), AppGallery | ⏳ planned (OHOS.6) |

The desktop shell (`apps/wordcraft`, eframe/winit) is **not** used on HarmonyOS: winit has no
OpenHarmony backend. The HarmonyOS shell draws the same `wordcraft-ui-egui` UI through
`egui::Context::run_ui`, into a native window buffer, and answers `Services` with OS calls.

## Why the port is thin

Both OHOS targets report `target_os = "linux"` with `target_env = "ohos"`. Everything WordCraft
needs from the OS is POSIX (`cfg(unix)` file work, threads, sockets), so the document engine,
layout, renderer (`vello_cpu`) and PDF writer build unchanged. Only the *shell* is platform code,
and it is already swappable: `wordcraft-ui-egui` has no eframe/winit dependency, and every OS
service the UI needs goes through `Services` (`crates/ui-egui/src/lib.rs`).

## Platform services

The shell pushes values into Rust (surface, input, configuration). Opening a file, reading the
clipboard and the like need the other direction: Rust asks, ArkTS answers. The bridge is
`crates/ohos-app/src/host.rs`:

- Rust queues a JSON request (`{"id": 7, "kind": "pickOpen", "purpose": "document"}`) and keeps a
  channel for the answer, with a deadline so a dialog that never answers cannot leak a caller.
- ArkTS polls `native.hostTake()` once per frame (it already runs a `displaySync` callback) and
  finishes with `native.hostAnswer(id, json)`, which hands the bytes back through the channel.
  One request per frame, so a slow picker never blocks the frame the user tapped in.
- No pointer enters Rust except that one borrowed answer string (`wc_host_answer` is `unsafe` for
  it, with the contract in `wc_abi.h`).

### Files: the picker grants a file, not its folder

`DocumentViewPicker` returns a URI, and the app's temporary grant covers **that file**. The engine
writes a document the way every careful editor does — a temporary file beside the destination,
then a rename — and creating that sibling is refused (`13900012`, permission denied; measured on
the emulator). Opening the picked file for writing works, so the grant is real; only the folder
is out of reach.

So the app edits a **working copy** in its sandbox:

- Open: ArkTS copies the picked file into `filesDir/documents/<n>/<name>` and answers that path;
  the engine opens it, and autosave, recovery and Recent files all work on ordinary paths.
- Save/Save As: the picker chooses the destination, and the engine writes the working copy.
- Half a second later (and only when the working copy changed), ArkTS copies it back over the
  picked URI. Saves, exports and Print-to-PDF all take that route, because they all write files
  the user chose.

One working copy per opened or saved file, each in its own numbered folder so the file keeps
exactly the name the user's file has (the title bar shows it).

### Clipboard

Text, HTML and pictures both ways. Reading returns the plain text, the HTML alternative another
program put there, the MIME types (so "nothing to paste" can say what *is* there) and any copied
file URIs; a clipboard picture (a `PixelMap`) is packed to a PNG in the app's cache folder and the
bytes are read (and the file removed) on the Rust side, the same `ClipboardPicture::Encoded` route
the desktop takes. Writing puts the text and the HTML alternative on the picker-facing clipboard
in one `PasteData`. HarmonyOS has no MathML clipboard record, so a copied equation arrives in
other programs as its HTML/text.

The API documents `ohos.permission.READ_PASTEBOARD` (a *restricted* user grant that AppGallery
approves per app) for reading another program's clipboard. On the test tablet (HarmonyOS 6.1.0) a
paste of a 备忘录 copy worked with no permission declared, so the grant is not needed there; if a
firmware or a release signature refuses the read, the app keeps its own last copy in the cache, so
in-app copy and paste keep working and a failed read never silently empties the clipboard. Verify
this again on release firmware before shipping.

## The project

```
build-profile.json5          the HarmonyOS project DevEco Studio opens (at the repository root)
AppScope/app.json5           bundleName karl.hmsoft.wordcraft, version, icon
hvigorfile.ts, hvigor/       the hvigor build (DevEco's tool)
ohos/entry/                  the project's only module
  src/main/ets/              EntryAbility, the page with the XComponent, input, system info
  src/main/cpp/              NAPI module, the surface's buffer queue, hilog (no unsafe in Rust)
  src/main/cpp/types/        the module's .d.ts, so ArkTS sees typed functions
  src/main/resources/        strings, colours, icons, the page list
apps/wordcraft-ohos/         the C ABI the C++ calls (wc_*), the only `#[unsafe(no_mangle)]` in the port
crates/ohos-app/             the app: configuration, input, the document session, the frame
packaging/ohos/              env discovery, build-native.sh, build-hap.sh, sign-debug.sh
```

DevEco Studio needs the project at the folder it opens, so the project sits at the repository root
and its module at `ohos/entry`; nothing else in the repository is HarmonyOS-specific.

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
| `crates/fonts/src/ui_fallback.rs` | `HarmonyOS_Sans_SC.ttf`/`_TC.ttf` first among the interface CJK fallbacks | Devices ship no Noto/Source Han; without it a Chinese interface drew every label as a missing-glyph mark |

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

`build-hap.sh` runs hvigor at the repository root (the project DevEco opens), so DevEco's signing
material — which it writes into `build-profile.json5` for this machine only — is what signs the
HAP. That file is tracked without it: do not commit the signing block.

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
| The interface | the ribbon in Chinese (harmonyOS ships `HarmonyOS_Sans_SC.ttf`), the ruler, the sample document with a table, numbered and bulleted lists, page numbers and the status bar |
| Buffer format | stripes read red-green-blue-white during step 2: the buffer is RGBA, not BGRA |
| Stride | the checkerboard's squares stayed square (step 2) |
| Density | the 10-vp ruler draws ten 15-px squares; touches land where they are put (a 1920×2799 surface at 1.5×) |
| Engine | the sample document loads, lays out and draws (41 paragraphs); a saved `.docx` reopens after a restart |
| System fonts | one folder reported by `font.getUIFontConfig()`, 696 faces catalogued |
| Frames | 8756 triangles, ~14.9M painted pixels, 9 textures; ui 1.0 ms, tessellate 0.3 ms, paint 50.4 ms per painted frame (~5 fps at 1920×2799 — the rasteriser is the bottleneck) |
| File picker | Ctrl+O opened `DocumentViewPicker`; a picked `.md` became a working copy; Save/Save As wrote an 11 KB `.docx` into *Documents* that reopened after a restart |
| Clipboard | Ctrl+C wrote plain text and HTML (`clipboard written (25 text, 68 html chars)`); Ctrl+V read them back (`clipboard read: 2 record(s) … text/html,text/plain`) and pasted the text |
| Shortcut text | Ctrl+S used to type `K` (ArkUI's `keyText` is unreliable while Ctrl is held); the character now travels with the key and a Ctrl/Meta chord types nothing |

Three things only a device showed, all handled:

- **`BufferHandle.virAddr` is null** on this emulator. The documented CPU path is to `mmap` the
  buffer's file descriptor for the frame and `munmap` it before `FlushBuffer`; that is what
  `present.cpp` does.
- **A different signature cannot replace an installed app.** Installing over a build signed with
  another certificate fails with `9568332 install sign info inconsistent`; `hdc uninstall
  karl.hmsoft.wordcraft` first. (DevEco's *Run* installed one such build; the CLI install then
  needed the uninstall.)
- **`keyText` is not the key's character while a modifier is held**: Ctrl+S arrives with the text
  `K`. Rust decides what types, not the platform (see *Platform notes*, Keys).

### Verified on a real tablet

HUAWEI BKY-W00, HarmonyOS 6.1.0.135 (API 24), 1840×2650 at 2.25×, 703 system font faces,
`karl.hmsoft.wordcraft` installed with DevEco's debug signature:

| What | Result |
|---|---|
| The interface | the ribbon in Chinese at 2.25×, the ruler, the sample document with its table and lists, the status bar |
| Frames | 7172 triangles, 3.7M/17.0M px blitted, 9 textures; ui 1.2 ms, tessellate 0.2 ms, paint 46.2 ms — the same wall as the emulator: the rasteriser, not the CPU class |
| File picker | Ctrl+O opened `选择文件`; Ctrl+S opened `选择路径` on the user's own folder list |
| Save | saved as `WordCraft-name-check.docx` in 个人: the working copy took the typed name, and 10,593 bytes were copied back over the picked URI |
| Clipboard | Ctrl+C wrote 2021 text chars + 4010 HTML chars; Ctrl+V read them back (`2 record(s) … text/html,text/plain`) |
| Clipboard from another app | copied 236 chars in 备忘录 and pasted them into WordCraft: `clipboard read: 1 record(s), 236 text chars, text/plain` — **no `READ_PASTEBOARD` grant was requested**, so HarmonyOS 6.1 allowed it (the permission is what the API documents; check it again on release firmware) |
| Keys | an injected F11 arrived with the text `K`, and Ctrl+A/Ctrl+S likewise: only keys that can type now carry a character |

A real device adds one finding the emulator hid: **`keyText` is unreliable for injected and
chorded keys in general**, not only for modifiers — an F11 carried the text `K`. Text is accepted
only from key codes that type (letters, digits, typing punctuation, space) and never from a
Ctrl/Meta chord; input-method text has its own entry, so nothing is lost.

Two things only a device shows, both handled:

- **`BufferHandle.virAddr` is null** on this emulator. The documented CPU path is to `mmap` the
  buffer's file descriptor for the frame and `munmap` it before `FlushBuffer`; that is what
  `present.cpp` does now.
- **A different signature cannot replace an installed app.** Installing over a build signed with
  another certificate fails with `9568332 install sign info inconsistent`; `hdc uninstall
  karl.hmsoft.wordcraft` first. (DevEco's *Run* installed one such build; the CLI install then
  needed the uninstall.)

## Platform notes

- **Fonts.** Bundled OFL families (`assets/fonts/`) always load. System fonts come from
  `/system/fonts` plus the folders the UI framework reports
  (`font.getUIFontConfig().fontDir`), passed to `wordcraft_fonts::set_system_font_dirs` at
  start-up. Word's font substitution (`crates/fonts/src/word.rs`) maps Calibri/Aptos and friends
  to what the device has.
- **Settings and logs.** `wordcraft_control_key::set_settings_dir(context.filesDir + "/settings")`
  at start-up; `WORDCRAFT_PROFILE_DIR` still overrides it for isolated runs.
- **Files.** User documents come from `DocumentViewPicker` and are edited through working copies
  in `filesDir/documents/` (see *Files: the picker grants a file, not its folder*). The engine
  therefore reads and writes ordinary sandbox paths, so saving, autosave and crash recovery
  (`crates/engine/src/recovery/desktop.rs`) work as on Linux. The grant lasts for the session;
  a document kept for the next start would need `fileshare.persistPermission` on its URI.
- **Keys.** The character a key produced travels with the key (`wc_key(…, text)`), and Rust
  decides whether it types: a Ctrl or Meta chord never does. ArkUI's `keyText` cannot be trusted
  while a modifier is held (Ctrl+S arrived as `K`), and a shortcut that types would corrupt the
  document. Input-method text has its own entry (`wc_text`).
- **Rendering.** Pages are rasterised on the CPU by `vello_cpu` (`crates/render`); the shell
  composites egui's triangles on the CPU as well and hands finished frames to the native window
  buffer (`OH_NativeWindow_NativeWindowRequestBuffer` → copy → `FlushBuffer`). No GPU context is
  created, so a driver bug can't take the editor down.
- **`unsafe`.** Every NAPI and NDK pointer stays in the small C++ layer
  (`ohos/entry/src/main/cpp`). Rust has one exception: exporting a symbol C can find needs
  `#[unsafe(no_mangle)]`, so `apps/wordcraft-ohos` sets `unsafe_code = "deny"` and only its
  `src/abi.rs` opts out. One export there borrows the shell's answer string (`wc_host_answer`,
  `unsafe` for that reason, length checked, never kept); nothing else dereferences a pointer.
  `crates/ohos-app` (all the logic) keeps `unsafe_code = "forbid"`.

## Known gaps (planned, tracked in [gaps.md](gaps.md))

- Frame rate: about 5 fps at 1920×2799 (paint 50.4 ms of a painted frame). The software
  rasteriser is the bottleneck; row-wise fills, a 1:1 blit for page bitmaps and dirty rectangles
  are the next work.
- Input method: no `inputMethod.attach` yet, so the soft keyboard and the Chinese IME do not
  reach the document (OHOS.5). Hardware keys work.
- Clipboard pictures: the read path (packed PNG) and the paste path are unit-tested, but no device
  test has pasted a copied picture yet (a copy from 图库 or a file manager would exercise it).
  Reading another app's text worked without the restricted `ohos.permission.READ_PASTEBOARD` on
  the test tablet; confirm that on release firmware.
- Pictures dragged from the file manager paste as text (the URIs reach the engine as plain text;
  the picture-file paste path exists for the desktop).
- Recent files, autosave and recovery record sandbox paths, not the user's folders; the file name
  is right, the location is the app's. Persisting the URI (`fileshare.persistPermission`) is part
  of OHOS.6.
- Printing: no `craft-print` port; the print UI hides itself (no `print_native` service).
- Read Aloud: no TTS hook yet.
- Zotero: needs the Zotero desktop app, which doesn't exist on HarmonyOS.
- Phone-sized layout: the ribbon collapses, but a phone-first layout is later work.
- No GPU renderer; very large windows on high-DPI screens are CPU-bound.

## Revision history

| Date | Kind | Change |
|---|---|---|
| 2026-10-22 | minor | A real tablet (BKY-W00, HarmonyOS 6.1.0, 2.25×): the UI, the picker and the clipboard verified there; a paste of another app's copy worked without `READ_PASTEBOARD`; only keys that can type carry a character (an injected F11 brought `K`); the working copy takes the file name the user typed |
| 2026-10-22 | minor | Platform services: the system file picker and the clipboard through a Rust→ArkTS request bridge; documents are opened and saved through sandbox working copies because a picker grants a file and not its folder (a save's sibling temporary file is refused, `13900012`), with the finished file copied back over the picked URI; open → edit → save → reopen and copy/paste verified on the emulator; Ctrl+S no longer types `K` |
| 2026-10-21 | minor | Chinese interface on the device: HarmonyOS ships `HarmonyOS_Sans_SC/TC.ttf`, not Noto or Source Han, so the interface CJK fallback found nothing and every label drew as a box; the kana in the Translate icon falls back to Inter's `α` where no face has kana, guarded by a test that sweeps every ribbon icon's characters |
| 2026-10-21 | minor | First frame verified on the emulator: RGBA order, stride, density, input and the engine all correct; `mmap` needed because `virAddr` is null; a changed signature needs an uninstall first |
| 2026-10-21 | trivial | Signing: `sign-debug.sh` (own material, or the SDK's OpenHarmony test chain), what a HarmonyOS image refuses, and the UDID a debug profile is bound to |
| 2026-10-21 | minor | Step 2: HAP project, C++ shell (NAPI, native window buffer, hilog), Rust app with configuration and input, test pattern; unsigned HAP builds for arm64 and x86_64 |
| 2026-10-21 | major | First version: `cargo xtask ohos` for both targets, font/settings injection points, upstream-footprint policy |
