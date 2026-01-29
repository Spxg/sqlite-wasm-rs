use std::ffi::{c_char, c_int, c_void, CString};
use sqlite_wasm_rs::{
    sqlite3, sqlite3_api_routines, sqlite3_context, sqlite3_value,
    sqlite3_create_function_v2, sqlite3_result_text, 
    SQLITE_UTF8, SQLITE_INNOCUOUS, 
    SQLITE_TRANSIENT,
};
use uuid::Uuid;

// --- SQL Functions ---

// uuid7() -> TEXT
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

// --- Extension Entry Point ---

#[no_mangle]
pub unsafe extern "C" fn sqlite3_uuid7_init(
    db: *mut sqlite3,
    _pz_err_msg: *mut *mut c_char,
    _p_api: *const sqlite3_api_routines,
) -> c_int {
    let flags = SQLITE_UTF8 | SQLITE_INNOCUOUS;

    let rc = sqlite3_create_function_v2(
        db,
        c"uuid7".as_ptr(),
        0,
        flags,
        std::ptr::null_mut(),
        Some(uuid7_func),
        None, None, None
    );
    
    rc
}

pub fn register(db: *mut c_void) -> c_int {
    unsafe {
        sqlite3_uuid7_init(db as *mut sqlite3, std::ptr::null_mut(), std::ptr::null())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{ffi::sqlite3_auto_extension, Connection};
    use wasm_bindgen_test::wasm_bindgen_test;

    /*
    #[wasm_bindgen_test]
    fn test_uuid_direct() {
        console_error_panic_hook::set_once();
        let u = Uuid::now_v7();
        assert_eq!(u.to_string().len(), 36);
        let u4 = Uuid::new_v4();
        assert_eq!(u4.to_string().len(), 36);
    }
    */

    #[wasm_bindgen_test]
    fn test_uuid7_via_rusqlite() {
        console_error_panic_hook::set_once();

        unsafe {
            sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_uuid7_init as *const ())));
        }
        
        let conn = Connection::open_in_memory().unwrap();
        
        // Test uuid7 generation and uniqueness
        let mut results = Vec::new();
        for _ in 0..100 {
            let u: String = conn.query_row("SELECT uuid7()", [], |r| r.get(0)).unwrap();
            results.push(u);
        }

        // Check format (simple length check)
        assert_eq!(results[0].len(), 36);

        // Check uniqueness
        let mut sorted = results.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(results.len(), sorted.len(), "UUIDv7 generated duplicates!");
    }
}
