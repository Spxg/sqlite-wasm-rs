//! OPFS sync access handle pool VFS, ported from sqlite-wasm.
//!
//! See [`opfs-sahpool`](https://sqlite.org/wasm/doc/trunk/persistence.md#vfs-opfs-sahpool) for details.
//!
//! Requires a secure context and a dedicated worker. Use one SQLite connection
//! per database at a time: repeated opens share storage but do not coordinate
//! locks between connections. Shared-memory WAL is not supported.
//!
//! ```rust
//! use sqlite_wasm_rs as ffi;
//! use sqlite_wasm_vfs::sahpool::{install, OpfsSAHPoolCfg};
//!
//! async fn open_db() {
//!     install::<ffi::WasmOsCallback>(&OpfsSAHPoolCfg::default(), true)
//!         .await
//!         .unwrap();
//!
//!     // The pool is now SQLite's default VFS.
//!     let mut db = std::ptr::null_mut();
//!     unsafe {
//!         assert_eq!(ffi::sqlite3_open(c"app.db".as_ptr(), &mut db), ffi::SQLITE_OK);
//!         assert_eq!(ffi::sqlite3_close(db), ffi::SQLITE_OK);
//!     }
//! }
//! ```

use rsqlite_vfs::transfer::{DbTransfer, ExportSource, ImportDbError, ImportTarget, TransferError};
use rsqlite_vfs::{
    ffi::{
        sqlite3_vfs_register, sqlite3_vfs_unregister, SQLITE_OK, SQLITE_OPEN_DELETEONCLOSE,
        SQLITE_OPEN_MAIN_DB, SQLITE_OPEN_MAIN_JOURNAL, SQLITE_OPEN_SUBJOURNAL,
        SQLITE_OPEN_SUPER_JOURNAL, SQLITE_OPEN_TEMP_DB, SQLITE_OPEN_TEMP_JOURNAL,
        SQLITE_OPEN_TRANSIENT_DB, SQLITE_OPEN_WAL,
    },
    register_vfs, registered_vfs, AccessMode, DeviceCharacteristics, FileKind, LockLevel,
    OpenAccess, OpenOptions, OpenedFile, OsCallback, RegisterVfsError, SQLiteIoMethods, SQLiteVfs,
    SectorSize, SyncOptions, VfsError, VfsErrorCode, VfsFile, VfsFilesManager, VfsRegistration,
    VfsResult, VfsStore,
};
use std::{
    any::TypeId,
    cell::{Cell, RefCell},
    collections::HashMap,
    future::poll_fn,
    ops::Deref,
    rc::Rc,
    task::{Poll, Waker},
};

use js_sys::{Array, IteratorNext, Reflect};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    DedicatedWorkerGlobalScope, FileSystemDirectoryHandle, FileSystemFileHandle,
    FileSystemGetDirectoryOptions, FileSystemGetFileOptions, FileSystemReadWriteOptions,
    FileSystemSyncAccessHandle,
};

const SECTOR_SIZE: usize = 4096;
const HEADER_MAX_FILENAME_SIZE: usize = 512;
// SQLite may append '-' followed by up to 11 characters to a database name.
const MAX_DB_FILENAME_SIZE: usize = HEADER_MAX_FILENAME_SIZE - 1 - 12;
// Each physical slot begins with a NUL-padded UTF-8 name and big-endian open
// flags. SQLite file offsets start after the reserved 4096-byte header region.
const HEADER_CORPUS_SIZE: usize = HEADER_MAX_FILENAME_SIZE + 4;
const HEADER_OFFSET_DATA: usize = SECTOR_SIZE;
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

// Bound filename collision retries so a faulty randomness callback cannot loop
// forever. This is a local policy, not a SQLite or OPFS requirement.
const MAX_FILENAME_ATTEMPTS: usize = 16;

const PERSISTENT_FILE_TYPES: i32 =
    SQLITE_OPEN_MAIN_DB | SQLITE_OPEN_MAIN_JOURNAL | SQLITE_OPEN_SUPER_JOURNAL | SQLITE_OPEN_WAL;
const TEMPORARY_FILE_TYPES: i32 = SQLITE_OPEN_TEMP_DB
    | SQLITE_OPEN_TEMP_JOURNAL
    | SQLITE_OPEN_TRANSIENT_DB
    | SQLITE_OPEN_SUBJOURNAL;

type Result<T, E = OpfsSAHError> = std::result::Result<T, E>;

fn read_write_options(at: f64) -> FileSystemReadWriteOptions {
    let options = FileSystemReadWriteOptions::new();
    options.set_at(at);
    options
}

fn validate_filename(filename: &str) -> Result<()> {
    if filename.is_empty() {
        Err(OpfsSAHError::InvalidFilename("filename is empty"))
    } else if filename.as_bytes().contains(&0) {
        Err(OpfsSAHError::InvalidFilename(
            "filename contains a NUL byte",
        ))
    } else if filename.len() >= HEADER_MAX_FILENAME_SIZE {
        Err(OpfsSAHError::InvalidFilename(
            "filename exceeds 511 UTF-8 bytes",
        ))
    } else {
        Ok(())
    }
}

fn validate_db_filename(filename: &str) -> Result<()> {
    validate_filename(filename)?;

    if filename.len() > MAX_DB_FILENAME_SIZE {
        return Err(OpfsSAHError::InvalidFilename(
            "database filename exceeds 499 UTF-8 bytes (reserved journal suffix space)",
        ));
    }

    Ok(())
}

fn normalize_directory(directory: &str) -> Result<String> {
    let parts: Vec<_> = directory
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();

    if parts.is_empty()
        || parts
            .iter()
            .any(|part| matches!(*part, "." | "..") || part.contains('\0'))
    {
        return Err(OpfsSAHError::InvalidDirectory);
    }

    Ok(parts.join("/"))
}

fn physical_offset(offset: u64, length: usize, code: VfsErrorCode) -> VfsResult<f64> {
    let start = offset.checked_add(HEADER_OFFSET_DATA as u64);
    let end = start.and_then(|start| start.checked_add(length as u64));

    if end.unwrap_or(u64::MAX) > MAX_SAFE_INTEGER {
        return Err(VfsError::new(
            code,
            "file offset or size exceeds JavaScript's safe integer range".into(),
        ));
    }

    Ok(start.unwrap() as f64)
}

/// One owner for the browser resource, even when the pool and SQLite share it.
struct SyncAccessFileInner {
    handle: FileSystemSyncAccessHandle,
    opaque: String,
    open_count: Cell<usize>,
}

impl Drop for SyncAccessFileInner {
    fn drop(&mut self) {
        self.handle.close();
    }
}

#[derive(Clone)]
struct SyncAccessFile(Rc<SyncAccessFileInner>);

impl Deref for SyncAccessFile {
    type Target = SyncAccessFileInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl SyncAccessFile {
    // A browser Promise cannot be cancelled. Let a local task adopt its result:
    // if the caller disappears, the task drops (and closes) the acquired handle.
    async fn acquire(handle: FileSystemFileHandle, opaque: String) -> Result<Self> {
        #[derive(Default)]
        struct Pending {
            result: Option<Result<SyncAccessFile>>,
            waker: Option<Waker>,
        }

        let pending = Rc::new(RefCell::new(Pending::default()));
        let completion = pending.clone();

        spawn_local(async move {
            let result = JsFuture::from(handle.create_sync_access_handle())
                .await
                .map(|value| {
                    Self(Rc::new(SyncAccessFileInner {
                        handle: value.into(),
                        opaque,
                        open_count: Cell::new(0),
                    }))
                })
                .map_err(|err| OpfsSAHError::js("acquire sync access handle", err));

            let waker = {
                let mut completion = completion.borrow_mut();
                completion.result = Some(result);
                completion.waker.take()
            };

            if let Some(waker) = waker {
                waker.wake();
            }
        });

        poll_fn(|cx| {
            let mut pending = pending.borrow_mut();

            if let Some(result) = pending.result.take() {
                Poll::Ready(result)
            } else {
                pending.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }

    fn physical_size(&self) -> Result<u64> {
        let size = self
            .handle
            .get_size()
            .map_err(|err| OpfsSAHError::js("get file size", err))?;

        if !size.is_finite() || size < 0.0 || size.fract() != 0.0 || size > MAX_SAFE_INTEGER as f64
        {
            return Err(OpfsSAHError::InvalidHeader {
                opaque: self.opaque.clone(),
                reason: "invalid physical file size",
            });
        }

        Ok(size as u64)
    }

    fn size(&self) -> VfsResult<u64> {
        self.physical_size()
            .and_then(|size| {
                size.checked_sub(HEADER_OFFSET_DATA as u64).ok_or_else(|| {
                    OpfsSAHError::InvalidHeader {
                        opaque: self.opaque.clone(),
                        reason: "file is shorter than its header",
                    }
                })
            })
            .map_err(|err| err.vfs_err(VfsErrorCode::IoStat))
    }

    fn read_at(&self, buf: &mut [u8], at: f64) -> Result<usize> {
        let count = self
            .handle
            .read_with_u8_array_and_options(buf, &read_write_options(at))
            .map_err(|err| OpfsSAHError::js("read file", err))?;

        if !count.is_finite() || count < 0.0 || count.fract() != 0.0 || count > buf.len() as f64 {
            return Err(OpfsSAHError::ShortIo {
                operation: "read file",
                expected: buf.len(),
                actual: count,
            });
        }

        Ok(count as usize)
    }

    fn write_at(&self, bytes: &[u8], at: f64) -> Result<()> {
        let count = self
            .handle
            .write_with_u8_array_and_options(bytes, &read_write_options(at))
            .map_err(|err| OpfsSAHError::js("write file", err))?;

        if count != bytes.len() as f64 {
            return Err(OpfsSAHError::ShortIo {
                operation: "write file",
                expected: bytes.len(),
                actual: count,
            });
        }

        Ok(())
    }

    fn flush(&self) -> Result<()> {
        self.handle
            .flush()
            .map_err(|err| OpfsSAHError::js("flush file", err))
    }

    fn associated_filename(&self) -> Result<Option<String>> {
        let size = self.physical_size()?;

        // A cancelled allocation can leave a newly created, empty slot.
        if size == 0 {
            return Ok(None);
        }

        let invalid = |reason| OpfsSAHError::InvalidHeader {
            opaque: self.opaque.clone(),
            reason,
        };

        if size < HEADER_OFFSET_DATA as u64 {
            // An interrupted initialization can leave a zero-filled prefix.
            // It cannot contain database bytes, which begin at offset 4096.
            let mut bytes = [0; HEADER_OFFSET_DATA];
            let bytes = &mut bytes[..size as usize];
            if self.read_at(bytes, 0.0)? != bytes.len() || bytes.iter().any(|&byte| byte != 0) {
                return Err(invalid("file is shorter than its header"));
            }

            return Ok(None);
        }

        let mut header = [0; HEADER_CORPUS_SIZE];
        if self.read_at(&mut header, 0.0)? != header.len() {
            return Err(invalid("incomplete header"));
        }

        let flags = u32::from_be_bytes(header[HEADER_MAX_FILENAME_SIZE..].try_into().unwrap());
        let name = &header[..HEADER_MAX_FILENAME_SIZE];

        let end = name
            .iter()
            .position(|&byte| byte == 0)
            .ok_or_else(|| invalid("filename is not NUL-terminated"))?;
        if name[end..].iter().any(|&byte| byte != 0) {
            return Err(invalid("nonzero bytes after filename terminator"));
        }

        if end == 0 {
            if flags != 0 {
                return Err(invalid("unnamed slot has nonzero flags"));
            }

            return Ok(None);
        }

        let name = std::str::from_utf8(&name[..end])
            .map_err(|_| invalid("filename is not valid UTF-8"))?;
        // Older imports stored only MAIN_DB, without access-mode bits. Validate
        // the persisted file kind rather than treating this as a new xOpen call.
        let kind = flags & (PERSISTENT_FILE_TYPES | TEMPORARY_FILE_TYPES) as u32;
        if kind.count_ones() != 1 {
            return Err(invalid("missing or conflicting file types"));
        }

        if flags & SQLITE_OPEN_DELETEONCLOSE as u32 != 0 {
            return Ok(None);
        }

        if kind & TEMPORARY_FILE_TYPES as u32 != 0 {
            return Err(invalid("temporary file is missing DELETEONCLOSE"));
        }

        Ok(Some(name.to_owned()))
    }

    // Only for new or verified zero-filled, incomplete slots. OPFS pads an
    // extension with zeros, so the complete empty header is initialized at once.
    fn initialize(&self) -> Result<()> {
        self.handle
            .truncate_with_u32(HEADER_OFFSET_DATA as u32)
            .map_err(|err| OpfsSAHError::js("initialize pool slot", err))?;
        self.flush()
    }

    fn associate(&self, filename: Option<&str>, flags: i32) -> Result<()> {
        let mut header = [0; HEADER_CORPUS_SIZE];
        if let Some(filename) = filename {
            validate_filename(filename)?;
            header[..filename.len()].copy_from_slice(filename.as_bytes());
        }
        header[HEADER_MAX_FILENAME_SIZE..].copy_from_slice(&(flags as u32).to_be_bytes());
        self.write_at(&header, 0.0)?;

        // The header is the persistent namespace. In particular, journal
        // deletion must reach storage before that slot can be reused.
        self.flush()?;

        if filename.is_none() {
            self.handle
                .truncate_with_u32(HEADER_OFFSET_DATA as u32)
                .map_err(|err| OpfsSAHError::js("truncate unused slot", err))?;
            self.flush()?;
        }

        Ok(())
    }
}

struct SyncAccessFileHandle {
    temporary_name: Option<String>,
    file: SyncAccessFile,
    read_only: bool,
    lock_level: LockLevel,
}

impl Drop for SyncAccessFileHandle {
    fn drop(&mut self) {
        self.file.open_count.set(self.file.open_count.get() - 1);
    }
}

impl SyncAccessFileHandle {
    fn check_writable(&self) -> VfsResult<()> {
        if self.read_only {
            Err(VfsError::new(
                VfsErrorCode::ReadOnly,
                "file is read-only".into(),
            ))
        } else {
            Ok(())
        }
    }
}

impl VfsFile for SyncAccessFileHandle {
    fn size_hint(&mut self, size: u64) -> VfsResult<bool> {
        if size > self.size()? {
            self.truncate(size)?;
        }

        Ok(true)
    }

    fn sector_size(&self) -> SectorSize {
        SectorSize::new(SECTOR_SIZE as u32).unwrap()
    }

    fn device_characteristics(&self) -> DeviceCharacteristics {
        DeviceCharacteristics::UNDELETABLE_WHEN_OPEN
    }

    fn read(&mut self, buf: &mut [u8], offset: u64) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let at = physical_offset(offset, buf.len(), VfsErrorCode::IoRead)?;
        self.file
            .read_at(buf, at)
            .map_err(|err| err.vfs_err(VfsErrorCode::IoRead))
    }

    fn write(&mut self, buf: &[u8], offset: u64) -> VfsResult<()> {
        self.check_writable()?;

        if buf.is_empty() {
            return Ok(());
        }

        let at = physical_offset(offset, buf.len(), VfsErrorCode::IoWrite)?;
        self.file
            .write_at(buf, at)
            .map_err(|err| err.vfs_err(VfsErrorCode::IoWrite))
    }

    fn truncate(&mut self, size: u64) -> VfsResult<()> {
        self.check_writable()?;

        let size = physical_offset(size, 0, VfsErrorCode::IoTruncate)?;
        self.file
            .handle
            .truncate_with_f64(size)
            .map_err(|err| OpfsSAHError::js("truncate file", err).vfs_err(VfsErrorCode::IoTruncate))
    }

    fn sync(&mut self, _options: SyncOptions) -> VfsResult<()> {
        // OPFS has one flush primitive, used for both sync strengths and metadata.
        self.file
            .flush()
            .map_err(|err| err.vfs_err(VfsErrorCode::IoSync))
    }

    fn size(&self) -> VfsResult<u64> {
        self.file.size()
    }

    fn lock(&mut self, level: LockLevel) -> VfsResult<()> {
        self.lock_level = self.lock_level.max(level);

        Ok(())
    }

    fn unlock(&mut self, level: LockLevel) -> VfsResult<()> {
        self.lock_level = self.lock_level.min(level);

        Ok(())
    }

    fn check_reserved_lock(&self) -> VfsResult<bool> {
        // OPFS excludes other workers. Multiple connections to the same DB
        // are unsupported; this is not a shared lock manager. A fresh
        // connection must report no RESERVED lock for hot-journal recovery.
        Ok(self.lock_level >= LockLevel::Reserved)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PoolState {
    Active,
    Paused,
    Removed,
}

struct Operation<'a>(&'a Cell<bool>);

impl Drop for Operation<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

struct OpfsSAHPool {
    last_error: RefCell<Option<VfsError>>,
    dh_opaque: FileSystemDirectoryHandle,
    lock_file: FileSystemFileHandle,
    lease: RefCell<Option<SyncAccessFile>>,
    available_files: RefCell<Vec<SyncAccessFile>>,
    map_filename_to_file: RefCell<HashMap<String, SyncAccessFile>>,
    // Failed cleanup leaves the on-disk name uncertain. Retain ownership until
    // clear or pause/resume reconciles it; never reuse these slots directly.
    quarantined_files: RefCell<Vec<SyncAccessFile>>,
    state: Cell<PoolState>,
    busy: Cell<bool>,
    needs_recovery: Cell<bool>,
    name: String,
    directory: String,
    callback_type: TypeId,
    make_default: Cell<bool>,
    // Registration owns an Rc back to this pool. Keep it alive even while
    // paused; explicit unsafe uninstall breaks the cycle and frees the VFS.
    registration: RefCell<Option<VfsRegistration<Rc<OpfsSAHPool>>>>,
    os: Box<dyn OsCallback>,
}

impl OpfsSAHPool {
    async fn new<C: OsCallback + Default + 'static>(options: &OpfsSAHPoolCfg) -> Result<Rc<Self>> {
        let directory = normalize_directory(&options.directory)?;
        let create = FileSystemGetDirectoryOptions::new();
        create.set_create(true);

        let worker = js_sys::global()
            .dyn_into::<DedicatedWorkerGlobalScope>()
            .map_err(|_| OpfsSAHError::NotSupported)?;
        for property in ["FileSystemSyncAccessHandle", "FileSystemFileHandle"] {
            if Reflect::get(&js_sys::global(), &property.into())
                .map_err(|err| OpfsSAHError::js("check OPFS support", err))?
                .is_undefined()
            {
                return Err(OpfsSAHError::NotSupported);
            }
        }

        let storage = worker.navigator().storage();
        if storage.is_undefined()
            || !Reflect::get(storage.as_ref(), &"getDirectory".into())
                .map_err(|err| OpfsSAHError::js("check OPFS support", err))?
                .is_function()
        {
            return Err(OpfsSAHError::NotSupported);
        }

        let mut handle: FileSystemDirectoryHandle = JsFuture::from(storage.get_directory())
            .await
            .map_err(|err| OpfsSAHError::js("get OPFS root", err))?
            .into();
        for part in directory.split('/') {
            handle = JsFuture::from(handle.get_directory_handle_with_options(part, &create))
                .await
                .map_err(|err| OpfsSAHError::js("get pool directory", err))?
                .into();
        }

        // Lock the namespace even when it has no slots yet. Otherwise two
        // workers can initialize an empty directory into disjoint pools.
        let options_lock = FileSystemGetFileOptions::new();
        options_lock.set_create(true);
        let lock_file: FileSystemFileHandle =
            JsFuture::from(handle.get_file_handle_with_options(".lock", &options_lock))
                .await
                .map_err(|err| OpfsSAHError::js("get pool lock file", err))?
                .into();
        let lease = SyncAccessFile::acquire(lock_file.clone(), ".lock".into()).await?;

        let dh_opaque =
            JsFuture::from(handle.get_directory_handle_with_options(".opaque", &create))
                .await
                .map_err(|err| OpfsSAHError::js("get opaque directory", err))?
                .into();

        let pool = Rc::new(Self {
            last_error: RefCell::new(None),
            dh_opaque,
            lock_file,
            lease: RefCell::new(Some(lease)),
            available_files: RefCell::new(Vec::new()),
            map_filename_to_file: RefCell::new(HashMap::new()),
            quarantined_files: RefCell::new(Vec::new()),
            state: Cell::new(PoolState::Active),
            busy: Cell::new(false),
            needs_recovery: Cell::new(false),
            name: options.vfs_name.clone(),
            directory,
            callback_type: TypeId::of::<C>(),
            make_default: Cell::new(false),
            registration: RefCell::new(None),
            os: Box::new(C::default()),
        });
        pool.acquire_access_handles(options.clear_on_init).await?;
        pool.ensure_capacity(options.initial_capacity).await?;

        Ok(pool)
    }

    fn check_state(&self) -> Result<()> {
        match self.state.get() {
            PoolState::Active => Ok(()),
            PoolState::Paused => Err(OpfsSAHError::Paused),
            PoolState::Removed => Err(OpfsSAHError::Uninstalled),
        }
    }

    fn check_active(&self) -> Result<()> {
        self.check_state()?;
        if self.busy.get() {
            return Err(OpfsSAHError::Busy);
        }

        if self.needs_recovery.get() {
            return Err(OpfsSAHError::NeedsRecovery);
        }

        Ok(())
    }

    fn begin_operation(&self) -> Result<Operation<'_>> {
        if self.state.get() == PoolState::Removed {
            return Err(OpfsSAHError::Uninstalled);
        }

        if self.busy.replace(true) {
            return Err(OpfsSAHError::Busy);
        }

        Ok(Operation(&self.busy))
    }

    fn check_closed(&self) -> Result<()> {
        if self
            .map_filename_to_file
            .borrow()
            .values()
            .any(|file| file.open_count.get() != 0)
        {
            Err(OpfsSAHError::FilesInUse)
        } else {
            Ok(())
        }
    }

    async fn add_capacity(&self, n: usize) -> Result<usize> {
        self.check_active()?;
        let _operation = self.begin_operation()?;

        let mut added = Vec::new();
        added
            .try_reserve(n)
            .map_err(|_| OpfsSAHError::OutOfMemory)?;
        self.available_files
            .borrow_mut()
            .try_reserve(n)
            .map_err(|_| OpfsSAHError::OutOfMemory)?;

        for _ in 0..n {
            let mut slot = None;
            for _ in 0..MAX_FILENAME_ATTEMPTS {
                let opaque = rsqlite_vfs::random_name(|buf| self.os.random(buf))?;
                if self
                    .available_files
                    .borrow()
                    .iter()
                    .chain(self.map_filename_to_file.borrow().values())
                    .chain(added.iter())
                    .any(|file: &SyncAccessFile| file.opaque == opaque)
                {
                    continue;
                }

                let options = FileSystemGetFileOptions::new();
                options.set_create(true);
                let handle = JsFuture::from(
                    self.dh_opaque
                        .get_file_handle_with_options(&opaque, &options),
                )
                .await
                .map_err(|err| OpfsSAHError::js("create pool file", err))?
                .into();
                let file = SyncAccessFile::acquire(handle, opaque).await?;

                // Never overwrite a file reached by a random-name collision.
                if file.physical_size()? != 0 {
                    continue;
                }

                if let Err(error) = file.initialize() {
                    let opaque = file.opaque.clone();
                    drop(file);

                    if let Err(err) = JsFuture::from(self.dh_opaque.remove_entry(&opaque)).await {
                        return Err(OpfsSAHError::Cleanup {
                            error: Box::new(error),
                            cleanup: Box::new(OpfsSAHError::js("remove incomplete slot", err)),
                        });
                    }

                    return Err(error);
                }

                slot = Some(file);
                break;
            }
            added.push(slot.ok_or(OpfsSAHError::NameCollision)?);
        }

        self.available_files.borrow_mut().extend(added);

        Ok(self.capacity())
    }

    async fn ensure_capacity(&self, min: usize) -> Result<()> {
        self.add_capacity(min.saturating_sub(self.capacity()))
            .await?;

        Ok(())
    }

    async fn reduce_capacity(&self, n: usize) -> Result<usize> {
        self.check_active()?;
        let _operation = self.begin_operation()?;

        let count = n.min(self.available_files.borrow().len());
        for _ in 0..count {
            let file = self.available_files.borrow_mut().pop().unwrap();
            let opaque = file.opaque.clone();
            drop(file);

            // Process one slot at a time: failure/cancellation cannot lose
            // ownership of the rest. An undeleted file is rediscovered on resume.
            JsFuture::from(self.dh_opaque.remove_entry(&opaque))
                .await
                .map_err(|err| OpfsSAHError::js("remove unused slot", err))?;
        }

        Ok(count)
    }

    fn capacity(&self) -> usize {
        self.map_filename_to_file.borrow().len()
            + self.available_files.borrow().len()
            + self.quarantined_files.borrow().len()
    }

    fn get_file_count(&self) -> usize {
        self.map_filename_to_file.borrow().len()
    }

    fn get_filenames(&self) -> Vec<String> {
        self.map_filename_to_file.borrow().keys().cloned().collect()
    }

    fn has_filename(&self, name: &str) -> bool {
        self.map_filename_to_file.borrow().contains_key(name)
    }

    async fn acquire_access_handles(&self, clear: bool) -> Result<()> {
        let mut acquired = Vec::new();
        let iter = self.dh_opaque.entries();
        loop {
            let next: IteratorNext = JsFuture::from(
                iter.next()
                    .map_err(|err| OpfsSAHError::js("iterate pool directory", err))?,
            )
            .await
            .map_err(|err| OpfsSAHError::js("iterate pool directory", err))?
            .into();
            if next.done() {
                break;
            }

            let array: Array = next.value().into();
            let opaque = array
                .get(0)
                .as_string()
                .ok_or(OpfsSAHError::InvalidDirectoryEntry)?;
            let value = array.get(1);
            let kind = Reflect::get(&value, &"kind".into())
                .map_err(|err| OpfsSAHError::js("get directory entry kind", err))?;
            if kind.as_string().as_deref() == Some("file") {
                acquired
                    .try_reserve(1)
                    .map_err(|_| OpfsSAHError::OutOfMemory)?;
                acquired.push(SyncAccessFile::acquire(value.into(), opaque).await?);
            }
        }

        // Acquire every handle before clearing any data or publishing the pool.
        let mut assigned = HashMap::new();
        let mut available = Vec::new();
        assigned
            .try_reserve(acquired.len())
            .map_err(|_| OpfsSAHError::OutOfMemory)?;
        available
            .try_reserve(acquired.len())
            .map_err(|_| OpfsSAHError::OutOfMemory)?;

        for file in acquired {
            let filename = if clear {
                None
            } else {
                file.associated_filename()?
            };

            if let Some(filename) = filename {
                if assigned.contains_key(&filename) {
                    return Err(OpfsSAHError::DuplicateFilename(filename));
                }
                assigned.insert(filename, file);
            } else {
                available.push(file);
            }
        }

        for file in &available {
            if !clear && file.physical_size()? < HEADER_OFFSET_DATA as u64 {
                file.initialize()?;
            } else {
                file.associate(None, 0)?;
            }
        }

        *self.map_filename_to_file.borrow_mut() = assigned;
        *self.available_files.borrow_mut() = available;
        self.needs_recovery.set(false);

        Ok(())
    }

    fn release_access_handles(&self) -> Result<()> {
        self.check_closed()?;

        self.available_files.borrow_mut().clear();
        self.map_filename_to_file.borrow_mut().clear();
        self.quarantined_files.borrow_mut().clear();
        self.lease.borrow_mut().take();

        Ok(())
    }

    fn delete_file(&self, filename: &str) -> Result<bool> {
        self.check_active()?;
        validate_filename(filename)?;

        let mut files = self.map_filename_to_file.borrow_mut();
        let Some(file) = files.get(filename) else {
            return Ok(false);
        };

        if file.open_count.get() != 0 {
            return Err(OpfsSAHError::FileInUse(filename.into()));
        }

        if let Err(err) = file.associate(None, 0) {
            // The durable name may now differ from our map. Fail closed until
            // an explicit pause/resume or clear reconciles the namespace.
            self.needs_recovery.set(true);
            return Err(err);
        }

        self.available_files
            .borrow_mut()
            .push(files.remove(filename).unwrap());

        Ok(true)
    }

    fn with_new_file(
        &self,
        filename: &str,
        flags: i32,
        write: impl FnOnce(&SyncAccessFile) -> Result<()>,
    ) -> Result<()> {
        self.check_active()?;
        validate_filename(filename)?;
        if self.has_filename(filename) {
            return Err(OpfsSAHError::FileExists(filename.into()));
        }

        self.map_filename_to_file
            .borrow_mut()
            .try_reserve(1)
            .map_err(|_| OpfsSAHError::OutOfMemory)?;
        let file = self
            .available_files
            .borrow_mut()
            .pop()
            .ok_or(OpfsSAHError::NoCapacity)?;

        // Write contents before publishing their name. associate flushes both.
        let result = write(&file).and_then(|()| file.associate(Some(filename), flags));
        if let Err(error) = result {
            if let Err(cleanup) = file.associate(None, 0) {
                self.quarantined_files.borrow_mut().push(file);
                self.needs_recovery.set(true);
                return Err(OpfsSAHError::Cleanup {
                    error: Box::new(error),
                    cleanup: Box::new(cleanup),
                });
            }

            self.available_files.borrow_mut().push(file);
            return Err(error);
        }

        self.map_filename_to_file
            .borrow_mut()
            .insert(filename.into(), file);

        Ok(())
    }

    fn pause(&self) -> Result<()> {
        let _operation = self.begin_operation()?;
        if self.state.get() == PoolState::Paused {
            return Ok(());
        }

        self.check_closed()?;
        if let Some(registration) = self.registration.borrow().as_ref() {
            let code = unsafe { sqlite3_vfs_unregister(registration.as_ptr()) };
            if code != SQLITE_OK {
                return Err(OpfsSAHError::sqlite("unregister VFS", code));
            }
        }

        self.release_access_handles()?;
        self.state.set(PoolState::Paused);

        Ok(())
    }

    async fn resume(&self) -> Result<()> {
        let _operation = self.begin_operation()?;
        if self.state.get() == PoolState::Active {
            return if self.needs_recovery.get() {
                Err(OpfsSAHError::NeedsRecovery)
            } else {
                Ok(())
            };
        }

        let vfs = self
            .registration
            .borrow()
            .as_ref()
            .ok_or(OpfsSAHError::Uninstalled)?
            .as_ptr();
        if let Some(existing) = unsafe { registered_vfs(&self.name)? } {
            if existing != vfs {
                return Err(RegisterVfsError::NameConflict(self.name.clone()).into());
            }
        }

        let lease = SyncAccessFile::acquire(self.lock_file.clone(), ".lock".into()).await?;
        self.acquire_access_handles(false).await?;

        // Check again after yielding: foreign code may have registered the name.
        let registered = unsafe { registered_vfs(&self.name)? };
        let result = if registered.is_some_and(|existing| existing != vfs) {
            Err(RegisterVfsError::NameConflict(self.name.clone()).into())
        } else {
            let code = unsafe { sqlite3_vfs_register(vfs, i32::from(self.make_default.get())) };
            if code == SQLITE_OK {
                Ok(())
            } else {
                Err(OpfsSAHError::sqlite("register VFS", code))
            }
        };

        if let Err(err) = result {
            self.release_access_handles()?;
            return Err(err);
        }
        *self.lease.borrow_mut() = Some(lease);
        self.state.set(PoolState::Active);

        Ok(())
    }

    fn clear(&self) -> Result<()> {
        self.check_state()?;
        let _operation = self.begin_operation()?;
        self.check_closed()?;

        // No await and no relinquishing ownership to other workers.
        self.needs_recovery.set(true);
        let names = self.get_filenames();
        for name in names {
            let mut files = self.map_filename_to_file.borrow_mut();
            let file = files.get(&name).unwrap();
            file.associate(None, 0)?;
            self.available_files
                .borrow_mut()
                .push(files.remove(&name).unwrap());
        }

        loop {
            let file = self.quarantined_files.borrow_mut().pop();
            let Some(file) = file else {
                break;
            };

            if let Err(err) = file.associate(None, 0) {
                self.quarantined_files.borrow_mut().push(file);
                return Err(err);
            }

            self.available_files.borrow_mut().push(file);
        }

        self.needs_recovery.set(false);

        Ok(())
    }
}

impl DbTransfer for OpfsSAHPool {
    type Error = OpfsSAHError;
    type Target<'a> = OpfsSAHImportTarget<'a>;
    type Source<'a> = OpfsSAHExportSource<'a>;

    fn open_export(&self, filename: &str) -> Result<Self::Source<'_>> {
        self.check_active()?;
        validate_filename(filename)?;

        let files = self.map_filename_to_file.borrow();
        let file = files
            .get(filename)
            .ok_or_else(|| OpfsSAHError::FileNotFound(filename.into()))?;
        if file.open_count.get() != 0 {
            return Err(OpfsSAHError::FileInUse(filename.into()));
        }

        // Conservative: even a retained PERSIST journal must be removed by the
        // caller after SQLite has recovered/closed the database.
        for suffix in ["-journal", "-wal"] {
            if let Some(sidecar) = files.get(&format!("{filename}{suffix}")) {
                if sidecar.size()? != 0 {
                    return Err(OpfsSAHError::RecoveryRequired(filename.into()));
                }
            }
        }

        let size = file.size()?;
        Ok(OpfsSAHExportSource {
            file: file.clone(),
            size,
            _operation: self.begin_operation()?,
        })
    }

    fn create_import(&self, filename: &str, size: u64) -> Result<Self::Target<'_>> {
        self.check_active()?;
        validate_db_filename(filename)?;
        physical_offset(size, 0, VfsErrorCode::IoWrite)?;

        if self.has_filename(filename) {
            return Err(OpfsSAHError::FileExists(filename.into()));
        }

        for suffix in ["-journal", "-wal"] {
            if self.has_filename(&format!("{filename}{suffix}")) {
                return Err(OpfsSAHError::RecoveryRequired(filename.into()));
            }
        }

        self.map_filename_to_file
            .borrow_mut()
            .try_reserve(1)
            .map_err(|_| OpfsSAHError::OutOfMemory)?;
        let file = self
            .available_files
            .borrow()
            .last()
            .cloned()
            .ok_or(OpfsSAHError::NoCapacity)?;

        // Keep the reserved slot counted in capacity. The operation guard
        // prevents reuse until finish/abort removes or clears it.
        Ok(OpfsSAHImportTarget {
            pool: self,
            file,
            filename: filename.into(),
            done: false,
            _operation: self.begin_operation()?,
        })
    }
}

type SyncAccessHandleAppData = Rc<OpfsSAHPool>;

thread_local! {
    // Keep paused pools discoverable independently of SQLite's registry.
    static REGISTERED_POOLS: RefCell<HashMap<String, Rc<OpfsSAHPool>>> = RefCell::new(HashMap::new());
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

        let flushed = file
            .file
            .flush()
            .map_err(|err| err.vfs_err(VfsErrorCode::IoSync));
        drop(file);

        let deleted = if options.delete_on_close() {
            Self::delete_file(data, name, false)
        } else {
            Ok(())
        };

        flushed.and(deleted)
    }

    fn record_error(data: &Self::AppData, error: VfsError) {
        data.last_error.replace(Some(error));
    }

    fn last_error(data: &Self::AppData) -> Option<VfsError> {
        data.last_error.borrow().clone()
    }

    fn open_file(
        pool: &Self::AppData,
        request: rsqlite_vfs::OpenRequest<'_>,
    ) -> VfsResult<OpenedFile<Self::File>> {
        let open = || -> Result<OpenedFile<Self::File>> {
            pool.check_active()?;

            let options = request.options;
            let temporary_name = if request.filename.is_none() {
                let mut name = None;
                for _ in 0..MAX_FILENAME_ATTEMPTS {
                    let candidate = rsqlite_vfs::random_name(|buf| pool.os.random(buf))?;
                    if !pool.has_filename(&candidate) {
                        name = Some(candidate);
                        break;
                    }
                }
                Some(name.ok_or(OpfsSAHError::NameCollision)?)
            } else {
                None
            };

            let filename = request
                .filename
                .map(|name| name.path())
                .or(temporary_name.as_deref())
                .unwrap();
            validate_filename(filename)?;

            if options.kind() == Some(FileKind::MainDb) {
                validate_db_filename(filename)?;
            }

            if pool.has_filename(filename) {
                if options.exclusive() {
                    return Err(OpfsSAHError::FileExists(filename.into()));
                }
            } else {
                if !options.create() {
                    return Err(OpfsSAHError::FileNotFound(filename.into()));
                }

                pool.with_new_file(filename, options.raw_flags(), |_| Ok(()))?;
            }

            let file = pool
                .map_filename_to_file
                .borrow()
                .get(filename)
                .unwrap()
                .clone();

            // Like upstream SAH pools, repeated opens share the underlying
            // handle. This is needed for SQLite's internal journal inspection,
            // but does not provide coordination between database connections.
            file.open_count.set(file.open_count.get() + 1);

            Ok(OpenedFile {
                file: SyncAccessFileHandle {
                    file,
                    temporary_name,
                    read_only: options.access() == OpenAccess::ReadOnly,
                    lock_level: LockLevel::None,
                },
                access: options.access(),
            })
        };

        open().map_err(|err| err.vfs_err(VfsErrorCode::CantOpen))
    }

    fn access(pool: &Self::AppData, name: &str, _mode: AccessMode) -> VfsResult<bool> {
        pool.check_active()
            .map_err(|err| err.vfs_err(VfsErrorCode::IoAccess))?;

        Ok(pool.has_filename(name))
    }

    fn full_pathname(_pool: &Self::AppData, name: &str) -> VfsResult<String> {
        validate_db_filename(name).map_err(|err| err.vfs_err(VfsErrorCode::CantOpen))?;

        Ok(name.into())
    }

    fn delete_file(pool: &Self::AppData, name: &str, _sync_dir: bool) -> VfsResult<()> {
        // Always flush the slot header, including when sync_dir is false.
        pool.delete_file(name)
            .map_err(|err| err.vfs_err(VfsErrorCode::IoDelete))?;

        Ok(())
    }
}

struct SyncAccessHandleIoMethods;

impl SQLiteIoMethods for SyncAccessHandleIoMethods {
    type Store = SyncAccessHandleStore;
}

struct SyncAccessHandleVfs;

impl SQLiteVfs<SyncAccessHandleIoMethods> for SyncAccessHandleVfs {
    type Os = dyn OsCallback;

    fn os(data: &SyncAccessHandleAppData) -> &Self::Os {
        &*data.os
    }

    const MAX_PATH_SIZE: std::os::raw::c_int = (HEADER_MAX_FILENAME_SIZE - 1) as _;
}

/// Builds an [`OpfsSAHPoolCfg`]. Validation occurs during [`install`].
pub struct OpfsSAHPoolCfgBuilder(OpfsSAHPoolCfg);

impl OpfsSAHPoolCfgBuilder {
    /// Starts with the default pool configuration.
    pub fn new() -> Self {
        Self(OpfsSAHPoolCfg::default())
    }

    /// The SQLite VFS name under which this pool's VFS is registered.
    pub fn vfs_name(mut self, name: &str) -> Self {
        self.0.vfs_name = name.into();
        self
    }

    /// Sets [`OpfsSAHPoolCfg::directory`]; see its path rules.
    pub fn directory(mut self, directory: &str) -> Self {
        self.0.directory = directory.into();
        self
    }

    /// Enables destructive first-install clearing; see [`OpfsSAHPoolCfg::clear_on_init`].
    pub fn clear_on_init(mut self, set: bool) -> Self {
        self.0.clear_on_init = set;
        self
    }

    /// Sets [`OpfsSAHPoolCfg::initial_capacity`], without shrinking larger pools.
    pub fn initial_capacity(mut self, cap: usize) -> Self {
        self.0.initial_capacity = cap;
        self
    }

    /// Returns the configuration without accessing storage or validating it.
    pub fn build(self) -> OpfsSAHPoolCfg {
        self.0
    }
}

impl Default for OpfsSAHPoolCfgBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Pool configuration, validated by [`install`].
pub struct OpfsSAHPoolCfg {
    /// Nonempty, NUL-free SQLite VFS name. Defaults to `opfs-sahpool`.
    pub vfs_name: String,
    /// OPFS-root-relative directory; defaults to `.opfs-sahpool`, independently
    /// of `vfs_name`. Empty slash-separated components are ignored; NUL, `.`,
    /// `..` and entirely empty paths are rejected.
    pub directory: String,
    /// Clears existing file contents after acquiring every slot on first install.
    /// Destructive and not atomic across files; ignored when reusing an installed pool.
    /// Defaults to `false`.
    pub clear_on_init: bool,
    /// Minimum total number of file slots at initialization.
    /// An existing larger pool is not shrunk. Defaults to six; journals also use slots.
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

/// Pool and OPFS errors. Match variants, not the human-readable display text.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum OpfsSAHError {
    #[error(transparent)]
    Backend(#[from] VfsError),
    #[error(transparent)]
    Vfs(#[from] RegisterVfsError),
    #[error(transparent)]
    ImportDb(#[from] ImportDbError),
    #[error("OPFS sync access handles require a supported dedicated worker in a secure context")]
    NotSupported,
    #[error("{operation}: {message}")]
    Opfs {
        /// Operation that failed, without including the browser's error text.
        operation: &'static str,
        /// Browser error name and message, or a fallback representation.
        message: String,
        /// Original JavaScript exception, retained for structured inspection.
        value: JsValue,
    },
    #[error("{0}")]
    InvalidFilename(&'static str),
    #[error("pool directory must contain normal path components, without NUL, '.' or '..'")]
    InvalidDirectory,
    #[error("invalid OPFS directory entry")]
    InvalidDirectoryEntry,
    #[error("file already exists: {0:?}")]
    FileExists(String),
    #[error("file not found: {0:?}")]
    FileNotFound(String),
    #[error("file is in use: {0:?}")]
    FileInUse(String),
    #[error("pool has open files; close all files before changing its lifecycle")]
    FilesInUse,
    #[error("another pool management operation is in progress")]
    Busy,
    #[error("pool is paused")]
    Paused,
    #[error("pool has been uninstalled")]
    Uninstalled,
    /// An interrupted namespace update requires reconciliation, not necessarily
    /// SQLite transaction recovery. `clear` discards all database contents.
    #[error("pool namespace needs recovery; close databases, then pause/resume or call clear to discard all data")]
    NeedsRecovery,
    #[error("existing pool uses a different directory or OS callback type")]
    ConfigurationMismatch,
    #[error("directory is already owned by another pool: {0:?}")]
    DirectoryInUse(String),
    #[error("no unused file slots; increase the pool capacity")]
    NoCapacity,
    #[error("could not generate a unique filename")]
    NameCollision,
    #[error("invalid header in OPFS file {opaque:?}: {reason}")]
    InvalidHeader {
        /// Physical OPFS filename, not the virtual database name.
        opaque: String,
        reason: &'static str,
    },
    #[error("duplicate filename in pool headers: {0:?}")]
    DuplicateFilename(String),
    #[error("{operation}: expected {expected} bytes, got {actual}")]
    ShortIo {
        operation: &'static str,
        expected: usize,
        actual: f64,
    },
    /// Sidecars prevent a standalone import/export. Their presence does not
    /// prove a hot journal; retained PERSIST journals can also trigger this.
    #[error("database has journal or WAL sidecars: {0:?}")]
    RecoveryRequired(String),
    #[error("file is too large to export into a contiguous memory buffer")]
    FileTooLarge,
    #[error("database import expected {expected} bytes, got {actual}")]
    ImportSizeMismatch { expected: u64, actual: u64 },
    #[error("database import cannot finish after a failed write")]
    ImportFailed,
    #[error(transparent)]
    Transfer(TransferError),
    #[error("unable to allocate memory for file data or pool capacity")]
    OutOfMemory,
    #[error("{error}; slot cleanup also failed: {cleanup}")]
    Cleanup {
        /// Original operation failure.
        error: Box<OpfsSAHError>,
        /// Subsequent failure while trying to reclaim the slot.
        cleanup: Box<OpfsSAHError>,
    },
}

impl From<TransferError> for OpfsSAHError {
    fn from(error: TransferError) -> Self {
        match error {
            TransferError::ImportDb(error) => Self::ImportDb(error),
            TransferError::SizeMismatch { expected, actual } => {
                Self::ImportSizeMismatch { expected, actual }
            }
            TransferError::ImportFailed => Self::ImportFailed,
            TransferError::FileTooLarge => Self::FileTooLarge,
            TransferError::OutOfMemory => Self::OutOfMemory,
            TransferError::ShortRead { expected, actual } => Self::ShortIo {
                operation: "export database",
                expected,
                actual: actual as f64,
            },
            error => Self::Transfer(error),
        }
    }
}

impl OpfsSAHError {
    fn js(operation: &'static str, value: JsValue) -> Self {
        let property = |name: &str| {
            Reflect::get(&value, &name.into())
                .ok()
                .and_then(|value| value.as_string())
        };

        let message = match (property("name"), property("message")) {
            (Some(name), Some(message)) => format!("{name}: {message}"),
            (_, Some(message)) => message,
            _ => value.as_string().unwrap_or_else(|| format!("{value:?}")),
        };

        Self::Opfs {
            operation,
            message,
            value,
        }
    }

    fn sqlite(operation: &'static str, code: i32) -> Self {
        Self::Backend(VfsError::new(
            VfsErrorCode::from_raw(code).expect("SQLite returned an error code"),
            format!("failed to {operation}").into(),
        ))
    }

    fn vfs_err(&self, fallback: VfsErrorCode) -> VfsError {
        if let Self::Backend(error) = self {
            return error.clone();
        }

        let code = match self {
            Self::OutOfMemory => VfsErrorCode::NoMemory,
            Self::Opfs { value, .. }
                if Reflect::get(value, &"name".into())
                    .ok()
                    .and_then(|name| name.as_string())
                    .as_deref()
                    == Some("QuotaExceededError") =>
            {
                VfsErrorCode::Full
            }
            _ => fallback,
        };

        VfsError::new(code, self.to_string().into())
    }
}

#[doc(hidden)]
pub struct OpfsSAHImportTarget<'a> {
    pool: &'a OpfsSAHPool,
    file: SyncAccessFile,
    filename: String,
    done: bool,
    _operation: Operation<'a>,
}

impl ImportTarget for OpfsSAHImportTarget<'_> {
    type Error = OpfsSAHError;

    fn write_at(&mut self, offset: u64, bytes: &[u8]) -> Result<()> {
        let at = physical_offset(offset, bytes.len(), VfsErrorCode::IoWrite)?;
        self.file.write_at(bytes, at)
    }

    fn commit(mut self) -> Result<()> {
        let result = (|| {
            self.file.flush()?;
            self.file
                .associate(Some(&self.filename), SQLITE_OPEN_MAIN_DB)
        })();

        if let Err(error) = result {
            return Err(self.abort_with_error(error));
        }

        let file = self.pool.available_files.borrow_mut().pop().unwrap();
        self.pool
            .map_filename_to_file
            .borrow_mut()
            .insert(self.filename.clone(), file);
        self.done = true;
        Ok(())
    }

    fn abort(mut self) -> Result<()> {
        self.cleanup()
    }

    fn abort_with_error(mut self, error: OpfsSAHError) -> OpfsSAHError {
        match self.cleanup() {
            Ok(()) => error,
            Err(cleanup) => OpfsSAHError::Cleanup {
                error: Box::new(error),
                cleanup: Box::new(cleanup),
            },
        }
    }
}

impl OpfsSAHImportTarget<'_> {
    fn cleanup(&mut self) -> Result<()> {
        self.done = true;
        if let Err(err) = self.file.associate(None, 0) {
            let file = self.pool.available_files.borrow_mut().pop().unwrap();
            self.pool.quarantined_files.borrow_mut().push(file);
            self.pool.needs_recovery.set(true);
            return Err(err);
        }

        Ok(())
    }
}

impl Drop for OpfsSAHImportTarget<'_> {
    fn drop(&mut self) {
        if !self.done {
            let _ = self.cleanup();
        }
    }
}

#[doc(hidden)]
pub struct OpfsSAHExportSource<'a> {
    file: SyncAccessFile,
    size: u64,
    _operation: Operation<'a>,
}

impl ExportSource for OpfsSAHExportSource<'_> {
    type Error = OpfsSAHError;

    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        let at = physical_offset(offset, buf.len(), VfsErrorCode::IoRead)?;
        self.file.read_at(buf, at)
    }
}

/// Management handle for one pool, confined to its dedicated worker.
///
/// Drop leaves the VFS installed. [`Self::pause`] releases OPFS locks;
/// [`Self::uninstall`] also frees registration and disables old handles.
/// Import [`VfsFilesManager`] for file management and [`DbTransfer`] for transfers.
#[derive(Clone)]
pub struct OpfsSAHPoolUtil {
    pool: Rc<OpfsSAHPool>,
}

/// Queries describe held files, returning an empty view while paused/uninstalled.
/// Removal flushes the namespace and keeps slots for reuse. Clear requires an
/// active, idle pool, but also permits namespace recovery.
impl VfsFilesManager for OpfsSAHPoolUtil {
    type Error = OpfsSAHError;

    fn remove(&self, filename: &str) -> Result<bool> {
        self.pool.delete_file(filename)
    }

    fn clear(&self) -> Result<()> {
        self.pool.clear()
    }

    fn contains(&self, filename: &str) -> bool {
        self.pool.has_filename(filename)
    }

    fn names(&self) -> Vec<String> {
        self.pool.get_filenames()
    }

    fn len(&self) -> usize {
        self.pool.get_file_count()
    }
}

/// Transfers block pool management/new opens; keep existing connections idle.
/// Export holds this guard through EOF until dropped. Dropped imports abort;
/// cleanup failure quarantines the slot and requires pool recovery.
/// Imports require new names of at most 499 UTF-8 bytes and no same-name sidecars.
/// Exports reject open files and nonempty journal/WAL files, including PERSIST
/// journals: recover/checkpoint and close first, then remove retained journals.
impl DbTransfer for OpfsSAHPoolUtil {
    type Error = OpfsSAHError;
    type Target<'a> = OpfsSAHImportTarget<'a>;
    type Source<'a> = OpfsSAHExportSource<'a>;

    fn create_import(&self, name: &str, size: u64) -> Result<Self::Target<'_>> {
        self.pool.create_import(name, size)
    }

    fn open_export(&self, name: &str) -> Result<Self::Source<'_>> {
        self.pool.open_export(name)
    }
}

impl OpfsSAHPoolUtil {
    /// Number of currently held slots, including assigned and quarantined slots.
    /// Returns zero while paused or after uninstall.
    pub fn capacity(&self) -> usize {
        self.pool.capacity()
    }

    /// Adds slots and returns the total capacity. Requires an active, idle pool.
    ///
    /// Failure/cancellation releases newly acquired handles; unused physical
    /// files may remain and are rediscovered on the next resume.
    pub async fn add_capacity(&self, n: usize) -> Result<usize> {
        self.pool.add_capacity(n).await
    }

    /// Removes up to `n` unused slots and returns the number removed.
    ///
    /// Assigned files are retained. Failure/cancellation may partially reduce
    /// capacity; an undeleted closed slot is rediscovered on the next resume.
    pub async fn reduce_capacity(&self, n: usize) -> Result<usize> {
        self.pool.reduce_capacity(n).await
    }

    /// Ensures at least `min` held slots, without shrinking an existing pool.
    pub async fn ensure_capacity(&self, min: usize) -> Result<()> {
        self.pool.ensure_capacity(min).await
    }

    /// Unregisters and releases OPFS handles, retaining data and registration
    /// memory for [`Self::resume`]. Close all databases first.
    /// Open files or active management cause failure without side effects.
    /// Already paused is a no-op.
    pub fn pause(&self) -> Result<()> {
        self.pool.pause()
    }

    /// Reacquires slots and restores registration/default status; active healthy
    /// pools are unchanged. Acquisition failure/cancellation leaves the pool
    /// paused for retry; pending acquisitions close unused handles in background.
    /// Reclaims abandoned temporary files and verified empty incomplete slots,
    /// but reports invalid persistent headers. Restores the namespace only;
    /// SQLite recovers transactions when opening a database.
    pub async fn resume(&self) -> Result<()> {
        self.pool.resume().await
    }

    /// Whether the pool is paused (not uninstalled).
    pub fn is_paused(&self) -> bool {
        self.pool.state.get() == PoolState::Paused
    }

    /// Whether this pool's SQLite registration has been permanently removed.
    pub fn is_uninstalled(&self) -> bool {
        self.pool.state.get() == PoolState::Removed
    }

    /// Unregisters and frees the VFS, releases handles, and permits reinstall.
    /// Persistent files are not deleted. Already uninstalled is a no-op.
    ///
    /// # Safety
    ///
    /// All SQLite connections using this VFS must be closed, including in-memory
    /// connections that do not open files. No saved VFS pointers, delegated
    /// wrappers or callbacks may remain in use. Serialize with all SQLite use.
    pub unsafe fn uninstall(&self) -> Result<()> {
        if self.is_uninstalled() {
            return Ok(());
        }

        let _operation = self.pool.begin_operation()?;
        self.pool.check_closed()?;

        let registration = self
            .pool
            .registration
            .borrow_mut()
            .take()
            .ok_or(OpfsSAHError::Uninstalled)?;

        // SAFETY: The caller guarantees no SQLite users or retained pointers.
        if let Err((registration, error)) = registration.unregister() {
            *self.pool.registration.borrow_mut() = Some(registration);
            return Err(error.into());
        }

        self.pool.release_access_handles()?;
        self.pool.state.set(PoolState::Removed);
        REGISTERED_POOLS.with(|pools| {
            pools.borrow_mut().remove(&self.pool.name);
        });

        Ok(())
    }
}

/// Installs or reuses a pool, validating [`OpfsSAHPoolCfg`].
/// Reuse requires the same directory and callback type; it neither resumes a
/// paused pool nor reapplies initial capacity/clearing. Each directory has one
/// owner per worker; uninstall that owner before using a different VFS name.
///
/// `default_vfs = true` promotes the VFS now or on resume; `false` never demotes.
/// Concurrent installs wait; overlapping management returns [`OpfsSAHError::Busy`].
pub async fn install<C: OsCallback + Default + 'static>(
    options: &OpfsSAHPoolCfg,
    default_vfs: bool,
) -> Result<OpfsSAHPoolUtil> {
    if options.vfs_name.is_empty() {
        return Err(RegisterVfsError::EmptyName.into());
    }

    if options.vfs_name.contains('\0') {
        return Err(RegisterVfsError::ToCStr.into());
    }

    let directory = normalize_directory(&options.directory)?;

    static REGISTER_GUARD: futures_util::lock::Mutex<()> = futures_util::lock::Mutex::new(());
    let _guard = REGISTER_GUARD.lock().await;

    let existing = REGISTERED_POOLS.with(|pools| pools.borrow().get(&options.vfs_name).cloned());
    let registered = unsafe { registered_vfs(&options.vfs_name)? };
    if let Some(pool) = existing {
        if pool.directory != directory || pool.callback_type != TypeId::of::<C>() {
            return Err(OpfsSAHError::ConfigurationMismatch);
        }

        let _operation = pool.begin_operation()?;
        let vfs = pool
            .registration
            .borrow()
            .as_ref()
            .ok_or(OpfsSAHError::Uninstalled)?
            .as_ptr();
        if registered.is_some_and(|registered| registered != vfs) {
            return Err(RegisterVfsError::NameConflict(options.vfs_name.clone()).into());
        }

        if default_vfs {
            if pool.state.get() == PoolState::Active {
                let code = unsafe { sqlite3_vfs_register(vfs, 1) };
                if code != SQLITE_OK {
                    return Err(OpfsSAHError::sqlite("set default VFS", code));
                }
            }

            pool.make_default.set(true);
        }

        drop(_operation);
        return Ok(OpfsSAHPoolUtil { pool });
    }

    if registered.is_some() {
        return Err(RegisterVfsError::NameConflict(options.vfs_name.clone()).into());
    }

    if REGISTERED_POOLS.with(|pools| {
        pools
            .borrow()
            .values()
            .any(|pool| pool.directory == directory)
    }) {
        return Err(OpfsSAHError::DirectoryInUse(directory));
    }

    let pool = OpfsSAHPool::new::<C>(options).await?;

    // SAFETY: Matching layout/store types on this SQLite worker. The module's
    // installation guard serializes its registrations. There is no yield
    // between the registry check inside register_vfs and registration.
    let registration = unsafe {
        register_vfs::<SyncAccessHandleIoMethods, SyncAccessHandleVfs>(
            &options.vfs_name,
            pool.clone(),
            default_vfs,
        )?
    };

    pool.make_default.set(default_vfs);
    *pool.registration.borrow_mut() = Some(registration);
    REGISTERED_POOLS.with(|pools| {
        pools
            .borrow_mut()
            .insert(options.vfs_name.clone(), pool.clone())
    });

    Ok(OpfsSAHPoolUtil { pool })
}

#[cfg(test)]
mod tests {
    use super::{
        install, physical_offset, OpfsSAHError, OpfsSAHPool, OpfsSAHPoolCfg, OpfsSAHPoolCfgBuilder,
        SyncAccessHandleStore, HEADER_OFFSET_DATA, MAX_SAFE_INTEGER,
    };
    use rsqlite_vfs::transfer::DbTransfer;
    use rsqlite_vfs::{
        test_suite::test_vfs_store, FileKind, LockLevel, OpenAccess, OpenOptions, OpenRequest,
        VfsAppData, VfsErrorCode, VfsFile, VfsFilesManager, VfsStore,
    };
    use sqlite_wasm_rs::{self as ffi, WasmOsCallback};
    use wasm_bindgen_test::wasm_bindgen_test;

    fn config(name: &str) -> OpfsSAHPoolCfg {
        OpfsSAHPoolCfgBuilder::new()
            .vfs_name(name)
            .directory(name)
            .clear_on_init(true)
            .build()
    }

    #[wasm_bindgen_test]
    async fn concurrent_installs_share_one_pool() {
        let options = config("test-opfs-reuse");
        let (first, second) = futures_util::future::join(
            install::<WasmOsCallback>(&options, false),
            install::<WasmOsCallback>(&options, false),
        )
        .await;
        let first = first.unwrap();
        let second = second.unwrap();
        assert!(std::rc::Rc::ptr_eq(&first.pool, &second.pool));

        unsafe {
            first.uninstall().unwrap();
        }
    }

    #[wasm_bindgen_test]
    fn physical_offsets_above_4_gib() {
        let offset = 1u64 << 32;
        assert_eq!(
            physical_offset(offset, 512, VfsErrorCode::IoWrite).unwrap(),
            (offset + HEADER_OFFSET_DATA as u64) as f64,
        );
        let max = MAX_SAFE_INTEGER - HEADER_OFFSET_DATA as u64;
        assert!(physical_offset(max, 0, VfsErrorCode::IoTruncate).is_ok());
        assert!(physical_offset(max, 1, VfsErrorCode::IoWrite).is_err());
        assert!(physical_offset(max + 1, 0, VfsErrorCode::IoTruncate).is_err());
        assert!(physical_offset(u64::MAX, 0, VfsErrorCode::IoRead).is_err());
    }

    #[wasm_bindgen_test]
    async fn size_hint_grows_but_does_not_shrink_file() {
        let pool = OpfsSAHPool::new::<WasmOsCallback>(&config("test-opfs-size-hint"))
            .await
            .unwrap();
        let options = OpenOptions::new(OpenAccess::ReadWrite, FileKind::MainDb).with_create();
        let mut file =
            SyncAccessHandleStore::open_file(&pool, OpenRequest::named("size-hint.db", options))
                .unwrap()
                .file;
        assert!(file.size_hint(2 * 8192).unwrap());
        assert_eq!(file.size().unwrap(), 2 * 8192);
        assert!(file.size_hint(8192).unwrap());
        assert_eq!(file.size().unwrap(), 2 * 8192);

        SyncAccessHandleStore::close_file(&pool, Some("size-hint.db"), file, options).unwrap();
    }

    #[wasm_bindgen_test]
    async fn store_conforms_to_vfs_contract() {
        let data = OpfsSAHPool::new::<WasmOsCallback>(&config("test-opfs-suite"))
            .await
            .unwrap();

        test_vfs_store::<SyncAccessHandleStore>(VfsAppData::new(data)).unwrap();
    }

    #[wasm_bindgen_test]
    async fn handles_protect_pool_until_last_close() {
        let config = config("test-handle-lifetimes");
        let util = install::<WasmOsCallback>(&config, false).await.unwrap();
        let flags = OpenOptions::new(OpenAccess::ReadWrite, FileKind::MainDb).with_create();
        let mut first =
            SyncAccessHandleStore::open_file(&util.pool, OpenRequest::named("handles.db", flags))
                .unwrap()
                .file;
        let mut second = SyncAccessHandleStore::open_file(
            &util.pool,
            OpenRequest::named(
                "handles.db",
                OpenOptions::new(OpenAccess::ReadOnly, FileKind::MainDb),
            ),
        )
        .unwrap()
        .file;
        assert_eq!(first.file.open_count.get(), 2);

        // A rejected open must not consume a slot or increment the open count.
        let available = util.pool.available_files.borrow().len();
        assert_eq!(
            SyncAccessHandleStore::open_file(
                &util.pool,
                OpenRequest::named("handles.db", flags.with_create_new()),
            )
            .err()
            .unwrap()
            .code(),
            VfsErrorCode::CantOpen
        );
        assert_eq!(util.pool.available_files.borrow().len(), available);
        assert_eq!(util.pool.map_filename_to_file.borrow().len(), 1);
        assert_eq!(first.file.open_count.get(), 2);

        assert!(!first.check_reserved_lock().unwrap());
        first.lock(LockLevel::Reserved).unwrap();
        assert!(first.check_reserved_lock().unwrap());
        first.unlock(LockLevel::Shared).unwrap();
        assert!(!first.check_reserved_lock().unwrap());

        first.write(&[41, 42], 0).unwrap();
        let capacity = util.capacity();
        assert!(matches!(util.pause(), Err(OpfsSAHError::FilesInUse)));
        assert!(matches!(
            util.remove("handles.db"),
            Err(OpfsSAHError::FileInUse(_))
        ));
        assert!(matches!(util.clear(), Err(OpfsSAHError::FilesInUse)));
        assert_eq!(util.capacity(), capacity);
        assert!(util.contains("handles.db"));

        SyncAccessHandleStore::close_file(&util.pool, Some("handles.db"), first, flags).unwrap();
        assert_eq!(second.file.open_count.get(), 1);

        assert_eq!(
            second.size_hint(4096).unwrap_err().code(),
            VfsErrorCode::ReadOnly
        );
        assert!(matches!(util.pause(), Err(OpfsSAHError::FilesInUse)));
        assert!(matches!(
            util.remove("handles.db"),
            Err(OpfsSAHError::FileInUse(_))
        ));
        assert!(matches!(util.clear(), Err(OpfsSAHError::FilesInUse)));
        let mut bytes = [0; 2];
        assert_eq!(second.read(&mut bytes, 0).unwrap(), 2);
        assert_eq!(bytes, [41, 42]);

        drop(second);
        assert!(util.remove("handles.db").unwrap());
        util.pause().unwrap();

        unsafe {
            util.uninstall().unwrap();
        }
    }

    #[wasm_bindgen_test]
    async fn hot_journal_recovers_uncommitted_pages() {
        let cfg = config("test-hot-journal");
        let util = install::<WasmOsCallback>(&cfg, false).await.unwrap();

        let open = |name: &std::ffi::CStr| {
            let mut db = std::ptr::null_mut();
            assert_eq!(
                unsafe {
                    ffi::sqlite3_open_v2(
                        name.as_ptr(),
                        &mut db,
                        ffi::SQLITE_OPEN_CREATE | ffi::SQLITE_OPEN_READWRITE,
                        c"test-hot-journal".as_ptr(),
                    )
                },
                ffi::SQLITE_OK
            );
            db
        };
        let exec = |db, sql: &std::ffi::CStr| {
            assert_eq!(
                unsafe {
                    ffi::sqlite3_exec(
                        db,
                        sql.as_ptr(),
                        None,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                },
                ffi::SQLITE_OK
            );
        };

        let db = open(c"source.db");
        exec(
            db,
            c"PRAGMA cache_size=5; CREATE TABLE t(n, payload);
            WITH RECURSIVE x(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM x WHERE n<100)
            INSERT INTO t SELECT 1, zeroblob(4096) FROM x;
            BEGIN IMMEDIATE; UPDATE t SET n=2;",
        );
        assert!(matches!(
            util.export_db("source.db"),
            Err(OpfsSAHError::FileInUse(_))
        ));

        // Private access is deliberate: export_db correctly rejects an open
        // database. Capture its spilled pages and hot journal to model a crash.
        let snapshot = |name: &str| {
            let files = util.pool.map_filename_to_file.borrow();
            let file = files.get(name).unwrap();
            let mut bytes = vec![0; file.size().unwrap() as usize];
            assert_eq!(
                file.read_at(&mut bytes, HEADER_OFFSET_DATA as f64).unwrap(),
                bytes.len()
            );
            bytes
        };

        let database = snapshot("source.db");
        let journal = snapshot("source.db-journal");
        assert_eq!(&journal[..8], &[217, 213, 5, 249, 32, 161, 99, 215]);

        exec(db, c"ROLLBACK");
        assert_eq!(unsafe { ffi::sqlite3_close(db) }, ffi::SQLITE_OK);
        assert_ne!(util.export_db("source.db").unwrap(), database);

        util.import_db_unchecked("recovered.db", &database).unwrap();
        util.pool
            .with_new_file(
                "recovered.db-journal",
                super::SQLITE_OPEN_MAIN_JOURNAL,
                |file| file.write_at(&journal, HEADER_OFFSET_DATA as f64),
            )
            .unwrap();

        util.pause().unwrap();
        util.resume().await.unwrap();
        assert!(matches!(
            util.export_db("recovered.db"),
            Err(OpfsSAHError::RecoveryRequired(_))
        ));

        let recovered = open(c"recovered.db");
        exec(
            recovered,
            c"CREATE TEMP TABLE verify(n CHECK(n=100));
            INSERT INTO verify SELECT count(*) FROM t WHERE n=1;
            CREATE TEMP TABLE dirty(n CHECK(n=0));
            INSERT INTO dirty SELECT count(*) FROM t WHERE n=2;",
        );
        assert_eq!(unsafe { ffi::sqlite3_close(recovered) }, ffi::SQLITE_OK);

        assert!(!util.contains("recovered.db-journal"));
        util.export_db("recovered.db").unwrap();

        unsafe {
            util.uninstall().unwrap();
        }
    }
}
