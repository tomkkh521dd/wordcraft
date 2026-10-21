/*
 * The C ABI the C++ shell calls: implemented in Rust by apps/wordcraft-ohos (src/abi.rs).
 *
 * No pointer is handed *to* Rust: arguments cross by value and configuration arrives through the
 * environment (see wordcraft::Configure). Frames come back as a (pointer, length) pair, which the
 * shell copies into the window buffer immediately — the pointer is valid until the next wc_* call.
 */
#ifndef WORDCRAFT_WC_ABI_H
#define WORDCRAFT_WC_ABI_H

#include <cstddef>
#include <cstdint>

#ifdef __cplusplus
extern "C" {
#endif

/* The shell's logger: UTF-8 bytes, valid for this call only. */
typedef void (*WcLogSink)(const uint8_t *data, size_t len);

/* --- life cycle ---------------------------------------------------------- */

/* Create the app and open the document the environment asks for. 0 when running (a second call
 * keeps the document), -1 when the app state is unusable. */
int32_t wc_app_new(void);
void wc_app_free(void);
void wc_set_logger(WcLogSink sink);

/* --- surface ------------------------------------------------------------- */

/* Surface size in px (from onSurfaceCreated/onSurfaceChanged) and display density. */
void wc_resize(uint32_t width, uint32_t height, float density);
void wc_surface_gone(void);

/* --- input (all by value; coordinates in vp) ----------------------------- */

/* kind: 0 down, 1 up, 2 move, 3 leave, 4 cancel.
 * button: 0 primary, 1 secondary, 2 middle, other none. */
void wc_pointer(int32_t kind, float x, float y, int32_t button, int32_t modifiers);
void wc_scroll(float dx, float dy);
void wc_key(int32_t code, int32_t pressed, int32_t repeated, int32_t modifiers);
void wc_text(uint32_t codepoint);
void wc_ime(int32_t composing);
void wc_focus(int32_t focused);

/* --- frames -------------------------------------------------------------- */

/* 1 when there is a frame to present, 0 when there is no surface (or one too large to be real). */
int32_t wc_render(void);
/* RGBA8888; wc_pixels_len() is wc_stride() * wc_height(). Valid until the next wc_* call. */
const uint8_t *wc_pixels(void);
size_t wc_pixels_len(void);
size_t wc_stride(void);

/* --- diagnostics --------------------------------------------------------- */

uint32_t wc_width(void);
uint32_t wc_height(void);
float wc_density(void);
uint64_t wc_frames(void);
uint64_t wc_events(void);
uint64_t wc_paragraphs(void);
uint64_t wc_font_dirs(void);
int32_t wc_focused(void);
int32_t wc_pressed(void);

/* Modifier bits, as wc_pointer/wc_key take them (matches src/input.rs). */
#define WC_MOD_SHIFT 1
#define WC_MOD_CTRL 2
#define WC_MOD_ALT 4
#define WC_MOD_META 8
#define WC_MOD_CAPS_LOCK 16

#ifdef __cplusplus
}
#endif

#endif /* WORDCRAFT_WC_ABI_H */
