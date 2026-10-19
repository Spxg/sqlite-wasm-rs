#![cfg(feature = "threadsafe")]

use core::time::Duration;
use libsqlite3_sys as sqlite;
use rsqlite_vfs::memvfs::{self, MemVfsContext, MemVfsUtil};
use rsqlite_vfs::transfer::DbTransfer;
use rsqlite_vfs::{OsCallback, VfsResult};

struct Os;

impl OsCallback for Os {
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
    fn random(&self, _: &mut [u8]) -> usize {
        0
    }
    fn epoch_timestamp_in_ms(&self) -> VfsResult<i64> {
        Ok(0)
    }
}

#[test]
fn context_accepts_core_locking_without_default_connection_mutexes() {
    // SAFETY: this isolated binary configures SQLite before initialization on its only SQLite thread.
    unsafe {
        assert_eq!(
            sqlite::sqlite3_config(sqlite::SQLITE_CONFIG_MULTITHREAD),
            sqlite::SQLITE_OK
        );
    }
    let context = MemVfsContext::try_new().unwrap();
    let util = memvfs::install(&context, Os, false).unwrap();
    util.import_db_unchecked("multithread.db", &[31; 512])
        .unwrap();
    let fresh = MemVfsUtil::get(&context).unwrap();
    assert_eq!(fresh.export_db("multithread.db").unwrap(), [31; 512]);
    drop((fresh, util, context));
    // SAFETY: all access and safe objects are retired, and no SQLite file references exist.
    unsafe { memvfs::uninstall() }.unwrap();
}
