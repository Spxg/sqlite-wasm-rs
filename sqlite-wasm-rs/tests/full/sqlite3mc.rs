use sqlite_wasm_rs::{mem_vfs::MemVfsUtil, sahpool_vfs::OpfsSAHPoolCfgBuilder, *};
use std::ffi::CString;
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

#[cfg(feature = "relaxed-idb")]
async unsafe fn test_relaxed_idb_vfs_cipher(cipher: &str) {
    let util = sqlite_wasm_rs::relaxed_idb_vfs::install(
        &sqlite_wasm_rs::relaxed_idb_vfs::RelaxedIdbCfgBuilder::new()
            .vfs_name("relaxed-db-cipher")
            .clear_on_init(true)
            .build(),
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
        &OpfsSAHPoolCfgBuilder::new()
            .vfs_name("sah-cipher")
            .directory("sah-cipher")
            .initial_capacity(20)
            .clear_on_init(true)
            .build(),
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

    let db1 = util.export_db(&db_name).unwrap();
    let new_db_name = format!("test_opfs_sah_vfs_{cipher}2.db");
    util.import_db_unchecked(&new_db_name, &db1).unwrap();
    let db2 = util.export_db(&new_db_name).unwrap();
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

macro_rules! sah_sqlite3_mc {
    ($name:ident, $cipher:literal) => {
        #[wasm_bindgen_test::wasm_bindgen_test]
        async fn $name() {
            unsafe {
                test_opfs_sah_vfs_cipher($cipher).await;
            }
        }
    };
}

macro_rules! relaxed_idb_sqlite3_mc {
    ($name:ident, $cipher:literal) => {
        #[cfg(feature = "relaxed-idb")]
        #[wasm_bindgen_test]
        async fn $name() {
            unsafe {
                test_relaxed_idb_vfs_cipher($cipher).await;
            }
        }
    };
}

macro_rules! mem_sqlite3_mc {
    ($name:ident, $cipher:literal) => {
        #[wasm_bindgen_test]
        fn $name() {
            unsafe {
                test_memvfs_cipher($cipher);
            }
        }
    };
}

sah_sqlite3_mc!(test_opfs_sah_vfs_cipher_aes128cbc, "aes128cbc");
sah_sqlite3_mc!(test_opfs_sah_vfs_cipher_aes256cbc, "aes256cbc");
sah_sqlite3_mc!(test_opfs_sah_vfs_cipher_chacha20, "chacha20");
sah_sqlite3_mc!(test_opfs_sah_vfs_cipher_sqlcipher, "sqlcipher");
sah_sqlite3_mc!(test_opfs_sah_vfs_cipher_rc4, "rc4");
sah_sqlite3_mc!(test_opfs_sah_vfs_cipher_ascon128, "ascon128");

relaxed_idb_sqlite3_mc!(test_relaxed_idb_vfs_cipher_aes128cbc, "aes128cbc");
relaxed_idb_sqlite3_mc!(test_relaxed_idb_vfs_cipher_aes256cbc, "aes256cbc");
relaxed_idb_sqlite3_mc!(test_relaxed_idb_vfs_cipher_chacha20, "chacha20");
relaxed_idb_sqlite3_mc!(test_relaxed_idb_vfs_cipher_sqlcipher, "sqlcipher");
relaxed_idb_sqlite3_mc!(test_relaxed_idb_vfs_cipher_rc4, "rc4");
relaxed_idb_sqlite3_mc!(test_relaxed_idb_vfs_cipher_ascon128, "ascon128");

mem_sqlite3_mc!(test_memvfs_cipher_aes128cbc, "aes128cbc");
mem_sqlite3_mc!(test_memvfs_cipher_aes256cbc, "aes256cbc");
mem_sqlite3_mc!(test_memvfs_cipher_chacha20, "chacha20");
mem_sqlite3_mc!(test_memvfs_cipher_sqlcipher, "sqlcipher");
mem_sqlite3_mc!(test_memvfs_cipher_rc4, "rc4");
mem_sqlite3_mc!(test_memvfs_cipher_ascon128, "ascon128");
