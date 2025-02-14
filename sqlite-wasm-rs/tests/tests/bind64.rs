use super::{cstr, memory_db};
use core::slice;
use sqlite_wasm_rs::export::*;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
#[allow(unused)]
fn test_bind_text64() {
    let db = memory_db();

    unsafe {
        sqlite3_exec(
            db,
            cstr("CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)").as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );

        let mut stmt = std::ptr::null_mut();
        sqlite3_prepare_v2(
            db,
            cstr("INSERT INTO test (name) VALUES (?)").as_ptr(),
            -1,
            &mut stmt as _,
            std::ptr::null_mut(),
        );

        sqlite3_bind_text64(
            stmt,
            1,
            cstr("John Doe").as_ptr(),
            "John Doe".len() as _,
            SQLITE_TRANSIENT(),
            SQLITE_UTF8 as _,
        );

        assert_eq!(SQLITE_DONE, sqlite3_step(stmt));
        sqlite3_finalize(stmt);

        sqlite3_prepare_v2(
            db,
            cstr("SELECT name FROM test WHERE id = 1").as_ptr(),
            -1,
            &mut stmt as _,
            std::ptr::null_mut(),
        );

        assert_eq!(SQLITE_ROW, sqlite3_step(stmt));

        let ptr = sqlite3_column_blob(stmt, 0);
        let len = sqlite3_column_bytes(stmt, 0);
        let s = std::str::from_utf8(slice::from_raw_parts(ptr as *mut u8, len as usize)).unwrap();
        assert_eq!("John Doe", s);
    }
}

#[wasm_bindgen_test]
#[allow(unused)]
fn test_bind_blob64() {
    let db = memory_db();

    unsafe {
        sqlite3_exec(
            db,
            cstr("CREATE TABLE test (id INTEGER PRIMARY KEY, data BLOB)").as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );

        let mut stmt = std::ptr::null_mut();
        sqlite3_prepare_v2(
            db,
            cstr("INSERT INTO test (data) VALUES (?)").as_ptr(),
            -1,
            &mut stmt as _,
            std::ptr::null_mut(),
        );

        let v: [u8; 2] = [1, 2];
        sqlite3_bind_blob64(stmt, 1, v.as_ptr() as _, v.len() as _, SQLITE_TRANSIENT());

        assert_eq!(SQLITE_DONE, sqlite3_step(stmt));
        sqlite3_finalize(stmt);

        sqlite3_prepare_v2(
            db,
            cstr("SELECT data FROM test WHERE id = 1").as_ptr(),
            -1,
            &mut stmt as _,
            std::ptr::null_mut(),
        );

        assert_eq!(SQLITE_ROW, sqlite3_step(stmt));

        let ptr = sqlite3_column_blob(stmt, 0);
        let len = sqlite3_column_bytes(stmt, 0);
        let s = slice::from_raw_parts(ptr as *mut u8, len as usize);
        assert_eq!(&[1, 2], s);
    }
}
