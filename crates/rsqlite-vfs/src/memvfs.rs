//! A platform-independent in-memory VFS.
//!
//! Install it with `install_unchecked`, or with `install` using a
//! `MemVfsContext` where safe access is available, then select `memvfs` by
//! name or as the default VFS.
//!
//! Files are volatile and limited by address space and memory.
//!
//! Stay on the installing thread unless the `threadsafe` feature is enabled.

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

#[cfg(feature = "threadsafe")]
use crate::sync::RegistrationGuard;
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

/// Platform services accepted by `install_unchecked` and, where available,
/// `install`, which must be `Send + Sync` with `threadsafe`.
#[cfg(not(feature = "threadsafe"))]
pub trait MemVfsOs: OsCallback + 'static {}
#[cfg(not(feature = "threadsafe"))]
impl<T: OsCallback + 'static> MemVfsOs for T {}

/// Platform services accepted by `install_unchecked` and, where available,
/// `install`, which must be `Send + Sync` with `threadsafe`.
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
    fn new(os: impl MemVfsOs, files: Locked<FileMap>) -> Self {
        Self {
            os: Shared::new(os),
            files: Shared::new(files),
            error: LastError::new(),
        }
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

/// Memory VFS context errors. Match variants rather than display text.
#[cfg(any(
    feature = "threadsafe",
    all(target_family = "wasm", not(target_feature = "atomics"))
))]
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum MemVfsContextError {
    #[error("SQLite is single-threaded or its core mutexes are unavailable")]
    SingleThreaded,
    #[error("SQLite's mutex methods cannot be verified")]
    UnverifiedMutexes,
    #[error("context construction is already in progress")]
    InitializationInProgress,
    #[error("SQLite context validation failed ({0})")]
    Sqlite(i32),
}

/// The context lease held by a safe memvfs object.
#[cfg(any(
    feature = "threadsafe",
    all(target_family = "wasm", not(target_feature = "atomics"))
))]
type ContextLease = Option<MemVfsContext>;

#[cfg(not(any(
    feature = "threadsafe",
    all(target_family = "wasm", not(target_feature = "atomics"))
)))]
#[derive(Clone)]
struct ContextLease;

#[cfg(any(
    feature = "threadsafe",
    all(target_family = "wasm", not(target_feature = "atomics"))
))]
const NO_CONTEXT: ContextLease = None;
#[cfg(not(any(
    feature = "threadsafe",
    all(target_family = "wasm", not(target_feature = "atomics"))
)))]
const NO_CONTEXT: ContextLease = ContextLease;

/// A verified environment for safe memvfs access.
///
/// Construction may initialize SQLite and run auto-extensions, while live contexts share their validation.
/// Reentrant or competing initialization returns `MemVfsContextError::InitializationInProgress` without waiting.
///
/// # Foreign interoperability
///
/// Foreign shutdown, mutex replacement and threading changes must exclude construction and every live context or derived safe object.
/// Dropping a context never shuts SQLite down.
#[cfg(feature = "threadsafe")]
pub struct MemVfsContext(alloc::sync::Arc<context_core::Core>);

#[cfg(feature = "threadsafe")]
impl MemVfsContext {
    /// Constructs or reuses the verified context for this process.
    ///
    /// # Errors
    ///
    /// Returns `MemVfsContextError` when initialization is in progress, mutex validation fails or SQLite reports an error.
    pub fn try_new() -> Result<Self, MemVfsContextError> {
        let previous = {
            let mut state = context_core::lock();
            match &*state {
                CoreState::Live(weak) => {
                    if let Some(core) = weak.upgrade() {
                        return Ok(Self(core));
                    }
                }
                CoreState::Idle => {}
                CoreState::Validating => {
                    return Err(MemVfsContextError::InitializationInProgress);
                }
            }
            core::mem::replace(&mut *state, CoreState::Validating)
        };
        struct Claim;
        impl Drop for Claim {
            fn drop(&mut self) {
                *context_core::lock() = CoreState::Idle;
            }
        }
        let claim = Claim;
        drop(previous);
        let core = validate()?;
        *context_core::lock() = CoreState::Live(alloc::sync::Arc::downgrade(&core));
        core::mem::forget(claim);
        Ok(Self(core))
    }
}

#[cfg(feature = "threadsafe")]
impl Clone for MemVfsContext {
    /// Acquires another lease on the verified core.
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// The cached verification state, guarded by a lock held for no SQLite call
/// or user callback.
#[cfg(feature = "threadsafe")]
mod context_core {
    use alloc::sync::Weak;
    use std::sync::{Mutex, MutexGuard, TryLockError};

    /// The immutable proof that the environment passed validation.
    pub(crate) struct Core;

    pub(crate) enum CoreState {
        Idle,
        Validating,
        Live(Weak<Core>),
    }

    static STATE: Mutex<CoreState> = Mutex::new(CoreState::Idle);

    pub(crate) fn lock() -> MutexGuard<'static, CoreState> {
        loop {
            match STATE.try_lock() {
                Ok(guard) => return guard,
                Err(TryLockError::Poisoned(poisoned)) => return poisoned.into_inner(),
                Err(TryLockError::WouldBlock) => core::hint::spin_loop(),
            }
        }
    }
}

#[cfg(feature = "threadsafe")]
use context_core::CoreState;

/// Verifies the current SQLite environment and builds the core proof.
///
/// C debug builds are rejected outright. Release no-op methods are detected
/// by the shared static probe pointer, and the probe connection checks the
/// core locks on a real connection before it is closed.
#[cfg(feature = "threadsafe")]
fn validate() -> Result<alloc::sync::Arc<context_core::Core>, MemVfsContextError> {
    // SAFETY: these queries read immutable build settings.
    let (threadsafe, debug) = unsafe {
        (
            bindings::sqlite3_threadsafe(),
            bindings::sqlite3_compileoption_used(c"DEBUG".as_ptr()),
        )
    };
    if threadsafe == 0 {
        return Err(MemVfsContextError::SingleThreaded);
    }
    if debug != 0 {
        return Err(MemVfsContextError::UnverifiedMutexes);
    }
    // SAFETY: static allocation initializes the mutex subsystem under the validation claim.
    let (first, second) = unsafe {
        (
            bindings::sqlite3_mutex_alloc(bindings::SQLITE_MUTEX_STATIC_APP1),
            bindings::sqlite3_mutex_alloc(bindings::SQLITE_MUTEX_STATIC_APP2),
        )
    };
    if first.is_null() || second.is_null() {
        return Err(MemVfsContextError::UnverifiedMutexes);
    }
    if first == second {
        return Err(MemVfsContextError::SingleThreaded);
    }
    // SAFETY: the validation claim serializes initialization, and foreign teardown excludes construction.
    let code = unsafe { bindings::sqlite3_initialize() };
    if code != bindings::SQLITE_OK {
        return Err(MemVfsContextError::Sqlite(code));
    }
    // Closing the probe before publication releases its VFS pointers.
    struct Probe {
        db: *mut bindings::sqlite3,
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            if !self.db.is_null() {
                // SAFETY: `db` came from `sqlite3_open_v2` and is closed
                // exactly once here.
                unsafe {
                    let _ = bindings::sqlite3_close_v2(self.db);
                }
            }
        }
    }

    let mut db: *mut bindings::sqlite3 = core::ptr::null_mut();
    let flags = bindings::SQLITE_OPEN_READONLY
        | bindings::SQLITE_OPEN_FULLMUTEX
        | bindings::SQLITE_OPEN_PRIVATECACHE;
    // SAFETY: the filename is a valid static C string and `db` is written
    // exactly once by the call.
    let code = unsafe {
        bindings::sqlite3_open_v2(c":memory:".as_ptr(), &mut db, flags, core::ptr::null())
    };
    let probe = Probe { db };
    if code != bindings::SQLITE_OK {
        return Err(MemVfsContextError::Sqlite(code));
    }
    // SAFETY: `probe.db` is the connection opened successfully above.
    if unsafe { bindings::sqlite3_db_mutex(probe.db) }.is_null() {
        return Err(MemVfsContextError::SingleThreaded);
    }
    drop(probe);
    Ok(alloc::sync::Arc::new(context_core::Core))
}

/// A verified environment for safe memvfs access.
///
/// Construction initializes the module on its sole thread without mutex probing.
///
/// # Foreign interoperability
///
/// Foreign shutdown, mutex replacement and threading changes must exclude construction and every live context or derived safe object.
/// Dropping a context never shuts SQLite down.
#[cfg(all(
    not(feature = "threadsafe"),
    target_family = "wasm",
    not(target_feature = "atomics")
))]
pub struct MemVfsContext(module_local::Lease);

#[cfg(all(
    not(feature = "threadsafe"),
    target_family = "wasm",
    not(target_feature = "atomics")
))]
impl MemVfsContext {
    /// Constructs or reuses the module-local context.
    ///
    /// # Errors
    ///
    /// Returns `MemVfsContextError` when initialization is in progress or SQLite reports an error.
    pub fn try_new() -> Result<Self, MemVfsContextError> {
        use core::sync::atomic::Ordering;
        use module_local::{Lease, IDLE, LIVE, STATE, VALIDATING};

        match STATE.load(Ordering::Acquire) {
            LIVE => return Ok(Self(Lease::reserved())),
            VALIDATING => {
                return Err(MemVfsContextError::InitializationInProgress);
            }
            _ => {}
        }
        match STATE.compare_exchange(IDLE, VALIDATING, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => {}
            Err(VALIDATING) => return Err(MemVfsContextError::InitializationInProgress),
            Err(LIVE) => return Ok(Self(Lease::reserved())),
            Err(_) => unreachable!(),
        }
        // SAFETY: the claimed validation state excludes concurrent
        // construction and reconfiguration on the module's single thread.
        let code = unsafe { bindings::sqlite3_initialize() };
        if code != bindings::SQLITE_OK {
            STATE.store(IDLE, Ordering::Release);
            return Err(MemVfsContextError::Sqlite(code));
        }
        module_local::LEASES.store(1, Ordering::Release);
        STATE.store(LIVE, Ordering::Release);
        Ok(Self(Lease::first()))
    }
}

#[cfg(all(
    not(feature = "threadsafe"),
    target_family = "wasm",
    not(target_feature = "atomics")
))]
impl Clone for MemVfsContext {
    /// Acquires another lease on the module context.
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// Validation state for a module with one execution thread.
#[cfg(all(
    not(feature = "threadsafe"),
    target_family = "wasm",
    not(target_feature = "atomics")
))]
mod module_local {
    use core::marker::PhantomData;
    use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

    pub(crate) const IDLE: i32 = 0;
    pub(crate) const VALIDATING: i32 = 1;
    pub(crate) const LIVE: i32 = 2;

    pub(crate) static STATE: AtomicI32 = AtomicI32::new(IDLE);
    pub(crate) static LEASES: AtomicUsize = AtomicUsize::new(0);

    /// A lease on the module's verified environment, module-local by design.
    pub(crate) struct Lease(PhantomData<*const ()>);

    impl Lease {
        /// Reserves another lease on the live context.
        pub(crate) fn reserved() -> Self {
            LEASES.fetch_add(1, Ordering::Relaxed);
            Self(PhantomData)
        }

        /// Creates the lease whose count the publisher initialized.
        pub(crate) fn first() -> Self {
            Self(PhantomData)
        }
    }

    impl Clone for Lease {
        fn clone(&self) -> Self {
            Self::reserved()
        }
    }

    impl Drop for Lease {
        fn drop(&mut self) {
            if LEASES.fetch_sub(1, Ordering::Relaxed) == 1 {
                // Retired leases require fresh initialization validation.
                STATE.store(IDLE, Ordering::Relaxed);
            }
        }
    }
}

/// A management handle that keeps memory files alive after VFS uninstallation.
///
/// A handle obtained through the context also holds its lease, so the
/// handle's file data and mutexes retire before the lease. After
/// reinstallation, obtain a new handle to access the new VFS instance.
///
/// Import [`VfsFilesManager`] for file management and [`DbTransfer`] for
/// transfers.
pub struct MemVfsUtil {
    inner: MemAppData,
    lease: ContextLease,
}

impl MemVfsUtil {
    /// Gets the installed memory VFS without registering it or changing the
    /// default VFS. SQLite's own automatic initialization may still run.
    ///
    /// # Safety
    ///
    /// Without `threadsafe`, all SQLite use of memvfs must stay on this thread.
    /// With `threadsafe`, preserve SQLite's global threading rules and real mutex methods for every returned object.
    /// Exclude uninstallation and SQLite shutdown during lookup.
    pub unsafe fn get_unchecked() -> Result<Self> {
        // SAFETY: the caller supplies the SQLite environment and excludes teardown.
        let inner = unsafe { get_inner()? };
        Ok(Self {
            inner,
            lease: NO_CONTEXT,
        })
    }

    /// Gets the installed memory VFS using the verified context.
    ///
    /// The returned handle holds a context lease. SQLite's own automatic
    /// initialization may still run.
    #[cfg(any(
        feature = "threadsafe",
        all(target_family = "wasm", not(target_feature = "atomics"))
    ))]
    pub fn get(context: &MemVfsContext) -> Result<Self> {
        // SAFETY: the context keeps core locking valid, and unsafe uninstallation excludes this call.
        let inner = unsafe { get_inner()? };
        Ok(Self {
            inner,
            lease: Some(context.clone()),
        })
    }
}

unsafe fn get_inner() -> Result<MemAppData> {
    // SAFETY: the caller provides the SQLite environment and excludes uninstallation.
    let vfs = unsafe { bindings::sqlite3_vfs_find(VFS_NAME.as_ptr()) };
    if vfs.is_null() {
        return Err(MemVfsError::NotInstalled);
    }
    check_owned(vfs)?;
    // SAFETY: check_owned proved the registration is owned by this crate, so
    // the vfs app data has the required type and alignment and outlives this.
    let data = unsafe { VfsAppData::<MemAppData>::get(vfs).data.clone() };
    Ok(data)
}

impl VfsFilesManager for MemVfsUtil {
    type Error = Infallible;

    fn remove(&self, filename: &str) -> Result<bool, Self::Error> {
        Ok(self.inner.files.lock().remove(filename).is_some())
    }

    fn clear(&self) -> Result<(), Self::Error> {
        core::mem::take(&mut *self.inner.files.lock());
        Ok(())
    }

    fn contains(&self, filename: &str) -> Result<bool, Self::Error> {
        Ok(self.inner.files.lock().contains_key(filename))
    }

    fn names(&self) -> Result<Vec<String>, Self::Error> {
        Ok(self.inner.files.lock().keys().cloned().collect())
    }

    fn len(&self) -> Result<usize, Self::Error> {
        Ok(self.inner.files.lock().len())
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
        if self.inner.files.lock().contains_key(name) {
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
            .inner
            .files
            .lock()
            .get(name)
            .cloned()
            .ok_or_else(|| MemVfsError::NotFound(name.into()))?;
        let size = file.lock().data.size()?;
        // The lease goes last so the export's data and mutexes retire
        // before it.
        Ok(MemExportSource {
            file,
            size,
            lease: self.lease.clone(),
        })
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
        let mut files = self.util.inner.files.lock();
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
    #[expect(dead_code, reason = "lease is held until the export drops")]
    lease: ContextLease,
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
/// Use a valid SQLite context. Without `threadsafe`, installation, management,
/// file access and uninstallation must stay on one thread. With it, configured
/// SQLite mutexes must provide real mutual exclusion. Installations serialize internally.
/// Exclude uninstallation and SQLite shutdown while this call runs.
/// Preserve SQLite's global threading rules and mutex lifetimes for every returned object.
/// Raw unregistration is allowed, but never free or replace owned VFS/app data
/// through raw pointers. Use [`uninstall`] for cleanup.
pub unsafe fn install_unchecked(
    os: impl MemVfsOs,
    default_vfs: bool,
) -> Result<MemVfsUtil, RegisterVfsError> {
    let inner = install_inner(os, default_vfs)?;
    Ok(MemVfsUtil {
        inner,
        lease: NO_CONTEXT,
    })
}

/// Installs memvfs using the verified context.
///
/// Reuses the owned registration, services and data if present, and
/// re-registers the same allocation if raw SQLite unregistration detached
/// it. The returned handle holds a context lease.
#[cfg(any(
    feature = "threadsafe",
    all(target_family = "wasm", not(target_feature = "atomics"))
))]
pub fn install(
    context: &MemVfsContext,
    os: impl MemVfsOs,
    default_vfs: bool,
) -> Result<MemVfsUtil, RegisterVfsError> {
    // SAFETY: the context verified the environment, and the unchecked
    // contract excludes shutdown while installation runs.
    let inner = unsafe { install_inner(os, default_vfs)? };
    Ok(MemVfsUtil {
        inner,
        lease: Some(context.clone()),
    })
}

unsafe fn install_inner(
    os: impl MemVfsOs,
    default_vfs: bool,
) -> Result<MemAppData, RegisterVfsError> {
    #[cfg(feature = "threadsafe")]
    let registration = RegistrationGuard::enter()?;
    let registered = bindings::sqlite3_vfs_find(VFS_NAME.as_ptr());
    if !registered.is_null() {
        check_owned(registered)?;
    }
    // Lookup may initialize SQLite and install memvfs through sqlite3_os_init.
    // Read ownership afterwards, independently of registry membership.
    let owned = MEM_VFS.load(Ordering::Acquire);

    let vfs = if owned.is_null() {
        // Keep the callback unconsumed until fallible allocation succeeds.
        let files = Locked::new(FileMap::new())
            .map_err(|error| RegisterVfsError::RegisterVfs(error.code()))?;
        let data = VfsAppData::new(MemAppData::new(os, files)).leak();
        // SAFETY: the name is static and MemVfs/MemIoMethods use the matching app-data and file layout.
        let vfs = Box::into_raw(Box::new(MemVfs::vfs(VFS_NAME.as_ptr(), data)));
        // Ownership must be visible to lookups as soon as SQLite exposes the registration.
        MEM_VFS.store(vfs, Ordering::Release);
        let code = bindings::sqlite3_vfs_register(vfs, i32::from(default_vfs));
        if code != bindings::SQLITE_OK {
            MEM_VFS.store(core::ptr::null_mut(), Ordering::Release);
            #[cfg(feature = "threadsafe")]
            drop(registration);
            drop(Box::from_raw(vfs));
            drop(VfsAppData::from_raw(data));
            return Err(RegisterVfsError::RegisterVfs(
                VfsErrorCode::from_raw(code).expect("SQLite must return a valid error code"),
            ));
        }
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

    // SAFETY: the registration is owned by this crate, so the app data has
    // the required type, alignment and lifetime.
    Ok(VfsAppData::<MemAppData>::get(vfs).data.clone())
}

fn check_owned(vfs: *mut bindings::sqlite3_vfs) -> Result<(), RegisterVfsError> {
    if vfs != MEM_VFS.load(Ordering::Acquire) {
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
/// Use a valid SQLite context on the installing thread unless `threadsafe` is enabled, and exclude safe and unchecked lookup, installation, context construction, uninstallation and SQLite initialization.
/// Close all files and retire VFS/app-data references, while `MemVfsUtil` handles may remain alive.
/// Owned allocations must not have been freed or replaced through raw pointers.
pub unsafe fn uninstall() -> Result<(), RegisterVfsError> {
    let registered = bindings::sqlite3_vfs_find(VFS_NAME.as_ptr());
    let vfs = MEM_VFS.load(Ordering::Acquire);

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

    MEM_VFS.store(core::ptr::null_mut(), Ordering::Release);
    // Reconstitute both owners before running backend destructors.
    let vfs = Box::from_raw(vfs);
    let data = Box::from_raw(vfs.pAppData.cast::<VfsAppData<MemAppData>>());
    drop(data);
    drop(vfs);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{FileMap, Locked, MemAppData, MemStore};
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

        let data = MemAppData::new(CallbackOs, Locked::new(FileMap::new()).unwrap());
        test_vfs_store::<MemStore>(VfsAppData::new(data)).unwrap();
    }
}
