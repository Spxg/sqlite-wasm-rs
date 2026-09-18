use sqlite_wasm_rs::*;
use sqlite_wasm_vfs::sahpool::{
    install, OpfsSAHError, OpfsSAHPoolCfg, OpfsSAHPoolCfgBuilder, OpfsSAHPoolUtil,
};
use wasm_bindgen_test::wasm_bindgen_test;

pub async fn install_opfs_sahpool(
    options: &OpfsSAHPoolCfg,
    default_vfs: bool,
) -> Result<OpfsSAHPoolUtil, OpfsSAHError> {
    install::<sqlite_wasm_rs::WasmOsCallback>(options, default_vfs).await
}

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
        .initial_capacity(6usize)
        .build();
    let util = install_opfs_sahpool(&cfg, false).await.unwrap();
    assert!(util.capacity() >= cfg.initial_capacity);

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

    assert_eq!(unsafe { sqlite3_close(db) }, SQLITE_OK);

    let before: usize = util.capacity();
    assert_eq!(util.add_capacity(0).await.unwrap(), before);
    assert_eq!(util.reduce_capacity(0).await.unwrap(), 0);
    assert_eq!(util.add_capacity(1).await.unwrap(), before + 1);
    assert_eq!(before + 1, util.capacity());

    assert_eq!(util.reduce_capacity(1).await.unwrap(), 1);
    assert_eq!(before, util.capacity());

    util.ensure_capacity(before + 2).await.unwrap();
    assert_eq!(before + 2, util.capacity());
    util.ensure_capacity(before + 2).await.unwrap();
    util.ensure_capacity(0).await.unwrap();
    assert_eq!(before + 2, util.capacity());

    let before: usize = util.count();
    assert_eq!(before, util.list().len());
    assert_eq!(util.list(), vec!["test_opfs_sah_util.db".to_string()]);
    assert!(!util.exists("missing-opfs-util.db"));
    assert!(!util.delete_db("missing-opfs-util.db").unwrap());

    // export and import to new.db
    let db = util.export_db("test_opfs_sah_util.db").unwrap();
    util.import_db("new.db", &db).unwrap();
    assert!(util.exists("new.db"));
    assert!(util.import_db("new.db", &db).is_err());
    assert_eq!(util.export_db("new.db").unwrap(), db);
    assert_eq!(before + 1, util.count());

    let unused = util.capacity() - util.count();
    assert_eq!(util.reduce_capacity(usize::MAX).await.unwrap(), unused);
    assert_eq!(util.capacity(), util.count());
    assert_eq!(util.reduce_capacity(1).await.unwrap(), 0);
    // Restore a spare slot for SQLite's rollback journal.
    util.add_capacity(1).await.unwrap();

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

    assert_eq!(unsafe { sqlite3_close(db) }, SQLITE_OK);
    let capacity = util.capacity();
    assert!(util.delete_db("new.db").unwrap());
    assert!(!util.delete_db("new.db").unwrap());
    assert!(!util.exists("new.db"));
    assert_eq!(util.count(), before);
    assert_eq!(util.capacity(), capacity);
    util.clear_all().await.unwrap();
    assert_eq!(util.count(), 0);
    assert!(util.list().is_empty());
    assert_eq!(util.capacity(), capacity);
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

    util.pause().unwrap_err();

    unsafe { sqlite3_close(db) };

    assert!(!util.is_paused());

    util.pause().unwrap();
    assert!(util.is_paused());
    util.pause().unwrap();

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

    util.resume().await.unwrap();
    assert!(!util.is_paused());
    util.resume().await.unwrap();

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
