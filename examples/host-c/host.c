#include "sqlite-wasm-rs.h"

/* Minimal WASI preview1 declarations, so this example needs no WASI SDK/libc.
 * Layout reference: WebAssembly/wasi-libc's wasi/wasip1.h.
 * Only the clock variant of subscription is needed here.
 */
enum { WASI_OK = 0, WASI_CLOCK_REALTIME = 0, WASI_CLOCK_MONOTONIC = 1,
       WASI_EVENT_CLOCK = 0 };

typedef struct {
    uint64_t userdata;
    uint8_t type;
    struct {
        uint32_t id;
        uint64_t timeout;
        uint64_t precision;
        uint16_t flags;
    } clock;
} clock_subscription;

typedef struct {
    uint64_t userdata;
    uint16_t error;
    uint8_t type;
    uint64_t nbytes;
    uint16_t flags;
} poll_event;

_Static_assert(sizeof(clock_subscription) == 48, "WASI subscription layout");
_Static_assert(offsetof(clock_subscription, clock.timeout) == 24, "WASI timeout offset");
_Static_assert(sizeof(poll_event) == 32, "WASI event layout");

__attribute__((import_module("wasi_snapshot_preview1"), import_name("clock_time_get")))
extern uint16_t wasi_clock_time_get(uint32_t clock, uint64_t precision, uint64_t *out);
__attribute__((import_module("wasi_snapshot_preview1"), import_name("random_get")))
extern uint16_t wasi_random_get(uint8_t *buf, size_t len);
__attribute__((import_module("wasi_snapshot_preview1"), import_name("poll_oneoff")))
extern uint16_t wasi_poll_oneoff(const clock_subscription *in, poll_event *out,
                                size_t count, size_t *events);

void rust_sqlite_wasm_host_sleep(uint64_t seconds, uint32_t nanoseconds) {
    /* Return early if the duration cannot be represented by a WASI timestamp. */
    if (seconds > (UINT64_MAX - nanoseconds) / 1000000000ULL) return;
    uint64_t timeout = seconds * 1000000000ULL + nanoseconds;
    if (timeout == 0) return;
    clock_subscription subscription = {
        .type = WASI_EVENT_CLOCK,
        .clock = { .id = WASI_CLOCK_MONOTONIC, .timeout = timeout }
    };
    poll_event event;
    size_t events;
    /* A failed wait may return early; never fall back to busy-waiting. */
    (void)wasi_poll_oneoff(&subscription, &event, 1, &events);
}

size_t rust_sqlite_wasm_host_random(uint8_t *buf, size_t len) {
    return rust_sqlite_wasm_host_fill_entropy(buf, len) == RUST_SQLITE_WASM_HOST_OK
        ? len : 0;
}

int32_t rust_sqlite_wasm_host_epoch_timestamp_in_ms(int64_t *out) {
    uint64_t nanos;
    if (wasi_clock_time_get(WASI_CLOCK_REALTIME, 1000000, &nanos) != WASI_OK)
        return RUST_SQLITE_WASM_HOST_UNAVAILABLE;
    *out = (int64_t)(nanos / 1000000);
    return RUST_SQLITE_WASM_HOST_OK;
}

int32_t rust_sqlite_wasm_host_fill_entropy(uint8_t *buf, size_t len) {
    if (len == 0) return RUST_SQLITE_WASM_HOST_OK;
    if (len > PTRDIFF_MAX || buf == NULL) return RUST_SQLITE_WASM_HOST_UNAVAILABLE;
    return wasi_random_get(buf, len) == WASI_OK
        ? RUST_SQLITE_WASM_HOST_OK : RUST_SQLITE_WASM_HOST_UNAVAILABLE;
}

int32_t rust_sqlite_wasm_host_localtime(int64_t unix_seconds,
                                      rust_sqlite_wasm_local_time *out) {
    /* WASI preview1 does not expose the host's local time zone. */
    (void)unix_seconds;
    (void)out;
    return RUST_SQLITE_WASM_HOST_UNSUPPORTED;
}
