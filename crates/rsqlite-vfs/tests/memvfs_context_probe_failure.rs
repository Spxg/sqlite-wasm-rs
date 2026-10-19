#![cfg(feature = "threadsafe")]

use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use libsqlite3_sys as sqlite;
use rsqlite_vfs::memvfs::{MemVfsContext, MemVfsContextError};

static CLOSED: AtomicUsize = AtomicUsize::new(0);
static CLEANUP_REFUSED_REENTRY: AtomicBool = AtomicBool::new(true);

unsafe extern "C" fn scalar(
    context: *mut sqlite::sqlite3_context,
    _: i32,
    _: *mut *mut sqlite::sqlite3_value,
) {
    // SAFETY: SQLite supplies the live result context for this callback.
    unsafe { sqlite::sqlite3_result_null(context) };
}

unsafe extern "C" fn destroyed(_: *mut c_void) {
    if !matches!(
        MemVfsContext::try_new(),
        Err(MemVfsContextError::InitializationInProgress)
    ) {
        CLEANUP_REFUSED_REENTRY.store(false, Ordering::Relaxed);
    }
    CLOSED.fetch_add(1, Ordering::Relaxed);
}

unsafe extern "C" fn fail_open(
    db: *mut sqlite::sqlite3,
    _: *mut *mut c_char,
    _: *const sqlite::sqlite3_api_routines,
) -> i32 {
    // SAFETY: SQLite lends the opening connection, and the static UTF-8 C string and callbacks outlive it.
    let code = unsafe {
        sqlite::sqlite3_create_function_v2(
            db,
            c"probe_lifetime".as_ptr(),
            0,
            sqlite::SQLITE_UTF8,
            core::ptr::null_mut(),
            Some(scalar),
            None,
            None,
            Some(destroyed),
        )
    };
    if code != sqlite::SQLITE_OK {
        return code;
    }
    sqlite::SQLITE_ERROR
}

#[test]
fn failed_probe_closes_its_connection_and_releases_initialization() {
    // SAFETY: this isolated binary registers a static callback before any connection opens.
    unsafe {
        assert_eq!(
            sqlite::sqlite3_auto_extension(Some(fail_open)),
            sqlite::SQLITE_OK
        )
    };
    for closed in 1..=2 {
        assert!(matches!(
            MemVfsContext::try_new(),
            Err(MemVfsContextError::Sqlite(sqlite::SQLITE_ERROR))
        ));
        assert_eq!(CLOSED.load(Ordering::Relaxed), closed);
        assert!(CLEANUP_REFUSED_REENTRY.load(Ordering::Relaxed));
    }
    // SAFETY: every probe is closed, and this binary owns its extension registration.
    unsafe { sqlite::sqlite3_reset_auto_extension() };
    let context = MemVfsContext::try_new().expect("validation must recover");
    let second = MemVfsContext::try_new().expect("the live context must clone");
    drop((context, second));
}
