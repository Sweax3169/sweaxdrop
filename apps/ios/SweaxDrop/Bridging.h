// The Rust core (crates/ffi), linked as libsweaxdrop.a.
#include <stdbool.h>
#include <stdint.h>

typedef struct {
    void *ctx;
    void (*on_event)(void *ctx, const char *json);
    void (*on_reply)(void *ctx, int32_t id, const char *json);
    int64_t (*open_read)(void *ctx, const char *token);
    int32_t (*create_output)(void *ctx, const char *req, char *out, int32_t cap);
    int32_t (*finish_output)(void *ctx, const char *handle, bool ok, int64_t mtime, char *out, int32_t cap);
    void (*set_clipboard)(void *ctx, const char *text);
    void (*notify)(void *ctx, const char *title, const char *body);
} SdCallbacks;

bool sd_start(const char *data_dir, const char *name, const char *model, const char *platform, bool discovery, SdCallbacks cb);
void sd_call(int32_t id, const char *req);
uint16_t sd_port(void);
const char *sd_device_id(void);
