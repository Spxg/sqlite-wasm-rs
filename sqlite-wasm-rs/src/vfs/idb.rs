use crate::vfs::utils::{
    copy_to_uint8_array, copy_to_vec, get_random_name, FilePtr, FragileComfirmed, VfsPtr,
};
use crate::{bail, check_option, check_result, libsqlite3::*};

use indexed_db_futures::database::Database;
use indexed_db_futures::prelude::*;
use indexed_db_futures::transaction::TransactionMode;
use js_sys::{Number, Object, Reflect, Uint8Array};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::{
    collections::HashMap,
    ffi::{c_char, CStr, CString},
    sync::Arc,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use wasm_bindgen::JsValue;

enum IdbCommitOp {
    Sync,
    Delete,
}

struct IdbCommit {
    file: String,
    op: IdbCommitOp,
}

struct IdbPool {
    idb: FragileComfirmed<Database>,
    file2name: RwLock<HashMap<FilePtr, String>>,
    name2file: RwLock<HashMap<String, IdbFile>>,
    tx: UnboundedSender<IdbCommit>,
}

fn key_range(file: &str, start: usize) -> std::ops::RangeInclusive<[JsValue; 2]> {
    [JsValue::from(file), JsValue::from(start)]
        ..=[
            JsValue::from(file),
            JsValue::from(Number::POSITIVE_INFINITY),
        ]
}

async fn preload_db_impl(
    indexed_db: &Database,
    preload: Preload,
) -> Result<HashMap<String, IdbFile>, IndexedDbError> {
    let transaction = indexed_db
        .transaction("blocks")
        .with_mode(TransactionMode::Readonly)
        .build()?;
    let blocks = transaction.object_store("blocks")?;

    let mut name2file = HashMap::new();
    let mut insert_fn = |block: JsValue| {
        let (path, offset, data) = get_block(block);
        let IdbFile {
            db: Idb::Main(db), ..
        } = name2file.entry(path.clone()).or_insert_with(|| IdbFile {
            // unknown for now
            flags: 0,
            db: Idb::Main(IdbBlocks {
                file_size: 0,
                block_size: data.len(),
                blocks: HashMap::new(),
                tx_blocks: HashMap::new(),
            }),
        })
        else {
            unreachable!();
        };
        db.file_size += db.block_size;
        db.blocks.insert(offset, data);
    };

    match preload {
        Preload::Empty => (),
        Preload::All => {
            for block in blocks.get_all::<JsValue>().await? {
                insert_fn(block?);
            }
        }
        Preload::Paths(items) => {
            for preload in items {
                for block in blocks
                    .get_all::<JsValue>()
                    .with_query(key_range(&preload, 0))
                    .await?
                {
                    insert_fn(block?);
                }
            }
        }
    }
    transaction.commit().await?;

    Ok(name2file)
}

impl IdbPool {
    async fn new(
        vfs_name: &str,
        preload: Preload,
        tx: UnboundedSender<IdbCommit>,
    ) -> Result<Self, IndexedDbError> {
        let indexed_db = Database::open(vfs_name)
            .with_version(1u8)
            .with_on_upgrade_needed(|_, db| {
                db.create_object_store("blocks")
                    .with_key_path(["path", "offset"].into())
                    .build()?;
                Ok(())
            })
            .await?;
        let name2file = preload_db_impl(&indexed_db, preload).await?;
        Ok(IdbPool {
            idb: FragileComfirmed::new(indexed_db),
            file2name: RwLock::new(HashMap::new()),
            name2file: RwLock::new(name2file),
            tx,
        })
    }

    async fn preload_db(&self, preload: Vec<String>) -> Result<(), IndexedDbError> {
        let preload = {
            let name2file = self.name2file.read();
            preload
                .into_iter()
                .filter(|x| !name2file.contains_key(x))
                .collect::<Vec<_>>()
        };
        let preload = preload_db_impl(&self.idb, Preload::Paths(preload)).await?;
        self.name2file.write().extend(preload);
        Ok(())
    }

    async fn commit_loop(self: Arc<Self>, mut rx: UnboundedReceiver<IdbCommit>) {
        async fn to_commit(pool: &IdbPool, commit: IdbCommit) -> Result<(), IndexedDbError> {
            let IdbCommit { file, op } = commit;

            let transaction = pool
                .idb
                .transaction("blocks")
                .with_mode(TransactionMode::Readwrite)
                .build()?;

            let store = transaction.object_store("blocks")?;

            match op {
                IdbCommitOp::Sync => {
                    let mut name2file = pool.name2file.write();
                    let Some(idb_file) = name2file.get_mut(&file) else {
                        return Ok(());
                    };

                    let Idb::Main(idb_blocks) = &mut idb_file.db else {
                        return Ok(());
                    };

                    let tx_blocks = std::mem::take(&mut idb_blocks.tx_blocks);
                    let blocks = &mut idb_blocks.blocks;
                    for (offset, data) in &tx_blocks {
                        blocks.insert(*offset, data.clone());
                    }

                    let file_size = idb_blocks.file_size;
                    let mut truncated_offset = idb_blocks.file_size;
                    while idb_blocks.blocks.remove(&truncated_offset).is_some() {
                        truncated_offset += idb_blocks.block_size;
                    }
                    drop(name2file);

                    for (offset, data) in tx_blocks {
                        store.put(&set_block(&file, offset, data)).build()?;
                    }

                    store.delete(key_range(&file, file_size)).build()?;
                }
                IdbCommitOp::Delete => {
                    store.delete(key_range(&file, 0)).build()?;
                }
            }
            transaction.commit().await?;

            Ok(())
        }

        while let Some(commit) = rx.recv().await {
            if to_commit(&self, commit).await.is_err() {
                // Todo: and log
            }
        }
    }
}

fn get_block(value: JsValue) -> (String, usize, Vec<u8>) {
    let path = Reflect::get(&value, &JsValue::from("path"))
        .unwrap()
        .as_string()
        .unwrap();
    let offset = Reflect::get(&value, &JsValue::from("offset"))
        .unwrap()
        .as_f64()
        .unwrap() as usize;
    let data = Reflect::get(&value, &JsValue::from("data")).unwrap();
    let data = copy_to_vec(&Uint8Array::new(&data));

    (path, offset, data)
}

fn set_block(path: &str, offset: usize, data: Vec<u8>) -> JsValue {
    let block = Object::new();
    Reflect::set(&block, &JsValue::from("path"), &JsValue::from(path)).unwrap();
    Reflect::set(&block, &JsValue::from("offset"), &JsValue::from(offset)).unwrap();
    Reflect::set(
        &block,
        &JsValue::from("data"),
        &JsValue::from(copy_to_uint8_array(&data)),
    )
    .unwrap();
    block.into()
}

struct IdbFile {
    flags: i32,
    db: Idb,
}

enum Idb {
    Main(IdbBlocks),
    Temp(Vec<u8>),
}

struct IdbBlocks {
    file_size: usize,
    block_size: usize,
    blocks: HashMap<usize, Vec<u8>>,
    tx_blocks: HashMap<usize, Vec<u8>>,
}

impl IdbFile {
    fn new(flags: i32) -> Self {
        let db = if flags & SQLITE_OPEN_MAIN_DB == 0 {
            Idb::Temp(vec![])
        } else {
            Idb::Main(IdbBlocks {
                file_size: 0,
                block_size: 0,
                blocks: HashMap::new(),
                tx_blocks: HashMap::from([(0, vec![])]),
            })
        };
        Self { flags, db }
    }

    fn delete_on_close(&self) -> bool {
        self.flags & SQLITE_OPEN_DELETEONCLOSE != 0
    }
}

static VFS2POOL: Lazy<RwLock<HashMap<VfsPtr, Arc<IdbPool>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

fn pool(vfs: *mut sqlite3_vfs) -> Arc<IdbPool> {
    // Already registered vfs will not be unregistered, so this is safe
    Arc::clone(VFS2POOL.read().get(&VfsPtr(vfs)).unwrap())
}

#[repr(C)]
struct SqliteIdbFile {
    io_methods: sqlite3_file,
    vfs: *mut sqlite3_vfs,
}

unsafe fn file2vfs(file: *mut sqlite3_file) -> *mut sqlite3_vfs {
    (*(file.cast::<SqliteIdbFile>())).vfs
}

unsafe extern "C" fn xOpen(
    pVfs: *mut sqlite3_vfs,
    zName: sqlite3_filename,
    pFile: *mut sqlite3_file,
    flags: ::std::os::raw::c_int,
    pOutFlags: *mut ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    let name = if zName.is_null() {
        get_random_name()
    } else {
        check_result!(CStr::from_ptr(zName).to_str()).into()
    };

    let pool = pool(pVfs);
    let mut name2file = pool.name2file.write();
    let mut file2name = pool.file2name.write();

    if let Some(file) = name2file.get_mut(&name) {
        file.flags = flags;
    } else {
        if flags & SQLITE_OPEN_CREATE == 0 {
            return SQLITE_CANTOPEN;
        }
        name2file.insert(name.clone(), IdbFile::new(flags));
    }

    file2name.insert(FilePtr(pFile), name);

    (*(pFile.cast::<SqliteIdbFile>())).vfs = pVfs;
    (*pFile).pMethods = &IO_METHODS;

    if !pOutFlags.is_null() {
        *pOutFlags = flags;
    }

    SQLITE_OK
}

unsafe extern "C" fn xDelete(
    pVfs: *mut sqlite3_vfs,
    zName: *const ::std::os::raw::c_char,
    _syncDir: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    bail!(zName.is_null(), SQLITE_IOERR_DELETE);
    let s = check_result!(CStr::from_ptr(zName).to_str());

    let pool = pool(pVfs);
    pool.name2file.write().remove(s);

    if pool
        .tx
        .send(IdbCommit {
            file: s.into(),
            op: IdbCommitOp::Delete,
        })
        .is_err()
    {
        return SQLITE_ERROR;
    }

    SQLITE_OK
}

unsafe extern "C" fn xAccess(
    pVfs: *mut sqlite3_vfs,
    zName: *const ::std::os::raw::c_char,
    _flags: ::std::os::raw::c_int,
    pResOut: *mut ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    *pResOut = if zName.is_null() {
        0
    } else {
        let s = check_result!(CStr::from_ptr(zName).to_str());
        let pool = pool(pVfs);
        let name2file = pool.name2file.read();
        i32::from(name2file.contains_key(s))
    };

    SQLITE_OK
}

unsafe extern "C" fn xFullPathname(
    _pVfs: *mut sqlite3_vfs,
    zName: *const ::std::os::raw::c_char,
    nOut: ::std::os::raw::c_int,
    zOut: *mut ::std::os::raw::c_char,
) -> ::std::os::raw::c_int {
    bail!(zName.is_null() || zOut.is_null(), SQLITE_CANTOPEN);

    let len = CStr::from_ptr(zName).count_bytes() + 1;

    bail!(len > nOut as usize, SQLITE_CANTOPEN);

    zName.copy_to(zOut, len);

    SQLITE_OK
}

unsafe extern "C" fn xGetLastError(
    _pVfs: *mut sqlite3_vfs,
    _nOut: ::std::os::raw::c_int,
    _zOut: *mut ::std::os::raw::c_char,
) -> ::std::os::raw::c_int {
    SQLITE_OK
}

unsafe extern "C" fn xClose(pFile: *mut sqlite3_file) -> ::std::os::raw::c_int {
    let pool = pool(file2vfs(pFile));
    let mut file2name = pool.file2name.write();
    let mut name2file = pool.name2file.write();

    if let Some(name) = file2name.remove(&FilePtr(pFile)) {
        if name2file.get(&name).is_some_and(|x| x.delete_on_close()) {
            name2file.remove(&name).unwrap();
            if pool
                .tx
                .send(IdbCommit {
                    file: name,
                    op: IdbCommitOp::Delete,
                })
                .is_err()
            {
                return SQLITE_ERROR;
            }
        }
    }
    SQLITE_OK
}

unsafe extern "C" fn xRead(
    pFile: *mut sqlite3_file,
    zBuf: *mut ::std::os::raw::c_void,
    iAmt: ::std::os::raw::c_int,
    iOfst: sqlite3_int64,
) -> ::std::os::raw::c_int {
    let pool = pool(file2vfs(pFile));
    let file2name = pool.file2name.read();
    let name2file = pool.name2file.read();

    let name = check_option!(file2name.get(&FilePtr(pFile)));
    let file = check_option!(name2file.get(name));

    let end = iOfst as usize + iAmt as usize;
    let slice = std::slice::from_raw_parts_mut(zBuf.cast::<u8>(), iAmt as usize);

    match &file.db {
        Idb::Main(file) => {
            if file.block_size == 0 {
                slice.fill(0);
                return SQLITE_IOERR_SHORT_READ;
            }
            let mut bytes_read = 0;
            let mut p_data_offset = 0;
            let p_data_length = iAmt as usize;
            let i_offset = iOfst as usize;
            let block_size = file.block_size;

            while p_data_offset < p_data_length {
                let file_offset = i_offset + p_data_offset;
                let block_idx = file_offset / block_size;
                let block_offset = file_offset % block_size;
                let block_addr = block_idx * file.block_size;

                let Some(block) = file
                    .tx_blocks
                    .get(&block_addr)
                    .or_else(|| file.blocks.get(&block_addr))
                else {
                    break;
                };

                if block.len() < block_offset {
                    break;
                }

                let block_length = (block.len() - block_offset).min(p_data_length - p_data_offset);
                slice[p_data_offset..p_data_offset + block_length]
                    .copy_from_slice(&block[block_offset..block_offset + block_length]);
                p_data_offset += block_length;
                bytes_read += block_length;
            }

            if bytes_read < p_data_length {
                slice[bytes_read..].fill(0);
                return SQLITE_IOERR_SHORT_READ;
            }

            return SQLITE_OK;
        }
        Idb::Temp(data) => {
            if data.len() <= iOfst as usize {
                slice.fill(0);
                return SQLITE_IOERR_SHORT_READ;
            }

            let read_size = end.min(data.len()) - iOfst as usize;
            slice[..read_size].copy_from_slice(&data[iOfst as usize..end.min(data.len())]);

            if read_size < iAmt as usize {
                slice[read_size..iAmt as usize].fill(0);
                return SQLITE_IOERR_SHORT_READ;
            }
        }
    }

    SQLITE_OK
}

unsafe extern "C" fn xWrite(
    pFile: *mut sqlite3_file,
    zBuf: *const ::std::os::raw::c_void,
    iAmt: ::std::os::raw::c_int,
    iOfst: sqlite3_int64,
) -> ::std::os::raw::c_int {
    let pool = pool(file2vfs(pFile));
    let file2name = pool.file2name.read();
    let mut name2file = pool.name2file.write();

    let name = check_option!(file2name.get(&FilePtr(pFile)));
    let file = check_option!(name2file.get_mut(name));

    let end = iOfst as usize + iAmt as usize;
    let slice = std::slice::from_raw_parts(zBuf.cast::<u8>(), iAmt as usize);
    match &mut file.db {
        Idb::Main(file) => {
            file.tx_blocks.insert(iOfst as usize, slice.to_vec());
            file.file_size = file.file_size.max(iOfst as usize + iAmt as usize);
            file.block_size = iAmt as usize;
        }
        Idb::Temp(data) => {
            if end > data.len() {
                data.resize(end, 0);
            }
            data[iOfst as usize..end].copy_from_slice(slice);
        }
    }
    SQLITE_OK
}

unsafe extern "C" fn xTruncate(
    pFile: *mut sqlite3_file,
    size: sqlite3_int64,
) -> ::std::os::raw::c_int {
    let pool = pool(file2vfs(pFile));
    let file2name = pool.file2name.read();
    let mut name2file = pool.name2file.write();

    let name = check_option!(file2name.get(&FilePtr(pFile)));
    let file = check_option!(name2file.get_mut(name));

    match &mut file.db {
        Idb::Main(file) => {
            file.file_size = size as usize;
        }
        Idb::Temp(data) => {
            let now = data.len();
            data.truncate(now.min(size as usize));
        }
    }

    SQLITE_OK
}

unsafe extern "C" fn xSync(
    _pFile: *mut sqlite3_file,
    _flags: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    SQLITE_OK
}

unsafe extern "C" fn xFileSize(
    pFile: *mut sqlite3_file,
    pSize: *mut sqlite3_int64,
) -> ::std::os::raw::c_int {
    let pool = pool(file2vfs(pFile));
    let file2name = pool.file2name.read();
    let name2file = pool.name2file.read();

    let name = check_option!(file2name.get(&FilePtr(pFile)));
    let file = check_option!(name2file.get(name));

    *pSize = match &file.db {
        Idb::Main(file) => file.file_size as sqlite3_int64,
        Idb::Temp(data) => data.len() as sqlite3_int64,
    };

    SQLITE_OK
}

unsafe extern "C" fn xLock(
    _pFile: *mut sqlite3_file,
    _eLock: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    SQLITE_OK
}

unsafe extern "C" fn xUnlock(
    _pFile: *mut sqlite3_file,
    _eLock: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    SQLITE_OK
}

unsafe extern "C" fn xCheckReservedLock(
    _pFile: *mut sqlite3_file,
    pResOut: *mut ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    *pResOut = 0;
    SQLITE_OK
}

unsafe extern "C" fn xFileControl(
    pFile: *mut sqlite3_file,
    op: ::std::os::raw::c_int,
    pArg: *mut ::std::os::raw::c_void,
) -> ::std::os::raw::c_int {
    let pool = pool(file2vfs(pFile));
    let file2name = pool.file2name.read();
    let mut name2file = pool.name2file.write();

    let name = check_option!(file2name.get(&FilePtr(pFile)));
    let file = check_option!(name2file.get_mut(name));

    let Idb::Main(file) = &mut file.db else {
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

            match key {
                "page_size" => {
                    let page_size = check_result!(value.parse::<usize>());

                    if page_size == file.block_size {
                        return SQLITE_OK;
                    } else if file.block_size == 0 {
                        file.block_size = page_size;
                    } else {
                        return SQLITE_ERROR;
                    }
                }
                "synchronous" => {
                    if value == "full" {
                        return SQLITE_ERROR;
                    }
                }
                _ => return SQLITE_NOTFOUND,
            }
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

unsafe extern "C" fn xSectorSize(_pFile: *mut sqlite3_file) -> ::std::os::raw::c_int {
    512
}

unsafe extern "C" fn xDeviceCharacteristics(_arg1: *mut sqlite3_file) -> ::std::os::raw::c_int {
    0
}

static IO_METHODS: sqlite3_io_methods = sqlite3_io_methods {
    iVersion: 1,
    xClose: Some(xClose),
    xRead: Some(xRead),
    xWrite: Some(xWrite),
    xTruncate: Some(xTruncate),
    xSync: Some(xSync),
    xFileSize: Some(xFileSize),
    xLock: Some(xLock),
    xUnlock: Some(xUnlock),
    xCheckReservedLock: Some(xCheckReservedLock),
    xFileControl: Some(xFileControl),
    xSectorSize: Some(xSectorSize),
    xDeviceCharacteristics: Some(xDeviceCharacteristics),
    xShmMap: None,
    xShmLock: None,
    xShmBarrier: None,
    xShmUnmap: None,
    xFetch: None,
    xUnfetch: None,
};

fn vfs(name: *const ::std::os::raw::c_char) -> sqlite3_vfs {
    let default_vfs = unsafe { sqlite3_vfs_find(std::ptr::null()) };
    let xRandomness = unsafe { (*default_vfs).xRandomness };
    let xSleep = unsafe { (*default_vfs).xSleep };
    let xCurrentTime = unsafe { (*default_vfs).xCurrentTime };
    let xCurrentTimeInt64 = unsafe { (*default_vfs).xCurrentTimeInt64 };

    sqlite3_vfs {
        iVersion: 1,
        szOsFile: std::mem::size_of::<SqliteIdbFile>() as i32,
        mxPathname: 1024,
        pNext: std::ptr::null_mut(),
        zName: name,
        pAppData: std::ptr::null_mut(),
        xOpen: Some(xOpen),
        xDelete: Some(xDelete),
        xAccess: Some(xAccess),
        xFullPathname: Some(xFullPathname),
        xDlOpen: None,
        xDlError: None,
        xDlSym: None,
        xDlClose: None,
        xRandomness,
        xSleep,
        xCurrentTime,
        xGetLastError: Some(xGetLastError),
        xCurrentTimeInt64,
        xSetSystemCall: None,
        xGetSystemCall: None,
        xNextSystemCall: None,
    }
}

#[derive(thiserror::Error, Debug)]
pub enum IndexedDbError {
    #[error("indexed db error")]
    IndexedDb(#[from] indexed_db_futures::error::Error),
    #[error("open db error")]
    OpenDb(#[from] indexed_db_futures::error::OpenDbError),
    #[error("custom error")]
    Custom(String),
}

pub enum Preload {
    All,
    Empty,
    Paths(Vec<String>),
}

pub struct IndexedDbUtil {
    pool: Arc<IdbPool>,
}
impl IndexedDbUtil {
    pub async fn preload_db(&self, prelod: Vec<String>) -> Result<(), IndexedDbError> {
        self.pool.preload_db(prelod).await
    }
}

pub async fn install_idb_vfs(
    vfs_name: &str,
    default_vfs: bool,
    preload: Preload,
) -> Result<IndexedDbUtil, IndexedDbError> {
    static NAME2VFS: Lazy<tokio::sync::Mutex<HashMap<String, Arc<IdbPool>>>> =
        Lazy::new(|| tokio::sync::Mutex::new(HashMap::new()));

    let register_vfs = || {
        let name = CString::new(vfs_name).map_err(|e| IndexedDbError::Custom(format!("{e:?}")))?;
        let vfs = Box::leak(Box::new(vfs(name.into_raw())));

        let ret = unsafe { sqlite3_vfs_register(vfs, i32::from(default_vfs)) };
        if ret != SQLITE_OK {
            unsafe {
                drop(Box::from_raw(vfs));
            }
            return Err(IndexedDbError::Custom(format!(
                "register {vfs_name} vfs failed",
            )));
        }

        Ok(vfs as *mut sqlite3_vfs)
    };

    let mut name2vfs = NAME2VFS.lock().await;
    if let Some(pool) = name2vfs.get(vfs_name) {
        Ok(IndexedDbUtil {
            pool: Arc::clone(pool),
        })
    } else {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let pool = Arc::new(IdbPool::new(vfs_name, preload, tx).await?);
        let vfs = register_vfs()?;
        name2vfs.insert(vfs_name.into(), Arc::clone(&pool));
        VFS2POOL.write().insert(VfsPtr(vfs), Arc::clone(&pool));
        wasm_bindgen_futures::spawn_local(Arc::clone(&pool).commit_loop(rx));
        Ok(IndexedDbUtil { pool })
    }
}
