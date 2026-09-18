//! Default JavaScript host adapter.

use super::{Error, LocalTime, OK, Result};
use core::time::Duration;
use js_sys::{Date, Math, Number};
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::wasm_bindgen;

#[unsafe(export_name = "rust_sqlite_wasm_host_sleep")]
pub extern "C" fn sleep(seconds: u64, nanoseconds: u32) {
    let duration = Duration::new(seconds, nanoseconds);
    #[cfg(target_feature = "atomics")]
    {
        let mut nanos = duration.as_nanos();
        while nanos > 0 {
            let amount = core::cmp::min(i64::MAX as u128, nanos);
            let mut word = 0;
            let status =
                unsafe { core::arch::wasm32::memory_atomic_wait32(&mut word, 0, amount as i64) };
            debug_assert_eq!(status, 2);
            nanos -= amount;
        }
    }
    // Browsers provide no synchronous sleep here without atomics. Do not busy-wait.
    #[cfg(not(target_feature = "atomics"))]
    let _ = duration;
}

#[unsafe(export_name = "rust_sqlite_wasm_host_random")]
pub unsafe extern "C" fn random(buf: *mut u8, len: usize) -> usize {
    let Ok(buf) = (unsafe { output_buffer(buf, len) }) else {
        return 0;
    };
    if fill_entropy_impl(buf).is_err() {
        // Preserve the non-cryptographic VFS fallback, never used for encryption.
        for byte in buf.iter_mut() {
            *byte = (Math::random() * 255000.0) as u32 as u8;
        }
    }
    buf.len()
}

#[unsafe(export_name = "rust_sqlite_wasm_host_epoch_timestamp_in_ms")]
pub unsafe extern "C" fn epoch_timestamp_in_ms(out: *mut i64) -> i32 {
    let milliseconds = Date::new_0().get_time();
    if milliseconds.is_finite() {
        unsafe { out.write(milliseconds as i64) };
        OK
    } else {
        Error::InvalidTime as i32
    }
}

#[wasm_bindgen]
extern "C" {
    #[cfg(not(target_feature = "atomics"))]
    #[wasm_bindgen(js_namespace = ["globalThis", "crypto"], js_name = getRandomValues, catch)]
    fn get_random_values(buf: &mut [u8]) -> core::result::Result<(), JsValue>;
    #[cfg(target_feature = "atomics")]
    #[wasm_bindgen(js_namespace = ["globalThis", "crypto"], js_name = getRandomValues, catch)]
    fn get_random_values(buf: &js_sys::Uint8Array) -> core::result::Result<(), JsValue>;
}

#[unsafe(export_name = "rust_sqlite_wasm_host_fill_entropy")]
pub unsafe extern "C" fn fill_entropy(buf: *mut u8, len: usize) -> i32 {
    match unsafe { output_buffer(buf, len) }.and_then(fill_entropy_impl) {
        Ok(()) => OK,
        Err(error) => error as i32,
    }
}

// C callers may supply uninitialized storage, or a null pointer for length zero.
// For accepted lengths, the caller must guarantee writable storage in a single
// allocation and exclusive access for the call. Reject invalid lengths before
// touching memory or constructing a slice.
unsafe fn output_buffer<'a>(buf: *mut u8, len: usize) -> Result<&'a mut [u8]> {
    if len == 0 {
        return Ok(&mut []);
    }
    if len > isize::MAX as usize || buf.is_null() {
        return Err(Error::Unavailable);
    }
    unsafe {
        buf.write_bytes(0, len);
        Ok(core::slice::from_raw_parts_mut(buf, len))
    }
}

fn fill_entropy_impl(buf: &mut [u8]) -> Result<()> {
    // Web Crypto limits each getRandomValues request to 65,536 bytes.
    for chunk in buf.chunks_mut(65_536) {
        #[cfg(not(target_feature = "atomics"))]
        get_random_values(chunk).map_err(|_| Error::Unavailable)?;

        #[cfg(target_feature = "atomics")]
        {
            // Web Crypto cannot fill a view backed by shared Wasm memory.
            let array = js_sys::Uint8Array::new_with_length(chunk.len() as u32);
            get_random_values(&array).map_err(|_| Error::Unavailable)?;
            array.copy_to(chunk);
        }
    }
    Ok(())
}

// Mirrors the existing Emscripten localtime handling, including DST logic.
#[unsafe(export_name = "rust_sqlite_wasm_host_localtime")]
pub unsafe extern "C" fn localtime(unix_seconds: i64, out: *mut LocalTime) -> i32 {
    let date = Date::new(&Number::from(unix_seconds as f64 * 1000.0).into());
    if !date.get_time().is_finite() {
        return Error::InvalidTime as i32;
    }
    let year = date.get_full_year();
    let summer_offset = Date::new_with_year_month_day(year, 6, 1).get_timezone_offset();
    let winter_offset = Date::new_with_year_month_day(year, 0, 1).get_timezone_offset();
    let offset = date.get_timezone_offset();
    let local = LocalTime {
        year: year as i32,
        month: (date.get_month() + 1) as i32,
        day: date.get_date() as i32,
        hour: date.get_hours() as i32,
        minute: date.get_minutes() as i32,
        second: date.get_seconds() as i32,
        weekday: date.get_day() as i32,
        yearday: yday_from_date(&date) as i32,
        is_dst: i32::from(
            summer_offset != winter_offset && offset == winter_offset.min(summer_offset),
        ),
        utc_offset_seconds: -(offset * 60.0) as i32,
    };
    unsafe { out.write(local) };
    OK
}

fn yday_from_date(date: &Date) -> u32 {
    const LEAP: [u32; 12] = [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];
    const REGULAR: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let year = date.get_full_year();
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = if leap { LEAP } else { REGULAR };
    days[date.get_month() as usize] + date.get_date() - 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn random_buffer_boundaries() {
        unsafe {
            assert_eq!(random(core::ptr::null_mut(), 0), 0);
            assert_eq!(fill_entropy(core::ptr::null_mut(), 0), OK);
            assert_eq!(random(core::ptr::null_mut(), 1), 0);
            assert_eq!(
                fill_entropy(core::ptr::null_mut(), 1),
                Error::Unavailable as i32
            );
            let mut byte = 42;
            for len in [isize::MAX as usize + 1, usize::MAX] {
                assert_eq!(random(&mut byte, len), 0);
                assert_eq!(fill_entropy(&mut byte, len), Error::Unavailable as i32);
                assert_eq!(byte, 42);
            }
        }
    }
}
