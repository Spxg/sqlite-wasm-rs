use crate::full::{check_persistent, check_result, prepare_simple_db};
use sqlite_wasm_rs::*;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn test_memory_vfs() {
    let mut db1 = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"file:test_memory_vfs.db?vfs=memvfs".as_ptr().cast(),
            &mut db1 as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    let state = check_persistent(db1);

    let ret = unsafe { sqlite3_close(db1) };
    assert_eq!(SQLITE_OK, ret);

    let mut db2 = std::ptr::null_mut();
    // is equivalent to the above
    let ret = unsafe {
        sqlite3_open_v2(
            c"test_memory_vfs.db".as_ptr().cast(),
            &mut db2 as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            c"memvfs".as_ptr().cast(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    assert_eq!(!state, check_persistent(db2));
    assert_eq!(unsafe { sqlite3_close(db2) }, SQLITE_OK);
}

#[wasm_bindgen_test]
fn test_memory_vfs_util() {
    let mut db1 = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"file:test_memory_vfs_util.db?vfs=memvfs".as_ptr().cast(),
            &mut db1 as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    prepare_simple_db(db1);
    let ret = unsafe { sqlite3_close(db1) };
    assert_eq!(SQLITE_OK, ret);

    let util = unsafe { vfs::memvfs::MemVfsUtil::get().unwrap() };
    assert!(util.exists("test_memory_vfs_util.db"));
    assert!(!util.exists("missing-memory-util.db"));
    assert!(!util.delete_db("missing-memory-util.db"));
    let before: usize = util.count();
    assert_eq!(before, util.list().len());

    let db = util.export_db("test_memory_vfs_util.db").unwrap();
    // Imports and opens must leave room for SQLite's longest journal suffix.
    for checked in [true, false] {
        for length in [500, 501] {
            let name = "é".repeat(250) + if length == 501 { "x" } else { "" };
            let result = if checked {
                util.import_db(&name, &db)
            } else {
                let page_size = vfs::check_import_db(&db).unwrap();
                util.import_db_unchecked(&name, &db, page_size)
            };
            if length == 501 {
                assert!(matches!(
                    result,
                    Err(vfs::memvfs::MemVfsError::InvalidFilename)
                ));
                assert!(!util.exists(&name));

                let filename = std::ffi::CString::new(name.as_str()).unwrap();
                let mut connection = std::ptr::null_mut();
                unsafe {
                    assert_eq!(
                        sqlite3_open_v2(
                            filename.as_ptr(),
                            &mut connection,
                            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
                            c"memvfs".as_ptr(),
                        ),
                        SQLITE_CANTOPEN
                    );
                    assert_eq!(sqlite3_close(connection), SQLITE_OK);
                }

                assert!(!util.exists(&name));
            } else {
                result.unwrap();
                let filename = std::ffi::CString::new(name.as_str()).unwrap();
                let mut connection = std::ptr::null_mut();
                unsafe {
                    assert_eq!(
                        sqlite3_open_v2(
                            filename.as_ptr(),
                            &mut connection,
                            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
                            c"memvfs".as_ptr(),
                        ),
                        SQLITE_OK
                    );
                    check_result(connection);
                    assert_eq!(
                        sqlite3_exec(
                            connection,
                            c"ATTACH 'memory-boundary-aux.db' AS aux;
                            PRAGMA main.page_size=512; VACUUM main;
                            PRAGMA aux.page_size=512;
                            CREATE TABLE main.boundary(n);
                            CREATE TABLE aux.boundary(n);
                            BEGIN;
                            INSERT INTO main.boundary VALUES(1);
                            INSERT INTO aux.boundary VALUES(1);
                            COMMIT; DETACH aux;"
                                .as_ptr(),
                            None,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                        ),
                        SQLITE_OK
                    );
                    assert_eq!(sqlite3_close(connection), SQLITE_OK);
                }

                assert!(util.delete_db("memory-boundary-aux.db"));
                assert!(util.delete_db(&name));
            }
            assert_eq!(util.count(), before);
        }
    }
    util.import_db("test_memory_vfs_util2.db", &db).unwrap();
    assert!(util.exists("test_memory_vfs_util2.db"));
    assert_eq!(util.count(), before + 1);
    assert!(util.import_db("test_memory_vfs_util2.db", &db).is_err());
    assert_eq!(util.export_db("test_memory_vfs_util2.db").unwrap(), db);
    assert_eq!(util.count(), before + 1);

    assert!(util.delete_db("test_memory_vfs_util.db"));
    assert!(!util.delete_db("test_memory_vfs_util.db"));
    assert!(!util.exists("test_memory_vfs_util.db"));
    assert_eq!(util.count(), before);

    let mut db2 = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"file:test_memory_vfs_util2.db?vfs=memvfs".as_ptr().cast(),
            &mut db2 as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    check_result(db2);
    assert_eq!(unsafe { sqlite3_close(db2) }, SQLITE_OK);
}
