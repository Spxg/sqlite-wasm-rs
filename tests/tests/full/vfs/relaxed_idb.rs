use sqlite_wasm_rs::*;
use sqlite_wasm_vfs::relaxed_idb::{
    install, Preload, RelaxedIdbCfg, RelaxedIdbCfgBuilder, RelaxedIdbError, RelaxedIdbUtil,
};
use wasm_bindgen_test::wasm_bindgen_test;

use crate::full::{check_persistent, prepare_simple_db};

pub async fn install_idb_vfs(
    options: &RelaxedIdbCfg,
    default_vfs: bool,
) -> Result<RelaxedIdbUtil, RelaxedIdbError> {
    install::<sqlite_wasm_rs::WasmOsCallback>(options, default_vfs).await
}

#[wasm_bindgen_test]
async fn test_idb_vfs_default() {
    install_idb_vfs(&RelaxedIdbCfg::default(), true)
        .await
        .unwrap();

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_idb_vfs_default.db".as_ptr().cast(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    let state = check_persistent(db);
    assert_eq!(!state, check_persistent(db));
}

#[wasm_bindgen_test]
async fn test_idb_vfs_default_error() {
    install_idb_vfs(&RelaxedIdbCfg::default(), true)
        .await
        .unwrap();

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_idb_vfs_default_error.db".as_ptr().cast(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE,
            std::ptr::null_mut(),
        )
    };

    assert_eq!(SQLITE_CANTOPEN, ret);
}

#[wasm_bindgen_test]
async fn test_idb_vfs_custom() {
    install_idb_vfs(
        &RelaxedIdbCfgBuilder::new()
            .vfs_name("relaxed-idb-custom")
            .clear_on_init(true)
            .preload(Preload::None)
            .build(),
        false,
    )
    .await
    .unwrap();

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_idb_vfs_custom.db".as_ptr().cast(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            c"relaxed-idb-custom".as_ptr().cast(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    let state = check_persistent(db);
    assert_eq!(!state, check_persistent(db));
}

#[wasm_bindgen_test]
async fn test_idb_vfs_utils() {
    let util = install_idb_vfs(
        &RelaxedIdbCfgBuilder::new()
            .vfs_name("relaxed-idb-utils")
            .clear_on_init(true)
            .preload(Preload::All)
            .build(),
        false,
    )
    .await
    .unwrap();

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_idb_vfs_utils.db".as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            c"relaxed-idb-utils".as_ptr().cast(),
        )
    };

    assert_eq!(SQLITE_OK, ret);
    prepare_simple_db(db);

    unsafe {
        sqlite3_close(db);
    };

    // export and import to new.db
    let db = util.export_db("test_idb_vfs_utils.db").unwrap();
    util.import_db("new.db", &db).unwrap().await.unwrap();
    assert!(util.exists("new.db"));

    let mut db = std::ptr::null_mut();

    let ret = unsafe {
        sqlite3_open_v2(
            c"new.db".as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            c"relaxed-idb-utils".as_ptr().cast(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    prepare_simple_db(db);

    unsafe {
        sqlite3_close(db);
    };

    util.delete_db("test_idb_vfs_utils.db")
        .unwrap()
        .await
        .unwrap();
    util.delete_db("new.db").unwrap().await.unwrap();
}

#[wasm_bindgen_test]
async fn test_idb_vfs_set_page_size() {
    let util = install_idb_vfs(
        &RelaxedIdbCfgBuilder::new()
            .vfs_name("relaxed-idb-pagesize")
            .clear_on_init(true)
            .preload(Preload::None)
            .build(),
        true,
    )
    .await
    .unwrap();

    util.preload_db(vec!["test_idb_vfs_set_page_size.db".into()])
        .await
        .unwrap();

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_idb_vfs_set_page_size.db".as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
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
    assert_eq!(SQLITE_OK, ret);

    prepare_simple_db(db);

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
async fn test_idb_vfs_synchronous() {
    install_idb_vfs(
        &RelaxedIdbCfgBuilder::new()
            .vfs_name("relaxed-idb-synchronous")
            .build(),
        true,
    )
    .await
    .unwrap();

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_idb_vfs_synchronous.db".as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null_mut(),
        )
    };

    assert_eq!(SQLITE_OK, ret);

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
