//! This module fills in the external functions needed to link to `sqlite.o`

use std::ffi::{c_char, c_double, c_int, c_void};
use std::ptr;

use js_sys::{Date, Number};
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::JsValue;

#[wasm_bindgen]
extern "C" {
    // crypto.getRandomValues()
    #[cfg(not(target_feature = "atomics"))]
    #[wasm_bindgen(js_namespace = ["globalThis", "crypto"], js_name = getRandomValues, catch)]
    fn get_random_values(buf: &mut [u8]) -> Result<(), JsValue>;
    #[cfg(target_feature = "atomics")]
    #[wasm_bindgen(js_namespace = ["globalThis", "crypto"], js_name = getRandomValues, catch)]
    fn get_random_values(buf: &js_sys::Uint8Array) -> Result<(), JsValue>;
}

type c_size_t = usize;
type c_time_t = std::os::raw::c_longlong;

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
unsafe fn localtime_js(t: c_time_t, tm: *mut tm) {
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

unsafe fn strspn_impl(s: *const c_char, c: *const c_char, reject: bool) -> c_size_t {
    let mut lookup_table = [false; 256];

    let mut list_ptr = c;
    while *list_ptr != 0 {
        lookup_table[*list_ptr as u8 as usize] = true;
        list_ptr = list_ptr.add(1);
    }

    let mut s_ptr = s;
    let mut count = 0;

    while *s_ptr != 0 {
        if lookup_table[*s_ptr as u8 as usize] == reject {
            break;
        }
        count += 1;
        s_ptr = s_ptr.add(1);
    }

    count
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/strcpy.html>.
#[cfg(feature = "sqlite3mc")]
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_strcpy(
    dest: *mut c_char,
    src: *const c_char,
) -> *mut c_char {
    let mut d = dest;
    let mut s = src;
    while *s != 0 {
        *d = *s;
        d = d.add(1);
        s = s.add(1);
    }
    *d = 0;
    dest
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/strncpy.html>.
#[cfg(feature = "sqlite3mc")]
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_strncpy(
    dest: *mut c_char,
    src: *const c_char,
    n: usize,
) -> *mut c_char {
    if n == 0 {
        return dest;
    }
    let mut d = dest;
    let mut s = src;
    let mut i = 0;

    while i < n && *s != 0 {
        *d = *s;
        d = d.add(1);
        s = s.add(1);
        i += 1;
    }

    while i < n {
        *d = 0;
        d = d.add(1);
        i += 1;
    }

    dest
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/strcat.html>.
#[cfg(feature = "sqlite3mc")]
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_strcat(
    dest: *mut c_char,
    src: *const c_char,
) -> *mut c_char {
    let mut d = dest;
    while *d != 0 {
        d = d.add(1);
    }

    let mut s = src;
    while *s != 0 {
        *d = *s;
        d = d.add(1);
        s = s.add(1);
    }
    *d = 0;

    dest
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/strncat.html>.
#[cfg(feature = "sqlite3mc")]
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_strncat(
    dest: *mut c_char,
    src: *const c_char,
    n: usize,
) -> *mut c_char {
    if n == 0 {
        return dest;
    }

    let mut d = dest;
    while *d != 0 {
        d = d.add(1);
    }

    let mut s = src;
    let mut i = 0;

    while i < n && *s != 0 {
        *d = *s;
        d = d.add(1);
        s = s.add(1);
        i += 1;
    }

    *d = 0;
    dest
}

/// https://github.com/emscripten-core/emscripten/blob/df69e2ccc287beab6f580f33b33e6b5692f5d20b/system/include/wasi/api.h#L2652
#[cfg(feature = "sqlite3mc")]
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_getentropy(
    buf: *mut u8,
    buf_len: usize,
) -> std::os::raw::c_ushort {
    // https://github.com/WebAssembly/wasi-libc/blob/e9524a0980b9bb6bb92e87a41ed1055bdda5bb86/libc-bottom-half/headers/public/wasi/api.h#L373
    const FUNCTION_NOT_SUPPORT: std::os::raw::c_ushort = 52;

    #[cfg(target_feature = "atomics")]
    {
        let array = js_sys::Uint8Array::new_with_length(buf_len as u32);
        if get_random_values(&array).is_err() {
            return FUNCTION_NOT_SUPPORT;
        }
        wasm_array_cp::ArrayBufferCopy::copy_to(
            &array,
            std::slice::from_raw_parts_mut(buf, buf_len),
        );
    }

    #[cfg(not(target_feature = "atomics"))]
    if get_random_values(std::slice::from_raw_parts_mut(buf, buf_len)).is_err() {
        return FUNCTION_NOT_SUPPORT;
    }

    0
}

#[cfg(feature = "sqlite3mc")]
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_abort() {
    std::process::abort();
}

// https://github.com/emscripten-core/emscripten/blob/089590d17eeb705424bf32f8a1afe34a034b4682/system/lib/libc/musl/src/errno/__errno_location.c#L10
#[cfg(feature = "sqlite3mc")]
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_errno_location() -> *mut c_int {
    thread_local! {
        static ERROR_STORAGE: std::cell::UnsafeCell<i32> = std::cell::UnsafeCell::new(0);
    }
    ERROR_STORAGE.with(|e| e.get())
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/strcmp.html>.
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_strcmp(
    s1: *const c_char,
    s2: *const c_char,
) -> c_int {
    let mut i = 0;
    loop {
        let c1 = *s1.add(i);
        let c2 = *s2.add(i);

        if c1 != c2 || c1 == 0 {
            return (c1 as c_int) - (c2 as c_int);
        }

        i += 1;
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/strncmp.html>.
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_strncmp(
    s1: *const c_char,
    s2: *const c_char,
    n: c_size_t,
) -> c_int {
    for i in 0..n {
        let c1 = *s1.add(i);
        let c2 = *s2.add(i);

        if c1 != c2 || c1 == 0 {
            return (c1 as c_int) - (c2 as c_int);
        }
    }
    0
}

// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/strcspn.html>.
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_strcspn(
    s: *const c_char,
    reject: *const c_char,
) -> c_size_t {
    strspn_impl(s, reject, true)
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/strspn.html>.
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_strspn(
    s: *const c_char,
    accept: *const c_char,
) -> usize {
    strspn_impl(s, accept, false)
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/strrchr.html>.
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_strrchr(
    s: *const c_char,
    c: c_int,
) -> *const c_char {
    let c = c as u8 as c_char;
    let mut ptr = s;
    let mut last = ptr::null();

    while *ptr != 0 {
        if *ptr == c {
            last = ptr;
        }
        ptr = ptr.add(1);
    }

    if c == 0 {
        return ptr;
    }

    last
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/strchr.html>.
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_strchr(s: *const c_char, c: c_int) -> *const c_char {
    let ch = c as u8 as c_char;
    let mut ptr = s;

    while *ptr != 0 {
        if *ptr == ch {
            return ptr;
        }
        ptr = ptr.add(1);
    }

    if ch == 0 {
        return ptr;
    }

    ptr::null()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/memchr.html>.
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_memchr(
    s: *const c_void,
    c: c_int,
    n: c_size_t,
) -> *const c_void {
    let s_ptr = s as *const u8;
    let c = c as u8;

    for i in 0..n {
        if *s_ptr.add(i) == c {
            return s_ptr.add(i) as *const c_void;
        }
    }

    ptr::null()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/acosh.html>.
#[no_mangle]
pub extern "C" fn rust_sqlite_wasm_shim_acosh(x: c_double) -> c_double {
    x.acosh()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/asinh.html>.
#[no_mangle]
pub extern "C" fn rust_sqlite_wasm_shim_asinh(x: c_double) -> c_double {
    x.asinh()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/atanh.html>.
#[no_mangle]
pub extern "C" fn rust_sqlite_wasm_shim_atanh(x: c_double) -> c_double {
    x.atanh()
}

/// See <https://github.com/emscripten-core/emscripten/blob/089590d17eeb705424bf32f8a1afe34a034b4682/system/lib/libc/mktime.c#L28>.
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_localtime(t: *const c_time_t) -> *mut tm {
    static mut TM: tm = tm {
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
    localtime_js(*t, std::ptr::addr_of_mut!(TM));
    std::ptr::addr_of_mut!(TM)
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

#[cfg(test)]
mod tests {
    use std::ffi::CStr;

    use crate::{
        sqlite3_column_count, sqlite3_column_name, sqlite3_column_text, sqlite3_column_type,
        sqlite3_open, sqlite3_prepare_v3, sqlite3_step, SQLITE_OK, SQLITE_ROW, SQLITE_TEXT,
    };

    #[cfg(feature = "sqlite3mc")]
    use super::rust_sqlite_wasm_shim_getentropy;
    use super::{
        localtime_js, rust_sqlite_wasm_shim_calloc, rust_sqlite_wasm_shim_free,
        rust_sqlite_wasm_shim_malloc, rust_sqlite_wasm_shim_realloc, tm,
    };
    use wasm_bindgen_test::{console_log, wasm_bindgen_test};

    #[cfg(feature = "sqlite3mc")]
    #[wasm_bindgen_test]
    fn test_random_get() {
        let mut buf = [0u8; 10];
        unsafe { rust_sqlite_wasm_shim_getentropy(buf.as_mut_ptr(), buf.len()) };
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
            assert_eq!(ret, SQLITE_OK);
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
            localtime_js(1733976732, &mut tm as *mut tm);
        };
        let gmtoff = tm.tm_gmtoff / 3600;

        assert_eq!(tm.tm_year, 2024 - 1900);
        assert_eq!(tm.tm_mon, 12 - 1);
        assert_eq!(tm.tm_mday, 12);
        assert_eq!(tm.tm_hour as std::os::raw::c_long, 12 - 8 + gmtoff);
        assert_eq!(tm.tm_min, 12);
        assert_eq!(tm.tm_sec, 12);
        assert_eq!(tm.tm_wday, 4);
        assert_eq!(tm.tm_yday, 346);
    }
}
