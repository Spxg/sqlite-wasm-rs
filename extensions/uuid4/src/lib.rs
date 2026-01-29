use std::ffi::{c_char, c_int, c_void, CString};
use sqlite_wasm_rs::{
    sqlite3, sqlite3_api_routines, sqlite3_context, sqlite3_value,
    sqlite3_create_function_v2, sqlite3_result_text, sqlite3_result_blob, sqlite3_result_null,
    sqlite3_value_type, sqlite3_value_text, sqlite3_value_blob, sqlite3_value_bytes,
    SQLITE_OK, SQLITE_UTF8, SQLITE_INNOCUOUS, SQLITE_DETERMINISTIC,
    SQLITE_TEXT, SQLITE_BLOB, SQLITE_TRANSIENT,
};
use uuid::Uuid;

// --- Helper: Parsing Input ---
// Input can be TEXT (32/36 chars) or BLOB (16 bytes).
// API returns Option<Uuid>
unsafe fn parse_uuid_arg(argv: *mut *mut sqlite3_value, index: usize) -> Option<Uuid> {
    let arg = *argv.add(index);
    let ty = sqlite3_value_type(arg);

    match ty {
        SQLITE_TEXT => {
            let text_ptr = sqlite3_value_text(arg);
            if text_ptr.is_null() { return None; }
            // Assuming UTF-8 valid string from SQLite, parse it
            let c_str = std::ffi::CStr::from_ptr(text_ptr as *const c_char);
            let s = c_str.to_str().ok()?;
            Uuid::parse_str(s).ok()
        },
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
        },
        _ => None
    }
}

// --- SQL Functions ---

// uuid() -> TEXT
unsafe extern "C" fn uuid_func(
    ctx: *mut sqlite3_context,
    _argc: c_int,
    _argv: *mut *mut sqlite3_value,
) {
    let u = Uuid::new_v4();
    let s = u.to_string(); // canonical 36-char string
    let c_str = CString::new(s).unwrap();
    sqlite3_result_text(ctx, c_str.as_ptr(), -1, SQLITE_TRANSIENT());
}

// uuid_str(X) -> TEXT
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

// uuid_blob(X) -> BLOB
unsafe extern "C" fn uuid_blob_func(
    ctx: *mut sqlite3_context,
    _argc: c_int,
    argv: *mut *mut sqlite3_value,
) {
    if let Some(u) = parse_uuid_arg(argv, 0) {
        let bytes = u.as_bytes();
        sqlite3_result_blob(ctx, bytes.as_ptr() as *const c_void, 16, SQLITE_TRANSIENT());
    } else {
        sqlite3_result_null(ctx);
    }
}

// --- Extension Entry Point ---

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
        None, None, None
    );
    if rc != SQLITE_OK { return rc; }

    let rc = sqlite3_create_function_v2(
        db,
        c"uuid_str".as_ptr(),
        1,
        deterministic,
        std::ptr::null_mut(),
        Some(uuid_str_func),
        None, None, None
    );
    if rc != SQLITE_OK { return rc; }

    let rc = sqlite3_create_function_v2(
        db,
        c"uuid_blob".as_ptr(),
        1,
        deterministic,
        std::ptr::null_mut(),
        Some(uuid_blob_func),
        None, None, None
    );
    
    rc
}

pub fn register(db: *mut c_void) -> c_int {
    unsafe {
        sqlite3_uuid4_init(db as *mut sqlite3, std::ptr::null_mut(), std::ptr::null())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{ffi::sqlite3_auto_extension, Connection};
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn test_uuid4_via_rusqlite() {
        unsafe {
            sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_uuid4_init as *const ())));
        }
        let conn = Connection::open_in_memory().unwrap();
        
        // Test uuid() generation
        let u1: String = conn.query_row("SELECT uuid()", [], |r| r.get(0)).unwrap();
        let u2: String = conn.query_row("SELECT uuid()", [], |r| r.get(0)).unwrap();
        assert_eq!(u1.len(), 36);
        assert_ne!(u1, u2);

        // Test uuid_blob length
        let len: i32 = conn.query_row(
            "SELECT length(uuid_blob('00000000-0000-0000-0000-000000000000'))", 
            [], 
            |r| r.get(0)
        ).unwrap();
        assert_eq!(len, 16);

        // Test uuid_str roundtrip
        let input = "12345678-1234-1234-1234-123456789abc";
        let roundtrip: String = conn.query_row(
            "SELECT uuid_str(uuid_blob(?1))", 
            [input], 
            |r| r.get(0)
        ).unwrap();
        assert_eq!(roundtrip, input);
    }
}
