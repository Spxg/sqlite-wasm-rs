//! A VFS that reports `SQLITE_IOCAP_BATCH_ATOMIC` receives each commit as one
//! batch with no rollback journal, except with SQLite3MC, which lacks batch writes.

use sqlite_wasm_rs::vfs::{
    ffi::{
        sqlite3_file, SQLITE_FCNTL_BEGIN_ATOMIC_WRITE, SQLITE_FCNTL_COMMIT_ATOMIC_WRITE,
        SQLITE_IOCAP_BATCH_ATOMIC, SQLITE_NOTFOUND, SQLITE_OK,
    },
    register_vfs, AccessMode, FileKind, MemChunksFile, OpenOptions, OpenRequest, OpenedFile,
    SQLiteIoMethods, SQLiteVfs, VfsResult, VfsStore,
};
use sqlite_wasm_rs::*;
use std::{cell::RefCell, ffi::c_void, ptr};
use wasm_bindgen_test::wasm_bindgen_test;

thread_local! {
    static EVENTS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

fn record(event: &'static str) {
    EVENTS.with_borrow_mut(|events| events.push(event));
}

/// Memory files that SQLite opens once each, which is all a single connection needs.
struct BatchStore;

impl VfsStore for BatchStore {
    type File = MemChunksFile;
    type AppData = ();

    fn open_file(_: &(), request: OpenRequest<'_>) -> VfsResult<OpenedFile<MemChunksFile>> {
        if request.options.kind() == Some(FileKind::MainJournal) {
            record("open journal");
        }
        Ok(OpenedFile {
            file: MemChunksFile::default(),
            access: request.options.access(),
        })
    }

    fn close_file(_: &(), _: Option<&str>, _: MemChunksFile, _: OpenOptions) -> VfsResult<()> {
        Ok(())
    }

    fn access(_: &(), _name: &str, _mode: AccessMode) -> VfsResult<bool> {
        Ok(false)
    }

    fn full_pathname(_: &(), name: &str) -> VfsResult<String> {
        Ok(name.into())
    }

    fn delete_file(_: &(), _name: &str, _sync_dir: bool) -> VfsResult<()> {
        Ok(())
    }
}

struct BatchIo;

impl SQLiteIoMethods for BatchIo {
    type Store = BatchStore;

    unsafe extern "C" fn xDeviceCharacteristics(_file: *mut sqlite3_file) -> i32 {
        SQLITE_IOCAP_BATCH_ATOMIC
    }

    // Memory writes cannot fail part-way, so every batch is atomic.
    unsafe extern "C" fn xFileControl(_file: *mut sqlite3_file, op: i32, _arg: *mut c_void) -> i32 {
        match op {
            SQLITE_FCNTL_BEGIN_ATOMIC_WRITE => record("begin"),
            SQLITE_FCNTL_COMMIT_ATOMIC_WRITE => record("commit"),
            _ => return SQLITE_NOTFOUND,
        }
        SQLITE_OK
    }
}

struct BatchVfs;

impl SQLiteVfs<BatchIo> for BatchVfs {
    type Os = WasmOsCallback;

    fn os(_: &()) -> &WasmOsCallback {
        &WasmOsCallback
    }
}

#[wasm_bindgen_test]
fn test_commit_is_one_atomic_batch() {
    // SAFETY: the default callbacks agree with BatchStore and SQLite is used
    // from this thread only. The registration lives for the test process.
    unsafe { register_vfs::<BatchIo, BatchVfs>("batch-atomic-test", (), false) }
        .unwrap()
        .into_raw();

    let mut db = ptr::null_mut();
    // SAFETY: both names are NUL-terminated literals and `db` is a valid out pointer.
    let code = unsafe {
        sqlite3_open_v2(
            c"batch.db".as_ptr(),
            &mut db,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            c"batch-atomic-test".as_ptr(),
        )
    };
    assert_eq!(code, SQLITE_OK);

    // SAFETY: `db` stays open until the end, and every statement is a NUL-terminated literal.
    let exec = |sql: &std::ffi::CStr| unsafe {
        sqlite3_exec(db, sql.as_ptr(), None, ptr::null_mut(), ptr::null_mut())
    };
    assert_eq!(exec(c"CREATE TABLE t(x)"), SQLITE_OK);
    EVENTS.with_borrow_mut(Vec::clear);

    assert_eq!(
        exec(c"BEGIN; INSERT INTO t VALUES (1); INSERT INTO t VALUES (2); COMMIT;"),
        SQLITE_OK
    );
    let expected: &[&str] = if cfg!(feature = "sqlite3mc") {
        &["open journal"]
    } else {
        &["begin", "commit"]
    };
    assert_eq!(EVENTS.take(), expected);

    // SAFETY: `db` is open and has no unfinalized statements.
    assert_eq!(unsafe { sqlite3_close(db) }, SQLITE_OK);
}
