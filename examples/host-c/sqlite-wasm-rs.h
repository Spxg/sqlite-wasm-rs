#ifndef SQLITE_WASM_RS_H
#define SQLITE_WASM_RS_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Host status codes, not SQLite result codes or errno. */
enum {
    RUST_SQLITE_WASM_HOST_OK = 0,
    RUST_SQLITE_WASM_HOST_UNSUPPORTED = 1,
    RUST_SQLITE_WASM_HOST_UNAVAILABLE = 2,
    RUST_SQLITE_WASM_HOST_INVALID_TIME = 3
};

/* Matches sqlite_wasm_rs::host::LocalTime, not libc's struct tm. */
typedef struct rust_sqlite_wasm_local_time {
    int32_t year;               /* Full Gregorian year, not years since 1900. */
    int32_t month;              /* 1..12 */
    int32_t day;                /* 1..31, valid for the month and year. */
    int32_t hour;               /* 0..23 */
    int32_t minute;             /* 0..59 */
    int32_t second;             /* 0..60, including a leap second. */
    int32_t weekday;            /* 0..6, Sunday = 0. */
    int32_t yearday;            /* 0..365, January 1 = 0. */
    int32_t is_dst;             /* 1 = active, 0 = inactive, -1 = unknown. */
    int32_t utc_offset_seconds; /* Local time minus UTC, in seconds. */
} rust_sqlite_wasm_local_time;

/* Define each hook once, or supply Wasm imports from module "env".
 * For imports, explicitly allow these undefined symbols at link time.
 * Calls are single-threaded; hooks must not reenter SQLite or retain pointers.
 * Pointers address the module's linear memory. Output pointers are non-null,
 * aligned and writable; buffers may be null only when len == 0, and need not
 * be initialized. Fallible hooks return a status above and fully write their
 * outputs on success. Output contents are unspecified on failure.
 * Buffers must lie within one allocation, with exclusive access during the
 * call and len <= PTRDIFF_MAX (Rust isize::MAX on wasm32). The wasm-bindgen adapter
 * rejects larger lengths or null buffers with nonzero lengths before accessing
 * memory: random returns 0, fill_entropy returns RUST_SQLITE_WASM_HOST_UNAVAILABLE.
 */

/* nanoseconds is the subsecond part, strictly less than 1,000,000,000.
 * May return early if synchronous sleep is unavailable, without busy-waiting.
 */
void rust_sqlite_wasm_host_sleep(uint64_t seconds, uint32_t nanoseconds);

/* Initializes and returns N bytes, 0 <= N <= len. Need not be cryptographic. */
size_t rust_sqlite_wasm_host_random(uint8_t *buf, size_t len);

/* UTC milliseconds since the Unix epoch. */
int32_t rust_sqlite_wasm_host_epoch_timestamp_in_ms(int64_t *out);

/* Fill the entire buffer with cryptographically secure bytes or fail.
 * Never fall back to weak randomness. SQLite3MC may abort on failure.
 */
int32_t rust_sqlite_wasm_host_fill_entropy(uint8_t *buf, size_t len);

/* Convert Unix seconds to the host's local time zone, or fail (no UTC fallback). */
int32_t rust_sqlite_wasm_host_localtime(int64_t unix_seconds,
                                      rust_sqlite_wasm_local_time *out);

#ifdef __cplusplus
}
#endif

#endif
