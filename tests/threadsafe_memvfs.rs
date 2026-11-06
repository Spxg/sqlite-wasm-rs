//! Workers that construct their safe contexts on top of the main thread's validated lease.

#[path = "support/workers.rs"]
mod workers;

use core::time::Duration;
use sqlite_wasm_rs::vfs::memvfs::{self, MemVfsContext, MemVfsUtil};
use sqlite_wasm_rs::vfs::transfer::DbTransfer;
use sqlite_wasm_rs::vfs::{OsCallback, VfsFilesManager, VfsResult};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_dedicated_worker);

#[wasm_bindgen(inline_js = "export function monotonic_ms() { return performance.now(); }")]
extern "C" {
    fn monotonic_ms() -> f64;
}

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

#[forbid(unsafe_code)]
#[wasm_bindgen_test]
async fn workers_share_the_main_context_across_construction() {
    const WORKERS: usize = 8;

    let context = MemVfsContext::try_new().unwrap();

    let ready = Arc::new(AtomicUsize::new(0));
    let tasks = (0..WORKERS)
        .map(|worker| {
            let ready = ready.clone();
            workers::task(move || {
                let deadline = monotonic_ms() + 10_000.0;
                ready.fetch_add(1, Ordering::AcqRel);
                while ready.load(Ordering::Acquire) != WORKERS {
                    assert!(
                        monotonic_ms() < deadline,
                        "safe worker {worker} start timed out"
                    );
                    core::hint::spin_loop();
                }
                let context = MemVfsContext::try_new().unwrap();
                let util = if worker % 2 == 0 {
                    MemVfsUtil::get(&context).unwrap()
                } else {
                    memvfs::install(&context, UnusedOs, false).unwrap()
                };
                let name = format!("race-{worker}.db");
                debug_assert!(
                    worker <= u8::MAX as usize,
                    "worker index fits the fill byte"
                );
                let fill = worker as u8;
                util.import_db_unchecked(&name, &[fill; 512]).unwrap();
                assert_eq!(
                    MemVfsUtil::get(&context).unwrap().export_db(&name).unwrap(),
                    [fill; 512]
                );
            })
        })
        .collect();
    workers::spawn(tasks).await.unwrap();

    let util = MemVfsUtil::get(&context).unwrap();
    let mut names = util.names().unwrap();
    names.sort();
    let mut expected: Vec<_> = (0..WORKERS)
        .map(|worker| format!("race-{worker}.db"))
        .collect();
    expected.sort();
    assert_eq!(names, expected);
    util.clear().unwrap();
}
