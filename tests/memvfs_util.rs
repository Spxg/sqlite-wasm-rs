//! Memory VFS management through `host::memvfs_util`, without `unsafe`.
#![deny(unsafe_code)]

use core::ptr;
use sqlite_wasm_rs::*;

#[allow(unsafe_code)]
fn create_with_connection(name: &core::ffi::CStr) {
    let mut db = ptr::null_mut();
    let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE;
    // SAFETY: the strings are NUL-terminated, and `db` is closed before returning.
    unsafe {
        assert_eq!(
            sqlite3_open_v2(name.as_ptr(), &mut db, flags, c"memvfs".as_ptr()),
            SQLITE_OK
        );
        let sql = c"CREATE TABLE t(n)".as_ptr();
        assert_eq!(
            sqlite3_exec(db, sql, None, ptr::null_mut(), ptr::null_mut()),
            SQLITE_OK
        );
        assert_eq!(sqlite3_close(db), SQLITE_OK);
    }
}

#[forbid(unsafe_code)]
mod safe {
    use sqlite_wasm_rs::host::memvfs_util;
    use wasm_bindgen_test::wasm_bindgen_test;

    // The shared-memory CI job builds on nightly, where `cfg` sees `atomics`.
    #[cfg(any(not(target_feature = "atomics"), feature = "threadsafe"))]
    #[wasm_bindgen_test]
    fn manages_files_created_by_a_connection() {
        use sqlite_wasm_rs::vfs::VfsFilesManager;

        super::create_with_connection(c"a.db");

        let util = memvfs_util().unwrap();
        assert!(util.contains("a.db").unwrap());
        assert_eq!(util.names().unwrap(), ["a.db"]);
        assert!(util.remove("a.db").unwrap());
        assert!(util.names().unwrap().is_empty());
    }

    #[cfg(all(target_feature = "atomics", not(feature = "threadsafe")))]
    #[wasm_bindgen_test]
    fn refuses_shared_memory() {
        super::create_with_connection(c"a.db");

        assert!(matches!(
            memvfs_util(),
            Err(sqlite_wasm_rs::host::MemVfsUtilError::SharedMemory)
        ));
    }
}
