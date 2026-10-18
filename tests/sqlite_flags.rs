//! Build with `RUSTFLAGS="--cfg sqlite_wasm_rs_flags"` and
//! `SQLITE_WASM_RS_FLAGS="SQLITE_SOUNDEX SQLITE_ENABLE_NORMALIZE -DSQLITE_ENABLE_FTS3 -USQLITE_ENABLE_MATH_FUNCTIONS"`.
#![cfg(sqlite_wasm_rs_flags)]

use sqlite_wasm_rs::*;
use std::{ffi::CStr, ptr};
use wasm_bindgen_test::wasm_bindgen_test;

struct Db(*mut sqlite3);

impl Db {
    fn open() -> Self {
        let mut raw = ptr::null_mut();
        let code = unsafe { sqlite3_open(c":memory:".as_ptr(), &mut raw) };
        let db = Self(raw);
        assert_eq!(code, SQLITE_OK);

        db
    }

    fn exec(&self, sql: &CStr) -> i32 {
        unsafe { sqlite3_exec(self.0, sql.as_ptr(), None, ptr::null_mut(), ptr::null_mut()) }
    }

    fn errmsg(&self) -> String {
        unsafe { CStr::from_ptr(sqlite3_errmsg(self.0)) }
            .to_string_lossy()
            .into_owned()
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        unsafe { sqlite3_close(self.0) };
    }
}

#[wasm_bindgen_test]
fn test_bare_name_defines() {
    let db = Db::open();
    assert_eq!(
        db.exec(c"SELECT soundex('Robert')"),
        SQLITE_OK,
        "{}",
        db.errmsg()
    );
}

#[wasm_bindgen_test]
fn test_define_extends_builtins() {
    let db = Db::open();
    for sql in [
        c"CREATE VIRTUAL TABLE t3 USING fts3(x)",
        c"CREATE VIRTUAL TABLE t5 USING fts5(x)",
    ] {
        assert_eq!(db.exec(sql), SQLITE_OK, "{}", db.errmsg());
    }
}

#[wasm_bindgen_test]
fn test_undefine_overrides_builtin() {
    let db = Db::open();
    assert_eq!(db.exec(c"SELECT pow(2, 10)"), SQLITE_ERROR);
    assert_eq!(db.errmsg(), "no such function: pow");
}

#[wasm_bindgen_test]
fn test_bindgen_sees_flags() {
    let db = Db::open();
    let mut stmt = ptr::null_mut();
    let code =
        unsafe { sqlite3_prepare_v2(db.0, c"SELECT 1".as_ptr(), -1, &mut stmt, ptr::null_mut()) };
    assert_eq!(code, SQLITE_OK, "{}", db.errmsg());
    // Declared in `sqlite3.h` only under `SQLITE_ENABLE_NORMALIZE`.
    let normalized = unsafe { sqlite3_normalized_sql(stmt) };
    assert!(!normalized.is_null());
    assert_eq!(unsafe { CStr::from_ptr(normalized) }, c"SELECT?;");
    assert_eq!(unsafe { sqlite3_finalize(stmt) }, SQLITE_OK);
}
