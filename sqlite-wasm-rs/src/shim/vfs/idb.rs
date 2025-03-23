use crate::{export::*, fragile::FragileComfirmed, shim::vfs};

use js_sys::{Function, Object, Promise, Reflect, Uint8Array};
use once_cell::sync::{Lazy, OnceCell};
use parking_lot::{Mutex, MutexGuard, RwLock};
use rexie::{KeyRange, ObjectStore, Rexie, TransactionMode};
use std::{collections::HashMap, ffi::CStr, sync::Arc};
use tokio::sync::{
    mpsc::{UnboundedReceiver, UnboundedSender},
    Notify,
};
use wasm_bindgen::{prelude::Closure, JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

#[derive(Hash, PartialEq, Eq)]
struct VfsPtr(*mut sqlite3_vfs);

/// Just be key
unsafe impl Send for VfsPtr {}

/// Just be key
unsafe impl Sync for VfsPtr {}

struct IdbPool {
    idb: FragileComfirmed<Rexie>,
    name2file: RwLock<HashMap<String, IdbFile>>,
    block_size: OnceCell<usize>,
    tx: UnboundedSender<String>,
}

impl IdbPool {
    async fn new(vfs_name: &str, tx: UnboundedSender<String>) -> Result<Self, IndexedDbError> {
        let rexie = Rexie::builder(vfs_name)
            .version(1)
            .add_object_store(ObjectStore::new("blocks").key_path_array(["path", "offset"]))
            .build()
            .await?;

        let transaction = rexie.transaction(&["blocks"], TransactionMode::ReadWrite)?;
        let blocks = transaction.store("blocks")?;

        let mut name2file = HashMap::new();
        for block in blocks.get_all(None, None).await? {
            let (path, offset, data) = get_block(block);
            name2file
                .entry(path.clone())
                .or_insert_with(|| IdbFile {
                    flags: 0,
                    blocks: HashMap::new(),
                    tx_blocks: HashMap::new(),
                })
                .blocks
                .insert(offset, data);
        }

        Ok(IdbPool {
            idb: FragileComfirmed::new(rexie),
            name2file: RwLock::new(name2file),
            block_size: OnceCell::new(),
            tx,
        })
    }

    async fn commit_loop(self: Arc<Self>, mut rx: UnboundedReceiver<String>) {
        while let Some(file) = rx.recv().await {
            let mut name2file = self.name2file.write();
            let Some(idb_file) = name2file.get_mut(&file) else {
                continue;
            };
            let tx_blocks = std::mem::take(&mut idb_file.tx_blocks);
            let blocks = &mut idb_file.blocks;
            for (offset, data) in &tx_blocks {
                blocks.insert(*offset, data.clone());
            }
            let Ok(transactiion) = self
                .idb
                .transaction(&["blocks"], TransactionMode::ReadWrite)
            else {
                continue;
            };
            let Ok(blocks) = transactiion.store("blocks") else {
                continue;
            };
            for (offset, data) in tx_blocks {
                let _ = blocks.put(&set_block(&file, offset, data), None).await;
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
    // todo
    let data = Uint8Array::from(data).to_vec();

    (path, offset, data)
}

fn set_block(path: &str, offset: usize, data: Vec<u8>) -> JsValue {
    let block = Object::new();
    Reflect::set(&block, &JsValue::from("path"), &JsValue::from(path)).unwrap();
    Reflect::set(&block, &JsValue::from("offset"), &JsValue::from(offset)).unwrap();
    Reflect::set(&block, &JsValue::from("data"), &JsValue::from(data)).unwrap();
    block.into()
}

/// An open file
struct IdbFile {
    /// flags
    flags: i32,
    blocks: HashMap<usize, Vec<u8>>,
    tx_blocks: HashMap<usize, Vec<u8>>,
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

/// pFile -> mem_file
fn file2name() -> MutexGuard<'static, HashMap<usize, String>> {
    static PFILE: Lazy<Mutex<HashMap<usize, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));

    PFILE.lock()
}

unsafe extern "C" fn xOpen(
    pVfs: *mut sqlite3_vfs,
    zName: sqlite3_filename,
    pFile: *mut sqlite3_file,
    flags: ::std::os::raw::c_int,
    pOutFlags: *mut ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    let Ok(s) = CStr::from_ptr(zName).to_str() else {
        return SQLITE_ERROR;
    };

    let pool = pool(pVfs);
    let mut name2file = pool.name2file.write();
    if let Some(file) = name2file.get_mut(s) {
        file.flags = flags;
    } else {
        if flags & SQLITE_OPEN_CREATE == 0 {
            return SQLITE_CANTOPEN;
        }
        let idb_file = if flags & SQLITE_OPEN_MAIN_DB == 0 || flags & SQLITE_OPEN_DELETEONCLOSE != 0
        {
            IdbFile {
                flags,
                blocks: HashMap::from([(0, vec![])]),
                tx_blocks: HashMap::new(),
            }
        } else {
            IdbFile {
                flags,
                blocks: HashMap::new(),
                tx_blocks: HashMap::from([(0, vec![])]),
            }
        };

        name2file.insert(s.into(), idb_file);
    }

    // (*pFile).pMethods = &IO_METHODS;

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
    let Ok(s) = CStr::from_ptr(zName).to_str() else {
        return SQLITE_ERROR;
    };
    let pool = pool(pVfs);
    let mut name2file = pool.name2file.write();
    name2file.remove(s);
    SQLITE_OK
}

unsafe extern "C" fn xAccess(
    pVfs: *mut sqlite3_vfs,
    zName: *const ::std::os::raw::c_char,
    _flags: ::std::os::raw::c_int,
    pResOut: *mut ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    let Ok(s) = CStr::from_ptr(zName).to_str() else {
        return SQLITE_ERROR;
    };
    let pool = pool(pVfs);
    let mut name2file = pool.name2file.read();

    *pResOut = i32::from(name2file.contains_key(s));
    SQLITE_OK
}

unsafe extern "C" fn xFullPathname(
    _pVfs: *mut sqlite3_vfs,
    zName: *const ::std::os::raw::c_char,
    nOut: ::std::os::raw::c_int,
    zOut: *mut ::std::os::raw::c_char,
) -> ::std::os::raw::c_int {
    zName.copy_to(zOut, nOut as usize);
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
    let mut name2file = pool.name2file.write();
    if let Some(name) = file2name().remove(&(pFile as usize)) {
        let _ = name2file.remove(&name);
    }
    SQLITE_OK
}
unsafe extern "C" fn xRead(
    pFile: *mut sqlite3_file,
    zBuf: *mut ::std::os::raw::c_void,
    iAmt: ::std::os::raw::c_int,
    iOfst: sqlite3_int64,
) -> ::std::os::raw::c_int {
    let vfs = file2vfs(pFile);
    let pool = pool(vfs);
    todo!()
}

#[derive(thiserror::Error, Debug)]
pub enum IndexedDbError {
    #[error("rexie error")]
    Rexie(#[from] rexie::Error),
}

pub async fn install_idb_vfs(vfs_name: &str) -> Result<(), IndexedDbError> {
    static NAME2VFS: Lazy<tokio::sync::Mutex<HashMap<String, Arc<IdbPool>>>> =
        Lazy::new(|| tokio::sync::Mutex::new(HashMap::new()));

    let mut name2vfs = NAME2VFS.lock().await;
    if !name2vfs.contains_key(vfs_name) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let pool = Arc::new(IdbPool::new(vfs_name, tx).await?);
        name2vfs.insert(vfs_name.into(), Arc::clone(&pool));
        // VFS2POOL.write().insert(VfsPtr(vfs), Arc::clone(&pool));
        wasm_bindgen_futures::spawn_local(pool.commit_loop(rx));
    }

    Ok(())
}
