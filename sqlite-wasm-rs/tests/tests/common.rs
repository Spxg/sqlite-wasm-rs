use super::{cstr, memory_db};
use core::slice;
use sqlite_wasm_rs::export::*;
use std::ffi::CStr;
use wasm_bindgen_test::{console_log, wasm_bindgen_test};

#[wasm_bindgen_test]
#[allow(unused)]
fn test_sqlite_prepare_v3_tail() {
    let db = memory_db();

    let sql = "
        CREATE TABLE test(id INTEGER PRIMARY KEY, name TEXT);
        INSERT INTO test VALUES(1, 'Alice');
        INSERT INTO test VALUES(2, 'Bob');
        INSERT INTO test VALUES(3, 'Charlie');
        SELECT * FROM test;
        DELETE FROM test WHERE id = 2;
        SELECT * FROM test;
        DROP TABLE test;
    ";

    let sql = cstr(sql.trim());
    let mut remaining_sql = sql.as_ptr();

    unsafe {
        while !remaining_sql.is_null() {
            let remain = CStr::from_ptr(remaining_sql);
            if remain.is_empty() {
                break;
            }
            let mut stmt: *mut sqlite3_stmt = std::ptr::null_mut();
            let mut pz_tail = std::ptr::null();

            let ret =
                sqlite3_prepare_v3(db, remaining_sql, -1, 0, &mut stmt as _, &mut pz_tail as _);
            assert_eq!(ret, SQLITE_OK);

            let mut rc = sqlite3_step(stmt);

            while rc == SQLITE_ROW {
                for col in 0..sqlite3_column_count(stmt) {
                    let value = sqlite3_column_value(stmt, col);
                    let text = sqlite3_value_text(value);
                    let len = sqlite3_value_bytes(value);
                    let slice = slice::from_raw_parts(text, len as usize);
                    let text = std::str::from_utf8(slice).unwrap();
                    console_log!("Column {}: {:?}", col, text);
                }
                rc = sqlite3_step(stmt);
            }

            if rc == SQLITE_DONE {
                let sql = CStr::from_ptr(unsafe { sqlite3_sql(stmt) });
                console_log!("SQL {sql:?} executed successfully.");
                sqlite3_finalize(stmt);
                remaining_sql = pz_tail;
            }
        }
    }
    unsafe {
        sqlite3_close(db);
    }
}

#[wasm_bindgen_test]
#[allow(unused)]
fn test_exec_errmsg() {
    let db = memory_db();
    let mut errmsg: *mut ::std::os::raw::c_char = std::ptr::null_mut();
    let sql = cstr("SELECT * FROM non_existent_table");
    let ret = unsafe {
        sqlite3_exec(
            db,
            sql.as_ptr(),
            None,
            std::ptr::null_mut(),
            &mut errmsg as *mut _,
        )
    };
    assert_eq!(SQLITE_ERROR, ret);
    let msg = unsafe { CStr::from_ptr(errmsg) };
    console_log!("{msg:?}");
    unsafe {
        sqlite3_free((errmsg).cast());
    }
}
