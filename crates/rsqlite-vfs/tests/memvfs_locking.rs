//! Several connections sharing one memvfs database, on one thread or, with `threadsafe`, on many.

use libsqlite3_sys::{
    sqlite3, sqlite3_busy_timeout, sqlite3_close, sqlite3_column_int64, sqlite3_errmsg,
    sqlite3_exec, sqlite3_finalize, sqlite3_open_v2, sqlite3_prepare_v2, sqlite3_step, SQLITE_BUSY,
    SQLITE_OK, SQLITE_OPEN_CREATE, SQLITE_OPEN_READWRITE, SQLITE_ROW,
};
use rsqlite_vfs::{memvfs, OsCallback, VfsResult};
use std::ffi::{CStr, CString};
use std::sync::Once;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct NativeOs;

impl OsCallback for NativeOs {
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn random(&self, buf: &mut [u8]) -> usize {
        buf.fill(0x5a);
        buf.len()
    }

    fn epoch_timestamp_in_ms(&self) -> VfsResult<i64> {
        let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        Ok(i64::try_from(elapsed.as_millis()).unwrap())
    }
}

fn install() {
    static INSTALL: Once = Once::new();
    // SAFETY: registration is serialized by `Once`, and memvfs stays installed for the whole binary.
    INSTALL.call_once(|| unsafe {
        memvfs::install(NativeOs, false).unwrap();
    });
}

/// One connection to a memvfs database, used by a single thread.
struct Connection(*mut sqlite3);

impl Connection {
    /// Opens `name`, waiting up to `busy_timeout_ms` for locks held by other connections.
    fn open(name: &str, busy_timeout_ms: i32) -> Self {
        let name = CString::new(name).unwrap();
        let mut db = std::ptr::null_mut();
        let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE;
        // SAFETY: both strings are NUL-terminated and outlive the call, and `db` is a valid out pointer.
        let rc = unsafe { sqlite3_open_v2(name.as_ptr(), &mut db, flags, c"memvfs".as_ptr()) };
        let connection = Self(db);
        assert_eq!(rc, SQLITE_OK, "open: {}", connection.error());
        // SAFETY: `db` is an open connection owned by `connection`.
        assert_eq!(
            unsafe { sqlite3_busy_timeout(db, busy_timeout_ms) },
            SQLITE_OK
        );
        connection
    }

    fn error(&self) -> String {
        // SAFETY: SQLite returns a NUL-terminated message owned by the connection, copied at once.
        unsafe { CStr::from_ptr(sqlite3_errmsg(self.0)) }
            .to_string_lossy()
            .into_owned()
    }

    fn try_exec(&self, sql: &str) -> i32 {
        let sql = CString::new(sql).unwrap();
        // SAFETY: the connection is open and `sql` is NUL-terminated for the whole call.
        unsafe {
            sqlite3_exec(
                self.0,
                sql.as_ptr(),
                None,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        }
    }

    fn exec(&self, sql: &str) {
        assert_eq!(self.try_exec(sql), SQLITE_OK, "{sql:?}: {}", self.error());
    }

    /// Returns the first `N` integer columns of the first row.
    fn query_row<const N: usize>(&self, sql: &str) -> [i64; N] {
        let sql = CString::new(sql).unwrap();
        let mut stmt = std::ptr::null_mut();
        // SAFETY: the connection is open, `sql` is NUL-terminated and `stmt` is a valid out pointer.
        let rc = unsafe {
            sqlite3_prepare_v2(self.0, sql.as_ptr(), -1, &mut stmt, std::ptr::null_mut())
        };
        assert_eq!(rc, SQLITE_OK, "{sql:?}: {}", self.error());
        // SAFETY: `stmt` was prepared above and is finalized below.
        let rc = unsafe { sqlite3_step(stmt) };
        assert_eq!(rc, SQLITE_ROW, "{sql:?}: {}", self.error());
        let row = std::array::from_fn(|column| {
            // SAFETY: `stmt` holds a row and the caller selects at least `N` columns.
            unsafe { sqlite3_column_int64(stmt, i32::try_from(column).unwrap()) }
        });
        // SAFETY: `stmt` is finalized exactly once.
        unsafe { sqlite3_finalize(stmt) };
        row
    }

    fn assert_integrity(&self) {
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

#[test]
fn open_reader_blocks_a_writer_until_it_commits() {
    const DB: &str = "locks.db";

    install();
    let reader = Connection::open(DB, 0);
    reader.exec("CREATE TABLE t(v INTEGER); BEGIN; SELECT count(*) FROM t;");
    let writer = Connection::open(DB, 0);
    assert_eq!(writer.try_exec("INSERT INTO t VALUES (1)"), SQLITE_BUSY);
    reader.exec("COMMIT");
    writer.exec("INSERT INTO t VALUES (1)");
    assert_eq!(reader.query_row("SELECT count(*) FROM t"), [1]);
    reader.assert_integrity();
}

#[cfg(feature = "threadsafe")]
mod threads {
    use super::{install, Connection};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Barrier;

    const BUSY_TIMEOUT_MS: i32 = 60_000;

    /// Counts a writer as finished even when it panics, so readers never wait forever.
    struct Finished<'a>(&'a AtomicUsize);

    impl Drop for Finished<'_> {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Release);
        }
    }

    #[test]
    fn concurrent_increments_lose_no_update() {
        const DB: &str = "increments.db";
        const THREADS: usize = 8;
        const INCREMENTS: usize = 200;

        install();
        let setup = Connection::open(DB, BUSY_TIMEOUT_MS);
        setup.exec("CREATE TABLE counter(n INTEGER NOT NULL); INSERT INTO counter VALUES (0);");
        let start = Barrier::new(THREADS);
        std::thread::scope(|scope| {
            for _ in 0..THREADS {
                scope.spawn(|| {
                    let connection = Connection::open(DB, BUSY_TIMEOUT_MS);
                    start.wait();
                    for _ in 0..INCREMENTS {
                        connection.exec("BEGIN IMMEDIATE; UPDATE counter SET n = n + 1; COMMIT;");
                    }
                });
            }
        });
        let expected = i64::try_from(THREADS * INCREMENTS).unwrap();
        assert_eq!(setup.query_row("SELECT n FROM counter"), [expected]);
        setup.assert_integrity();
    }

    #[test]
    fn readers_see_only_committed_states() {
        const DB: &str = "snapshots.db";
        const WRITERS: usize = 4;
        const READERS: usize = 4;
        const PAIRS: usize = 250;

        install();
        let setup = Connection::open(DB, BUSY_TIMEOUT_MS);
        setup.exec("CREATE TABLE t(k INTEGER PRIMARY KEY, v INTEGER NOT NULL);");
        let start = Barrier::new(WRITERS + READERS);
        let writers_done = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for writer in 0..WRITERS {
                let (start, writers_done) = (&start, &writers_done);
                scope.spawn(move || {
                    let _finished = Finished(writers_done);
                    let connection = Connection::open(DB, BUSY_TIMEOUT_MS);
                    start.wait();
                    for pair in 0..PAIRS {
                        let key = 1 + writer * PAIRS + pair;
                        // Each pair sums to zero, so a torn snapshot shows a nonzero sum or an odd count.
                        connection.exec(&format!(
                            "BEGIN IMMEDIATE; INSERT INTO t VALUES ({key}, {key}); INSERT INTO t VALUES (-{key}, -{key}); COMMIT;"
                        ));
                    }
                });
            }
            for _ in 0..READERS {
                scope.spawn(|| {
                    let connection = Connection::open(DB, BUSY_TIMEOUT_MS);
                    start.wait();
                    let mut last_count = 0;
                    loop {
                        let finished = writers_done.load(Ordering::Acquire) == WRITERS;
                        let [count, sum] =
                            connection.query_row("SELECT count(*), coalesce(sum(v), 0) FROM t");
                        assert_eq!(count % 2, 0, "torn snapshot with {count} rows");
                        assert_eq!(sum, 0, "torn snapshot with sum {sum}");
                        assert!(
                            count >= last_count,
                            "count went back from {last_count} to {count}"
                        );
                        last_count = count;
                        if finished {
                            break;
                        }
                    }
                });
            }
        });
        let expected = i64::try_from(2 * WRITERS * PAIRS).unwrap();
        assert_eq!(
            setup.query_row("SELECT count(*), sum(v) FROM t"),
            [expected, 0]
        );
        setup.assert_integrity();
    }
}
