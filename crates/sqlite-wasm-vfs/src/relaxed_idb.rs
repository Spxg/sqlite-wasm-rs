//! relaxed-idb vfs implementation
//!
//! ```rust
//! use sqlite_wasm_rs as ffi;
//! use sqlite_wasm_vfs::relaxed_idb::{
//!     install as install_idb_vfs,
//!     RelaxedIdbCfg
//! };
//!
//! async fn open_db() {
//!     // install relaxed-idb persistent vfs and set as default vfs
//!     install_idb_vfs::<ffi::WasmOsCallback>(&RelaxedIdbCfg::default(), true)
//!         .await
//!         .unwrap();
//!
//!     // open with relaxed-idb vfs
//!     let mut db = std::ptr::null_mut();
//!     let ret = unsafe {
//!         ffi::sqlite3_open_v2(
//!             c"relaxed-idb.db".as_ptr().cast(),
//!             &mut db as *mut _,
//!             ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
//!             std::ptr::null()
//!         )
//!     };
//!     assert_eq!(ffi::SQLITE_OK, ret);
//! }
//! ```
//!
//! Inspired by wa-sqlite's [`IDBMirrorVFS`](https://github.com/rhashimoto/wa-sqlite/blob/master/src/examples/IDBMirrorVFS.js),
//! this is an VFS used in a synchronization context.
//!
//! The principle is to preload the db into memory before xOpen, and then all operations are synchronous.
//! When sqlite calls sync, it asynchronously writes the changed blocks to the indexed db through the indexed transaction.
//! The difference from IDBMirrorVFS is that `RelaxedIdbVFS` does only support pragma `synchronous=off`.
//!
//! As for performance, since both reading and writing are done in memory, the performance is very good.
//! However, we need to pay attention to the performance of preload the database, because the database is divided
//! into multiple blocks and stored in the indexed db, and it takes some time to read all of them into memory.
//! After my test, when page_size is 64k, the loading speed is the fastest.
//!
//! As with MemoryVFS, you also need to pay attention to the memory size limit of the browser page.
//!
//! It is particularly important to note that using it on multiple pages may cause DB corruption.
//! It is recommended to use it in SharedWorker.

use rsqlite_vfs::{
    bail, check_db_and_page_size, check_import_db, check_option, check_result,
    ffi::{
        sqlite3_file, sqlite3_vfs, SQLITE_ERROR, SQLITE_FCNTL_COMMIT_PHASETWO, SQLITE_FCNTL_PRAGMA,
        SQLITE_FCNTL_SYNC, SQLITE_IOERR, SQLITE_IOERR_DELETE, SQLITE_IOERR_FSYNC, SQLITE_NOTFOUND,
        SQLITE_OK, SQLITE_OPEN_MAIN_DB,
    },
    register_vfs, registered_vfs, ImportDbError, MemChunksFile, OsCallback, RegisterVfsError,
    SQLiteIoMethods, SQLiteVfs, SQLiteVfsFile, VfsAppData, VfsError, VfsFile, VfsResult, VfsStore,
};
use std::time::Duration;
use std::{cell::RefCell, marker::PhantomData};

use indexed_db_futures::database::Database;
use indexed_db_futures::prelude::*;
use indexed_db_futures::transaction::TransactionMode;
use js_sys::{Number, Object, Reflect, Uint8Array};
use std::collections::{hash_map, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{
    collections::HashMap,
    ffi::{c_char, CStr},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use wasm_bindgen::JsValue;

type Result<T> = std::result::Result<T, RelaxedIdbError>;

fn page_read<T, G: Fn(usize) -> Option<T>, R: Fn(T, &mut [u8], (usize, usize))>(
    buf: &mut [u8],
    page_size: usize,
    file_size: usize,
    offset: usize,
    get_page: G,
    read_fn: R,
) -> bool {
    if page_size == 0 || file_size == 0 {
        buf.fill(0);
        return false;
    }

    let mut bytes_read = 0;
    let mut p_data_offset = 0;
    let p_data_length = buf.len();
    let i_offset = offset;

    while p_data_offset < p_data_length {
        let file_offset = i_offset + p_data_offset;
        let page_idx = file_offset / page_size;
        let page_offset = file_offset % page_size;
        let page_addr = page_idx * page_size;

        let Some(page) = get_page(page_addr) else {
            break;
        };

        let page_length = (page_size - page_offset).min(p_data_length - p_data_offset);
        read_fn(
            page,
            &mut buf[p_data_offset..p_data_offset + page_length],
            (page_offset, page_offset + page_length),
        );

        p_data_offset += page_length;
        bytes_read += page_length;
    }

    if bytes_read < p_data_length {
        buf[bytes_read..].fill(0);
        return false;
    }

    true
}

struct IdbCommit {
    op: IdbCommitOp,
    notify: Option<tokio::sync::oneshot::Sender<Result<()>>>,
}

enum IdbCommitOp {
    Sync(String),
    Delete(String),
    Clear,
}

enum IdbFile {
    Main(IdbPageFile),
    Temp(MemChunksFile),
}

impl IdbFile {
    fn new(flags: i32) -> Self {
        if flags & SQLITE_OPEN_MAIN_DB == 0 {
            Self::Temp(MemChunksFile::default())
        } else {
            Self::Main(IdbPageFile::default())
        }
    }
}

#[derive(Default)]
struct IdbPageFile {
    file_size: usize,
    block_size: usize,
    blocks: HashMap<usize, Uint8Array>,
    tx_blocks: HashSet<usize>,
    /// Whether blocks past the end of the file still have to be deleted from
    /// IndexedDB. Cleared only once a sync has actually stored the shorter file.
    tx_truncate: bool,
    sync_notified: bool,
    /// A commit failure which has not been reported to SQLite yet.
    unreported_err: Option<(i32, String)>,
}

impl VfsFile for IdbPageFile {
    fn read(&self, buf: &mut [u8], offset: usize) -> VfsResult<bool> {
        Ok(page_read(
            buf,
            self.block_size,
            self.file_size,
            offset,
            |addr| self.blocks.get(&addr),
            |page, buf, (start, end)| {
                page.subarray(start as u32, end as u32).copy_to(buf);
            },
        ))
    }

    fn write(&mut self, buf: &[u8], offset: usize) -> VfsResult<()> {
        let page_size = buf.len();

        for fill in (self.file_size..offset).step_by(page_size) {
            self.blocks
                .insert(fill, Uint8Array::new_with_length(page_size as u32));
            self.tx_blocks.insert(fill);
        }

        if let Some(buffer) = self.blocks.get_mut(&offset) {
            buffer.copy_from(buf);
        } else {
            self.blocks.insert(offset, Uint8Array::new_from_slice(buf));
        }

        self.tx_blocks.insert(offset);
        self.block_size = page_size;
        self.file_size = self.file_size.max(offset + page_size);
        Ok(())
    }

    fn truncate(&mut self, size: usize) -> VfsResult<()> {
        if size < self.file_size {
            // The blocks past the new end of the file are gone as far as the
            // database is concerned, and IndexedDB has to be told to delete them.
            // Dropping them here is not enough on its own: the deletion is carried
            // out by a sync, and a sync can fail, so the obligation is recorded
            // separately and outlives any single attempt at it.
            self.tx_truncate = true;

            if self.block_size > 0 {
                let mut offset = size;
                while self.blocks.remove(&offset).is_some() {
                    offset += self.block_size;
                }
            }
        }

        self.file_size = size;
        Ok(())
    }

    fn flush(&mut self) -> VfsResult<()> {
        Ok(())
    }

    fn size(&self) -> VfsResult<usize> {
        Ok(self.file_size)
    }
}

impl VfsFile for IdbFile {
    fn read(&self, buf: &mut [u8], offset: usize) -> VfsResult<bool> {
        match self {
            IdbFile::Main(idb_page_file) => idb_page_file.read(buf, offset),
            IdbFile::Temp(mem_chunks_file) => mem_chunks_file.read(buf, offset),
        }
    }

    fn write(&mut self, buf: &[u8], offset: usize) -> VfsResult<()> {
        match self {
            IdbFile::Main(idb_page_file) => idb_page_file.write(buf, offset),
            IdbFile::Temp(mem_chunks_file) => mem_chunks_file.write(buf, offset),
        }
    }

    fn truncate(&mut self, size: usize) -> VfsResult<()> {
        match self {
            IdbFile::Main(idb_page_file) => idb_page_file.truncate(size),
            IdbFile::Temp(mem_chunks_file) => mem_chunks_file.truncate(size),
        }
    }

    fn flush(&mut self) -> VfsResult<()> {
        match self {
            IdbFile::Main(idb_page_file) => idb_page_file.flush(),
            IdbFile::Temp(mem_chunks_file) => mem_chunks_file.flush(),
        }
    }

    fn size(&self) -> VfsResult<usize> {
        match self {
            IdbFile::Main(idb_page_file) => idb_page_file.size(),
            IdbFile::Temp(mem_chunks_file) => mem_chunks_file.size(),
        }
    }
}

fn key_range(file: &str, start: usize) -> std::ops::RangeInclusive<[JsValue; 2]> {
    [JsValue::from(file), JsValue::from(start)]
        ..=[
            JsValue::from(file),
            JsValue::from(Number::POSITIVE_INFINITY),
        ]
}

async fn clear_impl(indexed_db: &Database) -> Result<()> {
    let transaction = indexed_db
        .transaction("blocks")
        .with_mode(TransactionMode::Readwrite)
        .build()?;
    let blocks = transaction.object_store("blocks")?;
    blocks.clear()?;
    transaction.commit().await?;
    Ok(())
}

async fn preload_db_impl(
    indexed_db: &Database,
    preload: &Preload,
) -> Result<HashMap<String, IdbFile>> {
    if matches!(preload, &Preload::None) {
        return Ok(HashMap::new());
    }

    let transaction = indexed_db
        .transaction("blocks")
        .with_mode(TransactionMode::Readonly)
        .build()?;
    let blocks = transaction.object_store("blocks")?;

    let mut name2file = HashMap::new();
    let mut insert_fn = |block: JsValue| {
        let (path, offset, data) = get_block(block);
        match name2file.entry(path) {
            hash_map::Entry::Occupied(mut occupied_entry) => {
                let IdbFile::Main(db) = occupied_entry.get_mut() else {
                    unreachable!();
                };
                db.file_size += db.block_size;
                db.blocks.insert(offset, data);
            }
            hash_map::Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(IdbFile::Main(IdbPageFile {
                    file_size: data.length() as _,
                    block_size: data.length() as _,
                    blocks: HashMap::from([(offset, data)]),
                    tx_blocks: HashSet::new(),
                    tx_truncate: false,
                    sync_notified: false,
                    unreported_err: None,
                }));
            }
        }
    };

    match preload {
        Preload::All => {
            for block in blocks.get_all::<JsValue>().await? {
                insert_fn(block?);
            }
        }
        Preload::Paths(items) => {
            for file in items {
                for block in blocks
                    .get_all::<JsValue>()
                    .with_query(key_range(file, 0))
                    .await?
                {
                    insert_fn(block?);
                }
            }
        }
        Preload::None => unreachable!(),
    }

    Ok(name2file)
}

/// Test-only fault injection for the IndexedDB commits performed by
/// [`RelaxedIdb::sync_db_impl`].
#[cfg(test)]
#[derive(Default)]
struct SyncFaults {
    /// How many upcoming commits should be aborted instead of committed.
    fail_next: usize,
    /// How many commits have actually been aborted by fault injection.
    failures: usize,
}

/// Everything one sync is responsible for storing in IndexedDB.
///
/// It is moved out of the file while the sync is in flight, so that blocks dirtied
/// in the meantime accumulate separately instead of being mistaken for blocks the
/// sync has already stored, and is handed back if the commit fails.
struct SyncWork {
    /// Offsets of the blocks to write.
    blocks: HashSet<usize>,
    /// Whether blocks past the end of the file have to be deleted.
    truncate: bool,
    /// The length of the file when this work was claimed. Blocks at or past it are
    /// the ones to delete.
    file_size: usize,
}

struct RelaxedIdb {
    idb: Database,
    name2file: RefCell<HashMap<String, IdbFile>>,
    tx: UnboundedSender<IdbCommit>,
    #[cfg(test)]
    sync_faults: RefCell<SyncFaults>,
}

impl RelaxedIdb {
    async fn new(options: &RelaxedIdbCfg, tx: UnboundedSender<IdbCommit>) -> Result<Self> {
        let indexed_db = Database::open(&options.vfs_name)
            .with_version(1u8)
            .with_on_upgrade_needed(|_, db| {
                db.create_object_store("blocks")
                    .with_key_path(["path", "offset"].into())
                    .build()?;
                Ok(())
            })
            .await?;

        if options.clear_on_init {
            clear_impl(&indexed_db).await?;
        }

        let name2file = preload_db_impl(&indexed_db, &options.preload).await?;
        Ok(RelaxedIdb {
            idb: indexed_db,
            name2file: RefCell::new(name2file),
            tx,
            #[cfg(test)]
            sync_faults: RefCell::new(SyncFaults::default()),
        })
    }

    fn send_task(&self, op: IdbCommitOp) -> Result<()> {
        if self.tx.send(IdbCommit { op, notify: None }).is_err() {
            return Err(RelaxedIdbError::Generic(
                "failed to send commit task".into(),
            ));
        }
        Ok(())
    }

    fn send_task_with_notify(&self, op: IdbCommitOp) -> Result<WaitCommit> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let commit = IdbCommit {
            op,
            notify: Some(tx),
        };
        if self.tx.send(commit).is_err() {
            return Err(RelaxedIdbError::Generic(
                "failed to send commit task".into(),
            ));
        }
        Ok(WaitCommit(rx))
    }

    /// Arrange for the next `count` IndexedDB commits to be aborted instead of
    /// committed, as the browser itself would abort them when the storage quota is
    /// exhausted or the connection is going away.
    #[cfg(test)]
    fn fail_next_commits(&self, count: usize) {
        self.sync_faults.borrow_mut().fail_next += count;
    }

    /// How many IndexedDB commits have been aborted by fault injection so far.
    #[cfg(test)]
    fn injected_commit_failures(&self) -> usize {
        self.sync_faults.borrow().failures
    }

    /// Whether the commit about to be made should be aborted instead.
    #[cfg(test)]
    fn take_injected_failure(&self) -> bool {
        let mut faults = self.sync_faults.borrow_mut();
        let inject = faults.fail_next > 0;
        if inject {
            faults.fail_next -= 1;
            faults.failures += 1;
        }
        inject
    }

    async fn preload_db(&self, files: Vec<String>) -> Result<()> {
        let preload = {
            let name2file = self.name2file.borrow();
            files
                .into_iter()
                .filter(|x| !name2file.contains_key(x))
                .collect::<Vec<_>>()
        };
        let preload = preload_db_impl(&self.idb, &Preload::Paths(preload)).await?;
        self.name2file.borrow_mut().extend(preload);
        Ok(())
    }

    fn import_db(&self, filename: &str, bytes: &[u8]) -> Result<WaitCommit> {
        let page_size = check_import_db(bytes)?;
        self.import_db_unchecked(filename, bytes, page_size, true)
    }

    fn import_db_unchecked(
        &self,
        filename: &str,
        bytes: &[u8],
        page_size: usize,
        clear_wal: bool,
    ) -> Result<WaitCommit> {
        check_db_and_page_size(bytes.len(), page_size)?;

        if self.name2file.borrow().contains_key(filename) {
            return Err(RelaxedIdbError::Generic(format!(
                "{filename} file already exists"
            )));
        }

        let mut blocks: HashMap<usize, Uint8Array> = bytes
            .chunks(page_size)
            .enumerate()
            .map(|(idx, buffer)| (idx * page_size, Uint8Array::new_from_slice(buffer)))
            .collect();

        // forced to write back to legacy mode
        if clear_wal {
            let header = blocks.get_mut(&0).unwrap();
            header.subarray(18, 20).copy_from(&[1, 1]);
        }

        let tx_blocks = blocks.keys().copied().collect();

        self.name2file.borrow_mut().insert(
            filename.into(),
            IdbFile::Main(IdbPageFile {
                file_size: blocks.len() * page_size,
                block_size: page_size,
                blocks,
                tx_blocks,
                tx_truncate: false,
                sync_notified: false,
                unreported_err: None,
            }),
        );

        self.send_task_with_notify(IdbCommitOp::Sync(filename.into()))
    }

    fn export_db(&self, name: &str) -> Result<Vec<u8>> {
        let name2file = self.name2file.borrow();

        match name2file.get(name) {
            Some(IdbFile::Main(file)) => {
                let file_size = file.file_size;
                let mut ret = vec![0; file_size];
                for (&offset, buffer) in &file.blocks {
                    if offset >= file_size {
                        continue;
                    }
                    buffer.copy_to(&mut ret[offset..offset + file.block_size]);
                }
                Ok(ret)
            }
            Some(IdbFile::Temp(_)) => Err(RelaxedIdbError::Generic(
                "Does not support dumping temporary files".into(),
            )),
            None => Err(RelaxedIdbError::Generic(
                "The file to be exported does not exist".into(),
            )),
        }
    }

    fn delete_db(&self, name: &str) -> Result<WaitCommit> {
        self.name2file.borrow_mut().remove(name);
        self.send_task_with_notify(IdbCommitOp::Delete(name.into()))
    }

    fn clear_all(&self) -> Result<WaitCommit> {
        std::mem::take(&mut *self.name2file.borrow_mut());
        self.send_task_with_notify(IdbCommitOp::Clear)
    }

    fn exists(&self, file: &str) -> bool {
        self.name2file.borrow().contains_key(file)
    }

    async fn delete_db_impl(&self, file: &str) -> Result<()> {
        let transaction = self
            .idb
            .transaction("blocks")
            .with_mode(TransactionMode::Readwrite)
            .build()?;

        let store = transaction.object_store("blocks")?;

        store.delete(key_range(file, 0)).build()?;
        transaction.commit().await?;

        Ok(())
    }

    /// Claim the work the next sync of `file` is responsible for, leaving the file
    /// with a fresh, empty set of dirty blocks.
    ///
    /// Returns `None` when there is nothing to put or delete.
    fn take_sync_work(&self, file: &str) -> Option<SyncWork> {
        let mut name2file = self.name2file.borrow_mut();
        let IdbFile::Main(idb_blocks) = name2file.get_mut(file)? else {
            return None;
        };

        idb_blocks.sync_notified = false;

        if idb_blocks.tx_blocks.is_empty() && !idb_blocks.tx_truncate {
            // no need to put or delete
            return None;
        }

        Some(SyncWork {
            blocks: std::mem::take(&mut idb_blocks.tx_blocks),
            truncate: std::mem::take(&mut idb_blocks.tx_truncate),
            file_size: idb_blocks.file_size,
        })
    }

    /// Hand `work` back to `file` after a failed commit, merged with whatever was
    /// dirtied while that commit was in flight, so that the next sync carries both.
    fn restore_sync_work(&self, file: &str, work: SyncWork) {
        let mut name2file = self.name2file.borrow_mut();
        // The file may have been deleted while the commit was in flight, in which
        // case there is nothing left to store.
        let Some(IdbFile::Main(idb_blocks)) = name2file.get_mut(file) else {
            return;
        };

        idb_blocks.tx_blocks.extend(work.blocks);
        idb_blocks.tx_truncate |= work.truncate;
    }

    /// Write `work` to IndexedDB in a single transaction.
    async fn commit_sync_work(&self, file: &str, work: &SyncWork) -> Result<()> {
        let path = JsValue::from(file);

        let transaction = self
            .idb
            .transaction("blocks")
            .with_mode(TransactionMode::Readwrite)
            .build()?;

        let store = transaction.object_store("blocks")?;

        {
            // Borrowed only while the requests are issued, never across the commit
            // below: SQLite goes on writing to the file while that is in flight.
            let name2file = self.name2file.borrow();
            if let Some(IdbFile::Main(idb_blocks)) = name2file.get(file) {
                for &offset in &work.blocks {
                    if let Some(buffer) = idb_blocks.blocks.get(&offset) {
                        store.put(&set_block(&path, offset, buffer)).build()?;
                    }
                }
            }
        }

        // Unconditional, so that blocks left past the end of the file by an earlier
        // failure are cleaned up even when this sync only has blocks to write.
        store.delete(key_range(file, work.file_size)).build()?;

        #[cfg(test)]
        if self.take_injected_failure() {
            transaction.abort().await?;
            return Err(RelaxedIdbError::Generic(
                "injected IndexedDB commit failure".into(),
            ));
        }

        transaction.commit().await?;

        Ok(())
    }

    async fn sync_db_impl(&self, file: &str) -> Result<()> {
        let Some(work) = self.take_sync_work(file) else {
            return Ok(());
        };

        match self.commit_sync_work(file, &work).await {
            Ok(()) => {
                // Whatever went wrong before, the file is stored now, so an earlier
                // failure is no longer worth reporting.
                self.clear_unreported_err(file);
                Ok(())
            }
            Err(err) => {
                self.restore_sync_work(file, work);
                Err(err)
            }
        }
    }

    /// Record a commit failure for SQLite to report at the next write to `file`.
    ///
    /// A commit resolves long after the synchronous VFS call which queued it has
    /// returned, so there is no operation left to fail at the time the failure
    /// happens.
    ///
    /// A failure whose file is already gone goes unreported, which in practice means
    /// every failed deletion, a file being removed before its blocks are. The only
    /// file left to report such a failure against is an unrelated database whose own
    /// writes were stored perfectly well, and failing one of its operations would say
    /// something quite untrue about it. A deletion is rare, and a caller which
    /// asked for one is in a good position to notice that it did not happen and ask
    /// again; `RelaxedIdbUtil::delete_db` tells its caller outright.
    fn record_unreported_err(&self, file: &str, code: i32, message: String) {
        if let Some(IdbFile::Main(idb_blocks)) = self.name2file.borrow_mut().get_mut(file) {
            idb_blocks.unreported_err = Some((code, message));
        }
    }

    /// Take the failure to report against `file`, if there is one.
    ///
    /// Taking it as it is reported, rather than latching it, is what keeps the write
    /// path open: the operation SQLite retries after the error is the one which
    /// gives the failed sync's blocks another chance of being stored.
    fn take_unreported_err(&self, file: &str) -> Option<VfsError> {
        let mut name2file = self.name2file.borrow_mut();
        let Some(IdbFile::Main(idb_blocks)) = name2file.get_mut(file) else {
            return None;
        };

        idb_blocks
            .unreported_err
            .take()
            .map(|(code, message)| VfsError::new(code, message))
    }

    fn clear_unreported_err(&self, file: &str) {
        if let Some(IdbFile::Main(idb_blocks)) = self.name2file.borrow_mut().get_mut(file) {
            idb_blocks.unreported_err = None;
        }
    }

    async fn commit_loop(&self, mut rx: UnboundedReceiver<IdbCommit>) {
        while let Some(commit) = rx.recv().await {
            let IdbCommit { op, notify } = commit;
            // Alongside the result, the file a failure would be reported against
            // and the code to report it as. Clearing concerns every file at once, so
            // there is nothing specific enough to report it against.
            let (ret, report_against) = match op {
                IdbCommitOp::Sync(file) => (
                    self.sync_db_impl(&file).await,
                    Some((file, SQLITE_IOERR_FSYNC)),
                ),
                IdbCommitOp::Delete(file) => (
                    self.delete_db_impl(&file).await,
                    Some((file, SQLITE_IOERR_DELETE)),
                ),
                IdbCommitOp::Clear => (clear_impl(&self.idb).await, None),
            };

            // A failure nobody is waiting for would otherwise go unnoticed
            // altogether, so it is left for SQLite to report instead. An
            // unsuccessful send would be one where the corresponding receiver has
            // already been deallocated, which is no better than never having been
            // waited for.
            let unwaited = match notify {
                Some(notify) => notify.send(ret).err(),
                None => Some(ret),
            };

            if let (Some(Err(err)), Some((file, code))) = (unwaited, report_against) {
                self.record_unreported_err(&file, code, err.to_string());
            }
        }
    }
}

fn get_block(value: JsValue) -> (String, usize, Uint8Array) {
    let path = Reflect::get(&value, &JsValue::from("path"))
        .unwrap()
        .as_string()
        .unwrap();
    let offset = Reflect::get(&value, &JsValue::from("offset"))
        .unwrap()
        .as_f64()
        .unwrap() as usize;
    let data = Reflect::get(&value, &JsValue::from("data")).unwrap();

    (path, offset, Uint8Array::from(data))
}

fn set_block(path: &JsValue, offset: usize, data: &Uint8Array) -> JsValue {
    let block = Object::new();
    Reflect::set(&block, &JsValue::from("path"), path).unwrap();
    Reflect::set(&block, &JsValue::from("offset"), &JsValue::from(offset)).unwrap();
    Reflect::set(&block, &JsValue::from("data"), &JsValue::from(data)).unwrap();
    block.into()
}

struct RelaxedIdbStore;

impl VfsStore<IdbFile, RelaxedIdb> for RelaxedIdbStore {
    fn add_file(vfs: *mut sqlite3_vfs, file: &str, flags: i32) -> VfsResult<()> {
        let pool = unsafe { Self::app_data(vfs) };
        pool.name2file
            .borrow_mut()
            .insert(file.into(), IdbFile::new(flags));
        Ok(())
    }

    fn contains_file(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<bool> {
        let pool = unsafe { Self::app_data(vfs) };
        Ok(pool.name2file.borrow().contains_key(file))
    }

    fn delete_file(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<()> {
        let pool = unsafe { Self::app_data(vfs) };
        let idb_file = match pool.name2file.borrow_mut().remove(file) {
            Some(file) => file,
            None => {
                return Err(VfsError::new(
                    SQLITE_IOERR_DELETE,
                    format!("{file} not found"),
                ))
            }
        };
        // temp db never put into indexed db, no need to delete
        if let IdbFile::Main(_) = &idb_file {
            if pool.send_task(IdbCommitOp::Delete(file.into())).is_err() {
                return Err(VfsError::new(
                    SQLITE_IOERR_DELETE,
                    format!("failed to send delete task, file: {file}"),
                ));
            }
        }
        Ok(())
    }

    fn with_file<F: Fn(&IdbFile) -> VfsResult<i32>>(
        vfs_file: &SQLiteVfsFile,
        f: F,
    ) -> VfsResult<i32> {
        let name = unsafe { vfs_file.name() };
        let pool = unsafe { Self::app_data(vfs_file.vfs) };
        match pool.name2file.borrow().get(name) {
            Some(file) => f(file),
            None => Err(VfsError::new(SQLITE_IOERR, format!("{name} not found"))),
        }
    }

    fn with_file_mut<F: Fn(&mut IdbFile) -> VfsResult<i32>>(
        vfs_file: &SQLiteVfsFile,
        f: F,
    ) -> VfsResult<i32> {
        let name = unsafe { vfs_file.name() };
        let pool = unsafe { Self::app_data(vfs_file.vfs) };

        // `xWrite`, `xTruncate` and `xSync` all arrive here, which makes this the
        // first chance to tell SQLite that a sync has failed. Reads are deliberately
        // left alone: they are served from memory, and are correct regardless of
        // what IndexedDB holds.
        if let Some(err) = pool.take_unreported_err(name) {
            return Err(err);
        }

        match pool.name2file.borrow_mut().get_mut(name) {
            Some(file) => f(file),
            None => Err(VfsError::new(SQLITE_IOERR, format!("{name} not found"))),
        }
    }
}

struct RelaxedIdbIoMethods;

impl SQLiteIoMethods for RelaxedIdbIoMethods {
    type File = IdbFile;
    type AppData = RelaxedIdb;
    type Store = RelaxedIdbStore;

    const VERSION: ::std::os::raw::c_int = 1;

    unsafe extern "C" fn xFileControl(
        pFile: *mut sqlite3_file,
        op: ::std::os::raw::c_int,
        pArg: *mut ::std::os::raw::c_void,
    ) -> ::std::os::raw::c_int {
        let vfs_file = SQLiteVfsFile::from_file(pFile);
        let pool = Self::Store::app_data(vfs_file.vfs);
        let name = vfs_file.name();

        let mut name2file = pool.name2file.borrow_mut();
        let file = check_option!(name2file.get_mut(name));

        let IdbFile::Main(file) = file else {
            return SQLITE_NOTFOUND;
        };

        match op {
            SQLITE_FCNTL_PRAGMA => {
                let pArg = pArg as *mut *mut c_char;
                let name = *pArg.add(1);
                let value = *pArg.add(2);

                bail!(name.is_null());
                bail!(value.is_null(), SQLITE_NOTFOUND);

                let key = check_result!(CStr::from_ptr(name).to_str());
                let value = check_result!(CStr::from_ptr(value).to_str());

                if key.eq_ignore_ascii_case("page_size") {
                    let page_size = check_result!(value.parse::<usize>());
                    if page_size == file.block_size {
                        return SQLITE_OK;
                    } else if file.block_size == 0 {
                        file.block_size = page_size;
                    } else {
                        return pool.store_err(VfsError::new(
                            SQLITE_ERROR,
                            "page_size cannot be changed".into(),
                        ));
                    }
                } else if key.eq_ignore_ascii_case("synchronous")
                    && !value.eq_ignore_ascii_case("off")
                {
                    return pool.store_err(VfsError::new(
                        SQLITE_ERROR,
                        "relaxed-idb vfs only supports synchronous=off".into(),
                    ));
                };
            }
            SQLITE_FCNTL_SYNC | SQLITE_FCNTL_COMMIT_PHASETWO => {
                if !file.sync_notified {
                    if pool.send_task(IdbCommitOp::Sync(name.into())).is_err() {
                        return pool.store_err(VfsError::new(
                            SQLITE_ERROR,
                            format!("failed to send sync task, file: {name}"),
                        ));
                    }
                    file.sync_notified = true;
                }
            }
            _ => (),
        }

        SQLITE_NOTFOUND
    }
}

struct RelaxedIdbVfs<C>(PhantomData<C>);

impl<C> SQLiteVfs<RelaxedIdbIoMethods> for RelaxedIdbVfs<C>
where
    C: OsCallback,
{
    const VERSION: ::std::os::raw::c_int = 1;

    fn sleep(dur: Duration) {
        C::sleep(dur);
    }

    fn random(buf: &mut [u8]) {
        C::random(buf);
    }

    fn epoch_timestamp_in_ms() -> i64 {
        C::epoch_timestamp_in_ms()
    }
}

/// A future that resolves when a pending IndexedDB commit operation is complete.
pub struct WaitCommit(tokio::sync::oneshot::Receiver<Result<()>>);

impl Future for WaitCommit {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.0).poll(cx) {
            Poll::Ready(ret) => Poll::Ready(ret.unwrap_or_else(|_| {
                Err(RelaxedIdbError::Generic(
                    "Waiting for notify failure".into(),
                ))
            })),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum RelaxedIdbError {
    #[error(transparent)]
    Vfs(#[from] RegisterVfsError),
    #[error(transparent)]
    ImportDb(#[from] ImportDbError),
    #[error(transparent)]
    OpenDb(#[from] indexed_db_futures::error::OpenDbError),
    #[error(transparent)]
    IndexedDb(#[from] indexed_db_futures::error::Error),
    #[error("Generic error: {0}")]
    Generic(String),
}

/// Select which dbs to preload into memory.
pub enum Preload {
    /// Preload all databases
    All,
    /// Specify the path to load the database
    Paths(Vec<String>),
    /// Not preloaded, can be manually loaded later via `RelaxedIdbUtil`
    None,
}

/// Build `RelaxedIdbCfg`
pub struct RelaxedIdbCfgBuilder(RelaxedIdbCfg);

impl RelaxedIdbCfgBuilder {
    pub fn new() -> Self {
        Self(RelaxedIdbCfg::default())
    }

    /// The SQLite VFS name under which this pool's VFS is registered.
    pub fn vfs_name(mut self, name: &str) -> Self {
        self.0.vfs_name = name.into();
        self
    }

    /// Delete all files on initialization.
    pub fn clear_on_init(mut self, set: bool) -> Self {
        self.0.clear_on_init = set;
        self
    }

    /// Select which dbs to preload into memory.
    pub fn preload(mut self, preload: Preload) -> Self {
        self.0.preload = preload;
        self
    }

    /// Build `RelaxedIdbCfg`.
    pub fn build(self) -> RelaxedIdbCfg {
        self.0
    }
}

impl Default for RelaxedIdbCfgBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// `RelaxedIdb` options
pub struct RelaxedIdbCfg {
    /// The SQLite VFS name under which this pool's VFS is registered.
    pub vfs_name: String,
    /// Delete all files on initialization.
    pub clear_on_init: bool,
    /// Select which dbs to preload into memory.
    pub preload: Preload,
}

impl Default for RelaxedIdbCfg {
    fn default() -> Self {
        Self {
            vfs_name: "relaxed-idb".into(),
            clear_on_init: false,
            preload: Preload::All,
        }
    }
}

/// RelaxedIdbVfs management tool.
pub struct RelaxedIdbUtil {
    pool: &'static VfsAppData<RelaxedIdb>,
}

impl RelaxedIdbUtil {
    /// Preload the db.
    ///
    /// Because indexed db reading data is an asynchronous operation,
    /// the db must be preloaded into memory before opening the sqlite db.
    pub async fn preload_db(&self, preload: Vec<String>) -> Result<()> {
        self.pool.preload_db(preload).await
    }

    /// Import the database.
    ///
    /// If the database is imported with WAL mode enabled,
    /// it will be forced to write back to legacy mode, see
    /// <https://sqlite.org/forum/forumpost/67882c5b04>
    ///
    /// If the imported database is encrypted, use `import_db_unchecked` instead.
    pub fn import_db(&self, filename: &str, bytes: &[u8]) -> Result<WaitCommit> {
        self.pool.import_db(filename, bytes)
    }

    /// `import_db` without checking, can be used to import encrypted database.
    pub fn import_db_unchecked(
        &self,
        filename: &str,
        bytes: &[u8],
        page_size: usize,
    ) -> Result<WaitCommit> {
        self.pool
            .import_db_unchecked(filename, bytes, page_size, false)
    }

    /// Export the database.
    pub fn export_db(&self, filename: &str) -> Result<Vec<u8>> {
        self.pool.export_db(filename)
    }

    /// Delete the specified database, make sure that the database is closed.
    pub fn delete_db(&self, filename: &str) -> Result<WaitCommit> {
        self.pool.delete_db(filename)
    }

    /// Delete all database, make sure that all database is closed.
    pub fn clear_all(&self) -> Result<WaitCommit> {
        self.pool.clear_all()
    }

    /// Does the database exists.
    pub fn exists(&self, filename: &str) -> bool {
        self.pool.exists(filename)
    }

    /// List all files.
    pub fn list(&self) -> Vec<String> {
        self.pool.name2file.borrow().keys().cloned().collect()
    }

    /// Number of files.
    pub fn count(&self) -> usize {
        self.pool.name2file.borrow().len()
    }
}

/// Register `relaxed-idb` vfs and return a management tool which can be used
/// to perform basic administration of the file pool.
///
/// If the vfs corresponding to `options.vfs_name` has been registered,
/// only return a management tool without register.
pub async fn install<C: OsCallback>(
    options: &RelaxedIdbCfg,
    default_vfs: bool,
) -> Result<RelaxedIdbUtil> {
    static REGISTER_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = REGISTER_GUARD.lock().await;

    let pool = if let Some(vfs) = registered_vfs(&options.vfs_name)? {
        unsafe { RelaxedIdbStore::app_data(vfs) }
    } else {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let pool = RelaxedIdb::new(options, tx).await?;
        let vfs = register_vfs::<RelaxedIdbIoMethods, RelaxedIdbVfs<C>>(
            &options.vfs_name,
            pool,
            default_vfs,
        )?;

        let app_data = unsafe { RelaxedIdbStore::app_data(vfs) };
        wasm_bindgen_futures::spawn_local(app_data.commit_loop(rx));
        app_data
    };

    Ok(RelaxedIdbUtil { pool })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsqlite_vfs::test_suite::test_vfs_store;
    use sqlite_wasm_rs as ffi;
    use std::collections::BTreeSet;
    use std::ffi::CString;
    use std::task::Waker;
    use wasm_bindgen_test::wasm_bindgen_test;

    const OPEN_RW: i32 = ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE;

    /// A file name which is never created, used to synchronise with the commit loop.
    const BARRIER_FILE: &str = "__commit_barrier__";

    /// 128 rows of a kilobyte each: enough to allocate pages of its own, so that a
    /// transaction writing this is distinguishable from one writing a single row.
    const INSERT_MANY_ROWS: &CStr = c"WITH RECURSIVE counter(x) AS (
             VALUES(1) UNION ALL SELECT x + 1 FROM counter WHERE x < 128
         )
         INSERT INTO a(v) SELECT zeroblob(1000) FROM counter;";

    /// The number of rows [`INSERT_MANY_ROWS`] inserts.
    const MANY_ROWS: i64 = 128;

    fn errmsg(db: *mut ffi::sqlite3) -> String {
        unsafe {
            CStr::from_ptr(ffi::sqlite3_errmsg(db).cast())
                .to_string_lossy()
                .into_owned()
        }
    }

    fn exec(db: *mut ffi::sqlite3, sql: &CStr) -> i32 {
        unsafe {
            ffi::sqlite3_exec(
                db,
                sql.as_ptr().cast(),
                None,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        }
    }

    #[track_caller]
    fn exec_ok(db: *mut ffi::sqlite3, sql: &CStr) {
        let ret = exec(db, sql);
        assert_eq!(ffi::SQLITE_OK, ret, "{sql:?} failed: {}", errmsg(db));
    }

    /// Run a query which yields at least one row, and read its first column.
    #[track_caller]
    fn query<T>(
        db: *mut ffi::sqlite3,
        sql: &CStr,
        read: impl FnOnce(*mut ffi::sqlite3_stmt) -> T,
    ) -> T {
        let mut stmt = std::ptr::null_mut();
        let ret = unsafe {
            ffi::sqlite3_prepare_v3(
                db,
                sql.as_ptr().cast(),
                -1,
                0,
                &mut stmt as *mut _,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(
            ffi::SQLITE_OK,
            ret,
            "preparing {sql:?} failed: {}",
            errmsg(db)
        );

        let ret = unsafe { ffi::sqlite3_step(stmt) };
        assert_eq!(
            ffi::SQLITE_ROW,
            ret,
            "{sql:?} returned no row: {}",
            errmsg(db)
        );

        let value = read(stmt);
        unsafe { ffi::sqlite3_finalize(stmt) };
        value
    }

    #[track_caller]
    fn query_i64(db: *mut ffi::sqlite3, sql: &CStr) -> i64 {
        query(db, sql, |stmt| unsafe {
            ffi::sqlite3_column_int64(stmt, 0)
        })
    }

    #[track_caller]
    fn query_text(db: *mut ffi::sqlite3, sql: &CStr) -> String {
        query(db, sql, |stmt| unsafe {
            CStr::from_ptr(ffi::sqlite3_column_text(stmt, 0).cast())
                .to_string_lossy()
                .into_owned()
        })
    }

    #[track_caller]
    fn open_db(file: &CStr, vfs_name: &str, flags: i32) -> *mut ffi::sqlite3 {
        let vfs = CString::new(vfs_name).unwrap();
        let mut db = std::ptr::null_mut();
        let ret = unsafe {
            ffi::sqlite3_open_v2(
                file.as_ptr().cast(),
                &mut db as *mut _,
                flags,
                vfs.as_ptr().cast(),
            )
        };
        assert_eq!(ffi::SQLITE_OK, ret, "opening {file:?} failed");
        db
    }

    /// Register a `relaxed-idb` VFS named `name`, backed by a freshly emptied
    /// IndexedDB database of the same name.
    async fn install_vfs(name: &str) -> RelaxedIdbUtil {
        install::<ffi::WasmOsCallback>(
            &RelaxedIdbCfgBuilder::new()
                .vfs_name(name)
                .clear_on_init(true)
                .build(),
            false,
        )
        .await
        .unwrap()
    }

    /// Wait until every task queued before this call has been processed.
    ///
    /// The VFS only *queues* sync tasks; they are carried out by
    /// [`RelaxedIdb::commit_loop`] on a separate task, so a test has to yield to the
    /// event loop before it can observe the result of a sync. The barrier is a sync
    /// request for a file which does not exist, which the commit loop resolves as a
    /// no-op: waiting for it therefore never writes anything itself, while the
    /// channel's ordering guarantees every earlier task has already run.
    async fn drain_commits(pool: &RelaxedIdb) {
        pool.send_task_with_notify(IdbCommitOp::Sync(BARRIER_FILE.into()))
            .unwrap()
            .await
            .unwrap();
    }

    /// Sync `file` and report whether the IndexedDB commit succeeded.
    async fn sync(pool: &RelaxedIdb, file: &str) -> Result<()> {
        pool.send_task_with_notify(IdbCommitOp::Sync(file.into()))
            .unwrap()
            .await
    }

    /// The offsets of the blocks `pool` holds for `file`.
    fn block_offsets(pool: &RelaxedIdb, file: &str) -> BTreeSet<usize> {
        match pool.name2file.borrow().get(file) {
            Some(IdbFile::Main(main)) => main.blocks.keys().copied().collect(),
            _ => BTreeSet::new(),
        }
    }

    /// A second, independent view of an IndexedDB database, preloaded with whatever
    /// is durably stored there and registered as a SQLite VFS of its own.
    ///
    /// This goes through the same code a page load does, so it shows what a reader
    /// which has never seen the writer's in-memory state would find.
    struct Reloaded {
        vfs_name: String,
        pool: &'static VfsAppData<RelaxedIdb>,
    }

    impl Reloaded {
        async fn from_idb(idb_name: &str, vfs_name: &str) -> Self {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let pool = RelaxedIdb::new(&RelaxedIdbCfgBuilder::new().vfs_name(idb_name).build(), tx)
                .await
                .unwrap();

            let vfs = register_vfs::<RelaxedIdbIoMethods, RelaxedIdbVfs<ffi::WasmOsCallback>>(
                vfs_name, pool, false,
            )
            .unwrap();
            let pool = unsafe { RelaxedIdbStore::app_data(vfs) };
            wasm_bindgen_futures::spawn_local(pool.commit_loop(rx));

            Self {
                vfs_name: vfs_name.into(),
                pool,
            }
        }

        fn open(&self, file: &CStr) -> *mut ffi::sqlite3 {
            open_db(file, &self.vfs_name, ffi::SQLITE_OPEN_READONLY)
        }

        fn block_offsets(&self, file: &str) -> BTreeSet<usize> {
            block_offsets(self.pool, file)
        }
    }

    #[wasm_bindgen_test]
    async fn test_relaxed_idb_vfs_store() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        test_vfs_store::<RelaxedIdb, IdbFile, RelaxedIdbStore>(VfsAppData::new(
            RelaxedIdb::new(
                &RelaxedIdbCfgBuilder::new()
                    .vfs_name("test_relaxed_idb_suite")
                    .build(),
                tx,
            )
            .await
            .unwrap(),
        ))
        .unwrap();

        wasm_bindgen_futures::spawn_local(async move { while let Some(_) = rx.recv().await {} });
    }

    /// The blocks a failed sync could not write stay queued, so a later sync makes
    /// the transaction durable after all.
    ///
    /// The two transactions here touch disjoint pages: if the first one's blocks
    /// were dropped when its sync failed, the second one's successful sync would
    /// store an image which mixes pages from before the first transaction with pages
    /// from after the second, which corresponds to no state the database was ever
    /// in.
    #[wasm_bindgen_test]
    async fn failed_sync_does_not_lose_committed_rows() {
        const IDB: &str = "test-idb-failed-sync-rows";

        let util = install_vfs(IDB).await;
        let db = open_db(c"rows.db", IDB, OPEN_RW);

        exec_ok(db, c"CREATE TABLE a(v BLOB NOT NULL);");
        exec_ok(db, c"CREATE TABLE b(v BLOB NOT NULL);");
        drain_commits(util.pool).await;

        // A transaction committed while IndexedDB is failing.
        util.pool.fail_next_commits(1);
        exec_ok(db, INSERT_MANY_ROWS);
        drain_commits(util.pool).await;
        assert_eq!(
            1,
            util.pool.injected_commit_failures(),
            "the test did not exercise a failed sync"
        );

        // The failure is reported to the first write which follows it, so the next
        // transaction fails once.
        let ret = exec(db, c"INSERT INTO b(v) VALUES (x'01');");
        assert_eq!(
            ffi::SQLITE_IOERR,
            ret & 0xff,
            "expected the failed sync to be reported, got {ret}: {}",
            errmsg(db)
        );

        // Retried, it succeeds, and its sync stores the earlier transaction's blocks
        // along with its own.
        exec_ok(db, c"INSERT INTO b(v) VALUES (x'01');");
        drain_commits(util.pool).await;

        let reloaded = Reloaded::from_idb(IDB, "test-idb-failed-sync-rows-reloaded").await;
        let reloaded_db = reloaded.open(c"rows.db");
        assert_eq!("ok", query_text(reloaded_db, c"PRAGMA integrity_check;"));
        assert_eq!(
            MANY_ROWS,
            query_i64(reloaded_db, c"SELECT count(*) FROM a;")
        );
        assert_eq!(1, query_i64(reloaded_db, c"SELECT count(*) FROM b;"));
    }

    /// Deleting the blocks past the end of the file is part of a sync too, and it
    /// is just as much at risk as writing the dirty ones: a sync consumes the file's
    /// new length while preparing its IndexedDB transaction, so a sync which fails
    /// after that point leaves IndexedDB holding blocks the database no longer has,
    /// and leaves no record that they need deleting.
    ///
    /// Unlike the test above, nothing writes to the database after the failure here,
    /// so the retry is the only chance to store the shrunk database.
    #[wasm_bindgen_test]
    async fn failed_sync_does_not_lose_truncation() {
        const IDB: &str = "test-idb-failed-sync-truncation";
        const FILE: &str = "truncation.db";

        let util = install_vfs(IDB).await;
        let db = open_db(c"truncation.db", IDB, OPEN_RW);

        exec_ok(db, c"CREATE TABLE a(v BLOB NOT NULL);");
        exec_ok(db, INSERT_MANY_ROWS);
        drain_commits(util.pool).await;
        let grown = block_offsets(util.pool, FILE);

        // Shrink the database while IndexedDB is failing.
        util.pool.fail_next_commits(1);
        exec_ok(db, c"DELETE FROM a;");
        exec_ok(db, c"VACUUM;");
        drain_commits(util.pool).await;
        assert_eq!(
            1,
            util.pool.injected_commit_failures(),
            "the test did not exercise a failed sync"
        );

        let live = block_offsets(util.pool, FILE);
        assert!(
            live.len() < grown.len(),
            "the database should have shrunk: {} blocks before, {} after",
            grown.len(),
            live.len()
        );

        // The next sync has to finish what the failed one started.
        sync(util.pool, FILE).await.unwrap();

        let reloaded = Reloaded::from_idb(IDB, "test-idb-failed-sync-truncation-reloaded").await;
        assert_eq!(
            live,
            reloaded.block_offsets(FILE),
            "IndexedDB should hold exactly the blocks the database has"
        );

        let reloaded_db = reloaded.open(c"truncation.db");
        assert_eq!("ok", query_text(reloaded_db, c"PRAGMA integrity_check;"));
        assert_eq!(0, query_i64(reloaded_db, c"SELECT count(*) FROM a;"));
    }

    /// A sync has work to do even when no block is dirty, because a truncation
    /// leaves blocks to delete from IndexedDB.
    ///
    /// SQLite always dirties the header page when it shrinks a database, so the
    /// truncation is prepared here through the same [`VfsFile`] interface
    /// `xTruncate` uses, with nothing else pending. The database is left corrupt by
    /// that, which is why only the stored blocks are checked and not their contents.
    #[wasm_bindgen_test]
    async fn truncation_alone_is_synced() {
        const IDB: &str = "test-idb-truncation-alone";
        const FILE: &str = "truncation-alone.db";
        const DROPPED_PAGES: usize = 4;

        let util = install_vfs(IDB).await;
        let db = open_db(c"truncation-alone.db", IDB, OPEN_RW);

        exec_ok(db, c"CREATE TABLE a(v BLOB NOT NULL);");
        exec_ok(db, INSERT_MANY_ROWS);
        drain_commits(util.pool).await;
        let grown = block_offsets(util.pool, FILE);

        {
            let mut name2file = util.pool.name2file.borrow_mut();
            let Some(IdbFile::Main(main)) = name2file.get_mut(FILE) else {
                panic!("{FILE} is not a main database file");
            };
            assert!(
                main.tx_blocks.is_empty(),
                "the truncation should be the only work the next sync has"
            );

            let size = main.file_size - DROPPED_PAGES * main.block_size;
            main.truncate(size).unwrap();
        }

        sync(util.pool, FILE).await.unwrap();

        let live = block_offsets(util.pool, FILE);
        assert_eq!(grown.len(), live.len() + DROPPED_PAGES);

        let reloaded = Reloaded::from_idb(IDB, "test-idb-truncation-alone-reloaded").await;
        assert_eq!(
            live,
            reloaded.block_offsets(FILE),
            "IndexedDB should hold exactly the blocks the database has"
        );
    }

    /// SQLite keeps running while an IndexedDB commit is in flight, so blocks can be
    /// dirtied after a sync has collected the ones it is going to write. When that
    /// commit fails, the retry covers both sets.
    ///
    /// This test drives [`RelaxedIdb::sync_db_impl`] itself instead of letting the
    /// commit loop do it, so that it can run SQL at the one moment that matters:
    /// after the sync has handed its blocks to IndexedDB, but before the commit has
    /// resolved.
    #[wasm_bindgen_test]
    async fn failed_sync_keeps_blocks_dirtied_while_it_was_in_flight() {
        const IDB: &str = "test-idb-failed-sync-in-flight";
        const FILE: &str = "in-flight.db";

        // The commit loop is deliberately not spawned; `_rx` keeps the channel open
        // so that the sync requests the VFS makes on its own are simply ignored.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let pool = RelaxedIdb::new(
            &RelaxedIdbCfgBuilder::new()
                .vfs_name(IDB)
                .clear_on_init(true)
                .build(),
            tx,
        )
        .await
        .unwrap();
        let vfs = register_vfs::<RelaxedIdbIoMethods, RelaxedIdbVfs<ffi::WasmOsCallback>>(
            IDB, pool, false,
        )
        .unwrap();
        let pool = unsafe { RelaxedIdbStore::app_data(vfs) };

        let db = open_db(c"in-flight.db", IDB, OPEN_RW);
        exec_ok(db, c"CREATE TABLE a(v BLOB NOT NULL);");
        exec_ok(db, c"CREATE TABLE b(v BLOB NOT NULL);");
        pool.sync_db_impl(FILE).await.unwrap();

        // Start a sync which is going to fail, and hold it at the point where the
        // commit is in flight.
        pool.fail_next_commits(1);
        exec_ok(db, INSERT_MANY_ROWS);
        let mut sync = Box::pin(pool.sync_db_impl(FILE));
        assert!(
            sync.as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending(),
            "the IndexedDB commit should still be in flight"
        );

        // Dirty further blocks, then let the failing commit resolve.
        exec_ok(db, c"INSERT INTO b(v) VALUES (x'01');");
        assert!(sync.await.is_err(), "the injected commit failure was lost");
        assert_eq!(1, pool.injected_commit_failures());

        // One successful sync now has to store both transactions.
        pool.sync_db_impl(FILE).await.unwrap();

        let reloaded = Reloaded::from_idb(IDB, "test-idb-failed-sync-in-flight-reloaded").await;
        let reloaded_db = reloaded.open(c"in-flight.db");
        assert_eq!("ok", query_text(reloaded_db, c"PRAGMA integrity_check;"));
        assert_eq!(
            MANY_ROWS,
            query_i64(reloaded_db, c"SELECT count(*) FROM a;")
        );
        assert_eq!(1, query_i64(reloaded_db, c"SELECT count(*) FROM b;"));
    }

    /// A sync failure cannot be reported to SQLite as it happens: the IndexedDB
    /// transaction only resolves after the synchronous VFS call which queued it has
    /// long returned. It is reported at the next opportunity instead, so that an
    /// application which never looks at the VFS still finds out that its data is not
    /// being stored.
    #[wasm_bindgen_test]
    async fn failed_sync_is_reported_to_sqlite() {
        const IDB: &str = "test-idb-failed-sync-reported";

        let util = install_vfs(IDB).await;
        let db = open_db(c"reported.db", IDB, OPEN_RW);

        exec_ok(db, c"CREATE TABLE a(v BLOB NOT NULL);");
        drain_commits(util.pool).await;

        util.pool.fail_next_commits(1);
        exec_ok(db, c"INSERT INTO a(v) VALUES (x'01');");
        drain_commits(util.pool).await;
        assert_eq!(
            1,
            util.pool.injected_commit_failures(),
            "the test did not exercise a failed sync"
        );

        let ret = exec(db, c"INSERT INTO a(v) VALUES (x'02');");
        assert_eq!(
            ffi::SQLITE_IOERR,
            ret & 0xff,
            "expected an I/O error once a sync had failed, got {ret}: {}",
            errmsg(db)
        );
    }
}
