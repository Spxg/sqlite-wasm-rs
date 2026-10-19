#![cfg(feature = "threadsafe")]

use core::ffi::c_char;
use core::time::Duration;
use libsqlite3_sys as sqlite;
use rsqlite_vfs::memvfs::{self, MemVfsContext, MemVfsContextError};
use rsqlite_vfs::transfer::{DbTransfer, ExportSource};
use rsqlite_vfs::{OsCallback, VfsFilesManager, VfsResult};

struct NativeOs;

impl OsCallback for NativeOs {
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

unsafe extern "C" fn fail_open(
    _: *mut sqlite::sqlite3,
    _: *mut *mut c_char,
    _: *const sqlite::sqlite3_api_routines,
) -> i32 {
    sqlite::SQLITE_ERROR
}

#[test]
fn owned_export_retains_validation_until_its_data_retires() {
    let context = MemVfsContext::try_new().expect("context must validate");
    let util = memvfs::install(&context, NativeOs, false).expect("install must succeed");
    util.import_db_unchecked("lease.db", &[9; 4096])
        .expect("import must succeed");
    let mut source = util.open_export("lease.db").expect("export must open");
    drop(util);
    drop(context);
    // SAFETY: the probe is closed, and no SQLite files or registration access remain in flight.
    unsafe { memvfs::uninstall() }.expect("uninstall must succeed");
    // SAFETY: this isolated binary owns the extension registration, and its callback is static.
    unsafe {
        assert_eq!(
            sqlite::sqlite3_auto_extension(Some(fail_open)),
            sqlite::SQLITE_OK
        )
    };
    let cached = MemVfsContext::try_new().expect("the export must retain its validated context");
    drop(cached);
    let mut bytes = [0; 4096];
    assert_eq!(source.size(), 4096);
    assert_eq!(source.read_at(0, &mut bytes).unwrap(), 4096);
    assert_eq!(bytes, [9; 4096]);
    drop(source);
    assert!(matches!(
        MemVfsContext::try_new(),
        Err(MemVfsContextError::Sqlite(sqlite::SQLITE_ERROR))
    ));
    // SAFETY: no probe remains open, and this binary owns the registration.
    unsafe { sqlite::sqlite3_reset_auto_extension() };
    let context = MemVfsContext::try_new().expect("the environment must validate again");
    let util = memvfs::install(&context, NativeOs, false).unwrap();
    assert!(!util.contains("lease.db").unwrap());
    drop((util, context));
    // SAFETY: every safe object is retired and no registration access is in flight.
    unsafe { memvfs::uninstall() }.unwrap();
}
