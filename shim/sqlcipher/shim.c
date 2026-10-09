#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>

#include "libtomcrypt.h"
#include <tomcrypt.h>

/* POSIX, declared here because the shim has no unistd.h. */
int getentropy(void *buffer, size_t len);
int sqlcipher_extra_init(const char *arg);

long sqlcipher_wasm_no_clock(void) { abort(); }

/* Host entropy only, failing closed so rng_get_bytes never falls through to the clock. */
static unsigned long sqlcipher_wasm_rng(unsigned char *out, unsigned long len, void (*callback)(void)) {
  (void)callback;
  if (getentropy(out, len) != 0) abort();
  return len;
}

int sqlcipher_wasm_extra_init(const char *arg) {
  ltc_rng = sqlcipher_wasm_rng;
  return sqlcipher_extra_init(arg);
}

/* File logging/profiling and migration rename are unavailable. No exit handlers. */
FILE *const stdout = 0;
FILE *const stderr = 0;
FILE *fopen(const char *restrict path, const char *restrict mode) { (void)path; (void)mode; return 0; }
int fprintf(FILE *restrict f, const char *restrict fmt, ...) { (void)f; (void)fmt; return -1; }
int rename(const char *from, const char *to) { (void)from; (void)to; return -1; }
/* No host exit lifecycle: cleanup is invoked through sqlite3_shutdown instead. */
int atexit(void (*fn)(void)) { (void)fn; return 1; }
