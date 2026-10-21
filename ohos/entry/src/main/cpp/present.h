/*
 * Presenting frames on the XComponent's surface.
 *
 * The XComponent holds a Surface; ArkUI hands us its id, and the native window API turns that into
 * an OHNativeWindow whose buffer queue we write into: RequestBuffer → copy pixels → FlushBuffer.
 * That is the documented CPU path (see "XComponent 渲染上屏原理" in the HarmonyOS docs), and it
 * needs no GPU context — a driver bug cannot take the editor down.
 */
#ifndef WORDCRAFT_PRESENT_H
#define WORDCRAFT_PRESENT_H

#include <cstddef>
#include <cstdint>
#include <map>
#include <utility>

#include <native_window/external_window.h>

namespace wordcraft {

class Presenter {
public:
    Presenter() = default;
    ~Presenter();

    Presenter(const Presenter &) = delete;
    Presenter &operator=(const Presenter &) = delete;

    /* Adopt the surface with this id at this size. 0 on success. */
    int32_t Attach(uint64_t surfaceId, uint32_t width, uint32_t height);
    /* Forget the surface; the next Attach starts a new one. */
    void Detach();
    bool Ready() const { return window_ != nullptr; }

    /* Copy one RGBA8888 frame into the next buffer and flush it. False when nothing went out. */
    bool Present(const uint8_t *pixels, size_t len, uint32_t width, uint32_t height, size_t stride);

private:
    /* The buffer's memory, mapped once per file descriptor and kept until Detach. Mapping costs a
     * page-table walk over the whole 21 MB frame; doing it per frame cost more than drawing. */
    uint8_t *MapBuffer(BufferHandle *handle, size_t &size);
    void UnmapAll();

    OHNativeWindow *window_ = nullptr;
    std::map<int32_t, std::pair<void *, size_t>> mappings_;
    uint32_t width_ = 0;
    uint32_t height_ = 0;
    /* Set once, so a size mismatch between our frame and the buffer is reported once, not per frame. */
    bool warnedGeometry_ = false;
};

}  // namespace wordcraft

#endif  // WORDCRAFT_PRESENT_H
