use sqlite_wasm_rs as ffi;

/// Executes a small database operation without wasm-bindgen or generated glue.
#[unsafe(no_mangle)]
pub extern "C" fn run() -> i32 {
    unsafe {
        let mut db = core::ptr::null_mut();
        assert_eq!(
            ffi::sqlite3_open(c"demo.db".as_ptr(), &mut db),
            ffi::SQLITE_OK
        );
        ffi::sqlite3_sleep(1);
        #[cfg(feature = "sqlite3mc")]
        assert_eq!(
            ffi::sqlite3_exec(
                db,
                c"PRAGMA key = 'example';".as_ptr(),
                None,
                core::ptr::null_mut(),
                core::ptr::null_mut()
            ),
            ffi::SQLITE_OK
        );
        assert_eq!(
            ffi::sqlite3_exec(
                db,
                c"CREATE TABLE demo(value); INSERT INTO demo VALUES (42);".as_ptr(),
                None,
                core::ptr::null_mut(),
                core::ptr::null_mut()
            ),
            ffi::SQLITE_OK
        );
        let mut stmt = core::ptr::null_mut();
        assert_eq!(
            ffi::sqlite3_prepare_v2(
                db,
                c"SELECT value, datetime('now') FROM demo;".as_ptr(),
                -1,
                &mut stmt,
                core::ptr::null_mut()
            ),
            ffi::SQLITE_OK
        );
        assert_eq!(ffi::sqlite3_step(stmt), ffi::SQLITE_ROW);
        let value = ffi::sqlite3_column_int(stmt, 0);
        assert_eq!(ffi::sqlite3_column_type(stmt, 1), ffi::SQLITE_TEXT);
        assert_eq!(ffi::sqlite3_step(stmt), ffi::SQLITE_DONE);
        assert_eq!(ffi::sqlite3_finalize(stmt), ffi::SQLITE_OK);
        assert_eq!(ffi::sqlite3_close(db), ffi::SQLITE_OK);
        assert_eq!(ffi::sqlite3_shutdown(), ffi::SQLITE_OK);
        value
    }
}
