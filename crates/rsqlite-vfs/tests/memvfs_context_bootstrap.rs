#![cfg(feature = "threadsafe")]

use libsqlite3_sys as sqlite;
use rsqlite_vfs::memvfs::{MemVfsContext, MemVfsContextError};

#[test]
fn context_rejects_core_disabled_after_mutex_bootstrap() {
    // SAFETY: this isolated binary configures SQLite before initialization on its only SQLite thread.
    let debug = unsafe {
        sqlite::sqlite3_mutex_alloc(sqlite::SQLITE_MUTEX_STATIC_APP1);
        assert_eq!(
            sqlite::sqlite3_config(sqlite::SQLITE_CONFIG_SINGLETHREAD),
            sqlite::SQLITE_OK
        );
        sqlite::sqlite3_compileoption_used(c"DEBUG".as_ptr()) != 0
    };
    assert!(matches!(
        (debug, MemVfsContext::try_new()),
        (true, Err(MemVfsContextError::UnverifiedMutexes))
            | (false, Err(MemVfsContextError::SingleThreaded))
    ));
}
