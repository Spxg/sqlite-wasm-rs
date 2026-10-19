#![cfg(feature = "threadsafe")]

use libsqlite3_sys as sqlite;
use rsqlite_vfs::memvfs::{MemVfsContext, MemVfsContextError};

#[test]
fn context_matches_the_runtime_mutex_policy() {
    // SAFETY: this isolated binary reads compile options before exposing any handle.
    let (debug, noop) = unsafe {
        let debug = sqlite::sqlite3_compileoption_used(c"DEBUG".as_ptr()) != 0;
        let noop = sqlite::sqlite3_compileoption_used(c"MUTEX_NOOP".as_ptr()) != 0;
        (debug, noop)
    };
    let result = MemVfsContext::try_new();
    if debug {
        assert!(
            matches!(result, Err(MemVfsContextError::UnverifiedMutexes)),
            "a C debug build cannot be verified"
        );
    } else if noop {
        assert!(
            matches!(result, Err(MemVfsContextError::SingleThreaded)),
            "release no-op mutexes share one static pointer"
        );
    } else {
        let context = result.expect("verified mutexes must yield a context");
        let _second = MemVfsContext::try_new().expect("a live context must clone");
        drop(context);
    }
}
