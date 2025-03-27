use super::{cstr, test_vfs};
use sqlite_wasm_rs::export::*;
use std::ffi::CString;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_opfs_sah_vfs_default() {
    install_opfs_sahpool(None, true).await.unwrap();

    let filename = cstr("test_opfs_sah_vfs_default.db");
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    test_vfs(db);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_opfs_sah_vfs_custom() {
    let cfg = OpfsSAHPoolCfgBuilder::new()
        .vfs_name("test-vfs-1")
        .directory("custom/jjehewhjfbhjwe")
        .build();
    install_opfs_sahpool(Some(&cfg), false).await.unwrap();

    let filename = cstr("test_opfs_sah_vfs_custom.db");
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            c"test-vfs-1".as_ptr().cast(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    test_vfs(db);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_opfs_sah_vfs_default_error() {
    install_opfs_sahpool(None, true).await.unwrap();

    let filename = cstr("test_opfs_sah_vfs_default_error.db");
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE,
            std::ptr::null_mut(),
        )
    };

    assert_eq!(SQLITE_CANTOPEN, ret);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_opfs_sah_util() {
    let cfg = OpfsSAHPoolCfgBuilder::new()
        .vfs_name("test-vfs")
        .directory("custom/ndjwndjw")
        .build();
    let util = install_opfs_sahpool(Some(&cfg), false).await.unwrap();

    let filename = cstr("test_opfs_sah_util.db");
    let vfs = CString::new("test-vfs").unwrap();

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
    assert!(util.import_db("1", &[0]).is_err());
    util.import_db("/new.db", &db).unwrap();
    assert_eq!(before + 1, util.get_file_count());

    let filename = cstr("new.db");
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
    util.reduce_capacity(util.get_capacity() - 6).await.unwrap();

    util.wipe_files().await.unwrap();
    assert_eq!(0, util.get_file_count());
}
