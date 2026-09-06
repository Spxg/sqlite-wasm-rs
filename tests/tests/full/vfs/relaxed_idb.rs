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

    util.barrier("test_idb_vfs_utils.db")
        .unwrap()
        .await
        .unwrap();

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
async fn test_idb_vfs_barrier_reports_failure_before_retrying_dirty_blocks() {
    let util = install_idb_vfs(
        &RelaxedIdbCfgBuilder::new()
            .vfs_name("relaxed-idb-barrier-failure")
            .clear_on_init(true)
            .preload(Preload::None)
            .build(),
        true,
    )
    .await
    .unwrap();
    util.preload_db(vec!["test_idb_vfs_barrier_failure.db".into()])
        .await
        .unwrap();

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_idb_vfs_barrier_failure.db".as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    prepare_simple_db(db);
    unsafe { sqlite3_close(db) };

    util.fail_next_commit();
    assert!(util
        .barrier("test_idb_vfs_barrier_failure.db")
        .unwrap()
        .await
        .is_err());
    assert!(util
        .barrier("test_idb_vfs_barrier_failure.db")
        .unwrap()
        .await
        .is_ok());
    util.forget_memory_file("test_idb_vfs_barrier_failure.db");
    util.preload_db(vec!["test_idb_vfs_barrier_failure.db".into()])
        .await
        .unwrap();
    assert!(!util
        .export_db("test_idb_vfs_barrier_failure.db")
        .unwrap()
        .is_empty());
}

#[wasm_bindgen_test]
async fn test_idb_vfs_barrier_poisoned_after_failed_delete() {
    let util = install_idb_vfs(
        &RelaxedIdbCfgBuilder::new()
            .vfs_name("relaxed-idb-barrier-delete")
            .clear_on_init(true)
            .preload(Preload::None)
            .build(),
        true,
    )
    .await
    .unwrap();
    util.preload_db(vec!["test_idb_vfs_barrier_delete.db".into()])
        .await
        .unwrap();
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_idb_vfs_barrier_delete.db".as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    prepare_simple_db(db);
    unsafe { sqlite3_close(db) };
    util.barrier("test_idb_vfs_barrier_delete.db")
        .unwrap()
        .await
        .unwrap();

    util.fail_next_commit();
    util.delete_db("test_idb_vfs_barrier_delete.db").unwrap();
    assert!(util
        .barrier("test_idb_vfs_barrier_delete.db")
        .unwrap()
        .await
        .is_err());
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_idb_vfs_barrier_delete.db".as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    prepare_simple_db(db);
    unsafe { sqlite3_close(db) };
    assert!(util
        .barrier("test_idb_vfs_barrier_delete.db")
        .unwrap()
        .await
        .is_err());
}

#[wasm_bindgen_test]
async fn test_idb_vfs_preserves_a_newer_write_during_commit() {
    let options = RelaxedIdbCfgBuilder::new()
        .vfs_name("relaxed-idb-overwrite")
        .clear_on_init(true)
        .preload(Preload::None)
        .build();
    let util = install_idb_vfs(&options, true).await.unwrap();
    let filename = "test_idb_vfs_overwrite.db";
    util.preload_db(vec![filename.into()]).await.unwrap();
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_idb_vfs_overwrite.db".as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    prepare_simple_db(db);
    unsafe { sqlite3_close(db) };
    let (gate, entered) = util.pause_after_snapshot();
    let waiting = install_idb_vfs(&options, false).await.unwrap();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = waiting.barrier(filename).unwrap().await;
    });
    entered.notified().await;

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_idb_vfs_overwrite.db".as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    let ret = unsafe {
        sqlite3_exec(
            db,
            c"CREATE TABLE newer_marker (value TEXT); INSERT INTO newer_marker VALUES ('newer');"
                .as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    unsafe { sqlite3_close(db) };
    gate.add_permits(1);
    util.barrier(filename).unwrap().await.unwrap();
    util.forget_memory_file(filename);
    util.preload_db(vec![filename.into()]).await.unwrap();
    let mut reopened = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_idb_vfs_overwrite.db".as_ptr(),
            &mut reopened as *mut _,
            SQLITE_OPEN_READWRITE,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    let ret = unsafe {
        sqlite3_exec(
            reopened,
            c"SELECT value FROM newer_marker".as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    unsafe { sqlite3_close(reopened) };
}

#[wasm_bindgen_test]
async fn test_idb_vfs_overflow_failures_poison_the_barrier() {
    let options = RelaxedIdbCfgBuilder::new()
        .vfs_name("relaxed-idb-overflow")
        .clear_on_init(true)
        .preload(Preload::None)
        .build();
    let util = install_idb_vfs(&options, true).await.unwrap();
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_idb_vfs_overflow_source.db".as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    prepare_simple_db(db);
    unsafe { sqlite3_close(db) };
    let bytes = util.export_db("test_idb_vfs_overflow_source.db").unwrap();
    util.fail_commits(33);
    for index in 0..33 {
        let name = format!("overflow-{index}.db");
        let _ = util.import_db(&name, &bytes).unwrap();
    }
    assert!(util.barrier("overflow-0.db").unwrap().await.is_err());
    assert!(util.barrier("overflow-0.db").unwrap().await.is_err());
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
