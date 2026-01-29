use std::ffi::CStr;

use sqlite_wasm_rs::{
    sqlite3, sqlite3_close, sqlite3_column_text, sqlite3_finalize, sqlite3_open_v2,
    sqlite3_prepare_v3, sqlite3_step, SQLITE_OK, SQLITE_OPEN_CREATE, SQLITE_OPEN_READWRITE,
    SQLITE_ROW,
};
use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

macro_rules! console_log {
    ($($t:tt)*) => (log(&format_args!($($t)*).to_string()))
}

#[wasm_bindgen(start)]
async fn main() {
    let mut db: *mut sqlite3 = std::ptr::null_mut();
    // Open in-memory DB
    let ret = unsafe {
        sqlite3_open_v2(
            c":memory:".as_ptr().cast(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    console_log!("db opened: {db:?}");

    unsafe {
        // Register uuid4 extension
        let rc = sqlite_wasm_uuid4::register(db.cast());
        assert_eq!(SQLITE_OK, rc);
        console_log!("UUID4 extension registered");

        // Register uuid7 extension
        let rc = sqlite_wasm_uuid7::register(db.cast());
        assert_eq!(SQLITE_OK, rc);
        console_log!("UUID7 extension registered");

        // Test uuid()
        console_log!("Testing SELECT uuid();");
        let sql = c"SELECT uuid();";
        let mut stmt = std::ptr::null_mut();
        let rc = sqlite3_prepare_v3(
            db,
            sql.as_ptr().cast(),
            -1,
            0,
            &mut stmt,
            std::ptr::null_mut(),
        );
        assert_eq!(SQLITE_OK, rc);

        if sqlite3_step(stmt) == SQLITE_ROW {
            let val_ptr = sqlite3_column_text(stmt, 0);
            if !val_ptr.is_null() {
                let s = CStr::from_ptr(val_ptr.cast()).to_str().unwrap();
                console_log!("uuid() result: {}", s);

                // Validate format: 8-4-4-4-12 hex digits
                assert_eq!(s.len(), 36);
                assert_eq!(s.chars().nth(8), Some('-'));
                assert_eq!(s.chars().nth(13), Some('-'));
                assert_eq!(s.chars().nth(18), Some('-'));
                assert_eq!(s.chars().nth(23), Some('-'));
            } else {
                console_log!("uuid() returned NULL");
            }
        } else {
            console_log!("Error: No row returned for uuid()");
        }
        sqlite3_finalize(stmt);

        // Test uuid_str()
        console_log!("Testing SELECT uuid_str(uuid_blob('a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'));");
        // We know 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11' is a valid uuid
        let sql = c"SELECT uuid_str(uuid_blob('a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'));";
        let rc = sqlite3_prepare_v3(
            db,
            sql.as_ptr().cast(),
            -1,
            0,
            &mut stmt,
            std::ptr::null_mut(),
        );
        assert_eq!(SQLITE_OK, rc);

        if sqlite3_step(stmt) == SQLITE_ROW {
            let val_ptr = sqlite3_column_text(stmt, 0);
            if !val_ptr.is_null() {
                let s = CStr::from_ptr(val_ptr.cast()).to_str().unwrap();
                console_log!("uuid_str(...) result: {}", s);
                assert_eq!(s, "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11");
            } else {
                console_log!("uuid_str(...) returned NULL");
            }
        } else {
            console_log!("Error: No row returned for uuid_str()");
        }
        sqlite3_finalize(stmt);

        // Test uuid7()
        console_log!("Testing SELECT uuid7();");
        let sql = c"SELECT uuid7();";
        let mut stmt = std::ptr::null_mut();
        let rc = sqlite3_prepare_v3(
            db,
            sql.as_ptr().cast(),
            -1,
            0,
            &mut stmt,
            std::ptr::null_mut(),
        );
        assert_eq!(SQLITE_OK, rc);

        if sqlite3_step(stmt) == SQLITE_ROW {
            let val_ptr = sqlite3_column_text(stmt, 0);
            if !val_ptr.is_null() {
                let s = CStr::from_ptr(val_ptr.cast()).to_str().unwrap();
                console_log!("uuid7() result: {}", s);
                assert_eq!(s.len(), 36);
                assert_eq!(s.chars().nth(14), Some('7'));
            } else {
                console_log!("uuid7() returned NULL");
            }
        } else {
            console_log!("Error: No row returned for uuid7()");
        }
        sqlite3_finalize(stmt);

        sqlite3_close(db);
    }

    console_log!("All tests passed!");
}
