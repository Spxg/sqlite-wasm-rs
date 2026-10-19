#![cfg(feature = "threadsafe")]

extern crate alloc;

use alloc::{format, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use core::time::Duration;
use libsqlite3_sys as sqlite;
use rsqlite_vfs::memvfs::{self, MemVfsError, MemVfsUtil};
use rsqlite_vfs::transfer::DbTransfer;
use rsqlite_vfs::{OsCallback, RegisterVfsError, VfsErrorCode, VfsFilesManager, VfsResult};
use std::sync::{mpsc, OnceLock};
use std::time::Instant;

static METHODS: OnceLock<sqlite::sqlite3_mutex_methods> = OnceLock::new();
static FAIL_FAST: AtomicBool = AtomicBool::new(false);
static REENTERED: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn allocate_mutex(kind: i32) -> *mut sqlite::sqlite3_mutex {
    if kind == sqlite::SQLITE_MUTEX_FAST && FAIL_FAST.swap(false, Ordering::Relaxed) {
        return core::ptr::null_mut();
    }
    // SAFETY: the wrapper preserves the configured allocator and its arguments.
    unsafe { METHODS.get().unwrap().xMutexAlloc.unwrap()(kind) }
}

struct TestOs {
    reenter: bool,
}

impl OsCallback for TestOs {
    fn sleep(&self, _: Duration) {}

    fn random(&self, bytes: &mut [u8]) -> usize {
        bytes.fill(42);
        bytes.len()
    }

    fn epoch_timestamp_in_ms(&self) -> VfsResult<i64> {
        Ok(0)
    }
}

impl Drop for TestOs {
    fn drop(&mut self) {
        if self.reenter {
            // SAFETY: SQLite is initialized, real mutexes protect memvfs, and no uninstall runs.
            let result = unsafe { memvfs::install_unchecked(Self { reenter: false }, false) };
            REENTERED.store(result.is_ok(), Ordering::Release);
        }
    }
}

#[test]
fn registration_survives_reentrant_failure_and_concurrent_installers() {
    // SAFETY: no other thread uses SQLite, and `sqlite3_initialize` has not run.
    unsafe {
        sqlite::sqlite3_mutex_alloc(sqlite::SQLITE_MUTEX_STATIC_APP1);
        let mut methods = core::mem::zeroed::<sqlite::sqlite3_mutex_methods>();
        assert_eq!(
            sqlite::sqlite3_config(sqlite::SQLITE_CONFIG_GETMUTEX, &mut methods),
            sqlite::SQLITE_OK
        );
        assert!(METHODS.set(methods).is_ok());
        methods.xMutexAlloc = Some(allocate_mutex);
        assert_eq!(
            sqlite::sqlite3_config(sqlite::SQLITE_CONFIG_MUTEX, &methods),
            sqlite::SQLITE_OK
        );
        assert_eq!(sqlite::sqlite3_initialize(), sqlite::SQLITE_OK);
    }

    let deadline = Instant::now() + Duration::from_secs(10);
    let (finished_tx, finished_rx) = mpsc::channel();
    FAIL_FAST.store(true, Ordering::Relaxed);
    std::thread::spawn(move || {
        // SAFETY: real SQLite mutexes serialize registration and no uninstall runs.
        let result =
            unsafe { memvfs::install_unchecked(TestOs { reenter: true }, false) }.map(|_| ());
        let _ = finished_tx.send(("allocation-failure", result));
    });
    let (id, result) = finished_rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .expect("allocation-failure worker timed out");
    assert_eq!(id, "allocation-failure");
    assert!(matches!(
        result,
        Err(RegisterVfsError::RegisterVfs(VfsErrorCode::NoMemory))
    ));
    assert!(REENTERED.load(Ordering::Acquire));
    // SAFETY: the worker finished all SQLite use and no file or raw reference is live.
    unsafe { memvfs::uninstall() }.unwrap();

    const WORKERS: usize = 16;
    let deadline = Instant::now() + Duration::from_secs(10);
    let start = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    for id in 0..WORKERS {
        let (start, ready_tx, done_tx) = (start.clone(), ready_tx.clone(), done_tx.clone());
        std::thread::spawn(move || {
            ready_tx.send(id).unwrap();
            while !start.load(Ordering::Acquire) {
                assert!(Instant::now() < deadline, "worker start timed out");
                std::thread::yield_now();
            }
            let result = (|| -> Result<(), alloc::string::String> {
                let installed = if id % 2 == 0 {
                    // SAFETY: real SQLite mutexes serialize installation and no uninstall runs.
                    unsafe { memvfs::install_unchecked(TestOs { reenter: false }, false) }
                        .map_err(|error| error.to_string())?
                } else {
                    loop {
                        if Instant::now() >= deadline {
                            return Err(format!("lookup {id} timed out"));
                        }
                        // SAFETY: installations retain their allocations and no uninstall runs.
                        match unsafe { MemVfsUtil::get_unchecked() } {
                            Ok(util) => break util,
                            Err(MemVfsError::NotInstalled) => std::thread::yield_now(),
                            Err(error) => return Err(error.to_string()),
                        }
                    }
                };
                let name = format!("installer-{id}.db");
                installed
                    .import_db_unchecked(&name, &[42; 512])
                    .map_err(|error| error.to_string())?;
                // SAFETY: registration remains alive until every worker has completed.
                let found =
                    unsafe { MemVfsUtil::get_unchecked() }.map_err(|error| error.to_string())?;
                let bytes = found.export_db(&name).map_err(|error| error.to_string())?;
                if bytes != [42; 512] {
                    return Err(format!("unexpected bytes exported from {name}"));
                }
                Ok(())
            })();
            let _ = done_tx.send((id, result));
        });
    }
    let mut ready = [false; WORKERS];
    for _ in 0..WORKERS {
        let id = ready_rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("installer readiness timed out");
        assert!(!core::mem::replace(&mut ready[id], true));
    }
    start.store(true, Ordering::Release);
    let mut done = [false; WORKERS];
    for _ in 0..WORKERS {
        let (id, result) = done_rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("installer completion timed out");
        assert!(!core::mem::replace(&mut done[id], true));
        assert_eq!(result, Ok(()), "installer {id}");
    }

    // SAFETY: every installer finished and the registration remains alive.
    let util = unsafe { MemVfsUtil::get_unchecked() }.unwrap();
    let mut names = util.names().unwrap();
    names.sort();
    let mut expected: Vec<_> = (0..WORKERS)
        .map(|id| format!("installer-{id}.db"))
        .collect();
    expected.sort();
    assert_eq!(names, expected);
    drop(util);
    // SAFETY: every worker finished and no file or raw reference is live.
    unsafe { memvfs::uninstall() }.unwrap();
}
