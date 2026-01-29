#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::ffi::CString;
use alloc::string::ToString;
use core::ffi::{c_char, c_int, c_void, CStr};
use wasm_bindgen::prelude::*;

#[link(name = "sqlite_uuid7")]
extern "C" {
    pub fn sqlite3_uuid7_init(
        db: *mut c_void,
        pzErrMsg: *mut *mut c_char,
        pApi: *const c_void,
    ) -> c_int;
}

pub fn register(db: *mut c_void) -> c_int {
    unsafe { sqlite3_uuid7_init(db, core::ptr::null_mut(), core::ptr::null()) }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

macro_rules! console_log {
    ($($t:tt)*) => (log(&alloc::format!($($t)*)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ffi::CStr;
    use sqlite_wasm_rs as sqlite;

    fn setup_db() -> *mut sqlite::sqlite3 {
        let mut db: *mut sqlite::sqlite3 = core::ptr::null_mut();

        let filename = CString::new(":memory:").unwrap();
        // Standard open
        let rc = unsafe {
            sqlite::sqlite3_open_v2(
                filename.as_ptr(),
                &mut db,
                sqlite::SQLITE_OPEN_READWRITE | sqlite::SQLITE_OPEN_CREATE,
                core::ptr::null(),
            )
        };

        if rc != sqlite::SQLITE_OK {
            panic!("Failed to open DB: rc={}", rc);
        }
        console_log!("DB opened successfully");

        // Init extension
        unsafe {
            let rc = sqlite3_uuid7_init(db as *mut _, core::ptr::null_mut(), core::ptr::null_mut());
            if rc != sqlite::SQLITE_OK {
                panic!("sqlite3_uuid7_init failed: {}", rc);
            }
        }
        console_log!("Extension initialized");
        db
    }

    unsafe fn query_val(db: *mut sqlite::sqlite3, sql: &str) -> String {
        let mut stmt: *mut sqlite::sqlite3_stmt = core::ptr::null_mut();
        let c_sql = CString::new(sql).unwrap();

        let rc =
            sqlite::sqlite3_prepare_v2(db, c_sql.as_ptr(), -1, &mut stmt, core::ptr::null_mut());
        if rc != sqlite::SQLITE_OK {
            let err = sqlite::sqlite3_errmsg(db);
            let err_str = if !err.is_null() {
                CStr::from_ptr(err).to_string_lossy().to_string()
            } else {
                "Unknown error".to_string()
            };
            panic!("prepare failed: {} - {}", sql, err_str);
        }

        let rc = sqlite::sqlite3_step(stmt);
        let res = if rc == sqlite::SQLITE_ROW {
            let text = sqlite::sqlite3_column_text(stmt, 0);
            if text.is_null() {
                "NULL".to_string()
            } else {
                let c_str = CStr::from_ptr(text as *const _);
                c_str.to_string_lossy().to_string()
            }
        } else {
            String::new()
        };
        sqlite::sqlite3_finalize(stmt);
        res
    }

    #[wasm_bindgen_test::wasm_bindgen_test]
    fn test_uuid7() {
        console_log!("Starting test_uuid7 C extension verification");
        unsafe {
            let db = setup_db();

            let uuid = query_val(db, "SELECT uuid7()");
            console_log!("UUID7: {}", uuid);
            assert_eq!(uuid.len(), 36);
            assert_eq!(uuid.chars().nth(14).unwrap(), '7');

            sqlite::sqlite3_close(db);
        }
    }
}
