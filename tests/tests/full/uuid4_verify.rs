use super::{exec, prepare, text_from_col};
use sqlite_wasm_rs::*;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn test_uuid_extension() {
    register_uuid4_extension();

    let mut db = std::ptr::null_mut();
    unsafe {
        let ret = sqlite3_open(c":memory:".as_ptr(), &mut db);
        assert_eq!(ret, SQLITE_OK);
    }

    let stmt = prepare(db, "SELECT uuid();");
    unsafe {
        assert_eq!(sqlite3_step(stmt), SQLITE_ROW);
        let uuid = text_from_col(stmt, 0);
        assert_eq!(uuid.len(), 36);
        sqlite3_finalize(stmt);
    }

    exec(
        db,
        "CREATE TABLE users (id TEXT PRIMARY KEY DEFAULT (uuid()), name TEXT);",
    );
    exec(db, "INSERT INTO users (name) VALUES ('Alice');");

    let stmt = prepare(db, "SELECT id FROM users WHERE name = 'Alice';");
    unsafe {
        assert_eq!(sqlite3_step(stmt), SQLITE_ROW);
        let uuid = text_from_col(stmt, 0);
        assert_eq!(uuid.len(), 36);
        sqlite3_finalize(stmt);
    }

    unsafe { sqlite3_close(db) };
}
