//! Link-time host services, independent of the JavaScript binding toolchain.
//!
//! The default `wasm-bindgen` feature supplies all five hooks. To use another
//! host, disable default features and define these symbols exactly once in the
//! final application or a linked C/Rust adapter, using the C ABI:
//!
//! ```text
//! void rust_sqlite_wasm_host_sleep(uint64_t seconds, uint32_t nanoseconds);
//! size_t rust_sqlite_wasm_host_random(uint8_t *buf, size_t len);
//! int32_t rust_sqlite_wasm_host_epoch_timestamp_in_ms(int64_t *out);
//! int32_t rust_sqlite_wasm_host_fill_entropy(uint8_t *buf, size_t len);
//! int32_t rust_sqlite_wasm_host_localtime(int64_t unix_seconds, rust_sqlite_wasm_local_time *out);
//! ```
//!
//! The C declarations and layout are provided in
//! [`sqlite-wasm-rs.h`](https://github.com/Spxg/sqlite-wasm-rs/blob/master/sqlite-wasm-rs.h).
//! Rust adapters use
//! `#[unsafe(no_mangle)] pub unsafe extern "C" fn` with the corresponding raw
//! pointer and integer types. A runtime can also supply these as Wasm imports
//! from module `env` by explicitly allowing these undefined symbols at link time
//! (see `examples/host-js`). For a statically linked C adapter, see
//! `examples/host-c`. Pointers refer to
//! the module's linear memory; Wasm `i64` arguments reach JavaScript as `BigInt`.
//! Do not enable the default adapter anywhere in the dependency graph when
//! providing these symbols yourself (Cargo features are additive). Ensure a
//! Rust adapter crate is linked, e.g. with `use my_adapter as _;`.
//!
//! Hooks must not reenter SQLite. Calls follow SQLite's existing single-threaded
//! contract and may occur during initialization. Return errors instead of
//! panicking. No registration or mutable global host object is required.
//!
//! `sleep` and `random` follow [`rsqlite_vfs::OsCallback`]; the clock returns UTC
//! milliseconds since the Unix epoch. `fill_entropy` must fill the entire buffer
//! with cryptographically secure randomness or return an error, never using the
//! ordinary random hook as a weak fallback. SQLite3MC may abort if entropy fails.
//! `localtime` converts Unix seconds using the host's local time zone; returning
//! an error makes SQLite report local-time conversion failure, not UTC instead.
//!
//! Fallible hooks return [`OK`] or an [`Error`] discriminant as `i32`, not SQLite
//! result codes or errno. Unknown nonzero codes are treated as unavailable.
//! On success, output parameters must be fully written. Output pointers are
//! non-null, aligned and writable; buffers contain `len` writable bytes and may
//! be null only when `len` is zero. Buffers need not be initialized on entry.
//! Each buffer must lie within one allocation, with exclusive access during
//! the call and `len <= isize::MAX`. The default adapter rejects larger lengths
//! or null buffers with nonzero lengths before accessing memory: `random`
//! returns zero and `fill_entropy` returns [`Error::Unavailable`] as `i32`.
//! Hooks must not retain pointers after returning.
//! `random` initializes the first N bytes and returns N, at most `len`.
//! The sleep nanoseconds argument is the subsecond part, less than 1,000,000,000.

use core::fmt;
use core::time::Duration;
use rsqlite_vfs::{OsCallback, VfsError, VfsErrorCode, VfsResult};

#[cfg(feature = "wasm-bindgen")]
mod wasm_bindgen;

/// Successful completion of a fallible C ABI host hook.
pub const OK: i32 = 0;

/// A host capability is unsupported, unavailable, or returned an invalid time.
///
/// Return the discriminant as `i32` from a C ABI hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
#[non_exhaustive]
pub enum Error {
    Unsupported = 1,
    Unavailable = 2,
    InvalidTime = 3,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unsupported => "host capability is unsupported",
            Self::Unavailable => "host service is unavailable",
            Self::InvalidTime => "host returned an invalid time",
        })
    }
}

impl core::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;

/// C ABI local Gregorian calendar fields, matching `rust_sqlite_wasm_local_time`.
///
/// Ten consecutive `i32` fields (40 bytes, alignment 4 on wasm32), not libc's
/// `struct tm`. A zero-initialized value is not a valid calendar date.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct LocalTime {
    /// Full calendar year, not an offset from 1900.
    pub year: i32,
    /// Month in 1..=12.
    pub month: i32,
    /// Day of the month in 1..=31, valid for the month and year.
    pub day: i32,
    /// Hour in 0..=23.
    pub hour: i32,
    /// Minute in 0..=59.
    pub minute: i32,
    /// Second in 0..=60 (including a leap second).
    pub second: i32,
    /// Day of the week in 0..=6, with Sunday equal to zero.
    pub weekday: i32,
    /// Day of the year in 0..=365, with January 1 equal to zero.
    pub yearday: i32,
    /// Daylight saving time: 1 if active, 0 if inactive, -1 if unknown.
    pub is_dst: i32,
    /// Local time minus UTC, in seconds.
    pub utc_offset_seconds: i32,
}

mod ffi {
    use super::LocalTime;

    unsafe extern "C" {
        #[link_name = "rust_sqlite_wasm_host_sleep"]
        pub fn sleep(seconds: u64, nanoseconds: u32);
        #[link_name = "rust_sqlite_wasm_host_random"]
        pub fn random(buf: *mut u8, len: usize) -> usize;
        #[link_name = "rust_sqlite_wasm_host_epoch_timestamp_in_ms"]
        pub fn epoch_timestamp_in_ms(out: *mut i64) -> i32;
        #[link_name = "rust_sqlite_wasm_host_fill_entropy"]
        pub fn fill_entropy(buf: *mut u8, len: usize) -> i32;
        #[link_name = "rust_sqlite_wasm_host_localtime"]
        pub fn localtime(unix_seconds: i64, out: *mut LocalTime) -> i32;
    }
}

fn check_status(status: i32) -> Result<()> {
    match status {
        OK => Ok(()),
        value if value == Error::Unsupported as i32 => Err(Error::Unsupported),
        value if value == Error::InvalidTime as i32 => Err(Error::InvalidTime),
        _ => Err(Error::Unavailable),
    }
}

pub(crate) fn fill_entropy(buf: &mut [u8]) -> Result<()> {
    check_status(unsafe { ffi::fill_entropy(buf.as_mut_ptr(), buf.len()) })
}

pub(crate) fn localtime(unix_seconds: i64) -> Result<LocalTime> {
    let mut out = LocalTime::default();
    check_status(unsafe { ffi::localtime(unix_seconds, &mut out) })?;
    Ok(out)
}

/// VFS platform services supplied by the linked host adapter.
///
/// Uses wasm-bindgen by default, or application-defined hooks when that feature
/// is disabled. Does not make SQLite or the default memory VFS thread-safe.
#[derive(Default)]
pub struct WasmOsCallback;

impl OsCallback for WasmOsCallback {
    fn sleep(&self, duration: Duration) {
        unsafe { ffi::sleep(duration.as_secs(), duration.subsec_nanos()) }
    }

    fn random(&self, buf: &mut [u8]) -> usize {
        unsafe { ffi::random(buf.as_mut_ptr(), buf.len()) }
    }

    fn epoch_timestamp_in_ms(&self) -> VfsResult<i64> {
        let mut out = 0;
        check_status(unsafe { ffi::epoch_timestamp_in_ms(&mut out) }).map_err(|error| {
            VfsError::new(
                VfsErrorCode::Error,
                alloc::format!("host clock: {error}").into(),
            )
        })?;
        Ok(out)
    }
}
