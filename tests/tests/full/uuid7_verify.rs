use super::{exec, prepare, text_from_col};
use sqlite_wasm_rs::*;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn test_uuid7_extension() {
    register_uuid7_extension();

    let mut db = std::ptr::null_mut();
    unsafe {
        let ret = sqlite3_open(c":memory:".as_ptr(), &mut db);
        assert_eq!(ret, SQLITE_OK);
    }

    // Basic generation and version check
    let stmt = prepare(db, "SELECT uuid7();");
    unsafe {
        assert_eq!(sqlite3_step(stmt), SQLITE_ROW);
        let val = text_from_col(stmt, 0);
        assert_eq!(val.len(), 36);
        assert_eq!(val.chars().nth(14).unwrap(), '7');
        sqlite3_finalize(stmt);
    }

    // Default column value
    exec(
        db,
        "CREATE TABLE users (id TEXT PRIMARY KEY DEFAULT (uuid7()), name TEXT);",
    );
    exec(db, "INSERT INTO users (name) VALUES ('Alice');");

    let stmt = prepare(db, "SELECT id FROM users WHERE name='Alice';");
    unsafe {
        assert_eq!(sqlite3_step(stmt), SQLITE_ROW);
        let val = text_from_col(stmt, 0);
        assert_eq!(val.chars().nth(14).unwrap(), '7');
        sqlite3_finalize(stmt);
    }

    // Monotonicity check
    let mut prev_uuid = String::new();
    for i in 0..10 {
        let stmt = prepare(db, "SELECT uuid7();");
        unsafe {
            assert_eq!(
                sqlite3_step(stmt),
                SQLITE_ROW,
                "step failed iteration {}",
                i
            );
            let val = text_from_col(stmt, 0);
            sqlite3_finalize(stmt);

            if !prev_uuid.is_empty() {
                assert!(
                    val > prev_uuid,
                    "UUIDv7 monotonicity fail: {} <= {}",
                    val,
                    prev_uuid
                );
            }
            prev_uuid = val;
        }
    }

    // ORDER BY check
    exec(db, "CREATE TABLE events (id TEXT PRIMARY KEY, ts INTEGER);");
    for i in 0..10 {
        exec(
            db,
            &format!("INSERT INTO events (id, ts) VALUES (uuid7(), {});", i),
        );
    }

    let stmt = prepare(db, "SELECT ts FROM events ORDER BY id ASC;");
    let mut prev_ts = -1;
    unsafe {
        while sqlite3_step(stmt) == SQLITE_ROW {
            let ts = sqlite3_column_int(stmt, 0);
            assert!(
                ts > prev_ts,
                "ORDER BY violation: ts {} came after {}",
                ts,
                prev_ts
            );
            prev_ts = ts;
        }
        sqlite3_finalize(stmt);
        sqlite3_close(db);
    }
}
