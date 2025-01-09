wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);

use core::slice;
use sqlite_wasm_rs::c::{self, sqlite3, SQLITE_DONE};
use sqlite_wasm_rs::c::{sqlite3_stmt, SQLITE_ERROR};
use sqlite_wasm_rs::libsqlite3::{SQLITE_OK, SQLITE_OPEN_CREATE, SQLITE_OPEN_READWRITE};
use sqlite_wasm_rs::{init_sqlite, OpfsSAHPoolCfg};
use std::ffi::CStr;
use std::ffi::CString;
use wasm_bindgen_test::{console_log, wasm_bindgen_test};

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap()
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_exec_errmsg() {
    init_sqlite().await.unwrap();

    let filename = cstr(":memory:");
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        c::sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    let mut errmsg: *mut ::std::os::raw::c_char = std::ptr::null_mut();
    let sql = cstr("SELECT * FROM non_existent_table");
    let ret = unsafe {
        c::sqlite3_exec(
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
        c::sqlite3_free((errmsg).cast());
    }
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_sqlite_prepare_v3_tail() {
    init_sqlite().await.unwrap();
    let filename = cstr(":memory:");
    let mut db = std::ptr::null_mut();

    let vfs = CString::new("opfs").unwrap();
    let ret = unsafe {
        c::sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            vfs.as_ptr(),
        )
    };
    assert_eq!(ret, SQLITE_OK);

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
                c::sqlite3_prepare_v3(db, remaining_sql, -1, 0, &mut stmt as _, &mut pz_tail as _);
            assert_eq!(ret, SQLITE_OK);

            let mut rc = c::sqlite3_step(stmt);

            while rc == c::SQLITE_ROW {
                for col in 0..c::sqlite3_column_count(stmt) {
                    let value = c::sqlite3_column_value(stmt, col);
                    let text = c::sqlite3_value_text(value);
                    let len = c::sqlite3_value_bytes(value);
                    let slice = slice::from_raw_parts(text, len as usize);
                    let text = std::str::from_utf8(slice).unwrap();
                    console_log!("Column {}: {:?}", col, text);
                }
                rc = c::sqlite3_step(stmt);
            }

            if rc == SQLITE_DONE {
                console_log!("SQL executed successfully.");
                c::sqlite3_finalize(stmt);
                remaining_sql = pz_tail;
            }
        }
    }
    unsafe {
        c::sqlite3_close_v2(db);
    }
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_mem_vfs() {
    init_sqlite().await.unwrap();

    let filename = cstr("test_mem_vfs.db");
    let mut db = std::ptr::null_mut();

    let ret = unsafe {
        c::sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    test_vfs(db);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_opfs_vfs() {
    init_sqlite().await.unwrap();

    let filename = cstr("test_opfs_vfs.db");
    let mut db = std::ptr::null_mut();

    let vfs = CString::new("opfs").unwrap();
    let ret = unsafe {
        c::sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            vfs.as_ptr(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    test_vfs(db);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_opfs_sah_vfs_default() {
    let sqlite = init_sqlite().await.unwrap();
    sqlite.install_opfs_sahpool(None).await.unwrap();

    let vfs = CString::new("opfs-sahpool").unwrap();
    let filename = cstr("test_opfs_sah_vfs_default.db");
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        c::sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            vfs.as_ptr(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    test_vfs(db);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_opfs_sah_vfs() {
    let sqlite = init_sqlite().await.unwrap();
    sqlite
        .install_opfs_sahpool(Some(&OpfsSAHPoolCfg {
            clear_on_init: true,
            initial_capacity: 6,
            directory: "costom".into(),
            name: "cvfs".into(),
            force_reinit_if_previously_failed: false,
        }))
        .await
        .unwrap();

    let vfs = CString::new("cvfs").unwrap();
    let filename = cstr("test_opfs_sah_vfs.db");
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        c::sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            vfs.as_ptr(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    test_vfs(db);
}

fn test_vfs(db: *mut sqlite3) {
    let mut errmsg: *mut ::std::os::raw::c_char = std::ptr::null_mut();
    // drop first
    let sql = cstr("DROP TABLE COMPANY;");
    let ret = unsafe {
        c::sqlite3_exec(
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
        c::sqlite3_exec(
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
        c::sqlite3_exec(
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
        c::sqlite3_exec(
            db,
            sql.as_ptr(),
            Some(f),
            std::ptr::null_mut(),
            &mut errmsg as *mut _,
        )
    };
    assert_eq!(SQLITE_OK, ret);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_opfs_sah_util() {
    let sqlite = init_sqlite().await.unwrap();
    let util = sqlite
        .install_opfs_sahpool(Some(&OpfsSAHPoolCfg {
            clear_on_init: true,
            initial_capacity: 6,
            directory: "test_util".into(),
            name: "avfs".into(),
            force_reinit_if_previously_failed: false,
        }))
        .await
        .unwrap();

    let filename = cstr("test_opfs_sah_util.db");
    let vfs = CString::new("avfs").unwrap();

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        c::sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            vfs.as_ptr(),
        )
    };
    test_vfs(db);

    let before = util.get_capacity();
    util.add_capacity(1).await;
    assert_eq!(before + 1, util.get_capacity());
    util.reduce_capacity(1).await;
    assert_eq!(before, util.get_capacity());
    util.reserve_minimum_capacity(before + 2).await;
    assert_eq!(before + 2, util.get_capacity());

    let before = util.get_file_count();
    assert_eq!(
        util.get_file_names(),
        vec!["/test_opfs_sah_util.db".to_string()]
    );
    assert!(util.export_file("1").is_err());
    let db = util.export_file("/test_opfs_sah_util.db").unwrap();
    assert!(util.import_db("1", vec![0]).is_err());
    util.import_db("new.db", db).unwrap();
    assert_eq!(before + 1, util.get_file_count());
    util.wipe_files().await;
    assert_eq!(0, util.get_file_count());

    util.remove_vfs().await;
}
