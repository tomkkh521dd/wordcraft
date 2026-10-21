//! `wc_*`: the functions `ohos/entry/src/main/cpp/wc_abi.h` declares.
//!
//! Every function is a thin translation of values onto [`App`]. Nothing here touches a pointer:
//! the only exports that mention one return the pixel buffer's address, which is safe to create
//! and is the shell's to copy immediately (see the header's contract).
//!
//! # Threads
//!
//! The UI is not `Send` (egui's state and `wordcraft-ui-egui`'s services hold `Rc`s), and it does
//! not need to be: ArkTS runs the NAPI callbacks and the `displaySync` frame callback on its UI
//! thread. The app therefore lives in a thread-local, and a call from any other thread is logged
//! and ignored instead of panicking — a wrong thread is a bug in the shell, not a reason to lose
//! someone's document.
#![allow(unsafe_code)]

use std::cell::RefCell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::ThreadId;

use wordcraft_ohos_app::logging::Sink;
use wordcraft_ohos_app::{App, Button, FrameNeeded, KeyEvent, PointerKind, Scroll};

thread_local! {
    /// The app, owned by the thread that created it (the ArkTS UI thread).
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// The thread that created the app; every other call is refused.
static OWNER: OnceLock<ThreadId> = OnceLock::new();
/// Set once, so a wrong-thread call is reported once rather than every frame.
static WARNED: AtomicBool = AtomicBool::new(false);

/// Run `f` on the app when this is its thread and it exists; `default` otherwise.
fn with<R>(default: R, f: impl FnOnce(&mut App) -> R) -> R {
    let me = std::thread::current().id();
    match OWNER.get() {
        Some(owner) if *owner == me => {}
        Some(owner) => {
            if !WARNED.swap(true, Ordering::Relaxed) {
                log::error!("wc_* called from {me:?} but the app belongs to {owner:?}: ignored");
            }
            return default;
        }
        None => return default,
    }
    // A re-entrant call (a log message from inside a frame) would find the cell borrowed: answer
    // with the default rather than panicking.
    let mut f = Some(f);
    APP.try_with(|cell| {
        let mut slot = cell.try_borrow_mut().ok()?;
        let app = slot.as_mut()?;
        f.take().map(|f| f(app))
    })
    .ok()
    .flatten()
    .unwrap_or(default)
}

/// Create the app and open the document the environment asks for. `0` when running (a second call
/// keeps the existing document), `-1` when the state itself is unusable.
#[unsafe(no_mangle)]
pub extern "C" fn wc_app_new() -> i32 {
    let me = std::thread::current().id();
    if let Some(owner) = OWNER.get()
        && *owner != me
    {
        log::error!("wc_app_new called from {me:?} but the app belongs to {owner:?}: ignored");
        return -1;
    }
    let _ = OWNER.set(me);
    APP.try_with(|cell| {
        let Ok(mut slot) = cell.try_borrow_mut() else { return -1 };
        if slot.is_none() {
            *slot = Some(App::new());
        }
        0
    })
    .unwrap_or(-1)
}

/// Close the document and free everything the app holds.
#[unsafe(no_mangle)]
pub extern "C" fn wc_app_free() {
    let _ = APP.try_with(|cell| {
        if let Ok(mut slot) = cell.try_borrow_mut() {
            *slot = None;
        }
    });
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

/// Run one UI pass and paint it into the app's buffer. `1` when there is a frame to present, `0`
/// when there is no surface (or one too large to be real).
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
