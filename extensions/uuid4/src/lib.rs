//! SQLite extension for UUIDv4 (Random) generation.
//!
//! # SQL Functions
//!
//! - `uuid()`: Returns a new random Version 4 UUID as a 36-character string.
//! - `uuid_str(X)`: Parses X (blob or text) and returns a canonical 36-char string.
//! - `uuid_blob(X)`: Converts X to a 16-byte blob, or generates a new one if no X.

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
            // Assuming UTF-8 valid string from SQLite, parse it
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

// --- UUID Functions ---

/// Implementation of the `uuid()` SQL function.
///
/// Generates a new random Version 4 UUID and returns it as a canonical string
/// (36 characters, hyphenated).
///
/// # SQL Examples
///
/// ```sql
/// SELECT uuid();
/// -- Result example: 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'
///
/// CREATE TABLE items (
///     id TEXT PRIMARY KEY DEFAULT (uuid()),
///     name TEXT
/// );
/// ```
unsafe extern "C" fn uuid_func(
    ctx: *mut sqlite3_context,
    _argc: c_int,
    _argv: *mut *mut sqlite3_value,
) {
    let u = Uuid::new_v4();
    let s = u.to_string();
    let c_str = CString::new(s).unwrap();
    sqlite3_result_text(ctx, c_str.as_ptr(), -1, SQLITE_TRANSIENT());
}

/// Implementation of the `uuid_str(X)` SQL function.
///
/// Parses the input UUID (string or blob) and returns it as a canonical 36-char string.
///
/// # SQL Examples
///
/// ```sql
/// -- Convert BLOB to TEXT
/// SELECT uuid_str(uuid_blob('a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'));
/// -- Result example: 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'
///
/// -- Normalize string format
/// SELECT uuid_str('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11');
/// -- Result example: 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'
/// ```
unsafe extern "C" fn uuid_str_func(
    ctx: *mut sqlite3_context,
    _argc: c_int,
    argv: *mut *mut sqlite3_value,
) {
    if let Some(u) = parse_uuid_arg(argv, 0) {
        let s = u.to_string();
        let c_str = CString::new(s).unwrap();
        sqlite3_result_text(ctx, c_str.as_ptr(), -1, SQLITE_TRANSIENT());
    } else {
        sqlite3_result_null(ctx);
    }
}

/// Implementation of the `uuid_blob(X)` SQL function.
///
/// - If 0 arguments: Generates a new random UUID as a 16-byte BLOB.
/// - If 1 argument: Parses the input UUID (string or blob) and returns it as a 16-byte raw BLOB.
///
/// # SQL Examples
///
/// ```sql
/// -- Generate new BLOB UUID
/// SELECT hex(uuid_blob());
/// -- Result example: 'A0EEBC999C0B4EF8BB6D6BB9BD380A11' (16 bytes)
///
/// -- Convert TEXT to BLOB
/// SELECT hex(uuid_blob('a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'));
/// -- Result example: 'A0EEBC999C0B4EF8BB6D6BB9BD380A11'
///
/// CREATE TABLE items (
///     id BLOB PRIMARY KEY DEFAULT (uuid_blob()),
///     name TEXT
/// );
/// ```
unsafe extern "C" fn uuid_blob_func(
    ctx: *mut sqlite3_context,
    argc: c_int,
    argv: *mut *mut sqlite3_value,
) {
    if argc == 0 {
        let u = Uuid::new_v4();
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

/// Initializer for the `uuid4` extension.
///
/// Registers the following SQL functions:
/// - `uuid()`: Generates a new random UUID.
/// - `uuid_str(X)`: Converts UUID X to canonical string format.
/// - `uuid_blob(X)`: Converts UUID X to raw blob format.
///
/// # Arguments
/// * `db` - The SQLite database connection.
/// * `_pz_err_msg` - Pointer to write error message (unused).
/// * `_p_api` - Pointer to SQLite API (unused, linked statically).
///
/// # Returns
/// * `SQLITE_OK` on success, or an SQLite error code on failure.
#[no_mangle]
pub unsafe extern "C" fn sqlite3_uuid4_init(
    db: *mut sqlite3,
    _pz_err_msg: *mut *mut c_char,
    _p_api: *const sqlite3_api_routines,
) -> c_int {
    // Note: We are statically linking against sqlite-wasm-rs which likely exposes the API dynamically or statically.
    // If we were a loadable extension, we'd need SQLITE_EXTENSION_INIT2(_p_api).
    // But here we are just linking.

    // Check if we need to initialize API?
    // Since we use sqlite_wasm_rs symbols directly, we don't use the p_api pointer thunking normally used by dynamic extensions.

    let flags = SQLITE_UTF8 | SQLITE_INNOCUOUS;
    let deterministic = flags | SQLITE_DETERMINISTIC;

    let rc = sqlite3_create_function_v2(
        db,
        c"uuid".as_ptr(),
        0,
        flags,
        std::ptr::null_mut(),
        Some(uuid_func),
        None,
        None,
        None,
    );
    if rc != SQLITE_OK {
        return rc;
    }

    let rc = sqlite3_create_function_v2(
        db,
        c"uuid_str".as_ptr(),
        1,
        deterministic,
        std::ptr::null_mut(),
        Some(uuid_str_func),
        None,
        None,
        None,
    );
    if rc != SQLITE_OK {
        return rc;
    }

    // uuid_blob() -> blob (random)
    let rc = sqlite3_create_function_v2(
        db,
        c"uuid_blob".as_ptr(),
        0,
        flags,
        std::ptr::null_mut(),
        Some(uuid_blob_func),
        None,
        None,
        None,
    );
    if rc != SQLITE_OK {
        return rc;
    }

    // uuid_blob(x) -> blob
    let rc = sqlite3_create_function_v2(
        db,
        c"uuid_blob".as_ptr(),
        1,
        deterministic,
        std::ptr::null_mut(),
        Some(uuid_blob_func),
        None,
        None,
        None,
    );

    rc
}

/// Helper function to register the extension with an existing database connection.
///
/// This function acts as a bridge between the raw C-style init function and Rust API.
pub fn register(db: *mut c_void) -> c_int {
    unsafe { sqlite3_uuid4_init(db as *mut sqlite3, std::ptr::null_mut(), std::ptr::null()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{ffi::sqlite3_auto_extension, Connection};
    use wasm_bindgen_test::wasm_bindgen_test;

    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    /// Tests the `uuid4` extension via `rusqlite`.
    ///
    /// This test verifies:
    /// 1. **Basic Generation**: `uuid()` returns valid 36-char strings that differ on subsequent calls.
    /// 2. **BLOB Functionality**: `uuid_blob(X)` correctly handles keys and returns 16 bytes.
    /// 3. **Input Handling**: `uuid_blob` and `uuid_str` accept both TEXT and BLOB inputs.
    /// 4. **Roundtrip Consistency**: `uuid_str(uuid_blob(X))` returns X for a valid UUID.
    #[wasm_bindgen_test]
    fn test_uuid4_via_rusqlite() {
        unsafe {
            sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_uuid4_init as *const ())));
        }
        let conn = Connection::open_in_memory().unwrap();

        // 1. Basic Generation: Verify uuid() returns different values with correct length
        let u1: String = conn.query_row("SELECT uuid()", [], |r| r.get(0)).unwrap();
        let u2: String = conn.query_row("SELECT uuid()", [], |r| r.get(0)).unwrap();
        assert_eq!(u1.len(), 36, "uuid() string format length check");
        assert_ne!(u1, u2, "Consecutive uuid() calls must differ");

        // 2. BLOB Functionality: Verify uuid_blob returns 16 bytes for String input
        let blob_from_text: Vec<u8> = conn
            .query_row(
                "SELECT uuid_blob('00000000-0000-0000-0000-000000000000')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            blob_from_text.len(),
            16,
            "uuid_blob(TEXT) should produce 16 bytes"
        );
        assert_eq!(blob_from_text, vec![0; 16], "uuid_blob(TEXT) value check");

        // 3. Input Handling: Verify uuid_blob accepts BLOB input (Identity for 16-byte blob)
        // Pass the blob we just got back into uuid_blob
        let blob_from_blob: Vec<u8> = conn
            .query_row("SELECT uuid_blob(?1)", [&blob_from_text], |r| r.get(0))
            .unwrap();
        assert_eq!(
            blob_from_blob, blob_from_text,
            "uuid_blob(BLOB) should be identity"
        );

        // 4. Input Handling: Verify uuid_str accepts BLOB input
        let str_from_blob: String = conn
            .query_row("SELECT uuid_str(?1)", [&blob_from_text], |r| r.get(0))
            .unwrap();
        assert_eq!(
            str_from_blob, "00000000-0000-0000-0000-000000000000",
            "uuid_str(BLOB) failed"
        );

        // 5. Roundtrip Test: Verify uuid_str(uuid_blob(X)) == X consistency
        // This ensures the conversion functions are inverses of each other
        let input = "12345678-1234-1234-1234-123456789abc";
        let roundtrip: String = conn
            .query_row("SELECT uuid_str(uuid_blob(?1))", [input], |r| r.get(0))
            .unwrap();
        assert_eq!(roundtrip, input, "Roundtrip conversion failed");

        // 6. Generation: Verify uuid_blob() generates a new blob
        let len_gen: i64 = conn
            .query_row("SELECT length(uuid_blob())", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            len_gen, 16,
            "uuid_blob() (no args) should generate 16 bytes"
        );
    }

    /// Tests usage of `uuid()` and `uuid_blob()` as a `DEFAULT` clause value.
    ///
    /// This test verifies:
    /// 1. **Bulk Insertion**: High-volume insertion (1000 rows) works within a transaction.
    /// 2. **Uniqueness**: All generated default values are unique in the batch.
    /// 3. **Version Validation**: All generated keys are valid UUID version 4.
    /// 4. **Blob Support**: Verifies `uuid_blob()` works as a default value for BLOB columns.
    #[wasm_bindgen_test]
    fn test_uuid4_default() {
        // Goal: verify uuid() function usage as a DEFAULT clause in a table definition.

        unsafe {
            sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_uuid4_init as *const ())));
        }
        let mut conn = Connection::open_in_memory().unwrap();

        // Test 1: TEXT default
        conn.execute(
            "CREATE TABLE t(id TEXT PRIMARY KEY DEFAULT (uuid()), val INTEGER)",
            [],
        )
        .unwrap();

        let tx = conn.transaction().unwrap();
        {
            let mut stmt = tx.prepare("INSERT INTO t(val) VALUES (?)").unwrap();
            for i in 0..1000 {
                stmt.execute([i]).unwrap();
            }
        }
        tx.commit().unwrap();

        let count: i64 = conn
            .query_row("SELECT count(*) FROM t WHERE length(id) = 36", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 1000);

        let mut ids: Vec<String> = conn
            .prepare("SELECT id FROM t")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        for id in &ids {
            let u = Uuid::parse_str(id).unwrap();
            assert_eq!(u.get_version_num(), 4, "Generated UUID is not version 4");
        }

        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 1000, "Duplicate UUIDv4 generated!");

        // Test 2: BLOB default using uuid_blob() (direct generation)
        conn.execute("DROP TABLE t", []).unwrap();
        conn.execute(
            "CREATE TABLE t(id BLOB PRIMARY KEY DEFAULT (uuid_blob()), val INTEGER)",
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

        let mut blobs: Vec<Vec<u8>> = conn
            .prepare("SELECT id FROM t")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        for blob in &blobs {
            assert_eq!(blob.len(), 16);
            let u = Uuid::from_slice(blob).unwrap();
            assert_eq!(u.get_version_num(), 4);
        }

        blobs.sort();
        blobs.dedup();
        assert_eq!(blobs.len(), 100);
    }
}
