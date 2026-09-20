use sqlite_wasm_rs::*;
use std::{ffi::CStr, ptr, slice};
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

    fn prepare(&self, sql: &CStr) -> Statement<'_> {
        let mut raw = ptr::null_mut();
        let code =
            unsafe { sqlite3_prepare_v2(self.0, sql.as_ptr(), -1, &mut raw, ptr::null_mut()) };
        let statement = Statement { raw, _db: self };
        assert_eq!(code, SQLITE_OK);

        statement
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        assert_eq!(unsafe { sqlite3_close(self.0) }, SQLITE_OK);
    }
}

struct Statement<'a> {
    raw: *mut sqlite3_stmt,
    _db: &'a Db,
}

impl Drop for Statement<'_> {
    fn drop(&mut self) {
        assert_eq!(unsafe { sqlite3_finalize(self.raw) }, SQLITE_OK);
    }
}

#[wasm_bindgen_test]
fn test_bindings() {
    let db = Db::open();
    assert_eq!(
        db.exec(c"CREATE TABLE t(i INTEGER, r REAL, s TEXT, b BLOB, n);"),
        SQLITE_OK
    );

    let integer = (1i64 << 40) + 123;
    let text = "SQLite 中文\0text".as_bytes();
    let blob = [0, 127, 128, 255];
    let insert = db.prepare(c"INSERT INTO t VALUES(?1, ?2, ?3, ?4, ?5)");

    unsafe {
        assert_eq!(sqlite3_bind_int64(insert.raw, 1, integer), SQLITE_OK);
        assert_eq!(sqlite3_bind_double(insert.raw, 2, 123.5), SQLITE_OK);

        let mut text_buffer = text.to_vec();
        let mut blob_buffer = blob.to_vec();
        assert_eq!(
            sqlite3_bind_text(
                insert.raw,
                3,
                text_buffer.as_ptr().cast(),
                text_buffer.len() as i32,
                SQLITE_TRANSIENT(),
            ),
            SQLITE_OK
        );
        assert_eq!(
            sqlite3_bind_blob(
                insert.raw,
                4,
                blob_buffer.as_ptr().cast(),
                blob_buffer.len() as i32,
                SQLITE_TRANSIENT(),
            ),
            SQLITE_OK
        );
        assert_eq!(sqlite3_bind_null(insert.raw, 5), SQLITE_OK);

        // TRANSIENT must copy both buffers before returning from bind.
        text_buffer.fill(b'x');
        blob_buffer.fill(42);
        drop(text_buffer);
        drop(blob_buffer);
        assert_eq!(sqlite3_step(insert.raw), SQLITE_DONE);
    }
    drop(insert);

    let query = db.prepare(c"SELECT i, r, s, b, n FROM t");

    unsafe {
        assert_eq!(sqlite3_step(query.raw), SQLITE_ROW);
        for (column, kind) in [
            SQLITE_INTEGER,
            SQLITE_FLOAT,
            SQLITE_TEXT,
            SQLITE_BLOB,
            SQLITE_NULL,
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(sqlite3_column_type(query.raw, column as i32), kind);
        }
        assert_eq!(sqlite3_column_int64(query.raw, 0), integer);
        assert_eq!(sqlite3_column_double(query.raw, 1), 123.5);
        assert_eq!(sqlite3_column_bytes(query.raw, 2) as usize, text.len());
        assert_eq!(
            slice::from_raw_parts(sqlite3_column_text(query.raw, 2), text.len()),
            text
        );
        assert_eq!(sqlite3_column_bytes(query.raw, 3) as usize, blob.len());
        assert_eq!(
            slice::from_raw_parts(sqlite3_column_blob(query.raw, 3).cast::<u8>(), blob.len()),
            blob
        );
        assert_eq!(sqlite3_step(query.raw), SQLITE_DONE);
    }
}

#[wasm_bindgen_test]
fn test_statement_reuse() {
    let db = Db::open();
    let query = db.prepare(c"SELECT ?1");

    unsafe {
        assert_eq!(sqlite3_bind_int64(query.raw, 1, 41), SQLITE_OK);
        assert_eq!(sqlite3_step(query.raw), SQLITE_ROW);
        assert_eq!(sqlite3_column_int64(query.raw, 0), 41);

        // Reset preserves bindings; rebinding replaces them.
        assert_eq!(sqlite3_reset(query.raw), SQLITE_OK);
        assert_eq!(sqlite3_step(query.raw), SQLITE_ROW);
        assert_eq!(sqlite3_column_int64(query.raw, 0), 41);

        assert_eq!(sqlite3_reset(query.raw), SQLITE_OK);
        assert_eq!(sqlite3_bind_int64(query.raw, 1, 42), SQLITE_OK);
        assert_eq!(sqlite3_step(query.raw), SQLITE_ROW);
        assert_eq!(sqlite3_column_int64(query.raw, 0), 42);

        assert_eq!(sqlite3_reset(query.raw), SQLITE_OK);
        assert_eq!(sqlite3_clear_bindings(query.raw), SQLITE_OK);
        assert_eq!(sqlite3_step(query.raw), SQLITE_ROW);
        assert_eq!(sqlite3_column_type(query.raw, 0), SQLITE_NULL);
        assert_eq!(sqlite3_step(query.raw), SQLITE_DONE);
    }
}

#[wasm_bindgen_test]
fn test_transactions() {
    let db = Db::open();
    assert_eq!(
        db.exec(c"CREATE TABLE t(n); INSERT INTO t VALUES(10);"),
        SQLITE_OK
    );

    let sum = || {
        let query = db.prepare(c"SELECT sum(n) FROM t");

        unsafe {
            assert_eq!(sqlite3_step(query.raw), SQLITE_ROW);

            let value = sqlite3_column_int64(query.raw, 0);
            assert_eq!(sqlite3_step(query.raw), SQLITE_DONE);

            value
        }
    };

    assert_eq!(
        db.exec(c"BEGIN; UPDATE t SET n=20; INSERT INTO t VALUES(30);"),
        SQLITE_OK
    );
    assert_eq!(unsafe { sqlite3_get_autocommit(db.0) }, 0);
    assert_eq!(db.exec(c"COMMIT;"), SQLITE_OK);
    assert_eq!(unsafe { sqlite3_get_autocommit(db.0) }, 1);
    assert_eq!(sum(), 50);

    assert_eq!(
        db.exec(c"BEGIN; DELETE FROM t; INSERT INTO t VALUES(99);"),
        SQLITE_OK
    );
    assert_eq!(sum(), 99);
    assert_eq!(db.exec(c"ROLLBACK;"), SQLITE_OK);
    assert_eq!(unsafe { sqlite3_get_autocommit(db.0) }, 1);
    assert_eq!(sum(), 50);
}

#[wasm_bindgen_test]
fn test_errors() {
    let db = Db::open();
    assert_eq!(db.exec(c"SELEC 1;"), SQLITE_ERROR);
    unsafe {
        assert_eq!(sqlite3_errcode(db.0), SQLITE_ERROR);
        assert!(!CStr::from_ptr(sqlite3_errmsg(db.0)).is_empty());
    }

    assert_eq!(
        db.exec(c"CREATE TABLE t(n UNIQUE); INSERT INTO t VALUES(1);"),
        SQLITE_OK
    );
    assert_eq!(db.exec(c"INSERT INTO t VALUES(1);"), SQLITE_CONSTRAINT);
    unsafe {
        assert_eq!(sqlite3_errcode(db.0), SQLITE_CONSTRAINT);
        assert_eq!(sqlite3_extended_errcode(db.0), SQLITE_CONSTRAINT_UNIQUE);
    }

    assert_eq!(db.exec(c"INSERT INTO t VALUES(2);"), SQLITE_OK);

    let query = db.prepare(c"SELECT count(*) FROM t");

    unsafe {
        assert_eq!(sqlite3_step(query.raw), SQLITE_ROW);
        assert_eq!(sqlite3_column_int(query.raw, 0), 2);
        assert_eq!(sqlite3_step(query.raw), SQLITE_DONE);
    }
}
