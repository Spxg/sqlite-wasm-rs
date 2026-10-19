//! Memory VFS installation and management under `forbid(unsafe_code)`.
#![cfg(any(feature = "threadsafe", not(target_feature = "atomics")))]
#![forbid(unsafe_code)]

use core::time::Duration;
use sqlite_wasm_rs::vfs::memvfs::{self, MemVfsContext, MemVfsUtil};
use sqlite_wasm_rs::vfs::transfer::DbTransfer;
use sqlite_wasm_rs::vfs::{OsCallback, VfsFilesManager, VfsResult};
use wasm_bindgen_test::wasm_bindgen_test;

struct UnusedOs;

impl OsCallback for UnusedOs {
    fn sleep(&self, _: Duration) {}

    fn random(&self, buf: &mut [u8]) -> usize {
        buf.fill(0);
        buf.len()
    }

    fn epoch_timestamp_in_ms(&self) -> VfsResult<i64> {
        Ok(0)
    }
}

#[wasm_bindgen_test]
fn safe_accessors_share_the_installed_memvfs() {
    let context = MemVfsContext::try_new().unwrap();
    let util = MemVfsUtil::get(&context).unwrap();
    util.import_db_unchecked("safe-accessors.db", &[7; 512])
        .unwrap();

    let installed = memvfs::install(&context, UnusedOs, false).unwrap();
    assert!(installed.contains("safe-accessors.db").unwrap());
    assert_eq!(installed.export_db("safe-accessors.db").unwrap(), [7; 512]);

    assert!(MemVfsUtil::get(&context)
        .unwrap()
        .remove("safe-accessors.db")
        .unwrap());
    assert!(!util.contains("safe-accessors.db").unwrap());
}
