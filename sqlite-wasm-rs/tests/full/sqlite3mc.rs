use sqlite_wasm_rs::{
    mem_vfs::MemVfsUtil, relaxed_idb_vfs::RelaxedIdbCfgBuilder, sahpool_vfs::OpfsSAHPoolCfgBuilder,
    *,
};
use std::ffi::{CStr, CString};
use wasm_bindgen_test::wasm_bindgen_test;

use crate::full::{check_result, prepare_simple_db};

unsafe fn set_cipher(cipher: &str, db: *mut sqlite3) {
    let set_cipher = format!("PRAGMA cipher = {cipher};");
    let c_name = CString::new(set_cipher.clone()).unwrap();
    let ret = sqlite3_exec(
        db,
        c_name.as_ptr().cast(),
        None,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    assert_eq!(ret, SQLITE_OK);

    let set_key = c"PRAGMA key = 'My very secret passphrase';";
    let ret = sqlite3_exec(
        db,
        set_key.as_ptr().cast(),
        None,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    assert_eq!(ret, SQLITE_OK);
}

unsafe fn test_memvfs_cipher(cipher: &str) {
    let mut db = std::ptr::null_mut();
    let db_name = format!("test_memvfs_vfs_{cipher}.db");

    let c_name = CString::new(db_name.clone()).unwrap();
    let ret = sqlite3_open_v2(
        c_name.as_ptr().cast(),
        &mut db as *mut _,
        SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
        // https://utelle.github.io/SQLite3MultipleCiphers/docs/faq/faq_overview/#how-can-i-enable-encryption-for-a-non-default-sqlite-vfs
        c"multipleciphers-memvfs".as_ptr().cast(),
    );
    assert_eq!(SQLITE_OK, ret);

    set_cipher(cipher, db);
    prepare_simple_db(db);
    check_result(db);
    let ret = sqlite3_close(db);
    assert_eq!(ret, SQLITE_OK);

    let util = MemVfsUtil::new();
    let db1 = util.export_db(&db_name).unwrap();
    let new_db_name = format!("test_memvfs_vfs_{cipher}2.db");
    util.import_db_unchecked(&new_db_name, &db1, 8192).unwrap();
    let db2 = util.export_db(&new_db_name).unwrap();
    assert_eq!(db1, db2);

    let c_name = CString::new(db_name.clone()).unwrap();
    let ret = sqlite3_open_v2(
        c_name.as_ptr().cast(),
        &mut db as *mut _,
        SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
        // https://utelle.github.io/SQLite3MultipleCiphers/docs/faq/faq_overview/#how-can-i-enable-encryption-for-a-non-default-sqlite-vfs
        c"multipleciphers-memvfs".as_ptr().cast(),
    );
    assert_eq!(SQLITE_OK, ret);

    set_cipher(cipher, db);
    check_result(db);
    let ret = sqlite3_close(db);
    assert_eq!(ret, SQLITE_OK);
}

async unsafe fn test_relaxed_db_vfs_cipher(cipher: &str) {
    let util = sqlite_wasm_rs::relaxed_idb_vfs::install(
        Some(
            &RelaxedIdbCfgBuilder::new()
                .vfs_name("relaxed-db-cipher")
                .clear_on_init(true)
                .build(),
        ),
        false,
    )
    .await
    .unwrap();

    let mut db = std::ptr::null_mut();
    let db_name = format!("test_relaxed_db_vfs_{cipher}.db");

    let c_name = CString::new(db_name.clone()).unwrap();
    let ret = sqlite3_open_v2(
        c_name.as_ptr().cast(),
        &mut db as *mut _,
        SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
        c"multipleciphers-relaxed-db-cipher".as_ptr().cast(),
    );
    assert_eq!(SQLITE_OK, ret);

    set_cipher(cipher, db);
    prepare_simple_db(db);
    check_result(db);
    let ret = sqlite3_close(db);
    assert_eq!(ret, SQLITE_OK);

    let db1 = util.export_db(&db_name).unwrap();
    let new_db_name = format!("test_relaxed_db_vfs_{cipher}2.db");
    util.import_db_unchecked(&new_db_name, &db1, 8192)
        .unwrap()
        .await
        .unwrap();
    let db2 = util.export_db(&new_db_name).unwrap();
    assert_eq!(db1, db2);

    let mut db = std::ptr::null_mut();
    let c_name = CString::new(new_db_name).unwrap();
    let ret = sqlite3_open_v2(
        c_name.as_ptr().cast(),
        &mut db as *mut _,
        SQLITE_OPEN_READWRITE,
        c"multipleciphers-relaxed-db-cipher".as_ptr().cast(),
    );
    assert_eq!(SQLITE_OK, ret);

    set_cipher(cipher, db);
    check_result(db);
    let ret = sqlite3_close(db);
    assert_eq!(ret, SQLITE_OK);
}

async unsafe fn test_opfs_sah_vfs_cipher(cipher: &str) {
    let util = sqlite_wasm_rs::sahpool_vfs::install(
        Some(
            &OpfsSAHPoolCfgBuilder::new()
                .vfs_name("sah-cipher")
                .directory("sah-cipher")
                .initial_capacity(20)
                .clear_on_init(true)
                .build(),
        ),
        false,
    )
    .await
    .unwrap();

    let mut db = std::ptr::null_mut();
    let db_name = format!("test_opfs_sah_vfs_{cipher}.db");

    let c_name = CString::new(db_name.clone()).unwrap();
    let ret = sqlite3_open_v2(
        c_name.as_ptr().cast(),
        &mut db as *mut _,
        SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
        c"multipleciphers-sah-cipher".as_ptr().cast(),
    );
    assert_eq!(SQLITE_OK, ret);

    set_cipher(cipher, db);
    prepare_simple_db(db);
    check_result(db);
    let ret = sqlite3_close(db);
    assert_eq!(ret, SQLITE_OK);

    let db1 = util.export_file(&db_name).unwrap();
    let new_db_name = format!("test_opfs_sah_vfs_{cipher}2.db");
    util.import_db_unchecked(&new_db_name, &db1).unwrap();
    let db2 = util.export_file(&new_db_name).unwrap();
    assert_eq!(db1, db2);

    let mut db = std::ptr::null_mut();
    let c_name = CString::new(new_db_name).unwrap();
    let ret = sqlite3_open_v2(
        c_name.as_ptr().cast(),
        &mut db as *mut _,
        SQLITE_OPEN_READWRITE,
        c"multipleciphers-sah-cipher".as_ptr().cast(),
    );
    assert_eq!(SQLITE_OK, ret);

    set_cipher(cipher, db);
    check_result(db);
    let ret = sqlite3_close(db);
    assert_eq!(ret, SQLITE_OK);
}

macro_rules! test_sah_cipher {
    ($cipher:literal) => {
        paste::paste! {
            #[wasm_bindgen_test]
            async fn [<test_opfs_sah_vfs_cipher_$cipher>]() {
                unsafe {
                    test_opfs_sah_vfs_cipher($cipher).await;
                }
            }
        }
    };
}

macro_rules! test_relaxed_db_cipher {
    ($cipher:literal) => {
        paste::paste! {
            #[wasm_bindgen_test]
            async fn [<test_relaxed_db_vfs_cipher_$cipher>]() {
                unsafe {
                    test_relaxed_db_vfs_cipher($cipher).await;
                }
            }
        }
    };
}

macro_rules! test_mem_cipher {
    ($cipher:literal) => {
        paste::paste! {
            #[wasm_bindgen_test]
            fn [<test_memvfs_cipher_$cipher>]() {
                unsafe {
                    test_memvfs_cipher($cipher);
                }
            }
        }
    };
}

test_sah_cipher!("aes128cbc");
test_sah_cipher!("aes256cbc");
test_sah_cipher!("chacha20");
test_sah_cipher!("sqlcipher");
test_sah_cipher!("rc4");
test_sah_cipher!("ascon128");

test_relaxed_db_cipher!("aes128cbc");
test_relaxed_db_cipher!("aes256cbc");
test_relaxed_db_cipher!("chacha20");
test_relaxed_db_cipher!("sqlcipher");
test_relaxed_db_cipher!("rc4");
test_relaxed_db_cipher!("ascon128");

test_mem_cipher!("aes128cbc");
test_mem_cipher!("aes256cbc");
test_mem_cipher!("chacha20");
test_mem_cipher!("sqlcipher");
test_mem_cipher!("rc4");
test_mem_cipher!("ascon128");
