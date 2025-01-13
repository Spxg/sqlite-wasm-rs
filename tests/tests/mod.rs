mod aux;
mod bind64;
mod column_metadata;
mod common;
mod hook;
mod status;
mod vfs;

use sqlite_wasm_rs::export::*;
use std::ffi::CString;

pub fn cstr(s: &str) -> CString {
    CString::new(s).unwrap()
}

pub fn memory_db() -> *mut sqlite3 {
    let mut db = std::ptr::null_mut();
    let f = cstr(":memory:");
    let ret = unsafe {
        sqlite3_open_v2(
            f.as_ptr(),
            &mut db as _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(ret, SQLITE_OK);
    db
}
