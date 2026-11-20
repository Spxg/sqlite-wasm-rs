//! Safe memvfs management from dedicated workers sharing this module and its memory.

#[path = "support/workers.rs"]
mod workers;

use core::ptr;
use sqlite_wasm_rs::host::memvfs_util;
use sqlite_wasm_rs::vfs::VfsFilesManager;
use sqlite_wasm_rs::*;
use std::ffi::CString;
use std::sync::{Arc, Barrier};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_dedicated_worker);

fn create_with_connection(name: &str) {
    let name = CString::new(name).unwrap();
    let mut db = ptr::null_mut();
    let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE;
    // SAFETY: `name` and the SQL are NUL-terminated and outlive the calls, and `db` is closed before returning.
    unsafe {
        assert_eq!(
            sqlite3_open_v2(name.as_ptr(), &mut db, flags, c"memvfs".as_ptr()),
            SQLITE_OK
        );
        let sql = c"CREATE TABLE t(n)".as_ptr();
        assert_eq!(
            sqlite3_exec(db, sql, None, ptr::null_mut(), ptr::null_mut()),
            SQLITE_OK
        );
        assert_eq!(sqlite3_close(db), SQLITE_OK);
    }
}

#[wasm_bindgen_test]
async fn workers_manage_files_concurrently() {
    const WORKERS: usize = 4;
    const ROUNDS: usize = 20;

    // SAFETY: SQLite is initialized here before any worker starts.
    assert_eq!(unsafe { sqlite3_initialize() }, SQLITE_OK);
    let start = Arc::new(Barrier::new(WORKERS));
    let tasks = (0..WORKERS)
        .map(|worker| {
            let start = start.clone();
            workers::task(move || {
                let util = memvfs_util().unwrap();
                start.wait();
                for round in 0..ROUNDS {
                    let name = format!("worker-{worker}-{round}.db");
                    create_with_connection(&name);
                    assert!(util.contains(&name).unwrap());
                    assert!(util.names().unwrap().contains(&name));
                    assert!(util.remove(&name).unwrap());
                    assert!(!util.contains(&name).unwrap());
                }
            })
        })
        .collect();
    workers::spawn(tasks).await.unwrap();
    assert!(memvfs_util().unwrap().names().unwrap().is_empty());
}
