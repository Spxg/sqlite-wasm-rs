//! Dedicated workers sharing this module and its memory, driving SQLite concurrently.

#[path = "support/scenarios.rs"]
mod scenarios;
#[path = "support/workers.rs"]
mod workers;

use scenarios::{init, Connection};
use sqlite_wasm_rs::{
    sqlite3_mutex_alloc, sqlite3_mutex_enter, sqlite3_mutex_leave, sqlite3_mutex_try,
    sqlite3_threadsafe, SQLITE_BUSY, SQLITE_MUTEX_STATIC_APP1, SQLITE_OK,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_dedicated_worker);

fn spin_until(flag: &AtomicBool) {
    while !flag.load(Ordering::Acquire) {
        std::hint::spin_loop();
    }
}

#[wasm_bindgen_test]
fn sqlite_is_threadsafe() {
    // SAFETY: querying the compile-time threading mode has no precondition.
    assert_eq!(unsafe { sqlite3_threadsafe() }, 1);
}

/// Also fails if SQLite stops calling `sqlite3DefaultMutex()`, since its no-op mutex lets the try succeed.
#[wasm_bindgen_test]
async fn mutex_excludes_other_workers_and_recurses() {
    #[derive(Default)]
    struct Flags {
        held: AtomicBool,
        probed: AtomicBool,
        released: AtomicBool,
    }

    init();
    let flags = Arc::new(Flags::default());
    let holder = {
        let flags = flags.clone();
        move || {
            // SAFETY: static mutexes are allocated by SQLite for the whole process lifetime.
            let mutex = unsafe { sqlite3_mutex_alloc(SQLITE_MUTEX_STATIC_APP1) };
            // SAFETY: `mutex` is a valid static mutex, entered twice and left twice on this worker.
            unsafe {
                sqlite3_mutex_enter(mutex);
                sqlite3_mutex_enter(mutex);
            }
            flags.held.store(true, Ordering::Release);
            spin_until(&flags.probed);
            // SAFETY: this worker entered the mutex twice above.
            unsafe { sqlite3_mutex_leave(mutex) };
            // One level is still held, so the prober must keep waiting through this pause.
            std::thread::sleep(Duration::from_millis(50));
            flags.released.store(true, Ordering::Release);
            // SAFETY: this worker still holds the second level.
            unsafe { sqlite3_mutex_leave(mutex) };
        }
    };
    let prober = {
        let flags = flags.clone();
        move || {
            // SAFETY: static mutexes are allocated by SQLite for the whole process lifetime.
            let mutex = unsafe { sqlite3_mutex_alloc(SQLITE_MUTEX_STATIC_APP1) };
            spin_until(&flags.held);
            // SAFETY: `mutex` is valid, and a failed try leaves nothing to release.
            assert_eq!(unsafe { sqlite3_mutex_try(mutex) }, SQLITE_BUSY);
            flags.probed.store(true, Ordering::Release);
            // SAFETY: `mutex` is valid, entered once here and left once below on this worker.
            unsafe { sqlite3_mutex_enter(mutex) };
            assert!(
                flags.released.load(Ordering::Acquire),
                "entered while another worker held the mutex"
            );
            // SAFETY: entered above on this worker.
            unsafe { sqlite3_mutex_leave(mutex) };
            // SAFETY: the mutex is free again, so a try succeeds and is balanced by a leave.
            assert_eq!(unsafe { sqlite3_mutex_try(mutex) }, SQLITE_OK);
            // SAFETY: entered by the successful try above.
            unsafe { sqlite3_mutex_leave(mutex) };
        }
    };
    workers::spawn(vec![workers::task(holder), workers::task(prober)])
        .await
        .unwrap();
}

#[wasm_bindgen_test]
async fn workers_share_one_serialized_connection() {
    const WORKERS: usize = 4;
    const UPDATES: usize = 100;

    init();
    // `:memory:` keeps the database off memvfs's files, so only SQLite's own mutexes are exercised.
    let shared = Arc::new(Connection::open(":memory:"));
    scenarios::create_counter(&shared);
    let workers_shared = shared.clone();
    scenarios::run_together(WORKERS, move |_, start| {
        start.wait();
        for _ in 0..UPDATES {
            // One statement runs atomically under the connection mutex.
            workers_shared.exec("UPDATE counter SET n = n + 1");
        }
    })
    .await;
    scenarios::assert_counter(&shared, WORKERS * UPDATES);
}
