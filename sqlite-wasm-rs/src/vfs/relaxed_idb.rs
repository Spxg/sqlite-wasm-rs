//! relaxed-idb vfs implementation

use crate::vfs::utils::{
    copy_to_slice, copy_to_uint8_array, copy_to_uint8_array_subarray, import_db_check, page_read,
    register_vfs, FragileComfirmed, MemLinearFile, RegisterVfsError, SQLiteIoMethods, SQLiteVfs,
    SQLiteVfsFile, VfsAppData, VfsError, VfsFile, VfsResult, VfsStore,
};
use crate::{bail, check_option, check_result, libsqlite3::*};

use indexed_db_futures::database::Database;
use indexed_db_futures::prelude::*;
use indexed_db_futures::transaction::TransactionMode;
use js_sys::{Number, Object, Reflect, Uint8Array};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
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
    Temp(MemLinearFile),
}

impl IdbFile {
    fn new(flags: i32) -> Self {
        if flags & SQLITE_OPEN_MAIN_DB == 0 {
            Self::Temp(MemLinearFile::default())
        } else {
            Self::Main(IdbPageFile::default())
        }
    }
}

#[derive(Default)]
struct IdbPageFile {
    file_size: usize,
    block_size: usize,
    blocks: HashMap<usize, FragileComfirmed<Uint8Array>>,
    tx_blocks: HashSet<usize>,
    sync_notified: bool,
}

impl VfsFile for IdbPageFile {
    fn read(&self, buf: &mut [u8], offset: usize) -> VfsResult<i32> {
        Ok(page_read(
            buf,
            self.block_size,
            self.file_size,
            offset,
            |addr| self.blocks.get(&addr),
            |page, buf, (start, end)| {
                copy_to_slice(&page.subarray(start as u32, end as u32), buf);
            },
        ))
    }

    fn write(&mut self, buf: &[u8], offset: usize) -> VfsResult<()> {
        let page_size = buf.len();

        for fill in (self.file_size..offset).step_by(page_size) {
            self.blocks.insert(
                fill,
                FragileComfirmed::new(Uint8Array::new_with_length(page_size as u32)),
            );
            self.tx_blocks.insert(fill);
        }

        if let Some(buffer) = self.blocks.get_mut(&offset) {
            copy_to_uint8_array_subarray(buf, buffer);
        } else {
            self.blocks
                .insert(offset, FragileComfirmed::new(copy_to_uint8_array(buf)));
        }

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
    fn read(&self, buf: &mut [u8], offset: usize) -> VfsResult<i32> {
        match self {
            IdbFile::Main(idb_page_file) => idb_page_file.read(buf, offset),
            IdbFile::Temp(mem_linear_file) => mem_linear_file.read(buf, offset),
        }
    }

    fn write(&mut self, buf: &[u8], offset: usize) -> VfsResult<()> {
        match self {
            IdbFile::Main(idb_page_file) => idb_page_file.write(buf, offset),
            IdbFile::Temp(mem_linear_file) => mem_linear_file.write(buf, offset),
        }
    }

    fn truncate(&mut self, size: usize) -> VfsResult<()> {
        match self {
            IdbFile::Main(idb_page_file) => idb_page_file.truncate(size),
            IdbFile::Temp(mem_linear_file) => mem_linear_file.truncate(size),
        }
    }

    fn flush(&mut self) -> VfsResult<()> {
        match self {
            IdbFile::Main(idb_page_file) => idb_page_file.flush(),
            IdbFile::Temp(mem_linear_file) => mem_linear_file.flush(),
        }
    }

    fn size(&self) -> VfsResult<usize> {
        match self {
            IdbFile::Main(idb_page_file) => idb_page_file.size(),
            IdbFile::Temp(mem_linear_file) => mem_linear_file.size(),
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
                db.blocks.insert(offset, FragileComfirmed::new(data));
            }
            hash_map::Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(IdbFile::Main(IdbPageFile {
                    file_size: data.length() as _,
                    block_size: data.length() as _,
                    blocks: HashMap::from([(offset, FragileComfirmed::new(data))]),
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
    idb: FragileComfirmed<Database>,
    name2file: RwLock<HashMap<String, IdbFile>>,
    tx: UnboundedSender<IdbCommit>,
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
            idb: FragileComfirmed::new(indexed_db),
            name2file: RwLock::new(name2file),
            tx,
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

    async fn preload_db(&self, files: Vec<String>) -> Result<()> {
        let preload = {
            let name2file = self.name2file.read();
            files
                .into_iter()
                .filter(|x| !name2file.contains_key(x))
                .collect::<Vec<_>>()
        };
        let preload = preload_db_impl(&self.idb, &Preload::Paths(preload)).await?;
        self.name2file.write().extend(preload);
        Ok(())
    }

    fn import_db(&self, path: &str, bytes: &[u8]) -> Result<WaitCommit> {
        import_db_check(bytes).map_err(RelaxedIdbError::Generic)?;

        // The database page size in bytes.
        // Must be a power of two between 512 and 32768 inclusive, or the value 1 representing a page size of 65536.
        let page_size = u16::from_be_bytes([bytes[16], bytes[17]]);
        let page_size = if page_size == 1 {
            65536
        } else {
            usize::from(page_size)
        };

        self.import_db_unchecked(path, bytes, page_size, true)
    }

    fn import_db_unchecked(
        &self,
        path: &str,
        bytes: &[u8],
        page_size: usize,
        clear_wal: bool,
    ) -> Result<WaitCommit> {
        if !(page_size.is_power_of_two() && (512..=65536).contains(&page_size))
            || bytes.len() % page_size != 0
        {
            return Err(RelaxedIdbError::Generic(
                "Wrong page_size or wrong file length. \
                The file length needs to be an integer multiple of page_size."
                    .into(),
            ));
        }

        if self.name2file.read().contains_key(path) {
            return Err(RelaxedIdbError::Generic(format!(
                "{path} file already exists"
            )));
        }

        let mut blocks: HashMap<usize, FragileComfirmed<Uint8Array>> = bytes
            .chunks(page_size)
            .enumerate()
            .map(|(idx, buffer)| {
                (
                    idx * page_size,
                    FragileComfirmed::new(copy_to_uint8_array(&buffer)),
                )
            })
            .collect();

        // forced to write back to legacy mode
        if clear_wal {
            let header = blocks.get_mut(&0).unwrap();
            copy_to_uint8_array_subarray(&[1, 1], &header.subarray(18, 20));
        }

        let tx_blocks = blocks.keys().copied().collect();

        self.name2file.write().insert(
            path.into(),
            IdbFile::Main(IdbPageFile {
                file_size: blocks.len() * page_size,
                block_size: page_size,
                blocks,
                tx_blocks,
                sync_notified: false,
            }),
        );

        self.send_task_with_notify(IdbCommitOp::Sync(path.into()))
    }

    fn export_db(&self, name: &str) -> Result<Vec<u8>> {
        let name2file = self.name2file.read();

        if let Some(file) = name2file.get(name) {
            if let IdbFile::Main(file) = file {
                let file_size = file.file_size;
                let mut ret = vec![0; file.file_size];
                for (&offset, buffer) in &file.blocks {
                    if offset >= file_size {
                        continue;
                    }
                    copy_to_slice(buffer, &mut ret[offset..offset + file.block_size]);
                }
                Ok(ret)
            } else {
                Err(RelaxedIdbError::Generic(
                    "Does not support dumping temporary files".into(),
                ))
            }
        } else {
            Err(RelaxedIdbError::Generic(
                "The file to be exported does not exist".into(),
            ))
        }
    }

    fn delete_db(&self, name: &str) -> Result<WaitCommit> {
        self.name2file.write().remove(name);
        self.send_task_with_notify(IdbCommitOp::Delete(name.into()))
    }

    fn clear_all(&self) -> Result<WaitCommit> {
        std::mem::take(&mut *self.name2file.write());
        self.send_task_with_notify(IdbCommitOp::Clear)
    }

    fn exists(&self, file: &str) -> bool {
        self.name2file.read().contains_key(file)
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

    // already drop
    #[allow(clippy::await_holding_lock)]
    async fn sync_db_impl(&self, file: &str) -> Result<()> {
        let mut name2file = self.name2file.write();
        let Some(idb_file) = name2file.get_mut(file) else {
            return Ok(());
        };

        let IdbFile::Main(idb_blocks) = idb_file else {
            return Ok(());
        };

        idb_blocks.sync_notified = false;

        let file_size = idb_blocks.file_size;
        let mut truncated_offset = idb_blocks.file_size;
        while idb_blocks.blocks.remove(&truncated_offset).is_some() {
            truncated_offset += idb_blocks.block_size;
        }

        let tx_blocks = std::mem::take(&mut idb_blocks.tx_blocks);
        if tx_blocks.is_empty() && file_size == truncated_offset {
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

        for offset in tx_blocks {
            if let Some(buffer) = idb_blocks.blocks.get(&offset) {
                store.put(&set_block(&path, offset, buffer)).build()?;
            }
        }
        store.delete(key_range(file, file_size)).build()?;

        drop(name2file);

        transaction.commit().await?;

        Ok(())
    }

    async fn commit_loop(&self, mut rx: UnboundedReceiver<IdbCommit>) {
        while let Some(commit) = rx.recv().await {
            let IdbCommit { op, notify } = commit;
            let ret = match op {
                IdbCommitOp::Sync(file) => self.sync_db_impl(&file).await,
                IdbCommitOp::Delete(file) => self.delete_db_impl(&file).await,
                IdbCommitOp::Clear => clear_impl(&self.idb).await,
            };
            if let Some(notify) = notify {
                // An unsuccessful send would be one where the corresponding receiver
                // has already been deallocated.
                let _ = notify.send(ret);
            }
        }
    }
}

static ONCE_JS_VALUE: Lazy<FragileComfirmed<(JsValue, JsValue, JsValue)>> = Lazy::new(|| {
    FragileComfirmed::new((
        JsValue::from("path"),
        JsValue::from("offset"),
        JsValue::from("data"),
    ))
});

fn get_block(value: JsValue) -> (String, usize, Uint8Array) {
    let path = Reflect::get(&value, &ONCE_JS_VALUE.0)
        .unwrap()
        .as_string()
        .unwrap();
    let offset = Reflect::get(&value, &ONCE_JS_VALUE.1)
        .unwrap()
        .as_f64()
        .unwrap() as usize;
    let data = Reflect::get(&value, &ONCE_JS_VALUE.2).unwrap();

    (path, offset, Uint8Array::from(data))
}

fn set_block(path: &JsValue, offset: usize, data: &Uint8Array) -> JsValue {
    let block = Object::new();
    Reflect::set(&block, &ONCE_JS_VALUE.0, path).unwrap();
    Reflect::set(&block, &ONCE_JS_VALUE.1, &JsValue::from(offset)).unwrap();
    Reflect::set(&block, &ONCE_JS_VALUE.2, &JsValue::from(data)).unwrap();
    block.into()
}

struct RelaxedIdbStore;

impl VfsStore<IdbFile, RelaxedIdb> for RelaxedIdbStore {
    fn add_file(vfs: *mut sqlite3_vfs, file: &str, flags: i32) -> VfsResult<()> {
        let pool = unsafe { Self::app_data(vfs) };
        pool.name2file
            .write()
            .insert(file.into(), IdbFile::new(flags));
        Ok(())
    }

    fn contains_file(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<bool> {
        let pool = unsafe { Self::app_data(vfs) };
        Ok(pool.name2file.read().contains_key(file))
    }

    fn delete_file(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<()> {
        let pool = unsafe { Self::app_data(vfs) };
        let idb_file = match pool.name2file.write().remove(file) {
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

    fn with_file<F: Fn(&IdbFile) -> i32>(vfs_file: &SQLiteVfsFile, f: F) -> VfsResult<i32> {
        let name = unsafe { vfs_file.name() };
        let pool = unsafe { Self::app_data(vfs_file.vfs) };
        match pool.name2file.read().get(name) {
            Some(file) => Ok(f(file)),
            None => Err(VfsError::new(SQLITE_IOERR, format!("{name} not found"))),
        }
    }

    fn with_file_mut<F: Fn(&mut IdbFile) -> i32>(vfs_file: &SQLiteVfsFile, f: F) -> VfsResult<i32> {
        let name = unsafe { vfs_file.name() };
        let pool = unsafe { Self::app_data(vfs_file.vfs) };
        match pool.name2file.write().get_mut(name) {
            Some(file) => Ok(f(file)),
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

        let mut name2file = pool.name2file.write();
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

                let key = check_result!(CStr::from_ptr(name).to_str()).to_ascii_lowercase();
                let value = check_result!(CStr::from_ptr(value).to_str()).to_ascii_lowercase();

                if key == "page_size" {
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
                } else if key == "synchronous" && value != "off" {
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

struct RelaxedIdbVfs;

impl SQLiteVfs<RelaxedIdbIoMethods> for RelaxedIdbVfs {
    const VERSION: ::std::os::raw::c_int = 1;
}

/// Waiting for commit result
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

    /// Build `RelaxedIdbCfg`
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

/// RelaxedIdb management tools exposed to clients.
pub struct RelaxedIdbUtil {
    pool: &'static VfsAppData<RelaxedIdb>,
}

impl RelaxedIdbUtil {
    /// Preload the db.
    ///
    /// Because indexed db reading data is an asynchronous operation,
    /// the db must be preloaded into memory before opening the sqlite db.
    pub async fn preload_db(&self, prelod: Vec<String>) -> Result<()> {
        self.pool.preload_db(prelod).await
    }

    /// Import the db file.
    ///
    /// If the database is imported with WAL mode enabled,
    /// it will be forced to write back to legacy mode, see
    /// <https://sqlite.org/forum/forumpost/67882c5b04>
    ///
    /// If the imported DB is encrypted, use `import_db_unchecked` instead.
    pub fn import_db(&self, path: &str, bytes: &[u8]) -> Result<WaitCommit> {
        self.pool.import_db(path, bytes)
    }

    /// Can be used to import encrypted DB
    pub fn import_db_unchecked(
        &self,
        path: &str,
        bytes: &[u8],
        page_size: usize,
    ) -> Result<WaitCommit> {
        self.pool.import_db_unchecked(path, bytes, page_size, false)
    }

    /// Export database
    pub fn export_db(&self, name: &str) -> Result<Vec<u8>> {
        self.pool.export_db(name)
    }

    /// Delete the specified db, please make sure that the db is closed.
    pub fn delete_db(&self, name: &str) -> Result<WaitCommit> {
        self.pool.delete_db(name)
    }

    /// Delete all dbs, please make sure that all dbs is closed.
    pub fn clear_all(&self) -> Result<WaitCommit> {
        self.pool.clear_all()
    }

    /// Does the DB exist.
    pub fn exists(&self, file: &str) -> bool {
        self.pool.exists(file)
    }
}

/// Register `relaxed-idb` vfs and return a utility object which can be used
/// to perform basic administration of the file pool
pub async fn install(options: Option<&RelaxedIdbCfg>, default_vfs: bool) -> Result<RelaxedIdbUtil> {
    static NAME2VFS: Lazy<tokio::sync::Mutex<HashMap<String, &'static VfsAppData<RelaxedIdb>>>> =
        Lazy::new(|| tokio::sync::Mutex::new(HashMap::new()));

    let default_options = RelaxedIdbCfg::default();
    let options = options.unwrap_or(&default_options);
    let vfs_name = &options.vfs_name;

    let mut name2vfs = NAME2VFS.lock().await;
    let pool = if let Some(pool) = name2vfs.get(vfs_name) {
        pool
    } else {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let pool = RelaxedIdb::new(options, tx).await?;
        let vfs = register_vfs::<RelaxedIdbIoMethods, RelaxedIdbVfs>(vfs_name, pool, default_vfs)?;
        let app_data = unsafe { RelaxedIdbStore::app_data(vfs) };

        name2vfs.insert(vfs_name.into(), app_data);
        wasm_bindgen_futures::spawn_local(app_data.commit_loop(rx));
        app_data
    };

    Ok(RelaxedIdbUtil { pool })
}
