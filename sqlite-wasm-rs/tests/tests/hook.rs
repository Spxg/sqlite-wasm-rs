use super::{cstr, memory_db};
use sqlite_wasm_rs::export::*;
use std::ffi::CStr;
use wasm_bindgen_test::{console_log, wasm_bindgen_test};

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_commit_hook() {
    init_sqlite().await.unwrap();

    let mut hook_count = 0;

    unsafe extern "C" fn commit_hook(cb_arg: *mut ::std::os::raw::c_void) -> ::std::os::raw::c_int {
        let count: *mut i32 = cb_arg as _;
        *count += 1;
        0
    }

    let db = memory_db();
    unsafe {
        sqlite3_commit_hook(db, Some(commit_hook), &mut hook_count as *const _ as *mut _);
        sqlite3_exec(
            db,
            cstr("BEGIN").as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        sqlite3_exec(
            db,
            cstr("CREATE TABLE IF NOT EXISTS test (id INTEGER PRIMARY KEY, name TEXT)").as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        sqlite3_exec(
            db,
            cstr("INSERT INTO test (name) VALUES ('John Doe')").as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        sqlite3_exec(
            db,
            cstr("COMMIT").as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    };
    assert_eq!(hook_count, 1);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_progress_handler() {
    init_sqlite().await.unwrap();

    let mut progress_count = 0;

    unsafe extern "C" fn progress_handler(
        cb_arg: *mut ::std::os::raw::c_void,
    ) -> ::std::os::raw::c_int {
        let count: *mut i32 = cb_arg as _;
        *count += 1;
        0
    }

    let db = memory_db();
    unsafe {
        sqlite3_progress_handler(
            db,
            2,
            Some(progress_handler),
            &mut progress_count as *const _ as *mut _,
        );
        sqlite3_exec(
            db,
            cstr("BEGIN").as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        sqlite3_exec(
            db,
            cstr("CREATE TABLE IF NOT EXISTS test (id INTEGER PRIMARY KEY, name TEXT)").as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        sqlite3_exec(
            db,
            cstr("COMMIT").as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    };
    assert!(progress_count > 1);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_rollback_hook() {
    init_sqlite().await.unwrap();

    let mut rollback_count = 0;

    unsafe extern "C" fn rollback_hook(cb_arg: *mut ::std::os::raw::c_void) {
        let count: *mut i32 = cb_arg as _;
        *count += 1;
    }

    let db = memory_db();
    unsafe {
        sqlite3_rollback_hook(
            db,
            Some(rollback_hook),
            &mut rollback_count as *const _ as *mut _,
        );
        sqlite3_exec(
            db,
            cstr("BEGIN").as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        sqlite3_exec(
            db,
            cstr("ROLLBACK").as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    };
    assert_eq!(rollback_count, 1);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_update_hook() {
    init_sqlite().await.unwrap();

    let mut update_count = 0;

    unsafe extern "C" fn update_hook(
        user_ctx: *mut ::std::os::raw::c_void,
        op: ::std::os::raw::c_int,
        db_name: *const ::std::os::raw::c_char,
        table_name: *const ::std::os::raw::c_char,
        new_row_id: sqlite3_int64,
    ) {
        console_log!(
            "op: {op}, db_name: {:?}, table_name: {:?}, new_row_id: {new_row_id}",
            CStr::from_ptr(db_name),
            CStr::from_ptr(table_name)
        );
        let count: *mut i32 = user_ctx as _;
        *count += 1;
    }

    let db = memory_db();
    unsafe {
        sqlite3_update_hook(
            db,
            Some(update_hook),
            &mut update_count as *const _ as *mut _,
        );
        sqlite3_exec(
            db,
            cstr("BEGIN").as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        sqlite3_exec(
            db,
            cstr("CREATE TABLE IF NOT EXISTS test (id INTEGER PRIMARY KEY, name TEXT)").as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        sqlite3_exec(
            db,
            cstr("INSERT INTO test (name) VALUES ('John Doe')").as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        sqlite3_exec(
            db,
            cstr("ROLLBACK").as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    };
    assert_eq!(update_count, 1);
}
