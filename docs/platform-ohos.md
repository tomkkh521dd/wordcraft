# HarmonyOS (鸿蒙) platform

> **Last reviewed:** 2026-10-22 · **Last updated:** 2026-10-22 · **Change:** minor (device reports: resize flicker fixed, the composing field turned off (it cancelled the IME's composition), a copied picture read through its URI; previously minor (the keyboard now types into a field on the caret, so 拼音 composition is drawn in the page; previously minor (File › Share opens the system share sheet; previously minor ("Open with" from the file manager, and a verified copy-out that repairs a save killed mid-write on the next start; previously minor (files survive a restart: persisted file grants, the sandbox ↔ URI table and reopening the last document; previously minor (a selection handle under the caret that extends the selection from the far end, plus Back closing the keyboard; previously minor (touch first: pinch zoom, two-finger scroll with momentum and a long-press context menu, read from the touch stream because the platform's recognisers hide it from the page; previously trivial (rasteriser: whole-row 1:1 blits and per-row edge stepping — a full frame paints in 36.8 ms on the tablet instead of 46.2, with `cargo run -p wordcraft-ui-paint --release --example bench` printing the cases; previously minor: OHOS.5b: IME commits are one undo step and preview text is drawn at the caret when an IME sends it — the Huawei IME does not, so composing stays in its bubble; previously minor: OHOS.5a: the system input method is bound to the self-drawn canvas — soft keyboard, English and 拼音 text, Backspace/Enter/arrows, candidate window at the caret; previously minor: a real HarmonyOS 6.1 tablet: the UI at 2.25×, the picker and the clipboard verified there too, including a paste of another app's copy without the restricted clipboard permission; only keys that can type carry a character, since injected keys bring garbage `keyText`; the working copy takes the name the user typed; previously minor: the real UI runs on the device, the CJK interface face and the icon glyphs are fixed, and the frame rate is the open problem; previously minor: first frame verified on the emulator: format, stride, density, input, engine; `mmap` for the buffer, and the uninstall a changed signature needs; previously trivial: signing; previously minor: step 2, HAP project, C++ shell and test pattern; previously major: first version) · **Target:** HarmonyOS NEXT 5.1+ (API 24, built against API 26), PC/2in1 and tablet

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
| Platform services: file picker (open, save), clipboard (text, HTML, pictures) | ✅ done and verified on the emulator and a tablet |
| Files across restarts: persisted grants (`FILE_ACCESS_PERSIST`), the sandbox ↔ URI table, reopening the last document, write-back compensation | ✅ verified on a tablet (OHOS.9a/b) |
| "Open with" from the file manager (`.docx/.doc/.odt/.rtf/.md/.txt`) | ✅ verified on a tablet (OHOS.9b) |
| System share (File › Share ▸ Share Kit) | ✅ verified on a tablet (OHOS.9c) |
| Touch: pinch zoom, two-finger scroll with momentum, long-press context menu, drag selection, selection handle | ✅ (OHOS.8a/b): pinch scales the page (`view.zoom`, 10–500%), two-finger pan scrolls and flings, a still finger opens the document's context menu, and a draggable handle at the selection's end extends it with edge auto-scroll; the *anchor* handle is next |
| Input method | ✅ `inputMethod.attach` (commit, delete, Enter, cursor moves, candidate window at the caret). The composing-field variant is written but **off** (`USE_IME_FIELD = false`): with the Huawei IME a field reset after each change cancels the composition, so 拼音 and 五笔 keys stop committing |
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
| `crates/ui-egui/src/lib.rs` | `Services::share_file` (one field) and `file.share` using it when set | Only a host knows whether the platform has a share sheet |
| `crates/ui-egui/src/ribbon.rs` | the 共享 button runs `file.share` when `share_file` is set, else opens Export | the button is UI, not the command |

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
| Frames | 7172 triangles, 3.7M/17.0M px blitted, 9 textures; ui 1.2 ms, tessellate 0.2 ms, paint 46.2 ms before the rasteriser work below and 36.8 ms after it — the same wall as the emulator: the rasteriser, not the CPU class |
| File picker | Ctrl+O opened `选择文件`; Ctrl+S opened `选择路径` on the user's own folder list |
| Save | saved as `WordCraft-name-check.docx` in 个人: the working copy took the typed name, and 10,593 bytes were copied back over the picked URI |
| Clipboard | Ctrl+C wrote 2021 text chars + 4010 HTML chars; Ctrl+V read them back (`2 record(s) … text/html,text/plain`) |
| Clipboard from another app | copied 236 chars in 备忘录 and pasted them into WordCraft: `clipboard read: 1 record(s), 236 text chars, text/plain` — **no `READ_PASTEBOARD` grant was requested**, so HarmonyOS 6.1 allowed it (the permission is what the API documents; check it again on release firmware) |
| Keys | an injected F11 arrived with the text `K`, and Ctrl+A/Ctrl+S likewise: only keys that can type now carry a character |
| Input method | a tap in the document attached the platform input method at the caret (`input method attached at (869, 1223)`), the soft keyboard came up and the window re-laid out above it |
| English typing | tapping the keyboard's keys inserted `Gekko` into the document through `insertText` |
| Chinese typing | switched the keyboard to 拼音, typed `ni` → candidates 呢 拟 你 …, chose 呢: the character went into the document through the same `insertText` |
| IME keys | Backspace arrived as `deleteLeft` and inserted two `2055` key presses; `完成` (Done) arrived as `sendFunctionKey` and inserted `Enter` (`2054`) |
| IME commit | every commit arrives as one `insertText` and becomes one `ImeEvent::Commit`: typing `M`, `o` and 拼音 `mo` → 么 logged `ime commit "M"`, `"o"`, `"么"` and the document reads `Why share a studio?Mo 么` — one undo step per commit, not one per character |
| IME composing text | typing 拼音 `mo` showed the candidates 么 摸 万 … and the letters only in the keyboard's own bubble: `setPreviewText` never fired (subscribed before `attach`, as the API requires) |
| IME onto the caret (OHOS.10a, now off) | a tap put a transparent `TextInput` on the caret and the keyboard opened from it; each key produced one `ime commit "i"`, `⌫` produced `key 2055` (sentinel deleted), `完成` produced `key 2054` — but on a real 拼音/五笔 keyboard nothing committed, so the field is disabled |
| Flicker on touch | reported on the device: a tap flashed part of the window. Cause: a surface resize (the keyboard opening) reallocated the frame while the damage pass still compared against the previous frame's meshes, so the first frame after a resize painted only the changed part onto a zeroed buffer. `resize` now invalidates the previous frame |
| Copied picture (file manager → paste) | the clipboard read returns `text/uri`, the page copies the picture into its cache **through the URI** (`copied a clipboard picture to …/pasted-….png`) and reports that path as clipboard text; the paste still inserts nothing — the remaining gap is in the app's paste command, not in the clipboard |
| Long press | a still finger for 600 ms opened the document's context menu (`pointer down secondary` at the finger, menu with 剪切/复制/粘贴/字体…); the trailing lift is swallowed so the menu stays open |
| Drag selection | `uitest swipe` (which moves like a finger) reached the page as 33 pointer moves and selected the heading under it, with the mini toolbar above |
| Injected `uitest drag` | sends no intermediate moves on this firmware, so it looks like a 900 ms long press; use `swipe` for scripted drags |
| Selection handle | a blue dot under the caret, redrawn from the page every frame; pressing it with Shift and dragging extended the collapsed caret into the whole paragraph under the finger, and the handle moved to the new end |
| Back | the first Back closes the soft keyboard and stays in the app (it used to leave the app) |
| Save survives a restart | saved `WordCraft-resume-test.docx` (10,594 bytes copied out, `kept the grant for the picked file`); force-stop, relaunch → `re-activated 1 persisted file grant(s)`, `last document: …documents/1/WordCraft-resume-test.docx`, `opening … at launch`; a bold change and Ctrl+S then wrote **10,600 bytes to the user's own file** |
| "Open with" plumbing | `aa start … -U file://docs/…docx` reached the page (`asked to open …`) and became a working copy; reading it failed with `13900002` because an injected Want carries no grant — a file manager launch does |
| "Open with" from Files | long-pressed the `.docx` in 文件管理 → 打开方式 listed **WordCraft**; choosing it produced `asked to open file://docs/…/WordCraft-resume-test.docx`, a working copy in `documents/2/`, and `opening … at launch` |
| System share | pressing 共享 in the ribbon opened HarmonyOS's share sheet with `WordCraft-resume-test.docx · 10.6 KB` and its targets (华为分享, 备忘录, 邮件, 复制, 另存为 …); `share sheet shown for WordCraft-resume-test.docx` |
| Write-back compensation | with the copy-out timer parked (a debug build) a save was left in the sandbox; the next start logged `wrote the document back … 10602 bytes` and `wrote back 1 document(s) that a previous run did not` **before** reopening the document |

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
- **Share.** File › Share (`file.share`) goes through a new `Services::share_file` hook: the shell is
  handed the document's path, copies any pending change out first, and hands the **user's own file
  URI** (or the sandbox path as a file URI when the document never came from a picker) to Share Kit
  (`systemShare.SharedData` + `ShareController.show`). The ribbon's 共享 button runs `file.share`
  when that hook is set and keeps opening the Export page otherwise, so the desktop is unchanged.
- **Write-back safety.** A copy-out is written, then the picked file's size is checked against the copy's;
  on a mismatch it is written once more, and only a verified copy updates the `synced` marker (which
  is persisted). On the next start, any copy newer than its marker is written back before the
  document is reopened — a kill in the half second after a save can no longer leave the user's file
  older than what they saw.
- **Files across restarts.** A picked file's grant is temporary: it is gone after a restart. Each
  pick therefore asks HarmonyOS to keep it (`fileShare.persistPermission`, `READ|WRITE`, behind
  `ohos.permission.FILE_ACCESS_PERSIST` — normal and system-granted, though AppGallery wants the
  restricted-permission declaration at release), the sandbox ↔ URI table and the open document go
  into `preferences`, and the next launch re-activates the grants (`fileShare.activatePermission`)
  before the page asks the app to open the remembered document (the page calls `wc_ready` only
  after that state is loaded, or a cold start would race the answer and open nothing). Without that, a save after a
  restart wrote only the sandbox copy and the user's file silently went stale.
- **Files.** User documents come from `DocumentViewPicker` and are edited through working copies
  in `filesDir/documents/` (see *Files: the picker grants a file, not its folder*). The engine
  therefore reads and writes ordinary sandbox paths, so saving, autosave and crash recovery
  (`crates/engine/src/recovery/desktop.rs`) work as on Linux. The grant lasts for the session;
  a document kept for the next start would need `fileshare.persistPermission` on its URI.
- **Keys.** The character a key produced travels with the key (`wc_key(…, text)`), and Rust
  decides whether it types: a Ctrl or Meta chord never does, and only key codes that can type
  carry a character at all. ArkUI's `keyText` cannot be trusted for chords or injected keys
  (Ctrl+S and an injected F11 both arrived as `K`), and a shortcut that types would corrupt the
  document. Input-method text has its own entry (`wc_text`).
- **Selection handle.** The page exposes the caret in points (`wc_caret_x/y/height`) and whether the
  selection is a range (`wc_selection_range`); ArkTS keeps a 44 vp hit area with an 18 vp dot there,
  refreshed every frame so it follows typing, scrolling and menu changes. Dragging it sends a
  **Shift** pointer press at the handle and then moves, which the page reads as "extend the
  selection to here" with the anchor kept at the other end — Word's handle behaviour without a new
  selection API. Within 40 vp of the page's top or bottom it scrolls per move event, so a selection
  can run past the screen. Note that a component's `TouchObject.x/y` are local to that component;
  the handle must use `windowX/windowY` to speak the page's coordinates.
- **Back.** The page's `onBackPress` closes the soft keyboard first and only lets the system leave
  the app when there is none (before, the first Back left the app from under a menu).
- **Touch.** Pinch, two-finger scroll and the long-press menu are read from `TouchEvent.touches`
  rather than the platform's `PinchGesture`/`PanGesture`: once one of those recognisers is attached
  to the `XComponent`, ArkUI withholds the single-finger stream from `onTouch` while it decides,
  and a slow drag then looks exactly like a press (measured: the context menu opened in the middle
  of a selection drag, and the drag never reached the page). Two fingers cancel whatever the first
  one started (`POINTER_CANCEL`) and drive zoom (`wc_zoom` → `view.zoom`) and scroll/`wc_scroll_fling`
  (velocity decays per frame in Rust, and a touch stops it). The long press is a 600 ms timer on the
  same single-finger stream, so a drag cancels it the moment it moves 8 vp.
- **Input method (composing field).** A self-drawn client gets no preview text from the Huawei IME, so
  the composing letters would only ever appear in the keyboard's own bubble. The page therefore
  keeps a one-line `TextInput` **on the caret** — transparent background, the page's text colour,
  no caret of its own, `enablePreviewText(true)` — and focuses it when a tap leaves a caret. The
  platform draws the composition there (in the page, underlined) and hands over committed text;
  the field holds a zero-width sentinel so `⌫` has something to delete (an empty field swallows
  the key and the document never hears about it), a two-way `$$` binding keeps it empty after
  every commit, `onSubmit` forwards Enter, and `onKeyEvent` forwards hardware keys so the app's
  shortcuts keep working. Three device findings are behind that shape: focusing a component the
  same frame mounts it (error 150003) takes the app down unless the focus is deferred; a one-way
  `text` binding kept the typed characters, so every commit re-sent the whole string; and
  `backgroundColor(0x00000000)` painted an opaque black bar over the page.
- **Input method.** The document is a self-drawn canvas, so the platform's text components never
  see a tap and no keyboard would appear. ArkTS binds the canvas to the input method itself
  (`inputMethod.getController().attach(...)`) whenever a tap leaves a caret (`wc_caret_present`,
  from `canvas::caret_screen`), and passes the caret in pixels (`wc_caret_x/y/height`) so the
  candidate window sits where the text goes. While attached it listens for `insertText` (one
  `wc_text` per character), `deleteLeft`/`deleteRight` (Backspace and Delete presses),
  `sendFunctionKey` (Enter) and `moveCursor` (arrow keys), re-syncs the cursor after every insert,
  and detaches when the caret goes or the window loses focus. A commit becomes one
  `ImeEvent::Commit` (one undo step), and preview text becomes `ImeEvent::Preedit`, which the
  desktop's IME path already draws at the caret. On the 6.1.0 device the Huawei IME **never sent**
  preview text to this self-drawn client (the subscription is made before `attach`, as the API
  requires), so the composing letters are only visible in the keyboard's own bubble; an inline
  composition would need the fallback of a real `TextInput` overlay at the caret.
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

- Frame rate: paint 36.8 ms for a full 1840×2650 frame on the tablet (was 46 ms). The 1:1 page
  blit now copies whole texture rows (12.6 → 4.2 ms in the bench) and the general triangle path
  steps its edge functions along a row instead of recomputing them per pixel (glyph quads 50 → 38
  ms). Text sampling (bilinear per glyph pixel) and repainting only the damaged part of a frame
  are the next work; `cargo run -p wordcraft-ui-paint --release --example bench` prints the
  per-case numbers.
- Files, what is still missing: clipboard pictures on a device (the Rust side is unit-tested, the
  `packPicture` step has only been exercised in code), sharing an exported PDF rather than the
  document file, and Recent/autosave naming the user's folder rather than the sandbox path.
- Touch, what is still missing: the *anchor* end's handle (only the end being edited has one, so
  extending backwards means re-dragging), a magnifier while dragging, a pinch that keeps the point
  under the fingers fixed (the view zooms around its own centre), momentum friction tuned on
  hardware, and a Back key that also closes an open menu (it closes the keyboard first now).
- Input method, what is still missing: inline composition (the composing field is written but off —
  resetting it mid-composition cancels the IME's composition, so it needs `onChange`'s `previewText`
  to tell composition from committed text); selection sync (`changeSelection` /
  `getLeftTextOfCursor`), so IME-driven select-all, cut and copy have nothing to act on.
- Clipboard pictures: a picture copied in the file manager is read through its URI into the app's
  cache (the path under `/storage/Users` is not readable), and reported to the paste as a picture
  path, but the paste inserts nothing yet — the next step is to follow `system_clipboard::fill` or
  to hand the paste an `Event::Paste` with the paths.
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
| 2026-10-22 | minor | OHOS.10b, from device reports: a resize (the keyboard opening) now invalidates the previous frame, so a tap no longer flashes a half-painted buffer; the composing field is turned off again because a reset field cancels the Huawei IME's composition (拼音 and 五笔 stopped committing), leaving `inputMethod.attach` as the working path; a copied picture is read through its URI into the app cache; the clipboard is read when the page comes to the front, before the copying app's grant lapses |
| 2026-10-22 | minor | OHOS.10a: the keyboard types into a transparent `TextInput` placed on the caret, so the platform draws 拼音 composition inside the page; commits are one character each, `⌫` works through a zero-width sentinel, Enter and hardware keys are forwarded. Three device traps recorded: focusing a not-yet-mounted component (150003) killed the app, a one-way `text` binding accumulated every commit, and a numeric colour painted an opaque bar |
| 2026-10-22 | minor | OHOS.9c system share: File › Share hands the document to HarmonyOS's share sheet (`Services::share_file` → Share Kit), and the ribbon's 共享 button uses it when a host provides one; verified on the tablet with the file name, size and targets in the sheet |
| 2026-10-22 | minor | OHOS.9b: "Open with" declared for `.docx/.doc/.odt/.rtf/.md/.txt` (the file manager lists WordCraft; choosing it opened the document), and a copy-out is now verified (size checked, one retry) with the `synced` marker persisted — a copy left behind by a kill is written back at the next start, before the document reopens |
| 2026-10-22 | minor | OHOS.9a file safety: a picked file's grant is persisted (`FILE_ACCESS_PERSIST`) and re-activated on the next launch, the sandbox ↔ URI table and the open document are kept in `preferences`, and the app reopens the document it had — so a save after a restart reaches the user's own file (verified: 10,600 bytes written back). "Open with" arrives as a `Want` URI and becomes a working copy; the manifest association is still to come |
| 2026-10-22 | minor | OHOS.8b selection handle: a dot under the caret (18 vp on a 44 vp hit area), refreshed every frame, dragged with a Shift press so the anchor stays put, scrolling the page near its edges; `wc_selection_range` tells a caret from a selection. Also: the first Back now closes the keyboard instead of leaving the app |
| 2026-10-22 | minor | OHOS.8a touch first: a two-finger pinch zooms the page (10–500%, one `view.zoom` step per gesture update), a two-finger pan scrolls with per-frame momentum, and a still finger opens the document's context menu; the platform's own gesture recognisers are not used, because they withhold the single-finger stream and a slow drag then reads as a long press (measured on the tablet) |
| 2026-10-22 | trivial | Rasteriser: the 1:1 blit copies whole texture rows when the source is contiguous (12.6 → 4.2 ms per 4.9 MP frame in the new `crates/ui-paint/examples/bench.rs`), and the general triangle path evaluates each edge function once per row and steps it per pixel (8000 glyph quads 50 → 38 ms); a full UI frame on the tablet paints in 36.8 ms instead of 46.2 |
| 2026-10-22 | minor | OHOS.5b: IME text arrives as one `ImeEvent::Commit` per commit (one undo step) and preview text as `ImeEvent::Preedit`, drawn at the caret by the desktop path; the Huawei IME on 6.1.0 sends no preview text to self-drawn clients, so composing letters stay in the keyboard's bubble |
| 2026-10-22 | minor | OHOS.5a: the system input method is bound to the self-drawn canvas — a tap brings up the soft keyboard at the caret, English and Chinese (拼音 candidate) text land in the document, Backspace/Delete/Enter/arrow moves are wired, the candidate window follows the caret; composing text is not yet drawn inline |
| 2026-10-22 | minor | A real tablet (BKY-W00, HarmonyOS 6.1.0, 2.25×): the UI, the picker and the clipboard verified there; a paste of another app's copy worked without `READ_PASTEBOARD`; only keys that can type carry a character (an injected F11 brought `K`); the working copy takes the file name the user typed |
| 2026-10-22 | minor | Platform services: the system file picker and the clipboard through a Rust→ArkTS request bridge; documents are opened and saved through sandbox working copies because a picker grants a file and not its folder (a save's sibling temporary file is refused, `13900012`), with the finished file copied back over the picked URI; open → edit → save → reopen and copy/paste verified on the emulator; Ctrl+S no longer types `K` |
| 2026-10-21 | minor | Chinese interface on the device: HarmonyOS ships `HarmonyOS_Sans_SC/TC.ttf`, not Noto or Source Han, so the interface CJK fallback found nothing and every label drew as a box; the kana in the Translate icon falls back to Inter's `α` where no face has kana, guarded by a test that sweeps every ribbon icon's characters |
| 2026-10-21 | minor | First frame verified on the emulator: RGBA order, stride, density, input and the engine all correct; `mmap` needed because `virAddr` is null; a changed signature needs an uninstall first |
| 2026-10-21 | trivial | Signing: `sign-debug.sh` (own material, or the SDK's OpenHarmony test chain), what a HarmonyOS image refuses, and the UDID a debug profile is bound to |
| 2026-10-21 | minor | Step 2: HAP project, C++ shell (NAPI, native window buffer, hilog), Rust app with configuration and input, test pattern; unsigned HAP builds for arm64 and x86_64 |
| 2026-10-21 | major | First version: `cargo xtask ohos` for both targets, font/settings injection points, upstream-footprint policy |
