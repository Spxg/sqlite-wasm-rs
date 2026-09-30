/* SQLCipher and its LibTomCrypt provider as one translation unit, with the sqlcipher-src directory on the include path. */
#define SQLITE_HAS_CODEC 1
#define SQLCIPHER_CRYPTO_LIBTOMCRYPT 1
/* Installs the entropy hook before SQLCipher's own init runs. */
#define SQLITE_EXTRA_INIT sqlcipher_wasm_extra_init
#define SQLITE_EXTRA_SHUTDOWN sqlcipher_extra_shutdown
/* SQLCipher's log writes timestamps through gettimeofday, which the shim lacks. */
#define SQLCIPHER_OMIT_LOG 1
#define SQLCIPHER_OMIT_LOG_DEVICE 1
#define OMIT_MEMLOCK 1
/* SQLCipher refuses SQLITE_THREADSAFE=0, and SQLite runs single-threaded here, so no-op mutexes suffice. */
#undef SQLITE_THREADSAFE
#define SQLITE_THREADSAFE 1
#define SQLITE_MUTEX_NOOP 1

#define LTC_NOTHING
#define LTC_RIJNDAEL
#define LTC_CBC_MODE
#define LTC_SHA1
#define LTC_SHA256
#define LTC_SHA512
#define LTC_HASH_HELPERS
#define LTC_HMAC
#define LTC_FORTUNA
#define LTC_PKCS_5
#define LTC_RNG_GET_BYTES
#define LTC_PRNG_ENABLE_LTC_RNG
#define LTC_NO_TEST
#define LTC_NO_FILE
#define LTC_NO_ASM
#define LTC_NO_MATH
#define LTC_CLEAN_STACK
/* Bad arguments return CRYPT_INVALID_ARG instead of raising a signal the shim lacks. */
#define ARGTYPE 4
#define LTC_SOURCE
/* rng_get_bytes always ends in a clock-jitter generator, so reaching it must abort. */
#define XCLOCK sqlcipher_wasm_no_clock
#define XCLOCKS_PER_SEC 1
long sqlcipher_wasm_no_clock(void);

#include "sqlcipher.c"
#include "libtomcrypt.c"

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

/* No files or exit handlers here, so cipher_log, cipher_profile and cipher_migrate fail and atexit is a no-op. */
#if defined(__wasm__)
FILE *const stdout = 0;
FILE *const stderr = 0;
FILE *fopen(const char *restrict path, const char *restrict mode) { (void)path; (void)mode; return 0; }
int fprintf(FILE *restrict f, const char *restrict fmt, ...) { (void)f; (void)fmt; return -1; }
int rename(const char *from, const char *to) { (void)from; (void)to; return -1; }
int atexit(void (*fn)(void)) { (void)fn; return 0; }
#endif
