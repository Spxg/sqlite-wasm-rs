#include "wasm-shim.h"

void *malloc(size_t size) { return rust_sqlite_wasm_shim_malloc(size); }

void *realloc(void *ptr, size_t size) {
  return rust_sqlite_wasm_shim_realloc(ptr, size);
}

void *calloc(size_t num, size_t size) {
  return rust_sqlite_wasm_shim_calloc(num, size);
}

void free(void *ptr) { return rust_sqlite_wasm_shim_free(ptr); }

double emscripten_get_now(void) {
  return rust_sqlite_wasm_shim_emscripten_get_now();
}

void _localtime_js(time_t t, struct tm *__restrict__ tm) {
  return rust_sqlite_wasm_shim_localtime_js(t, tm);
}

void _tzset_js(long *timezone, int *daylight, char *std_name, char *dst_name) {
  return rust_sqlite_wasm_shim_tzset_js(timezone, daylight, std_name, dst_name);
}

uint16_t __wasi_random_get(uint8_t *buf, size_t buf_len) {
  return rust_sqlite_wasm_shim_wasi_random_get(buf, buf_len);
}

void exit(int code) { rust_sqlite_wasm_shim_exit(code); }

void _abort_js() { rust_sqlite_wasm_shim_abort_js(); }
