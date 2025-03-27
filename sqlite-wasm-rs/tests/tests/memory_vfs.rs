use super::{cstr, test_vfs};
use sqlite_wasm_rs::export::*;
use wasm_bindgen_test::wasm_bindgen_test;

fn create_foo_table(db: *mut sqlite3) -> i32 {
    let sql = cstr(
        "CREATE TABLE IF NOT EXISTS FOO(
            ID INT PRIMARY KEY     NOT NULL,
            NAME           TEXT    NOT NULL );",
    );
    unsafe {
        sqlite3_exec(
            db,
            sql.as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }
}

fn drop_foo_table(db: *mut sqlite3) -> i32 {
    let sql = cstr("DROP TABLE FOO;");
    unsafe {
        sqlite3_exec(
            db,
            sql.as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }
}

#[wasm_bindgen_test]
#[allow(unused)]
fn test_memdb_vfs() {
    let mut db1 = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"file:/mem.db:?vfs=memdb".as_ptr().cast(),
            &mut db1 as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_ne!(SQLITE_OK, drop_foo_table(db1));
    assert_eq!(SQLITE_OK, create_foo_table(db1));
    let mut db2 = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"file:/mem.db:?vfs=memdb".as_ptr().cast(),
            &mut db2 as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(SQLITE_OK, drop_foo_table(db1));
    test_vfs(db2);
}

#[wasm_bindgen_test]
#[allow(unused)]
fn test_shared_cache_vfs() {
    let mut db1 = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"file::memory:?cache=shared".as_ptr().cast(),
            &mut db1 as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_ne!(SQLITE_OK, drop_foo_table(db1));
    assert_eq!(SQLITE_OK, create_foo_table(db1));
    let mut db2 = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"file::memory:?cache=shared".as_ptr().cast(),
            &mut db2 as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(SQLITE_OK, drop_foo_table(db1));
    test_vfs(db2);
}

#[wasm_bindgen_test]
#[allow(unused)]
fn test_memory_vfs() {
    let mut db1 = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"mem.db".as_ptr().cast(),
            &mut db1 as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            c"memvfs".as_ptr().cast(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    test_vfs(db1);

    let mut db2 = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"mem.db".as_ptr().cast(),
            &mut db2 as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            c"memvfs".as_ptr().cast(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    test_vfs(db2);
}
