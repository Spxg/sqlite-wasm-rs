//! This module fills in the external functions needed to link to `sqlite.o`

use crate::{host, WasmOsCallback};
use core::alloc::Layout;
use core::ffi::{c_char, c_int, c_long, c_longlong, c_void};
use core::ptr;

#[allow(non_camel_case_types)]
type c_size_t = usize;

#[allow(non_camel_case_types)]
type c_time_t = c_longlong;

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
/// Uses the host's secure entropy hook, without a weak fallback or errno reporting.
///
/// # Safety
/// For a nonzero length, `buf` must be writable for `buf_len` bytes in a single
/// allocation, with `buf_len <= isize::MAX`. Its contents may be uninitialized.
#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_getentropy(buf: *mut u8, buf_len: c_size_t) -> c_int {
    if buf_len == 0 {
        return 0;
    }
    unsafe {
        // C output buffers need not be initialized, but a Rust byte slice must be.
        ptr::write_bytes(buf, 0, buf_len);
        match host::fill_entropy(core::slice::from_raw_parts_mut(buf, buf_len)) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    }
}

#[no_mangle]
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

#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_abort() {
    core::unreachable!();
}

/// Converts host calendar fields to the C ABI. A failed conversion returns null.
#[no_mangle]
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
        let Ok(local) = host::localtime(*t) else {
            return ptr::null_mut();
        };
        let Some(year) = local.year.checked_sub(1900) else {
            return ptr::null_mut();
        };
        if !(1..=12).contains(&local.month)
            || !(1..=31).contains(&local.day)
            || !(0..=23).contains(&local.hour)
            || !(0..=59).contains(&local.minute)
            || !(0..=60).contains(&local.second)
            || !(0..=6).contains(&local.weekday)
            || !(0..=365).contains(&local.yearday)
            || !(-1..=1).contains(&local.is_dst)
        {
            return ptr::null_mut();
        }
        ptr::addr_of_mut!(TM).write(tm {
            tm_sec: local.second,
            tm_min: local.minute,
            tm_hour: local.hour,
            tm_mday: local.day,
            tm_mon: local.month - 1,
            tm_year: year,
            tm_wday: local.weekday,
            tm_yday: local.yearday,
            tm_isdst: local.is_dst,
            tm_gmtoff: local.utc_offset_seconds,
            tm_zone: ptr::null_mut(),
        });
        ptr::addr_of_mut!(TM)
    }
}

// https://github.com/alexcrichton/dlmalloc-rs/blob/fb116603713825b43b113cc734bb7d663cb64be9/src/dlmalloc.rs#L141
const ALIGN: usize = core::mem::size_of::<usize>() * 2;

fn allocation_layout(size: usize) -> Option<Layout> {
    Layout::from_size_align(size.checked_add(ALIGN)?, ALIGN).ok()
}

#[no_mangle]
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

#[no_mangle]
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

#[no_mangle]
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

#[no_mangle]
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

/// Installs the default memory VFS during SQLite initialization.
#[no_mangle]
pub unsafe extern "C" fn sqlite3_os_init() -> core::ffi::c_int {
    unsafe {
        match rsqlite_vfs::memvfs::install(WasmOsCallback, true) {
            Ok(_) => crate::bindings::SQLITE_OK,
            Err(_) => crate::bindings::SQLITE_ERROR,
        }
    }
}

/// Reclaims the memory VFS during SQLite shutdown.
#[no_mangle]
pub unsafe extern "C" fn sqlite3_os_end() -> core::ffi::c_int {
    unsafe {
        match rsqlite_vfs::memvfs::uninstall() {
            Ok(()) => crate::bindings::SQLITE_OK,
            Err(_) => crate::bindings::SQLITE_ERROR,
        }
    }
}

#[cfg(all(test, feature = "wasm-bindgen"))]
mod tests {
    use super::*;
    use crate::{
        sqlite3_close, sqlite3_column_count, sqlite3_column_text, sqlite3_column_type,
        sqlite3_finalize, sqlite3_initialize, sqlite3_open, sqlite3_prepare_v3, sqlite3_shutdown,
        sqlite3_step, SQLITE_DONE, SQLITE_OK, SQLITE_ROW, SQLITE_TEXT,
    };

    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn test_initialize_shutdown() {
        unsafe {
            assert_eq!(sqlite3_initialize(), SQLITE_OK, "failed to initialize");
            let util = crate::vfs::memvfs::MemVfsUtil::get().unwrap();
            let original_default = crate::sqlite3_vfs_find(core::ptr::null());
            for name in ["", "hidden\0suffix"] {
                assert!(matches!(
                    util.import_db_unchecked(name, &[42; 512], 512),
                    Err(crate::vfs::memvfs::MemVfsError::InvalidFilename)
                ));
            }
            assert!(util
                .import_db_unchecked("invalid-page-size.db", &[42; 512], 0)
                .is_err());
            assert_eq!(util.count(), 0);
            assert_eq!(crate::sqlite3_vfs_find(core::ptr::null()), original_default);
            util.import_db_unchecked("survives-shutdown.db", &[42; 512], 512)
                .unwrap();
            assert!(matches!(
                util.import_db_unchecked("survives-shutdown.db", &[99; 512], 512),
                Err(crate::vfs::memvfs::MemVfsError::AlreadyExists(_))
            ));
            assert_eq!(sqlite3_shutdown(), SQLITE_OK, "failed to shutdown");
            assert_eq!(util.export_db("survives-shutdown.db").unwrap(), [42; 512]);
            let new_util = crate::vfs::memvfs::MemVfsUtil::get().unwrap();
            assert!(!new_util.exists("survives-shutdown.db"));
            util.delete_db("survives-shutdown.db");
            assert_eq!(sqlite3_shutdown(), SQLITE_OK, "failed to shutdown again");

            // Raw unregistration removes registry membership, not ownership.
            use alloc::rc::Rc;
            use core::time::Duration;
            use rsqlite_vfs::{memvfs, OsCallback, VfsResult};
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
        host::fill_entropy(&mut large).unwrap();
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
        let tm = unsafe { rust_sqlite_wasm_localtime(&1733976732) };
        assert!(!tm.is_null());
        let tm = unsafe { &*tm };
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
