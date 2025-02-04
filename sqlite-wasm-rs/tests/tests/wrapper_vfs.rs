use super::{cstr, test_vfs};
use sqlite_wasm_rs::export::*;
use std::ffi::CString;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_mem_vfs() {
    init_sqlite().await.unwrap();

    let filename = cstr("test_mem_vfs.db");
    let mut db = std::ptr::null_mut();

    let ret = unsafe {
        sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    test_vfs(db);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_opfs_vfs() {
    init_sqlite().await.unwrap();

    let filename = cstr("test_opfs_vfs.db");
    let mut db = std::ptr::null_mut();

    let vfs = CString::new("opfs").unwrap();
    let ret = unsafe {
        sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            vfs.as_ptr(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    test_vfs(db);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_opfs_sah_vfs_default() {
    let sqlite = init_sqlite().await.unwrap();
    sqlite.install_opfs_sahpool(None).await.unwrap();

    let vfs = CString::new("opfs-sahpool").unwrap();
    let filename = cstr("test_opfs_sah_vfs_default.db");
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            vfs.as_ptr(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    test_vfs(db);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_opfs_sah_vfs() {
    let sqlite = init_sqlite().await.unwrap();
    sqlite
        .install_opfs_sahpool(Some(&OpfsSAHPoolCfg {
            clear_on_init: true,
            initial_capacity: 6,
            directory: "costom".into(),
            name: "cvfs".into(),
            force_reinit_if_previously_failed: false,
        }))
        .await
        .unwrap();

    let vfs = CString::new("cvfs").unwrap();
    let filename = cstr("test_opfs_sah_vfs.db");
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            vfs.as_ptr(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    test_vfs(db);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_opfs_sah_util() {
    let sqlite = init_sqlite().await.unwrap();
    let util = sqlite
        .install_opfs_sahpool(Some(&OpfsSAHPoolCfg {
            clear_on_init: true,
            initial_capacity: 6,
            directory: "test_util".into(),
            name: "avfs".into(),
            force_reinit_if_previously_failed: false,
        }))
        .await
        .unwrap();

    let filename = cstr("test_opfs_sah_util.db");
    let vfs = CString::new("avfs").unwrap();

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
    assert!(util.import_db("1", vec![0]).is_err());
    util.import_db("new.db", db).unwrap();
    assert_eq!(before + 1, util.get_file_count());
    util.wipe_files().await;
    assert_eq!(0, util.get_file_count());

    util.remove_vfs().await;
}
