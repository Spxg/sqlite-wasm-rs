mod memory;
#[cfg(feature = "relaxed-idb")]
mod relaxed_idb;
mod sahpool;

use std::ffi::CStr;

use sqlite_wasm_rs::*;
use wasm_bindgen_test::console_log;

pub fn check_persistent(db: *mut sqlite3) -> bool {
    let drop_or_create = drop_or_create_foo_table(db);
    if drop_or_create {
        console_log!("foo table not exists, created.");
    } else {
        console_log!("foo table exists, dropped.");
    }
    drop_or_create
}

pub fn drop_or_create_foo_table(db: *mut sqlite3) -> bool {
    let ret = unsafe {
        sqlite3_exec(
            db,
            c"DROP TABLE FOO;".as_ptr().cast(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };

    if SQLITE_OK == ret {
        return false;
    }

    let sql = c"CREATE TABLE IF NOT EXISTS FOO(
            ID INT PRIMARY KEY     NOT NULL,
            NAME           TEXT    NOT NULL );";

    let ret = unsafe {
        sqlite3_exec(
            db,
            sql.as_ptr().cast(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };

    assert_eq!(SQLITE_OK, ret);

    true
}
