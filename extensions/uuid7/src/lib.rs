//! SQLite extension for UUIDv7 (Time-ordered) generation.
//!
//! # SQL Functions
//!
//! - `uuid7()`: Returns a new Version 7 UUID as a 36-character string.
//! - `uuid7_blob()`: Returns a new Version 7 UUID as a 16-byte BLOB. If called with 1 argument, converts the input UUID (TEXT or BLOB format) to a 16-byte BLOB.

use sqlite_wasm_rs::{
    sqlite3, sqlite3_api_routines, sqlite3_context, sqlite3_create_function_v2,
    sqlite3_result_blob, sqlite3_result_null, sqlite3_result_text, sqlite3_value,
    sqlite3_value_blob, sqlite3_value_bytes, sqlite3_value_text, sqlite3_value_type, SQLITE_BLOB,
    SQLITE_DETERMINISTIC, SQLITE_INNOCUOUS, SQLITE_OK, SQLITE_TEXT, SQLITE_TRANSIENT, SQLITE_UTF8,
};
use std::ffi::{c_char, c_int, c_void, CString};
use uuid::Uuid;

/// Helper function to parse a UUID from an SQLite argument value.
///
/// Supports two input formats:
/// - **TEXT**: A 32 (hex) or 36 (hyphenated) character string string.
/// - **BLOB**: A raw 16-byte UUID buffer.
///
/// # Arguments
/// * `argv` - Pointer to the array of sqlite3_value pointers.
/// * `index` - Index of the argument to check.
///
/// # Returns
/// * `Option<Uuid>` - The parsed UUID if valid, or `None` if invalid/wrong type.
unsafe fn parse_uuid_arg(argv: *mut *mut sqlite3_value, index: usize) -> Option<Uuid> {
    let arg = *argv.add(index);
    let ty = sqlite3_value_type(arg);

    match ty {
        SQLITE_TEXT => {
            let text_ptr = sqlite3_value_text(arg);
            if text_ptr.is_null() {
                return None;
            }
            let c_str = std::ffi::CStr::from_ptr(text_ptr as *const c_char);
            let s = c_str.to_str().ok()?;
            Uuid::parse_str(s).ok()
        }
        SQLITE_BLOB => {
            let blob_ptr = sqlite3_value_blob(arg);
            let bytes = sqlite3_value_bytes(arg);
            if bytes == 16 && !blob_ptr.is_null() {
                let slice = std::slice::from_raw_parts(blob_ptr as *const u8, 16);
                let array: [u8; 16] = slice.try_into().ok()?;
                Some(Uuid::from_bytes(array))
            } else {
                None
            }
        }
        _ => None,
    }
}

// --- SQL Functions ---

/// SQL Function: `uuid7()`
///
/// Generates a UUIDv7 (time-ordered) and returns it as a canonical 36-character string.
///
/// # SQL Examples
///
/// ```sql
/// SELECT uuid7();
/// -- Result example: '018e9a2b-8c00-7e00-8000-000000000000'
///
/// CREATE TABLE events (
///     id TEXT PRIMARY KEY DEFAULT (uuid7()),
///     description TEXT
/// );
/// ```
unsafe extern "C" fn uuid7_func(
    ctx: *mut sqlite3_context,
    _argc: c_int,
    _argv: *mut *mut sqlite3_value,
) {
    let u = Uuid::now_v7();
    let s = u.to_string(); // canonical 36-char string
    let c_str = CString::new(s).unwrap();
    sqlite3_result_text(ctx, c_str.as_ptr(), -1, SQLITE_TRANSIENT());
}

/// SQL Function: `uuid7_blob()`
///
/// - If 0 arguments: Generates a new UUIDv7 (time-ordered) and returns it as a 16-byte blob.
/// - If 1 argument: Parses the input UUID (string or blob) and returns it as a 16-byte blob.
///
/// # SQL Examples
///
/// ```sql
/// -- Generate new BLOB UUIDv7
/// SELECT hex(uuid7_blob());
/// -- Result example: '018E9A2B8C007E008000000000000000' (16 bytes)
///
/// -- Convert TEXT to BLOB
/// SELECT hex(uuid7_blob('018e9a2b-8c00-7e00-8000-000000000000'));
///
/// CREATE TABLE events (
///     id BLOB PRIMARY KEY DEFAULT (uuid7_blob()),
///     description TEXT
/// );
/// ```
unsafe extern "C" fn uuid7_blob_func(
    ctx: *mut sqlite3_context,
    argc: c_int,
    argv: *mut *mut sqlite3_value,
) {
    if argc == 0 {
        let u = Uuid::now_v7();
        let bytes = u.as_bytes();
        sqlite3_result_blob(ctx, bytes.as_ptr() as *const c_void, 16, SQLITE_TRANSIENT());
        return;
    }

    if let Some(u) = parse_uuid_arg(argv, 0) {
        let bytes = u.as_bytes();
        sqlite3_result_blob(ctx, bytes.as_ptr() as *const c_void, 16, SQLITE_TRANSIENT());
    } else {
        sqlite3_result_null(ctx);
    }
}

// --- Extension Entry Point ---

/// SQLite Extension Entry Point: `sqlite3_uuid7_init`
///
/// Registers the `uuid7` function with the SQLite database connection.
///
/// # Arguments
/// * `db` - The SQLite database connection.
/// * `_pz_err_msg` - Pointer to error message pointer (unused).
/// * `_p_api` - Pointer to SQLite API (unused, assuming linked implementation).
///
/// # Returns
/// * `SQLITE_OK` on success, or an error code.
#[no_mangle]
pub unsafe extern "C" fn sqlite3_uuid7_init(
    db: *mut sqlite3,
    _pz_err_msg: *mut *mut c_char,
    _p_api: *const sqlite3_api_routines,
) -> c_int {
    let flags = SQLITE_UTF8 | SQLITE_INNOCUOUS;
    let deterministic = flags | SQLITE_DETERMINISTIC;

    let rc = sqlite3_create_function_v2(
        db,
        c"uuid7".as_ptr(),
        0,
        flags,
        std::ptr::null_mut(),
        Some(uuid7_func),
        None,
        None,
        None,
    );
    if rc != SQLITE_OK {
        return rc;
    }

    // uuid7_blob() -> blob (time-ordered)
    let rc = sqlite3_create_function_v2(
        db,
        c"uuid7_blob".as_ptr(),
        0,
        flags,
        std::ptr::null_mut(),
        Some(uuid7_blob_func),
        None,
        None,
        None,
    );
    if rc != SQLITE_OK {
        return rc;
    }

    // uuid7_blob(x) -> blob
    let rc = sqlite3_create_function_v2(
        db,
        c"uuid7_blob".as_ptr(),
        1,
        deterministic,
        std::ptr::null_mut(),
        Some(uuid7_blob_func),
        None,
        None,
        None,
    );

    rc
}

/// Rust-friendly helper to register the extension.
///
/// Wraps `sqlite3_uuid7_init` for easier usage in Rust contexts where a raw `*mut c_void`
/// might be passed (e.g., from some FFI bindings).
///
/// # Arguments
/// * `db` - The SQLite database connection handle (casted to `*mut c_void`).
pub fn register(db: *mut c_void) -> c_int {
    unsafe { sqlite3_uuid7_init(db as *mut sqlite3, std::ptr::null_mut(), std::ptr::null()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{ffi::sqlite3_auto_extension, Connection};

    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    /// Tests the `uuid7` extension via `rusqlite`.
    ///
    /// This test verifies:
    /// 1. **Basic Generation**: usage of `uuid7()` returns a string of length 36.
    /// 2. **BLOB Generation**: usage of `uuid7_blob()` returns a blob of length 16.
    /// 3. **Uniqueness**: generating a batch of UUIDs results in no duplicates.
    /// 4. **Monotonicity**: subsequent calls return strictly greater UUIDs (time-ordered).
    #[wasm_bindgen_test::wasm_bindgen_test]
    fn test_uuid7_via_rusqlite() {
        unsafe {
            sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_uuid7_init as *const ())));
        }

        let conn = Connection::open_in_memory().unwrap();

        // 1. Basic Generation (TEXT): Ensure uuid7() returns a string of length 36
        let mut results = Vec::new();
        for _ in 0..100 {
            let u: String = conn.query_row("SELECT uuid7()", [], |r| r.get(0)).unwrap();
            results.push(u);
        }
        assert_eq!(results[0].len(), 36);

        // 2. Basic Generation (BLOB): Ensure uuid7 in blob mode works manually
        let blob: Vec<u8> = conn
            .query_row("SELECT uuid7_blob()", [], |r| r.get(0))
            .unwrap();
        assert_eq!(blob.len(), 16);
        let u_blob = Uuid::from_slice(&blob).unwrap();
        assert_eq!(u_blob.get_version_num(), 7);

        // 3. Conversion Support Check
        let input_text = u_blob.to_string();
        let blob_from_text: Vec<u8> = conn
            .query_row("SELECT uuid7_blob(?1)", [&input_text], |r| r.get(0))
            .unwrap();
        assert_eq!(blob_from_text, blob, "uuid7_blob(TEXT) failed");

        let blob_from_blob: Vec<u8> = conn
            .query_row("SELECT uuid7_blob(?1)", [&blob], |r| r.get(0))
            .unwrap();
        assert_eq!(blob_from_blob, blob, "uuid7_blob(BLOB) failed");

        // 4. Uniqueness: Ensure generated UUIDs are unique in a small batch
        let mut sorted = results.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            results.len(),
            sorted.len(),
            "UUIDv7 (Text) generated duplicates!"
        );

        // 4. Monotonicity: Ensure UUIDv7s are strictly increasing (time-ordered)
        for i in 0..results.len() - 1 {
            assert!(
                results[i] < results[i + 1],
                "UUIDv7 (Text) not sorted at index {}",
                i
            );
        }
    }

    /// Tests usage of `uuid7()` as a `DEFAULT` clause value.
    ///
    /// This test verifies:
    /// 1. **Bulk Insertion**: High-volume insertion (1000 rows) works within a transaction.
    /// 2. **Correctness**: All generated keys are present and have correct length.
    /// 3. **Ordering**: Default values maintain time-ordered sorting relative to insertion order.
    /// 4. **Version Validation**: All generated keys are valid UUID version 7.
    /// 5. **Blob Support**: Verifies `uuid7_blob()` works as a default value for BLOB columns.
    #[wasm_bindgen_test::wasm_bindgen_test]
    fn test_uuid7_default() {
        // Goal: verify uuid7 function usage as a DEFAULT clause in a table definition.
        unsafe {
            sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_uuid7_init as *const ())));
        }
        let mut conn = Connection::open_in_memory().unwrap();

        // Test 1: TEXT default
        conn.execute(
            "CREATE TABLE t(id TEXT PRIMARY KEY DEFAULT (uuid7()), val INTEGER)",
            [],
        )
        .unwrap();

        // 1. Bulk Insertion: Insert 1000 records to test performance and collision resistance in a transaction
        let tx = conn.transaction().unwrap();
        {
            let mut stmt = tx.prepare("INSERT INTO t(val) VALUES (?)").unwrap();
            for i in 0..1000 {
                stmt.execute([i]).unwrap();
            }
        }
        tx.commit().unwrap();

        // 2. Count verification
        let count: i64 = conn
            .query_row("SELECT count(*) FROM t WHERE length(id) = 36", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 1000);

        // 3. Order verification: UUIDv7s should effectively sort by creation time (proxying for insertion order 'val')
        let ids: Vec<String> = conn
            .prepare("SELECT id FROM t ORDER BY val")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        for i in 0..ids.len() - 1 {
            assert!(
                ids[i] < ids[i + 1],
                "UUIDv7 not sorted at index {}: {} >= {}",
                i,
                ids[i],
                ids[i + 1]
            );
        }

        // 4. Version Validation: Ensure all generated IDs are strictly UUIDv7
        for id in &ids {
            let u = Uuid::parse_str(id).unwrap();
            assert_eq!(u.get_version_num(), 7);
        }

        // Test 2: BLOB default using uuid7_blob()
        conn.execute("DROP TABLE t", []).unwrap();
        conn.execute(
            "CREATE TABLE t(id BLOB PRIMARY KEY DEFAULT (uuid7_blob()), val INTEGER)",
            [],
        )
        .unwrap();

        let tx = conn.transaction().unwrap();
        {
            let mut stmt = tx.prepare("INSERT INTO t(val) VALUES (?)").unwrap();
            for i in 0..100 {
                stmt.execute([i]).unwrap();
            }
        }
        tx.commit().unwrap();

        let count: i64 = conn
            .query_row("SELECT count(*) FROM t WHERE length(id) = 16", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 100);

        // Verify ordering (lexicographical check of blobs roughly equivalent to time ordering for UUIDv7)
        let blobs: Vec<Vec<u8>> = conn
            .prepare("SELECT id FROM t ORDER BY val")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        for i in 0..blobs.len() - 1 {
            // Rust's Vec<u8> comparison is lexicographical, which matches SQLite's memcmp and UUIDv7 constraints
            assert!(
                blobs[i] < blobs[i + 1],
                "UUIDv7 (Blob) not sorted at index {}",
                i
            );
        }
    }
}
