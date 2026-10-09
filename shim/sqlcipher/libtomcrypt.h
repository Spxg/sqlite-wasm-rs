/* The one LibTomCrypt configuration, which every SQLCipher and LibTomCrypt unit must share for struct layouts to agree. */
#ifndef SQLCIPHER_LTC_H
#define SQLCIPHER_LTC_H

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
/* rng_get_bytes always ends in a clock-jitter generator, so reaching it must abort. */
#define XCLOCK sqlcipher_wasm_no_clock
#define XCLOCKS_PER_SEC 1
long sqlcipher_wasm_no_clock(void);

#endif
