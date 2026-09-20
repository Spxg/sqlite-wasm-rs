wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);

mod common;

use common::Db;
use sqlite_wasm_rs::vfs::memvfs::{MemVfsError, MemVfsUtil};
use sqlite_wasm_rs::vfs::transfer::DbTransfer;
use sqlite_wasm_rs::vfs::VfsFilesManager;
use sqlite_wasm_rs::*;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn export_import_preserves_rows_after_reopen() {
    let util = unsafe { MemVfsUtil::get().unwrap() };
    let db = Db::open(
        "file:memory-original.db?vfs=memvfs",
        "memvfs",
        SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_URI,
    )
    .unwrap();
    db.prepare();
    drop(db);

    // URI and plain filenames must refer to the same database.
    let db = Db::open("memory-original.db", "memvfs", SQLITE_OPEN_READWRITE).unwrap();
    db.check_rows();
    drop(db);

    let bytes = util.export_db("memory-original.db").unwrap();
    assert!(util.remove("memory-original.db").unwrap());
    let mut import =
        DbTransfer::begin_import(&util, "memory-restored.db", bytes.len() as u64).unwrap();
    import.write(&bytes[..17]).unwrap();
    import.write(&bytes[17..]).unwrap();
    assert!(!util.contains("memory-restored.db"));
    import.finish().unwrap();

    let mut export = DbTransfer::begin_export(&util, "memory-restored.db").unwrap();
    let mut prefix = [0; 17];
    export.read(&mut prefix).unwrap();
    assert_eq!(prefix, bytes[..17]);
    assert_eq!(export.read_to_vec().unwrap(), bytes[17..]);

    // Reject replacement without damaging the existing database.
    assert!(util.import_db("memory-restored.db", &bytes).is_err());

    let db = Db::open("memory-restored.db", "memvfs", SQLITE_OPEN_READWRITE).unwrap();
    db.check_rows();
    drop(db);

    assert!(util.remove("memory-restored.db").unwrap());
}

#[wasm_bindgen_test]
fn database_names_reserve_room_for_all_journals() {
    let util = unsafe { MemVfsUtil::get().unwrap() };
    let db = Db::open(
        "memory-boundary-source.db",
        "memvfs",
        SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
    )
    .unwrap();
    db.prepare();
    drop(db);

    let bytes = util.export_db("memory-boundary-source.db").unwrap();
    assert!(util.remove("memory-boundary-source.db").unwrap());
    let before = util.len();

    for checked in [true, false] {
        for length in [1012, 1013] {
            let name = "é".repeat(506) + if length == 1013 { "x" } else { "" };
            let result = if checked {
                util.import_db(&name, &bytes)
            } else {
                util.import_db_unchecked(&name, &bytes)
            };

            if length == 1013 {
                assert!(matches!(result, Err(MemVfsError::InvalidFilename)));
                assert!(matches!(
                    Db::open(&name, "memvfs", SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE),
                    Err(SQLITE_CANTOPEN)
                ));
                assert!(!util.contains(&name));
            } else {
                result.unwrap();

                let db =
                    Db::open(&name, "memvfs", SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE).unwrap();
                db.check_rows();
                assert_eq!(
                    db.exec(
                        c"
                        ATTACH 'memory-boundary-aux.db' AS aux;
                        PRAGMA main.page_size=512; VACUUM main;
                        PRAGMA aux.page_size=512;
                        CREATE TABLE main.boundary(n);
                        CREATE TABLE aux.boundary(n);
                        BEGIN;
                        INSERT INTO main.boundary VALUES(1);
                        INSERT INTO aux.boundary VALUES(1);
                        COMMIT; DETACH aux;
                    "
                    ),
                    SQLITE_OK
                );
                drop(db);

                assert!(util.remove("memory-boundary-aux.db").unwrap());
                assert!(util.remove(&name).unwrap());
            }

            assert_eq!(util.len(), before);
        }
    }
}
