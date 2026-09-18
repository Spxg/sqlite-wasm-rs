mod common;

use common::Db;
use sqlite_wasm_rs as ffi;
use sqlite_wasm_vfs::sahpool::{install, OpfsSAHError, OpfsSAHPoolCfg, OpfsSAHPoolCfgBuilder};
use wasm_bindgen_test::wasm_bindgen_test;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);

fn config(name: &str) -> OpfsSAHPoolCfg {
    OpfsSAHPoolCfgBuilder::new()
        .vfs_name(name)
        .directory(name)
        .initial_capacity(3)
        .clear_on_init(true)
        .build()
}

#[wasm_bindgen_test]
async fn install_rejects_foreign_vfs() {
    let memory = unsafe { ffi::vfs::memvfs::MemVfsUtil::get().unwrap() };
    let before = unsafe { ffi::vfs::registered_vfs("memvfs").unwrap().unwrap() };
    let count = memory.count();
    let result = install::<ffi::WasmOsCallback>(&config("memvfs"), false).await;

    assert!(
        matches!(result, Err(OpfsSAHError::Vfs(ffi::vfs::RegisterVfsError::NameConflict(name))) if name == "memvfs")
    );
    assert_eq!(
        unsafe { ffi::vfs::registered_vfs("memvfs").unwrap() },
        Some(before)
    );
    assert_eq!(memory.count(), count);
}

#[wasm_bindgen_test]
async fn lifecycle_reuses_paused_pool_and_reclaims_registration() {
    let cfg = config("test-pool-lifecycle");
    let pool = install::<ffi::WasmOsCallback>(&cfg, false).await.unwrap();
    pool.import_db_unchecked("keep.db", b"keep").unwrap();

    let original_default = unsafe { ffi::sqlite3_vfs_find(std::ptr::null()) };
    install::<ffi::WasmOsCallback>(&cfg, true).await.unwrap();
    assert_eq!(unsafe { ffi::sqlite3_vfs_find(std::ptr::null()) }, unsafe {
        ffi::sqlite3_vfs_find(c"test-pool-lifecycle".as_ptr())
    });

    pool.pause().unwrap();
    assert!(matches!(
        pool.add_capacity(1).await,
        Err(OpfsSAHError::Paused)
    ));
    assert!(matches!(pool.clear_all().await, Err(OpfsSAHError::Paused)));

    let reused = install::<ffi::WasmOsCallback>(&cfg, false).await.unwrap();
    assert!(reused.is_paused());

    reused.resume().await.unwrap();
    assert!(!pool.is_paused());
    assert_eq!(pool.export_db("keep.db").unwrap(), b"keep");

    let mut wrong = config("test-pool-lifecycle");
    wrong.directory = "another-directory".into();
    assert!(matches!(
        install::<ffi::WasmOsCallback>(&wrong, false).await,
        Err(OpfsSAHError::ConfigurationMismatch)
    ));

    unsafe {
        pool.uninstall().unwrap();
    }
    assert!(reused.is_uninstalled());
    assert!(matches!(
        reused.resume().await,
        Err(OpfsSAHError::Uninstalled)
    ));

    let mut reopen = cfg;
    reopen.clear_on_init = false;
    let replacement = install::<ffi::WasmOsCallback>(&reopen, false)
        .await
        .unwrap();
    assert_eq!(replacement.export_db("keep.db").unwrap(), b"keep");

    unsafe {
        replacement.uninstall().unwrap();
        ffi::sqlite3_vfs_register(original_default, 1);
    }
}

#[wasm_bindgen_test]
async fn database_names_reserve_room_for_all_journals() {
    let mut cfg = config("test-database-name-boundary");
    cfg.initial_capacity = 6;
    let pool = install::<ffi::WasmOsCallback>(&cfg, false).await.unwrap();
    let name = "é".repeat(249) + "x";
    let db = Db::open(
        &name,
        &cfg.vfs_name,
        ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
    )
    .unwrap();
    assert_eq!(
        db.exec(
            c"ATTACH 'aux.db' AS aux;
        CREATE TABLE main.t(n); CREATE TABLE aux.t(n);
        BEGIN; INSERT INTO main.t VALUES(1); INSERT INTO aux.t VALUES(1);
        COMMIT;"
        ),
        ffi::SQLITE_OK
    );
    drop(db);

    let bytes = pool.export_db(&name).unwrap();
    pool.delete_db(&name).unwrap();
    for checked in [true, false] {
        let import = |name: &str| {
            if checked {
                pool.import_db(name, &bytes)
            } else {
                pool.import_db_unchecked(name, &bytes)
            }
        };
        import(&name).unwrap();
        assert_eq!(pool.export_db(&name).unwrap(), bytes);
        pool.delete_db(&name).unwrap();

        for length in [500, 504, 511, 512] {
            let invalid = "x".repeat(length);
            assert!(matches!(
                import(&invalid),
                Err(OpfsSAHError::InvalidFilename(_))
            ));
            assert!(matches!(
                Db::open(
                    &invalid,
                    &cfg.vfs_name,
                    ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE
                ),
                Err(ffi::SQLITE_CANTOPEN)
            ));
            assert!(!pool.exists(&invalid));
        }
    }

    unsafe {
        pool.uninstall().unwrap();
    }
}

#[wasm_bindgen_test]
async fn restored_database_survives_resize_and_resume() {
    let cfg = config("test-restore-resize-resume");
    let pool = install::<ffi::WasmOsCallback>(&cfg, false).await.unwrap();
    let db = Db::open(
        "original.db",
        &cfg.vfs_name,
        ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
    )
    .unwrap();
    db.prepare();

    assert!(matches!(pool.pause(), Err(OpfsSAHError::FilesInUse)));
    db.check_rows();
    drop(db);

    let bytes = pool.export_db("original.db").unwrap();
    assert!(pool.delete_db("original.db").unwrap());
    pool.import_db("restored.db", &bytes).unwrap();
    assert!(pool.import_db("restored.db", &bytes).is_err());

    // Shrinking must remove only unused slots, never an imported database.
    let unused = pool.capacity() - 1;
    assert_eq!(pool.reduce_capacity(usize::MAX).await.unwrap(), unused);
    assert_eq!(pool.capacity(), 1);
    pool.ensure_capacity(3).await.unwrap();

    pool.pause().unwrap();
    assert!(matches!(
        Db::open("restored.db", &cfg.vfs_name, ffi::SQLITE_OPEN_READWRITE),
        Err(ffi::SQLITE_ERROR)
    ));
    pool.resume().await.unwrap();

    let db = Db::open("restored.db", &cfg.vfs_name, ffi::SQLITE_OPEN_READWRITE).unwrap();
    db.check_rows();

    // Restored spare slots must also support new journal writes.
    assert_eq!(
        db.exec(c"BEGIN; UPDATE employees SET salary=0; ROLLBACK;"),
        ffi::SQLITE_OK
    );
    db.check_rows();
    drop(db);

    pool.clear_all().await.unwrap();
    assert!(matches!(
        Db::open("restored.db", &cfg.vfs_name, ffi::SQLITE_OPEN_READWRITE),
        Err(ffi::SQLITE_CANTOPEN)
    ));

    unsafe {
        pool.uninstall().unwrap();
    }
}
