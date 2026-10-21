/*
 * hilog wrappers. Everything the C++ shell and the Rust app log lands in
 * `hdc shell hilog -T WordCraft`, which is the only window into a device without a debugger.
 */
#ifndef WORDCRAFT_LOG_H
#define WORDCRAFT_LOG_H

#include <hilog/log.h>

#undef LOG_DOMAIN
#undef LOG_TAG
#define LOG_DOMAIN 0x0000
#define LOG_TAG "WordCraft"

/* `%{public}` keeps hilog from hiding the values behind its privacy filter. */
#define WC_LOGI(fmt, ...) OH_LOG_Print(LOG_APP, LOG_INFO, LOG_DOMAIN, LOG_TAG, fmt, ##__VA_ARGS__)
#define WC_LOGW(fmt, ...) OH_LOG_Print(LOG_APP, LOG_WARN, LOG_DOMAIN, LOG_TAG, fmt, ##__VA_ARGS__)
#define WC_LOGE(fmt, ...) OH_LOG_Print(LOG_APP, LOG_ERROR, LOG_DOMAIN, LOG_TAG, fmt, ##__VA_ARGS__)
#define WC_LOGD(fmt, ...) OH_LOG_Print(LOG_APP, LOG_DEBUG, LOG_DOMAIN, LOG_TAG, fmt, ##__VA_ARGS__)

#endif  // WORDCRAFT_LOG_H
