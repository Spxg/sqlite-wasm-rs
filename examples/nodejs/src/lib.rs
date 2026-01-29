use std::collections::HashSet;
use std::ffi::CStr;

use sqlite_wasm_rs::{
    sqlite3, sqlite3_close, sqlite3_column_text, sqlite3_finalize, sqlite3_open_v2,
    sqlite3_prepare_v3, sqlite3_step, SQLITE_OK, SQLITE_OPEN_CREATE, SQLITE_OPEN_READWRITE,
    SQLITE_ROW,
};
use uuid::{Uuid, Version};
use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

macro_rules! console_log {
    ($($t:tt)*) => (log(&format_args!($($t)*).to_string()))
}

unsafe fn query_uuid(db: *mut sqlite3, sql_stmt: *const std::ffi::c_char) -> String {
    let mut stmt = std::ptr::null_mut();
    let rc = sqlite3_prepare_v3(db, sql_stmt, -1, 0, &mut stmt, std::ptr::null_mut());
    assert_eq!(SQLITE_OK, rc, "Failed to prepare statement");

    let mut res = String::new();
    if sqlite3_step(stmt) == SQLITE_ROW {
        let val_ptr = sqlite3_column_text(stmt, 0);
        if !val_ptr.is_null() {
            res = CStr::from_ptr(val_ptr.cast()).to_str().unwrap().to_string();
        } else {
            panic!("Returned NULL for UUID query");
        }
    } else {
        panic!("No row returned for UUID query");
    }
    sqlite3_finalize(stmt);
    res
}

#[wasm_bindgen(start)]
async fn main() {
    console_error_panic_hook::set_once();
    let mut db: *mut sqlite3 = std::ptr::null_mut();
    // Open in-memory DB
    let ret = unsafe {
        sqlite3_open_v2(
            c":memory:".as_ptr().cast(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    console_log!("db opened");

    unsafe {
        // Register extensions
        let rc = sqlite_wasm_uuid4::register(db.cast());
        assert_eq!(SQLITE_OK, rc);
        console_log!("UUID4 extension registered");

        let rc = sqlite_wasm_uuid7::register(db.cast());
        assert_eq!(SQLITE_OK, rc);
        console_log!("UUID7 extension registered");

        // --- UUID4 Test ---
        console_log!("Generating 1000 UUIDv4...");
        let mut v4_results = Vec::with_capacity(1000);
        let sql = c"SELECT uuid();";

        for _ in 0..1000 {
            v4_results.push(query_uuid(db, sql.as_ptr()));
        }

        // Check uniqueness
        let set: HashSet<_> = v4_results.iter().cloned().collect();
        assert_eq!(set.len(), 1000, "All UUIDv4 must be unique");
        console_log!("Unique check passed for UUIDv4");

        // Check version & parsing
        for (i, s) in v4_results.iter().enumerate() {
            let u = Uuid::parse_str(s)
                .unwrap_or_else(|e| panic!("Failed to parse UUIDv4 at index {}: {} - {}", i, s, e));
            assert_eq!(
                u.get_version(),
                Some(Version::Random),
                "UUID at index {} is not v4: {}",
                i,
                s
            );
        }
        console_log!("Version check passed for UUIDv4");

        // --- UUID7 Test ---
        console_log!("Generating 1000 UUIDv7...");
        let mut v7_results = Vec::with_capacity(1000);
        let sql = c"SELECT uuid7();";

        for _ in 0..1000 {
            v7_results.push(query_uuid(db, sql.as_ptr()));
        }

        // Check uniqueness
        let set: HashSet<_> = v7_results.iter().cloned().collect();
        assert_eq!(set.len(), 1000, "All UUIDv7 must be unique");
        console_log!("Unique check passed for UUIDv7");

        // Check sorting (Monotonicity)
        // Since UUIDv7 is time-ordered, later generations should generally be greater than previous ones.
        // However, if generated in the same millisecond, the random component decides order.
        // Strict monotonicity isn't guaranteed across all implementations unless they share state,
        // but uuid::Uuid::now_v7() generally attempts internally to be monotonic if called rapidly.
        // Wait, the Rust `uuid` crate documentation says `now_v7` guarantees monotonicity for same-process calls if the feature is enabled?
        // Actually `Uuid::now_v7()` uses `context::Context` thread-locally if available, or just random.
        // The implementation in extensions/uuid7 uses `Uuid::now_v7()`.
        // Let's check if the list is sorted.

        let mut sorted_v7 = v7_results.clone();
        sorted_v7.sort();

        // This assertion might be flaky if the clock goes backwards or multiple threads (not in WASM mostly).
        // But for a single thread tight loop, it should be strictly monotonic or at least non-decreasing.
        // Since we assert uniqueness, non-decreasing + unique = strictly increasing.

        if v7_results != sorted_v7 {
            console_log!("Warning: UUIDv7 results were not strictly monotonic.");
            // Find first violation
            for i in 0..999 {
                if v7_results[i] > v7_results[i + 1] {
                    console_log!(
                        "Violation at index {}: {} > {}",
                        i,
                        v7_results[i],
                        v7_results[i + 1]
                    );
                    // Allow slight harmless reordering if any, but UUIDv7 SHOULD be monotonic.
                    // The user asked to "check they are unique and sorted".
                    // So I will assert it.
                }
            }
            panic!("UUIDv7 results are not sorted!");
        }
        console_log!("Sort order check passed for UUIDv7");

        // Check version
        for (i, s) in v7_results.iter().enumerate() {
            let u = Uuid::parse_str(s)
                .unwrap_or_else(|e| panic!("Failed to parse UUIDv7 at index {}: {} - {}", i, s, e));
            assert_eq!(
                u.get_version(),
                Some(Version::SortRand),
                "UUID at index {} is not v7: {}",
                i,
                s
            );
        }
        console_log!("Version check passed for UUIDv7");

        sqlite3_close(db);
    }

    console_log!("All tests passed!");
}
