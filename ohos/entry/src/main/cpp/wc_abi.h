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
/* A key, with the character it produced (0 for none), as a Unicode scalar value. */
void wc_key(int32_t code, int32_t pressed, int32_t repeated, int32_t modifiers, uint32_t text);
void wc_text(uint32_t codepoint);
void wc_ime(int32_t composing);

/* --- the text caret: where the input method's candidate window belongs ---------------------- */

/* 1 when the document is editing text (a caret is on screen), 0 otherwise. */
int32_t wc_caret_present(void);
/* The caret's x and top y in points, and the height of its line (all 0 when there is no caret). */
float wc_caret_x(void);
float wc_caret_y(void);
float wc_caret_height(void);
void wc_focus(int32_t focused);

/* --- frames -------------------------------------------------------------- */

/* 1 when there is a frame to present, 0 when there is no surface (or one too large to be real). */
int32_t wc_render(void);
/* RGBA8888; wc_pixels_len() is wc_stride() * wc_height(). Valid until the next wc_* call. */
const uint8_t *wc_pixels(void);
size_t wc_pixels_len(void);
size_t wc_stride(void);

/* --- diagnostics --------------------------------------------------------- */

/* The part of the frame that changed since the previous one, in pixels. Copy and flush only this
 * region; a width or height of zero means nothing changed since the last frame was presented. */
uint32_t wc_damage_x(void);
uint32_t wc_damage_y(void);
uint32_t wc_damage_width(void);
uint32_t wc_damage_height(void);

/* --- the host bridge: Rust asks, ArkTS answers -------------------------------------------- */

/* The next request for ArkTS (UTF-8 JSON), or null. Call wc_host_take_len() *after* this; the bytes
 * stay valid until the next wc_host_take(). */
const uint8_t *wc_host_take(void);
size_t wc_host_take_len(void);
/* ArkTS's answer: a UTF-8 JSON object, valid for this call only. */
void wc_host_answer(uint64_t id, const uint8_t *json, size_t len);

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
