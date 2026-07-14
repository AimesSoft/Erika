#include <stdarg.h>
#include <stdio.h>

typedef struct ass_library ASS_Library;

typedef void (*ErikaAssLogSink)(void *opaque, int level, const char *message);

typedef struct ErikaAssLogBridge {
    ErikaAssLogSink sink;
    void *opaque;
} ErikaAssLogBridge;

extern void ass_set_message_cb(
    ASS_Library *library,
    void (*callback)(int level, const char *format, va_list args, void *data),
    void *data);

static void erika_ass_message_callback(
    int level,
    const char *format,
    va_list args,
    void *data) {
    ErikaAssLogBridge *bridge = (ErikaAssLogBridge *) data;
    if (!bridge || !bridge->sink || !format)
        return;

    char message[2048];
    va_list copy;
    va_copy(copy, args);
    int written = vsnprintf(message, sizeof(message), format, copy);
    va_end(copy);
    if (written < 0)
        return;
    message[sizeof(message) - 1] = '\0';
    bridge->sink(bridge->opaque, level, message);
}

void erika_ass_install_log_bridge(
    ASS_Library *library,
    ErikaAssLogBridge *bridge) {
    if (!library || !bridge)
        return;
    ass_set_message_cb(library, erika_ass_message_callback, bridge);
}
