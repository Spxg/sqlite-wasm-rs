//! relaxed-idb vfs implementation

use crate::vfs::utils::{
    copy_to_slice, copy_to_uint8_array, copy_to_uint8_array_subarray, page_read, register_vfs,
    FragileComfirmed, MemLinearStore, SQLiteIoMethods, SQLiteVfs, SQLiteVfsFile, StoreControl,
    VfsError, VfsPtr, VfsStore, SQLITE3_HEADER,
};
use crate::{bail, check_option, check_result, libsqlite3::*};

use indexed_db_futures::database::Database;
use indexed_db_futures::prelude::*;
use indexed_db_futures::transaction::TransactionMode;
use js_sys::{Number, Object, Reflect, Uint8Array};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::collections::{hash_map, HashSet};
use std::sync::Arc;
use std::{
    collections::HashMap,
    ffi::{c_char, CStr},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use wasm_bindgen::JsValue;

type Result<T> = std::result::Result<T, RelaxedIdbError>;

struct IdbCommit {
    file: String,
    op: IdbCommitOp,
}

enum IdbCommitOp {
    Sync,
    Delete,
}

enum IdbFile {
    Main(IdbPageStore),
    Temp(MemLinearStore),
}

impl IdbFile {
    fn new(flags: i32) -> Self {
        if flags & SQLITE_OPEN_MAIN_DB == 0 {
            Self::Temp(MemLinearStore::default())
        } else {
            Self::Main(IdbPageStore::default())
        }
    }
}

#[derive(Default)]
struct IdbPageStore {
    file_size: usize,
    block_size: usize,
    blocks: HashMap<usize, FragileComfirmed<Uint8Array>>,
    tx_blocks: HashSet<usize>,
}

impl VfsStore for IdbPageStore {
    fn read(&self, buf: &mut [u8], offset: usize) -> i32 {
        page_read(
            buf,
            self.block_size,
            self.file_size,
            offset,
            |addr| self.blocks.get(&addr),
            |page, buf, (start, end)| {
                copy_to_slice(&page.subarray(start as u32, end as u32), buf);
            },
        )
    }

    fn write(&mut self, buf: &[u8], offset: usize) {
        let size = buf.len();
        let end = size + offset;
        for fill in (self.file_size..end).step_by(size) {
            self.blocks.insert(
                fill,
                FragileComfirmed::new(Uint8Array::new_with_length(size as u32)),
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
        self.file_size = self.file_size.max(end);
        self.block_size = size;
    }

    fn truncate(&mut self, size: usize) {
        self.file_size = size;
    }

    fn size(&self) -> usize {
        self.file_size
    }
}

impl VfsStore for IdbFile {
    fn read(&self, buf: &mut [u8], offset: usize) -> i32 {
        match self {
            IdbFile::Main(idb_page_store) => idb_page_store.read(buf, offset),
            IdbFile::Temp(mem_linear_store) => mem_linear_store.read(buf, offset),
        }
    }

    fn write(&mut self, buf: &[u8], offset: usize) {
        match self {
            IdbFile::Main(idb_page_store) => idb_page_store.write(buf, offset),
            IdbFile::Temp(mem_linear_store) => mem_linear_store.write(buf, offset),
        }
    }

    fn truncate(&mut self, size: usize) {
        match self {
            IdbFile::Main(idb_page_store) => idb_page_store.truncate(size),
            IdbFile::Temp(mem_linear_store) => mem_linear_store.truncate(size),
        }
    }

    fn size(&self) -> usize {
        match self {
            IdbFile::Main(idb_page_store) => idb_page_store.size(),
            IdbFile::Temp(mem_linear_store) => mem_linear_store.size(),
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
    blocks.clear()?.await?;
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
                vacant_entry.insert(IdbFile::Main(IdbPageStore {
                    file_size: data.length() as _,
                    block_size: data.length() as _,
                    blocks: HashMap::from([(offset, FragileComfirmed::new(data))]),
                    tx_blocks: HashSet::new(),
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

    async fn import_db(&self, path: &str, bytes: &[u8], page_size: usize) -> Result<()> {
        if !(page_size.is_power_of_two() && (512..=65536).contains(&page_size)) {
            return Err(RelaxedIdbError::Generic(
                "The page size must be a power of two between 512 and 65536 inclusive.".into(),
            ));
        }

        if self.name2file.read().contains_key(path) {
            return Err(RelaxedIdbError::Generic(format!(
                "{path} file already exists"
            )));
        }

        if SQLITE3_HEADER
            .as_bytes()
            .iter()
            .zip(bytes)
            .any(|(x, y)| x != y)
        {
            return Err(RelaxedIdbError::Generic(
                "Input does not contain an SQLite database header.".into(),
            ));
        }

        let mut blocks = HashMap::new();
        for (idx, chunk) in bytes.chunks(page_size).enumerate() {
            let mut buffer = chunk.to_vec();
            if buffer.len() < page_size {
                buffer.resize(page_size, 0);
            }
            blocks.insert(
                idx * page_size,
                FragileComfirmed::new(copy_to_uint8_array(&buffer)),
            );
        }

        let tx_blocks = blocks.keys().copied().collect();
        self.name2file.write().insert(
            path.into(),
            IdbFile::Main(IdbPageStore {
                file_size: blocks.len() * page_size,
                block_size: page_size,
                blocks,
                tx_blocks,
            }),
        );

        self.sync_file_impl(path).await?;

        Ok(())
    }

    fn export_file(&self, name: &str) -> Result<Vec<u8>> {
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

    async fn delete_file(&self, name: &str) -> Result<()> {
        self.name2file.write().remove(name);
        self.delete_file_impl(name).await?;
        Ok(())
    }

    async fn clear_all(&self) -> Result<()> {
        std::mem::take(&mut *self.name2file.write());
        clear_impl(&self.idb).await
    }

    async fn delete_file_impl(&self, file: &str) -> Result<()> {
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
    async fn sync_file_impl(&self, file: &str) -> Result<()> {
        let mut name2file = self.name2file.write();
        let Some(idb_file) = name2file.get_mut(file) else {
            return Ok(());
        };

        let IdbFile::Main(idb_blocks) = idb_file else {
            return Ok(());
        };

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

    async fn commit_loop(self: Arc<Self>, mut rx: UnboundedReceiver<IdbCommit>) {
        while let Some(commit) = rx.recv().await {
            let IdbCommit { file, op } = commit;
            if let Err(_e) = match op {
                IdbCommitOp::Sync => self.sync_file_impl(&file).await,
                IdbCommitOp::Delete => self.delete_file_impl(&file).await,
            } {}
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

static VFS2POOL: Lazy<RwLock<HashMap<VfsPtr, Arc<RelaxedIdb>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

fn pool(vfs: *mut sqlite3_vfs) -> Arc<RelaxedIdb> {
    // Already registered vfs will not be unregistered, so this is safe
    Arc::clone(VFS2POOL.read().get(&VfsPtr(vfs)).unwrap())
}

struct RelaxedIdbStoreControl;

impl StoreControl<IdbFile> for RelaxedIdbStoreControl {
    fn add_file(vfs: *mut sqlite3_vfs, file: &str, flags: i32) {
        pool(vfs)
            .name2file
            .write()
            .insert(file.into(), IdbFile::new(flags));
    }

    fn contains_file(vfs: *mut sqlite3_vfs, file: &str) -> bool {
        pool(vfs).name2file.read().contains_key(file)
    }

    fn delete_file(vfs: *mut sqlite3_vfs, file: &str) -> Option<IdbFile> {
        let pool = pool(vfs);
        let idb_file = pool.name2file.write().remove(file);
        // temp db never put into indexed db, no need to delete
        if let Some(IdbFile::Main(_)) = &idb_file {
            if pool
                .tx
                .send(IdbCommit {
                    file: file.into(),
                    op: IdbCommitOp::Delete,
                })
                .is_err()
            {}
        }
        idb_file
    }

    fn with_file<F: Fn(&IdbFile) -> i32>(vfs_file: &SQLiteVfsFile, f: F) -> Option<i32> {
        Some(unsafe { f(pool(vfs_file.vfs).name2file.read().get(vfs_file.name())?) })
    }

    fn with_file_mut<F: Fn(&mut IdbFile) -> i32>(vfs_file: &SQLiteVfsFile, f: F) -> Option<i32> {
        Some(unsafe {
            f(pool(vfs_file.vfs)
                .name2file
                .write()
                .get_mut(vfs_file.name())?)
        })
    }
}

struct RelaxedIdbIoMethods;

impl SQLiteIoMethods for RelaxedIdbIoMethods {
    type Store = IdbFile;
    type StoreControl = RelaxedIdbStoreControl;

    const VERSION: ::std::os::raw::c_int = 1;

    unsafe extern "C" fn xFileControl(
        pFile: *mut sqlite3_file,
        op: ::std::os::raw::c_int,
        pArg: *mut ::std::os::raw::c_void,
    ) -> ::std::os::raw::c_int {
        let vfs_file = SQLiteVfsFile::from_file(pFile);
        let pool = pool(vfs_file.vfs);
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
                        return SQLITE_ERROR;
                    }
                } else if key == "synchronous" && value == "full" {
                    return SQLITE_ERROR;
                };
            }
            SQLITE_FCNTL_SYNC | SQLITE_FCNTL_COMMIT_PHASETWO => {
                if pool
                    .tx
                    .send(IdbCommit {
                        file: name.into(),
                        op: IdbCommitOp::Sync,
                    })
                    .is_err()
                {
                    return SQLITE_ERROR;
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

#[derive(thiserror::Error, Debug)]
pub enum RelaxedIdbError {
    #[error(transparent)]
    Vfs(#[from] VfsError),
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
    pool: Arc<RelaxedIdb>,
}

impl RelaxedIdbUtil {
    /// Preload the db.
    /// Because indexed db reading data is an asynchronous operation,
    /// the db must be preloaded into memory before opening the sqlite db.
    pub async fn preload_db(&self, prelod: Vec<String>) -> Result<()> {
        self.pool.preload_db(prelod).await
    }

    /// Import the db file into the pool and indexed db.
    /// The page_size must be a power of two between 512 and 65536 inclusive.
    pub async fn import_db(&self, path: &str, bytes: &[u8], page_size: usize) -> Result<()> {
        self.pool.import_db(path, bytes, page_size).await
    }

    /// Export database
    pub fn export_file(&self, name: &str) -> Result<Vec<u8>> {
        self.pool.export_file(name)
    }

    /// Delete the specified file in the indexed db.
    ///
    /// # Attention
    ///
    /// Please make sure that the deleted db is closed.
    pub async fn delete_file(&self, name: &str) -> Result<()> {
        self.pool.delete_file(name).await
    }

    /// Delete all files in the indexed db.
    ///
    /// # Attention
    ///
    /// Please make sure that all dbs is closed.
    pub async fn clear_all(&self) -> Result<()> {
        self.pool.clear_all().await
    }
}

/// Register `relaxed-idb` vfs and return a utility object which can be used
/// to perform basic administration of the file pool
pub async fn install(options: Option<&RelaxedIdbCfg>, default_vfs: bool) -> Result<RelaxedIdbUtil> {
    static NAME2VFS: Lazy<tokio::sync::Mutex<HashMap<String, Arc<RelaxedIdb>>>> =
        Lazy::new(|| tokio::sync::Mutex::new(HashMap::new()));

    let default_options = RelaxedIdbCfg::default();
    let options = options.unwrap_or(&default_options);
    let vfs_name = &options.vfs_name;

    let mut name2vfs = NAME2VFS.lock().await;
    let pool = if let Some(pool) = name2vfs.get(vfs_name) {
        Arc::clone(pool)
    } else {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let pool = Arc::new(RelaxedIdb::new(options, tx).await?);
        let vfs = register_vfs(vfs_name, default_vfs, RelaxedIdbVfs::vfs)?;

        name2vfs.insert(vfs_name.into(), Arc::clone(&pool));
        VFS2POOL.write().insert(VfsPtr(vfs), Arc::clone(&pool));
        wasm_bindgen_futures::spawn_local(Arc::clone(&pool).commit_loop(rx));
        pool
    };

    Ok(RelaxedIdbUtil { pool })
}
