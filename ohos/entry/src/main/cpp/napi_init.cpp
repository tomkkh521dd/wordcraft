/*
 * The NAPI module ArkTS loads as `libwordcraft.so`.
 *
 * ArkTS owns what belongs to the UI framework — the XComponent and its surface lifecycle, input
 * events, the display, the system's fonts, language and colour mode — and passes the results here
 * as plain values. This file sets them up as environment variables, calls into the Rust app
 * (`wc_*`, see wc_abi.h) and presents the frames through the native window buffer queue.
 */

#include <napi/native_api.h>

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

#include "log.h"
#include "present.h"
#include "wc_abi.h"

namespace {

using wordcraft::Presenter;

Presenter g_presenter;
/* Frames presented, for the periodic log line. */
uint64_t g_presented = 0;

/* The frames the app draws are capped long before this; the clamp only keeps a bad value finite. */
uint32_t Dim(double v) {
    if (!std::isfinite(v) || v <= 0.0) {
        return 0;
    }
    return static_cast<uint32_t>(std::clamp(v, 0.0, 65535.0));
}

float Density(double v) {
    if (!std::isfinite(v) || v <= 0.0) {
        return 1.0f;
    }
    return static_cast<float>(std::clamp(v, 0.5, 8.0));
}

napi_value Number(napi_env env, double value) {
    napi_value out = nullptr;
    napi_create_double(env, value, &out);
    return out;
}

napi_value Undefined(napi_env env) {
    napi_value out = nullptr;
    napi_get_undefined(env, &out);
    return out;
}

napi_value Boolean(napi_env env, bool value) {
    napi_value out = nullptr;
    napi_get_boolean(env, value, &out);
    return out;
}

bool ArgCount(napi_env env, napi_callback_info info, napi_value *argv, size_t want) {
    size_t argc = want;
    if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
        WC_LOGE("cannot read the arguments");
        return false;
    }
    if (argc < want) {
        WC_LOGW("expected %{public}zu arguments, got %{public}zu", want, argc);
        return false;
    }
    return true;
}

bool ReadString(napi_env env, napi_value value, std::string &out) {
    size_t len = 0;
    if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok || len > 64 * 1024) {
        return false;
    }
    std::vector<char> buf(len + 1, '\0');
    if (napi_get_value_string_utf8(env, value, buf.data(), buf.size(), &len) != napi_ok) {
        return false;
    }
    out.assign(buf.data(), len);
    return true;
}

bool ReadDouble(napi_env env, napi_value value, double &out) {
    return napi_get_value_double(env, value, &out) == napi_ok;
}

bool ReadBool(napi_env env, napi_value value, bool &out) {
    return napi_get_value_bool(env, value, &out) == napi_ok;
}

bool ReadInt(napi_env env, napi_value value, int32_t &out) {
    return napi_get_value_int32(env, value, &out) == napi_ok;
}

/* The system's font folders, as the path list the Rust side splits. */
bool ReadFontDirs(napi_env env, napi_value value, std::string &joined) {
    bool isArray = false;
    if (napi_is_array(env, value, &isArray) != napi_ok || !isArray) {
        return false;
    }
    uint32_t length = 0;
    if (napi_get_array_length(env, value, &length) != napi_ok) {
        return false;
    }
    for (uint32_t i = 0; i < std::min(length, 64u); ++i) {
        napi_value item = nullptr;
        std::string one;
        if (napi_get_element(env, value, i, &item) != napi_ok || !ReadString(env, item, one)) {
            continue;
        }
        // ':' separates the list; a folder holding one cannot be expressed (and never occurs).
        if (one.empty() || one.find(':') != std::string::npos) {
            continue;
        }
        if (!joined.empty()) {
            joined.push_back(':');
        }
        joined += one;
    }
    return !joined.empty();
}

/* The first Unicode scalar value of a JS string, 0 when there is none (or it isn't printable). */
uint32_t FirstCodepoint(napi_env env, napi_value value) {
    size_t len = 0;
    if (napi_get_value_string_utf16(env, value, nullptr, 0, &len) != napi_ok || len == 0 || len > 16) {
        return 0;
    }
    std::vector<char16_t> buf(len + 1, 0);
    size_t copied = 0;
    if (napi_get_value_string_utf16(env, value, buf.data(), buf.size(), &copied) != napi_ok || copied == 0) {
        return 0;
    }
    char16_t first = buf[0];
    uint32_t cp = first;
    if (first >= 0xD800 && first <= 0xDBFF && copied >= 2) {
        char16_t second = buf[1];
        if (second >= 0xDC00 && second <= 0xDFFF) {
            cp = 0x10000u + ((static_cast<uint32_t>(first) - 0xD800u) << 10) + (static_cast<uint32_t>(second) - 0xDC00u);
        }
    }
    // Control characters are keys (Enter, Tab, Backspace), which the key event already carries.
    return cp >= 0x20 && cp != 0x7F ? cp : 0;
}

/* Where the Rust app's `log` records go. */
void RustLog(const uint8_t *data, size_t len) {
    if (data == nullptr || len == 0) {
        return;
    }
    std::string line(reinterpret_cast<const char *>(data), len);
    while (!line.empty() && (line.back() == '\n' || line.back() == '\r')) {
        line.pop_back();
    }
    OH_LOG_Print(LOG_APP, LOG_INFO, LOG_DOMAIN, LOG_TAG, "%{public}s", line.c_str());
}

/* init(settingsDir, tmpDir, fontDirs, locale, theme, start) -> int */
napi_value InitApp(napi_env env, napi_callback_info info) {
    napi_value argv[6];
    if (!ArgCount(env, info, argv, 6)) {
        return Number(env, -2.0);
    }
    std::string settings;
    std::string tmp;
    std::string fontDirs;
    std::string locale;
    std::string theme;
    std::string start;
    ReadString(env, argv[0], settings);
    ReadString(env, argv[1], tmp);
    ReadFontDirs(env, argv[2], fontDirs);
    ReadString(env, argv[3], locale);
    ReadString(env, argv[4], theme);
    ReadString(env, argv[5], start);

    if (!settings.empty()) {
        setenv("WORDCRAFT_OHOS_SETTINGS_DIR", settings.c_str(), 1);
    }
    if (!tmp.empty()) {
        setenv("TMPDIR", tmp.c_str(), 1);
    }
    if (!fontDirs.empty()) {
        setenv("WORDCRAFT_OHOS_FONT_DIRS", fontDirs.c_str(), 1);
    }
    if (!locale.empty()) {
        // sys-locale and the region detection read these; the app also gets the raw value.
        setenv("LANG", locale.c_str(), 1);
        setenv("LC_ALL", locale.c_str(), 1);
        setenv("WORDCRAFT_OHOS_LOCALE", locale.c_str(), 1);
    }
    setenv("WORDCRAFT_OHOS_THEME", theme.empty() ? "auto" : theme.c_str(), 1);
    setenv("WORDCRAFT_OHOS_START", start.empty() ? "sample" : start.c_str(), 1);

    wc_set_logger(&RustLog);
    int32_t rc = wc_app_new();
    WC_LOGI("app %{public}d: %{public}llu paragraph(s), %{public}llu font folder(s), locale %{public}s, theme %{public}s",
            rc, (unsigned long long)wc_paragraphs(), (unsigned long long)wc_font_dirs(), locale.c_str(), theme.c_str());
    return Number(env, rc);
}

/* surface(surfaceId, width, height, density) -> int */
napi_value Surface(napi_env env, napi_callback_info info) {
    napi_value argv[4];
    if (!ArgCount(env, info, argv, 4)) {
        return Number(env, -2.0);
    }
    uint64_t surfaceId = 0;
    bool lossless = false;
    if (napi_get_value_bigint_uint64(env, argv[0], &surfaceId, &lossless) != napi_ok) {
        WC_LOGE("surface: the id is not a bigint");
        return Number(env, -3.0);
    }
    double w = 0;
    double h = 0;
    double d = 1;
    ReadDouble(env, argv[1], w);
    ReadDouble(env, argv[2], h);
    ReadDouble(env, argv[3], d);
    int32_t rc = g_presenter.Attach(surfaceId, Dim(w), Dim(h));
    wc_resize(Dim(w), Dim(h), Density(d));
    return Number(env, rc);
}

napi_value SurfaceGone(napi_env env, napi_callback_info info) {
    (void)env;
    (void)info;
    g_presenter.Detach();
    wc_surface_gone();
    return Undefined(env);
}

napi_value Resize(napi_env env, napi_callback_info info) {
    napi_value argv[3];
    if (!ArgCount(env, info, argv, 3)) {
        return Undefined(env);
    }
    double w = 0;
    double h = 0;
    double d = 1;
    ReadDouble(env, argv[0], w);
    ReadDouble(env, argv[1], h);
    ReadDouble(env, argv[2], d);
    wc_resize(Dim(w), Dim(h), Density(d));
    return Undefined(env);
}

napi_value Pointer(napi_env env, napi_callback_info info) {
    napi_value argv[5];
    if (!ArgCount(env, info, argv, 5)) {
        return Undefined(env);
    }
    int32_t kind = 2;
    int32_t button = 3;
    int32_t modifiers = 0;
    double x = 0;
    double y = 0;
    ReadInt(env, argv[0], kind);
    ReadDouble(env, argv[1], x);
    ReadDouble(env, argv[2], y);
    ReadInt(env, argv[3], button);
    ReadInt(env, argv[4], modifiers);
    wc_pointer(kind, static_cast<float>(x), static_cast<float>(y), button, modifiers);
    return Undefined(env);
}

napi_value Scroll(napi_env env, napi_callback_info info) {
    napi_value argv[2];
    if (!ArgCount(env, info, argv, 2)) {
        return Undefined(env);
    }
    double dx = 0;
    double dy = 0;
    ReadDouble(env, argv[0], dx);
    ReadDouble(env, argv[1], dy);
    wc_scroll(static_cast<float>(dx), static_cast<float>(dy));
    return Undefined(env);
}

/* key(code, pressed, repeated, modifiers, text) */
napi_value Key(napi_env env, napi_callback_info info) {
    napi_value argv[5];
    if (!ArgCount(env, info, argv, 5)) {
        return Undefined(env);
    }
    int32_t code = 0;
    bool pressed = false;
    bool repeated = false;
    int32_t modifiers = 0;
    ReadInt(env, argv[0], code);
    ReadBool(env, argv[1], pressed);
    ReadBool(env, argv[2], repeated);
    ReadInt(env, argv[3], modifiers);
    /* The character travels with the key: whether a chord types anything is decided in Rust
     * (ArkUI's keyText is unreliable while Ctrl is held — Ctrl+S arrives as 'K' on the emulator). */
    const uint32_t typed = pressed ? FirstCodepoint(env, argv[4]) : 0;
    wc_key(code, pressed ? 1 : 0, repeated ? 1 : 0, modifiers, typed);
    return Undefined(env);
}

napi_value Text(napi_env env, napi_callback_info info) {
    napi_value argv[1];
    if (!ArgCount(env, info, argv, 1)) {
        return Undefined(env);
    }
    int32_t cp = 0;
    ReadInt(env, argv[0], cp);
    if (cp > 0) {
        wc_text(static_cast<uint32_t>(cp));
    }
    return Undefined(env);
}

/* ready(): the page is up; ask for a document to open at launch */
napi_value Ready(napi_env env, napi_callback_info info) {
    (void)env;
    (void)info;
    wc_ready();
    return Undefined(env);
}

/* documentPath() -> the current document's path, or '' */
napi_value DocumentPath(napi_env env, napi_callback_info info) {
    (void)info;
    const uint8_t *data = wc_document_path();
    const size_t len = wc_document_path_len();
    napi_value out = nullptr;
    if (data == nullptr || len == 0) {
        napi_create_string_utf8(env, "", 0, &out);
        return out;
    }
    napi_create_string_utf8(env, reinterpret_cast<const char *>(data), len, &out);
    return out;
}

/* clipboardPreload(): read the clipboard now (the page just came to the foreground) */
napi_value ClipboardPreload(napi_env env, napi_callback_info info) {
    (void)env;
    (void)info;
    wc_clipboard_preload();
    return Undefined(env);
}

/* zoom(factor): a pinch (1.0 changes nothing) */
napi_value Zoom(napi_env env, napi_callback_info info) {
    napi_value argv[1];
    if (!ArgCount(env, info, argv, 1)) {
        return Undefined(env);
    }
    double factor = 1;
    ReadDouble(env, argv[0], factor);
    wc_zoom(static_cast<float>(factor));
    return Undefined(env);
}

/* scrollFling(vx, vy): the release velocity of a two-finger scroll, in points per second */
napi_value ScrollFling(napi_env env, napi_callback_info info) {
    napi_value argv[2];
    if (!ArgCount(env, info, argv, 2)) {
        return Undefined(env);
    }
    double vx = 0;
    double vy = 0;
    ReadDouble(env, argv[0], vx);
    ReadDouble(env, argv[1], vy);
    wc_scroll_fling(static_cast<float>(vx), static_cast<float>(vy));
    return Undefined(env);
}

/* flinging() -> 1 while a fling is still running */
napi_value Flinging(napi_env env, napi_callback_info info) {
    (void)info;
    return Number(env, wc_flinging());
}

/* caretPresent() -> 1 when the document is editing text */
napi_value CaretPresent(napi_env env, napi_callback_info info) {
    (void)info;
    return Number(env, wc_caret_present());
}

/* overlayOpen() -> 1 when the context menu (or another app-drawn overlay) is up */
napi_value OverlayOpen(napi_env env, napi_callback_info info) {
    (void)info;
    return Number(env, wc_overlay_open());
}

/* selectionRange() -> 1 when text is selected */
napi_value SelectionRange(napi_env env, napi_callback_info info) {
    (void)info;
    return Number(env, wc_selection_range());
}

/* caretX() -> the caret's x in points */
napi_value CaretX(napi_env env, napi_callback_info info) {
    (void)info;
    return Number(env, wc_caret_x());
}

/* caretY() -> the caret's top y in points */
napi_value CaretY(napi_env env, napi_callback_info info) {
    (void)info;
    return Number(env, wc_caret_y());
}

/* caretHeight() -> how tall the caret's line is, in points */
napi_value CaretHeight(napi_env env, napi_callback_info info) {
    (void)info;
    return Number(env, wc_caret_height());
}

/* handleX() -> the drag handle's x in points */
napi_value HandleX(napi_env env, napi_callback_info info) {
    (void)info;
    return Number(env, wc_handle_x());
}

/* handleY() -> the drag handle's top y in points */
napi_value HandleY(napi_env env, napi_callback_info info) {
    (void)info;
    return Number(env, wc_handle_y());
}

/* handleHeight() -> how tall the drag handle's line is, in points */
napi_value HandleHeight(napi_env env, napi_callback_info info) {
    (void)info;
    return Number(env, wc_handle_height());
}

/* imePreview(text): the composing text, drawn at the caret until the commit replaces it */
napi_value ImePreview(napi_env env, napi_callback_info info) {
    napi_value argv[1];
    if (!ArgCount(env, info, argv, 1)) {
        return Undefined(env);
    }
    std::string text;
    if (!ReadString(env, argv[0], text)) {
        WC_LOGW("the input method's preview is not a string");
        return Undefined(env);
    }
    wc_ime_preview(reinterpret_cast<const uint8_t *>(text.data()), text.size());
    return Undefined(env);
}

/* imeCommit(text): the committed text, one undoable insert */
napi_value ImeCommit(napi_env env, napi_callback_info info) {
    napi_value argv[1];
    if (!ArgCount(env, info, argv, 1)) {
        return Undefined(env);
    }
    std::string text;
    if (!ReadString(env, argv[0], text)) {
        WC_LOGW("the input method's text is not a string");
        return Undefined(env);
    }
    wc_ime_commit(reinterpret_cast<const uint8_t *>(text.data()), text.size());
    return Undefined(env);
}

napi_value Ime(napi_env env, napi_callback_info info) {
    napi_value argv[1];
    if (!ArgCount(env, info, argv, 1)) {
        return Undefined(env);
    }
    bool composing = false;
    ReadBool(env, argv[0], composing);
    wc_ime(composing ? 1 : 0);
    return Undefined(env);
}

napi_value Focus(napi_env env, napi_callback_info info) {
    napi_value argv[1];
    if (!ArgCount(env, info, argv, 1)) {
        return Undefined(env);
    }
    bool focused = false;
    ReadBool(env, argv[0], focused);
    wc_focus(focused ? 1 : 0);
    return Undefined(env);
}

/* hostTake() -> the next request for ArkTS (a JSON string), or null */
napi_value HostTake(napi_env env, napi_callback_info info) {
    (void)info;
    const uint8_t *data = wc_host_take();
    const size_t len = wc_host_take_len();
    napi_value out = nullptr;
    if (data == nullptr || len == 0) {
        napi_get_null(env, &out);
        return out;
    }
    napi_create_string_utf8(env, reinterpret_cast<const char *>(data), len, &out);
    return out;
}

/* hostAnswer(id, json) */
napi_value HostAnswer(napi_env env, napi_callback_info info) {
    napi_value argv[2];
    if (!ArgCount(env, info, argv, 2)) {
        return Undefined(env);
    }
    double id = 0;
    ReadDouble(env, argv[0], id);
    std::string json;
    if (!ReadString(env, argv[1], json)) {
        WC_LOGW("host answer is not a string");
        return Undefined(env);
    }
    wc_host_answer(static_cast<uint64_t>(id), reinterpret_cast<const uint8_t *>(json.data()), json.size());
    return Undefined(env);
}

/* render() -> 1 when a frame went to the screen */
napi_value Render(napi_env env, napi_callback_info info) {
    (void)info;
    if (!g_presenter.Ready() || wc_render() != 1) {
        return Number(env, 0.0);
    }
    const uint32_t damage_w = wc_damage_width();
    const uint32_t damage_h = wc_damage_height();
    if (damage_w == 0 || damage_h == 0) {
        // Nothing changed visually; the screen already shows the last frame.
        return Number(env, 0.0);
    }
    const bool presented = g_presenter.Present(wc_pixels(), wc_pixels_len(), wc_width(), wc_height(), wc_stride(),
                                               wc_damage_x(), wc_damage_y(), damage_w, damage_h);
    if (!presented) {
        return Number(env, 0.0);
    }
    g_presented++;
    if (g_presented % 300 == 0) {
        WC_LOGD("%{public}llu frames, %{public}llu events, %{public}ux%{public}u", (unsigned long long)g_presented,
                (unsigned long long)wc_events(), wc_width(), wc_height());
    }
    return Number(env, 1.0);
}

/* stats() -> JSON, for ArkTS logs and the About screen to come */
napi_value Stats(napi_env env, napi_callback_info info) {
    (void)info;
    char buf[512];
    snprintf(buf, sizeof(buf),
             "{\"width\":%u,\"height\":%u,\"density\":%.3f,\"frames\":%llu,\"events\":%llu,"
             "\"paragraphs\":%llu,\"fontDirs\":%llu,\"focused\":%d,\"pressed\":%d,\"presented\":%llu}",
             wc_width(), wc_height(), wc_density(), (unsigned long long)wc_frames(), (unsigned long long)wc_events(),
             (unsigned long long)wc_paragraphs(), (unsigned long long)wc_font_dirs(), wc_focused(), wc_pressed(),
             (unsigned long long)g_presented);
    napi_value out = nullptr;
    napi_create_string_utf8(env, buf, NAPI_AUTO_LENGTH, &out);
    return out;
}

napi_value Destroy(napi_env env, napi_callback_info info) {
    (void)env;
    (void)info;
    g_presenter.Detach();
    wc_app_free();
    WC_LOGI("app freed");
    return Undefined(env);
}

napi_value Init(napi_env env, napi_value exports) {
    napi_property_descriptor desc[] = {
        {"init", nullptr, InitApp, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"surface", nullptr, Surface, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"surfaceGone", nullptr, SurfaceGone, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"resize", nullptr, Resize, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"pointer", nullptr, Pointer, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"scroll", nullptr, Scroll, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"key", nullptr, Key, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"text", nullptr, Text, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"ime", nullptr, Ime, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"focus", nullptr, Focus, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"render", nullptr, Render, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"stats", nullptr, Stats, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"ready", nullptr, Ready, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"documentPath", nullptr, DocumentPath, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"clipboardPreload", nullptr, ClipboardPreload, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"zoom", nullptr, Zoom, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"scrollFling", nullptr, ScrollFling, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"flinging", nullptr, Flinging, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"imePreview", nullptr, ImePreview, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"imeCommit", nullptr, ImeCommit, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"caretPresent", nullptr, CaretPresent, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"selectionRange", nullptr, SelectionRange, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"overlayOpen", nullptr, OverlayOpen, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"caretX", nullptr, CaretX, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"caretY", nullptr, CaretY, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"caretHeight", nullptr, CaretHeight, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"handleX", nullptr, HandleX, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"handleY", nullptr, HandleY, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"handleHeight", nullptr, HandleHeight, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"hostTake", nullptr, HostTake, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"hostAnswer", nullptr, HostAnswer, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"destroy", nullptr, Destroy, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    if (napi_define_properties(env, exports, sizeof(desc) / sizeof(desc[0]), desc) != napi_ok) {
        WC_LOGE("cannot define the module's functions");
    }
    WC_LOGI("native module loaded");
    return exports;
}

}  // namespace

/* ArkTS reaches this through `import native from 'libwordcraft.so'`: the module name is the file
 * name without the prefix, which is why it is "wordcraft". */
static napi_module g_wordcraftModule = {1, 0, nullptr, Init, "wordcraft", nullptr, {nullptr, nullptr, nullptr, nullptr}};

extern "C" __attribute__((constructor)) void RegisterWordcraftModule(void) {
    napi_module_register(&g_wordcraftModule);
}
