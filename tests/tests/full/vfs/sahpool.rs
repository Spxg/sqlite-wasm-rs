use sqlite_wasm_rs::sahpool_vfs::{install as install_opfs_sahpool, *};
use sqlite_wasm_rs::*;
use wasm_bindgen_test::wasm_bindgen_test;

use crate::full::{check_persistent, prepare_simple_db};

#[wasm_bindgen_test]
async fn test_opfs_sah_vfs_default() {
    install_opfs_sahpool(&OpfsSAHPoolCfg::default(), true)
        .await
        .unwrap();

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_opfs_sah_vfs_default.db".as_ptr().cast(),
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
async fn test_opfs_sah_vfs_default_error() {
    install_opfs_sahpool(&OpfsSAHPoolCfg::default(), true)
        .await
        .unwrap();

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_opfs_sah_vfs_default_error.db".as_ptr().cast(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE,
            std::ptr::null_mut(),
        )
    };

    assert_eq!(SQLITE_CANTOPEN, ret);
}

#[wasm_bindgen_test]
async fn test_opfs_sah_vfs_custom() {
    let cfg = OpfsSAHPoolCfgBuilder::new()
        .vfs_name("test-vfs-1")
        .directory("custom/bar")
        .build();
    install_opfs_sahpool(&cfg, false).await.unwrap();

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_opfs_sah_vfs_custom.db".as_ptr().cast(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            c"test-vfs-1".as_ptr().cast(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    let state = check_persistent(db);
    assert_eq!(!state, check_persistent(db));
}

#[wasm_bindgen_test]
async fn test_opfs_sah_vfs_util() {
    let cfg = OpfsSAHPoolCfgBuilder::new()
        .vfs_name("test-vfs-2")
        .directory("custom/foo")
        .clear_on_init(true)
        .build();
    let util = install_opfs_sahpool(&cfg, false).await.unwrap();

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_opfs_sah_util.db".as_ptr().cast(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            c"test-vfs-2".as_ptr().cast(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    prepare_simple_db(db);

    let before = util.get_capacity();
    util.add_capacity(1).await.unwrap();
    assert_eq!(before + 1, util.get_capacity());

    util.reduce_capacity(1).await.unwrap();
    assert_eq!(before, util.get_capacity());

    util.reserve_minimum_capacity(before + 2).await.unwrap();
    assert_eq!(before + 2, util.get_capacity());

    let before = util.count();
    assert_eq!(util.list(), vec!["test_opfs_sah_util.db".to_string()]);

    // export and import to new.db
    let db = util.export_db("test_opfs_sah_util.db").unwrap();
    util.import_db("new.db", &db).unwrap();
    assert!(util.exists("new.db").unwrap_or_default());
    assert_eq!(before + 1, util.count());

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"new.db".as_ptr().cast(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            c"test-vfs-2".as_ptr().cast(),
        )
    };

    assert_eq!(SQLITE_OK, ret);

    let state = check_persistent(db);
    assert_eq!(!state, check_persistent(db));
}

#[wasm_bindgen_test]
async fn test_opfs_sah_vfs_pause() {
    let cfg = OpfsSAHPoolCfgBuilder::new()
        .vfs_name("test-vfs-pause")
        .directory("custom/pause-test")
        .build();
    let util = install_opfs_sahpool(&cfg, false).await.unwrap();

    //
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_pause.db".as_ptr().cast(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            c"test-vfs-pause".as_ptr().cast(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    prepare_simple_db(db);

    util.pause_vfs().unwrap_err();

    unsafe { sqlite3_close(db) };

    assert!(!util.is_paused());

    util.pause_vfs().unwrap();
    assert!(util.is_paused());
    util.pause_vfs().unwrap();

    let mut db2 = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_pause2.db".as_ptr().cast(),
            &mut db2 as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            c"test-vfs-pause".as_ptr().cast(),
        )
    };
    assert_ne!(SQLITE_OK, ret);

    util.unpause_vfs().await.unwrap();
    assert!(!util.is_paused());
    util.unpause_vfs().await.unwrap();

    let mut db3 = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_pause.db".as_ptr().cast(),
            &mut db3 as *mut _,
            SQLITE_OPEN_READWRITE,
            c"test-vfs-pause".as_ptr().cast(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    let state = check_persistent(db3);
    assert_eq!(!state, check_persistent(db3));

    unsafe { sqlite3_close(db3) };
}
