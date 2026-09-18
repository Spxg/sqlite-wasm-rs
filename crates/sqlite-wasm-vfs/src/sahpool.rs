//! opfs-sahpool vfs implementation, ported from sqlite-wasm.
//!
//! See [`opfs-sahpool`](https://sqlite.org/wasm/doc/trunk/persistence.md#vfs-opfs-sahpool) for details.
//!
//! ```rust
//! use sqlite_wasm_rs as ffi;
//! use sqlite_wasm_vfs::sahpool::{install as install_opfs_sahpool, OpfsSAHPoolCfg};
//!
//! async fn open_db() {
//!     // install opfs-sahpool persistent vfs and set as default vfs
//!     install_opfs_sahpool::<ffi::WasmOsCallback>(&OpfsSAHPoolCfg::default(), true)
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

use rsqlite_vfs::{
    check_import_db,
    ffi::{
        sqlite3_file, sqlite3_vfs, sqlite3_vfs_register, sqlite3_vfs_unregister,
        SQLITE_FCNTL_SIZE_HINT, SQLITE_IOCAP_UNDELETABLE_WHEN_OPEN, SQLITE_NOTFOUND, SQLITE_OK,
        SQLITE_OPEN_DELETEONCLOSE, SQLITE_OPEN_MAIN_DB, SQLITE_OPEN_MAIN_JOURNAL,
        SQLITE_OPEN_SUPER_JOURNAL, SQLITE_OPEN_WAL,
    },
    register_vfs, registered_vfs, AccessMode, ImportDbError, LockLevel, OpenAccess, OpenOptions,
    OpenedFile, OsCallback, RegisterVfsError, SQLiteIoMethods, SQLiteVfs, SQLiteVfsFile,
    SyncOptions, VfsAppData, VfsError, VfsErrorCode, VfsFile, VfsResult, VfsStore,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use js_sys::{Array, DataView, IteratorNext, Reflect, Uint8Array};
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

const PERSISTENT_FILE_TYPES: i32 =
    SQLITE_OPEN_MAIN_DB | SQLITE_OPEN_MAIN_JOURNAL | SQLITE_OPEN_SUPER_JOURNAL | SQLITE_OPEN_WAL;

type Result<T, E = OpfsSAHError> = std::result::Result<T, E>;

fn read_write_options(at: f64) -> FileSystemReadWriteOptions {
    let options = FileSystemReadWriteOptions::new();
    options.set_at(at);
    options
}

#[derive(Clone)]
struct SyncAccessFile {
    handle: FileSystemSyncAccessHandle,
    opaque: String,
    open_count: Rc<Cell<usize>>,
}

struct SyncAccessFileHandle {
    temporary_name: Option<String>,
    file: SyncAccessFile,
    read_only: bool,
}

impl Drop for SyncAccessFileHandle {
    fn drop(&mut self) {
        self.file.open_count.set(self.file.open_count.get() - 1);
    }
}

impl SyncAccessFileHandle {
    fn check_writable(&self) -> VfsResult<()> {
        if self.read_only {
            return Err(VfsError::new(
                VfsErrorCode::ReadOnly,
                "File is read-only".into(),
            ));
        }
        Ok(())
    }

    fn apply_size_hint(&mut self, hint: i64) -> VfsResult<()> {
        if hint <= 0 || hint as u64 <= self.size()? {
            return Ok(());
        }
        self.check_writable()?;
        self.file.apply_size_hint(hint)
    }
}

impl VfsFile for SyncAccessFileHandle {
    fn read(&mut self, buf: &mut [u8], offset: u64) -> VfsResult<usize> {
        self.file.read(buf, offset)
    }
    fn write(&mut self, buf: &[u8], offset: u64) -> VfsResult<()> {
        self.check_writable()?;
        self.file.write(buf, offset)
    }
    fn truncate(&mut self, size: u64) -> VfsResult<()> {
        self.check_writable()?;
        self.file.truncate(size)
    }
    fn sync(&mut self, options: SyncOptions) -> VfsResult<()> {
        self.file.sync(options)
    }
    fn lock(&mut self, level: LockLevel) -> VfsResult<()> {
        self.file.lock(level)
    }
    fn unlock(&mut self, level: LockLevel) -> VfsResult<()> {
        self.file.unlock(level)
    }
    fn check_reserved_lock(&self) -> VfsResult<bool> {
        self.file.check_reserved_lock()
    }
    fn size(&self) -> VfsResult<u64> {
        self.file.size()
    }
}

impl SyncAccessFile {
    /// Pre-extends files because growing OPFS once is cheaper than growing per page.
    fn apply_size_hint(&mut self, hint: i64) -> VfsResult<()> {
        if hint <= 0 || hint as u64 <= self.size()? {
            return Ok(());
        }

        self.truncate(hint as u64)
    }
}

// OPFS offsets use JavaScript Numbers.
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

fn physical_offset(offset: u64, length: usize, code: VfsErrorCode) -> VfsResult<f64> {
    let start = offset.checked_add(HEADER_OFFSET_DATA as u64);
    let end = start.and_then(|start| start.checked_add(length as u64));
    if end.unwrap_or(u64::MAX) > MAX_SAFE_INTEGER {
        return Err(VfsError::new(
            code,
            "File offset or size exceeds JavaScript's safe integer range".into(),
        ));
    }
    Ok(start.unwrap() as f64)
}

struct OpfsSAHPool {
    last_error: RefCell<Option<VfsError>>,
    /// Directory handle to the `.opaque` subdirectory within the VFS root.
    /// This directory holds the actual files, which have randomly-generated names.
    dh_opaque: FileSystemDirectoryHandle,
    /// A reusable buffer for reading and writing file headers.
    header_buffer: Uint8Array,
    /// A `DataView` for accessing the binary data in `header_buffer`.
    header_buffer_view: DataView,
    /// A pool of available `SyncAccessHandle`s that are not currently associated with a database file.
    available_files: RefCell<Vec<SyncAccessFile>>,
    /// Maps the user-facing database filenames to their underlying `SyncAccessFile`.
    map_filename_to_file: RefCell<HashMap<String, SyncAccessFile>>,
    /// A flag to indicate whether the VFS is currently paused.
    is_paused: Cell<bool>,
    /// A tuple holding the raw pointer to the `sqlite3_vfs` struct and whether it was registered as the default.
    vfs: Cell<(*mut sqlite3_vfs, bool)>,
    os: Box<dyn OsCallback>,
}

impl OpfsSAHPool {
    async fn new<C: OsCallback + Default + 'static>(
        options: &OpfsSAHPoolCfg,
    ) -> Result<OpfsSAHPool> {
        const OPAQUE_DIR_NAME: &str = ".opaque";

        let vfs_dir = &options.directory;
        let capacity = options.initial_capacity;
        let clear_files = options.clear_on_init;

        let create_option = FileSystemGetDirectoryOptions::new();
        create_option.set_create(true);

        let mut handle: FileSystemDirectoryHandle = JsFuture::from(
            js_sys::global()
                .dyn_into::<WorkerGlobalScope>()
                .map_err(|_| OpfsSAHError::NotSupported)?
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
            last_error: RefCell::new(None),
            dh_opaque,
            header_buffer: ap_body,
            header_buffer_view: dv_body,
            map_filename_to_file: RefCell::new(HashMap::new()),
            available_files: RefCell::new(Vec::new()),
            is_paused: Cell::new(false),
            vfs: Cell::new((std::ptr::null_mut(), false)),
            os: Box::new(C::default()),
        };

        pool.acquire_access_handles(clear_files).await?;
        pool.ensure_capacity(capacity).await?;

        Ok(pool)
    }

    async fn add_capacity(&self, n: usize) -> Result<usize> {
        for _ in 0..n {
            let opaque = rsqlite_vfs::random_name(|buf| self.os.random(buf))?;
            let handle: FileSystemFileHandle =
                JsFuture::from(self.dh_opaque.get_file_handle_with_options(&opaque, &{
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
            let file = SyncAccessFile {
                handle: sah,
                opaque,
                open_count: Rc::new(Cell::new(0)),
            };
            self.set_associated_filename(&file.handle, None, 0)?;
            self.available_files.borrow_mut().push(file);
        }
        Ok(self.capacity())
    }

    async fn ensure_capacity(&self, min: usize) -> Result<()> {
        self.add_capacity(min.saturating_sub(self.capacity()))
            .await?;
        Ok(())
    }

    #[allow(clippy::await_holding_refcell_ref)]
    async fn reduce_capacity(&self, n: usize) -> Result<usize> {
        let mut available_files = self.available_files.borrow_mut();
        let available_length = available_files.len();
        let max_reduce = available_length.min(n);
        let files = available_files.split_off(available_length - max_reduce);
        // The `RefMut` from `name2file` is explicitly dropped here to avoid holding the borrow across an `.await` point.
        drop(available_files);

        for file in files {
            file.handle.close();
            JsFuture::from(self.dh_opaque.remove_entry(&file.opaque))
                .await
                .map_err(OpfsSAHError::RemoveEntity)?;
        }

        Ok(max_reduce)
    }

    fn capacity(&self) -> usize {
        self.map_filename_to_file.borrow().len() + self.available_files.borrow().len()
    }

    fn get_file_count(&self) -> usize {
        self.map_filename_to_file.borrow().len()
    }

    fn get_filenames(&self) -> Vec<String> {
        self.map_filename_to_file.borrow().keys().cloned().collect()
    }

    fn get_associated_filename(&self, sah: &FileSystemSyncAccessHandle) -> Result<Option<String>> {
        sah.read_with_buffer_source_and_options(&self.header_buffer, &read_write_options(0.0))
            .map_err(OpfsSAHError::Read)?;
        let flags = self.header_buffer_view.get_uint32(HEADER_OFFSET_FLAGS);
        if self.header_buffer.get_index(0) != 0
            && ((flags & SQLITE_OPEN_DELETEONCLOSE as u32 != 0)
                || (flags & PERSISTENT_FILE_TYPES as u32) == 0)
        {
            return Ok(None);
        }

        let name_length = self
            .header_buffer
            .to_vec()
            .iter()
            .position(|&x| x == 0)
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
        } else {
            self.header_buffer
                .fill(0, 0, HEADER_MAX_FILENAME_SIZE as u32);
            sah.truncate_with_u32(HEADER_OFFSET_DATA as u32)
                .map_err(OpfsSAHError::Truncate)?;
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
            let opaque = array
                .get(0)
                .as_string()
                .ok_or_else(|| OpfsSAHError::Generic("Failed to get file's opaque name".into()))?;
            let value = array.get(1);
            let kind = Reflect::get(&value, &JsValue::from("kind"))
                .map_err(OpfsSAHError::Reflect)?
                .as_string();
            if kind.as_deref() == Some("file") {
                let handle = FileSystemFileHandle::from(value);
                let sah = JsFuture::from(handle.create_sync_access_handle())
                    .await
                    .map_err(OpfsSAHError::CreateSyncAccessHandle)?;
                let sah = FileSystemSyncAccessHandle::from(sah);
                let file = SyncAccessFile {
                    handle: sah,
                    opaque,
                    open_count: Rc::new(Cell::new(0)),
                };
                let clear_file = |file: SyncAccessFile| -> Result<()> {
                    self.set_associated_filename(&file.handle, None, 0)?;
                    self.available_files.borrow_mut().push(file);
                    Ok(())
                };
                if clear_files {
                    clear_file(file)?;
                } else if let Some(filename) = self.get_associated_filename(&file.handle)? {
                    self.map_filename_to_file
                        .borrow_mut()
                        .insert(filename, file);
                } else {
                    clear_file(file)?;
                }
            }
        }

        Ok(())
    }

    fn release_access_handles(&self) -> Result<()> {
        if self
            .map_filename_to_file
            .borrow()
            .values()
            .any(|file| file.open_count.get() != 0)
        {
            return Err(OpfsSAHError::Generic(
                "Cannot release handles: files are in use".into(),
            ));
        }
        for file in std::mem::take(&mut *self.available_files.borrow_mut())
            .into_iter()
            .chain(std::mem::take(&mut *self.map_filename_to_file.borrow_mut()).into_values())
        {
            file.handle.close();
        }
        Ok(())
    }

    fn delete_file(&self, filename: &str) -> Result<bool> {
        let mut map_filename_to_file = self.map_filename_to_file.borrow_mut();
        let mut available_files = self.available_files.borrow_mut();

        if let Some(file) = map_filename_to_file.get(filename) {
            if file.open_count.get() != 0 {
                return Err(OpfsSAHError::Generic("Cannot delete an open file".into()));
            }
            self.set_associated_filename(&file.handle, None, 0)?;
        }
        if let Some(file) = map_filename_to_file.remove(filename) {
            available_files.push(file);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn has_filename(&self, filename: &str) -> bool {
        self.map_filename_to_file.borrow().contains_key(filename)
    }

    fn with_new_file<E, F: Fn(&SyncAccessFile) -> Result<(), E>>(
        &self,
        filename: &str,
        flags: i32,
        f: F,
    ) -> Result<Result<(), E>> {
        let mut map_filename_to_file = self.map_filename_to_file.borrow_mut();
        let mut available_files = self.available_files.borrow_mut();
        if map_filename_to_file.contains_key(filename) {
            return Err(OpfsSAHError::Generic(format!(
                "{filename} file already exists"
            )));
        }
        let file = available_files
            .pop()
            .ok_or_else(|| OpfsSAHError::Generic("No files available in the pool".into()))?;
        map_filename_to_file.insert(filename.into(), file);

        let Some(file) = map_filename_to_file.get(filename) else {
            unreachable!();
        };
        self.set_associated_filename(&file.handle, Some(filename), flags)?;
        Ok(f(file))
    }

    fn pause(&self) -> Result<()> {
        if self.is_paused.get() {
            return Ok(());
        }

        if self
            .map_filename_to_file
            .borrow()
            .values()
            .any(|file| file.open_count.get() != 0)
        {
            return Err(OpfsSAHError::Generic(
                "Cannot pause: files may be in use".to_string(),
            ));
        }

        let (vfs, _) = self.vfs.get();
        if !vfs.is_null() {
            unsafe {
                sqlite3_vfs_unregister(vfs);
            }
        }
        self.release_access_handles()?;

        self.is_paused.set(true);

        Ok(())
    }

    async fn resume(&self) -> Result<()> {
        if !self.is_paused.get() {
            return Ok(());
        }

        self.acquire_access_handles(false).await?;

        let (vfs, make_default) = self.vfs.get();
        if vfs.is_null() {
            return Err(OpfsSAHError::Generic(
                "VFS pointer is null. Did you forget to install?".to_string(),
            ));
        }

        match unsafe { sqlite3_vfs_register(vfs, i32::from(make_default)) } {
            SQLITE_OK => {
                self.is_paused.set(false);
                Ok(())
            }
            error_code => Err(OpfsSAHError::Generic(format!(
                "Failed to register VFS (SQLite error code: {error_code})"
            ))),
        }
    }

    fn export_db(&self, filename: &str) -> Result<Vec<u8>> {
        let files = self.map_filename_to_file.borrow();
        let file = files
            .get(filename)
            .ok_or_else(|| OpfsSAHError::Generic(format!("File not found: {filename}")))?;

        let sah = &file.handle;
        let actual_size = file
            .size()
            .map_err(|err| OpfsSAHError::Generic(format!("Failed to get file size: {err:?}")))?;
        let actual_size = usize::try_from(actual_size)
            .ok()
            .filter(|&size| size <= isize::MAX as usize)
            .ok_or_else(|| {
                OpfsSAHError::Generic("File is too large to export into memory".into())
            })?;

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
        self.with_new_file(filename, SQLITE_OPEN_MAIN_DB, |file| {
            let sah = &file.handle;
            let length = bytes.len() as f64;
            let written = sah
                .write_with_u8_array_and_options(
                    bytes,
                    &read_write_options(HEADER_OFFSET_DATA as f64),
                )
                .map_err(OpfsSAHError::Write)?;

            if written != length {
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

            Ok(())
        })?
    }
}

impl VfsFile for SyncAccessFile {
    fn read(&mut self, buf: &mut [u8], offset: u64) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let at = physical_offset(offset, buf.len(), VfsErrorCode::IoRead)?;
        let n_read = self
            .handle
            .read_with_u8_array_and_options(buf, &read_write_options(at))
            .map_err(OpfsSAHError::Read)
            .map_err(|err| err.vfs_err(VfsErrorCode::IoRead))?;

        Ok(n_read as usize)
    }

    fn write(&mut self, buf: &[u8], offset: u64) -> VfsResult<()> {
        let at = physical_offset(offset, buf.len(), VfsErrorCode::IoWrite)?;
        let n_write = self
            .handle
            .write_with_u8_array_and_options(buf, &read_write_options(at))
            .map_err(OpfsSAHError::Write)
            .map_err(|err| err.vfs_err(VfsErrorCode::IoWrite))?;

        if buf.len() != n_write as usize {
            return Err(VfsError::new(
                VfsErrorCode::IoWrite,
                "failed to write file".into(),
            ));
        }

        Ok(())
    }

    fn truncate(&mut self, size: u64) -> VfsResult<()> {
        let size = physical_offset(size, 0, VfsErrorCode::IoTruncate)?;
        self.handle
            .truncate_with_f64(size)
            .map_err(OpfsSAHError::Truncate)
            .map_err(|err| err.vfs_err(VfsErrorCode::IoTruncate))
    }

    fn sync(&mut self, _options: SyncOptions) -> VfsResult<()> {
        // OPFS exposes one flush primitive. Use it for both sync strengths and
        // also flush metadata when only data was requested.
        FileSystemSyncAccessHandle::flush(&self.handle)
            .map_err(OpfsSAHError::Flush)
            .map_err(|err| err.vfs_err(VfsErrorCode::IoSync))
    }
    // Preserve SAH pool's existing exclusive-ownership policy. This is not
    // transaction locking between multiple SQLite connections in this worker.
    fn lock(&mut self, _level: LockLevel) -> VfsResult<()> {
        Ok(())
    }
    fn unlock(&mut self, _level: LockLevel) -> VfsResult<()> {
        Ok(())
    }
    fn check_reserved_lock(&self) -> VfsResult<bool> {
        Ok(true)
    }

    fn size(&self) -> VfsResult<u64> {
        let size = self
            .handle
            .get_size()
            .map_err(OpfsSAHError::GetSize)
            .map_err(|err| err.vfs_err(VfsErrorCode::IoStat))?;
        if !size.is_finite() || size < 0.0 || size.fract() != 0.0 || size > MAX_SAFE_INTEGER as f64
        {
            return Err(VfsError::new(
                VfsErrorCode::IoStat,
                "Invalid OPFS file size".into(),
            ));
        }
        Ok((size as u64).saturating_sub(HEADER_OFFSET_DATA as u64))
    }
}

type SyncAccessHandleAppData = OpfsSAHPool;

thread_local! {
    // Only instances created here have app data with the expected type and
    // lifetime. Looking up a name in SQLite alone cannot establish either.
    static REGISTERED_POOLS: RefCell<HashMap<*mut sqlite3_vfs, &'static VfsAppData<SyncAccessHandleAppData>>> =
        RefCell::new(HashMap::new());
}

struct SyncAccessHandleStore;

impl VfsStore for SyncAccessHandleStore {
    type File = SyncAccessFileHandle;
    type AppData = SyncAccessHandleAppData;
    fn close_file(
        data: &Self::AppData,
        name: Option<&str>,
        mut file: Self::File,
        options: OpenOptions,
    ) -> VfsResult<()> {
        let temporary_name = file.temporary_name.take();
        let name = name
            .or(temporary_name.as_deref())
            .expect("opened SAH file has a name");
        drop(file);
        if options.delete_on_close() {
            Self::delete_file(data, name, false)?;
        }
        Ok(())
    }
    fn record_error(data: &Self::AppData, error: VfsError) {
        data.last_error.replace(Some(error));
    }
    fn last_error(data: &Self::AppData) -> Option<VfsError> {
        data.last_error.borrow().clone()
    }
    fn open_file(
        pool: &SyncAccessHandleAppData,
        request: rsqlite_vfs::OpenRequest<'_>,
    ) -> VfsResult<OpenedFile<SyncAccessFileHandle>> {
        let options = request.options;
        let temporary_name = request
            .filename
            .is_none()
            .then(|| rsqlite_vfs::random_name(|buf| pool.os.random(buf)))
            .transpose()?;
        let filename = request
            .filename
            .map(|name| name.path())
            .or(temporary_name.as_deref())
            .unwrap();
        if pool.is_paused.get() {
            return Err(VfsError::new(
                VfsErrorCode::CantOpen,
                "VFS is paused".into(),
            ));
        }
        if pool.has_filename(filename) {
            if options.exclusive() {
                return Err(VfsError::new(
                    VfsErrorCode::CantOpen,
                    format!("{filename} already exists").into(),
                ));
            }
        } else {
            if !options.create() {
                return Err(VfsError::new(
                    VfsErrorCode::CantOpen,
                    format!("{filename} not found").into(),
                ));
            }
            pool.with_new_file(filename, options.raw_flags(), |_| Ok(()))
                .map_err(|err| err.vfs_err(VfsErrorCode::CantOpen))??;
        }
        let file = pool
            .map_filename_to_file
            .borrow()
            .get(filename)
            .unwrap()
            .clone();
        file.open_count.set(file.open_count.get() + 1);
        Ok(OpenedFile {
            file: SyncAccessFileHandle {
                file,
                temporary_name,
                read_only: options.access() == OpenAccess::ReadOnly,
            },
            access: options.access(),
        })
    }

    fn access(pool: &SyncAccessHandleAppData, file: &str, _mode: AccessMode) -> VfsResult<bool> {
        // All named files use read-write sync handles; no per-file ACL exists.
        Ok(pool.has_filename(file))
    }

    fn full_pathname(_pool: &SyncAccessHandleAppData, name: &str) -> VfsResult<String> {
        Ok(name.into())
    }

    fn delete_file(pool: &SyncAccessHandleAppData, file: &str, _sync_dir: bool) -> VfsResult<()> {
        // Deletion clears and flushes the slot header (our persistent namespace).
        pool.delete_file(file)
            .map_err(|err| err.vfs_err(VfsErrorCode::IoDelete))?;
        Ok(())
    }
}

struct SyncAccessHandleIoMethods;

impl SQLiteIoMethods for SyncAccessHandleIoMethods {
    type Store = SyncAccessHandleStore;

    const VERSION: ::std::os::raw::c_int = 1;

    unsafe extern "C" fn xFileControl(
        pFile: *mut sqlite3_file,
        op: ::std::os::raw::c_int,
        pArg: *mut ::std::os::raw::c_void,
    ) -> ::std::os::raw::c_int {
        if op != SQLITE_FCNTL_SIZE_HINT {
            return SQLITE_NOTFOUND;
        }

        let vfs_file = &mut *SQLiteVfsFile::from_file(pFile);
        let app_data = VfsAppData::<<Self::Store as VfsStore>::AppData>::get(vfs_file.vfs);
        let hint = *pArg.cast::<i64>();
        match vfs_file
            .handle_mut::<<Self::Store as VfsStore>::File>()
            .apply_size_hint(hint)
        {
            Ok(()) => SQLITE_OK,
            Err(err) => {
                let code = err.raw_code();
                Self::Store::record_error(app_data, err);
                code
            }
        }
    }

    unsafe extern "C" fn xSectorSize(_pFile: *mut sqlite3_file) -> ::std::os::raw::c_int {
        SECTOR_SIZE as i32
    }

    unsafe extern "C" fn xDeviceCharacteristics(
        _pFile: *mut sqlite3_file,
    ) -> ::std::os::raw::c_int {
        SQLITE_IOCAP_UNDELETABLE_WHEN_OPEN
    }
}

struct SyncAccessHandleVfs;

impl SQLiteVfs<SyncAccessHandleIoMethods> for SyncAccessHandleVfs {
    type Os = dyn OsCallback;
    fn os(data: &SyncAccessHandleAppData) -> &Self::Os {
        &*data.os
    }

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

    /// Sets the minimum total number of file slots at initialization.
    /// An existing larger pool is not shrunk.
    pub fn initial_capacity(mut self, cap: usize) -> Self {
        self.0.initial_capacity = cap;
        self
    }

    /// Build `OpfsSAHPoolCfg`.
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
    /// Specifies the OPFS directory name in which to store metadata for the `vfs_name`.
    pub directory: String,
    /// If truthy, contents and filename mapping are removed from each SAH
    /// as it is acquired during initalization of the VFS, leaving the VFS's
    /// storage in a pristine state. Use this only for databases which need not
    /// survive a page reload.
    pub clear_on_init: bool,
    /// Minimum total number of file slots at initialization.
    /// An existing larger pool is not shrunk.
    pub initial_capacity: usize,
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
    Backend(#[from] VfsError),
    #[error(transparent)]
    Vfs(#[from] RegisterVfsError),
    #[error(transparent)]
    ImportDb(#[from] ImportDbError),
    #[error("This vfs is only available in dedicated worker")]
    NotSupported,
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
    fn vfs_err(&self, code: VfsErrorCode) -> VfsError {
        VfsError::new(code, format!("{self}").into())
    }
}

/// SAHPoolVfs management tool.
pub struct OpfsSAHPoolUtil {
    pool: &'static VfsAppData<SyncAccessHandleAppData>,
}

impl OpfsSAHPoolUtil {
    /// Returns the total number of file slots, both assigned and available.
    pub fn capacity(&self) -> usize {
        self.pool.capacity()
    }

    /// Adds `n` file slots and returns the resulting total capacity.
    pub async fn add_capacity(&self, n: usize) -> Result<usize> {
        self.pool.add_capacity(n).await
    }

    /// Removes up to `n` unused file slots and returns the number removed.
    /// Slots assigned to files are retained, even when those files are closed.
    pub async fn reduce_capacity(&self, n: usize) -> Result<usize> {
        self.pool.reduce_capacity(n).await
    }

    /// Ensures the total capacity is at least `min`, adding slots if needed.
    /// Does nothing when the capacity is already sufficient; never shrinks it.
    pub async fn ensure_capacity(&self, min: usize) -> Result<()> {
        self.pool.ensure_capacity(min).await
    }
}

impl OpfsSAHPoolUtil {
    /// Imports the contents of an SQLite database, provided as a byte array
    /// under the given name. Returns an error if the name already exists.
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
    /// Returns an error if the name already exists.
    pub fn import_db_unchecked(&self, filename: &str, bytes: &[u8]) -> Result<()> {
        self.pool.import_db_unchecked(filename, bytes, false)
    }

    /// Export the database.
    pub fn export_db(&self, filename: &str) -> Result<Vec<u8>> {
        self.pool.export_db(filename)
    }

    /// Deletes the named file, returning whether it existed.
    /// The database must be closed before deleting its files.
    pub fn delete_db(&self, filename: &str) -> Result<bool> {
        self.pool.delete_file(filename)
    }

    /// Deletes all files. All databases must be closed first.
    pub async fn clear_all(&self) -> Result<()> {
        self.pool.release_access_handles()?;
        self.pool.acquire_access_handles(true).await?;
        Ok(())
    }

    /// Returns whether the named file exists in the VFS.
    pub fn exists(&self, filename: &str) -> bool {
        self.pool.has_filename(filename)
    }

    /// Returns all filenames in unspecified order, including auxiliary files.
    pub fn list(&self) -> Vec<String> {
        self.pool.get_filenames()
    }

    /// Returns the number of files, including auxiliary files, not unused slots.
    pub fn count(&self) -> usize {
        self.pool.get_file_count()
    }

    /// "Pauses" this VFS by unregistering it from SQLite and
    /// relinquishing all open SAHs, leaving the associated files
    /// intact. If this instance is already paused, this is a
    /// no-op. Returns a Result.
    ///
    /// This method returns an error if SQLite has any opened file handles
    /// hosted by this VFS, as the alternative would be to invoke
    /// Undefined Behavior by closing file handles out from under the
    /// library. Similarly, automatically closing any database handles
    /// opened by this VFS would invoke Undefined Behavior in
    /// downstream code which is holding those pointers.
    ///
    /// If this method returns and error due to open file handles then it has
    /// no side effects. If the OPFS API returns an error while closing handles
    /// then the VFS is left in an undefined state.
    pub fn pause(&self) -> Result<()> {
        self.pool.pause()
    }

    /// Resumes this VFS, reacquiring all SAHs and (if successful)
    /// re-registering it with SQLite. This is a no-op if the VFS is
    /// not currently paused.
    ///
    /// The returned a Result. See acquire_access_handles() for how it
    /// behaves if it returns an error due to SAH acquisition failure.
    pub async fn resume(&self) -> Result<()> {
        self.pool.resume().await
    }

    /// Check if VFS is paused.
    pub fn is_paused(&self) -> bool {
        self.pool.is_paused.get()
    }
}

/// Register `opfs-sahpool` vfs and return a management tool which can be used
/// to perform basic administration of the file pool.
///
/// Reuses an existing SAH pool registered by this module under the same name.
/// Returns a name conflict error if another implementation owns that name.
pub async fn install<C: OsCallback + Default + 'static>(
    options: &OpfsSAHPoolCfg,
    default_vfs: bool,
) -> Result<OpfsSAHPoolUtil> {
    static REGISTER_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = REGISTER_GUARD.lock().await;

    // SAFETY: Lookup runs synchronously on this SQLite worker. No registered
    // SAH pool is freed; the guard serializes this module's installation flow.
    let (vfs, pool) = match unsafe { registered_vfs(&options.vfs_name)? } {
        Some(vfs) => {
            let pool = REGISTERED_POOLS
                .with(|pools| pools.borrow().get(&vfs).copied())
                .ok_or_else(|| RegisterVfsError::NameConflict(options.vfs_name.clone()))?;
            (vfs, pool)
        }
        None => {
            let data = OpfsSAHPool::new::<C>(options).await?;
            // SAFETY: Our VFS uses the default SQLiteVfsFile layout and retains
            // the supplied name/app-data pointers. Its methods use the matching
            // pool type on this worker; installation is serialized by the guard.
            let vfs = unsafe {
                register_vfs::<SyncAccessHandleIoMethods, SyncAccessHandleVfs>(
                    &options.vfs_name,
                    data,
                    default_vfs,
                )?
                .into_raw()
            };
            // This instance was just allocated by us and is retained for the
            // lifetime of the module, including while paused/unregistered.
            let pool = unsafe { VfsAppData::<SyncAccessHandleAppData>::get(vfs) };
            REGISTERED_POOLS.with(|pools| pools.borrow_mut().insert(vfs, pool));
            (vfs, pool)
        }
    };

    pool.vfs.set((vfs, default_vfs));

    Ok(OpfsSAHPoolUtil { pool })
}

#[cfg(test)]
mod tests {
    use super::{OpfsSAHError, OpfsSAHPool, OpfsSAHPoolCfgBuilder, SyncAccessHandleStore};
    use rsqlite_vfs::{
        test_suite::test_vfs_store, FileKind, OpenAccess, OpenOptions, VfsAppData, VfsErrorCode,
        VfsFile, VfsStore,
    };
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    async fn install_rejects_foreign_vfs() {
        let memory = unsafe { sqlite_wasm_rs::vfs::memvfs::MemVfsUtil::get().unwrap() };
        let before = unsafe { super::registered_vfs("memvfs").unwrap().unwrap() };
        let options = OpfsSAHPoolCfgBuilder::new().vfs_name("memvfs").build();
        let result = super::install::<sqlite_wasm_rs::WasmOsCallback>(&options, false).await;
        assert!(
            matches!(result, Err(OpfsSAHError::Vfs(super::RegisterVfsError::NameConflict(name))) if name == "memvfs")
        );
        assert_eq!(
            unsafe { super::registered_vfs("memvfs").unwrap() },
            Some(before)
        );
        assert_eq!(memory.count(), 0);
    }

    #[wasm_bindgen_test]
    async fn install_reuses_own_vfs() {
        let options = OpfsSAHPoolCfgBuilder::new()
            .vfs_name("test-opfs-reuse")
            .directory("test_opfs_reuse")
            .build();
        let first = super::install::<sqlite_wasm_rs::WasmOsCallback>(&options, false)
            .await
            .unwrap();
        let second = super::install::<sqlite_wasm_rs::WasmOsCallback>(&options, false)
            .await
            .unwrap();
        assert!(std::ptr::eq(first.pool, second.pool));
        first.pause().unwrap();
        first.resume().await.unwrap();
        let third = super::install::<sqlite_wasm_rs::WasmOsCallback>(&options, false)
            .await
            .unwrap();
        assert!(std::ptr::eq(first.pool, third.pool));
    }

    #[wasm_bindgen_test]
    fn physical_offsets_above_4_gib() {
        let offset = 1u64 << 32;
        assert_eq!(
            super::physical_offset(offset, 512, VfsErrorCode::IoWrite).unwrap(),
            (offset + super::HEADER_OFFSET_DATA as u64) as f64,
        );
        let max = super::MAX_SAFE_INTEGER - super::HEADER_OFFSET_DATA as u64;
        assert!(super::physical_offset(max, 0, VfsErrorCode::IoTruncate).is_ok());
        assert!(super::physical_offset(max, 1, VfsErrorCode::IoWrite).is_err());
        assert!(super::physical_offset(max + 1, 0, VfsErrorCode::IoTruncate).is_err());
        assert!(super::physical_offset(u64::MAX, 0, VfsErrorCode::IoRead).is_err());
    }

    #[wasm_bindgen_test]
    async fn size_hint_grows_but_does_not_shrink_file() {
        let pool = OpfsSAHPool::new::<sqlite_wasm_rs::WasmOsCallback>(
            &OpfsSAHPoolCfgBuilder::new()
                .directory("test_opfs_size_hint")
                .clear_on_init(true)
                .build(),
        )
        .await
        .unwrap();
        let mut file = SyncAccessHandleStore::open_file(
            &pool,
            rsqlite_vfs::OpenRequest::named(
                "size-hint.db",
                OpenOptions::new(OpenAccess::ReadWrite, FileKind::MainDb).with_create(),
            ),
        )
        .unwrap()
        .file;
        file.apply_size_hint(2 * 8192).unwrap();
        assert_eq!(file.size().unwrap(), 2 * 8192);
        file.apply_size_hint(8192).unwrap();
        assert_eq!(file.size().unwrap(), 2 * 8192);
    }

    #[wasm_bindgen_test]
    async fn test_opfs_vfs_store() {
        let data = OpfsSAHPool::new::<sqlite_wasm_rs::WasmOsCallback>(
            &OpfsSAHPoolCfgBuilder::new()
                .directory("test_opfs_suite")
                .build(),
        )
        .await
        .unwrap();

        test_vfs_store::<SyncAccessHandleStore>(VfsAppData::new(data)).unwrap();
    }

    #[wasm_bindgen_test]
    async fn exclusive_create_preserves_existing_file() {
        let pool = OpfsSAHPool::new::<sqlite_wasm_rs::WasmOsCallback>(
            &OpfsSAHPoolCfgBuilder::new()
                .directory("test_opfs_exclusive_create")
                .clear_on_init(true)
                .build(),
        )
        .await
        .unwrap();
        let flags = OpenOptions::new(OpenAccess::ReadWrite, FileKind::MainDb).with_create();
        let exclusive = flags.with_create_new();
        let mut file = SyncAccessHandleStore::open_file(
            &pool,
            rsqlite_vfs::OpenRequest::named("exclusive.db", exclusive),
        )
        .unwrap()
        .file;
        file.write(&[41, 42], 0).unwrap();
        SyncAccessHandleStore::close_file(&pool, Some("exclusive.db"), file, exclusive).unwrap();
        let available = pool.available_files.borrow().len();

        assert_eq!(
            SyncAccessHandleStore::open_file(
                &pool,
                rsqlite_vfs::OpenRequest::named("exclusive.db", exclusive)
            )
            .err()
            .unwrap()
            .code(),
            VfsErrorCode::CantOpen
        );
        assert_eq!(pool.available_files.borrow().len(), available);
        assert_eq!(pool.map_filename_to_file.borrow().len(), 1);
        let mut file = SyncAccessHandleStore::open_file(
            &pool,
            rsqlite_vfs::OpenRequest::named("exclusive.db", flags),
        )
        .unwrap()
        .file;
        assert_eq!(file.file.open_count.get(), 1);
        let mut bytes = [0; 2];
        assert_eq!(file.size().unwrap(), 2);
        assert_eq!(file.read(&mut bytes, 0).unwrap(), 2);
        assert_eq!(bytes, [41, 42]);
        SyncAccessHandleStore::close_file(&pool, Some("exclusive.db"), file, flags).unwrap();
        pool.release_access_handles().unwrap();
    }

    #[wasm_bindgen_test]
    async fn handles_protect_pool_until_last_close() {
        let config = OpfsSAHPoolCfgBuilder::new()
            .vfs_name("test-handle-lifetimes")
            .directory("test_handle_lifetimes")
            .clear_on_init(true)
            .build();
        let util = super::install::<sqlite_wasm_rs::WasmOsCallback>(&config, false)
            .await
            .unwrap();
        let flags = OpenOptions::new(OpenAccess::ReadWrite, FileKind::MainDb).with_create();
        let mut first = SyncAccessHandleStore::open_file(
            util.pool,
            rsqlite_vfs::OpenRequest::named("handles.db", flags),
        )
        .unwrap()
        .file;
        let mut second = SyncAccessHandleStore::open_file(
            util.pool,
            rsqlite_vfs::OpenRequest::named(
                "handles.db",
                OpenOptions::new(OpenAccess::ReadOnly, FileKind::MainDb),
            ),
        )
        .unwrap()
        .file;
        assert_eq!(first.file.open_count.get(), 2);
        first.write(&[41, 42], 0).unwrap();
        let mut bytes = [0; 2];
        assert_eq!(second.read(&mut bytes, 0).unwrap(), 2);
        assert_eq!(bytes, [41, 42]);
        assert_eq!(
            second.write(&[99], 0).unwrap_err().code(),
            VfsErrorCode::ReadOnly
        );
        assert_eq!(
            second.truncate(0).unwrap_err().code(),
            VfsErrorCode::ReadOnly
        );
        assert_eq!(
            second.apply_size_hint(4096).unwrap_err().code(),
            VfsErrorCode::ReadOnly
        );
        let capacity = util.capacity();
        assert!(util.pause().is_err());
        assert!(util.delete_db("handles.db").is_err());
        assert!(util.clear_all().await.is_err());
        assert_eq!(util.capacity(), capacity);
        assert!(util.exists("handles.db"));
        SyncAccessHandleStore::close_file(util.pool, Some("handles.db"), first, flags).unwrap();
        assert_eq!(second.file.open_count.get(), 1);
        assert!(util.pause().is_err());
        assert!(util.delete_db("handles.db").is_err());
        assert!(util.clear_all().await.is_err());
        assert_eq!(second.read(&mut bytes, 0).unwrap(), 2);
        drop(second);
        assert!(util.delete_db("handles.db").unwrap());
        util.pause().unwrap();
        util.resume().await.unwrap();

        let file = SyncAccessHandleStore::open_file(
            util.pool,
            rsqlite_vfs::OpenRequest::named("temporary.db", flags),
        )
        .unwrap()
        .file;
        SyncAccessHandleStore::close_file(
            util.pool,
            Some("temporary.db"),
            file,
            flags.with_delete_on_close(),
        )
        .unwrap();
        assert!(!util.exists("temporary.db"));
        util.pause().unwrap();
    }
}
