use sqlite_wasm_rs::vfs::VfsFilesManager;
use sqlite_wasm_rs::*;
use std::{
    ffi::{CStr, CString},
    ptr,
};
use wasm_bindgen_test::wasm_bindgen_test;

struct Db(*mut sqlite3);

impl Db {
    fn open(name: &CStr, flags: i32) -> Self {
        let mut raw = ptr::null_mut();

        // The default VFS is the SQLite3MC wrapper around memvfs.
        let code = unsafe { sqlite3_open_v2(name.as_ptr(), &mut raw, flags, ptr::null()) };
        let db = Self(raw);
        assert_eq!(code, SQLITE_OK);

        db
    }

    fn exec(&self, sql: &CStr) -> i32 {
        unsafe { sqlite3_exec(self.0, sql.as_ptr(), None, ptr::null_mut(), ptr::null_mut()) }
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        assert_eq!(unsafe { sqlite3_close(self.0) }, SQLITE_OK);
    }
}

fn test_cipher(cipher: &str) {
    let name = CString::new(format!("cipher-{cipher}.db")).unwrap();
    let config = CString::new(format!("PRAGMA cipher='{cipher}';")).unwrap();
    let key = b"My very secret passphrase";

    let set_key = |db: &Db, key: &[u8]| {
        assert_eq!(
            unsafe { sqlite3_key(db.0, key.as_ptr().cast(), key.len() as i32) },
            SQLITE_OK
        );
    };

    let db = Db::open(&name, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE);
    assert_eq!(db.exec(&config), SQLITE_OK);
    set_key(&db, key);
    assert_eq!(
        db.exec(c"CREATE TABLE t(n); INSERT INTO t VALUES(42);"),
        SQLITE_OK
    );
    drop(db);

    for key in [None, Some(b"wrong passphrase".as_slice())] {
        let db = Db::open(&name, SQLITE_OPEN_READWRITE);
        assert_eq!(db.exec(&config), SQLITE_OK);

        if let Some(key) = key {
            set_key(&db, key);
        }

        assert_ne!(db.exec(c"SELECT n FROM t;"), SQLITE_OK);
    }

    let db = Db::open(&name, SQLITE_OPEN_READWRITE);
    assert_eq!(db.exec(&config), SQLITE_OK);
    set_key(&db, key);

    unsafe {
        let mut query = ptr::null_mut();
        assert_eq!(
            sqlite3_prepare_v2(
                db.0,
                c"SELECT n FROM t".as_ptr(),
                -1,
                &mut query,
                ptr::null_mut()
            ),
            SQLITE_OK
        );
        assert_eq!(sqlite3_step(query), SQLITE_ROW);
        assert_eq!(sqlite3_column_int(query, 0), 42);
        assert_eq!(sqlite3_step(query), SQLITE_DONE);
        assert_eq!(sqlite3_finalize(query), SQLITE_OK);
    }
    drop(db);

    // Remove the test fixture after every connection has closed.
    unsafe { vfs::memvfs::MemVfsUtil::get().unwrap() }
        .remove(name.to_str().unwrap())
        .unwrap();
}

macro_rules! cipher_test {
    ($name:ident, $cipher:literal) => {
        #[wasm_bindgen_test]
        fn $name() {
            test_cipher($cipher);
        }
    };
}

cipher_test!(test_cipher_aes128cbc, "aes128cbc");
cipher_test!(test_cipher_aes256cbc, "aes256cbc");
cipher_test!(test_cipher_chacha20, "chacha20");
cipher_test!(test_cipher_sqlcipher, "sqlcipher");
cipher_test!(test_cipher_rc4, "rc4");
cipher_test!(test_cipher_ascon128, "ascon128");
