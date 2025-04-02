//! relaxed-idb vfs implementation

use crate::vfs::utils::{
    copy_to_uint8_array, copy_to_vec, get_random_name, register_vfs, FragileComfirmed,
    SQLiteVfsFile, VfsError, VfsPtr, SQLITE3_HEADER,
};
use crate::{bail, check_option, check_result, libsqlite3::*};

use indexed_db_futures::database::Database;
use indexed_db_futures::prelude::*;
use indexed_db_futures::transaction::TransactionMode;
use js_sys::{Number, Object, Reflect, Uint8Array};
use once_cell::sync::{Lazy, OnceCell};
use parking_lot::RwLock;
use std::cell::Cell;
use std::collections::hash_map;
use std::{
    collections::HashMap,
    ffi::{c_char, CStr},
    sync::Arc,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::Notify;
use wasm_bindgen::JsValue;

type Result<T> = std::result::Result<T, RelaxedIdbError>;

struct IdbCommit {
    file: String,
    op: IdbCommitOp,
    notify: Option<Arc<Notify>>,
}

enum IdbCommitOp {
    Sync,
    Delete,
}

enum IdbFile {
    Main(IdbBlockFile),
    Temp(Vec<u8>),
}

impl IdbFile {
    fn new(flags: i32) -> Self {
        if flags & SQLITE_OPEN_MAIN_DB == 0 {
            Self::Temp(vec![])
        } else {
            Self::Main(IdbBlockFile {
                file_size: 0,
                block_size: 0,
                blocks: HashMap::new(),
                tx_blocks: vec![],
            })
        }
    }
}

struct IdbBlockFile {
    file_size: usize,
    block_size: usize,
    blocks: HashMap<usize, LazyBuffer>,
    tx_blocks: Vec<usize>,
}

struct LazyBuffer {
    cell: OnceCell<Vec<u8>>,
    init: Cell<Option<FragileComfirmed<Uint8Array>>>,
}

unsafe impl Sync for LazyBuffer {}

impl LazyBuffer {
    fn new(value: Uint8Array) -> Self {
        Self {
            cell: OnceCell::new(),
            init: Cell::new(Some(FragileComfirmed::new(value))),
        }
    }

    fn ready(value: Vec<u8>) -> Self {
        Self {
            cell: OnceCell::with_value(value),
            init: Cell::new(None),
        }
    }

    fn get(&self) -> &Vec<u8> {
        self.cell
            .get_or_init(|| copy_to_vec(&self.init.take().unwrap()))
    }

    fn get_mut(&mut self) -> Option<&mut Vec<u8>> {
        self.cell.get_mut()
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
                db.blocks.insert(offset, LazyBuffer::new(data));
            }
            hash_map::Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(IdbFile::Main(IdbBlockFile {
                    file_size: data.length() as _,
                    block_size: data.length() as _,
                    blocks: HashMap::from([(offset, LazyBuffer::new(data))]),
                    tx_blocks: Vec::new(),
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
        Preload::None => (),
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
            blocks.insert(idx * page_size, LazyBuffer::ready(buffer));
        }

        let tx_blocks = blocks.keys().copied().collect();
        self.name2file.write().insert(
            path.into(),
            IdbFile::Main(IdbBlockFile {
                file_size: blocks.len() * page_size,
                block_size: page_size,
                blocks,
                tx_blocks,
            }),
        );

        let notify = Arc::new(Notify::new());
        let wait = Arc::clone(&notify);

        if self
            .tx
            .send(IdbCommit {
                file: path.into(),
                op: IdbCommitOp::Sync,
                notify: Some(notify),
            })
            .is_err()
        {
            return Err(RelaxedIdbError::Generic(
                "sync db to indexed db failed".into(),
            ));
        }

        wait.notified().await;

        Ok(())
    }

    fn export_file(&self, name: &str) -> Result<Vec<u8>> {
        let name2file = self.name2file.read();

        if let Some(file) = name2file.get(name) {
            match file {
                IdbFile::Main(file) => {
                    let file_size = file.file_size;
                    let mut ret = vec![0; file.file_size];
                    for (&offset, buffer) in &file.blocks {
                        if offset >= file_size {
                            continue;
                        }
                        ret[offset..offset + file.block_size].copy_from_slice(buffer.get());
                    }
                    Ok(ret)
                }
                IdbFile::Temp(items) => Ok(items.clone()),
            }
        } else {
            Err(RelaxedIdbError::Generic(
                "the file to be exported does not exist".into(),
            ))
        }
    }

    async fn delete_file(&self, name: &str) -> Result<()> {
        self.name2file.write().remove(name);

        let notify = Arc::new(Notify::new());
        let wait = Arc::clone(&notify);

        if self
            .tx
            .send(IdbCommit {
                file: name.into(),
                op: IdbCommitOp::Delete,
                notify: Some(notify),
            })
            .is_err()
        {
            return Err(RelaxedIdbError::Generic(
                "sync db to indexed db failed".into(),
            ));
        }

        wait.notified().await;

        Ok(())
    }

    async fn clear_all(&self) -> Result<()> {
        std::mem::take(&mut *self.name2file.write());
        clear_impl(&self.idb).await
    }

    async fn commit_loop(self: Arc<Self>, mut rx: UnboundedReceiver<IdbCommit>) {
        async fn to_commit(pool: &RelaxedIdb, commit: IdbCommit) -> Result<()> {
            let IdbCommit { file, op, notify } = commit;

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

                    let IdbFile::Main(idb_blocks) = idb_file else {
                        return Ok(());
                    };

                    let tx_blocks = std::mem::take(&mut idb_blocks.tx_blocks);

                    let file_size = idb_blocks.file_size;
                    let mut truncated_offset = idb_blocks.file_size;
                    while idb_blocks.blocks.remove(&truncated_offset).is_some() {
                        truncated_offset += idb_blocks.block_size;
                    }

                    for offset in tx_blocks {
                        if let Some(buffer) = idb_blocks.blocks.get(&offset).map(|x| x.get()) {
                            store.put(&set_block(&file, offset, buffer)).build()?;
                        }
                    }

                    store.delete(key_range(&file, file_size)).build()?;
                }
                IdbCommitOp::Delete => {
                    store.delete(key_range(&file, 0)).build()?;
                }
            }
            transaction.commit().await?;

            if let Some(notify) = notify {
                notify.notify_one();
            }

            Ok(())
        }

        while let Some(commit) = rx.recv().await {
            if to_commit(&self, commit).await.is_err() {}
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

fn set_block(path: &str, offset: usize, data: &[u8]) -> JsValue {
    let block = Object::new();
    Reflect::set(&block, &JsValue::from("path"), &JsValue::from(path)).unwrap();
    Reflect::set(&block, &JsValue::from("offset"), &JsValue::from(offset)).unwrap();
    Reflect::set(
        &block,
        &JsValue::from("data"),
        &JsValue::from(copy_to_uint8_array(data)),
    )
    .unwrap();
    block.into()
}

static VFS2POOL: Lazy<RwLock<HashMap<VfsPtr, Arc<RelaxedIdb>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

fn pool(vfs: *mut sqlite3_vfs) -> Arc<RelaxedIdb> {
    // Already registered vfs will not be unregistered, so this is safe
    Arc::clone(VFS2POOL.read().get(&VfsPtr(vfs)).unwrap())
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

    if !name2file.contains_key(&name) {
        if flags & SQLITE_OPEN_CREATE == 0 {
            return SQLITE_CANTOPEN;
        }
        name2file.insert(name.clone(), IdbFile::new(flags));
    }

    let leak = name.leak();
    let vfs_file = pFile.cast::<SQLiteVfsFile>();
    (*vfs_file).vfs = pVfs;
    (*vfs_file).flags = flags;
    (*vfs_file).name_ptr = leak.as_ptr();
    (*vfs_file).name_length = leak.len();

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
            notify: None,
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
    let vfs_file = SQLiteVfsFile::from_file(pFile);
    let pool = pool(vfs_file.vfs);
    let name = vfs_file.name();

    let mut name2file = pool.name2file.write();
    if vfs_file.flags & SQLITE_OPEN_DELETEONCLOSE != 0 {
        name2file.remove(name);
        if pool
            .tx
            .send(IdbCommit {
                file: name.into(),
                op: IdbCommitOp::Delete,
                notify: None,
            })
            .is_err()
        {
            return SQLITE_ERROR;
        }
    }

    drop(unsafe { Box::from_raw(name) });

    SQLITE_OK
}

unsafe extern "C" fn xRead(
    pFile: *mut sqlite3_file,
    zBuf: *mut ::std::os::raw::c_void,
    iAmt: ::std::os::raw::c_int,
    iOfst: sqlite3_int64,
) -> ::std::os::raw::c_int {
    let vfs_file = SQLiteVfsFile::from_file(pFile);
    let pool = pool(vfs_file.vfs);
    let name = vfs_file.name();

    let name2file = pool.name2file.read();
    let file = check_option!(name2file.get(name));

    let end = iOfst as usize + iAmt as usize;
    let slice = std::slice::from_raw_parts_mut(zBuf.cast::<u8>(), iAmt as usize);

    match file {
        IdbFile::Main(file) => {
            if file.block_size == 0 || file.file_size == 0 {
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

                let Some(block) = file.blocks.get(&block_addr).map(|x| x.get()) else {
                    break;
                };

                assert!(block_size == block.len());
                let block_length = (block_size - block_offset).min(p_data_length - p_data_offset);
                slice[p_data_offset..p_data_offset + block_length]
                    .copy_from_slice(&block[block_offset..block_offset + block_length]);
                p_data_offset += block_length;
                bytes_read += block_length;
            }

            if bytes_read < p_data_length {
                slice[bytes_read..].fill(0);
                return SQLITE_IOERR_SHORT_READ;
            }
        }
        IdbFile::Temp(data) => {
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
    let vfs_file = SQLiteVfsFile::from_file(pFile);
    let pool = pool(vfs_file.vfs);
    let name = vfs_file.name();

    let mut name2file = pool.name2file.write();
    let file = check_option!(name2file.get_mut(name));

    let end = iOfst as usize + iAmt as usize;
    let slice = std::slice::from_raw_parts(zBuf.cast::<u8>(), iAmt as usize);
    match file {
        IdbFile::Main(file) => {
            if let Some(Some(buffer)) = file
                .blocks
                .get_mut(&(iOfst as usize))
                .map(|buffer| buffer.get_mut())
            {
                buffer.copy_from_slice(slice);
            } else {
                file.blocks
                    .insert(iOfst as usize, LazyBuffer::ready(slice.to_vec()));
            }
            file.tx_blocks.push(iOfst as usize);
            file.file_size = file.file_size.max(iOfst as usize + iAmt as usize);
            file.block_size = iAmt as usize;
        }
        IdbFile::Temp(data) => {
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
    let vfs_file = SQLiteVfsFile::from_file(pFile);
    let pool = pool(vfs_file.vfs);
    let name = vfs_file.name();

    let mut name2file = pool.name2file.write();
    let file = check_option!(name2file.get_mut(name));

    match file {
        IdbFile::Main(file) => {
            file.file_size = size as usize;
        }
        IdbFile::Temp(data) => {
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
    let vfs_file = SQLiteVfsFile::from_file(pFile);
    let pool = pool(vfs_file.vfs);
    let name = vfs_file.name();

    let name2file = pool.name2file.read();
    let file = check_option!(name2file.get(name));

    *pSize = match file {
        IdbFile::Main(file) => file.file_size as sqlite3_int64,
        IdbFile::Temp(data) => data.len() as sqlite3_int64,
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
                    notify: None,
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
        szOsFile: std::mem::size_of::<SQLiteVfsFile>() as i32,
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
        let vfs = register_vfs(vfs_name, default_vfs, vfs)?;
        name2vfs.insert(vfs_name.into(), Arc::clone(&pool));
        VFS2POOL.write().insert(VfsPtr(vfs), Arc::clone(&pool));
        wasm_bindgen_futures::spawn_local(Arc::clone(&pool).commit_loop(rx));
        pool
    };

    Ok(RelaxedIdbUtil { pool })
}
