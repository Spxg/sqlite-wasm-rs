#![cfg(feature = "threadsafe")]

use libsqlite3_sys as sqlite;
use rsqlite_vfs::memvfs::{MemVfsContext, MemVfsContextError};

#[test]
fn context_rejects_single_thread_configured_before_init() {
    // SAFETY: this isolated binary configures SQLite before initialization on
    // its only SQLite thread.
    let debug = unsafe {
        assert_eq!(
            sqlite::sqlite3_config(sqlite::SQLITE_CONFIG_SINGLETHREAD),
            sqlite::SQLITE_OK
        );
        sqlite::sqlite3_compileoption_used(c"DEBUG".as_ptr()) != 0
    };
    // A failed validation releases the claimed state and caches no success.
    for pass in 0..2 {
        let result = MemVfsContext::try_new();
        if debug {
            assert!(
                matches!(result, Err(MemVfsContextError::UnverifiedMutexes)),
                "debug pass {pass}"
            );
        } else {
            assert!(
                matches!(result, Err(MemVfsContextError::SingleThreaded)),
                "core locks are disabled after SINGLETHREAD on pass {pass}"
            );
        }
    }
}
