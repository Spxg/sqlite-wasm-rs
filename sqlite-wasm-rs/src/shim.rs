//! This module fills in the external functions needed to link to `sqlite.o`

use std::ffi::{c_char, c_double, c_int, c_long, c_longlong, c_void};
use std::{alloc, ptr};

use js_sys::{Date, Number};
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::JsValue;

type c_size_t = usize;
type c_time_t = c_longlong;

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
    struct BitSet([u128; 2]);

    impl BitSet {
        pub fn set(&mut self, idx: usize) {
            if idx < 128 {
                self.0[0] |= 1 << idx;
            } else {
                self.0[1] |= 1 << (idx - 128);
            }
        }

        pub fn contains(&self, idx: usize) -> bool {
            if idx < 128 {
                (self.0[0] & (1 << idx)) != 0
            } else {
                (self.0[1] & (1 << (idx - 128))) != 0
            }
        }
    }

    let mut bit_set = BitSet([0; 2]);

    let mut c_ptr = c;
    while *c_ptr != 0 {
        bit_set.set(*c_ptr as u8 as usize);
        c_ptr = c_ptr.add(1);
    }

    let mut s_ptr = s;
    let mut count = 0;

    while *s_ptr != 0 {
        if bit_set.contains(*s_ptr as u8 as usize) == reject {
            break;
        }
        count += 1;
        s_ptr = s_ptr.add(1);
    }

    count
}

/// https://github.com/emscripten-core/emscripten/blob/df69e2ccc287beab6f580f33b33e6b5692f5d20b/system/lib/libc/musl/include/time.h#L40
#[repr(C)]
pub struct tm {
    pub tm_sec: c_int,
    pub tm_min: c_int,
    pub tm_hour: c_int,
    pub tm_mday: c_int,
    pub tm_mon: c_int,
    pub tm_year: c_int,
    pub tm_wday: c_int,
    pub tm_yday: c_int,
    pub tm_isdst: c_int,
    pub tm_gmtoff: c_long,
    pub tm_zone: *mut c_char,
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
    buf_len: c_size_t,
) -> std::ffi::c_ushort {
    // https://github.com/WebAssembly/wasi-libc/blob/e9524a0980b9bb6bb92e87a41ed1055bdda5bb86/libc-bottom-half/headers/public/wasi/api.h#L373
    const FUNCTION_NOT_SUPPORT: std::ffi::c_ushort = 52;

    #[cfg(target_feature = "atomics")]
    {
        let array = js_sys::Uint8Array::new_with_length(buf_len as u32);
        if get_random_values(&array).is_err() {
            return FUNCTION_NOT_SUPPORT;
        }
        array.copy_to(std::slice::from_raw_parts_mut(buf, buf_len));
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

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/trunc.html>.
#[no_mangle]
pub extern "C" fn rust_sqlite_wasm_shim_trunc(x: c_double) -> c_double {
    x.trunc()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sqrt.html>.
#[no_mangle]
pub extern "C" fn rust_sqlite_wasm_shim_sqrt(x: c_double) -> c_double {
    x.sqrt()
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
        tm_zone: ptr::null_mut(),
    };
    localtime_js(*t, ptr::addr_of_mut!(TM));
    ptr::addr_of_mut!(TM)
}

// https://github.com/alexcrichton/dlmalloc-rs/blob/fb116603713825b43b113cc734bb7d663cb64be9/src/dlmalloc.rs#L141
const ALIGN: usize = std::mem::size_of::<usize>() * 2;

#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_malloc(size: c_size_t) -> *mut c_void {
    let layout = alloc::Layout::from_size_align_unchecked(size + ALIGN, ALIGN);
    let ptr = alloc::alloc(layout);

    if ptr.is_null() {
        return ptr::null_mut();
    }
    *ptr.cast::<usize>() = size;

    ptr.add(ALIGN).cast()
}

#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_free(ptr: *mut c_void) {
    let ptr: *mut u8 = ptr.sub(ALIGN).cast();
    let size = *(ptr.cast::<usize>());

    let layout = alloc::Layout::from_size_align_unchecked(size + ALIGN, ALIGN);
    alloc::dealloc(ptr, layout);
}

#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_realloc(
    ptr: *mut c_void,
    new_size: c_size_t,
) -> *mut c_void {
    let ptr: *mut u8 = ptr.sub(ALIGN).cast();
    let size = *(ptr.cast::<usize>());

    let layout = alloc::Layout::from_size_align_unchecked(size + ALIGN, ALIGN);
    let ptr = alloc::realloc(ptr, layout, new_size + ALIGN);

    if ptr.is_null() {
        return ptr::null_mut();
    }
    *ptr.cast::<usize>() = new_size;

    ptr.add(ALIGN).cast()
}

#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_shim_calloc(
    num: c_size_t,
    size: c_size_t,
) -> *mut c_void {
    let total = num * size;
    let ptr: *mut u8 = rust_sqlite_wasm_shim_malloc(total).cast();
    if !ptr.is_null() {
        ptr::write_bytes(ptr, 0, total);
    }
    ptr.cast()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;
    use std::ffi::CString;

    use crate::{
        sqlite3_column_count, sqlite3_column_name, sqlite3_column_text, sqlite3_column_type,
        sqlite3_open, sqlite3_prepare_v3, sqlite3_step, SQLITE_OK, SQLITE_ROW, SQLITE_TEXT,
    };

    use wasm_bindgen_test::{console_log, wasm_bindgen_test};

    #[cfg(feature = "sqlite3mc")]
    #[wasm_bindgen_test]
    fn test_random_get() {
        let mut buf = [0u8; 10];
        unsafe { rust_sqlite_wasm_shim_getentropy(buf.as_mut_ptr(), buf.len()) };
        console_log!("test_random_get: {buf:?}");
    }

    fn to_cstring(s: &str) -> CString {
        CString::new(s).unwrap()
    }

    #[wasm_bindgen_test]
    fn test_memory() {
        unsafe {
            let ptr1 = rust_sqlite_wasm_shim_malloc(10);
            let ptr2 = rust_sqlite_wasm_shim_realloc(ptr1, 100);
            rust_sqlite_wasm_shim_free(ptr2);
            console_log!("test_memory: {ptr1:?} {ptr2:?}");

            let ptr: *mut u8 = rust_sqlite_wasm_shim_calloc(2, 8).cast();
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

    #[wasm_bindgen_test]
    fn test_rust_sqlite_wasm_shim_strcmp() {
        unsafe {
            let s1 = to_cstring("hello");
            let s2 = to_cstring("hello");
            assert_eq!(rust_sqlite_wasm_shim_strcmp(s1.as_ptr(), s2.as_ptr()), 0);

            let s1 = to_cstring("apple");
            let s2 = to_cstring("banana");
            assert!(rust_sqlite_wasm_shim_strcmp(s1.as_ptr(), s2.as_ptr()) < 0);

            let s1 = to_cstring("zebra");
            let s2 = to_cstring("apple");
            assert!(rust_sqlite_wasm_shim_strcmp(s1.as_ptr(), s2.as_ptr()) > 0);

            let s1 = to_cstring("");
            let s2 = to_cstring("");
            assert_eq!(rust_sqlite_wasm_shim_strcmp(s1.as_ptr(), s2.as_ptr()), 0);

            let s1 = to_cstring("");
            let s2 = to_cstring("test");
            assert!(rust_sqlite_wasm_shim_strcmp(s1.as_ptr(), s2.as_ptr()) < 0);
        }
    }

    #[wasm_bindgen_test]
    fn test_rust_sqlite_wasm_shim_strncmp() {
        unsafe {
            let s1 = to_cstring("hello");
            let s2 = to_cstring("hello");
            assert_eq!(
                rust_sqlite_wasm_shim_strncmp(s1.as_ptr(), s2.as_ptr(), 5),
                0
            );

            let s1 = to_cstring("hello world");
            let s2 = to_cstring("hello there");
            assert_eq!(
                rust_sqlite_wasm_shim_strncmp(s1.as_ptr(), s2.as_ptr(), 5),
                0
            );

            let s1 = to_cstring("apple");
            let s2 = to_cstring("banana");
            assert!(rust_sqlite_wasm_shim_strncmp(s1.as_ptr(), s2.as_ptr(), 3) < 0);

            let s1 = to_cstring("abc");
            let s2 = to_cstring("def");
            assert_eq!(
                rust_sqlite_wasm_shim_strncmp(s1.as_ptr(), s2.as_ptr(), 0),
                0
            );

            let s1 = to_cstring("hi");
            let s2 = to_cstring("hi");
            assert_eq!(
                rust_sqlite_wasm_shim_strncmp(s1.as_ptr(), s2.as_ptr(), 10),
                0
            );
        }
    }

    #[wasm_bindgen_test]
    fn test_rust_sqlite_wasm_shim_strchr() {
        unsafe {
            let s = to_cstring("hello world");

            let result = rust_sqlite_wasm_shim_strchr(s.as_ptr(), b'e' as c_int);
            assert!(!result.is_null());
            assert_eq!(*result, b'e' as c_char);

            let result = rust_sqlite_wasm_shim_strchr(s.as_ptr(), b'x' as c_int);
            assert!(result.is_null());

            let result = rust_sqlite_wasm_shim_strchr(s.as_ptr(), 0);
            assert!(!result.is_null());
            assert_eq!(*result, 0);

            let result = rust_sqlite_wasm_shim_strchr(s.as_ptr(), b'h' as c_int);
            assert!(!result.is_null());
            assert_eq!(*result, b'h' as c_char);
        }
    }

    #[wasm_bindgen_test]
    fn test_rust_sqlite_wasm_shim_strrchr() {
        unsafe {
            let s = to_cstring("hello world");

            let result = rust_sqlite_wasm_shim_strrchr(s.as_ptr(), b'l' as c_int);
            assert!(!result.is_null());
            assert_eq!(*result, b'l' as c_char);

            let first_l = rust_sqlite_wasm_shim_strchr(s.as_ptr(), b'l' as c_int);
            let last_l = rust_sqlite_wasm_shim_strrchr(s.as_ptr(), b'l' as c_int);
            assert!(last_l as usize > first_l as usize);

            let result = rust_sqlite_wasm_shim_strrchr(s.as_ptr(), b'x' as c_int);
            assert!(result.is_null());

            let result = rust_sqlite_wasm_shim_strrchr(s.as_ptr(), 0);
            assert!(!result.is_null());
            assert_eq!(*result, 0);
        }
    }

    #[wasm_bindgen_test]
    fn test_rust_sqlite_wasm_shim_strcspn() {
        unsafe {
            let s = to_cstring("hello world");
            let reject = to_cstring("aeiou");

            let result = rust_sqlite_wasm_shim_strcspn(s.as_ptr(), reject.as_ptr());
            assert_eq!(result, 1);

            let reject2 = to_cstring("xyz");
            let result = rust_sqlite_wasm_shim_strcspn(s.as_ptr(), reject2.as_ptr());
            assert_eq!(result, 11);

            let reject3 = to_cstring("h");
            let result = rust_sqlite_wasm_shim_strcspn(s.as_ptr(), reject3.as_ptr());
            assert_eq!(result, 0);

            let empty = to_cstring("");
            let result = rust_sqlite_wasm_shim_strcspn(empty.as_ptr(), reject.as_ptr());
            assert_eq!(result, 0);
        }
    }

    #[wasm_bindgen_test]
    fn test_rust_sqlite_wasm_shim_strspn() {
        unsafe {
            let s = to_cstring("hello world");
            let accept = to_cstring("helo");

            let result = rust_sqlite_wasm_shim_strspn(s.as_ptr(), accept.as_ptr());
            assert_eq!(result, 5);

            let accept2 = to_cstring("xyz");
            let result = rust_sqlite_wasm_shim_strspn(s.as_ptr(), accept2.as_ptr());
            assert_eq!(result, 0);

            let accept3 = to_cstring("helo wrd");
            let result = rust_sqlite_wasm_shim_strspn(s.as_ptr(), accept3.as_ptr());
            assert_eq!(result, 11);

            let empty = to_cstring("");
            let result = rust_sqlite_wasm_shim_strspn(empty.as_ptr(), accept.as_ptr());
            assert_eq!(result, 0);
        }
    }

    #[wasm_bindgen_test]
    fn test_rust_sqlite_wasm_shim_memchr() {
        unsafe {
            let data = b"hello world";
            let c = b'o' as c_int;

            let result = rust_sqlite_wasm_shim_memchr(data.as_ptr() as *const c_void, c, 11);
            assert!(!result.is_null());
            let found_char = *(result as *const u8);
            assert_eq!(found_char, b'o');

            let result =
                rust_sqlite_wasm_shim_memchr(data.as_ptr() as *const c_void, b'x' as c_int, 11);
            assert!(result.is_null());

            let result =
                rust_sqlite_wasm_shim_memchr(data.as_ptr() as *const c_void, b'o' as c_int, 4);
            assert!(result.is_null());

            let result = rust_sqlite_wasm_shim_memchr(ptr::null(), c, 0);
            assert!(result.is_null());
        }
    }

    #[wasm_bindgen_test]
    #[cfg(feature = "sqlite3mc")]
    fn test_rust_sqlite_wasm_shim_strcpy() {
        unsafe {
            let src = to_cstring("hello world");
            let mut dest = [0u8; 20];

            let result =
                rust_sqlite_wasm_shim_strcpy(dest.as_mut_ptr() as *mut c_char, src.as_ptr());

            assert_eq!(result as *const u8, dest.as_ptr());

            let copied_str = CStr::from_ptr(dest.as_ptr() as *const c_char);
            assert_eq!(copied_str.to_str().unwrap(), "hello world");

            let empty_src = to_cstring("");
            let mut dest2 = [0u8; 10];
            rust_sqlite_wasm_shim_strcpy(dest2.as_mut_ptr() as *mut c_char, empty_src.as_ptr());
            let copied_empty = CStr::from_ptr(dest2.as_ptr() as *const c_char);
            assert_eq!(copied_empty.to_str().unwrap(), "");
        }
    }

    #[cfg(feature = "sqlite3mc")]
    #[wasm_bindgen_test]
    fn test_rust_sqlite_wasm_shim_strncpy() {
        unsafe {
            let src = to_cstring("hello");
            let mut dest = [0u8; 10];

            let result =
                rust_sqlite_wasm_shim_strncpy(dest.as_mut_ptr() as *mut c_char, src.as_ptr(), 6);
            assert_eq!(result as *const u8, dest.as_ptr());

            assert_eq!(dest[0], b'h');
            assert_eq!(dest[1], b'e');
            assert_eq!(dest[2], b'l');
            assert_eq!(dest[3], b'l');
            assert_eq!(dest[4], b'o');
            assert_eq!(dest[5], 0);

            let mut dest2 = [0u8; 10];
            rust_sqlite_wasm_shim_strncpy(dest2.as_mut_ptr() as *mut c_char, src.as_ptr(), 3);
            assert_eq!(dest2[0], b'h');
            assert_eq!(dest2[1], b'e');
            assert_eq!(dest2[2], b'l');
            assert_eq!(dest2[3], 0);

            let mut dest3 = [0u8; 10];
            rust_sqlite_wasm_shim_strncpy(dest3.as_mut_ptr() as *mut c_char, src.as_ptr(), 0);
        }
    }

    #[cfg(feature = "sqlite3mc")]
    #[wasm_bindgen_test]
    fn test_rust_sqlite_wasm_shim_strcat() {
        unsafe {
            let mut dest = [0u8; 20];
            let dest_ptr = dest.as_mut_ptr() as *mut c_char;

            let s1 = to_cstring("hello");
            rust_sqlite_wasm_shim_strcpy(dest_ptr, s1.as_ptr());

            let s2 = to_cstring(" world");
            let result = rust_sqlite_wasm_shim_strcat(dest_ptr, s2.as_ptr());

            assert_eq!(result as *const u8, dest.as_ptr());

            let final_str = CStr::from_ptr(dest.as_ptr() as *const c_char);
            assert_eq!(final_str.to_str().unwrap(), "hello world");

            let mut dest2 = [0u8; 10];
            let dest2_ptr = dest2.as_mut_ptr() as *mut c_char;
            *dest2_ptr = 0;

            rust_sqlite_wasm_shim_strcat(dest2_ptr, to_cstring("test").as_ptr());
            let final_str2 = CStr::from_ptr(dest2.as_ptr() as *const c_char);
            assert_eq!(final_str2.to_str().unwrap(), "test");
        }
    }

    #[cfg(feature = "sqlite3mc")]
    #[wasm_bindgen_test]
    fn test_rust_sqlite_wasm_shim_strncat() {
        unsafe {
            let mut dest = [0u8; 20];
            let dest_ptr = dest.as_mut_ptr() as *mut c_char;

            let initial = to_cstring("hello");
            rust_sqlite_wasm_shim_strcpy(dest_ptr, initial.as_ptr());

            let to_append = to_cstring(" world");
            let result = rust_sqlite_wasm_shim_strncat(dest_ptr, to_append.as_ptr(), 6);

            assert_eq!(result as *const u8, dest.as_ptr());

            let final_str = CStr::from_ptr(dest.as_ptr() as *const c_char);
            assert_eq!(final_str.to_str().unwrap(), "hello world");

            let mut dest2 = [0u8; 20];
            let dest2_ptr = dest2.as_mut_ptr() as *mut c_char;

            rust_sqlite_wasm_shim_strcpy(dest2_ptr, to_cstring("test").as_ptr());
            rust_sqlite_wasm_shim_strncat(dest2_ptr, to_cstring(" string").into_raw(), 3);

            let final_str2 = CStr::from_ptr(dest2.as_ptr() as *const c_char);
            assert_eq!(final_str2.to_str().unwrap(), "test st");

            let mut dest3 = [0u8; 20];
            let dest3_ptr = dest3.as_mut_ptr() as *mut c_char;

            rust_sqlite_wasm_shim_strcpy(dest3_ptr, to_cstring("base").as_ptr());
            rust_sqlite_wasm_shim_strncat(dest3_ptr, to_cstring(" appended").into_raw(), 0);

            let final_str3 = CStr::from_ptr(dest3.as_ptr() as *const c_char);
            assert_eq!(final_str3.to_str().unwrap(), "base");

            let mut dest4 = [0u8; 20];
            let dest4_ptr = dest4.as_mut_ptr() as *mut c_char;

            rust_sqlite_wasm_shim_strcpy(dest4_ptr, to_cstring("hi").as_ptr());
            rust_sqlite_wasm_shim_strncat(dest4_ptr, to_cstring(" there").into_raw(), 10);

            let final_str4 = CStr::from_ptr(dest4.as_ptr() as *const c_char);
            assert_eq!(final_str4.to_str().unwrap(), "hi there");

            let mut dest5 = [0u8; 20];
            let dest5_ptr = dest5.as_mut_ptr() as *mut c_char;

            *dest5_ptr = 0;
            rust_sqlite_wasm_shim_strncat(dest5_ptr, to_cstring("hello").as_ptr(), 5);

            let final_str5 = CStr::from_ptr(dest5.as_ptr() as *const c_char);
            assert_eq!(final_str5.to_str().unwrap(), "hello");

            let mut dest6 = [0u8; 20];
            let dest6_ptr = dest6.as_mut_ptr() as *mut c_char;

            rust_sqlite_wasm_shim_strcpy(dest6_ptr, to_cstring("start").as_ptr());
            rust_sqlite_wasm_shim_strncat(dest6_ptr, to_cstring("").as_ptr(), 5);

            let final_str6 = CStr::from_ptr(dest6.as_ptr() as *const c_char);
            assert_eq!(final_str6.to_str().unwrap(), "start");

            let mut dest7 = [0u8; 10];
            let dest7_ptr = dest7.as_mut_ptr() as *mut c_char;

            rust_sqlite_wasm_shim_strcpy(dest7_ptr, to_cstring("12345").as_ptr());
            rust_sqlite_wasm_shim_strncat(dest7_ptr, to_cstring("678").as_ptr(), 3);

            let final_str7 = CStr::from_ptr(dest7.as_ptr() as *const c_char);
            assert_eq!(final_str7.to_str().unwrap(), "12345678");

            let mut dest8 = [0u8; 30];
            let dest8_ptr = dest8.as_mut_ptr() as *mut c_char;

            rust_sqlite_wasm_shim_strcpy(dest8_ptr, to_cstring("line1\n").as_ptr());
            rust_sqlite_wasm_shim_strncat(dest8_ptr, to_cstring("line2\t").as_ptr(), 6);

            let final_str8 = CStr::from_ptr(dest8.as_ptr() as *const c_char);
            assert_eq!(final_str8.to_str().unwrap(), "line1\nline2\t");

            let mut dest9 = [0u8; 15];
            let dest9_ptr = dest9.as_mut_ptr() as *mut c_char;

            rust_sqlite_wasm_shim_strcpy(dest9_ptr, to_cstring("abc").as_ptr());
            rust_sqlite_wasm_shim_strncat(dest9_ptr, to_cstring("def").as_ptr(), 2);

            assert_eq!(dest9[0], b'a');
            assert_eq!(dest9[1], b'b');
            assert_eq!(dest9[2], b'c');
            assert_eq!(dest9[3], b'd');
            assert_eq!(dest9[4], b'e');
            assert_eq!(dest9[5], 0);
            assert_eq!(dest9[6], 0);

            let mut sql_buffer = [0u8; 50];
            let sql_buffer_ptr = sql_buffer.as_mut_ptr() as *mut c_char;

            rust_sqlite_wasm_shim_strcpy(sql_buffer_ptr, to_cstring("SELECT ").as_ptr());
            rust_sqlite_wasm_shim_strncat(
                sql_buffer_ptr,
                to_cstring("name, age FROM users").as_ptr(),
                12,
            );

            let sql_result = CStr::from_ptr(sql_buffer.as_ptr() as *const c_char);
            assert_eq!(sql_result.to_str().unwrap(), "SELECT name, age FR");
        }
    }
}
