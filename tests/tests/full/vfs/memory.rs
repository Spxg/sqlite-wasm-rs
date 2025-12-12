use crate::full::{check_persistent, check_result, prepare_simple_db};
use sqlite_wasm_rs::*;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn test_memory_vfs() {
    let mut db1 = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"file:test_memory_vfs.db?vfs=memvfs".as_ptr().cast(),
            &mut db1 as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    let state = check_persistent(db1);

    let ret = unsafe { sqlite3_close(db1) };
    assert_eq!(SQLITE_OK, ret);

    let mut db2 = std::ptr::null_mut();
    // is equivalent to the above
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_memory_vfs.db".as_ptr().cast(),
            &mut db2 as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            c"memvfs".as_ptr().cast(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    assert_eq!(!state, check_persistent(db2));
}

#[wasm_bindgen_test]
fn test_memory_vfs_util() {
    let mut db1 = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"file:test_memory_vfs_util.db?vfs=memvfs".as_ptr().cast(),
            &mut db1 as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    prepare_simple_db(db1);
    let ret = unsafe { sqlite3_close(db1) };
    assert_eq!(SQLITE_OK, ret);

    let util = MemVfsUtil::<sqlite_wasm_rs::WasmOsCallback>::new();
    assert!(util.exists("test_memory_vfs_util.db"));

    let db = util.export_db("test_memory_vfs_util.db").unwrap();
    util.import_db("test_memory_vfs_util2.db", &db).unwrap();
    assert!(util.exists("test_memory_vfs_util2.db"));

    util.delete_db("test_memory_vfs_util.db");
    assert!(!util.exists("test_memory_vfs_util.db"));

    let mut db2 = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"file:test_memory_vfs_util2.db?vfs=memvfs".as_ptr().cast(),
            &mut db2 as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    check_result(db2);
}
