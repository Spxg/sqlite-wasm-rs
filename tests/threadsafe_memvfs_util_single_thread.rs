//! `memvfs_util` refuses shared memory once SQLite is configured without mutexes.

use sqlite_wasm_rs::host::{memvfs_util, MemVfsUtilError};
use sqlite_wasm_rs::{sqlite3_config, sqlite3_initialize, SQLITE_CONFIG_SINGLETHREAD, SQLITE_OK};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn refuses_shared_memory_after_single_thread_configuration() {
    // SAFETY: SQLite is not yet initialized in this test binary, and no other thread uses it.
    unsafe {
        assert_eq!(sqlite3_config(SQLITE_CONFIG_SINGLETHREAD), SQLITE_OK);
        assert_eq!(sqlite3_initialize(), SQLITE_OK);
    }
    assert!(matches!(memvfs_util(), Err(MemVfsUtilError::SharedMemory)));
}
