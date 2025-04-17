//! opfs-sahpool vfs implementation, ported from sqlite-wasm
//!
//! No large-scale reconstruction is required to facilitate future maintenance.
//!
//! <https://github.com/sqlite/sqlite/blob/master/ext/wasm/api/sqlite3-vfs-opfs-sahpool.c-pp.js>

use crate::libsqlite3::*;
use crate::vfs::utils::{
    copy_to_uint8_array_subarray, copy_to_vec, get_random_name, register_vfs, FragileComfirmed,
    RegisterVfsError, SQLiteIoMethods, SQLiteVfs, VfsAppData, VfsError, VfsFile, VfsResult,
    VfsStore, SQLITE3_HEADER,
};

use js_sys::{Array, DataView, IteratorNext, Map, Reflect, Set, Uint32Array, Uint8Array};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetDirectoryOptions,
    FileSystemGetFileOptions, FileSystemReadWriteOptions, FileSystemSyncAccessHandle, Url,
    WorkerGlobalScope,
};

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

type Result<T> = std::result::Result<T, OpfsSAHError>;

fn read_write_options(at: f64) -> FileSystemReadWriteOptions {
    let options = FileSystemReadWriteOptions::new();
    options.set_at(at);
    options
}

// this function only return [0, 0] for now
//
// https://github.com/sqlite/sqlite-wasm/issues/97
fn compute_digest(_byte_array: &Uint8Array) -> Uint32Array {
    Uint32Array::new_with_length(2)
}

/// Class for managing OPFS-related state for the OPFS
/// SharedAccessHandle Pool sqlite3_vfs.
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
}

impl OpfsSAHPool {
    async fn new(options: &OpfsSAHPoolCfg) -> Result<OpfsSAHPool> {
        const OPAQUE_DIR_NAME: &str = ".opaque";

        let vfs_dir = &options.directory;
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
    async fn add_capacity(&self, n: u32) -> Result<u32> {
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
    async fn reduce_capacity(&self, n: u32) -> Result<u32> {
        let mut result = 0;
        for sah in Array::from(&self.available_sah) {
            if result == n || self.get_capacity() == self.get_file_count() {
                break;
            }
            let sah = FileSystemSyncAccessHandle::from(sah);

            let name = self.map_sah_to_name.get(&sah);
            assert!(!name.is_undefined(), "name must exists");
            let name = name.as_string().unwrap();

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
        for name in self.map_filename_to_sah.keys().into_iter().flatten() {
            result.push(name.as_string().unwrap());
        }
        result
    }

    /// Given an SAH, returns the client-specified name of
    /// that file by extracting it from the SAH's header.
    /// On error, it disassociates SAH from the pool and
    /// returns an empty string.
    fn get_associated_path(&self, sah: &FileSystemSyncAccessHandle) -> Result<Option<String>> {
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
            let vec = copy_to_vec(&path_bytes);
            // set_associated_path ensures that it is utf8
            let path = String::from_utf8(vec).unwrap();
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
    ) -> Result<()> {
        if HEADER_MAX_PATH_SIZE < path.len() {
            return Err(OpfsSAHError::Generic(format!("Path too long: {path}")));
        }
        copy_to_uint8_array_subarray(
            path.as_bytes(),
            &self.ap_body.subarray(0, path.len() as u32),
        );

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
    async fn acquire_access_handles(&self, clear_files: bool) -> Result<()> {
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
                .map_err(OpfsSAHError::Reflect)?
                .as_string();
            if kind.as_deref() == Some("file") {
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
                } else if let Some(path) = self.get_associated_path(&sah)? {
                    self.map_filename_to_sah.set(&JsValue::from(path), &sah);
                } else {
                    self.available_sah.add(&sah);
                }
            }
            Ok::<_, OpfsSAHError>(())
        };

        if let Err(e) = fut.await {
            self.release_access_handles();
            return Err(e);
        }

        Ok(())
    }

    /// Releases all currently-opened SAHs. The only legal
    /// operation after this is acquireAccessHandles().
    fn release_access_handles(&self) {
        for sah in self.map_sah_to_name.keys().into_iter().flatten() {
            let sah = FileSystemSyncAccessHandle::from(sah);
            sah.close();
        }
        self.map_sah_to_name.clear();
        self.map_filename_to_sah.clear();
        self.available_sah.clear();
    }

    /// Removes the association of the given client-specified file
    /// name (JS string) from the pool. Returns true if a mapping
    /// is found, else false.
    fn delete_path(&self, path: &str) -> Result<bool> {
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
    fn get_path(&self, name: &str) -> Result<String> {
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

    fn export_file(&self, name: &str) -> Result<Vec<u8>> {
        let sah = self.map_filename_to_sah.get(&JsValue::from(name));
        if sah.is_undefined() {
            return Err(OpfsSAHError::Generic(format!("File not found: {name}")));
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
                return Err(OpfsSAHError::Generic(format!(
                    "Expected to read {} bytes but read {}.",
                    n, read
                )));
            }
        }
        Ok(data)
    }

    fn import_db(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let sah = self.map_filename_to_sah.get(&JsValue::from(path));
        let sah = if sah.is_undefined() {
            self.next_available_sah()
                .ok_or_else(|| OpfsSAHError::Generic("No available handles to import to.".into()))?
        } else {
            FileSystemSyncAccessHandle::from(sah)
        };
        let length = bytes.len();
        if length < 512 && length % 512 != 0 {
            return Err(OpfsSAHError::Generic(
                "Byte array size is invalid for an SQLite db.".into(),
            ));
        }
        if SQLITE3_HEADER
            .as_bytes()
            .iter()
            .zip(bytes)
            .any(|(x, y)| x != y)
        {
            return Err(OpfsSAHError::Generic(
                "Input does not contain an SQLite database header.".into(),
            ));
        }
        let write = sah
            .write_with_u8_array_and_options(bytes, &read_write_options(HEADER_OFFSET_DATA as f64))
            .map_err(OpfsSAHError::Write)?;
        if write != length as f64 {
            self.set_associated_path(&sah, "", 0)?;
            return Err(OpfsSAHError::Generic(format!(
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

impl VfsFile for FileSystemSyncAccessHandle {
    fn read(&self, buf: &mut [u8], offset: usize) -> VfsResult<i32> {
        let n_read = self
            .read_with_u8_array_and_options(
                buf,
                &read_write_options((HEADER_OFFSET_DATA + offset) as f64),
            )
            .map_err(OpfsSAHError::Read)
            .map_err(|err| err.vfs_err(SQLITE_IOERR))?;

        if (n_read as usize) < buf.len() {
            buf[n_read as usize..].fill(0);
            return Ok(SQLITE_IOERR_SHORT_READ);
        }

        Ok(SQLITE_OK)
    }

    fn write(&mut self, buf: &[u8], offset: usize) -> VfsResult<()> {
        let n_write = self
            .write_with_u8_array_and_options(
                buf,
                &read_write_options((HEADER_OFFSET_DATA + offset) as f64),
            )
            .map_err(OpfsSAHError::Read)
            .map_err(|err| err.vfs_err(SQLITE_IOERR))?;

        if buf.len() != n_write as usize {
            return Err(VfsError::new(SQLITE_ERROR, "failed to write file".into()));
        }

        Ok(())
    }

    fn truncate(&mut self, size: usize) -> VfsResult<()> {
        self.truncate_with_f64((HEADER_OFFSET_DATA + size) as f64)
            .map_err(OpfsSAHError::Truncate)
            .map_err(|err| err.vfs_err(SQLITE_IOERR))
    }

    fn flush(&mut self) -> VfsResult<()> {
        FileSystemSyncAccessHandle::flush(self)
            .map_err(OpfsSAHError::Flush)
            .map_err(|err| err.vfs_err(SQLITE_IOERR))
    }

    fn size(&self) -> VfsResult<usize> {
        Ok(self
            .get_size()
            .map_err(OpfsSAHError::GetSize)
            .map_err(|err| err.vfs_err(SQLITE_IOERR))? as usize
            - HEADER_OFFSET_DATA)
    }
}

type SyncAccessHandleAppData = FragileComfirmed<OpfsSAHPool>;

struct SyncAccessHandleStore;

impl VfsStore<FileSystemSyncAccessHandle, SyncAccessHandleAppData> for SyncAccessHandleStore {
    fn name2path(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<String> {
        let pool = unsafe { Self::app_data(vfs) };
        let file = pool
            .get_path(file)
            .map_err(|err| err.vfs_err(SQLITE_CANTOPEN))?;
        Ok(file)
    }

    fn add_file(vfs: *mut sqlite3_vfs, file: &str, flags: i32) -> VfsResult<()> {
        let pool = unsafe { Self::app_data(vfs) };

        if let Some(sah) = pool.next_available_sah() {
            pool.set_associated_path(&sah, file, flags)
                .map_err(|err| err.vfs_err(SQLITE_CANTOPEN))?;
        } else {
            return Err(VfsError::new(
                SQLITE_CANTOPEN,
                "SAH pool is full. Cannot create file".into(),
            ));
        };

        Ok(())
    }

    fn contains_file(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<bool> {
        let pool = unsafe { Self::app_data(vfs) };
        Ok(pool.has_filename(file))
    }

    fn delete_file(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<()> {
        let pool = unsafe { Self::app_data(vfs) };
        if let Err(err) = pool.get_path(file).map(|file| pool.delete_path(&file)) {
            return Err(err.vfs_err(SQLITE_IOERR_DELETE));
        }
        Ok(())
    }

    fn with_file<F: Fn(&FileSystemSyncAccessHandle) -> i32>(
        vfs_file: &super::utils::SQLiteVfsFile,
        f: F,
    ) -> VfsResult<i32> {
        let name = unsafe { vfs_file.name() };
        let pool = unsafe { Self::app_data(vfs_file.vfs) };
        match pool.get_sah_for_path(name) {
            Some(file) => Ok(f(&file)),
            None => Err(VfsError::new(SQLITE_IOERR, format!("{name} not found"))),
        }
    }

    fn with_file_mut<F: Fn(&mut FileSystemSyncAccessHandle) -> i32>(
        vfs_file: &super::utils::SQLiteVfsFile,
        f: F,
    ) -> VfsResult<i32> {
        let name = unsafe { vfs_file.name() };
        let pool = unsafe { Self::app_data(vfs_file.vfs) };
        match pool.get_sah_for_path(name) {
            Some(mut file) => Ok(f(&mut file)),
            None => Err(VfsError::new(SQLITE_IOERR, format!("{name} not found"))),
        }
    }
}

struct SyncAccessHandleIoMethods;

impl SQLiteIoMethods for SyncAccessHandleIoMethods {
    type File = FileSystemSyncAccessHandle;
    type AppData = SyncAccessHandleAppData;
    type Store = SyncAccessHandleStore;

    const VERSION: ::std::os::raw::c_int = 1;

    unsafe extern "C" fn xCheckReservedLock(
        _pFile: *mut sqlite3_file,
        pResOut: *mut ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        *pResOut = 1;
        SQLITE_OK
    }

    unsafe extern "C" fn xDeviceCharacteristics(
        _pFile: *mut sqlite3_file,
    ) -> ::std::os::raw::c_int {
        SQLITE_IOCAP_UNDELETABLE_WHEN_OPEN
    }
}

struct SyncAccessHandleVfs;

impl SQLiteVfs<SyncAccessHandleIoMethods> for SyncAccessHandleVfs {
    const VERSION: ::std::os::raw::c_int = 2;
    const MAX_PATH_SIZE: ::std::os::raw::c_int = HEADER_MAX_PATH_SIZE as _;
}

/// Build `OpfsSAHPoolCfg`
pub struct OpfsSAHPoolCfgBuilder(OpfsSAHPoolCfg);

impl OpfsSAHPoolCfgBuilder {
    pub fn new() -> Self {
        Self(OpfsSAHPoolCfg::default())
    }

    /// The SQLite VFS name under which this pool's VFS is registered.
    pub fn vfs_name(mut self, name: &str) -> Self {
        self.0.vfs_name = name.into();
        self
    }

    /// Specifies the OPFS directory name in which to store metadata for the `vfs_name`
    pub fn directory(mut self, directory: &str) -> Self {
        self.0.directory = directory.into();
        self
    }

    /// If truthy, contents and filename mapping are removed from each SAH
    /// as it is acquired during initalization of the VFS, leaving the VFS's
    /// storage in a pristine state. Use this only for databases which need not
    /// survive a page reload.
    pub fn clear_on_init(mut self, set: bool) -> Self {
        self.0.clear_on_init = set;
        self
    }

    /// Specifies the default capacity of the VFS, i.e. the number of files
    /// it may contain.
    pub fn initial_capacity(mut self, cap: u32) -> Self {
        self.0.initial_capacity = cap;
        self
    }

    /// Build OpfsSAHPoolCfg
    pub fn build(self) -> OpfsSAHPoolCfg {
        self.0
    }
}

impl Default for OpfsSAHPoolCfgBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// `OpfsSAHPool` options
pub struct OpfsSAHPoolCfg {
    /// The SQLite VFS name under which this pool's VFS is registered.
    pub vfs_name: String,
    /// Specifies the OPFS directory name in which to store metadata for the `vfs_name`
    pub directory: String,
    /// If truthy, contents and filename mapping are removed from each SAH
    /// as it is acquired during initalization of the VFS, leaving the VFS's
    /// storage in a pristine state. Use this only for databases which need not
    /// survive a page reload.
    pub clear_on_init: bool,
    /// Specifies the default capacity of the VFS, i.e. the number of files
    /// it may contain.
    pub initial_capacity: u32,
}

impl Default for OpfsSAHPoolCfg {
    fn default() -> Self {
        Self {
            vfs_name: "opfs-sahpool".into(),
            directory: ".opfs-sahpool".into(),
            clear_on_init: false,
            initial_capacity: 6,
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum OpfsSAHError {
    #[error(transparent)]
    Vfs(#[from] RegisterVfsError),
    #[error("This vfs is only available in dedicated worker")]
    NotSuported,
    #[error("An error occurred while getting the directory handle")]
    GetDirHandle(JsValue),
    #[error("An error occurred while getting the file handle")]
    GetFileHandle(JsValue),
    #[error("An error occurred while creating sync access handle")]
    CreateSyncAccessHandle(JsValue),
    #[error("An error occurred while iterating")]
    IterHandle(JsValue),
    #[error("An error occurred while getting path")]
    GetPath(JsValue),
    #[error("An error occurred while removing entity")]
    RemoveEntity(JsValue),
    #[error("An error occurred while getting size")]
    GetSize(JsValue),
    #[error("An error occurred while reading data")]
    Read(JsValue),
    #[error("An error occurred while writing data")]
    Write(JsValue),
    #[error("An error occurred while flushing data")]
    Flush(JsValue),
    #[error("An error occurred while truncating data")]
    Truncate(JsValue),
    #[error("An error occurred while getting data using reflect")]
    Reflect(JsValue),
    #[error("Generic error: {0}")]
    Generic(String),
    #[deprecated(note = "Has been renamed to OpfsSAHError::Generic")]
    #[error("custom error")]
    Custom(String),
}

impl OpfsSAHError {
    fn vfs_err(&self, code: i32) -> VfsError {
        VfsError::new(code, format!("{self:?}"))
    }
}

/// A OpfsSAHPoolUtil instance is exposed to clients in order to
/// manipulate an OpfsSAHPool object without directly exposing that
/// object and allowing for some semantic changes compared to that
/// class.
pub struct OpfsSAHPoolUtil {
    pool: &'static VfsAppData<SyncAccessHandleAppData>,
}

impl OpfsSAHPoolUtil {
    /// Adds n entries to the current pool.
    pub async fn add_capacity(&self, n: u32) -> Result<u32> {
        self.pool.add_capacity(n).await
    }

    /// Removes up to n entries from the pool, with the caveat that
    /// it can only remove currently-unused entries.
    pub async fn reduce_capacity(&self, n: u32) -> Result<u32> {
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
    pub async fn reserve_minimum_capacity(&self, min: u32) -> Result<()> {
        let now = self.pool.get_capacity();
        if min > now {
            self.pool.add_capacity(min - now).await?;
        }
        Ok(())
    }

    /// If a virtual file exists with the given name, disassociates it
    /// from the pool and returns true, else returns false without side effects.
    pub fn unlink(&self, name: &str) -> Result<bool> {
        self.pool.delete_path(name)
    }

    /// Synchronously reads the contents of the given file into a Uint8Array and returns it.
    pub fn export_file(&self, name: &str) -> Result<Vec<u8>> {
        self.pool.export_file(name)
    }

    /// Imports the contents of an SQLite database, provided as a byte array or ArrayBuffer,
    /// under the given name, overwriting any existing content.
    ///
    /// path must start with '/'
    pub fn import_db(&self, path: &str, bytes: &[u8]) -> Result<()> {
        if !path.starts_with('/') {
            return Err(OpfsSAHError::Generic("path must start with '/'".into()));
        }
        self.pool.import_db(path, bytes)
    }

    /// Clears all client-defined state of all SAHs and makes all of them available
    /// for re-use by the pool.
    pub async fn wipe_files(&self) -> Result<()> {
        self.pool.release_access_handles();
        self.pool.acquire_access_handles(true).await?;
        Ok(())
    }
}

/// Register `opfs-sahpool` vfs and return a utility object which can be used
/// to perform basic administration of the file pool
pub async fn install(
    options: Option<&OpfsSAHPoolCfg>,
    default_vfs: bool,
) -> Result<OpfsSAHPoolUtil> {
    static NAME2VFS: Lazy<
        tokio::sync::Mutex<HashMap<String, &'static VfsAppData<SyncAccessHandleAppData>>>,
    > = Lazy::new(|| tokio::sync::Mutex::new(HashMap::new()));

    let default_options = OpfsSAHPoolCfg::default();
    let options = options.unwrap_or(&default_options);
    let vfs_name = &options.vfs_name;

    let create_pool = async {
        let pool = OpfsSAHPool::new(options).await?;
        Ok::<_, OpfsSAHError>(FragileComfirmed::new(pool))
    };

    let mut name2vfs = NAME2VFS.lock().await;

    let pool = if let Some(pool) = name2vfs.get(vfs_name) {
        pool
    } else {
        let pool = create_pool.await?;
        let vfs = register_vfs::<SyncAccessHandleIoMethods, SyncAccessHandleVfs>(
            vfs_name,
            pool,
            default_vfs,
        )?;
        let pool = unsafe { SyncAccessHandleStore::app_data(vfs) };
        name2vfs.insert(vfs_name.clone(), pool);
        pool
    };

    let util = OpfsSAHPoolUtil { pool };

    Ok(util)
}
