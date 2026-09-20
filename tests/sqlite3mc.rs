wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);

mod common;

use common::Db;
use sqlite_wasm_rs::vfs::transfer::DbTransfer;
use sqlite_wasm_rs::vfs::VfsFilesManager;
use sqlite_wasm_rs::*;
use sqlite_wasm_vfs::sahpool::{install, OpfsSAHPoolCfgBuilder};
use std::ffi::CString;
use wasm_bindgen_test::wasm_bindgen_test;

fn set_cipher(cipher: &str, db: &Db) {
    let sql = CString::new(format!("PRAGMA cipher = {cipher};")).unwrap();
    assert_eq!(db.exec(&sql), SQLITE_OK);
    assert_eq!(
        db.exec(c"PRAGMA key = 'My very secret passphrase';"),
        SQLITE_OK
    );
}

fn check_encrypted_copy(name: &str, vfs: &str, cipher: &str) {
    let db = Db::open(name, vfs, SQLITE_OPEN_READWRITE).unwrap();
    let sql = CString::new(format!("PRAGMA cipher = {cipher};")).unwrap();
    assert_eq!(db.exec(&sql), SQLITE_OK);

    // Without a key, reading must fail: ignored encryption PRAGMAs would
    // otherwise make a plaintext database pass the entire round trip.
    assert_ne!(db.exec(c"SELECT * FROM employees;"), SQLITE_OK);
    drop(db);

    let db = Db::open(name, vfs, SQLITE_OPEN_READWRITE).unwrap();
    set_cipher(cipher, &db);
    db.check_rows();
}

fn test_memvfs_cipher(cipher: &str) {
    let original = format!("memory-{cipher}.db");
    let restored = format!("memory-{cipher}-restored.db");
    // Non-default VFSes require the SQLite3MultipleCiphers wrapper.
    let vfs = "multipleciphers-memvfs";
    let db = Db::open(&original, vfs, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE).unwrap();
    set_cipher(cipher, &db);
    db.prepare();
    drop(db);

    let util = unsafe { vfs::memvfs::MemVfsUtil::get().unwrap() };
    let bytes = util.export_db(&original).unwrap();
    assert!(util.remove(&original).unwrap());
    util.import_db_unchecked(&restored, &bytes).unwrap();

    check_encrypted_copy(&restored, vfs, cipher);
    assert!(util.remove(&restored).unwrap());
}

async fn test_opfs_sah_vfs_cipher(cipher: &str) {
    let name = format!("sah-cipher-{cipher}");
    let cfg = OpfsSAHPoolCfgBuilder::new()
        .vfs_name(&name)
        .directory(&name)
        .initial_capacity(3)
        .clear_on_init(true)
        .build();
    let pool = install::<WasmOsCallback>(&cfg, false).await.unwrap();
    let vfs = format!("multipleciphers-{name}");
    let db = Db::open(
        "original.db",
        &vfs,
        SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
    )
    .unwrap();
    set_cipher(cipher, &db);
    db.prepare();
    drop(db);

    let bytes = pool.export_db("original.db").unwrap();
    assert!(pool.remove("original.db").unwrap());
    let mut import = pool
        .begin_import_unchecked("restored.db", bytes.len() as u64)
        .unwrap();
    for chunk in bytes.chunks(4093) {
        import.write(chunk).unwrap();
    }
    import.finish().unwrap();

    check_encrypted_copy("restored.db", &vfs, cipher);
    assert!(pool.remove("restored.db").unwrap());
    // Release OPFS handles but keep registration alive for the cipher wrapper.
    pool.pause().unwrap();
}

macro_rules! sah_sqlite3_mc {
    ($name:ident, $cipher:literal) => {
        #[wasm_bindgen_test::wasm_bindgen_test]
        async fn $name() {
            test_opfs_sah_vfs_cipher($cipher).await;
        }
    };
}

macro_rules! mem_sqlite3_mc {
    ($name:ident, $cipher:literal) => {
        #[wasm_bindgen_test]
        fn $name() {
            test_memvfs_cipher($cipher);
        }
    };
}

sah_sqlite3_mc!(test_opfs_sah_vfs_cipher_aes128cbc, "aes128cbc");
sah_sqlite3_mc!(test_opfs_sah_vfs_cipher_aes256cbc, "aes256cbc");
sah_sqlite3_mc!(test_opfs_sah_vfs_cipher_chacha20, "chacha20");
sah_sqlite3_mc!(test_opfs_sah_vfs_cipher_sqlcipher, "sqlcipher");
sah_sqlite3_mc!(test_opfs_sah_vfs_cipher_rc4, "rc4");
sah_sqlite3_mc!(test_opfs_sah_vfs_cipher_ascon128, "ascon128");

mem_sqlite3_mc!(test_memvfs_cipher_aes128cbc, "aes128cbc");
mem_sqlite3_mc!(test_memvfs_cipher_aes256cbc, "aes256cbc");
mem_sqlite3_mc!(test_memvfs_cipher_chacha20, "chacha20");
mem_sqlite3_mc!(test_memvfs_cipher_sqlcipher, "sqlcipher");
mem_sqlite3_mc!(test_memvfs_cipher_rc4, "rc4");
mem_sqlite3_mc!(test_memvfs_cipher_ascon128, "ascon128");
