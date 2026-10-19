#![cfg(feature = "threadsafe")]

use core::sync::atomic::{AtomicBool, Ordering};
use libsqlite3_sys as sqlite;
use rsqlite_vfs::memvfs::{MemVfsContext, MemVfsContextError};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};

static METHODS: OnceLock<sqlite::sqlite3_mutex_methods> = OnceLock::new();
static ENTERED: OnceLock<mpsc::Sender<&'static str>> = OnceLock::new();
static RESUME: OnceLock<Mutex<mpsc::Receiver<&'static str>>> = OnceLock::new();
static DEADLINE: OnceLock<Instant> = OnceLock::new();
static FIRST: AtomicBool = AtomicBool::new(true);
static REENTRY_REFUSED: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn initialize_mutexes() -> i32 {
    if FIRST.swap(false, Ordering::Relaxed) {
        REENTRY_REFUSED.store(
            matches!(
                MemVfsContext::try_new(),
                Err(MemVfsContextError::InitializationInProgress)
            ),
            Ordering::Relaxed,
        );
        ENTERED.get().unwrap().send("context-init-v1").unwrap();
        let reply = RESUME
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .recv_timeout(
                DEADLINE
                    .get()
                    .unwrap()
                    .saturating_duration_since(Instant::now()),
            )
            .unwrap();
        assert_eq!(reply, "context-init-v1");
    }
    // SAFETY: this wrapper delegates initialization to the retained native provider.
    unsafe { METHODS.get().unwrap().xMutexInit.unwrap()() }
}

#[test]
fn context_refuses_reentry_and_competing_initialization_then_revalidates() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let deadline = Instant::now() + Duration::from_secs(10);
    assert!(ENTERED.set(entered_tx).is_ok());
    assert!(RESUME.set(Mutex::new(resume_rx)).is_ok());
    assert!(DEADLINE.set(deadline).is_ok());
    // SAFETY: this binary captures and wraps the provider before full SQLite initialization.
    unsafe {
        sqlite::sqlite3_mutex_alloc(sqlite::SQLITE_MUTEX_STATIC_APP1);
        let mut methods = core::mem::zeroed::<sqlite::sqlite3_mutex_methods>();
        assert_eq!(
            sqlite::sqlite3_config(sqlite::SQLITE_CONFIG_GETMUTEX, &mut methods),
            sqlite::SQLITE_OK
        );
        assert!(METHODS.set(methods).is_ok());
        methods.xMutexInit = Some(initialize_mutexes);
        assert_eq!(
            sqlite::sqlite3_config(sqlite::SQLITE_CONFIG_MUTEX, &methods),
            sqlite::SQLITE_OK
        );
    }
    std::thread::spawn(move || {
        let result = MemVfsContext::try_new();
        done_tx.send(("context-worker-v1", result)).unwrap();
    });
    let entered = entered_rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .unwrap();
    assert_eq!(entered, "context-init-v1");
    assert!(REENTRY_REFUSED.load(Ordering::Relaxed));
    assert!(matches!(
        MemVfsContext::try_new(),
        Err(MemVfsContextError::InitializationInProgress)
    ));
    resume_tx.send(entered).unwrap();
    let (id, result) = done_rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .unwrap();
    assert_eq!(id, "context-worker-v1");
    let context = result.expect("the initial constructor must complete");
    let cached = MemVfsContext::try_new().expect("a live context must clone");
    drop((context, cached));
    // SAFETY: construction and every context lease have retired before this global reset.
    unsafe {
        assert_eq!(sqlite::sqlite3_shutdown(), sqlite::SQLITE_OK);
        assert_eq!(
            sqlite::sqlite3_config(sqlite::SQLITE_CONFIG_SINGLETHREAD),
            sqlite::SQLITE_OK
        );
    }
    assert!(matches!(
        MemVfsContext::try_new(),
        Err(MemVfsContextError::SingleThreaded)
    ));
}
