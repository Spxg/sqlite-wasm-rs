use indexed_db_futures::database::Database;
use indexed_db_futures::prelude::*;
use indexed_db_futures::transaction::TransactionMode;
use js_sys::{Object, Reflect, Uint8Array};
use sqlite_wasm_rs::export::*;
use sqlite_wasm_rs::relaxed_idb_vfs::{install as install_idb_vfs, Preload, RelaxedIdbCfgBuilder};
use wasm_bindgen::JsValue;
use wasm_bindgen_test::{console_log, wasm_bindgen_test};

use crate::allin::{check_persistent, prepare_simple_db};

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
            &RelaxedIdbCfgBuilder::new()
                .vfs_name("relaxed-idb-custom")
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
        Some(
            &RelaxedIdbCfgBuilder::new()
                .vfs_name("relaxed-idb-utils")
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
            c"relaxed-idb-utils".as_ptr().cast(),
        )
    };

    assert_eq!(SQLITE_OK, ret);
    prepare_simple_db(db);

    unsafe {
        sqlite3_close(db);
    };

    // export and import to new.db
    let db = util.export_file("test_idb_vfs_utils.db").unwrap();
    util.import_db("new.db", &db).unwrap().await.unwrap();

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

    util.delete_file("test_idb_vfs_utils.db")
        .unwrap()
        .await
        .unwrap();
    util.delete_file("new.db").unwrap().await.unwrap();
}

#[wasm_bindgen_test]
async fn test_idb_vfs_set_page_size() {
    let util = install_idb_vfs(
        Some(
            &RelaxedIdbCfgBuilder::new()
                .vfs_name("relaxed-idb-pagesize")
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
async fn test_idb_vfs_synchronous() {
    install_idb_vfs(
        Some(
            &RelaxedIdbCfgBuilder::new()
                .vfs_name("relaxed-idb-synchronous")
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
        &JsValue::from(Uint8Array::new_with_length(block_size as u32)),
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
        "page_size {}k: write {count} block use {:?}, pre {:?}",
        block_size / 1024,
        elapsed,
        elapsed / count as u32
    );
}

async fn test_idb_vfs_preload(block_size: usize) {
    let now = web_time::Instant::now();
    let util = install_idb_vfs(
        Some(
            &RelaxedIdbCfgBuilder::new()
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
        "page_size {}k: read {count} block use {:?}, per {:?}",
        block_size / 1024,
        elapsed,
        elapsed / count as u32
    );
}

const SIZE: usize = 1024;

#[ignore]
#[wasm_bindgen_test]
async fn test_idb_prepare_all() {
    sqlite3_preload_prepare(SIZE * 1024 * 1024).await;
    sqlite3_preload_prepare(4096).await;
    sqlite3_preload_prepare(8192).await;
    sqlite3_preload_prepare(16384).await;
    sqlite3_preload_prepare(32768).await;
    sqlite3_preload_prepare(65536).await;
}

#[ignore]
#[wasm_bindgen_test]
async fn test_idb_vfs_preload_base() {
    test_idb_vfs_preload(SIZE * 1024 * 1024).await;
}

#[ignore]
#[wasm_bindgen_test]
async fn test_idb_vfs_preload_4k() {
    test_idb_vfs_preload(4096).await;
}

#[ignore]
#[wasm_bindgen_test]
async fn test_idb_vfs_preload_8k() {
    test_idb_vfs_preload(8192).await;
}

#[ignore]
#[wasm_bindgen_test]
async fn test_idb_vfs_preload_16k() {
    test_idb_vfs_preload(16384).await;
}

#[ignore]
#[wasm_bindgen_test]
async fn test_idb_vfs_preload_32k() {
    test_idb_vfs_preload(32768).await;
}

#[ignore]
#[wasm_bindgen_test]
async fn test_idb_vfs_preload_64k() {
    test_idb_vfs_preload(65536).await;
}
