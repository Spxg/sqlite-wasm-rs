wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);

use sqlite_wasm_rs::c;
use sqlite_wasm_rs::init_sqlite;
use sqlite_wasm_rs::libsqlite3_sys::{SQLITE_OK, SQLITE_OPEN_CREATE, SQLITE_OPEN_READWRITE};
use std::ffi::CString;
use wasm_bindgen_test::{console_log, wasm_bindgen_test};

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap()
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_open_v2_and_exec_opfs_c() {
    init_sqlite().await;

    let filename = cstr("test_open_v2_and_exec_opfs_c.db");
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        c::sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            CString::new("opfs").unwrap().as_ptr(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    let errmsg = std::ptr::null_mut();
    // drop first
    let sql = cstr("DROP TABLE COMPANY;");
    let ret = unsafe { c::sqlite3_exec(db, sql.as_ptr(), None, std::ptr::null_mut(), errmsg) };
    if (SQLITE_OK == ret) {
        console_log!("test_open_v2_and_exec_opfs: table exist before test, dropped");
    }

    let sql = cstr(
        "CREATE TABLE IF NOT EXISTS COMPANY(
                        ID INT PRIMARY KEY     NOT NULL,
                        NAME           TEXT    NOT NULL );",
    );

    let ret = unsafe { c::sqlite3_exec(db, sql.as_ptr(), None, std::ptr::null_mut(), errmsg) };
    assert_eq!(SQLITE_OK, ret);

    let sql = cstr("INSERT INTO COMPANY (ID,NAME) VALUES (1, 'John Doe');");
    let ret = unsafe { c::sqlite3_exec(db, sql.as_ptr(), None, std::ptr::null_mut(), errmsg) };
    assert_eq!(SQLITE_OK, ret);

    let sql = cstr("SELECT * FROM COMPANY;");
    unsafe extern "C" fn f(
        arg1: *mut ::std::os::raw::c_void,
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
    let ret = unsafe { c::sqlite3_exec(db, sql.as_ptr(), Some(f), std::ptr::null_mut(), errmsg) };
    assert_eq!(SQLITE_OK, ret);
}
