//! A platform-independent in-memory VFS.
//!
//! Call [`install`] before use; select `memvfs` by name or install as default.
//! Files are volatile and limited by address space and memory.
//!
//! Every database carries real lock levels, so several connections may share
//! one. By default, stay on the installing thread. With the `threadsafe`
//! feature, SQLite's mutexes guard the file map and every file, so those
//! connections may live on different threads.

use crate::ffi as bindings;
use crate::transfer::{DbTransfer, ExportSource, ImportTarget, TransferError};

use crate::{
    AccessMode, FileKind, LockLevel, MemChunksFile, OpenAccess, OpenOptions, OpenedFile,
    OsCallback, RegisterVfsError, SQLiteIoMethods, SQLiteVfs, SyncOptions, VfsAppData, VfsError,
    VfsErrorCode, VfsFile, VfsFilesManager, VfsResult, VfsStore,
};

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::convert::Infallible;
use core::ffi::CStr;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::sync::{LastError, Locked, Shared};

const VFS_NAME: &CStr = c"memvfs";
const MAX_PATH_SIZE: i32 = 1024;
// SQLite may append '-' followed by up to 11 characters (e.g. super-journals).
const MAX_DB_FILENAME_SIZE: usize = MAX_PATH_SIZE as usize - 12;

// Records our allocation, so an unrelated VFS with the same name is never cast
// to MemAppData or freed by uninstall. This does not make the VFS thread-safe.
static MEM_VFS: AtomicPtr<bindings::sqlite3_vfs> = AtomicPtr::new(core::ptr::null_mut());

type Result<T, E = MemVfsError> = core::result::Result<T, E>;

fn validate_db_filename(name: &str) -> Result<()> {
    if name.is_empty() || name.as_bytes().contains(&0) || name.len() > MAX_DB_FILENAME_SIZE {
        return Err(MemVfsError::InvalidFilename);
    }

    Ok(())
}

/// Platform services accepted by [`install`], which must be `Send + Sync` with `threadsafe`.
#[cfg(not(feature = "threadsafe"))]
pub trait MemVfsOs: OsCallback + 'static {}
#[cfg(not(feature = "threadsafe"))]
impl<T: OsCallback + 'static> MemVfsOs for T {}

/// Platform services accepted by [`install`], which must be `Send + Sync` with `threadsafe`.
#[cfg(feature = "threadsafe")]
pub trait MemVfsOs: OsCallback + Send + Sync + 'static {}
#[cfg(feature = "threadsafe")]
impl<T: OsCallback + Send + Sync + 'static> MemVfsOs for T {}

type FileRef = Shared<Locked<MemFile>>;
type FileMap = BTreeMap<String, FileRef>;

/// File bytes and the lock levels of every connection to it.
struct MemFile {
    data: MemChunksFile,
    locks: LockTable,
}

impl MemFile {
    fn shared(data: MemChunksFile) -> VfsResult<FileRef> {
        let file = MemFile {
            data,
            locks: LockTable::default(),
        };
        Ok(Shared::new(Locked::new(file)?))
    }
}

/// Lock levels across all connections to one file, following SQLite's unix VFS.
#[derive(Default)]
struct LockTable {
    /// Connections holding `Shared` or stronger.
    readers: usize,
    /// Whether a connection holds `Reserved` or stronger.
    writer: bool,
    /// Whether a connection holds `Pending` or `Exclusive`, which turns new readers away.
    pending: bool,
}

impl LockTable {
    fn busy() -> VfsError {
        VfsError::new(VfsErrorCode::Busy, "database is locked".into())
    }

    fn lock(&mut self, held: &mut LockLevel, level: LockLevel) -> VfsResult<()> {
        if level <= *held {
            return Ok(());
        }
        if *held == LockLevel::None {
            if level != LockLevel::Shared {
                // SQLite always takes a shared lock before any stronger one.
                return Err(VfsError::new(
                    VfsErrorCode::IoLock,
                    "lock upgrade without a shared lock".into(),
                ));
            }
            if self.pending {
                return Err(Self::busy());
            }
            self.readers += 1;
            *held = LockLevel::Shared;
            return Ok(());
        }
        if *held == LockLevel::Shared {
            if self.writer {
                return Err(Self::busy());
            }
            self.writer = true;
            *held = LockLevel::Reserved;
        }
        if level >= LockLevel::Pending && *held == LockLevel::Reserved {
            self.pending = true;
            *held = LockLevel::Pending;
        }
        if level == LockLevel::Exclusive {
            // Readers already inside drain while `pending` keeps new ones out.
            if self.readers > 1 {
                return Err(Self::busy());
            }
            *held = LockLevel::Exclusive;
        }
        Ok(())
    }

    fn unlock(&mut self, held: &mut LockLevel, level: LockLevel) {
        if level >= *held {
            return;
        }
        if *held >= LockLevel::Pending && level < LockLevel::Pending {
            self.pending = false;
        }
        if *held >= LockLevel::Reserved && level < LockLevel::Reserved {
            self.writer = false;
        }
        if level == LockLevel::None {
            // `held` is at least `Shared` here, so this connection is counted in `readers`.
            self.readers -= 1;
        }
        *held = level;
    }
}

#[derive(Clone)]
struct MemAppData {
    os: Shared<dyn MemVfsOs>,
    files: Shared<Locked<FileMap>>,
    error: LastError,
}

impl MemAppData {
    fn new(os: impl MemVfsOs) -> VfsResult<Self> {
        Ok(Self {
            os: Shared::new(os),
            files: Shared::new(Locked::new(FileMap::new())?),
            error: LastError::new(),
        })
    }
}

/// An independent open instance sharing only the underlying file data.
struct MemFileHandle {
    file: FileRef,
    read_only: bool,
    level: LockLevel,
}

impl MemFileHandle {
    fn new(file: FileRef, read_only: bool) -> Self {
        Self {
            file,
            read_only,
            level: LockLevel::None,
        }
    }

    fn check_writable(&self) -> VfsResult<()> {
        if self.read_only {
            return Err(VfsError::new(
                VfsErrorCode::ReadOnly,
                "file is read-only".into(),
            ));
        }
        Ok(())
    }
}

impl VfsFile for MemFileHandle {
    fn read(&mut self, buf: &mut [u8], offset: u64) -> VfsResult<usize> {
        self.file.lock().data.read(buf, offset)
    }

    fn write(&mut self, buf: &[u8], offset: u64) -> VfsResult<()> {
        self.check_writable()?;
        self.file.lock().data.write(buf, offset)
    }

    fn truncate(&mut self, size: u64) -> VfsResult<()> {
        self.check_writable()?;
        self.file.lock().data.truncate(size)
    }

    fn sync(&mut self, options: SyncOptions) -> VfsResult<()> {
        self.file.lock().data.sync(options)
    }

    fn lock(&mut self, level: LockLevel) -> VfsResult<()> {
        self.file.lock().locks.lock(&mut self.level, level)
    }

    fn unlock(&mut self, level: LockLevel) -> VfsResult<()> {
        self.file.lock().locks.unlock(&mut self.level, level);
        Ok(())
    }

    fn check_reserved_lock(&self) -> VfsResult<bool> {
        Ok(self.file.lock().locks.writer)
    }

    fn size(&self) -> VfsResult<u64> {
        self.file.lock().data.size()
    }
}

#[derive(Copy, Clone, Default)]
struct MemStore;

impl VfsStore for MemStore {
    type File = MemFileHandle;
    type AppData = MemAppData;

    fn record_error(data: &MemAppData, error: VfsError) {
        data.error.set(error);
    }

    fn last_error(data: &MemAppData) -> Option<VfsError> {
        data.error.get()
    }

    fn open_file(
        app_data: &MemAppData,
        request: crate::OpenRequest<'_>,
    ) -> VfsResult<OpenedFile<MemFileHandle>> {
        let options = request.options;
        let Some(filename) = request.filename else {
            return Ok(OpenedFile {
                file: MemFileHandle::new(
                    MemFile::shared(MemChunksFile::default())?,
                    options.access() == OpenAccess::ReadOnly,
                ),
                access: options.access(),
            });
        };
        let name = filename.path();

        if options.kind() == Some(FileKind::MainDb) {
            validate_db_filename(name)
                .map_err(|err| VfsError::new(VfsErrorCode::CantOpen, format!("{err}").into()))?;
        }

        let mut files = app_data.files.lock();
        let file = match files.get(name) {
            Some(_) if options.exclusive() => {
                return Err(VfsError::new(
                    VfsErrorCode::CantOpen,
                    format!("file already exists: {name:?}").into(),
                ));
            }
            Some(file) => file.clone(),
            None if options.create() => {
                let file = if options.kind() == Some(FileKind::MainDb) {
                    MemChunksFile::waiting_for_write()
                } else {
                    MemChunksFile::default()
                };
                let file = MemFile::shared(file)?;
                files.insert(name.into(), file.clone());
                file
            }
            None => {
                return Err(VfsError::new(
                    VfsErrorCode::CantOpen,
                    format!("file not found: {name:?}").into(),
                ));
            }
        };
        Ok(OpenedFile {
            file: MemFileHandle::new(file, options.access() == OpenAccess::ReadOnly),
            access: options.access(),
        })
    }

    fn close_file(
        app_data: &MemAppData,
        name: Option<&str>,
        mut file: MemFileHandle,
        options: OpenOptions,
    ) -> VfsResult<()> {
        // SQLite may close a file while it still holds a lock.
        file.unlock(LockLevel::None)?;
        let Some(name) = name else {
            return Ok(());
        };
        if options.delete_on_close() {
            let mut files = app_data.files.lock();
            // Do not delete a replacement created under the same name.
            if files
                .get(name)
                .is_some_and(|current| Shared::ptr_eq(current, &file.file))
            {
                files.remove(name);
            }
        }
        Ok(())
    }

    fn access(app_data: &MemAppData, file: &str, _mode: AccessMode) -> VfsResult<bool> {
        // Every file in this flat memory namespace is readable and writable.
        Ok(app_data.files.lock().contains_key(file))
    }

    fn full_pathname(_data: &MemAppData, name: &str) -> VfsResult<String> {
        validate_db_filename(name)
            .map_err(|err| VfsError::new(VfsErrorCode::CantOpen, format!("{err}").into()))?;

        Ok(name.into())
    }

    fn delete_file(app_data: &MemAppData, file: &str, _sync_dir: bool) -> VfsResult<()> {
        if app_data.files.lock().remove(file).is_none() {
            return Err(VfsError::new(
                VfsErrorCode::IoDelete,
                format!("file not found: {file:?}").into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Default)]
struct MemIoMethods;

impl SQLiteIoMethods for MemIoMethods {
    type Store = MemStore;
}

#[derive(Clone, Copy, Default)]
struct MemVfs;

impl SQLiteVfs<MemIoMethods> for MemVfs {
    type Os = dyn MemVfsOs;

    fn os(data: &MemAppData) -> &Self::Os {
        &*data.os
    }

    const MAX_PATH_SIZE: ::core::ffi::c_int = MAX_PATH_SIZE;
}

/// Memory VFS management errors. Match variants rather than display text.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum MemVfsError {
    #[error("memory VFS is not installed")]
    NotInstalled,
    #[error(transparent)]
    Registration(#[from] RegisterVfsError),
    #[error("filename must be nonempty, NUL-free and at most 1012 UTF-8 bytes")]
    InvalidFilename,
    #[error("file already exists: {0:?}")]
    AlreadyExists(String),
    #[error("file not found: {0:?}")]
    NotFound(String),
    #[error(transparent)]
    Io(#[from] VfsError),
    #[error(transparent)]
    Transfer(#[from] TransferError),
}

/// A management handle that keeps memory files alive after VFS uninstallation.
///
/// After reinstallation, obtain a new handle to access the new VFS instance.
/// Import [`VfsFilesManager`] for file management and [`DbTransfer`] for transfers.
pub struct MemVfsUtil(MemAppData);

impl MemVfsUtil {
    /// Gets the installed memory VFS without registering it or changing the
    /// default VFS. SQLite's own automatic initialization may still run.
    ///
    /// # Safety
    ///
    /// Call with serialized access to SQLite VFS registration. Without the
    /// `threadsafe` feature, all SQLite use of memvfs must stay on this thread.
    pub unsafe fn get() -> Result<Self> {
        let vfs = bindings::sqlite3_vfs_find(VFS_NAME.as_ptr());
        if vfs.is_null() {
            return Err(MemVfsError::NotInstalled);
        }
        check_owned(vfs)?;
        Ok(Self(VfsAppData::<MemAppData>::get(vfs).data.clone()))
    }
}

impl VfsFilesManager for MemVfsUtil {
    type Error = Infallible;

    fn remove(&self, filename: &str) -> Result<bool, Self::Error> {
        Ok(self.0.files.lock().remove(filename).is_some())
    }

    fn clear(&self) -> Result<(), Self::Error> {
        core::mem::take(&mut *self.0.files.lock());
        Ok(())
    }

    fn contains(&self, filename: &str) -> Result<bool, Self::Error> {
        Ok(self.0.files.lock().contains_key(filename))
    }

    fn names(&self) -> Result<Vec<String>, Self::Error> {
        Ok(self.0.files.lock().keys().cloned().collect())
    }

    fn len(&self) -> Result<usize, Self::Error> {
        Ok(self.0.files.lock().len())
    }
}

/// Imports use 4 KiB memory chunks independently of SQLite's page size.
/// New names must be nonempty, NUL-free and at most 1012 UTF-8 bytes.
impl DbTransfer for MemVfsUtil {
    type Error = MemVfsError;
    type Target<'a> = MemImportTarget<'a>;
    type Source<'a> = MemExportSource;

    fn create_import(&self, name: &str, size: u64) -> Result<Self::Target<'_>> {
        validate_db_filename(name)?;
        if self.0.files.lock().contains_key(name) {
            return Err(MemVfsError::AlreadyExists(name.into()));
        }
        usize::try_from(size).map_err(|_| {
            VfsError::new(VfsErrorCode::Full, "file size exceeds address space".into())
        })?;

        Ok(MemImportTarget {
            util: self,
            filename: name.into(),
            file: MemChunksFile::new(4096),
        })
    }

    fn open_export(&self, name: &str) -> Result<Self::Source<'_>> {
        let file = self
            .0
            .files
            .lock()
            .get(name)
            .cloned()
            .ok_or_else(|| MemVfsError::NotFound(name.into()))?;
        let size = file.lock().data.size()?;
        Ok(MemExportSource { file, size })
    }
}

#[doc(hidden)]
pub struct MemImportTarget<'a> {
    util: &'a MemVfsUtil,
    filename: String,
    file: MemChunksFile,
}

impl ImportTarget for MemImportTarget<'_> {
    type Error = MemVfsError;

    fn write_at(&mut self, offset: u64, bytes: &[u8]) -> Result<()> {
        self.file.write(bytes, offset)?;
        Ok(())
    }

    fn commit(self) -> Result<()> {
        let file = MemFile::shared(self.file)?;
        let mut files = self.util.0.files.lock();
        if files.contains_key(&self.filename) {
            return Err(MemVfsError::AlreadyExists(self.filename));
        }
        files.insert(self.filename, file);
        Ok(())
    }

    fn abort(self) -> Result<()> {
        Ok(())
    }

    fn abort_with_error(self, error: MemVfsError) -> MemVfsError {
        error
    }
}

/// Reads a byte snapshot per call. A writer's uncommitted pages may be visible,
/// so export only while no transaction writes.
#[doc(hidden)]
pub struct MemExportSource {
    file: FileRef,
    size: u64,
}

impl ExportSource for MemExportSource {
    type Error = MemVfsError;

    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        Ok(self.file.lock().data.read(buf, offset)?)
    }
}

/// Installs memvfs, reusing its owned registration, services and data if present.
///
/// Re-registers the same allocation if raw SQLite unregistration detached it.
///
/// # Safety
///
/// Use a valid SQLite context with serialized registration. Without the
/// `threadsafe` feature, installation, management, file access and
/// uninstallation must stay on one thread. With it, file access and management
/// may come from any thread, while installation and uninstallation stay
/// serialized with SQLite initialization and with each other.
/// Raw unregistration is allowed; never free or replace owned VFS/app data
/// through raw pointers. Use [`uninstall`] for cleanup.
pub unsafe fn install(
    os: impl MemVfsOs,
    default_vfs: bool,
) -> Result<MemVfsUtil, RegisterVfsError> {
    let registered = bindings::sqlite3_vfs_find(VFS_NAME.as_ptr());
    if !registered.is_null() {
        check_owned(registered)?;
    }
    // Lookup may initialize SQLite and install memvfs through sqlite3_os_init.
    // Read ownership afterwards, independently of registry membership.
    let owned = MEM_VFS.load(Ordering::Relaxed);

    let vfs = if owned.is_null() {
        // SAFETY: The name is static, app data is retained until uninstall,
        // and MemVfs/MemIoMethods use the matching default file layout.
        // The caller guarantees serialized access to the SQLite context.
        let data =
            MemAppData::new(os).map_err(|error| RegisterVfsError::RegisterVfs(error.code()))?;
        let data = VfsAppData::new(data).leak();
        let vfs = Box::into_raw(Box::new(MemVfs::vfs(VFS_NAME.as_ptr(), data)));
        let code = bindings::sqlite3_vfs_register(vfs, i32::from(default_vfs));
        if code != bindings::SQLITE_OK {
            drop(Box::from_raw(vfs));
            drop(VfsAppData::from_raw(data));
            return Err(RegisterVfsError::RegisterVfs(
                VfsErrorCode::from_raw(code).expect("SQLite must return a valid error code"),
            ));
        }
        MEM_VFS.store(vfs, Ordering::Relaxed);
        vfs
    } else {
        if registered.is_null() || default_vfs {
            let code = bindings::sqlite3_vfs_register(owned, i32::from(default_vfs));
            if code != bindings::SQLITE_OK {
                return Err(RegisterVfsError::RegisterVfs(
                    VfsErrorCode::from_raw(code).expect("SQLite must return a valid error code"),
                ));
            }
        }
        owned
    };

    Ok(MemVfsUtil(VfsAppData::<MemAppData>::get(vfs).data.clone()))
}

fn check_owned(vfs: *mut bindings::sqlite3_vfs) -> Result<(), RegisterVfsError> {
    if vfs != MEM_VFS.load(Ordering::Relaxed) {
        return Err(RegisterVfsError::NameConflict("memvfs".into()));
    }
    Ok(())
}

/// Frees the owned registration, even if already unregistered through SQLite.
///
/// Leaves any same-name replacement alone. [`MemVfsUtil`] handles retain their file data.
///
/// # Safety
///
/// Use a valid SQLite context, on the installing thread unless `threadsafe` is
/// enabled, and serialize with VFS installation/uninstallation and with SQLite
/// initialization. Close all files and retire VFS/app-data
/// references (`MemVfsUtil` may outlive uninstall). Owned allocations must
/// not have been freed or replaced through raw pointers.
pub unsafe fn uninstall() -> Result<(), RegisterVfsError> {
    let registered = bindings::sqlite3_vfs_find(VFS_NAME.as_ptr());
    let vfs = MEM_VFS.load(Ordering::Relaxed);

    if vfs.is_null() {
        // A same-name VFS belongs to someone else.
        if !registered.is_null() {
            check_owned(registered)?;
        }
        return Ok(());
    }

    let code = bindings::sqlite3_vfs_unregister(vfs);
    if code != bindings::SQLITE_OK {
        return Err(RegisterVfsError::UnregisterVfs(
            VfsErrorCode::from_raw(code).expect("SQLite must return a valid error code"),
        ));
    }

    MEM_VFS.store(core::ptr::null_mut(), Ordering::Relaxed);
    // Reconstitute both owners before running backend destructors.
    let vfs = Box::from_raw(vfs);
    let data = Box::from_raw(vfs.pAppData.cast::<VfsAppData<MemAppData>>());
    drop(data);
    drop(vfs);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MemAppData, MemStore};
    use crate::{test_suite::test_vfs_store, OsCallback, VfsAppData, VfsResult};

    #[test]
    fn test_memory_vfs_store() {
        struct CallbackOs;

        impl OsCallback for CallbackOs {
            fn sleep(&self, _: core::time::Duration) {}

            fn random(&self, buf: &mut [u8]) -> usize {
                assert!(!buf.is_empty());
                buf.fill(42);
                buf.len()
            }

            fn epoch_timestamp_in_ms(&self) -> VfsResult<i64> {
                Ok(0)
            }
        }

        let data = MemAppData::new(CallbackOs).unwrap();
        test_vfs_store::<MemStore>(VfsAppData::new(data)).unwrap();
    }
}
