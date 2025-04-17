//! This module fills in the external functions needed to link to `sqlite.o`

use js_sys::{Date, Number};
use wasm_bindgen::JsCast;
use web_sys::{ServiceWorkerGlobalScope, SharedWorkerGlobalScope, WorkerGlobalScope};

pub type time_t = std::os::raw::c_longlong;

/// https://github.com/emscripten-core/emscripten/blob/df69e2ccc287beab6f580f33b33e6b5692f5d20b/system/lib/libc/musl/include/time.h#L40
#[repr(C)]
pub struct tm {
    pub tm_sec: std::os::raw::c_int,
    pub tm_min: std::os::raw::c_int,
    pub tm_hour: std::os::raw::c_int,
    pub tm_mday: std::os::raw::c_int,
    pub tm_mon: std::os::raw::c_int,
    pub tm_year: std::os::raw::c_int,
    pub tm_wday: std::os::raw::c_int,
    pub tm_yday: std::os::raw::c_int,
    pub tm_isdst: std::os::raw::c_int,
    pub tm_gmtoff: std::os::raw::c_long,
    pub tm_zone: *mut std::os::raw::c_char,
}

fn yday_from_date(date: &Date) -> u32 {
    const MONTH_DAYS_LEAP_CUMULATIVE: [u32; 12] =
        [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];

    const MONTH_DAYS_REGULAR_CUMULATIVE: [u32; 12] =
        [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];

    let year = date.get_full_year();
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);

    let month_days_cumulative = if leap {
        MONTH_DAYS_LEAP_CUMULATIVE
    } else {
        MONTH_DAYS_REGULAR_CUMULATIVE
    };
    month_days_cumulative[date.get_month() as usize] + date.get_date() - 1
}

/// https://github.com/emscripten-core/emscripten/blob/df69e2ccc287beab6f580f33b33e6b5692f5d20b/system/lib/libc/emscripten_internal.h#L42
///
/// https://github.com/sqlite/sqlite-wasm/blob/7c1b309c3bd07d8e6d92f82344108cebbd14f161/sqlite-wasm/jswasm/sqlite3-bundler-friendly.mjs#L3404
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_localtime_js(t: time_t, tm: *mut tm) {
    let date = Date::new(&Number::from((t * 1000) as f64).into());

    (*tm).tm_sec = date.get_seconds() as _;
    (*tm).tm_min = date.get_minutes() as _;
    (*tm).tm_hour = date.get_hours() as _;
    (*tm).tm_mday = date.get_date() as _;
    (*tm).tm_mon = date.get_month() as _;
    (*tm).tm_year = (date.get_full_year() - 1900) as _;
    (*tm).tm_wday = date.get_day() as _;
    (*tm).tm_yday = yday_from_date(&date) as _;

    let start = Date::new_with_year_month_day(date.get_full_year(), 0, 1);
    let summer_offset =
        Date::new_with_year_month_day(date.get_full_year(), 6, 1).get_timezone_offset();
    let winter_offset = start.get_timezone_offset();
    (*tm).tm_isdst = i32::from(
        summer_offset != winter_offset
            && date.get_timezone_offset() == winter_offset.min(summer_offset),
    );

    (*tm).tm_gmtoff = -(date.get_timezone_offset() * 60.0) as _;
}

/// https://github.com/emscripten-core/emscripten/blob/df69e2ccc287beab6f580f33b33e6b5692f5d20b/system/lib/libc/emscripten_internal.h#L45
///
/// https://github.com/sqlite/sqlite-wasm/blob/7c1b309c3bd07d8e6d92f82344108cebbd14f161/sqlite-wasm/jswasm/sqlite3-bundler-friendly.mjs#L3460
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_tzset_js(
    timezone: *mut std::os::raw::c_long,
    daylight: *mut std::os::raw::c_int,
    std_name: *mut std::os::raw::c_char,
    dst_name: *mut std::os::raw::c_char,
) {
    unsafe fn set_name(name: String, dst: *mut std::os::raw::c_char) {
        for (idx, byte) in name.bytes().enumerate() {
            *dst.add(idx) = byte as _;
        }
        *dst.add(name.len()) = 0;
    }

    fn extract_zone(timezone_offset: f64) -> String {
        let sign = if timezone_offset >= 0.0 { '-' } else { '+' };
        let offset = timezone_offset.abs();
        let hours = format!("{:02}", (offset / 60.0).floor() as i32);
        let minutes = format!("{:02}", (offset % 60.0) as i32);
        format!("UTC{sign}{hours}{minutes}")
    }

    let current_year = Date::new_0().get_full_year();
    let winter = Date::new_with_year_month_day(current_year, 0, 1);
    let summer = Date::new_with_year_month_day(current_year, 6, 1);
    let winter_offset = winter.get_timezone_offset();
    let summer_offset = summer.get_timezone_offset();

    let std_timezone_offset = winter_offset.max(summer_offset);
    *timezone = (std_timezone_offset * 60.0) as _;
    *daylight = i32::from(winter_offset != summer_offset);

    let winter_name = extract_zone(winter_offset);
    let summer_name = extract_zone(summer_offset);

    if summer_offset < winter_offset {
        set_name(winter_name, std_name);
        set_name(summer_name, dst_name);
    } else {
        set_name(winter_name, dst_name);
        set_name(summer_name, std_name);
    }
}

/// https://github.com/sqlite/sqlite-wasm/blob/7c1b309c3bd07d8e6d92f82344108cebbd14f161/sqlite-wasm/jswasm/sqlite3-bundler-friendly.mjs#L3496
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_emscripten_get_now() -> std::os::raw::c_double {
    let performance = if let Some(window) = web_sys::window() {
        window.performance()
    } else if let Ok(worker) = js_sys::global().dyn_into::<WorkerGlobalScope>() {
        worker.performance()
    } else if let Ok(worker) = js_sys::global().dyn_into::<SharedWorkerGlobalScope>() {
        worker.performance()
    } else if let Ok(worker) = js_sys::global().dyn_into::<ServiceWorkerGlobalScope>() {
        worker.performance()
    } else {
        panic!("unsupported operating environment");
    }
    .expect("performance should be available");
    performance.now()
}

/// https://github.com/emscripten-core/emscripten/blob/df69e2ccc287beab6f580f33b33e6b5692f5d20b/system/include/wasi/api.h#L2652
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_wasi_random_get(
    buf: *mut u8,
    buf_len: usize,
) -> std::os::raw::c_ushort {
    let crypto = if let Some(window) = web_sys::window() {
        window.crypto()
    } else if let Ok(worker) = js_sys::global().dyn_into::<WorkerGlobalScope>() {
        worker.crypto()
    } else if let Ok(worker) = js_sys::global().dyn_into::<SharedWorkerGlobalScope>() {
        worker.crypto()
    } else if let Ok(worker) = js_sys::global().dyn_into::<ServiceWorkerGlobalScope>() {
        worker.crypto()
    } else {
        panic!("unsupported operating environment");
    }
    .expect("crypto should be available");

    #[cfg(target_feature = "atomics")]
    {
        let array = js_sys::Uint8Array::new_with_length(buf_len as u32);
        crypto
            // The provided ArrayBufferView value must not be shared.
            .get_random_values_with_js_u8_array(&array)
            // https://developer.mozilla.org/en-US/docs/Web/API/Crypto/getRandomValues#exceptions
            .expect("buffer size exceeds 65,536.");
        crate::utils::copy_to_slice(&array, std::slice::from_raw_parts_mut(buf, buf_len));
    }

    #[cfg(not(target_feature = "atomics"))]
    crypto
        .get_random_values_with_u8_array(std::slice::from_raw_parts_mut(buf, buf_len))
        // https://developer.mozilla.org/en-US/docs/Web/API/Crypto/getRandomValues#exceptions
        .expect("buffer size exceeds 65,536.");

    0
}

/// https://github.com/emscripten-core/emscripten/blob/df69e2ccc287beab6f580f33b33e6b5692f5d20b/system/lib/libc/musl/src/exit/exit.c#L27
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_exit(code: std::os::raw::c_int) {
    panic!("{}", format!("wasm exit, code: {code}"));
}

/// https://github.com/emscripten-core/emscripten/blob/df69e2ccc287beab6f580f33b33e6b5692f5d20b/system/lib/libc/emscripten_internal.h#L29
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_abort_js() {
    panic!("{}", format!("wasm abort"));
}

// https://github.com/alexcrichton/dlmalloc-rs/blob/fb116603713825b43b113cc734bb7d663cb64be9/src/dlmalloc.rs#L141
const ALIGN: usize = std::mem::size_of::<usize>() * 2;

#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_malloc(size: usize) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align_unchecked(size + ALIGN, ALIGN);
    let ptr = std::alloc::alloc(layout);

    if ptr.is_null() {
        return std::ptr::null_mut();
    }
    *ptr.cast::<usize>() = size;

    ptr.add(ALIGN)
}

#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_free(ptr: *mut u8) {
    let ptr = ptr.sub(ALIGN);
    let size = *(ptr.cast::<usize>());

    let layout = std::alloc::Layout::from_size_align_unchecked(size + ALIGN, ALIGN);
    std::alloc::dealloc(ptr, layout);
}

#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_realloc(ptr: *mut u8, new_size: usize) -> *mut u8 {
    let ptr = ptr.sub(ALIGN);
    let size = *(ptr.cast::<usize>());

    let layout = std::alloc::Layout::from_size_align_unchecked(size + ALIGN, ALIGN);
    let ptr = std::alloc::realloc(ptr, layout, new_size + ALIGN);

    if ptr.is_null() {
        return std::ptr::null_mut();
    }
    *ptr.cast::<usize>() = new_size;

    ptr.add(ALIGN)
}

#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_calloc(num: usize, size: usize) -> *mut u8 {
    let total = num * size;
    let ptr = rust_sqlite_wasm_shim_malloc(total);
    if !ptr.is_null() {
        std::ptr::write_bytes(ptr, 0, total);
    }
    ptr
}

#[no_mangle]
pub unsafe extern "C" fn sqlite3_os_init() -> std::os::raw::c_int {
    super::vfs::memory::install()
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;

    use crate::{
        sqlite3_column_count, sqlite3_column_name, sqlite3_column_text, sqlite3_column_type,
        sqlite3_open, sqlite3_prepare_v3, sqlite3_step, SQLITE_OK, SQLITE_ROW, SQLITE_TEXT,
    };

    use super::{
        rust_sqlite_wasm_shim_calloc, rust_sqlite_wasm_shim_emscripten_get_now,
        rust_sqlite_wasm_shim_free, rust_sqlite_wasm_shim_localtime_js,
        rust_sqlite_wasm_shim_malloc, rust_sqlite_wasm_shim_realloc,
        rust_sqlite_wasm_shim_tzset_js, rust_sqlite_wasm_shim_wasi_random_get, tm,
    };
    use wasm_bindgen_test::{console_log, wasm_bindgen_test};

    #[wasm_bindgen_test]
    fn test_random_get() {
        let mut buf = [0u8; 10];
        unsafe { rust_sqlite_wasm_shim_wasi_random_get(buf.as_mut_ptr(), buf.len()) };
        console_log!("test_random_get: {buf:?}");
    }

    #[wasm_bindgen_test]
    fn test_memory() {
        unsafe {
            let ptr1 = rust_sqlite_wasm_shim_malloc(10);
            let ptr2 = rust_sqlite_wasm_shim_realloc(ptr1, 100);
            rust_sqlite_wasm_shim_free(ptr2);
            console_log!("test_memory: {ptr1:?} {ptr2:?}");

            let ptr = rust_sqlite_wasm_shim_calloc(2, 8);
            let buf = std::slice::from_raw_parts(ptr, 2 * 8);

            assert!(buf.iter().all(|&x| x == 0));
        }
    }

    #[wasm_bindgen_test]
    fn test_get_now() {
        let now = unsafe { rust_sqlite_wasm_shim_emscripten_get_now() };
        console_log!("test_get_now: {now}");
    }

    #[wasm_bindgen_test]
    fn test_tzset() {
        let mut timezone: std::os::raw::c_long = 0;
        let mut daylight: std::os::raw::c_int = 0;
        let mut std_name = [0i8; 9];
        let mut dst_name = [0i8; 9];

        unsafe {
            rust_sqlite_wasm_shim_tzset_js(
                &mut timezone as _,
                &mut daylight as _,
                std_name.as_mut_ptr(),
                dst_name.as_mut_ptr(),
            );
        }

        let std_name = unsafe { CStr::from_ptr(std_name.as_ptr()) };
        let dst_name = unsafe { CStr::from_ptr(dst_name.as_ptr()) };

        console_log!("test_tzset: {timezone} {daylight} {std_name:?} {dst_name:?}");
    }

    #[wasm_bindgen_test]
    fn test_localtime_sqlite() {
        unsafe {
            let mut db = std::ptr::null_mut();
            let ret = sqlite3_open(c":memory:".as_ptr().cast(), &mut db as *mut _);
            assert_eq!(ret, SQLITE_OK);
            let mut stmt = std::ptr::null_mut();
            let ret = sqlite3_prepare_v3(
                db,
                c"SELECT datetime('now', 'localtime');".as_ptr().cast(),
                -1,
                0,
                &mut stmt as *mut _,
                std::ptr::null_mut(),
            );
            while sqlite3_step(stmt) == SQLITE_ROW {
                let count = sqlite3_column_count(stmt);
                for col in 0..count {
                    let name = sqlite3_column_name(stmt, col);
                    let ty = sqlite3_column_type(stmt, col);
                    assert_eq!(ty, SQLITE_TEXT);
                    console_log!(
                        "col {:?}, time: {:?}",
                        CStr::from_ptr(name),
                        CStr::from_ptr(sqlite3_column_text(stmt, col).cast())
                    );
                }
            }
            assert_eq!(ret, SQLITE_OK);
        }
    }

    #[wasm_bindgen_test]
    fn test_localtime() {
        let mut tm = tm {
            tm_sec: 0,
            tm_min: 0,
            tm_hour: 0,
            tm_mday: 0,
            tm_mon: 0,
            tm_year: 0,
            tm_wday: 0,
            tm_yday: 0,
            tm_isdst: 0,
            tm_gmtoff: 0,
            tm_zone: std::ptr::null_mut(),
        };
        unsafe {
            rust_sqlite_wasm_shim_localtime_js(1733976732, &mut tm as *mut tm);
        };

        let gmtoff = tm.tm_gmtoff / 3600;
        assert_eq!(tm.tm_year, 2024 - 1900);
        assert_eq!(tm.tm_mon, 12 - 1);
        assert_eq!(tm.tm_mday, 12);
        assert_eq!(tm.tm_hour, 12 - 8 + gmtoff);
        assert_eq!(tm.tm_min, 12);
        assert_eq!(tm.tm_sec, 12);
        assert_eq!(tm.tm_wday, 4);
        assert_eq!(tm.tm_yday, 346);
    }
}
