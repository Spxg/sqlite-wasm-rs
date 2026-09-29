use sqlite_wasm_rs::vfs::memvfs::MemVfsUtil;
use sqlite_wasm_rs::vfs::transfer::DbTransfer;
use sqlite_wasm_rs::vfs::VfsFilesManager;
use sqlite_wasm_rs::*;
use std::{ffi::CStr, mem::ManuallyDrop, ptr};
use wasm_bindgen_test::wasm_bindgen_test;

const KEY: &[u8] = b"My very secret passphrase";

struct Db(*mut sqlite3);

impl Db {
    fn open(name: &CStr, flags: i32) -> Self {
        let mut raw = ptr::null_mut();
        let code = unsafe { sqlite3_open_v2(name.as_ptr(), &mut raw, flags, ptr::null()) };
        let db = Self(raw);
        assert_eq!(code, SQLITE_OK);

        db
    }

    fn exec(&self, sql: &CStr) -> i32 {
        unsafe { sqlite3_exec(self.0, sql.as_ptr(), None, ptr::null_mut(), ptr::null_mut()) }
    }

    fn key(&self, key: &[u8]) {
        let len = i32::try_from(key.len()).unwrap();
        assert_eq!(
            unsafe { sqlite3_key(self.0, key.as_ptr().cast(), len) },
            SQLITE_OK
        );
    }

    fn rekey(&self, key: &[u8]) {
        let len = i32::try_from(key.len()).unwrap();
        assert_eq!(
            unsafe { sqlite3_rekey(self.0, key.as_ptr().cast(), len) },
            SQLITE_OK
        );
    }

    fn close(self) {
        let db = ManuallyDrop::new(self);
        assert_eq!(unsafe { sqlite3_close(db.0) }, SQLITE_OK);
    }

    /// Returns the text of the single row and column `sql` selects.
    fn query(&self, sql: &CStr) -> Vec<u8> {
        unsafe {
            let mut stmt = ptr::null_mut();
            assert_eq!(
                sqlite3_prepare_v2(self.0, sql.as_ptr(), -1, &mut stmt, ptr::null_mut()),
                SQLITE_OK
            );
            assert_eq!(sqlite3_step(stmt), SQLITE_ROW);
            let text = sqlite3_column_text(stmt, 0);
            assert!(!text.is_null());
            // SAFETY: SQLite returns a NUL-terminated string that stays valid until the next step.
            let value = CStr::from_ptr(text.cast()).to_bytes().to_vec();
            assert_eq!(sqlite3_step(stmt), SQLITE_DONE);
            assert_eq!(sqlite3_finalize(stmt), SQLITE_OK);

            value
        }
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        // Best effort for a test that panicked before `close`.
        unsafe { sqlite3_close(self.0) };
    }
}

fn create(name: &CStr) {
    let db = Db::open(name, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE);
    db.key(KEY);
    assert_eq!(
        db.exec(c"CREATE TABLE t(s); INSERT INTO t VALUES('hello sqlcipher');"),
        SQLITE_OK
    );
    db.close();
}

fn remove(name: &CStr) {
    unsafe { MemVfsUtil::get().unwrap() }
        .remove(name.to_str().unwrap())
        .unwrap();
}

#[wasm_bindgen_test]
fn test_encrypt_decrypt() {
    create(c"encrypted.db");

    let bytes = unsafe { MemVfsUtil::get().unwrap() }
        .export_db("encrypted.db")
        .unwrap();
    assert!(!bytes.windows(16).any(|w| w == b"SQLite format 3\0"));
    assert!(!bytes.windows(15).any(|w| w == b"hello sqlcipher"));

    for key in [None, Some(b"wrong passphrase".as_slice())] {
        let db = Db::open(c"encrypted.db", SQLITE_OPEN_READWRITE);
        if let Some(key) = key {
            db.key(key);
        }
        assert_ne!(db.exec(c"SELECT s FROM t;"), SQLITE_OK);
        db.close();
    }

    let db = Db::open(c"encrypted.db", SQLITE_OPEN_READWRITE);
    db.key(KEY);
    assert_eq!(db.query(c"SELECT s FROM t"), b"hello sqlcipher");
    db.close();
    remove(c"encrypted.db");
}

#[wasm_bindgen_test]
fn test_rekey() {
    create(c"rekey.db");

    let db = Db::open(c"rekey.db", SQLITE_OPEN_READWRITE);
    db.key(KEY);
    db.rekey(b"new passphrase");
    db.close();

    let db = Db::open(c"rekey.db", SQLITE_OPEN_READWRITE);
    db.key(KEY);
    assert_ne!(db.exec(c"SELECT s FROM t;"), SQLITE_OK);
    db.close();

    let db = Db::open(c"rekey.db", SQLITE_OPEN_READWRITE);
    db.key(b"new passphrase");
    assert_eq!(db.query(c"SELECT s FROM t"), b"hello sqlcipher");
    db.close();
    remove(c"rekey.db");
}
