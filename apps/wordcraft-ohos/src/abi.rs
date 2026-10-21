//! `wc_*`: the functions `ohos/entry/src/main/cpp/wc_abi.h` declares.
//!
//! Every function is a thin translation of values onto [`App`]. Nothing here touches a pointer:
//! the only exports that mention one return the pixel buffer's address, which is safe to create
//! and is the shell's to copy immediately (see the header's contract).
#![allow(unsafe_code)]

use std::sync::{Mutex, OnceLock};

use wordcraft_ohos_app::logging::Sink;
use wordcraft_ohos_app::{App, Button, FrameNeeded, KeyEvent, PointerKind, Scroll};

/// The app, with the global the shell talks to. `None` before `wc_app_new` and after
/// `wc_app_free`.
static APP: OnceLock<Mutex<Option<App>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<App>> {
    APP.get_or_init(|| Mutex::new(None))
}

/// Run `f` against the app when there is one, `default` otherwise. A poisoned lock means an
/// earlier call panicked, so the app is treated as gone rather than panicking again.
fn with<R>(default: R, f: impl FnOnce(&mut App) -> R) -> R {
    let Ok(mut guard) = slot().lock() else {
        return default;
    };
    match guard.as_mut() {
        Some(app) => f(app),
        None => default,
    }
}

/// Create the app and open the document the environment asks for. `0` when running (a second call
/// keeps the existing document), `-1` when the state itself is unusable.
#[unsafe(no_mangle)]
pub extern "C" fn wc_app_new() -> i32 {
    let mut guard = match slot().lock() {
        Ok(guard) => guard,
        Err(_) => return -1,
    };
    if guard.is_none() {
        *guard = Some(App::new());
    }
    0
}

/// Close the document and free everything the app holds.
#[unsafe(no_mangle)]
pub extern "C" fn wc_app_free() {
    if let Ok(mut guard) = slot().lock() {
        *guard = None;
    }
}

/// Where `log` records go (hilog on device). `null` drops them.
#[unsafe(no_mangle)]
pub extern "C" fn wc_set_logger(sink: Option<Sink>) {
    wordcraft_ohos_app::logging::set_sink(sink);
}

/// The surface's size in pixels (from `onSurfaceCreated`/`onSurfaceChanged`) and the display
/// density (`display.getDefaultDisplaySync().densityPixels`).
#[unsafe(no_mangle)]
pub extern "C" fn wc_resize(width: u32, height: u32, density: f32) {
    with((), |app| app.resize(width, height, density));
}

/// The surface was destroyed; the next `wc_resize` starts a new one.
#[unsafe(no_mangle)]
pub extern "C" fn wc_surface_gone() {
    with((), |app| app.surface_gone());
}

#[unsafe(no_mangle)]
pub extern "C" fn wc_focus(focused: i32) {
    with((), |app| app.focus(focused != 0));
}

/// A pointer event. `kind`: 0 down, 1 up, 2 move, 3 leave, 4 cancel. `button`: 0 primary,
/// 1 secondary, 2 middle, other none. Coordinates are vp.
#[unsafe(no_mangle)]
pub extern "C" fn wc_pointer(kind: i32, x: f32, y: f32, button: i32, modifiers: i32) {
    let Some(kind) = PointerKind::from_int(kind) else {
        return;
    };
    with((), |app| app.pointer(kind, x, y, Button::from_int(button), modifiers));
}

/// A scroll gesture in vp (ArkUI's axis values), positive down and right.
#[unsafe(no_mangle)]
pub extern "C" fn wc_scroll(dx: f32, dy: f32) {
    with((), |app| app.scroll(Scroll { dx, dy }));
}

/// A hardware key. `code` is the platform's `KeyCode`; characters arrive through `wc_text`.
#[unsafe(no_mangle)]
pub extern "C" fn wc_key(code: i32, pressed: i32, repeated: i32, modifiers: i32) {
    with((), |app| app.key(KeyEvent { code, pressed: pressed != 0, repeated: repeated != 0, modifiers, text: None }));
}

/// One character of typing or input-method text (a Unicode scalar value).
#[unsafe(no_mangle)]
pub extern "C" fn wc_text(codepoint: u32) {
    if let Some(c) = char::from_u32(codepoint) {
        with((), |app| app.text(c));
    }
}

/// The input method started (`1`) or finished (`0`) composing.
#[unsafe(no_mangle)]
pub extern "C" fn wc_ime(composing: i32) {
    with((), |app| app.ime_composing(composing != 0));
}

/// Draw a frame into the app's buffer. `1` when there is something to present, `0` when there is
/// no surface (or one too large to be real).
#[unsafe(no_mangle)]
pub extern "C" fn wc_render() -> i32 {
    with(0, |app| if app.render() == FrameNeeded::Yes { 1 } else { 0 })
}

/// The frame's first byte. Valid until the next `wc_*` call: the shell copies it into the window
/// buffer straight away, then presents.
#[unsafe(no_mangle)]
pub extern "C" fn wc_pixels() -> *const u8 {
    with(std::ptr::null(), |app| app.pixels().as_ptr())
}

/// The frame's length in bytes (`stride * height`).
#[unsafe(no_mangle)]
pub extern "C" fn wc_pixels_len() -> usize {
    with(0, |app| app.pixels().len())
}

/// The frame's row length in bytes (RGBA8888, so `width * 4`).
#[unsafe(no_mangle)]
pub extern "C" fn wc_stride() -> usize {
    with(0, |app| app.stride())
}

#[unsafe(no_mangle)]
pub extern "C" fn wc_width() -> u32 {
    with(0, |app| app.width())
}

#[unsafe(no_mangle)]
pub extern "C" fn wc_height() -> u32 {
    with(0, |app| app.height())
}

#[unsafe(no_mangle)]
pub extern "C" fn wc_density() -> f32 {
    with(1.0, |app| app.density())
}

#[unsafe(no_mangle)]
pub extern "C" fn wc_frames() -> u64 {
    with(0, |app| app.frames())
}

#[unsafe(no_mangle)]
pub extern "C" fn wc_events() -> u64 {
    with(0, |app| app.events())
}

/// The open document's paragraph count (the engine runs on the device).
#[unsafe(no_mangle)]
pub extern "C" fn wc_paragraphs() -> u64 {
    with(0, |app| u64::try_from(app.paragraphs()).unwrap_or(u64::MAX))
}

/// How many system font folders the shell reported.
#[unsafe(no_mangle)]
pub extern "C" fn wc_font_dirs() -> u64 {
    with(0, |app| u64::try_from(app.font_dirs()).unwrap_or(u64::MAX))
}

#[unsafe(no_mangle)]
pub extern "C" fn wc_focused() -> i32 {
    with(0, |app| i32::from(app.focused()))
}

#[unsafe(no_mangle)]
pub extern "C" fn wc_pressed() -> i32 {
    with(0, |app| i32::from(app.pressed()))
}
