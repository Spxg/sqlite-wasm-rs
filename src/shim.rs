//! This module fills in the external functions needed to link to `sqlite.o`

use core::alloc::Layout;
use core::ffi::{c_char, c_int, c_long, c_longlong, c_void};
use core::ptr;
use core::time::Duration;

use js_sys::{Date, Math, Number};
use rsqlite_vfs::OsCallback;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::wasm_bindgen;

#[derive(Default)]
pub struct WasmOsCallback;

impl OsCallback for WasmOsCallback {
    /// thread::sleep is available when atomics is enabled
    #[cfg(target_feature = "atomics")]
    fn sleep(&self, dur: Duration) {
        let mut nanos = dur.as_nanos();
        while nanos > 0 {
            let amt = core::cmp::min(i64::MAX as u128, nanos);
            let mut x = 0;
            // memory_atomic_wait32 returns 2 on timeout; loop until elapsed.
            let val = unsafe { core::arch::wasm32::memory_atomic_wait32(&mut x, 0, amt as i64) };
            debug_assert_eq!(val, 2);
            nanos -= amt;
        }
    }

    #[cfg(not(target_feature = "atomics"))]
    // Browsers provide no synchronous sleep primitive here. This platform
    // limitation also applies to VFS xSleep/sqlite3_sleep; do not busy-wait.
    fn sleep(&self, _dur: Duration) {}

    fn random(&self, buf: &mut [u8]) -> usize {
        fn fallback(buf: &mut [u8]) {
            // Non-cryptographic fallback when crypto.getRandomValues is unavailable.
            for b in buf {
                *b = (Math::random() * 255000.0) as u32 as u8;
            }
        }

        fill_random(buf).unwrap_or_else(|_| fallback(buf));
        buf.len()
    }

    fn epoch_timestamp_in_ms(&self) -> rsqlite_vfs::VfsResult<i64> {
        Ok(Date::new_0().get_time() as i64)
    }
}

#[allow(non_camel_case_types)]
type c_size_t = usize;

#[allow(non_camel_case_types)]
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

fn fill_random(buf: &mut [u8]) -> Result<(), JsValue> {
    // Web Crypto limits each getRandomValues request to 65,536 bytes.
    for chunk in buf.chunks_mut(65_536) {
        #[cfg(not(target_feature = "atomics"))]
        get_random_values(chunk)?;

        #[cfg(target_feature = "atomics")]
        {
            // Web Crypto cannot fill a view backed by shared Wasm memory.
            let array = js_sys::Uint8Array::new_with_length(chunk.len() as u32);
            get_random_values(&array)?;
            array.copy_to(chunk);
        }
    }
    Ok(())
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
// Mirrors emscripten/sqlite-wasm localtime handling, including DST logic.
unsafe fn localtime_js(t: c_time_t, tm: *mut tm) {
    unsafe {
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
        let tz_offset = date.get_timezone_offset();
        let summer_offset =
            Date::new_with_year_month_day(date.get_full_year(), 6, 1).get_timezone_offset();
        let winter_offset = start.get_timezone_offset();
        (*tm).tm_isdst = i32::from(
            summer_offset != winter_offset && tz_offset == winter_offset.min(summer_offset),
        );

        (*tm).tm_gmtoff = -(tz_offset * 60.0) as _;
    }
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

/// Internal SQLite3MC entropy hook: returns 0 on success and -1 on failure.
/// Uses only Web Crypto, without a weak fallback or POSIX errno reporting.
///
/// # Safety
/// For a nonzero length, `buf` must be writable for `buf_len` bytes in a single
/// allocation, with `buf_len <= isize::MAX`. Its contents may be uninitialized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rust_sqlite_wasm_getentropy(buf: *mut u8, buf_len: c_size_t) -> c_int {
    if buf_len == 0 {
        return 0;
    }
    unsafe {
        // C output buffers need not be initialized, but a Rust byte slice must be.
        ptr::write_bytes(buf, 0, buf_len);
        match fill_random(core::slice::from_raw_parts_mut(buf, buf_len)) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rust_sqlite_wasm_assert_fail(
    expr: *const c_char,
    file: *const c_char,
    line: c_int,
    func: *const c_char,
) {
    unsafe {
        let expr = core::ffi::CStr::from_ptr(expr).to_string_lossy();
        let file = core::ffi::CStr::from_ptr(file).to_string_lossy();
        let func = core::ffi::CStr::from_ptr(func).to_string_lossy();
        panic!("Assertion failed: {expr} ({file}: {func}: {line})");
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rust_sqlite_wasm_abort() {
    core::unreachable!();
}

/// See <https://github.com/emscripten-core/emscripten/blob/089590d17eeb705424bf32f8a1afe34a034b4682/system/lib/libc/mktime.c#L28>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rust_sqlite_wasm_localtime(t: *const c_time_t) -> *mut tm {
    unsafe {
        // Single shared buffer, matches libc behavior; assumes no concurrent callers.
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
}

// https://github.com/alexcrichton/dlmalloc-rs/blob/fb116603713825b43b113cc734bb7d663cb64be9/src/dlmalloc.rs#L141
const ALIGN: usize = core::mem::size_of::<usize>() * 2;

fn allocation_layout(size: usize) -> Option<Layout> {
    Layout::from_size_align(size.checked_add(ALIGN)?, ALIGN).ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rust_sqlite_wasm_malloc(size: c_size_t) -> *mut c_void {
    let Some(layout) = allocation_layout(size) else {
        return ptr::null_mut();
    };
    unsafe {
        let ptr = alloc::alloc::alloc(layout);

        if ptr.is_null() {
            return ptr::null_mut();
        }
        // Store size for free/realloc; pointer returned is offset by ALIGN.
        *ptr.cast::<usize>() = size;

        ptr.add(ALIGN).cast()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rust_sqlite_wasm_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        // Only accepts pointers allocated by rust_sqlite_wasm_malloc/realloc.
        let ptr: *mut u8 = ptr.sub(ALIGN).cast();
        let size = *(ptr.cast::<usize>());

        // This size was validated before allocating the block.
        let layout = Layout::from_size_align_unchecked(size + ALIGN, ALIGN);
        alloc::alloc::dealloc(ptr, layout);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rust_sqlite_wasm_realloc(
    ptr: *mut c_void,
    new_size: c_size_t,
) -> *mut c_void {
    if ptr.is_null() {
        return unsafe { rust_sqlite_wasm_malloc(new_size) };
    }
    let Some(new_layout) = allocation_layout(new_size) else {
        // A failed realloc must leave the original allocation intact.
        return ptr::null_mut();
    };
    unsafe {
        // Only accepts pointers allocated by rust_sqlite_wasm_malloc/realloc.
        let ptr: *mut u8 = ptr.sub(ALIGN).cast();
        let size = *(ptr.cast::<usize>());

        // The old size was validated before allocating the block.
        let layout = Layout::from_size_align_unchecked(size + ALIGN, ALIGN);
        let ptr = alloc::alloc::realloc(ptr, layout, new_layout.size());

        if ptr.is_null() {
            return ptr::null_mut();
        }
        *ptr.cast::<usize>() = new_size;

        ptr.add(ALIGN).cast()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rust_sqlite_wasm_calloc(num: c_size_t, size: c_size_t) -> *mut c_void {
    let Some(total) = num.checked_mul(size) else {
        return ptr::null_mut();
    };
    unsafe {
        let ptr: *mut u8 = rust_sqlite_wasm_malloc(total).cast();
        if !ptr.is_null() {
            ptr::write_bytes(ptr, 0, total);
        }
        ptr.cast()
    }
}

/// SQLite OS initialization entry point.
///
/// This function is called by SQLite when it is initialized. It sets up the
/// default VFS for the environment, which in this case is the in-memory VFS.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sqlite3_os_init() -> core::ffi::c_int {
    unsafe {
        match rsqlite_vfs::memvfs::install(WasmOsCallback, true) {
            Ok(_) => crate::bindings::SQLITE_OK,
            Err(_) => crate::bindings::SQLITE_ERROR,
        }
    }
}

/// SQLite OS shutdown entry point.
///
/// This function is called by SQLite when it is shut down. It cleans up
/// any resources allocated by `sqlite3_os_init`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sqlite3_os_end() -> core::ffi::c_int {
    unsafe {
        match rsqlite_vfs::memvfs::uninstall() {
            Ok(()) => crate::bindings::SQLITE_OK,
            Err(_) => crate::bindings::SQLITE_ERROR,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        SQLITE_DONE, SQLITE_OK, SQLITE_ROW, SQLITE_TEXT, sqlite3_close, sqlite3_column_count,
        sqlite3_column_text, sqlite3_column_type, sqlite3_finalize, sqlite3_initialize,
        sqlite3_open, sqlite3_prepare_v3, sqlite3_shutdown, sqlite3_step,
    };

    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn test_initialize_shutdown() {
        unsafe {
            assert_eq!(sqlite3_initialize(), SQLITE_OK, "failed to initialize");
            let util = crate::MemVfsUtil::get().unwrap();
            let original_default = crate::sqlite3_vfs_find(core::ptr::null());
            for name in ["", "hidden\0suffix"] {
                assert!(matches!(
                    util.import_db_unchecked(name, &[42; 512], 512),
                    Err(crate::MemVfsError::InvalidFilename)
                ));
            }
            assert!(
                util.import_db_unchecked("invalid-page-size.db", &[42; 512], 0)
                    .is_err()
            );
            assert_eq!(util.count(), 0);
            assert_eq!(crate::sqlite3_vfs_find(core::ptr::null()), original_default);
            util.import_db_unchecked("survives-shutdown.db", &[42; 512], 512)
                .unwrap();
            assert!(matches!(
                util.import_db_unchecked("survives-shutdown.db", &[99; 512], 512),
                Err(crate::MemVfsError::AlreadyExists(_))
            ));
            assert_eq!(sqlite3_shutdown(), SQLITE_OK, "failed to shutdown");
            assert_eq!(util.export_db("survives-shutdown.db").unwrap(), [42; 512]);
            let new_util = crate::MemVfsUtil::get().unwrap();
            assert!(!new_util.exists("survives-shutdown.db"));
            util.delete_db("survives-shutdown.db");
            assert_eq!(sqlite3_shutdown(), SQLITE_OK, "failed to shutdown again");

            // Raw unregistration removes registry membership, not ownership.
            use alloc::rc::Rc;
            use core::time::Duration;
            use rsqlite_vfs::{OsCallback, VfsResult, memvfs};
            struct TrackedOs {
                _token: Rc<()>,
            }
            impl OsCallback for TrackedOs {
                fn sleep(&self, duration: Duration) {
                    WasmOsCallback.sleep(duration);
                }
                fn random(&self, bytes: &mut [u8]) -> usize {
                    WasmOsCallback.random(bytes)
                }
                fn epoch_timestamp_in_ms(&self) -> VfsResult<i64> {
                    WasmOsCallback.epoch_timestamp_in_ms()
                }
            }
            assert_eq!(sqlite3_initialize(), SQLITE_OK);
            memvfs::uninstall().unwrap();
            let token = Rc::new(());
            let util = memvfs::install(
                TrackedOs {
                    _token: token.clone(),
                },
                true,
            )
            .unwrap();
            util.import_db_unchecked("detached.db", &[42; 512], 512)
                .unwrap();
            let original = crate::sqlite3_vfs_find(c"memvfs".as_ptr());
            assert_eq!(crate::sqlite3_vfs_unregister(original), SQLITE_OK);
            let reinstalled = memvfs::install(WasmOsCallback, true).unwrap();
            assert_eq!(crate::sqlite3_vfs_find(c"memvfs".as_ptr()), original);
            assert_eq!(reinstalled.export_db("detached.db").unwrap(), [42; 512]);
            drop(util);
            drop(reinstalled);
            assert_eq!(Rc::strong_count(&token), 2);
            assert_eq!(crate::sqlite3_vfs_unregister(original), SQLITE_OK);
            memvfs::uninstall().unwrap();
            assert_eq!(Rc::strong_count(&token), 1);
            assert_eq!(sqlite3_shutdown(), SQLITE_OK);
        }
    }

    #[wasm_bindgen_test]
    fn test_random_get() {
        let mut buf = core::mem::MaybeUninit::<[u8; 32]>::uninit();
        assert_eq!(
            unsafe { rust_sqlite_wasm_getentropy(buf.as_mut_ptr().cast(), 32) },
            0
        );
        assert_eq!(
            unsafe { rust_sqlite_wasm_getentropy(ptr::null_mut(), 0) },
            0
        );

        // Must succeed using crypto itself, not the VFS's Math.random fallback.
        let mut large = alloc::vec![0; 65_537];
        fill_random(&mut large).unwrap();
    }

    #[wasm_bindgen_test]
    fn test_memory() {
        unsafe {
            rust_sqlite_wasm_free(ptr::null_mut());
            let ptr1 = rust_sqlite_wasm_realloc(ptr::null_mut(), 10);
            assert!(!ptr1.is_null());
            ptr::write_bytes(ptr1.cast::<u8>(), 42, 10);
            let ptr2 = rust_sqlite_wasm_realloc(ptr1, 100);
            assert!(!ptr2.is_null());
            assert_eq!(
                core::slice::from_raw_parts(ptr2.cast::<u8>(), 10),
                &[42; 10]
            );
            for size in [usize::MAX, isize::MAX as usize, isize::MAX as usize - ALIGN] {
                assert!(rust_sqlite_wasm_malloc(size).is_null());
                assert!(rust_sqlite_wasm_realloc(ptr2, size).is_null());
                assert_eq!(
                    core::slice::from_raw_parts(ptr2.cast::<u8>(), 10),
                    &[42; 10]
                );
            }
            rust_sqlite_wasm_free(ptr2);
            assert!(rust_sqlite_wasm_calloc(usize::MAX / 2 + 1, 2).is_null());

            let ptr: *mut u8 = rust_sqlite_wasm_calloc(2, 8).cast();
            assert!(!ptr.is_null());
            let buf = core::slice::from_raw_parts(ptr, 2 * 8);

            assert!(buf.iter().all(|&x| x == 0));
            rust_sqlite_wasm_free(ptr.cast());
        }
    }

    #[wasm_bindgen_test]
    fn test_localtime_sqlite() {
        unsafe {
            let mut db = core::ptr::null_mut();
            let ret = sqlite3_open(c":memory:".as_ptr().cast(), &mut db as *mut _);
            assert_eq!(ret, SQLITE_OK);
            let mut stmt = core::ptr::null_mut();
            let ret = sqlite3_prepare_v3(
                db,
                c"SELECT datetime('now', 'localtime');".as_ptr().cast(),
                -1,
                0,
                &mut stmt as *mut _,
                core::ptr::null_mut(),
            );
            assert_eq!(ret, SQLITE_OK);
            assert_eq!(sqlite3_step(stmt), SQLITE_ROW);
            assert_eq!(sqlite3_column_count(stmt), 1);
            assert_eq!(sqlite3_column_type(stmt, 0), SQLITE_TEXT);
            assert!(!sqlite3_column_text(stmt, 0).is_null());
            assert_eq!(sqlite3_step(stmt), SQLITE_DONE);
            assert_eq!(sqlite3_finalize(stmt), SQLITE_OK);
            assert_eq!(sqlite3_close(db), SQLITE_OK);
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
            tm_zone: core::ptr::null_mut(),
        };
        unsafe {
            localtime_js(1733976732, &mut tm as *mut tm);
        };
        let gmtoff = tm.tm_gmtoff / 3600;

        assert_eq!(tm.tm_year, 2024 - 1900);
        assert_eq!(tm.tm_mon, 12 - 1);
        assert_eq!(tm.tm_mday, 12);
        assert_eq!(tm.tm_hour as core::ffi::c_long, 12 - 8 + gmtoff);
        assert_eq!(tm.tm_min, 12);
        assert_eq!(tm.tm_sec, 12);
        assert_eq!(tm.tm_wday, 4);
        assert_eq!(tm.tm_yday, 346);
    }
}
