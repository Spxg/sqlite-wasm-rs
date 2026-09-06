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
        SQLITE_FCNTL_SYNC, SQLITE_IOERR, SQLITE_IOERR_DELETE, SQLITE_NOTFOUND, SQLITE_OK,
        SQLITE_OPEN_MAIN_DB,
    },
    register_vfs, registered_vfs, ImportDbError, MemChunksFile, OsCallback, RegisterVfsError,
    SQLiteIoMethods, SQLiteVfs, SQLiteVfsFile, VfsAppData, VfsError, VfsFile, VfsResult, VfsStore,
};
#[cfg(feature = "test-util")]
use std::cell::Cell;
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
    sync::atomic::{AtomicU64, Ordering},
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

#[derive(Clone)]
enum IdbCommitOp {
    Sync(String),
    Delete(String),
    Clear,
    Barrier(String),
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

struct IdbPageFile {
    incarnation: u64,
    file_size: usize,
    block_size: usize,
    blocks: HashMap<usize, Uint8Array>,
    block_generation: HashMap<usize, u64>,
    tx_blocks: HashSet<usize>,
    sync_notified: bool,
}

static NEXT_FILE_INCARNATION: AtomicU64 = AtomicU64::new(1);
static NEXT_BLOCK_GENERATION: AtomicU64 = AtomicU64::new(1);

impl Default for IdbPageFile {
    fn default() -> Self {
        Self {
            incarnation: NEXT_FILE_INCARNATION.fetch_add(1, Ordering::Relaxed),
            file_size: 0,
            block_size: 0,
            blocks: HashMap::new(),
            block_generation: HashMap::new(),
            tx_blocks: HashSet::new(),
            sync_notified: false,
        }
    }
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
            self.block_generation
                .insert(fill, NEXT_BLOCK_GENERATION.fetch_add(1, Ordering::Relaxed));
            self.tx_blocks.insert(fill);
        }

        if let Some(buffer) = self.blocks.get_mut(&offset) {
            buffer.copy_from(buf);
        } else {
            self.blocks.insert(offset, Uint8Array::new_from_slice(buf));
        }

        self.block_generation.insert(
            offset,
            NEXT_BLOCK_GENERATION.fetch_add(1, Ordering::Relaxed),
        );
        self.tx_blocks.insert(offset);
        self.block_size = page_size;
        self.file_size = self.file_size.max(offset + page_size);
        Ok(())
    }

    fn truncate(&mut self, size: usize) -> VfsResult<()> {
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
                db.block_generation.insert(
                    offset,
                    NEXT_BLOCK_GENERATION.fetch_add(1, Ordering::Relaxed),
                );
            }
            hash_map::Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(IdbFile::Main(IdbPageFile {
                    incarnation: NEXT_FILE_INCARNATION.fetch_add(1, Ordering::Relaxed),
                    file_size: data.length() as _,
                    block_size: data.length() as _,
                    blocks: HashMap::from([(offset, data)]),
                    block_generation: HashMap::from([(
                        offset,
                        NEXT_BLOCK_GENERATION.fetch_add(1, Ordering::Relaxed),
                    )]),
                    tx_blocks: HashSet::new(),
                    sync_notified: false,
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

struct RelaxedIdb {
    idb: Database,
    name2file: RefCell<HashMap<String, IdbFile>>,
    tx: UnboundedSender<IdbCommit>,
    commit_errors: RefCell<Vec<CommitFailure>>,
    fatal_error: RefCell<Option<String>>,
    #[cfg(feature = "test-util")]
    fail_next_commit: Cell<bool>,
}

struct CommitFailure {
    op: IdbCommitOp,
    incarnation: Option<u64>,
    message: String,
    count: u32,
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
            commit_errors: RefCell::new(Vec::new()),
            fatal_error: RefCell::new(None),
            #[cfg(feature = "test-util")]
            fail_next_commit: Cell::new(false),
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

    fn barrier(&self, filename: &str) -> Result<WaitCommit> {
        self.send_task_with_notify(IdbCommitOp::Barrier(filename.into()))
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
        let block_generation = blocks
            .keys()
            .map(|offset| {
                (
                    *offset,
                    NEXT_BLOCK_GENERATION.fetch_add(1, Ordering::Relaxed),
                )
            })
            .collect();

        self.name2file.borrow_mut().insert(
            filename.into(),
            IdbFile::Main(IdbPageFile {
                incarnation: NEXT_FILE_INCARNATION.fetch_add(1, Ordering::Relaxed),
                file_size: blocks.len() * page_size,
                block_size: page_size,
                blocks,
                block_generation,
                tx_blocks,
                sync_notified: false,
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

    async fn sync_db_impl(&self, file: &str) -> Result<()> {
        if let Some(IdbFile::Main(idb_blocks)) = self.name2file.borrow_mut().get_mut(file) {
            idb_blocks.sync_notified = false;
        }

        let (incarnation, file_size, tx_blocks, blocks_to_put, has_truncation) = {
            let name2file = self.name2file.borrow();
            let Some(IdbFile::Main(idb_blocks)) = name2file.get(file) else {
                return Ok(());
            };

            let incarnation = idb_blocks.incarnation;
            let file_size = idb_blocks.file_size;
            let tx_blocks: Vec<_> = idb_blocks.tx_blocks.iter().copied().collect();
            let blocks_to_put: Vec<_> = tx_blocks
                .iter()
                .filter_map(|offset| {
                    let buffer = idb_blocks.blocks.get(offset)?;
                    if *offset >= file_size {
                        return None;
                    }
                    let mut bytes = vec![0; buffer.length() as usize];
                    buffer.copy_to(&mut bytes);
                    Some((
                        *offset,
                        idb_blocks
                            .block_generation
                            .get(offset)
                            .copied()
                            .unwrap_or(0),
                        Uint8Array::new_from_slice(&bytes),
                    ))
                })
                .collect();
            let has_truncation = idb_blocks.blocks.keys().any(|offset| *offset >= file_size);
            (
                incarnation,
                file_size,
                tx_blocks,
                blocks_to_put,
                has_truncation,
            )
        };

        if blocks_to_put.is_empty() && !has_truncation {
            // no need to put or delete
            return Ok(());
        }

        let path = JsValue::from(file);

        let transaction = self
            .idb
            .transaction("blocks")
            .with_mode(TransactionMode::Readwrite)
            .build()?;

        let store = transaction.object_store("blocks")?;

        for (offset, _, buffer) in &blocks_to_put {
            store.put(&set_block(&path, *offset, buffer)).build()?;
        }
        store.delete(key_range(file, file_size)).build()?;
        transaction.commit().await?;

        let mut name2file = self.name2file.borrow_mut();
        if let Some(IdbFile::Main(idb_blocks)) = name2file.get_mut(file) {
            if idb_blocks.incarnation == incarnation {
                for (offset, generation, _) in &blocks_to_put {
                    if idb_blocks.block_generation.get(offset) == Some(generation) {
                        idb_blocks.tx_blocks.remove(offset);
                    }
                }
                if idb_blocks.file_size == file_size {
                    idb_blocks.blocks.retain(|offset, _| *offset < file_size);
                    idb_blocks
                        .block_generation
                        .retain(|offset, _| *offset < file_size);
                    for offset in tx_blocks {
                        if offset >= file_size {
                            idb_blocks.tx_blocks.remove(&offset);
                        }
                    }
                }
            }
            idb_blocks.sync_notified = false;
        }

        Ok(())
    }

    async fn commit_loop(&self, mut rx: UnboundedReceiver<IdbCommit>) {
        while let Some(commit) = rx.recv().await {
            let IdbCommit { op, notify } = commit;
            let is_barrier = matches!(&op, IdbCommitOp::Barrier(_));
            let failed_op = op.clone();
            let ret = if !is_barrier && self.should_fail_next_commit() {
                Err(RelaxedIdbError::Generic("injected commit failure".into()))
            } else {
                match op {
                    IdbCommitOp::Sync(file) => self.sync_db_impl(&file).await,
                    IdbCommitOp::Delete(file) => self.delete_db_impl(&file).await,
                    IdbCommitOp::Clear => clear_impl(&self.idb).await,
                    IdbCommitOp::Barrier(file) => self.barrier_impl(&file).await,
                }
            };
            if !is_barrier {
                if let Err(error) = &ret {
                    self.record_commit_error(failed_op, error);
                }
            }
            if let Some(notify) = notify {
                // An unsuccessful send would be one where the corresponding receiver
                // has already been deallocated.
                let _ = notify.send(ret);
            }
        }
    }

    #[cfg(feature = "test-util")]
    fn should_fail_next_commit(&self) -> bool {
        self.fail_next_commit.replace(false)
    }

    #[cfg(not(feature = "test-util"))]
    fn should_fail_next_commit(&self) -> bool {
        false
    }

    fn record_commit_error(&self, op: IdbCommitOp, error: &RelaxedIdbError) {
        if matches!(op, IdbCommitOp::Delete(_) | IdbCommitOp::Clear) {
            let mut fatal = self.fatal_error.borrow_mut();
            if fatal.is_none() {
                *fatal = Some(error.to_string());
            }
            return;
        }
        let incarnation = match &op {
            IdbCommitOp::Sync(file) => self.file_incarnation(file),
            _ => None,
        };
        let mut pending = self.commit_errors.borrow_mut();
        if let Some(failure) = pending.iter_mut().find(|failure| same_op(&failure.op, &op)) {
            failure.count = failure.count.saturating_add(1);
        } else if pending.len() < 32 {
            pending.push(CommitFailure {
                op,
                incarnation,
                message: error.to_string(),
                count: 1,
            });
        } else if let Some(failure) = pending.last_mut() {
            failure.count = failure.count.saturating_add(1);
            *self.fatal_error.borrow_mut() = Some("too many distinct IndexedDB failures".into());
        }
    }

    fn file_incarnation(&self, file: &str) -> Option<u64> {
        self.name2file
            .borrow()
            .get(file)
            .and_then(|entry| match entry {
                IdbFile::Main(file) => Some(file.incarnation),
                IdbFile::Temp(_) => None,
            })
    }

    fn retry_operation<'a>(
        &'a self,
        op: &'a IdbCommitOp,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
        Box::pin(async move {
            match op {
                IdbCommitOp::Sync(file) => self.sync_db_impl(file).await,
                IdbCommitOp::Delete(_) | IdbCommitOp::Clear | IdbCommitOp::Barrier(_) => Err(
                    RelaxedIdbError::Generic("destructive commit failure cannot be retried".into()),
                ),
            }
        })
    }

    async fn barrier_impl(&self, file: &str) -> Result<()> {
        if let Some(error) = self.fatal_error.borrow().clone() {
            return Err(RelaxedIdbError::Generic(error));
        }
        let pending = std::mem::take(&mut *self.commit_errors.borrow_mut());
        let mut unresolved = Vec::new();
        let mut reported = None;
        for failure in pending {
            let Some(failed_file) = op_file(&failure.op) else {
                *self.fatal_error.borrow_mut() = Some(failure.message.clone());
                return Err(RelaxedIdbError::Generic(failure.message));
            };
            if self.file_incarnation(failed_file) != failure.incarnation {
                *self.fatal_error.borrow_mut() = Some(failure.message.clone());
                return Err(RelaxedIdbError::Generic(failure.message));
            }
            match self.retry_operation(&failure.op).await {
                Ok(()) => reported = Some(failure.message),
                Err(error) => unresolved.push(CommitFailure {
                    op: failure.op,
                    incarnation: failure.incarnation,
                    message: format!("{}; retry failed: {error}", failure.message),
                    count: failure.count,
                }),
            }
        }
        let mut files = self.name2file.borrow().keys().cloned().collect::<Vec<_>>();
        if !files.iter().any(|name| name == file) {
            files.push(file.to_string());
        }
        for name in files {
            if let Err(error) = self.sync_db_impl(&name).await {
                let incarnation = self.file_incarnation(&name);
                if incarnation.is_none() {
                    *self.fatal_error.borrow_mut() = Some(error.to_string());
                    return Err(error);
                }
                unresolved.push(CommitFailure {
                    op: IdbCommitOp::Sync(name),
                    incarnation,
                    message: error.to_string(),
                    count: 1,
                });
            }
        }
        if !unresolved.is_empty() {
            let message = unresolved
                .first()
                .map(|failure| failure.message.clone())
                .unwrap_or_else(|| "IndexedDB commit failed".into());
            *self.commit_errors.borrow_mut() = unresolved;
            return Err(RelaxedIdbError::Generic(message));
        }
        if let Some(message) = reported {
            return Err(RelaxedIdbError::Generic(message));
        }
        Ok(())
    }
}

fn op_file(op: &IdbCommitOp) -> Option<&str> {
    match op {
        IdbCommitOp::Sync(file) => Some(file),
        _ => None,
    }
}

fn same_op(left: &IdbCommitOp, right: &IdbCommitOp) -> bool {
    match (left, right) {
        (IdbCommitOp::Sync(a), IdbCommitOp::Sync(b))
        | (IdbCommitOp::Delete(a), IdbCommitOp::Delete(b)) => a == b,
        (IdbCommitOp::Clear, IdbCommitOp::Clear) => true,
        _ => false,
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

    /// Wait until every commit queued before this call has completed.
    ///
    /// SQLite's synchronous commit callback cannot await IndexedDB, so ordinary
    /// VFS sync notifications are intentionally fire-and-forget. This barrier
    /// is ordered on the same queue and reports every earlier failure, including
    /// failures whose original notification had no receiver.
    pub fn barrier(&self, filename: &str) -> Result<WaitCommit> {
        self.pool.barrier(filename)
    }

    #[cfg(feature = "test-util")]
    pub fn fail_next_commit(&self) {
        self.pool.fail_next_commit.set(true);
    }

    #[cfg(feature = "test-util")]
    pub fn forget_memory_file(&self, filename: &str) {
        self.pool.name2file.borrow_mut().remove(filename);
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
    use super::{IdbFile, RelaxedIdb, RelaxedIdbCfgBuilder, RelaxedIdbStore};
    use rsqlite_vfs::{test_suite::test_vfs_store, VfsAppData};
    use wasm_bindgen_test::wasm_bindgen_test;

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
}
