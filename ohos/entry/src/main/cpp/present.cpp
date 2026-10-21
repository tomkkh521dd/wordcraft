#include "present.h"

#include <algorithm>
#include <cstring>
#include <sys/mman.h>
#include <unistd.h>

#include <native_buffer/native_buffer.h>

#include "log.h"

namespace wordcraft {

Presenter::~Presenter() { Detach(); }

int32_t Presenter::Attach(uint64_t surfaceId, uint32_t width, uint32_t height) {
    Detach();
    OHNativeWindow *window = nullptr;
    int32_t ret = OH_NativeWindow_CreateNativeWindowFromSurfaceId(surfaceId, &window);
    if (ret != 0 || window == nullptr) {
        WC_LOGE("surface %{public}llu: no native window (%{public}d)", (unsigned long long)surfaceId, ret);
        return ret != 0 ? ret : -1;
    }
    // RGBA8888, written by the CPU: the app writes every byte of every frame.
    int32_t format = NATIVEBUFFER_PIXEL_FMT_RGBA_8888;
    ret = OH_NativeWindow_NativeWindowHandleOpt(window, SET_FORMAT, format);
    if (ret != 0) {
        WC_LOGE("surface %{public}llu: SET_FORMAT failed (%{public}d)", (unsigned long long)surfaceId, ret);
    }
    uint64_t usage = NATIVEBUFFER_USAGE_CPU_WRITE | NATIVEBUFFER_USAGE_MEM_DMA;
    ret = OH_NativeWindow_NativeWindowHandleOpt(window, SET_USAGE, usage);
    if (ret != 0) {
        // Some devices refuse one usage flag; the format above is the part that matters.
        WC_LOGW("surface %{public}llu: SET_USAGE failed (%{public}d)", (unsigned long long)surfaceId, ret);
    }
    if (width > 0 && height > 0) {
        ret = OH_NativeWindow_NativeWindowHandleOpt(window, SET_BUFFER_GEOMETRY, (int32_t)width, (int32_t)height);
        if (ret != 0) {
            WC_LOGW("surface %{public}llu: SET_BUFFER_GEOMETRY failed (%{public}d)", (unsigned long long)surfaceId, ret);
        }
    }
    window_ = window;
    width_ = width;
    height_ = height;
    warnedGeometry_ = false;
    WC_LOGI("surface %{public}llu attached, %{public}ux%{public}u", (unsigned long long)surfaceId, width, height);
    return 0;
}

uint8_t *Presenter::MapBuffer(BufferHandle *handle, size_t &size) {
    if (handle == nullptr || handle->fd < 0) {
        return nullptr;
    }
    if (handle->virAddr != nullptr) {
        size = static_cast<size_t>(handle->size);
        return static_cast<uint8_t *>(handle->virAddr);
    }
    auto found = mappings_.find(handle->fd);
    if (found != mappings_.end()) {
        size = found->second.second;
        return static_cast<uint8_t *>(found->second.first);
    }
    size = handle->size > 0 ? static_cast<size_t>(handle->size) : (size_t)handle->stride * (size_t)handle->height;
    if (size == 0) {
        return nullptr;
    }
    void *mapped = mmap(nullptr, size, PROT_READ | PROT_WRITE, MAP_SHARED, handle->fd, 0);
    if (mapped == MAP_FAILED) {
        return nullptr;
    }
    mappings_[handle->fd] = {mapped, size};
    return static_cast<uint8_t *>(mapped);
}

void Presenter::UnmapAll() {
    for (auto &entry : mappings_) {
        munmap(entry.second.first, entry.second.second);
    }
    mappings_.clear();
}

void Presenter::Detach() {
    UnmapAll();
    if (window_ != nullptr) {
        OH_NativeWindow_DestroyNativeWindow(window_);
        window_ = nullptr;
        WC_LOGI("surface detached");
    }
    width_ = 0;
    height_ = 0;
}

bool Presenter::Present(const uint8_t *pixels, size_t len, uint32_t width, uint32_t height, size_t stride,
                       uint32_t damage_x, uint32_t damage_y, uint32_t damage_w, uint32_t damage_h) {
    if (window_ == nullptr || pixels == nullptr || width == 0 || height == 0) {
        return false;
    }
    // Everything is bounds-checked before a row is copied: these numbers come from the platform.
    if (stride < (size_t)width * 4 || len < stride * (size_t)height) {
        WC_LOGE("frame does not fit: %{public}ux%{public}u stride %{public}zu len %{public}zu", width, height, stride, len);
        return false;
    }

    OHNativeWindowBuffer *buffer = nullptr;
    int fence = -1;
    int32_t ret = OH_NativeWindow_NativeWindowRequestBuffer(window_, &buffer, &fence);
    if (ret != 0 || buffer == nullptr) {
        WC_LOGE("RequestBuffer failed (%{public}d)", ret);
        return false;
    }
    BufferHandle *handle = OH_NativeWindow_GetBufferHandleFromNative(buffer);
    if (handle == nullptr) {
        WC_LOGE("buffer has no handle");
        OH_NativeWindow_NativeWindowAbortBuffer(window_, buffer);
        if (fence >= 0) {
            close(fence);
        }
        return false;
    }
    // `virAddr` is not always filled in: the documented CPU path maps the buffer's file descriptor,
    // once per buffer (see MapBuffer).
    size_t mapped_size = 0;
    uint8_t *dst = MapBuffer(handle, mapped_size);
    if (dst == nullptr || handle->stride <= 0 || handle->width <= 0 || handle->height <= 0) {
        if (!warnedGeometry_) {
            WC_LOGE("buffer is not writable: %{public}dx%{public}d stride %{public}d size %{public}d fd %{public}d virAddr %{public}d",
                    handle->width, handle->height, handle->stride, handle->size, handle->fd, handle->virAddr == nullptr ? 0 : 1);
            warnedGeometry_ = true;
        }
        OH_NativeWindow_NativeWindowAbortBuffer(window_, buffer);
        if (fence >= 0) {
            close(fence);
        }
        return false;
    }

    // Only the damaged part: the buffer keeps the previous frame everywhere else.
    const uint32_t frame_w = std::min(width, (uint32_t)handle->width);
    const uint32_t frame_h = std::min(height, (uint32_t)handle->height);
    const uint32_t copyX = std::min(damage_x, frame_w);
    const uint32_t copyY = std::min(damage_y, frame_h);
    const uint32_t copyW = std::min(damage_w, frame_w - copyX);
    const uint32_t copyH = std::min(damage_h, frame_h - copyY);
    if (copyW == 0 || copyH == 0) {
        OH_NativeWindow_NativeWindowAbortBuffer(window_, buffer);
        if (fence >= 0) {
            close(fence);
        }
        return false;
    }
    if (!warnedGeometry_ && (copyW != width || copyH != height)) {
        // The surface is a different size than the frame we drew: copy what fits and say so once.
        WC_LOGW("frame %{public}ux%{public}u into %{public}dx%{public}d buffer", width, height, handle->width, handle->height);
        warnedGeometry_ = true;
    }
    for (uint32_t y = 0; y < copyH; ++y) {
        const uint32_t row = copyY + y;
        std::memcpy(dst + (size_t)row * (size_t)handle->stride + (size_t)copyX * 4,
                    pixels + (size_t)row * stride + (size_t)copyX * 4, (size_t)copyW * 4);
    }

    // Tell the compositor which part to pick up.
    Region::Rect dirty{copyX, copyY, copyW, copyH};
    Region region{&dirty, 1};
    ret = OH_NativeWindow_NativeWindowFlushBuffer(window_, buffer, fence, region);
    if (ret != 0) {
        WC_LOGE("FlushBuffer failed (%{public}d)", ret);
        return false;
    }
    return true;
}

}  // namespace wordcraft
