use crate::export::*;

use crate::fragile::FragileComfirmed;
use js_sys::{
    Array, DataView, IteratorNext, Map, Math, Number, Object, Reflect, Set, Uint32Array, Uint8Array,
};
use std::{
    ffi::{CStr, CString},
    sync::Mutex,
    usize,
};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetDirectoryOptions,
    FileSystemGetFileOptions, FileSystemReadWriteOptions, FileSystemSyncAccessHandle, Url,
    WorkerGlobalScope,
};

const DEFAULT_VFS_DIR: &str = ".opfs-sahpool";

const SECTOR_SIZE: usize = 4096;
const HEADER_MAX_PATH_SIZE: usize = 512;
const HEADER_FLAGS_SIZE: usize = 4;
const HEADER_DIGEST_SIZE: usize = 8;
const HEADER_CORPUS_SIZE: usize = HEADER_MAX_PATH_SIZE + HEADER_FLAGS_SIZE;
const HEADER_OFFSET_FLAGS: usize = HEADER_MAX_PATH_SIZE;
const HEADER_OFFSET_DIGEST: usize = HEADER_CORPUS_SIZE;
const HEADER_OFFSET_DATA: usize = SECTOR_SIZE;

const PERSISTENT_FILE_TYPES: i32 =
    SQLITE_OPEN_MAIN_DB | SQLITE_OPEN_MAIN_JOURNAL | SQLITE_OPEN_SUPER_JOURNAL | SQLITE_OPEN_WAL;

/// `OpfsSAHPool` options
pub struct OpfsSAHPoolCfg {
    // If truthy, contents and filename mapping are removed from each SAH
    // as it is acquired during initalization of the VFS, leaving the VFS's
    // storage in a pristine state. Use this only for databases which need not
    // survive a page reload.
    pub clear_on_init: bool,
    // Specifies the default capacity of the VFS, i.e. the number of files
    // it may contain.
    pub initial_capacity: u32,
}

impl Default for OpfsSAHPoolCfg {
    fn default() -> Self {
        Self {
            clear_on_init: false,
            initial_capacity: 6,
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum OpfsSAHError {
    #[error("this vfs is only available in workers")]
    NotSuported,
    #[error("get directory handle error")]
    GetDirHandle(JsValue),
    #[error("get file handle error")]
    GetFileHandle(JsValue),
    #[error("create sync access handle error")]
    CreateSyncAccessHandle(JsValue),
    #[error("iterate handle error")]
    IterHandle(JsValue),
    #[error("get path error")]
    GetPath(JsValue),
    #[error("remove entity error")]
    RemoveEntity(JsValue),
    #[error("get size error")]
    GetSize(JsValue),
    #[error("sah read error")]
    Read(JsValue),
    #[error("sah write error")]
    Write(JsValue),
    #[error("sah flush error")]
    Flush(JsValue),
    #[error("sah truncate error")]
    Truncate(JsValue),
    #[error("custom error")]
    Custom(String),
}

// this function only return [0, 0] for now
//
// https://github.com/sqlite/sqlite-wasm/issues/97
fn compute_digest(_byte_array: &Uint8Array) -> Uint32Array {
    let u32_array = Uint32Array::new_with_length(2);
    u32_array.set_index(0, 0);
    u32_array.set_index(1, 0);
    u32_array
}

fn get_random_name() -> String {
    let random = Number::from(Math::random()).to_string(36).unwrap();
    let name = random.slice(2, random.length()).as_string().unwrap();
    name
}

///  Class for managing OPFS-related state for the OPFS
///  SharedAccessHandle Pool sqlite3_vfs.
struct OpfsSAHPool {
    /// Directory handle to the subdir of vfs root which holds
    /// the randomly-named "opaque" files. This subdir exists in the
    /// hope that we can eventually support client-created files in
    dh_opaque: FileSystemDirectoryHandle,
    /// Buffer used by [sg]etAssociatedPath()
    ap_body: Uint8Array,
    /// DataView for self.apBody
    dv_body: DataView,
    /// Maps client-side file names to SAHs
    map_filename_to_sah: Map,
    /// Set of currently-unused SAHs
    available_sah: Set,
    /// Maps SAHs to their opaque file names
    map_sah_to_name: Map,
    /// Maps (sqlite3_file*) to xOpen's file objects
    map_s3_file_to_o_file: Map,
    /// Store last_error
    last_error: Mutex<Option<(i32, String)>>,
}

impl OpfsSAHPool {
    async fn new(options: Option<&OpfsSAHPoolCfg>) -> Result<OpfsSAHPool, OpfsSAHError> {
        const OPAQUE_DIR_NAME: &str = ".opaque";

        let default_options = OpfsSAHPoolCfg::default();
        let options = options.unwrap_or_else(|| &default_options);

        let vfs_dir = DEFAULT_VFS_DIR;
        let capacity = options.initial_capacity;
        let clear_files = options.clear_on_init;

        let create_option = FileSystemGetDirectoryOptions::new();
        create_option.set_create(true);

        let mut handle: FileSystemDirectoryHandle = JsFuture::from(
            js_sys::global()
                .dyn_into::<WorkerGlobalScope>()
                .map_err(|_| OpfsSAHError::NotSuported)?
                .navigator()
                .storage()
                .get_directory(),
        )
        .await
        .map_err(OpfsSAHError::GetDirHandle)?
        .into();

        for dir in vfs_dir.split('/').filter(|x| !x.is_empty()) {
            let next =
                JsFuture::from(handle.get_directory_handle_with_options(dir, &create_option))
                    .await
                    .map_err(OpfsSAHError::GetDirHandle)?
                    .into();
            handle = next;
        }

        let dh_opaque = JsFuture::from(
            handle.get_directory_handle_with_options(OPAQUE_DIR_NAME, &create_option),
        )
        .await
        .map_err(OpfsSAHError::GetDirHandle)?
        .into();

        let ap_body = Uint8Array::new_with_length(HEADER_CORPUS_SIZE as _);
        let dv_body = DataView::new(
            &ap_body.buffer(),
            ap_body.byte_offset() as usize,
            (ap_body.byte_length() - ap_body.byte_offset()) as usize,
        );

        let pool = Self {
            dh_opaque,
            ap_body,
            dv_body,
            map_filename_to_sah: Map::new(),
            available_sah: Set::default(),
            map_sah_to_name: Map::new(),
            map_s3_file_to_o_file: Map::new(),
            last_error: Mutex::new(None),
        };
        pool.acquire_access_handles(clear_files).await?;
        if pool.get_capacity() == 0 {
            pool.add_capacity(capacity).await?;
        }

        Ok(pool)
    }

    /// Adds n files to the pool's capacity. This change is
    /// persistent across settings. Returns a Promise which resolves
    /// to the new capacity.
    async fn add_capacity(&self, n: u32) -> Result<u32, OpfsSAHError> {
        for _ in 0..n {
            let name = get_random_name();
            let handle: FileSystemFileHandle =
                JsFuture::from(self.dh_opaque.get_file_handle_with_options(&name, &{
                    let options = FileSystemGetFileOptions::new();
                    options.set_create(true);
                    options
                }))
                .await
                .map_err(OpfsSAHError::GetFileHandle)?
                .into();
            let sah: FileSystemSyncAccessHandle =
                JsFuture::from(handle.create_sync_access_handle())
                    .await
                    .map_err(OpfsSAHError::CreateSyncAccessHandle)?
                    .into();
            self.map_sah_to_name.set(&sah, &JsValue::from(name));
            self.set_associated_path(&sah, "", 0)?;
        }
        Ok(self.get_capacity())
    }

    /// Reduce capacity by n, but can only reduce up to the limit
    /// of currently-available SAHs. Returns a Promise which resolves
    /// to the number of slots really removed.
    async fn reduce_capacity(&self, n: u32) -> Result<u32, OpfsSAHError> {
        let mut result = 0;
        for sah in Array::from(&self.available_sah) {
            if result == n || self.get_capacity() == self.get_file_count() {
                break;
            }
            let sah = FileSystemSyncAccessHandle::from(sah);
            let name = self.map_sah_to_name.get(&sah).as_string().unwrap();
            sah.close();
            JsFuture::from(self.dh_opaque.remove_entry(&name))
                .await
                .map_err(OpfsSAHError::RemoveEntity)?;
            self.map_sah_to_name.delete(&sah);
            self.available_sah.delete(&sah);
            result += 1;
        }
        Ok(result)
    }

    /// Current pool capacity.
    fn get_capacity(&self) -> u32 {
        self.map_sah_to_name.size()
    }

    /// Current number of in-use files from pool.
    fn get_file_count(&self) -> u32 {
        self.map_filename_to_sah.size()
    }

    /// Returns an array of the names of all
    /// currently-opened client-specified filenames.
    fn get_file_names(&self) -> Vec<String> {
        let mut result = vec![];
        for n in self.map_filename_to_sah.keys() {
            if let Ok(name) = n {
                result.push(name.as_string().unwrap());
            }
        }
        result
    }

    /// Given an SAH, returns the client-specified name of
    /// that file by extracting it from the SAH's header.
    /// On error, it disassociates SAH from the pool and
    /// returns an empty string.
    fn get_associated_path(
        &self,
        sah: &FileSystemSyncAccessHandle,
    ) -> Result<Option<String>, OpfsSAHError> {
        sah.read_with_buffer_source_and_options(&self.ap_body, &read_write_options(0.0))
            .map_err(OpfsSAHError::Read)?;
        let flags = self.dv_body.get_uint32(HEADER_OFFSET_FLAGS);
        if self.ap_body.get_index(0) != 0
            && ((flags & SQLITE_OPEN_DELETEONCLOSE as u32 != 0)
                || (flags & PERSISTENT_FILE_TYPES as u32) == 0)
        {
            self.set_associated_path(sah, "", 0)?;
            return Ok(None);
        }

        // size is 2
        let file_digest = Uint32Array::new_with_length(HEADER_DIGEST_SIZE as u32 / 4);
        sah.read_with_buffer_source_and_options(
            &file_digest,
            &read_write_options(HEADER_OFFSET_DIGEST as f64),
        )
        .map_err(OpfsSAHError::Read)?;

        let comp_digest = compute_digest(&self.ap_body);
        if Array::from(&file_digest)
            .every(&mut |v, i, _| v.as_f64().unwrap() as u32 == comp_digest.get_index(i))
        {
            let path_size = Array::from(&self.ap_body)
                .find_index(&mut |x, _, _| x.as_f64().unwrap() as u8 == 0)
                as u32;
            if path_size == 0 {
                sah.truncate_with_u32(HEADER_OFFSET_DATA as u32)
                    .map_err(OpfsSAHError::Truncate)?;
                return Ok(None);
            }
            let path_bytes = self.ap_body.subarray(0, path_size);
            let mut path = vec![0; path_size as usize];
            for idx in 0..path_size {
                // why not `copy_to`?
                //
                // see <https://github.com/rustwasm/wasm-bindgen/issues/4395>
                path[idx as usize] = path_bytes.get_index(idx);
            }
            // set_associated_path ensures that it is utf8
            let path = String::from_utf8(path).unwrap();
            Ok(Some(path))
        } else {
            self.set_associated_path(sah, "", 0)?;
            Ok(None)
        }
    }

    /// Stores the given client-defined path and SQLITE_OPEN_xyz flags
    /// into the given SAH. If path is an empty string then the file is
    /// disassociated from the pool but its previous name is preserved
    /// in the metadata.
    fn set_associated_path(
        &self,
        sah: &FileSystemSyncAccessHandle,
        path: &str,
        flags: i32,
    ) -> Result<(), OpfsSAHError> {
        if HEADER_MAX_PATH_SIZE < path.len() {
            return Err(OpfsSAHError::Custom(format!("Path too long: {path}")));
        }
        for (idx, byte) in path.bytes().enumerate() {
            // why not `copy_from`?
            //
            // see <https://github.com/rustwasm/wasm-bindgen/issues/4395>
            self.ap_body.set_index(idx as u32, byte);
        }

        self.ap_body
            .fill(0, path.len() as u32, HEADER_MAX_PATH_SIZE as u32);
        self.dv_body.set_uint32(HEADER_OFFSET_FLAGS, flags as u32);

        let digest = compute_digest(&self.ap_body);

        sah.write_with_js_u8_array_and_options(&self.ap_body, &read_write_options(0.0))
            .map_err(OpfsSAHError::Write)?;
        sah.write_with_buffer_source_and_options(
            &digest,
            &read_write_options(HEADER_OFFSET_DIGEST as f64),
        )
        .map_err(OpfsSAHError::Write)?;
        sah.flush().map_err(OpfsSAHError::Flush)?;

        if path.is_empty() {
            sah.truncate_with_u32(HEADER_OFFSET_DATA as u32)
                .map_err(OpfsSAHError::Truncate)?;
            self.available_sah.add(sah);
        } else {
            self.map_filename_to_sah.set(&JsValue::from(path), sah);
            self.available_sah.delete(sah);
        }

        Ok(())
    }

    /// Opens all files under self.dh_opaque and acquires
    /// a SAH for each. returns a Promise which resolves to no value
    /// but completes once all SAHs are acquired. If acquiring an SAH
    /// throws, SAHPool.$error will contain the corresponding
    /// exception.
    ///
    /// If clearFiles is true, the client-stored state of each file is
    /// cleared when its handle is acquired, including its name, flags,
    /// and any data stored after the metadata block.
    async fn acquire_access_handles(&self, clear_files: bool) -> Result<(), OpfsSAHError> {
        let mut files = vec![];
        let iter = self.dh_opaque.entries();
        while let Ok(future) = iter.next() {
            let next: IteratorNext = JsFuture::from(future)
                .await
                .map_err(OpfsSAHError::IterHandle)?
                .into();
            if next.done() {
                break;
            }
            let array: Array = next.value().into();
            let key = array.get(0);
            let value = array.get(1);
            let kind = Reflect::get(&value, &JsValue::from("kind"))
                .unwrap()
                .as_string()
                .unwrap();
            if kind == "file" {
                files.push((key, FileSystemFileHandle::from(value)));
            }
        }

        let fut = async {
            for (file, handle) in files {
                let sah = JsFuture::from(handle.create_sync_access_handle())
                    .await
                    .map_err(OpfsSAHError::CreateSyncAccessHandle)?;
                self.map_sah_to_name.set(&sah, &file);
                let sah = FileSystemSyncAccessHandle::from(sah);
                if clear_files {
                    sah.truncate_with_u32(HEADER_OFFSET_DATA as u32)
                        .map_err(OpfsSAHError::Truncate)?;
                    self.set_associated_path(&sah, "", 0)?;
                } else {
                    if let Some(path) = self.get_associated_path(&sah)? {
                        self.map_filename_to_sah.set(&JsValue::from(path), &sah);
                    } else {
                        self.available_sah.add(&sah);
                    }
                }
            }
            Ok::<_, OpfsSAHError>(())
        };

        if let Err(e) = fut.await {
            self.store_err(Some(&e), None);
            self.release_access_handles();
            return Err(e);
        }

        Ok(())
    }

    /// Releases all currently-opened SAHs. The only legal
    /// operation after this is acquireAccessHandles().
    fn release_access_handles(&self) {
        for sah in self.map_sah_to_name.keys() {
            if let Ok(sah) = sah {
                let sah = FileSystemSyncAccessHandle::from(sah);
                sah.close();
            }
        }
        self.map_sah_to_name.clear();
        self.map_filename_to_sah.clear();
        self.available_sah.clear();
    }

    /// Pops this object's Error object and returns
    /// it (a falsy value if no error is set).
    fn pop_err(&self) -> Option<(i32, String)> {
        self.last_error.lock().unwrap().take()
    }

    /// Sets e (an Error object) as this object's current error. Pass a
    /// falsy (or no) value to clear it. If code is truthy it is
    /// assumed to be an SQLITE_xxx result code, defaulting to
    /// SQLITE_IOERR if code is falsy.
    fn store_err(&self, err: Option<&OpfsSAHError>, code: Option<i32>) -> i32 {
        if err.is_none() {
            self.last_error.lock().unwrap().take();
        }
        let code = code.unwrap_or(SQLITE_IOERR);
        self.last_error
            .lock()
            .unwrap()
            .replace((code, format!("{:?}", err)));
        code
    }

    /// Given an (sqlite3_file*), returns the mapped
    /// xOpen file object.
    fn get_o_file_for_s3_file(&self, p_file: *mut sqlite3_file) -> Result<Object, OpfsSAHError> {
        let file = self.map_s3_file_to_o_file.get(&JsValue::from(p_file));
        if file.is_undefined() {
            return Err(OpfsSAHError::Custom("open file not exists".into()));
        }
        Ok(file.into())
    }

    /// Maps or unmaps (if file is falsy) the given (sqlite3_file*)
    /// to an xOpen file object and to this pool object.
    fn map_s3_file_to_o_file(&self, p_file: *mut sqlite3_file, file: Option<Object>) {
        if let Some(file) = file {
            self.map_s3_file_to_o_file
                .set(&JsValue::from(p_file), &JsValue::from(file));
        } else {
            self.map_s3_file_to_o_file.delete(&JsValue::from(p_file));
        }
    }

    /// Removes the association of the given client-specified file
    /// name (JS string) from the pool. Returns true if a mapping
    /// is found, else false.
    fn delete_path(&self, path: &str) -> Result<bool, OpfsSAHError> {
        let sah = self.map_filename_to_sah.get(&JsValue::from(path));
        let found = !sah.is_undefined();
        if found {
            let sah: FileSystemSyncAccessHandle = sah.into();
            self.map_filename_to_sah.delete(&JsValue::from(path));
            self.set_associated_path(&sah, "", 0)?;
        }
        Ok(found)
    }

    /// All "../" parts and duplicate slashes are resolve/removed from
    /// the returned result.
    fn get_path(&self, name: *const ::std::os::raw::c_char) -> Result<String, OpfsSAHError> {
        if name.is_null() {
            return Err(OpfsSAHError::Custom("name is null ptr".into()));
        }
        let name = unsafe {
            CStr::from_ptr(name)
                .to_str()
                .map_err(|e| OpfsSAHError::Custom(format!("{e:?}")))?
        };
        Url::new_with_base(name, "file://localhost/")
            .map(|x| x.pathname())
            .map_err(OpfsSAHError::GetPath)
    }

    /// Returns true if the given client-defined file name is in this
    /// object's name-to-SAH map.
    fn has_filename(&self, name: &str) -> bool {
        self.map_filename_to_sah.has(&JsValue::from(name))
    }

    /// Returns the SAH associated with the given
    /// client-defined file name.
    fn get_sah_for_path(&self, path: &str) -> Option<FileSystemSyncAccessHandle> {
        self.has_filename(path)
            .then(|| self.map_filename_to_sah.get(&JsValue::from(path)).into())
    }

    /// Returns the next available SAH without removing
    /// it from the set.
    fn next_available_sah(&self) -> Option<FileSystemSyncAccessHandle> {
        self.available_sah
            .keys()
            .next()
            .ok()
            .filter(|x| !x.done())
            .map(|x| x.value().into())
    }

    fn export_file(&self, name: &str) -> Result<Vec<u8>, OpfsSAHError> {
        let sah = self.map_filename_to_sah.get(&JsValue::from(name));
        if sah.is_undefined() {
            return Err(OpfsSAHError::Custom("File not found:".into()));
        }
        let sah = FileSystemSyncAccessHandle::from(sah);
        let n = sah.get_size().map_err(OpfsSAHError::GetSize)? - HEADER_OFFSET_DATA as f64;
        let n = n.max(0.0) as usize;
        let mut data = vec![0; n];
        if n > 0 {
            let read = sah
                .read_with_u8_array_and_options(
                    &mut data,
                    &read_write_options(HEADER_OFFSET_DATA as f64),
                )
                .map_err(OpfsSAHError::Read)?;
            if read != n as f64 {
                return Err(OpfsSAHError::Custom(format!(
                    "Expected to read {} bytes but read {}.",
                    n, read
                )));
            }
        }
        Ok(data)
    }

    fn import_db(&self, path: &str, bytes: &[u8]) -> Result<(), OpfsSAHError> {
        const HEADER: &str = "SQLite format 3";

        let sah = self.map_filename_to_sah.get(&JsValue::from(path));
        let sah = if sah.is_undefined() {
            self.next_available_sah()
                .ok_or_else(|| OpfsSAHError::Custom("No available handles to import to.".into()))?
        } else {
            FileSystemSyncAccessHandle::from(sah)
        };
        let length = bytes.len();
        if length < 512 && length % 512 != 0 {
            return Err(OpfsSAHError::Custom(
                "Byte array size is invalid for an SQLite db.".into(),
            ));
        }
        if HEADER.as_bytes().iter().zip(bytes).any(|(x, y)| x != y) {
            return Err(OpfsSAHError::Custom(
                "Input does not contain an SQLite database header.".into(),
            ));
        }
        let write = sah
            .write_with_u8_array_and_options(bytes, &read_write_options(HEADER_OFFSET_DATA as f64))
            .map_err(OpfsSAHError::Write)?;
        if write != length as f64 {
            self.set_associated_path(&sah, "", 0)?;
            return Err(OpfsSAHError::Custom(format!(
                "Expected to write {} bytes but wrote {}.",
                length, write
            )));
        }

        let bytes = [1, 1];
        sah.write_with_u8_array_and_options(
            &bytes,
            &read_write_options((HEADER_OFFSET_DATA + 18) as f64),
        )
        .map_err(OpfsSAHError::Write)?;
        self.set_associated_path(&sah, path, SQLITE_OPEN_MAIN_DB)?;

        Ok(())
    }
}

/// A OpfsSAHPoolUtil instance is exposed to clients in order to
/// manipulate an OpfsSAHPool object without directly exposing that
/// object and allowing for some semantic changes compared to that
/// class.
pub struct OpfsSAHPoolUtil {
    pool: &'static FragileComfirmed<OpfsSAHPool>,
}

impl OpfsSAHPoolUtil {
    /// Adds n entries to the current pool.
    pub async fn add_capacity(&self, n: u32) -> Result<u32, OpfsSAHError> {
        self.pool.add_capacity(n).await
    }

    /// Removes up to n entries from the pool, with the caveat that
    /// it can only remove currently-unused entries.
    pub async fn reduce_capacity(&self, n: u32) -> Result<u32, OpfsSAHError> {
        self.pool.reduce_capacity(n).await
    }

    /// Returns the number of files currently contained in the SAH pool.
    pub fn get_capacity(&self) -> u32 {
        self.pool.get_capacity()
    }

    /// Returns the number of files from the pool currently allocated to VFS slots.
    pub fn get_file_count(&self) -> u32 {
        self.pool.get_file_count()
    }

    /// Returns an array of the names of the files currently allocated to VFS slots.
    pub fn get_file_names(&self) -> Vec<String> {
        self.pool.get_file_names()
    }

    /// Removes up to n entries from the pool, with the caveat that it can only
    /// remove currently-unused entries.
    pub async fn reserve_minimum_capacity(&self, min: u32) -> Result<(), OpfsSAHError> {
        let now = self.pool.get_capacity();
        if min > now {
            self.pool.add_capacity(min - now).await?;
        }
        Ok(())
    }

    /// If a virtual file exists with the given name, disassociates it
    /// from the pool and returns true, else returns false without side effects.
    pub fn unlink(&self, name: &str) -> Result<bool, OpfsSAHError> {
        self.pool.delete_path(name)
    }

    /// Synchronously reads the contents of the given file into a Uint8Array and returns it.
    pub fn export_file(&self, name: &str) -> Result<Vec<u8>, OpfsSAHError> {
        self.pool.export_file(name)
    }

    /// Imports the contents of an SQLite database, provided as a byte array or ArrayBuffer,
    /// under the given name, overwriting any existing content.
    ///
    /// path must start with '/'
    pub fn import_db(&self, path: &str, bytes: &[u8]) -> Result<(), OpfsSAHError> {
        if !path.starts_with('/') {
            return Err(OpfsSAHError::Custom("path must start with '/'".into()));
        }
        self.pool.import_db(path, bytes)
    }

    /// Clears all client-defined state of all SAHs and makes all of them available
    /// for re-use by the pool.
    pub async fn wipe_files(&self) -> Result<(), OpfsSAHError> {
        self.pool.release_access_handles();
        self.pool.acquire_access_handles(true).await?;
        Ok(())
    }
}

static POOL: tokio::sync::OnceCell<FragileComfirmed<OpfsSAHPool>> =
    tokio::sync::OnceCell::const_new();

fn pool() -> &'static FragileComfirmed<OpfsSAHPool> {
    POOL.get().expect("init pool first")
}

fn read_write_options(at: f64) -> FileSystemReadWriteOptions {
    let options = FileSystemReadWriteOptions::new();
    options.set_at(at);
    options
}

// (path, flags, sah)
fn get_file_fields(obj: &Object) -> (String, i32, FileSystemSyncAccessHandle) {
    let path: String = Reflect::get(&obj, &JsValue::from("path"))
        .unwrap()
        .as_string()
        .unwrap();
    let flags: i32 = Reflect::get(&obj, &JsValue::from("flags"))
        .unwrap()
        .as_f64()
        .unwrap() as i32;
    let sah: FileSystemSyncAccessHandle = Reflect::get(&obj, &JsValue::from("sah")).unwrap().into();
    (path, flags, sah)
}

fn io_methods() -> sqlite3_io_methods {
    unsafe extern "C" fn xCheckReservedLock(
        _arg1: *mut sqlite3_file,
        pResOut: *mut ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        let pool = pool();
        pool.store_err(None, None);
        *pResOut = 1;
        0
    }

    unsafe extern "C" fn xClose(arg1: *mut sqlite3_file) -> ::std::os::raw::c_int {
        let pool = pool();
        let f = || {
            if let Ok(file) = pool.get_o_file_for_s3_file(arg1) {
                pool.map_s3_file_to_o_file(arg1, None);
                let (path, flags, sah) = get_file_fields(&file);
                sah.flush().map_err(OpfsSAHError::Flush)?;
                if (flags & SQLITE_OPEN_DELETEONCLOSE) != 0 {
                    pool.delete_path(&path)?;
                }
            }
            Ok::<_, OpfsSAHError>(())
        };
        if let Err(e) = f() {
            return pool.store_err(Some(&e), Some(SQLITE_IOERR));
        }
        0
    }

    unsafe extern "C" fn xDeviceCharacteristics(_arg1: *mut sqlite3_file) -> ::std::os::raw::c_int {
        SQLITE_IOCAP_UNDELETABLE_WHEN_OPEN
    }

    unsafe extern "C" fn xFileControl(
        _arg1: *mut sqlite3_file,
        _op: ::std::os::raw::c_int,
        _pArg: *mut ::std::os::raw::c_void,
    ) -> ::std::os::raw::c_int {
        SQLITE_NOTFOUND
    }

    unsafe extern "C" fn xFileSize(
        arg1: *mut sqlite3_file,
        pSize: *mut sqlite3_int64,
    ) -> ::std::os::raw::c_int {
        let pool = pool();
        if let Ok(file) = pool.get_o_file_for_s3_file(arg1) {
            let (_, _, sah) = get_file_fields(&file);
            let size = sah.get_size().unwrap() as i64 - HEADER_OFFSET_DATA as i64;
            *pSize = size;
        }
        0
    }

    unsafe extern "C" fn xLock(
        arg1: *mut sqlite3_file,
        arg2: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        let pool = pool();
        pool.store_err(None, None);
        if let Ok(file) = pool.get_o_file_for_s3_file(arg1) {
            // seems unused
            Reflect::set(&file, &JsValue::from("lockType"), &JsValue::from(arg2)).unwrap();
        }
        0
    }

    unsafe extern "C" fn xRead(
        arg1: *mut sqlite3_file,
        arg2: *mut ::std::os::raw::c_void,
        iAmt: ::std::os::raw::c_int,
        iOfst: sqlite3_int64,
    ) -> ::std::os::raw::c_int {
        let pool = pool();
        pool.store_err(None, None);
        let f = || {
            let file = pool.get_o_file_for_s3_file(arg1)?;
            let (_, _, sah) = get_file_fields(&file);
            let slice = std::slice::from_raw_parts_mut(arg2.cast::<u8>(), iAmt as usize);

            let n_read = sah
                .read_with_u8_array_and_options(
                    slice,
                    &read_write_options((HEADER_OFFSET_DATA as i64 + iOfst) as f64),
                )
                .map_err(OpfsSAHError::Read)?;

            if (n_read as i32) < iAmt {
                slice[n_read as usize..iAmt as usize].fill(0);
                return Ok(SQLITE_IOERR_SHORT_READ);
            }

            Ok::<i32, OpfsSAHError>(0)
        };
        match f() {
            Ok(ret) => ret,
            Err(e) => pool.store_err(Some(&e), Some(SQLITE_IOERR)),
        }
    }

    unsafe extern "C" fn xSectorSize(_arg1: *mut sqlite3_file) -> ::std::os::raw::c_int {
        SECTOR_SIZE as i32
    }

    unsafe extern "C" fn xSync(
        arg1: *mut sqlite3_file,
        _flags: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        let pool = pool();
        pool.store_err(None, None);

        let get_sah = pool
            .get_o_file_for_s3_file(arg1)
            .map(|obj| get_file_fields(&obj).2);

        if let Err(e) = get_sah.and_then(|sah| sah.flush().map_err(OpfsSAHError::Flush)) {
            return pool.store_err(Some(&e), Some(SQLITE_IOERR));
        }

        0
    }

    unsafe extern "C" fn xTruncate(
        arg1: *mut sqlite3_file,
        size: sqlite3_int64,
    ) -> ::std::os::raw::c_int {
        let pool = pool();
        pool.store_err(None, None);

        let get_sah = pool
            .get_o_file_for_s3_file(arg1)
            .map(|obj| get_file_fields(&obj).2);

        if let Err(e) = get_sah.and_then(|sah| {
            sah.truncate_with_f64((HEADER_OFFSET_DATA as i64 + size) as f64)
                .map_err(OpfsSAHError::Truncate)
        }) {
            return pool.store_err(Some(&e), Some(SQLITE_IOERR));
        }

        0
    }

    unsafe extern "C" fn xUnlock(
        arg1: *mut sqlite3_file,
        arg2: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        let pool = pool();
        if let Ok(file) = pool.get_o_file_for_s3_file(arg1) {
            // seems unused
            Reflect::set(&file, &JsValue::from("lockType"), &JsValue::from(arg2)).unwrap();
        }
        0
    }

    unsafe extern "C" fn xWrite(
        arg1: *mut sqlite3_file,
        arg2: *const ::std::os::raw::c_void,
        iAmt: ::std::os::raw::c_int,
        iOfst: sqlite3_int64,
    ) -> ::std::os::raw::c_int {
        let pool = pool();
        pool.store_err(None, None);

        let f = || {
            let file = pool.get_o_file_for_s3_file(arg1)?;
            let (_, _, sah) = get_file_fields(&file);
            let slice = std::slice::from_raw_parts(arg2.cast::<u8>(), iAmt as usize);

            let n_write = sah
                .write_with_u8_array_and_options(
                    slice,
                    &read_write_options((HEADER_OFFSET_DATA as i64 + iOfst) as f64),
                )
                .map_err(OpfsSAHError::Read)?;

            let ret = if iAmt == n_write as i32 {
                0
            } else {
                SQLITE_ERROR
            };

            Ok::<i32, OpfsSAHError>(ret)
        };
        match f() {
            Ok(ret) => ret,
            Err(e) => pool.store_err(Some(&e), Some(SQLITE_IOERR)),
        }
    }

    sqlite3_io_methods {
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
    }
}

fn vfs() -> sqlite3_vfs {
    unsafe extern "C" fn xAccess(
        _arg1: *mut sqlite3_vfs,
        zName: *const ::std::os::raw::c_char,
        _flags: ::std::os::raw::c_int,
        pResOut: *mut ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        let pool = pool();
        pool.store_err(None, None);
        *pResOut = match pool.get_path(zName) {
            Ok(s) => i32::from(pool.has_filename(&s)),
            Err(_) => 0,
        };
        0
    }

    unsafe extern "C" fn xDelete(
        _arg1: *mut sqlite3_vfs,
        zName: *const ::std::os::raw::c_char,
        _syncDir: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        let pool = pool();
        pool.store_err(None, None);

        if let Err(e) = pool.get_path(zName).map(|name| pool.delete_path(&name)) {
            return pool.store_err(Some(&e), Some(SQLITE_IOERR_DELETE));
        }
        0
    }

    unsafe extern "C" fn xFullPathname(
        _arg1: *mut sqlite3_vfs,
        zName: *const ::std::os::raw::c_char,
        nOut: ::std::os::raw::c_int,
        zOut: *mut ::std::os::raw::c_char,
    ) -> ::std::os::raw::c_int {
        zName.copy_to(zOut, nOut as usize);
        0
    }

    unsafe extern "C" fn xGetLastError(
        _arg1: *mut sqlite3_vfs,
        arg2: ::std::os::raw::c_int,
        arg3: *mut ::std::os::raw::c_char,
    ) -> ::std::os::raw::c_int {
        let pool = pool();
        if let Some((_, msg)) = pool.pop_err() {
            let cstr = CString::new(msg).unwrap();
            cstr.as_ptr().copy_to(arg3, arg2 as usize);
            std::ptr::write(arg3.add(arg2 as usize - 1), 0);
        }
        0
    }

    unsafe extern "C" fn xOpen(
        _arg1: *mut sqlite3_vfs,
        zName: sqlite3_filename,
        arg2: *mut sqlite3_file,
        flags: ::std::os::raw::c_int,
        pOutFlags: *mut ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        let pool = pool();

        let f = || {
            let name = pool.get_path(zName)?;
            let sah = match pool.get_sah_for_path(&name) {
                Some(sah) => sah,
                None => {
                    if flags & SQLITE_OPEN_CREATE == 0 {
                        return Ok(SQLITE_ERROR);
                    }
                    if let Some(sah) = pool.next_available_sah() {
                        pool.set_associated_path(&sah, &name, flags)?;
                        sah
                    } else {
                        return Ok(SQLITE_ERROR);
                    }
                }
            };
            let file = Object::new();
            Reflect::set(&file, &JsValue::from("path"), &JsValue::from(name)).unwrap();
            Reflect::set(&file, &JsValue::from("flags"), &JsValue::from(flags)).unwrap();
            Reflect::set(&file, &JsValue::from("sah"), &JsValue::from(sah)).unwrap();
            Reflect::set(
                &file,
                &JsValue::from("lockType"),
                &JsValue::from(SQLITE_LOCK_NONE),
            )
            .unwrap();
            pool.map_s3_file_to_o_file(arg2, Some(file));

            let io_method = Box::leak(Box::new(io_methods()));
            (*arg2).pMethods = io_method;
            *pOutFlags = flags;

            Ok::<i32, OpfsSAHError>(0)
        };
        match f() {
            Ok(ret) => ret,
            Err(e) => pool.store_err(Some(&e), Some(SQLITE_CANTOPEN)),
        }
    }

    let default_vfs = unsafe { sqlite3_vfs_find(std::ptr::null()) };
    let xRandomness = unsafe { (*default_vfs).xRandomness };
    let xSleep = unsafe { (*default_vfs).xSleep };
    let xCurrentTime = unsafe { (*default_vfs).xCurrentTime };
    let xCurrentTimeInt64 = unsafe { (*default_vfs).xCurrentTimeInt64 };

    sqlite3_vfs {
        iVersion: 1,
        szOsFile: std::mem::size_of::<sqlite3_file>() as i32,
        mxPathname: HEADER_MAX_PATH_SIZE as i32,
        pNext: std::ptr::null_mut(),
        zName: "opfs-sahpool\0".as_ptr().cast(),
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

/// Register `opfs-sahpool` vfs and return a utility object which can be used
/// to perform basic administration of the file pool
pub async fn install_opfs_sahpool(
    options: Option<&OpfsSAHPoolCfg>,
) -> Result<OpfsSAHPoolUtil, OpfsSAHError> {
    let pool = POOL
        .get_or_try_init(|| async {
            let pool = OpfsSAHPool::new(options).await?;
            Ok(FragileComfirmed::new(pool))
        })
        .await?;

    let ret = unsafe { sqlite3_vfs_register(Box::leak(Box::new(vfs())), 1) };
    if ret != SQLITE_OK {
        return Err(OpfsSAHError::Custom(
            "register opfs-sahpool vfs failed".into(),
        ));
    }

    let util = OpfsSAHPoolUtil { pool };

    Ok(util)
}
