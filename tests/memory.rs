wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);

mod common;

use common::Db;
use sqlite_wasm_rs::vfs::memvfs::{MemVfsError, MemVfsUtil};
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
    assert!(util.delete_db("memory-original.db"));
    util.import_db("memory-restored.db", &bytes).unwrap();

    // Reject replacement without damaging the existing database.
    assert!(util.import_db("memory-restored.db", &bytes).is_err());

    let db = Db::open("memory-restored.db", "memvfs", SQLITE_OPEN_READWRITE).unwrap();
    db.check_rows();
    drop(db);

    assert!(util.delete_db("memory-restored.db"));
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
    assert!(util.delete_db("memory-boundary-source.db"));
    let before = util.count();

    for checked in [true, false] {
        for length in [500, 501] {
            let name = "é".repeat(250) + if length == 501 { "x" } else { "" };
            let result = if checked {
                util.import_db(&name, &bytes)
            } else {
                let page_size = vfs::check_import_db(&bytes).unwrap();
                util.import_db_unchecked(&name, &bytes, page_size)
            };

            if length == 501 {
                assert!(matches!(result, Err(MemVfsError::InvalidFilename)));
                assert!(matches!(
                    Db::open(&name, "memvfs", SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE),
                    Err(SQLITE_CANTOPEN)
                ));
                assert!(!util.exists(&name));
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

                assert!(util.delete_db("memory-boundary-aux.db"));
                assert!(util.delete_db(&name));
            }

            assert_eq!(util.count(), before);
        }
    }
}
