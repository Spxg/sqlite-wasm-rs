/* SQLCipher with its LibTomCrypt provider, whose sources build as separate units with the same configuration. */
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

#include "sqlcipher-ltc.h"
#include "sqlcipher.c"
