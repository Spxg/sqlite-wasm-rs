//! Memory VFS management through `MemVfsContext`, without `unsafe`.
#![cfg(all(target_family = "wasm", not(target_feature = "atomics")))]
#![deny(unsafe_code)]

use core::ptr;
use sqlite_wasm_rs::*;

#[allow(unsafe_code)]
fn create_with_connection() {
    let mut db = ptr::null_mut();
    let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE;
    // SAFETY: the strings are NUL-terminated and `db` is closed below on this thread.
    unsafe {
        assert_eq!(
            sqlite3_open_v2(c"a.db".as_ptr(), &mut db, flags, c"memvfs".as_ptr()),
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
    use sqlite_wasm_rs::vfs::memvfs::{self, MemVfsContext, MemVfsUtil};
    use sqlite_wasm_rs::vfs::VfsFilesManager;
    use sqlite_wasm_rs::WasmOsCallback;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn safe_handles_manage_connection_created_files() {
        let context = MemVfsContext::try_new().unwrap();
        super::create_with_connection();

        let util = MemVfsUtil::get(&context).unwrap();
        assert!(util.contains("a.db").unwrap());
        assert_eq!(util.names().unwrap(), ["a.db"]);

        // Reinstalling reuses the registration, so both handles share files.
        let installed = memvfs::install(&context, WasmOsCallback, false).unwrap();
        assert!(util.remove("a.db").unwrap());
        assert!(installed.names().unwrap().is_empty());
    }
}
