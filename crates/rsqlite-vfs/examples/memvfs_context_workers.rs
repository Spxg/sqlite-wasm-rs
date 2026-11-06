//! Shares one verified `MemVfsContext` across identified worker threads.
//!
//! Run with `cargo run -p rsqlite-vfs --example memvfs_context_workers --features threadsafe`.

#![forbid(unsafe_code)]

use libsqlite3_sys as _;
use rsqlite_vfs::memvfs::{self, MemVfsContext, MemVfsUtil};
use rsqlite_vfs::transfer::DbTransfer;
use rsqlite_vfs::{OsCallback, VfsError, VfsErrorCode, VfsFilesManager, VfsResult};
use std::collections::BTreeSet;
use std::io::Read;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct NativeOs;

impl OsCallback for NativeOs {
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn random(&self, bytes: &mut [u8]) -> usize {
        std::fs::File::open("/dev/urandom")
            .and_then(|mut file| file.read(bytes))
            .unwrap_or(0)
    }

    fn epoch_timestamp_in_ms(&self) -> VfsResult<i64> {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| VfsError::new(VfsErrorCode::Error, error.to_string().into()))?;
        i64::try_from(elapsed.as_millis())
            .map_err(|error| VfsError::new(VfsErrorCode::Error, error.to_string().into()))
    }
}

fn worker(context: MemVfsContext, id: usize) -> Result<(), String> {
    let name = format!("context-cli-v1-{id}.db");
    let util = MemVfsUtil::get(&context).map_err(|error| error.to_string())?;
    let byte = u8::try_from(id + 17).expect("worker ids stay below 240");
    let bytes = [byte; 4096];
    util.import_db_unchecked(&name, &bytes)
        .map_err(|error| error.to_string())?;
    let fresh = MemVfsUtil::get(&context).map_err(|error| error.to_string())?;
    let exported = fresh.export_db(&name).map_err(|error| error.to_string())?;
    if exported != bytes {
        return Err(format!("incorrect bytes for worker {id}"));
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let context = MemVfsContext::try_new()?;
    let util = memvfs::install(&context, NativeOs, false)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let (sender, receiver) = mpsc::channel();
    for id in 0..4 {
        let (context, sender) = (context.clone(), sender.clone());
        std::thread::spawn(move || {
            let result = worker(context, id);
            sender.send((id, result)).unwrap();
        });
    }
    drop(sender);
    let mut completed = BTreeSet::new();
    for _ in 0..4 {
        let (id, result) =
            receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))?;
        result.map_err(std::io::Error::other)?;
        assert!(id < 4 && completed.insert(id));
        println!("context-cli-v1 worker={id} bytes=4096 verified");
    }
    let names: BTreeSet<_> = util.names()?.into_iter().collect();
    let expected: BTreeSet<_> = (0..4).map(|id| format!("context-cli-v1-{id}.db")).collect();
    assert_eq!(names, expected);
    println!(
        "context-cli-v1 safe construction, concurrent lookup and exact file contents verified"
    );
    Ok(())
}
