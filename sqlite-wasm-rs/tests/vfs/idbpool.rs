use crate::vfs::{check_persistent, prepare_simple_db};
use indexed_db_futures::database::Database;
use indexed_db_futures::prelude::*;
use indexed_db_futures::transaction::TransactionMode;
use js_sys::{Object, Reflect};
use sqlite_wasm_rs::export::*;
use sqlite_wasm_rs::idbpool_vfs::{install as install_idb_vfs, IndexedDbPoolCfgBuilder, Preload};
use sqlite_wasm_rs::utils::copy_to_uint8_array;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::{console_log, wasm_bindgen_test};

#[wasm_bindgen_test]
async fn test_idb_vfs_default() {
    install_idb_vfs(None, true).await.unwrap();

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
    install_idb_vfs(None, true).await.unwrap();

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
        Some(
            &IndexedDbPoolCfgBuilder::new()
                .vfs_name("idbpool-custom")
                .clear_on_init(true)
                .preload(Preload::None)
                .build(),
        ),
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
            c"idbpool-custom".as_ptr().cast(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    let state = check_persistent(db);
    assert_eq!(!state, check_persistent(db));
}

#[wasm_bindgen_test]
async fn test_idb_vfs_utils() {
    let util = install_idb_vfs(
        Some(
            &IndexedDbPoolCfgBuilder::new()
                .vfs_name("idbpool-utils")
                .clear_on_init(true)
                .preload(Preload::All)
                .build(),
        ),
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
            c"idbpool-utils".as_ptr().cast(),
        )
    };

    assert_eq!(SQLITE_OK, ret);
    prepare_simple_db(db);

    unsafe {
        sqlite3_close(db);
    };

    // export and import to new.db
    let db = util.export_file("test_idb_vfs_utils.db").unwrap();
    util.import_db("new.db", &db, 512).await.unwrap();

    let mut db = std::ptr::null_mut();

    let ret = unsafe {
        sqlite3_open_v2(
            c"new.db".as_ptr(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            c"idbpool-utils".as_ptr().cast(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    unsafe {
        sqlite3_close(db);
    };

    util.delete_file("test_idb_vfs_utils.db").await.unwrap();
    util.delete_file("new.db").await.unwrap();
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_idb_vfs_set_page_size() {
    let util = install_idb_vfs(
        Some(
            &IndexedDbPoolCfgBuilder::new()
                .vfs_name("idbpool-pagesize")
                .clear_on_init(true)
                .preload(Preload::None)
                .build(),
        ),
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
#[allow(unused)]
async fn test_idb_vfs_synchronous() {
    install_idb_vfs(
        Some(
            &IndexedDbPoolCfgBuilder::new()
                .vfs_name("idbpool-synchronous")
                .build(),
        ),
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

const SIZE: usize = 50;

async fn sqlite3_preload_prepare(block_size: usize) {
    let indexed_db = Database::open("idb-preload")
        .with_version(1u8)
        .with_on_upgrade_needed(|_, db| {
            db.create_object_store("blocks")
                .with_key_path(["path", "offset"].into())
                .build()?;
            Ok(())
        })
        .await
        .unwrap();

    let transaction = indexed_db
        .transaction("blocks")
        .with_mode(TransactionMode::Readwrite)
        .build()
        .unwrap();
    let blocks = transaction.object_store("blocks").unwrap();

    let block = Object::new();
    Reflect::set(
        &block,
        &JsValue::from("path"),
        &JsValue::from(format!("test_idb_vfs_preload_{block_size}")),
    )
    .unwrap();
    Reflect::set(
        &block,
        &JsValue::from("data"),
        &JsValue::from(copy_to_uint8_array(&vec![0; block_size])),
    )
    .unwrap();

    let now = web_time::Instant::now();
    let count = SIZE * 1024 * 1024 / block_size;
    for offset in (0..).step_by(block_size).take(count) {
        Reflect::set(&block, &JsValue::from("offset"), &JsValue::from(offset)).unwrap();
        blocks.put(&block).build().unwrap();
    }
    transaction.commit().await.unwrap();
    let elapsed = now.elapsed();
    console_log!(
        "{block_size}: write {count} block use {:?}, pre {:?}",
        elapsed,
        elapsed / count as u32
    );
}

async fn test_idb_vfs_preload(block_size: usize) {
    let now = web_time::Instant::now();
    let util = install_idb_vfs(
        Some(
            &IndexedDbPoolCfgBuilder::new()
                .vfs_name("idb-preload")
                .preload(Preload::None)
                .build(),
        ),
        true,
    )
    .await
    .unwrap();
    util.preload_db(vec![format!("test_idb_vfs_preload_{block_size}")])
        .await
        .unwrap();
    let elapsed = now.elapsed();
    let count = SIZE * 1024 * 1024 / block_size;
    console_log!(
        "{block_size}: read {count} block use {:?}, per {:?}",
        elapsed,
        elapsed / count as u32
    );
}

#[ignore]
#[wasm_bindgen_test]
async fn test_idb_vfs_preload_64k() {
    sqlite3_preload_prepare(65536).await;
    test_idb_vfs_preload(65536).await;
}

#[ignore]
#[wasm_bindgen_test]
async fn test_idb_vfs_preload_4k() {
    sqlite3_preload_prepare(4096).await;
    test_idb_vfs_preload(4096).await;
}

#[ignore]
#[wasm_bindgen_test]
async fn test_idb_vfs_preload_8k() {
    sqlite3_preload_prepare(8192).await;
    test_idb_vfs_preload(8192).await;
}
