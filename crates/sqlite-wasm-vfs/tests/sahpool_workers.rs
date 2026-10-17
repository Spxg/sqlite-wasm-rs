//! A pool's sync access handles live in the installing worker, so other workers must be refused.
#![cfg(target_feature = "atomics")]

#[path = "../../../tests/support/workers.rs"]
mod workers;

use rsqlite_vfs::{transfer::DbTransfer, VfsFilesManager};
use sqlite_wasm_rs::*;
use sqlite_wasm_vfs::sahpool::{install, OpfsSAHError, OpfsSAHPoolCfgBuilder, OpfsSAHPoolUtil};
use std::ffi::{CStr, CString};
use std::future::Future;
use std::sync::{Arc, Mutex};
use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);

/// A task whose future runs on the worker's own event loop, for work that awaits browser APIs.
fn async_task<F>(work: impl FnOnce() -> F + Send + 'static) -> workers::Task
where
    F: Future<Output = ()> + 'static,
{
    Box::new(move || {
        let work = work();
        wasm_bindgen_futures::future_to_promise(async move {
            work.await;
            Ok(JsValue::UNDEFINED)
        })
        .into()
    })
}

const VFS: &CStr = c"sahpool-owner";
const OTHER_VFS: &CStr = c"sahpool-other";
const MANAGED_VFS: &CStr = c"sahpool-managed";
const BUSY_VFS: &CStr = c"sahpool-busy";

fn open_with(name: &str, vfs: &CStr, flags: i32) -> (i32, *mut sqlite3) {
    let name = CString::new(name).unwrap();
    let mut db = std::ptr::null_mut();
    // SAFETY: both strings are NUL-terminated and outlive the call, and `db` is a valid out pointer.
    let rc = unsafe { sqlite3_open_v2(name.as_ptr(), &mut db, flags, vfs.as_ptr()) };
    (rc, db)
}

/// Opens or creates `name` on `vfs`, which must succeed.
fn open(name: &str, vfs: &CStr) -> *mut sqlite3 {
    let (rc, db) = open_with(name, vfs, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE);
    assert_eq!(rc, SQLITE_OK);
    db
}

/// Returns the result code of opening `name`, closing whatever connection SQLite handed back.
fn open_code(name: &str, vfs: &CStr, flags: i32) -> i32 {
    let (rc, db) = open_with(name, vfs, flags);
    // SAFETY: SQLite returns a handle to close even when opening fails.
    unsafe { sqlite3_close(db) };
    rc
}

/// Runs `sql`, which must succeed.
fn exec(db: *mut sqlite3, sql: &CStr) {
    // SAFETY: `db` is an open connection and `sql` is NUL-terminated.
    let rc = unsafe {
        sqlite3_exec(
            db,
            sql.as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(rc, SQLITE_OK);
}

/// Spills a temporary table to disk, which gives `db` a file deleted on close.
fn spill(db: *mut sqlite3) {
    exec(
        db,
        c"PRAGMA temp_store = FILE; CREATE TEMP TABLE spill(x); PRAGMA temp.cache_size = 2;
          WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 64)
          INSERT INTO spill SELECT randomblob(4096) FROM n;",
    );
}

async fn install_pool(vfs: &CStr) -> OpfsSAHPoolUtil {
    let name = vfs.to_str().unwrap();
    let config = OpfsSAHPoolCfgBuilder::new()
        .vfs_name(name)
        .directory(&format!("sqlite-wasm-vfs-tests/{name}"))
        .clear_on_init(true)
        .build();
    install::<WasmOsCallback>(&config, false).await.unwrap()
}

fn first_integer(db: *mut sqlite3, sql: &CStr) -> (i32, i64) {
    let mut stmt = std::ptr::null_mut();
    // SAFETY: `db` is an open connection, `sql` is NUL-terminated and `stmt` is a valid out pointer.
    let rc = unsafe { sqlite3_prepare_v2(db, sql.as_ptr(), -1, &mut stmt, std::ptr::null_mut()) };
    if rc != SQLITE_OK {
        return (rc, 0);
    }
    // SAFETY: `stmt` was prepared above, read only while it holds a row, and finalized once.
    unsafe {
        let rc = sqlite3_step(stmt);
        let value = if rc == SQLITE_ROW {
            sqlite3_column_int64(stmt, 0)
        } else {
            0
        };
        sqlite3_finalize(stmt);
        (rc, value)
    }
}

/// The main database file under `db`, as SQLite hands it to the VFS.
fn main_file(db: *mut sqlite3) -> *mut sqlite3_file {
    let mut file = std::ptr::null_mut::<sqlite3_file>();
    // SAFETY: `db` is an open connection and `file` is the out pointer `SQLITE_FCNTL_FILE_POINTER` expects.
    let rc = unsafe {
        sqlite3_file_control(
            db,
            c"main".as_ptr(),
            SQLITE_FCNTL_FILE_POINTER,
            (&mut file as *mut *mut sqlite3_file).cast(),
        )
    };
    assert_eq!(rc, SQLITE_OK);
    file
}

/// The message `xGetLastError` reports to the calling worker.
fn last_error(vfs: &CStr) -> String {
    let mut message = [0u8; 256];
    // SAFETY: the VFS is registered, and `message` holds the byte count passed with it.
    unsafe {
        let vfs = sqlite3_vfs_find(vfs.as_ptr());
        (*vfs).xGetLastError.unwrap()(vfs, message.len() as i32, message.as_mut_ptr().cast());
    }
    CStr::from_bytes_until_nul(&message)
        .unwrap()
        .to_str()
        .unwrap()
        .into()
}

/// Calls every guarded VFS and file method on the owner's pool, naming each result code.
fn call_every_method(owner: *mut sqlite3) -> Vec<(&'static str, i32)> {
    let open_rc = open_code("owner.db", VFS, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE);
    let file = main_file(owner);
    let mut page = [0u8; 4];
    let (mut size, mut exists, mut out_flags) = (0, 0, 0);
    // SAFETY: `file` stays open on the owner's idle connection, and every buffer outlives its call.
    let calls = unsafe {
        let io = &*(*file).pMethods;
        let vfs = sqlite3_vfs_find(VFS.as_ptr());
        // `u64` words give the file object the alignment of its pointer fields.
        let mut opened = vec![0u64; ((*vfs).szOsFile as usize).div_ceil(8)];
        [
            ("sqlite3_open_v2", open_rc),
            (
                "xOpen",
                (*vfs).xOpen.unwrap()(
                    vfs,
                    c"owner.db".as_ptr(),
                    opened.as_mut_ptr().cast(),
                    SQLITE_OPEN_READWRITE | SQLITE_OPEN_MAIN_DB,
                    &mut out_flags,
                ),
            ),
            (
                "xAccess",
                (*vfs).xAccess.unwrap()(
                    vfs,
                    c"owner.db".as_ptr(),
                    SQLITE_ACCESS_EXISTS,
                    &mut exists,
                ),
            ),
            (
                "xDelete",
                (*vfs).xDelete.unwrap()(vfs, c"owner.db".as_ptr(), 0),
            ),
            (
                "xRead",
                io.xRead.unwrap()(file, page.as_mut_ptr().cast(), 4, 0),
            ),
            (
                "xWrite",
                io.xWrite.unwrap()(file, page.as_ptr().cast(), 4, 0),
            ),
            ("xTruncate", io.xTruncate.unwrap()(file, 4096)),
            ("xSync", io.xSync.unwrap()(file, SQLITE_SYNC_NORMAL)),
            ("xFileSize", io.xFileSize.unwrap()(file, &mut size)),
            ("xLock", io.xLock.unwrap()(file, SQLITE_LOCK_SHARED)),
        ]
    };
    let query = ("query", first_integer(owner, c"SELECT x FROM t").0);
    calls.into_iter().chain([query]).collect()
}

/// A raw SQLite pointer the owner lends to the other worker while it awaits that worker.
struct Lent<T>(*mut T);

// SAFETY: the owner awaits the other worker, so the pointee is never used by two threads at once.
unsafe impl<T> Send for Lent<T> {}

impl<T> Lent<T> {
    // A method call makes closures capture the whole `Send` wrapper instead of its raw field.
    fn get(&self) -> *mut T {
        self.0
    }
}

/// What the other worker saw when it used the owner's pool.
#[derive(Debug, PartialEq)]
struct Outcome {
    calls: Vec<(&'static str, i32)>,
    last_error: String,
    close: i32,
    own_pool: i32,
}

#[wasm_bindgen_test]
async fn other_workers_are_refused() {
    let pool = install_pool(VFS).await;
    let db = open("owner.db", VFS);
    exec(db, c"CREATE TABLE t(x); INSERT INTO t VALUES (42);");
    let closed = open("closed.db", VFS);
    spill(closed);
    assert_eq!(pool.len().unwrap(), 3);
    assert_eq!(
        open_code("missing.db", VFS, SQLITE_OPEN_READWRITE),
        SQLITE_CANTOPEN
    );
    let owner_error = last_error(VFS);
    assert_ne!(owner_error, "");

    let outcome = Arc::new(Mutex::new(None));
    let other = {
        let (owner, closed, outcome) = (Lent(db), Lent(closed), outcome.clone());
        async_task(move || async move {
            // A pool of its own fills this worker's handle table, so foreign handle indexes could alias live handles.
            let _own = install_pool(OTHER_VFS).await;
            let own = open("other.db", OTHER_VFS);
            exec(own, c"CREATE TABLE t(x); INSERT INTO t VALUES (7);");

            let calls = call_every_method(owner.get());
            let last_error = last_error(VFS);
            // SAFETY: no statement is pending on `closed`, which the owner no longer touches.
            let close = unsafe { sqlite3_close(closed.get()) };
            let own_pool = first_integer(own, c"SELECT x FROM t").0;
            // SAFETY: no statement is pending on `own`.
            assert_eq!(unsafe { sqlite3_close(own) }, SQLITE_OK);
            *outcome.lock().unwrap() = Some(Outcome {
                calls,
                last_error,
                close,
                own_pool,
            });
        })
    };
    workers::spawn(vec![other]).await.unwrap();

    let outcome = outcome.lock().unwrap().take().unwrap();
    // Without the guards these calls fail with SQLITE_IOERR or succeed on the wrong handles.
    let refused = outcome
        .calls
        .iter()
        .map(|&(name, _)| (name, SQLITE_MISUSE))
        .collect();
    assert_eq!(
        outcome,
        Outcome {
            calls: refused,
            last_error: String::new(),
            close: SQLITE_OK,
            own_pool: SQLITE_ROW,
        }
    );
    assert_eq!(last_error(VFS), owner_error);

    assert_eq!(first_integer(db, c"SELECT x FROM t"), (SQLITE_ROW, 42));
    // SAFETY: every statement was finalized and the other worker has finished.
    assert_eq!(unsafe { sqlite3_close(db) }, SQLITE_OK);
    // The pool has released the close from another worker, including its delete on close.
    let mut names = pool.names().unwrap();
    names.sort();
    assert_eq!(names, ["closed.db", "owner.db"]);
}

/// Closes `db` on another worker.
async fn close_on_other_worker(db: *mut sqlite3) {
    let db = Lent(db);
    workers::spawn(vec![workers::task(move || {
        // SAFETY: no statement is pending on `db`, which this worker's spawner no longer touches.
        assert_eq!(unsafe { sqlite3_close(db.get()) }, SQLITE_OK);
    })])
    .await
    .unwrap();
}

#[wasm_bindgen_test]
async fn pause_and_uninstall_release_closes_from_other_workers() {
    let pool = install_pool(MANAGED_VFS).await;

    close_on_other_worker(open("main.db", MANAGED_VFS)).await;
    pool.pause().unwrap();
    pool.resume().await.unwrap();

    close_on_other_worker(open("main.db", MANAGED_VFS)).await;
    // SAFETY: the only connection was closed, and no VFS pointer is retained.
    unsafe { pool.uninstall() }.unwrap();
}

#[wasm_bindgen_test]
async fn closes_from_other_workers_outlast_pool_operations() {
    let pool = install_pool(BUSY_VFS).await;
    let db = open("main.db", BUSY_VFS);
    spill(db);
    assert_eq!(pool.len().unwrap(), 2);

    let import = pool.begin_import_unchecked("import.db", 3).unwrap();
    close_on_other_worker(db).await;
    assert!(matches!(pool.len(), Err(OpfsSAHError::Busy)));
    drop(import);

    // The close from the other worker deletes its temporary file once the import ends.
    assert_eq!(pool.names().unwrap(), ["main.db"]);
}
