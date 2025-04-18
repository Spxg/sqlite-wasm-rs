use sqlite_wasm_rs::{export::OpfsSAHPoolCfgBuilder, sahpool_vfs::install, *};
use std::ffi::{CStr, CString};
use wasm_bindgen_test::wasm_bindgen_test;

use crate::allin::{check_result, prepare_simple_db};

unsafe fn set_cipher(cipher: &str, db: *mut sqlite3) {
    let set_cipher = format!("PRAGMA cipher = {cipher};");
    let ret = sqlite3_exec(
        db,
        CString::new(set_cipher.clone()).unwrap().as_ptr().cast(),
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

async unsafe fn test_opfs_sah_vfs_cipher(cipher: &str) {
    let util = install(
        Some(
            &OpfsSAHPoolCfgBuilder::new()
                .vfs_name("cipher")
                .directory("cipher")
                .initial_capacity(20)
                .clear_on_init(true)
                .build(),
        ),
        true,
    )
    .await
    .unwrap();

    let mut db = std::ptr::null_mut();
    let db_name = format!("test_opfs_sah_vfs_{cipher}.db");

    let ret = sqlite3_open_v2(
        CString::new(db_name.clone()).unwrap().as_ptr().cast(),
        &mut db as *mut _,
        SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
        // https://utelle.github.io/SQLite3MultipleCiphers/docs/faq/faq_overview/#how-can-i-enable-encryption-for-a-non-default-sqlite-vfs
        c"multipleciphers-cipher".as_ptr().cast(),
    );
    assert_eq!(SQLITE_OK, ret);

    set_cipher(cipher, db);
    prepare_simple_db(db);
    check_result(db);
    sqlite3_close(db);

    let db1 = util.export_file(&db_name).unwrap();
    let new_db_name = format!("test_opfs_sah_vfs_{cipher}2.db");
    util.import_db(&new_db_name, &db1).unwrap();
    let db2 = util.export_file(&new_db_name).unwrap();
    assert_eq!(db1, db2);

    let mut db = std::ptr::null_mut();
    let ret = sqlite3_open_v2(
        CString::new(new_db_name).unwrap().as_ptr().cast(),
        &mut db as *mut _,
        SQLITE_OPEN_READWRITE,
        // https://utelle.github.io/SQLite3MultipleCiphers/docs/faq/faq_overview/#how-can-i-enable-encryption-for-a-non-default-sqlite-vfs
        c"multipleciphers-cipher".as_ptr().cast(),
    );
    assert_eq!(SQLITE_OK, ret);

    set_cipher(cipher, db);
    check_result(db);
    sqlite3_close(db);
}

macro_rules! test_cipher {
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

test_cipher!("aes128cbc");
test_cipher!("aes256cbc");
test_cipher!("chacha20");
test_cipher!("sqlcipher");
test_cipher!("rc4");
test_cipher!("ascon128");
