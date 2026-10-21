#include "present.h"

#include <algorithm>
#include <cstring>
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

void Presenter::Detach() {
    if (window_ != nullptr) {
        OH_NativeWindow_DestroyNativeWindow(window_);
        window_ = nullptr;
        WC_LOGI("surface detached");
    }
    width_ = 0;
    height_ = 0;
}

bool Presenter::Present(const uint8_t *pixels, size_t len, uint32_t width, uint32_t height, size_t stride) {
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
    if (handle == nullptr || handle->virAddr == nullptr || handle->stride <= 0 || handle->width <= 0 || handle->height <= 0) {
        WC_LOGE("buffer has no writable memory");
        OH_NativeWindow_NativeWindowAbortBuffer(window_, buffer);
        if (fence >= 0) {
            close(fence);
        }
        return false;
    }

    const uint32_t copyW = std::min(width, (uint32_t)handle->width);
    const uint32_t copyH = std::min(height, (uint32_t)handle->height);
    if (!warnedGeometry_ && (copyW != width || copyH != height)) {
        // The surface is a different size than the frame we drew: copy what fits and say so once.
        WC_LOGW("frame %{public}ux%{public}u into %{public}dx%{public}d buffer", width, height, handle->width, handle->height);
        warnedGeometry_ = true;
    }
    uint8_t *dst = static_cast<uint8_t *>(handle->virAddr);
    for (uint32_t y = 0; y < copyH; ++y) {
        std::memcpy(dst + (size_t)y * (size_t)handle->stride, pixels + (size_t)y * stride, (size_t)copyW * 4);
    }

    Region region{nullptr, 0};  // the whole buffer is dirty
    ret = OH_NativeWindow_NativeWindowFlushBuffer(window_, buffer, fence, region);
    if (ret != 0) {
        WC_LOGE("FlushBuffer failed (%{public}d)", ret);
        return false;
    }
    return true;
}

}  // namespace wordcraft
