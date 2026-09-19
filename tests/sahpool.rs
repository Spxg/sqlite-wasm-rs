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

#[wasm_bindgen_test]
async fn chunked_transfer_round_trips_sqlite_pages() {
    let cfg = config("test-chunked-transfer");
    let pool = install::<ffi::WasmOsCallback>(&cfg, false).await.unwrap();

    for page_size in [c"PRAGMA page_size=4096", c"PRAGMA page_size=65536"] {
        let db = Db::open(
            "source.db",
            &cfg.vfs_name,
            ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
        )
        .unwrap();
        assert_eq!(db.exec(page_size), ffi::SQLITE_OK);
        db.prepare();
        assert!(matches!(
            pool.begin_export("source.db"),
            Err(OpfsSAHError::FileInUse(_))
        ));
        drop(db);

        let expected = pool.export_db("source.db").unwrap();
        let mut export = pool.begin_export("source.db").unwrap();
        assert_eq!(export.size(), expected.len() as u64);
        assert!(matches!(
            pool.delete_db("source.db"),
            Err(OpfsSAHError::Busy)
        ));
        assert!(matches!(pool.pause(), Err(OpfsSAHError::Busy)));
        assert!(Db::open("source.db", &cfg.vfs_name, ffi::SQLITE_OPEN_READWRITE).is_err());

        let mut bytes = Vec::new();
        let mut buffer = [0; 4093];
        assert_eq!(export.read(&mut []).unwrap(), 0);
        loop {
            let count = export.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..count]);
        }
        assert_eq!(export.read(&mut buffer).unwrap(), 0);
        drop(export);
        assert_eq!(bytes, expected);
        pool.delete_db("source.db").unwrap();

        // Checked import resets WAL flags even when chunks split the header.
        bytes[18..20].copy_from_slice(&[2, 2]);
        let mut import = pool
            .begin_import("restored.db", bytes.len() as u64)
            .unwrap();
        assert!(!pool.exists("restored.db"));
        assert_eq!(pool.capacity(), 3);
        assert!(matches!(
            pool.reduce_capacity(1).await,
            Err(OpfsSAHError::Busy)
        ));
        assert!(matches!(pool.clear_all().await, Err(OpfsSAHError::Busy)));

        import.write(&bytes[..17]).unwrap();
        import.write(&[]).unwrap();
        for chunk in bytes[17..].chunks(4093) {
            import.write(chunk).unwrap();
        }
        import.finish().unwrap();
        assert_eq!(pool.export_db("restored.db").unwrap(), expected);

        pool.pause().unwrap();
        pool.resume().await.unwrap();
        let db = Db::open("restored.db", &cfg.vfs_name, ffi::SQLITE_OPEN_READWRITE).unwrap();
        db.check_rows();
        drop(db);
        pool.delete_db("restored.db").unwrap();
    }

    unsafe { pool.uninstall().unwrap() };
}

#[wasm_bindgen_test]
async fn incomplete_chunked_imports_never_publish_or_leak_slots() {
    let cfg = config("test-chunked-abort");
    let pool = install::<ffi::WasmOsCallback>(&cfg, false).await.unwrap();
    pool.reduce_capacity(2).await.unwrap();

    let mut import = pool.begin_import_unchecked("partial.db", 10).unwrap();
    import.write(b"partial").unwrap();
    drop(import);
    assert!(!pool.exists("partial.db"));

    let mut import = pool.begin_import_unchecked("partial.db", 10).unwrap();
    import.write(b"partial").unwrap();
    import.abort().unwrap();

    // Declaring a >4 GiB image must not allocate it or narrow its length.
    let size = (1u64 << 32) + 512;
    let mut import = pool.begin_import("large.db", size).unwrap();
    import.write(b"prefix").unwrap();
    assert!(
        matches!(import.finish(), Err(OpfsSAHError::ImportSizeMismatch { expected, actual: 6 }) if expected == size)
    );

    let mut import = pool.begin_import_unchecked("excess.db", 2).unwrap();
    assert!(matches!(
        import.write(b"abc"),
        Err(OpfsSAHError::ImportSizeMismatch { .. })
    ));
    assert!(matches!(import.finish(), Err(OpfsSAHError::ImportFailed)));

    let mut header = [0; 1024];
    for (page_size, valid_signature) in [(512u16, false), (513, true), (4096, true)] {
        header[..16].copy_from_slice(if valid_signature {
            b"SQLite format 3\0"
        } else {
            b"not a database!!"
        });
        header[16..18].copy_from_slice(&page_size.to_be_bytes());
        let mut import = pool
            .begin_import("invalid.db", header.len() as u64)
            .unwrap();
        import.write(&header).unwrap();
        assert!(matches!(import.finish(), Err(OpfsSAHError::ImportDb(_))));
    }

    assert!(pool.list().is_empty());
    assert_eq!(pool.capacity(), 1);
    pool.pause().unwrap();
    pool.resume().await.unwrap();
    assert!(pool.list().is_empty());

    // Reusing the only slot must not expose bytes left by an aborted import.
    let mut import = pool.begin_import_unchecked("raw.db", 3).unwrap();
    import.write(b"a").unwrap();
    import.write(b"bc").unwrap();
    import.finish().unwrap();
    assert_eq!(pool.export_db("raw.db").unwrap(), b"abc");
    assert!(matches!(
        pool.begin_import_unchecked("raw.db", 3),
        Err(OpfsSAHError::FileExists(_))
    ));
    pool.delete_db("raw.db").unwrap();

    pool.begin_import_unchecked("empty.db", 0)
        .unwrap()
        .finish()
        .unwrap();
    let mut export = pool.begin_export("empty.db").unwrap();
    assert_eq!(export.size(), 0);
    assert_eq!(export.read(&mut [0; 1]).unwrap(), 0);
    drop(export);

    unsafe { pool.uninstall().unwrap() };
}
