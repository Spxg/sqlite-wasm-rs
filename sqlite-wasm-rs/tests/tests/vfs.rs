use super::{cstr, test_vfs};
use sqlite_wasm_rs::{
    export::*,
    idb::{install_idb_vfs, Preload},
};
use std::ffi::CString;
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

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_opfs_sah_vfs_default() {
    install_opfs_sahpool(None, true).await.unwrap();

    let filename = cstr("test_opfs_sah_vfs_default.db");
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    test_vfs(db);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_opfs_sah_vfs_custom() {
    let cfg = OpfsSAHPoolCfgBuilder::new()
        .vfs_name("test-vfs-1")
        .directory("custom/jjehewhjfbhjwe")
        .build();
    install_opfs_sahpool(Some(&cfg), false).await.unwrap();

    let filename = cstr("test_opfs_sah_vfs_custom.db");
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            c"test-vfs-1".as_ptr().cast(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    test_vfs(db);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_opfs_sah_vfs_default_error() {
    install_opfs_sahpool(None, true).await.unwrap();

    let filename = cstr("test_opfs_sah_vfs_default_error.db");
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE,
            std::ptr::null_mut(),
        )
    };

    assert_eq!(SQLITE_CANTOPEN, ret);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_opfs_sah_util() {
    let cfg = OpfsSAHPoolCfgBuilder::new()
        .vfs_name("test-vfs")
        .directory("custom/ndjwndjw")
        .build();
    let util = install_opfs_sahpool(Some(&cfg), false).await.unwrap();

    let filename = cstr("test_opfs_sah_util.db");
    let vfs = CString::new("test-vfs").unwrap();

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
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
    assert!(util.import_db("1", &[0]).is_err());
    util.import_db("/new.db", &db).unwrap();
    assert_eq!(before + 1, util.get_file_count());

    let filename = cstr("new.db");
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            vfs.as_ptr(),
        )
    };
    test_vfs(db);
    util.reduce_capacity(util.get_capacity() - 6).await.unwrap();

    util.wipe_files().await.unwrap();
    assert_eq!(0, util.get_file_count());
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_indexed_db_vfs_page_size_after_create() {
    let util = install_idb_vfs("sqlite-wasm-rs", true, Preload::Empty)
        .await
        .unwrap();
    util.preload_db(vec!["test_indexed_db_vfs_page_size_after_create.db".into()])
        .await
        .unwrap();

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_indexed_db_vfs_page_size_after_create.db".as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null_mut(),
        )
    };

    let ret = unsafe {
        sqlite3_exec(
            db,
            c"PRAGMA page_size = 4096;".as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    test_vfs(db);

    let ret = unsafe {
        sqlite3_exec(
            db,
            c"PRAGMA page_size = 4096;".as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    let ret = unsafe {
        sqlite3_exec(
            db,
            c"PRAGMA page_size = 8192;".as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_ERROR, ret);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_indexed_db_vfs_page_size() {
    install_idb_vfs("sqlite-wasm-rs", true, Preload::All)
        .await
        .unwrap();

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_indexed_db_vfs_page_size.db".as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null_mut(),
        )
    };
    let ret = unsafe {
        sqlite3_exec(
            db,
            c"PRAGMA page_size = 8192;".as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    let ret = unsafe {
        sqlite3_exec(
            db,
            c"PRAGMA page_size = 4096;".as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_ERROR, ret);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_indexed_db_vfs_synchronous() {
    install_idb_vfs("sqlite-wasm-rs", true, Preload::All)
        .await
        .unwrap();

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_indexed_db_vfs_synchronous.db".as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null_mut(),
        )
    };
    let ret = unsafe {
        sqlite3_exec(
            db,
            c"PRAGMA synchronous = full;".as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_ERROR, ret);
}
