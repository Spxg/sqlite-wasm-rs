//! opfs-sahpool vfs implementation, ported from sqlite-wasm.
//!
//! See [`opfs-sahpool`](https://sqlite.org/wasm/doc/trunk/persistence.md#vfs-opfs-sahpool) for details.
//!
//! ```rust
//! use sqlite_wasm_rs::{
//!     self as ffi,
//!     sahpool_vfs::{install as install_opfs_sahpool, OpfsSAHPoolCfg},
//! };
//!
//! async fn open_db() {
//!     // install opfs-sahpool persistent vfs and set as default vfs
//!     install_opfs_sahpool(&OpfsSAHPoolCfg::default(), true)
//!         .await
//!         .unwrap();
//!
//!     // open with opfs-sahpool vfs
//!     let mut db = std::ptr::null_mut();
//!     let ret = unsafe {
//!         ffi::sqlite3_open_v2(
//!             c"opfs-sahpool.db".as_ptr().cast(),
//!             &mut db as *mut _,
//!             ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
//!             std::ptr::null()
//!         )
//!     };
//!     assert_eq!(ffi::SQLITE_OK, ret);
//! }
//! ```
//!
//! The VFS is based on
//! [`FileSystemSyncAccessHandle`](https://developer.mozilla.org/en-US/docs/Web/API/FileSystemSyncAccessHandle)
//! read and write, and you can install the
//! [`opfs-explorer`](https://chromewebstore.google.com/detail/opfs-explorer/acndjpgkpaclldomagafnognkcgjignd)
//! plugin to browse files.

use crate::libsqlite3::*;
use crate::vfs::utils::{
    check_import_db, random_name, register_vfs, registered_vfs, ImportDbError, RegisterVfsError,
    SQLiteIoMethods, SQLiteVfs, VfsAppData, VfsError, VfsFile, VfsResult, VfsStore,
};

use js_sys::{Array, DataView, IteratorNext, Map, Reflect, Set, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetDirectoryOptions,
    FileSystemGetFileOptions, FileSystemReadWriteOptions, FileSystemSyncAccessHandle,
    WorkerGlobalScope,
};

const SECTOR_SIZE: usize = 4096;
const HEADER_MAX_FILENAME_SIZE: usize = 512;
const HEADER_FLAGS_SIZE: usize = 4;
const HEADER_CORPUS_SIZE: usize = HEADER_MAX_FILENAME_SIZE + HEADER_FLAGS_SIZE;
const HEADER_OFFSET_FLAGS: usize = HEADER_MAX_FILENAME_SIZE;
const HEADER_OFFSET_DATA: usize = SECTOR_SIZE;

// Will be enabled after sqlite fix is released
//
// <https://sqlite.org/src/forumpost/042d53c928382021>
//
// <https://github.com/sqlite/sqlite-wasm/issues/97>
//
// const HEADER_DIGEST_SIZE: usize = 8;
// const HEADER_OFFSET_DIGEST: usize = HEADER_CORPUS_SIZE;

const PERSISTENT_FILE_TYPES: i32 =
    SQLITE_OPEN_MAIN_DB | SQLITE_OPEN_MAIN_JOURNAL | SQLITE_OPEN_SUPER_JOURNAL | SQLITE_OPEN_WAL;

type Result<T> = std::result::Result<T, OpfsSAHError>;

fn read_write_options(at: f64) -> FileSystemReadWriteOptions {
    let options = FileSystemReadWriteOptions::new();
    options.set_at(at);
    options
}

struct OpfsSAHPool {
    /// Directory handle to the subdir of vfs root which holds
    /// the randomly-named "opaque" files. This subdir exists in the
    /// hope that we can eventually support client-created files in
    dh_opaque: FileSystemDirectoryHandle,
    /// Buffer used by [sg]et_associated_filename()
    header_buffer: Uint8Array,
    /// DataView for self.header_buffer
    header_buffer_view: DataView,
    /// Set of currently-unused SAHs
    available_sah: Set,
    /// Maps client-side file names to SAHs
    map_filename_to_sah: Map,
    /// Maps SAHs to their opaque file names
    map_sah_to_opaque_name: Map,
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
            header_buffer: ap_body,
            header_buffer_view: dv_body,
            map_filename_to_sah: Map::new(),
            available_sah: Set::default(),
            map_sah_to_opaque_name: Map::new(),
        };

        pool.acquire_access_handles(clear_files).await?;
        pool.reserve_minimum_capacity(capacity).await?;

        Ok(pool)
    }

    async fn add_capacity(&self, n: u32) -> Result<u32> {
        for _ in 0..n {
            let name = random_name();
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
            self.map_sah_to_opaque_name.set(&sah, &JsValue::from(name));
            self.set_associated_filename(&sah, None, 0)?;
        }
        Ok(self.get_capacity())
    }

    async fn reserve_minimum_capacity(&self, min: u32) -> Result<()> {
        self.add_capacity(min.saturating_sub(self.get_capacity()))
            .await?;
        Ok(())
    }

    async fn reduce_capacity(&self, n: u32) -> Result<u32> {
        let mut result = 0;
        for sah in Array::from(&self.available_sah) {
            if result == n || self.get_capacity() == self.get_file_count() {
                break;
            }
            let sah = FileSystemSyncAccessHandle::from(sah);
            let name = self.map_sah_to_opaque_name.get(&sah).as_string().unwrap();

            sah.close();
            JsFuture::from(self.dh_opaque.remove_entry(&name))
                .await
                .map_err(OpfsSAHError::RemoveEntity)?;
            self.map_sah_to_opaque_name.delete(&sah);
            self.available_sah.delete(&sah);

            result += 1;
        }
        Ok(result)
    }

    fn get_capacity(&self) -> u32 {
        self.map_sah_to_opaque_name.size()
    }

    fn get_file_count(&self) -> u32 {
        self.map_filename_to_sah.size()
    }

    fn get_filenames(&self) -> Vec<String> {
        self.map_filename_to_sah
            .keys()
            .into_iter()
            .flatten()
            .map(|x| x.as_string().unwrap())
            .collect()
    }

    fn get_associated_filename(&self, sah: &FileSystemSyncAccessHandle) -> Result<Option<String>> {
        sah.read_with_buffer_source_and_options(&self.header_buffer, &read_write_options(0.0))
            .map_err(OpfsSAHError::Read)?;
        let flags = self.header_buffer_view.get_uint32(HEADER_OFFSET_FLAGS);
        if self.header_buffer.get_index(0) != 0
            && ((flags & SQLITE_OPEN_DELETEONCLOSE as u32 != 0)
                || (flags & PERSISTENT_FILE_TYPES as u32) == 0)
        {
            self.set_associated_filename(sah, None, 0)?;
            return Ok(None);
        }

        let name_length = Array::from(&self.header_buffer)
            .iter()
            .position(|x| x.as_f64().unwrap() as u8 == 0)
            .unwrap_or_default();
        if name_length == 0 {
            sah.truncate_with_u32(HEADER_OFFSET_DATA as u32)
                .map_err(OpfsSAHError::Truncate)?;
            return Ok(None);
        }
        // set_associated_filename ensures that it is utf8
        let filename =
            String::from_utf8(self.header_buffer.subarray(0, name_length as u32).to_vec()).unwrap();
        Ok(Some(filename))
    }

    fn set_associated_filename(
        &self,
        sah: &FileSystemSyncAccessHandle,
        filename: Option<&str>,
        flags: i32,
    ) -> Result<()> {
        self.header_buffer_view
            .set_uint32(HEADER_OFFSET_FLAGS, flags as u32);

        if let Some(filename) = filename {
            if filename.is_empty() {
                return Err(OpfsSAHError::Generic("Filename is empty".into()));
            }
            if HEADER_MAX_FILENAME_SIZE <= filename.len() + 1 {
                return Err(OpfsSAHError::Generic(format!(
                    "Filename too long: {filename}"
                )));
            }
            self.header_buffer
                .subarray(0, filename.len() as u32)
                .copy_from(filename.as_bytes());
            self.header_buffer
                .fill(0, filename.len() as u32, HEADER_MAX_FILENAME_SIZE as u32);
            self.map_filename_to_sah.set(&JsValue::from(filename), sah);
            self.available_sah.delete(sah);
        } else {
            self.header_buffer
                .fill(0, 0, HEADER_MAX_FILENAME_SIZE as u32);
            sah.truncate_with_u32(HEADER_OFFSET_DATA as u32)
                .map_err(OpfsSAHError::Truncate)?;
            self.available_sah.add(sah);
        }

        sah.write_with_js_u8_array_and_options(&self.header_buffer, &read_write_options(0.0))
            .map_err(OpfsSAHError::Write)?;

        Ok(())
    }

    async fn acquire_access_handles(&self, clear_files: bool) -> Result<()> {
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
            let name = array.get(0);
            let value = array.get(1);
            let kind = Reflect::get(&value, &JsValue::from("kind"))
                .map_err(OpfsSAHError::Reflect)?
                .as_string();
            if kind.as_deref() == Some("file") {
                let handle = FileSystemFileHandle::from(value);
                let sah = JsFuture::from(handle.create_sync_access_handle())
                    .await
                    .map_err(OpfsSAHError::CreateSyncAccessHandle)?;
                self.map_sah_to_opaque_name.set(&sah, &name);
                let sah = FileSystemSyncAccessHandle::from(sah);
                if clear_files {
                    self.set_associated_filename(&sah, None, 0)?;
                } else if let Some(filename) = self.get_associated_filename(&sah)? {
                    self.map_filename_to_sah.set(&JsValue::from(filename), &sah);
                } else {
                    self.available_sah.add(&sah);
                }
            }
        }

        Ok(())
    }

    fn release_access_handles(&self) {
        for sah in self.map_sah_to_opaque_name.keys().into_iter().flatten() {
            let sah = FileSystemSyncAccessHandle::from(sah);
            sah.close();
        }
        self.map_sah_to_opaque_name.clear();
        self.map_filename_to_sah.clear();
        self.available_sah.clear();
    }

    fn delete_file(&self, filename: &str) -> Result<bool> {
        let sah = self.map_filename_to_sah.get(&JsValue::from(filename));
        let found = !sah.is_undefined();
        if found {
            let sah: FileSystemSyncAccessHandle = sah.into();
            self.map_filename_to_sah.delete(&JsValue::from(filename));
            self.set_associated_filename(&sah, None, 0)?;
        }
        Ok(found)
    }

    fn has_filename(&self, filename: &str) -> bool {
        self.map_filename_to_sah.has(&JsValue::from(filename))
    }

    fn get_sah(&self, filename: &str) -> Option<FileSystemSyncAccessHandle> {
        self.has_filename(filename).then(|| {
            self.map_filename_to_sah
                .get(&JsValue::from(filename))
                .into()
        })
    }

    fn next_available_sah(&self) -> Option<FileSystemSyncAccessHandle> {
        self.available_sah
            .keys()
            .next()
            .ok()
            .filter(|x| !x.done())
            .map(|x| x.value().into())
    }

    fn export_db(&self, filename: &str) -> Result<Vec<u8>> {
        let sah = self.map_filename_to_sah.get(&JsValue::from(filename));
        if sah.is_undefined() {
            return Err(OpfsSAHError::Generic(format!("File not found: {filename}")));
        }

        let sah = FileSystemSyncAccessHandle::from(sah);
        let actual_size = (sah.get_size().map_err(OpfsSAHError::GetSize)?
            - HEADER_OFFSET_DATA as f64)
            .max(0.0) as usize;

        let mut data = vec![0; actual_size];
        if actual_size > 0 {
            let read = sah
                .read_with_u8_array_and_options(
                    &mut data,
                    &read_write_options(HEADER_OFFSET_DATA as f64),
                )
                .map_err(OpfsSAHError::Read)?;
            if read != actual_size as f64 {
                return Err(OpfsSAHError::Generic(format!(
                    "Expected to read {actual_size} bytes but read {read}.",
                )));
            }
        }
        Ok(data)
    }

    fn import_db(&self, filename: &str, bytes: &[u8]) -> Result<()> {
        check_import_db(bytes)?;
        self.import_db_unchecked(filename, bytes, true)
    }

    fn import_db_unchecked(&self, filename: &str, bytes: &[u8], clear_wal: bool) -> Result<()> {
        if self.has_filename(filename) {
            return Err(OpfsSAHError::Generic(format!(
                "{filename} file already exists."
            )));
        }

        let sah = self
            .next_available_sah()
            .ok_or_else(|| OpfsSAHError::Generic("No available handles to import to.".into()))?;

        let length = bytes.len() as f64;
        let written = sah
            .write_with_u8_array_and_options(bytes, &read_write_options(HEADER_OFFSET_DATA as f64))
            .map_err(OpfsSAHError::Write)?;
        if written != length {
            self.set_associated_filename(&sah, None, 0)?;
            return Err(OpfsSAHError::Generic(format!(
                "Expected to write {length} bytes but wrote {written}.",
            )));
        }

        if clear_wal {
            // forced to write back to legacy mode
            sah.write_with_u8_array_and_options(
                &[1, 1],
                &read_write_options((HEADER_OFFSET_DATA + 18) as f64),
            )
            .map_err(OpfsSAHError::Write)?;
        }

        self.set_associated_filename(&sah, Some(filename), SQLITE_OPEN_MAIN_DB)?;

        Ok(())
    }
}

impl VfsFile for FileSystemSyncAccessHandle {
    fn read(&self, buf: &mut [u8], offset: usize) -> VfsResult<bool> {
        let n_read = self
            .read_with_u8_array_and_options(
                buf,
                &read_write_options((HEADER_OFFSET_DATA + offset) as f64),
            )
            .map_err(OpfsSAHError::Read)
            .map_err(|err| err.vfs_err(SQLITE_IOERR))?;

        if (n_read as usize) < buf.len() {
            buf[n_read as usize..].fill(0);
            return Ok(false);
        }

        Ok(true)
    }

    fn write(&mut self, buf: &[u8], offset: usize) -> VfsResult<()> {
        let n_write = self
            .write_with_u8_array_and_options(
                buf,
                &read_write_options((HEADER_OFFSET_DATA + offset) as f64),
            )
            .map_err(OpfsSAHError::Write)
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

type SyncAccessHandleAppData = OpfsSAHPool;

struct SyncAccessHandleStore;

impl VfsStore<FileSystemSyncAccessHandle, SyncAccessHandleAppData> for SyncAccessHandleStore {
    fn add_file(vfs: *mut sqlite3_vfs, file: &str, flags: i32) -> VfsResult<()> {
        let pool = unsafe { Self::app_data(vfs) };

        if let Some(sah) = pool.next_available_sah() {
            pool.set_associated_filename(&sah, Some(file), flags)
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
        if let Err(err) = pool.delete_file(file) {
            return Err(err.vfs_err(SQLITE_IOERR_DELETE));
        }
        Ok(())
    }

    fn with_file<F: Fn(&FileSystemSyncAccessHandle) -> VfsResult<i32>>(
        vfs_file: &super::utils::SQLiteVfsFile,
        f: F,
    ) -> VfsResult<i32> {
        let name = unsafe { vfs_file.name() };
        let pool = unsafe { Self::app_data(vfs_file.vfs) };
        match pool.get_sah(name) {
            Some(file) => f(&file),
            None => Err(VfsError::new(SQLITE_IOERR, format!("{name} not found"))),
        }
    }

    fn with_file_mut<F: Fn(&mut FileSystemSyncAccessHandle) -> VfsResult<i32>>(
        vfs_file: &super::utils::SQLiteVfsFile,
        f: F,
    ) -> VfsResult<i32> {
        let name = unsafe { vfs_file.name() };
        let pool = unsafe { Self::app_data(vfs_file.vfs) };
        match pool.get_sah(name) {
            Some(mut file) => f(&mut file),
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

    unsafe extern "C" fn xSectorSize(_pFile: *mut sqlite3_file) -> ::std::os::raw::c_int {
        SECTOR_SIZE as i32
    }

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
    const MAX_PATH_SIZE: ::std::os::raw::c_int = HEADER_MAX_FILENAME_SIZE as _;
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
    #[error(transparent)]
    ImportDb(#[from] ImportDbError),
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
    #[error("An error occurred while getting filename")]
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
}

impl OpfsSAHError {
    fn vfs_err(&self, code: i32) -> VfsError {
        VfsError::new(code, format!("{self}"))
    }
}

/// A OpfsSAHPoolUtil instance is exposed to clients in order to
/// manipulate an OpfsSAHPool object without directly exposing that
/// object and allowing for some semantic changes compared to that
/// class.
pub struct OpfsSAHPoolUtil {
    pool: &'static VfsAppData<SyncAccessHandleAppData>,
}

unsafe impl Send for OpfsSAHPoolUtil {}

unsafe impl Sync for OpfsSAHPoolUtil {}

impl OpfsSAHPoolUtil {
    /// Returns the number of files currently contained in the SAH pool.
    pub fn get_capacity(&self) -> u32 {
        self.pool.get_capacity()
    }

    /// Adds n entries to the current pool.
    pub async fn add_capacity(&self, n: u32) -> Result<u32> {
        self.pool.add_capacity(n).await
    }

    /// Removes up to n entries from the pool, with the caveat that
    /// it can only remove currently-unused entries.
    pub async fn reduce_capacity(&self, n: u32) -> Result<u32> {
        self.pool.reduce_capacity(n).await
    }

    /// Removes up to n entries from the pool, with the caveat that it can only
    /// remove currently-unused entries.
    pub async fn reserve_minimum_capacity(&self, min: u32) -> Result<()> {
        self.pool.reserve_minimum_capacity(min).await
    }
}

impl OpfsSAHPoolUtil {
    /// Imports the contents of an SQLite database, provided as a byte array
    /// under the given name, overwriting any existing content.
    ///
    /// If the database is imported with WAL mode enabled,
    /// it will be forced to write back to legacy mode, see
    /// <https://sqlite.org/forum/forumpost/67882c5b04>.
    ///
    /// If the imported database is encrypted, use `import_db_unchecked` instead.
    pub fn import_db(&self, filename: &str, bytes: &[u8]) -> Result<()> {
        self.pool.import_db(filename, bytes)
    }

    /// `import_db` without checking, can be used to import encrypted database.
    pub fn import_db_unchecked(&self, filename: &str, bytes: &[u8]) -> Result<()> {
        self.pool.import_db_unchecked(filename, bytes, false)
    }

    /// Export the database.
    pub fn export_db(&self, filename: &str) -> Result<Vec<u8>> {
        self.pool.export_db(filename)
    }

    /// Delete the specified database, please make sure that the database is closed.
    pub fn delete_db(&self, filename: &str) -> Result<bool> {
        self.pool.delete_file(filename)
    }

    /// Delete all database, please make sure that all database is closed.
    pub async fn clear_all(&self) -> Result<()> {
        self.pool.release_access_handles();
        self.pool.acquire_access_handles(true).await?;
        Ok(())
    }

    /// Does the database exists.
    pub fn exists(&self, filename: &str) -> Result<bool> {
        Ok(self.pool.has_filename(filename))
    }

    /// List all files.
    pub fn list(&self) -> Vec<String> {
        self.pool.get_filenames()
    }

    /// Number of files.
    pub fn count(&self) -> u32 {
        self.pool.get_file_count()
    }
}

/// Register `opfs-sahpool` vfs and return a utility object which can be used
/// to perform basic administration of the file pool
pub async fn install(options: &OpfsSAHPoolCfg, default_vfs: bool) -> Result<OpfsSAHPoolUtil> {
    static REGISTER_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = REGISTER_GUARD.lock().await;

    let vfs = match registered_vfs(&options.vfs_name) {
        Ok(vfs) => vfs,
        Err(RegisterVfsError::VfsNotRegistered) => {
            register_vfs::<SyncAccessHandleIoMethods, SyncAccessHandleVfs>(
                &options.vfs_name,
                OpfsSAHPool::new(options).await?,
                default_vfs,
            )?
        }
        Err(vfs_error) => return Err(OpfsSAHError::Vfs(vfs_error)),
    };
    let pool = unsafe { SyncAccessHandleStore::app_data(vfs) };

    Ok(OpfsSAHPoolUtil { pool })
}

#[cfg(test)]
mod tests {
    use crate::{
        sahpool_vfs::{
            OpfsSAHPool, OpfsSAHPoolCfgBuilder, SyncAccessHandleAppData, SyncAccessHandleStore,
        },
        utils::{test_suite::test_vfs_store, VfsAppData},
    };
    use wasm_bindgen_test::wasm_bindgen_test;
    use web_sys::FileSystemSyncAccessHandle;

    #[wasm_bindgen_test]
    async fn test_opfs_vfs_store() {
        let data = OpfsSAHPool::new(
            &OpfsSAHPoolCfgBuilder::new()
                .directory("test_opfs_suite")
                .build(),
        )
        .await
        .unwrap();

        test_vfs_store::<SyncAccessHandleAppData, FileSystemSyncAccessHandle, SyncAccessHandleStore>(VfsAppData::new(data))
            .unwrap();
    }
}
