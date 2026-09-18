//! A platform-independent, single-threaded in-memory VFS.
//!
//! Call [`install`] with platform services before use. Select `memvfs` by name
//! when opening a database, or pass `true` to `install` to make it the default VFS.
//!
//! Files are not persistent and are limited by the platform's address space and
//! available memory. All use must stay on the installing thread. File locks are
//! no-ops. Multiple simultaneous connections to the same database are unsupported;
//! repeated opens share file data without enforcing this restriction.

use crate::ffi as bindings;

use crate::{
    AccessMode, FileKind, ImportDbError, LockLevel, MemChunksFile, OpenAccess, OpenOptions,
    OpenedFile, OsCallback, SQLiteIoMethods, SQLiteVfs, SyncOptions, VfsAppData, VfsError,
    VfsErrorCode, VfsFile, VfsResult, VfsStore, check_db_and_page_size, check_import_db,
};

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::ffi::CStr;
use core::sync::atomic::{AtomicPtr, Ordering};

const VFS_NAME: &CStr = c"memvfs";
// SQLite's pager uses page-sized scratch space for super-journal names.
// Keep paths within even the minimum supported 512-byte database page.
const MAX_PATH_SIZE: i32 = 512;
// SQLite may append '-' followed by up to 11 characters (e.g. super-journals).
const MAX_DB_FILENAME_SIZE: usize = MAX_PATH_SIZE as usize - 12;

// Records our allocation, so an unrelated VFS with the same name is never cast
// to MemAppData or freed by uninstall. This does not make the VFS thread-safe.
static MEM_VFS: AtomicPtr<bindings::sqlite3_vfs> = AtomicPtr::new(core::ptr::null_mut());

type Result<T> = core::result::Result<T, MemVfsError>;

fn validate_db_filename(name: &str) -> Result<()> {
    if name.is_empty() || name.as_bytes().contains(&0) || name.len() > MAX_DB_FILENAME_SIZE {
        return Err(MemVfsError::InvalidFilename);
    }

    Ok(())
}

#[derive(Clone)]
struct MemAppData {
    os: Rc<dyn OsCallback>,
    files: Rc<RefCell<BTreeMap<String, Rc<RefCell<MemChunksFile>>>>>,
    error: Rc<RefCell<Option<VfsError>>>,
}

impl MemAppData {
    fn new(os: impl OsCallback + 'static) -> Self {
        Self {
            os: Rc::new(os),
            files: Rc::default(),
            error: Rc::default(),
        }
    }
}

impl core::ops::Deref for MemAppData {
    type Target = RefCell<BTreeMap<String, Rc<RefCell<MemChunksFile>>>>;

    fn deref(&self) -> &Self::Target {
        &self.files
    }
}

/// An independent open instance sharing only the underlying file data.
struct MemFileHandle {
    file: Rc<RefCell<MemChunksFile>>,
    read_only: bool,
}

impl MemFileHandle {
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
        self.file.borrow_mut().read(buf, offset)
    }

    fn write(&mut self, buf: &[u8], offset: u64) -> VfsResult<()> {
        self.check_writable()?;
        self.file.borrow_mut().write(buf, offset)
    }

    fn truncate(&mut self, size: u64) -> VfsResult<()> {
        self.check_writable()?;
        self.file.borrow_mut().truncate(size)
    }

    fn sync(&mut self, options: SyncOptions) -> VfsResult<()> {
        self.file.borrow_mut().sync(options)
    }

    // Multiple connections to the same database are unsupported, not rejected.
    fn lock(&mut self, level: LockLevel) -> VfsResult<()> {
        self.file.borrow_mut().lock(level)
    }

    fn unlock(&mut self, level: LockLevel) -> VfsResult<()> {
        self.file.borrow_mut().unlock(level)
    }

    fn check_reserved_lock(&self) -> VfsResult<bool> {
        self.file.borrow().check_reserved_lock()
    }

    fn size(&self) -> VfsResult<u64> {
        self.file.borrow().size()
    }
}

#[derive(Copy, Clone, Default)]
struct MemStore;

impl VfsStore for MemStore {
    type File = MemFileHandle;
    type AppData = MemAppData;

    fn record_error(data: &MemAppData, error: VfsError) {
        data.error.replace(Some(error));
    }

    fn last_error(data: &MemAppData) -> Option<VfsError> {
        data.error.borrow().clone()
    }

    fn open_file(
        app_data: &MemAppData,
        request: crate::OpenRequest<'_>,
    ) -> VfsResult<OpenedFile<MemFileHandle>> {
        let options = request.options;
        let Some(filename) = request.filename else {
            return Ok(OpenedFile {
                file: MemFileHandle {
                    file: Rc::new(RefCell::new(MemChunksFile::default())),
                    read_only: options.access() == OpenAccess::ReadOnly,
                },
                access: options.access(),
            });
        };
        let name = filename.path();

        if options.kind() == Some(FileKind::MainDb) {
            validate_db_filename(name)
                .map_err(|err| VfsError::new(VfsErrorCode::CantOpen, format!("{err}").into()))?;
        }

        let mut files = app_data.borrow_mut();
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
                let file = Rc::new(RefCell::new(file));
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
            file: MemFileHandle {
                file,
                read_only: options.access() == OpenAccess::ReadOnly,
            },
            access: options.access(),
        })
    }

    fn close_file(
        app_data: &MemAppData,
        name: Option<&str>,
        file: MemFileHandle,
        options: OpenOptions,
    ) -> VfsResult<()> {
        let Some(name) = name else {
            return Ok(());
        };
        if options.delete_on_close() {
            let mut files = app_data.borrow_mut();
            // Do not delete a replacement created under the same name.
            if files
                .get(name)
                .is_some_and(|current| Rc::ptr_eq(current, &file.file))
            {
                files.remove(name);
            }
        }
        Ok(())
    }

    fn access(app_data: &MemAppData, file: &str, _mode: AccessMode) -> VfsResult<bool> {
        // Every file in this flat memory namespace is readable and writable.
        Ok(app_data.borrow().contains_key(file))
    }

    fn full_pathname(_data: &MemAppData, name: &str) -> VfsResult<String> {
        validate_db_filename(name)
            .map_err(|err| VfsError::new(VfsErrorCode::CantOpen, format!("{err}").into()))?;

        Ok(name.into())
    }

    fn delete_file(app_data: &MemAppData, file: &str, _sync_dir: bool) -> VfsResult<()> {
        if app_data.borrow_mut().remove(file).is_none() {
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

    const VERSION: ::core::ffi::c_int = 1;
}

#[derive(Clone, Copy, Default)]
struct MemVfs;

impl SQLiteVfs<MemIoMethods> for MemVfs {
    type Os = dyn OsCallback;

    fn os(data: &MemAppData) -> &Self::Os {
        &*data.os
    }

    const VERSION: ::core::ffi::c_int = 1;
    const MAX_PATH_SIZE: ::core::ffi::c_int = MAX_PATH_SIZE;
}

/// Memory VFS management errors. Match variants rather than display text.
#[derive(thiserror::Error, Debug)]
pub enum MemVfsError {
    #[error("memory VFS is not installed")]
    NotInstalled,
    #[error(transparent)]
    Registration(#[from] crate::RegisterVfsError),
    #[error("filename must be nonempty, NUL-free and at most 500 UTF-8 bytes")]
    InvalidFilename,
    #[error(transparent)]
    ImportDb(#[from] ImportDbError),
    #[error("file already exists: {0:?}")]
    AlreadyExists(String),
    #[error("file not found: {0:?}")]
    NotFound(String),
    #[error("file is too large to export into a contiguous memory buffer")]
    FileTooLarge,
    #[error(transparent)]
    Io(#[from] VfsError),
}

/// MemVfs management tool. Keeps its data alive even if the VFS is uninstalled.
/// After reinstallation, create a new tool to access the new VFS instance.
pub struct MemVfsUtil(MemAppData);

impl MemVfsUtil {
    /// Gets the installed memory VFS without registering it or changing the
    /// default VFS. SQLite's own automatic initialization may still run.
    ///
    /// # Safety
    /// Call on the installing thread, with serialized access to SQLite VFS
    /// registration. All SQLite use of memvfs must stay on that same thread.
    pub unsafe fn get() -> Result<Self> {
        unsafe {
            let vfs = bindings::sqlite3_vfs_find(VFS_NAME.as_ptr());
            if vfs.is_null() {
                return Err(MemVfsError::NotInstalled);
            }
            check_owned(vfs)?;
            Ok(Self(VfsAppData::<MemAppData>::get(vfs).data.clone()))
        }
    }

    fn import_db_unchecked_impl(
        &self,
        filename: &str,
        bytes: &[u8],
        page_size: usize,
        clear_wal: bool,
    ) -> Result<()> {
        validate_db_filename(filename)?;
        check_db_and_page_size(bytes.len(), page_size)?;
        if self.exists(filename) {
            return Err(MemVfsError::AlreadyExists(filename.into()));
        }

        self.0.borrow_mut().insert(filename.into(), {
            let mut file = MemChunksFile::new(page_size);
            file.write(bytes, 0)?;
            if clear_wal {
                // Set the read/write format versions to rollback-journal mode.
                // This does not checkpoint a WAL or recover missing pages.
                file.write(&[1, 1], 18)?;
            }
            Rc::new(RefCell::new(file))
        });

        Ok(())
    }

    /// Imports a standalone database image under a new filename.
    ///
    /// Validates the signature, page size and alignment, not database integrity.
    /// Sets the header's read/write format versions to rollback-journal mode;
    /// this does not checkpoint or import a WAL. Supply an image that does not
    /// depend on a separate WAL or rollback journal for recovery.
    ///
    /// The filename must be nonempty, NUL-free and at most 500 UTF-8 bytes,
    /// leaving room for SQLite's journal and super-journal suffixes within the
    /// VFS path limit.
    /// Returns an error for an invalid or existing name, invalid image layout,
    /// or failure to allocate file data. For encrypted images, use
    /// [`Self::import_db_unchecked`] instead.
    pub fn import_db(&self, filename: &str, bytes: &[u8]) -> Result<()> {
        let page_size = check_import_db(bytes)?;
        self.import_db_unchecked_impl(filename, bytes, page_size, true)
    }

    /// Imports an image without inspecting or modifying its header, for example
    /// an encrypted database. `page_size` is in bytes; it and file alignment are
    /// still validated. An empty image is allowed.
    ///
    /// The filename restrictions and standalone-image requirement of
    /// [`Self::import_db`] also apply. Returns an error for an invalid or existing
    /// name, invalid page size/alignment, or failure to allocate file data.
    pub fn import_db_unchecked(
        &self,
        filename: &str,
        bytes: &[u8],
        page_size: usize,
    ) -> Result<()> {
        self.import_db_unchecked_impl(filename, bytes, page_size, false)
    }

    /// Copies the current bytes of a named file into memory.
    /// This is not a SQLite backup or a transactional snapshot, and does not
    /// include auxiliary files. To export a standalone database, finish its
    /// transactions, checkpoint any WAL and close its connections first.
    /// Requires one contiguous allocation, limited to `isize::MAX` bytes and
    /// available memory (less than 2 GiB on wasm32).
    /// Returns an error if the file is absent or cannot fit in an allocated buffer.
    pub fn export_db(&self, filename: &str) -> Result<Vec<u8>> {
        let name2file = self.0.borrow();

        if let Some(file) = name2file.get(filename) {
            let mut file = file.borrow_mut();
            let file_size = usize::try_from(file.size()?)
                .ok()
                .filter(|&size| size <= isize::MAX as usize)
                .ok_or(MemVfsError::FileTooLarge)?;
            let mut ret = Vec::new();
            ret.try_reserve_exact(file_size)
                .map_err(|_| crate::no_memory())?;
            ret.resize(file_size, 0);
            file.read(&mut ret, 0)?;
            Ok(ret)
        } else {
            Err(MemVfsError::NotFound(filename.into()))
        }
    }

    /// Deletes the named file, returning whether it existed.
    /// The database must be closed before deleting any of its files, including
    /// sidecars. Does not automatically delete companion journal/WAL files.
    pub fn delete_db(&self, filename: &str) -> bool {
        self.0.borrow_mut().remove(filename).is_some()
    }

    /// Deletes all files. All databases must be closed first.
    pub fn clear_all(&self) {
        core::mem::take(&mut *self.0.borrow_mut());
    }

    /// Returns whether the named file exists in the VFS.
    pub fn exists(&self, filename: &str) -> bool {
        self.0.borrow().contains_key(filename)
    }

    /// Returns all filenames in unspecified order, including auxiliary files.
    pub fn list(&self) -> Vec<String> {
        self.0.borrow().keys().cloned().collect()
    }

    /// Returns the number of files, including auxiliary files.
    pub fn count(&self) -> usize {
        self.0.borrow().len()
    }
}

/// Installs memvfs with the supplied platform service and default-VFS policy.
/// Reuses an existing owned registration without replacing its service or data.
/// If raw SQLite unregistration detached it, registers that same allocation again.
///
/// # Safety
///
/// Requires a valid SQLite global context and serialized registration. All
/// installation, management, SQLite file access and uninstallation must occur
/// on the same thread. This backend deliberately uses `Rc` and `RefCell`.
/// Raw unregistration is allowed, but callers must not free or replace the
/// owned VFS or its app data; use [`uninstall`] to reclaim them.
pub unsafe fn install(
    os: impl OsCallback + 'static,
    default_vfs: bool,
) -> core::result::Result<MemVfsUtil, crate::RegisterVfsError> {
    unsafe {
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
            let data = VfsAppData::new(MemAppData::new(os)).leak();
            let vfs = Box::into_raw(Box::new(MemVfs::vfs(VFS_NAME.as_ptr(), data)));
            let code = bindings::sqlite3_vfs_register(vfs, i32::from(default_vfs));
            if code != bindings::SQLITE_OK {
                drop(Box::from_raw(vfs));
                drop(VfsAppData::from_raw(data));
                return Err(crate::RegisterVfsError::RegisterVfs(
                    VfsErrorCode::from_raw(code).expect("SQLite must return a valid error code"),
                ));
            }
            MEM_VFS.store(vfs, Ordering::Relaxed);
            vfs
        } else {
            if registered.is_null() || default_vfs {
                let code = bindings::sqlite3_vfs_register(owned, i32::from(default_vfs));
                if code != bindings::SQLITE_OK {
                    return Err(crate::RegisterVfsError::RegisterVfs(
                        VfsErrorCode::from_raw(code)
                            .expect("SQLite must return a valid error code"),
                    ));
                }
            }
            owned
        };

        Ok(MemVfsUtil(VfsAppData::<MemAppData>::get(vfs).data.clone()))
    }
}

fn check_owned(
    vfs: *mut bindings::sqlite3_vfs,
) -> core::result::Result<(), crate::RegisterVfsError> {
    if vfs != MEM_VFS.load(Ordering::Relaxed) {
        return Err(crate::RegisterVfsError::NameConflict("memvfs".into()));
    }
    Ok(())
}

/// Unregisters the memory VFS and frees its registration resources.
/// File data remains alive while any [`MemVfsUtil`] still owns it.
/// Also reclaims an owned VFS already detached by raw SQLite unregistration,
/// without unregistering an unrelated replacement using the same name.
///
/// # Safety
///
/// Requires a valid SQLite global context and serialized access to VFS
/// installation/uninstallation. All files must be closed and no references to
/// the VFS or its app data may remain in use. `MemVfsUtil` owns its data separately
/// and may outlive uninstall. Call on the installing thread.
/// The owned VFS and app-data allocations must not have been freed or replaced
/// through raw pointers.
pub unsafe fn uninstall() -> core::result::Result<(), crate::RegisterVfsError> {
    unsafe {
        let registered = bindings::sqlite3_vfs_find(VFS_NAME.as_ptr());
        let vfs = MEM_VFS.load(Ordering::Relaxed);

        if vfs.is_null() {
            if !registered.is_null() {
                check_owned(registered)?;
            }
        } else {
            let code = bindings::sqlite3_vfs_unregister(vfs);
            if code != bindings::SQLITE_OK {
                return Err(crate::RegisterVfsError::UnregisterVfs(
                    VfsErrorCode::from_raw(code).expect("SQLite must return a valid error code"),
                ));
            }
            MEM_VFS.store(core::ptr::null_mut(), Ordering::Relaxed);
            // Reconstitute both owners before running backend destructors.
            let vfs = Box::from_raw(vfs);
            let data = Box::from_raw(vfs.pAppData.cast::<VfsAppData<MemAppData>>());
            drop(data);
            drop(vfs);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        AccessMode, FileKind, OpenAccess, OpenOptions, VfsAppData,
        memvfs::{MemAppData, MemStore},
        test_suite::test_vfs_store,
    };

    #[test]
    fn default_callbacks_use_os_services() {
        use super::{MemVfs, OsCallback, SQLiteVfs};
        use core::time::Duration;

        struct TestOs {
            now: Option<i64>,
            count: usize,
        }
        impl OsCallback for TestOs {
            fn sleep(&self, duration: Duration) {
                assert_eq!(duration, Duration::from_micros(123));
            }

            fn random(&self, buf: &mut [u8]) -> usize {
                buf[..self.count].fill(42);
                self.count
            }

            fn epoch_timestamp_in_ms(&self) -> crate::VfsResult<i64> {
                self.now.ok_or_else(|| {
                    crate::VfsError::new(crate::VfsErrorCode::Io, "clock unavailable".into())
                })
            }
        }

        let mut random = [0u8; 8];
        let mut time = 0.0;
        let mut time_ms = 0;
        let mut data = VfsAppData::new(MemAppData::new(TestOs {
            now: Some(86_400_000),
            count: 8,
        }));
        let mut vfs = unsafe { MemVfs::vfs(c"callbacks".as_ptr(), &mut data) };
        let vfs = core::ptr::from_mut(&mut vfs);
        unsafe {
            assert_eq!(
                MemVfs::xRandomness(vfs, random.len() as i32, random.as_mut_ptr().cast(),),
                8
            );
            assert_eq!(MemVfs::xCurrentTime(vfs, &mut time), 0);
            assert_eq!(MemVfs::xCurrentTimeInt64(vfs, &mut time_ms), 0);
            assert_eq!(MemVfs::xSleep(vfs, 123), 123);
            assert_eq!(MemVfs::xSleep(vfs, 0), 0);
            assert_eq!(MemVfs::xSleep(vfs, -1), 0);
        }
        assert_eq!(random, [42; 8]);
        assert_eq!(time, 2_440_588.5);
        assert_eq!(time_ms, 210_866_846_400_000);
        let mut other_data = VfsAppData::new(MemAppData::new(TestOs {
            now: None,
            count: 3,
        }));
        let mut other = unsafe { MemVfs::vfs(c"other".as_ptr(), &mut other_data) };
        random.fill(99);
        unsafe {
            assert_eq!(
                MemVfs::xRandomness(&mut other, 8, random.as_mut_ptr().cast()),
                3
            );
            assert_eq!(random, [42, 42, 42, 0, 0, 0, 0, 0]);
            assert_eq!(
                MemVfs::xCurrentTime(&mut other, &mut time),
                crate::ffi::SQLITE_IOERR
            );
            assert_eq!(
                MemVfs::xCurrentTimeInt64(&mut other, &mut time_ms),
                crate::ffi::SQLITE_IOERR
            );
            assert_eq!(time, 0.0);
            assert_eq!(time_ms, 0);
            assert_eq!(
                MemVfs::xCurrentTimeInt64(vfs, &mut time_ms),
                crate::ffi::SQLITE_OK
            );
            assert_eq!(time_ms, 210_866_846_400_000);
        }
    }

    #[test]
    fn test_memory_vfs_store() {
        test_vfs_store::<MemStore>(VfsAppData::new(MemAppData::new(
            crate::test_support::CallbackOs::<0>,
        )))
        .unwrap();
    }

    #[test]
    fn exclusive_create_preserves_existing_file() {
        use crate::{VfsFile, VfsStore, ffi::*};

        let data = MemAppData::new(crate::test_support::CallbackOs::<0>);
        let flags = OpenOptions::new(OpenAccess::ReadWrite, FileKind::MainDb).with_create();
        let exclusive = flags.with_create_new();
        let mut file =
            MemStore::open_file(&data, crate::OpenRequest::named("exclusive.db", exclusive))
                .unwrap()
                .file;
        file.write(&[41, 42], 0).unwrap();
        MemStore::close_file(&data, Some("exclusive.db"), file, exclusive).unwrap();

        assert_eq!(
            MemStore::open_file(&data, crate::OpenRequest::named("exclusive.db", exclusive))
                .err()
                .unwrap()
                .raw_code(),
            SQLITE_CANTOPEN
        );
        assert_eq!(data.borrow().len(), 1);
        let mut file = MemStore::open_file(&data, crate::OpenRequest::named("exclusive.db", flags))
            .unwrap()
            .file;
        assert_eq!(alloc::rc::Rc::strong_count(&file.file), 2);
        let mut bytes = [0; 2];
        assert_eq!(file.size().unwrap(), 2);
        assert_eq!(file.read(&mut bytes, 0).unwrap(), 2);
        assert_eq!(bytes, [41, 42]);
    }

    #[test]
    fn handles_keep_identity_and_access_mode() {
        use crate::{
            VfsFile, VfsStore,
            ffi::{SQLITE_CANTOPEN, SQLITE_READONLY},
        };

        let data = MemAppData::new(crate::test_support::CallbackOs::<0>);
        assert_eq!(
            MemStore::open_file(
                &data,
                crate::OpenRequest::named(
                    "owned.db",
                    OpenOptions::new(OpenAccess::ReadWrite, FileKind::MainDb)
                )
            )
            .err()
            .unwrap()
            .raw_code(),
            SQLITE_CANTOPEN
        );
        let flags = OpenOptions::new(OpenAccess::ReadWrite, FileKind::MainDb).with_create();
        let mut first = MemStore::open_file(&data, crate::OpenRequest::named("owned.db", flags))
            .unwrap()
            .file;
        first.write(&[41, 42, 43], 0).unwrap();
        let mut second = MemStore::open_file(
            &data,
            crate::OpenRequest::named(
                "owned.db",
                OpenOptions::new(OpenAccess::ReadOnly, FileKind::MainDb),
            ),
        )
        .unwrap()
        .file;
        let mut bytes = [0; 3];
        assert_eq!(second.read(&mut bytes, 0).unwrap(), 3);
        assert_eq!(bytes, [41, 42, 43]);
        assert_eq!(
            second.write(&[0], 0).unwrap_err().raw_code(),
            SQLITE_READONLY
        );
        assert_eq!(second.truncate(0).unwrap_err().raw_code(), SQLITE_READONLY);
        MemStore::delete_file(&data, "owned.db", false).unwrap();
        let mut replacement =
            MemStore::open_file(&data, crate::OpenRequest::named("owned.db", flags))
                .unwrap()
                .file;
        replacement.write(&[99], 0).unwrap();
        first.write(&[44], 0).unwrap();
        assert_eq!(second.read(&mut bytes, 0).unwrap(), 3);
        assert_eq!(bytes, [44, 42, 43]);
        assert_eq!(replacement.size().unwrap(), 1);
        let old = alloc::rc::Rc::downgrade(&first.file);
        MemStore::close_file(&data, Some("owned.db"), first, flags.with_delete_on_close()).unwrap();
        assert!(MemStore::access(&data, "owned.db", AccessMode::Exists).unwrap());
        assert!(old.upgrade().is_some());
        drop(second);
        assert!(old.upgrade().is_none());
        MemStore::close_file(
            &data,
            Some("owned.db"),
            replacement,
            flags.with_delete_on_close(),
        )
        .unwrap();
        assert!(!MemStore::access(&data, "owned.db", AccessMode::Exists).unwrap());
    }

    #[test]
    fn callbacks_own_handles_and_release_them_on_close_errors() {
        use super::*;
        use crate::{SQLiteVfsFile, ffi::*};

        struct TestOs;
        impl OsCallback for TestOs {
            fn sleep(&self, _: core::time::Duration) {}

            fn random(&self, buf: &mut [u8]) -> usize {
                buf.fill(42);
                buf.len()
            }

            fn epoch_timestamp_in_ms(&self) -> crate::VfsResult<i64> {
                Ok(0)
            }
        }
        // Exercise fallible backend close and unconditional framework cleanup.
        struct Store;
        impl VfsStore for Store {
            type File = MemFileHandle;

            fn close_file(
                data: &MemAppData,
                name: Option<&str>,
                file: MemFileHandle,
                options: OpenOptions,
            ) -> VfsResult<()> {
                drop(file);
                if options.delete_on_close() {
                    Self::delete_file(data, name.unwrap(), false)?;
                }
                Ok(())
            }

            type AppData = MemAppData;

            fn record_error(data: &MemAppData, error: VfsError) {
                MemStore::record_error(data, error);
            }

            fn last_error(data: &MemAppData) -> Option<VfsError> {
                MemStore::last_error(data)
            }

            fn open_file(
                data: &MemAppData,
                request: crate::OpenRequest<'_>,
            ) -> VfsResult<OpenedFile<MemFileHandle>> {
                MemStore::open_file(data, request)
            }

            fn access(data: &MemAppData, name: &str, mode: AccessMode) -> VfsResult<bool> {
                MemStore::access(data, name, mode)
            }

            fn full_pathname(data: &MemAppData, name: &str) -> VfsResult<String> {
                MemStore::full_pathname(data, name)
            }

            fn delete_file(data: &MemAppData, name: &str, sync_dir: bool) -> VfsResult<()> {
                MemStore::delete_file(data, name, sync_dir)
            }
        }
        struct Io;
        impl SQLiteIoMethods for Io {
            type Store = Store;
            const VERSION: i32 = 1;
        }
        struct Vfs;
        impl SQLiteVfs<Io> for Vfs {
            type Os = TestOs;

            fn os(_: &MemAppData) -> &Self::Os {
                &TestOs
            }

            const VERSION: i32 = 1;
        }

        let data = MemAppData::new(crate::test_support::CallbackOs::<0>);
        let mut app_data = VfsAppData::new(data.clone());
        // SAFETY: All allocations outlive the callbacks, and the types/layout match.
        let mut vfs = unsafe { Vfs::vfs(c"test".as_ptr(), &mut app_data) };
        let vfs_ptr = core::ptr::from_mut(&mut vfs);
        let mut file: SQLiteVfsFile = unsafe { core::mem::zeroed() };
        let mut out_flags = -1;
        unsafe {
            assert_eq!(
                Vfs::xOpen(
                    vfs_ptr,
                    c"file.db".as_ptr(),
                    file.sqlite3_file(),
                    SQLITE_OPEN_READWRITE,
                    &mut out_flags
                ),
                SQLITE_CANTOPEN
            );
            assert!(file.io_methods.pMethods.is_null());
            assert!(file.handle_ptr.is_null());
            assert_eq!(out_flags, -1);
            app_data.error.borrow_mut().take();

            let flags = SQLITE_OPEN_CREATE | SQLITE_OPEN_READWRITE | SQLITE_OPEN_DELETEONCLOSE;
            assert_eq!(
                Vfs::xOpen(
                    vfs_ptr,
                    c"file.db".as_ptr(),
                    file.sqlite3_file(),
                    flags,
                    &mut out_flags
                ),
                SQLITE_OK
            );
            assert_eq!(out_flags, flags);
            let weak = Rc::downgrade(&data.borrow()["file.db"]);
            assert_eq!(weak.strong_count(), 2);
            // Reject signed lengths/offsets before constructing Rust slices or
            // forwarding an invalid request to the backend.
            let mut scratch = [99u8; 4];
            for (length, offset) in [(-1, 0), (1, -1)] {
                assert_eq!(
                    Io::xRead(
                        file.sqlite3_file(),
                        scratch.as_mut_ptr().cast(),
                        length,
                        offset
                    ),
                    SQLITE_IOERR_READ
                );
                assert_eq!(scratch, [99; 4]);
                assert_eq!(
                    Io::xWrite(file.sqlite3_file(), scratch.as_ptr().cast(), length, offset),
                    SQLITE_IOERR_WRITE
                );
            }
            assert_eq!(
                Io::xTruncate(file.sqlite3_file(), -1),
                SQLITE_IOERR_TRUNCATE
            );
            assert_eq!(
                Io::xRead(file.sqlite3_file(), core::ptr::null_mut(), 0, 0),
                SQLITE_OK
            );
            assert_eq!(
                Io::xWrite(file.sqlite3_file(), core::ptr::null(), 0, 0),
                SQLITE_OK
            );
            let bytes = [41u8, 42, 43];
            assert_eq!(
                Io::xWrite(file.sqlite3_file(), bytes.as_ptr().cast(), 3, 0),
                SQLITE_OK
            );
            assert_eq!(Io::xTruncate(file.sqlite3_file(), 2), SQLITE_OK);
            assert_eq!(
                Io::xSync(file.sqlite3_file(), SQLITE_SYNC_NORMAL),
                SQLITE_OK
            );
            let mut size = 0;
            assert_eq!(Io::xFileSize(file.sqlite3_file(), &mut size), SQLITE_OK);
            assert_eq!(size, 2);

            // A live handle works after its namespace entry disappears.
            MemStore::delete_file(&data, "file.db", false).unwrap();
            let mut bytes = [99u8; 3];
            assert_eq!(
                Io::xRead(file.sqlite3_file(), bytes.as_mut_ptr().cast(), 3, 0),
                SQLITE_IOERR_SHORT_READ
            );
            assert_eq!(bytes, [41, 42, 0]);
            // DELETEONCLOSE now fails, but the handle must still be freed.
            assert_eq!(Io::xClose(file.sqlite3_file()), SQLITE_IOERR_DELETE);
            assert_eq!(weak.strong_count(), 0);
            assert!(file.handle_ptr.is_null());
            assert!(file.name_ptr.is_null());
            assert!(file.io_methods.pMethods.is_null());
            assert_eq!(
                app_data.error.borrow_mut().take().unwrap().raw_code(),
                SQLITE_IOERR_DELETE
            );

            assert_eq!(
                Vfs::xOpen(
                    vfs_ptr,
                    c"file.db".as_ptr(),
                    file.sqlite3_file(),
                    flags,
                    core::ptr::null_mut()
                ),
                SQLITE_OK
            );
            let weak = Rc::downgrade(&data.borrow()["file.db"]);
            assert_eq!(Io::xClose(file.sqlite3_file()), SQLITE_OK);
            assert_eq!(weak.strong_count(), 0);
            assert!(!data.borrow().contains_key("file.db"));
            // Anonymous files have independent storage and no namespace entry.
            let mut mem_vfs = super::MemVfs::vfs(c"anonymous".as_ptr(), &mut app_data);
            assert_eq!(
                super::MemVfs::xOpen(
                    &mut mem_vfs,
                    core::ptr::null(),
                    file.sqlite3_file(),
                    flags,
                    core::ptr::null_mut()
                ),
                SQLITE_OK
            );
            assert!(file.name().is_none());
            assert!(data.borrow().is_empty());
            assert_eq!(
                super::MemIoMethods::xWrite(file.sqlite3_file(), bytes.as_ptr().cast(), 3, 0),
                SQLITE_OK
            );
            assert_eq!(super::MemIoMethods::xClose(file.sqlite3_file()), SQLITE_OK);
        }
    }
}
