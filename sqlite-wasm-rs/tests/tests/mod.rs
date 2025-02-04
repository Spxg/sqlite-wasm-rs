mod auxdata;
mod bind64;
mod column_metadata;
mod common;
mod hook;
#[cfg(feature = "polyfill")]
mod polyfill_vfs;
mod status;
#[cfg(feature = "wrapper")]
mod wrapper_vfs;

use sqlite_wasm_rs::export::*;
use std::ffi::CString;
use wasm_bindgen_test::console_log;

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

pub fn test_vfs(db: *mut sqlite3) {
    let mut errmsg: *mut ::std::os::raw::c_char = std::ptr::null_mut();
    // drop first
    let sql = cstr("DROP TABLE COMPANY;");
    let ret = unsafe {
        sqlite3_exec(
            db,
            sql.as_ptr(),
            None,
            std::ptr::null_mut(),
            &mut errmsg as *mut _,
        )
    };
    if SQLITE_OK == ret {
        console_log!("test_vfs: table exist before test, dropped");
    }

    let sql = cstr(
        "CREATE TABLE IF NOT EXISTS COMPANY(
            ID INT PRIMARY KEY     NOT NULL,
            NAME           TEXT    NOT NULL );",
    );

    let ret = unsafe {
        sqlite3_exec(
            db,
            sql.as_ptr(),
            None,
            std::ptr::null_mut(),
            &mut errmsg as *mut _,
        )
    };
    assert_eq!(SQLITE_OK, ret);

    let sql = cstr("INSERT INTO COMPANY (ID,NAME) VALUES (1, 'John Doe');");
    let ret = unsafe {
        sqlite3_exec(
            db,
            sql.as_ptr(),
            None,
            std::ptr::null_mut(),
            &mut errmsg as *mut _,
        )
    };
    assert_eq!(SQLITE_OK, ret);

    let sql = cstr("SELECT * FROM COMPANY;");
    unsafe extern "C" fn f(
        _: *mut ::std::os::raw::c_void,
        arg2: ::std::os::raw::c_int,
        arg3: *mut *mut ::std::os::raw::c_char,
        arg4: *mut *mut ::std::os::raw::c_char,
    ) -> ::std::os::raw::c_int {
        assert_eq!(2, arg2);
        let values = Vec::from_raw_parts(arg3, arg2 as usize, arg2 as usize);
        let names = Vec::from_raw_parts(arg4, arg2 as usize, arg2 as usize);
        let mut iter = values
            .iter()
            .cloned()
            .map(|s| CString::from_raw(s))
            .zip(names.iter().cloned().map(|s| CString::from_raw(s)));

        let next = iter.next().unwrap();
        assert_eq!((cstr("1"), cstr("ID")), next);
        std::mem::forget(next);

        let next = iter.next().unwrap();
        assert_eq!((cstr("John Doe"), cstr("NAME")), next);
        std::mem::forget(next);

        std::mem::forget(values);
        std::mem::forget(names);
        0
    }
    let ret = unsafe {
        sqlite3_exec(
            db,
            sql.as_ptr(),
            Some(f),
            std::ptr::null_mut(),
            &mut errmsg as *mut _,
        )
    };
    assert_eq!(SQLITE_OK, ret);
}
