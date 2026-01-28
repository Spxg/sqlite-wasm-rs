use sqlite_wasm_rs::*;
use std::ffi::CStr;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn test_uuid_extension() {
    unsafe {
        // Initialize the UUID extension
        register_uuid_extension();

        // Open an in-memory database
        let mut db = std::ptr::null_mut();
        let ret = sqlite3_open(c":memory:".as_ptr(), &mut db);
        assert_eq!(ret, SQLITE_OK, "Failed to open in-memory database");

        // Prepare the statement to call uuid()
        let sql = c"SELECT uuid();";
        let mut stmt = std::ptr::null_mut();

        let ret = sqlite3_prepare_v2(db, sql.as_ptr(), -1, &mut stmt, std::ptr::null_mut());
        assert_eq!(ret, SQLITE_OK, "Failed to prepare statement");

        // Execute the statement
        let ret = sqlite3_step(stmt);
        assert_eq!(ret, SQLITE_ROW, "Expected a row from SELECT uuid()");

        // Verify the result
        let uuid_ptr = sqlite3_column_text(stmt, 0);
        assert!(!uuid_ptr.is_null(), "UUID result should not be null");

        let uuid_str = CStr::from_ptr(uuid_ptr as *const _).to_str().unwrap();
        assert_eq!(uuid_str.len(), 36, "UUID string length should be 36");

        // Cleanup statement
        sqlite3_finalize(stmt);

        // --- Test: UUID as default column value ---

        // Create table with UUID default
        let sql = c"CREATE TABLE users (id TEXT PRIMARY KEY DEFAULT (uuid()), name TEXT);";
        let mut err_msg = std::ptr::null_mut();
        let ret = sqlite3_exec(db, sql.as_ptr(), None, std::ptr::null_mut(), &mut err_msg);
        if ret != SQLITE_OK {
            let err = CStr::from_ptr(err_msg as *const _).to_str().unwrap();
            panic!("Failed to create table: {}", err);
        }

        // Insert row using default UUID
        let sql = c"INSERT INTO users (name) VALUES ('Alice');";
        let ret = sqlite3_exec(db, sql.as_ptr(), None, std::ptr::null_mut(), &mut err_msg);
        if ret != SQLITE_OK {
            let err = CStr::from_ptr(err_msg as *const _).to_str().unwrap();
            panic!("Failed to insert row: {}", err);
        }

        // Verify the inserted UUID
        let sql = c"SELECT id FROM users WHERE name = 'Alice';";
        let mut stmt = std::ptr::null_mut();
        let ret = sqlite3_prepare_v2(db, sql.as_ptr(), -1, &mut stmt, std::ptr::null_mut());
        assert_eq!(ret, SQLITE_OK, "Failed to prepare select statement");

        let ret = sqlite3_step(stmt);
        assert_eq!(ret, SQLITE_ROW, "Expected a row for Alice");

        let uuid_ptr = sqlite3_column_text(stmt, 0);
        assert!(!uuid_ptr.is_null(), "UUID column should not be null");
        let uuid_str = CStr::from_ptr(uuid_ptr as *const _).to_str().unwrap();
        assert_eq!(uuid_str.len(), 36, "UUID default value should be length 36");

        sqlite3_finalize(stmt);
        sqlite3_close(db);
    }
}
