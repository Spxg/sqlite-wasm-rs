//! Serialized connections and workers that start together, shared by the worker suites.
//!
//! The including test crate also declares `mod workers`.

use sqlite_wasm_rs::{
    sqlite3, sqlite3_busy_timeout, sqlite3_close, sqlite3_column_int64, sqlite3_errmsg,
    sqlite3_exec, sqlite3_finalize, sqlite3_initialize, sqlite3_open_v2, sqlite3_prepare_v2,
    sqlite3_step, SQLITE_DONE, SQLITE_OK, SQLITE_OPEN_CREATE, SQLITE_OPEN_FULLMUTEX,
    SQLITE_OPEN_READWRITE, SQLITE_ROW,
};
use std::ffi::{CStr, CString};
use std::sync::{Arc, Barrier, Once};

use crate::workers;

/// Initializes SQLite on the calling thread, since concurrent first initialization is unsupported.
pub fn init() {
    static INIT: Once = Once::new();
    // SAFETY: initialization has no precondition.
    INIT.call_once(|| assert_eq!(unsafe { sqlite3_initialize() }, SQLITE_OK));
}

/// A connection opened through memvfs, serialized by SQLite so workers may share it.
pub struct Connection(*mut sqlite3);

// SAFETY: every connection is opened with SQLITE_OPEN_FULLMUTEX, so SQLite serializes all use of it.
unsafe impl Send for Connection {}
// SAFETY: as for `Send`, the connection mutex serializes calls made through a shared reference.
unsafe impl Sync for Connection {}

impl Connection {
    pub fn open(name: &str) -> Self {
        let name = CString::new(name).unwrap();
        let mut db = std::ptr::null_mut();
        let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX;
        // SAFETY: both strings are NUL-terminated and outlive the call, and `db` is a valid out pointer.
        let rc = unsafe { sqlite3_open_v2(name.as_ptr(), &mut db, flags, c"memvfs".as_ptr()) };
        let connection = Self(db);
        assert_eq!(rc, SQLITE_OK, "open: {}", connection.error());
        // SAFETY: `db` is an open connection owned by `connection`.
        assert_eq!(unsafe { sqlite3_busy_timeout(db, 60_000) }, SQLITE_OK);
        connection
    }

    fn error(&self) -> String {
        // SAFETY: SQLite returns a NUL-terminated message owned by the connection, copied at once.
        unsafe { CStr::from_ptr(sqlite3_errmsg(self.0)) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn exec(&self, sql: &str) {
        let sql = CString::new(sql).unwrap();
        // SAFETY: the connection is open and `sql` is NUL-terminated for the whole call.
        let rc = unsafe {
            sqlite3_exec(
                self.0,
                sql.as_ptr(),
                None,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(rc, SQLITE_OK, "{sql:?}: {}", self.error());
    }

    /// The first `N` integer columns of the first row, `None` without rows, or the failing result code.
    pub fn try_query_row<const N: usize>(&self, sql: &str) -> Result<Option<[i64; N]>, i32> {
        let sql = CString::new(sql).unwrap();
        let mut stmt = std::ptr::null_mut();
        // SAFETY: the connection is open, `sql` is NUL-terminated and `stmt` is a valid out pointer.
        let rc = unsafe {
            sqlite3_prepare_v2(self.0, sql.as_ptr(), -1, &mut stmt, std::ptr::null_mut())
        };
        if rc != SQLITE_OK {
            return Err(rc);
        }
        // SAFETY: `stmt` was prepared above and is finalized below.
        let row = match unsafe { sqlite3_step(stmt) } {
            SQLITE_ROW => Ok(Some(std::array::from_fn(|column| {
                // SAFETY: `stmt` holds a row and the caller selects at least `N` columns.
                unsafe { sqlite3_column_int64(stmt, i32::try_from(column).unwrap()) }
            }))),
            SQLITE_DONE => Ok(None),
            rc => Err(rc),
        };
        // SAFETY: `stmt` is finalized exactly once.
        unsafe { sqlite3_finalize(stmt) };
        row
    }

    pub fn query_row<const N: usize>(&self, sql: &str) -> [i64; N] {
        match self.try_query_row(sql) {
            Ok(Some(row)) => row,
            Ok(None) => panic!("{sql:?} returned no row"),
            Err(rc) => panic!("{sql:?} failed with {rc}: {}", self.error()),
        }
    }

    pub fn assert_integrity(&self) {
        let [ok] = self.query_row("SELECT integrity_check = 'ok' FROM pragma_integrity_check");
        assert_eq!(ok, 1, "integrity_check failed");
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // SAFETY: the connection is open and every statement was finalized.
        unsafe { sqlite3_close(self.0) };
    }
}

pub fn create_counter(connection: &Connection) {
    connection.exec("CREATE TABLE counter(n INTEGER NOT NULL); INSERT INTO counter VALUES (0);");
}

pub fn assert_counter(connection: &Connection, expected: usize) {
    let expected = i64::try_from(expected).unwrap();
    assert_eq!(connection.query_row("SELECT n FROM counter"), [expected]);
    connection.assert_integrity();
}

/// Runs `work(index, start)` on `parties` workers, each of which calls `start.wait()` once to begin together.
pub async fn run_together(parties: usize, work: impl Fn(usize, &Barrier) + Send + Sync + 'static) {
    let work = Arc::new(work);
    let start = Arc::new(Barrier::new(parties));
    let tasks = (0..parties)
        .map(|index| {
            let (work, start) = (work.clone(), start.clone());
            workers::task(move || work(index, &start))
        })
        .collect();
    workers::spawn(tasks).await.unwrap();
}
