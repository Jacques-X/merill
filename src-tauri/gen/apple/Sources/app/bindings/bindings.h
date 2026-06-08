#pragma once

#include <stdbool.h>

#ifdef __cplusplus
namespace ffi {
extern "C" {
#endif
void start_app();
#ifdef __cplusplus
}
}
#endif

typedef void (*MerillCallback)(const char *response_json, void *context);

#ifdef __cplusplus
extern "C" {
#endif
bool merill_initialize(const char *data_dir);
void merill_call_async(const char *request_json, MerillCallback callback, void *context);
void merill_free_string(char *pointer);
#ifdef __cplusplus
}
#endif
