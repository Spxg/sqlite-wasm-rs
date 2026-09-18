//! Low-level utilities and traits for implementing custom SQLite Virtual File Systems (VFS).
//!
//! Includes a platform-independent, single-threaded in-memory VFS in [`memvfs`].
#![no_std]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

extern crate alloc;

/// SQLite C types and bindings used to implement a VFS.
#[rustfmt::skip]
pub mod ffi;
mod error;
mod filename;
pub mod memvfs;
mod options;
pub use error::{RawVfsErrorCode, SystemErrorCode, VfsErrorCode};
pub use filename::{OpenRequest, VfsFilename};
pub use options::{
    AccessMode, DeviceCharacteristics, FileKind, LockLevel, OpenAccess, OpenOptions, SectorSize,
    SyncMode, SyncOptions,
};

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::{boxed::Box, ffi::CString};
use core::time::Duration;
use core::{ffi::CStr, ops::Deref};
use ffi::*;

// Write a bounded, NUL-terminated UTF-8 diagnostic. A zero-sized or absent
// buffer is valid for callers which only want an error code.
unsafe fn write_message(out: *mut core::ffi::c_char, capacity: i32, message: &str) {
    if out.is_null() || capacity <= 0 {
        return;
    }
    let mut count = message.len().min(capacity as usize - 1);
    while !message.is_char_boundary(count) {
        count -= 1;
    }
    unsafe {
        message.as_ptr().copy_to(out.cast(), count);
        out.add(count).write(0);
    }
}

/// SQLite database signature, including its terminating NUL byte.
pub const SQLITE3_HEADER: &str = "SQLite format 3\0";

/// Generates a temporary filename, rejecting unavailable or incomplete randomness.
pub fn random_name(randomness: impl FnOnce(&mut [u8]) -> usize) -> VfsResult<String> {
    const GEN_ASCII_STR_CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                abcdefghijklmnopqrstuvwxyz\
                0123456789";
    const GEN_LEN: u8 = GEN_ASCII_STR_CHARSET.len() as u8;
    let mut random_buffer = [0; 32];
    if randomness(&mut random_buffer) != random_buffer.len() {
        return Err(VfsError::new(
            VfsErrorCode::CantOpen,
            "insufficient randomness for a temporary filename".into(),
        ));
    }
    Ok(random_buffer
        .into_iter()
        .map(|e| {
            let idx = e.saturating_sub(GEN_LEN * (e / GEN_LEN));
            GEN_ASCII_STR_CHARSET[idx as usize] as char
        })
        .collect())
}

/// Chunked temporary storage, limited by address space and available memory.
/// Truncation only shrinks. Sync and locks are no-ops: no persistence or
/// coordination between connections.
pub struct MemChunksFile {
    chunks: Vec<Box<[u8]>>,
    chunk_size: Option<usize>,
    file_size: usize,
}

impl Default for MemChunksFile {
    fn default() -> Self {
        Self::new(512)
    }
}

impl MemChunksFile {
    fn allocate_chunk(size: usize) -> VfsResult<Box<[u8]>> {
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(size).map_err(|_| no_memory())?;
        bytes.resize(size, 0);
        Ok(bytes.into_boxed_slice())
    }

    /// Creates a new `MemChunksFile` with a specified chunk size.
    ///
    /// # Panics
    /// Panics if `chunk_size` is zero.
    pub fn new(chunk_size: usize) -> Self {
        assert!(chunk_size != 0, "chunk size can't be zero");
        MemChunksFile {
            chunks: Vec::new(),
            chunk_size: Some(chunk_size),
            file_size: 0,
        }
    }

    /// Uses the first successful nonempty write's length as the chunk size.
    pub fn waiting_for_write() -> Self {
        MemChunksFile {
            chunks: Vec::new(),
            chunk_size: None,
            file_size: 0,
        }
    }
}

impl VfsFile for MemChunksFile {
    fn read(&mut self, buf: &mut [u8], offset: u64) -> VfsResult<usize> {
        if buf.is_empty() || offset >= self.file_size as u64 {
            return Ok(0);
        }
        // Bounded by the in-memory file size above.
        let offset = offset as usize;
        let Some(chunk_size) = self.chunk_size else {
            return Ok(0);
        };

        if chunk_size == buf.len()
            && offset % chunk_size == 0
            && self.file_size - offset >= buf.len()
        {
            buf.copy_from_slice(&self.chunks[offset / chunk_size]);
            Ok(buf.len())
        } else {
            let mut size = core::cmp::min(buf.len(), self.file_size - offset);
            let chunk_idx = offset / chunk_size;
            let mut remaining_idx = offset % chunk_size;
            let mut offset = 0;

            for chunk in &self.chunks[chunk_idx..] {
                let n = core::cmp::min(chunk_size - remaining_idx, size);
                buf[offset..offset + n].copy_from_slice(&chunk[remaining_idx..remaining_idx + n]);
                offset += n;
                size -= n;
                remaining_idx = 0;
                if size == 0 {
                    break;
                }
            }

            Ok(offset)
        }
    }

    fn write(&mut self, buf: &[u8], offset: u64) -> VfsResult<()> {
        if buf.is_empty() {
            return Ok(());
        }
        let offset = usize::try_from(offset).map_err(|_| {
            VfsError::new(
                VfsErrorCode::Full,
                "file offset exceeds address space".into(),
            )
        })?;
        let end = offset.checked_add(buf.len()).ok_or_else(|| {
            VfsError::new(VfsErrorCode::Full, "file size exceeds address space".into())
        })?;

        let chunk_size = self.chunk_size.unwrap_or(buf.len());
        let required = (end - 1) / chunk_size + 1;
        let original_chunks = self.chunks.len();
        self.chunks
            .try_reserve(required.saturating_sub(original_chunks))
            .map_err(|_| no_memory())?;
        for _ in original_chunks..required {
            match Self::allocate_chunk(chunk_size) {
                Ok(chunk) => self.chunks.push(chunk),
                Err(error) => {
                    // Preserve all original bytes and EOF on allocation failure.
                    self.chunks.truncate(original_chunks);
                    return Err(error);
                }
            }
        }
        self.chunk_size = Some(chunk_size);

        let new_length = self.file_size.max(end);

        if chunk_size == buf.len() && offset % chunk_size == 0 {
            self.chunks[offset / chunk_size].copy_from_slice(buf);
        } else {
            let mut size = buf.len();
            let chunk_start_idx = offset / chunk_size;
            let chunk_end_idx = (end - 1) / chunk_size;
            let mut remaining_idx = offset % chunk_size;
            let mut offset = 0;

            for idx in chunk_start_idx..=chunk_end_idx {
                let n = core::cmp::min(chunk_size - remaining_idx, size);
                self.chunks[idx][remaining_idx..remaining_idx + n]
                    .copy_from_slice(&buf[offset..offset + n]);
                offset += n;
                size -= n;
                remaining_idx = 0;
                if size == 0 {
                    break;
                }
            }
        }

        self.file_size = new_length;

        Ok(())
    }

    fn truncate(&mut self, size: u64) -> VfsResult<()> {
        if size > self.file_size as u64 {
            return Err(VfsError::new(
                VfsErrorCode::IoTruncate,
                "cannot extend an in-memory file by truncating".into(),
            ));
        }
        let size = usize::try_from(size).map_err(|_| {
            VfsError::new(VfsErrorCode::Full, "file size exceeds address space".into())
        })?;
        if let Some(chunk_size) = self.chunk_size {
            if size == 0 {
                core::mem::take(&mut self.chunks);
            } else {
                let idx = ((size - 1) / chunk_size) + 1;
                self.chunks.drain(idx..);
                // Keep allocated bytes beyond EOF zeroed, so a later sparse
                // write cannot expose data removed by this truncation.
                let tail = size % chunk_size;
                if size < self.file_size && tail != 0 {
                    self.chunks[idx - 1][tail..].fill(0);
                }
            }
        }
        self.file_size = size;
        Ok(())
    }

    fn sync(&mut self, _options: SyncOptions) -> VfsResult<()> {
        Ok(())
    }

    // A standalone memory buffer has no shared lock manager.
    fn lock(&mut self, _level: LockLevel) -> VfsResult<()> {
        Ok(())
    }

    fn unlock(&mut self, _level: LockLevel) -> VfsResult<()> {
        Ok(())
    }

    fn check_reserved_lock(&self) -> VfsResult<bool> {
        Ok(false)
    }

    fn size(&self) -> VfsResult<u64> {
        Ok(self.file_size as u64)
    }
}

/// C-compatible file handle. Set `szOsFile` to this type's size.
#[repr(C)]
pub struct SQLiteVfsFile {
    /// SQLite header; must remain the first field.
    pub io_methods: sqlite3_file,
    /// Owning VFS.
    pub vfs: *mut sqlite3_vfs,
    /// Flags used to open the database.
    pub flags: i32,
    /// SQLite-owned filename, valid until xClose. Null for anonymous files.
    pub name_ptr: *const u8,
    /// Filename length in bytes.
    pub name_length: usize,
    /// An owned box of the selected `VfsStore::File` type, initialized by
    /// xOpen and consumed by xClose. Must not be accessed after close.
    pub handle_ptr: *mut core::ffi::c_void,
}

impl SQLiteVfsFile {
    /// Casts without dereferencing. Dereferencing requires a live, initialized
    /// `SQLiteVfsFile` and valid lifetime/aliasing.
    pub fn from_file(file: *mut sqlite3_file) -> *mut SQLiteVfsFile {
        file.cast()
    }

    /// Get the file name.
    ///
    /// # Safety
    ///
    /// `name_ptr` is null for an anonymous file; otherwise it and `name_length`
    /// must describe valid UTF-8 bytes which remain alive and unmodified for
    /// the borrow. The file must not have been closed.
    ///
    /// ```compile_fail
    /// use rsqlite_vfs::SQLiteVfsFile;
    /// fn extend(file: &SQLiteVfsFile) -> Option<&'static str> {
    ///     unsafe { file.name() }
    /// }
    /// ```
    pub unsafe fn name(&self) -> Option<&str> {
        if self.name_ptr.is_null() {
            return None;
        }
        unsafe {
            Some(core::str::from_utf8_unchecked(core::slice::from_raw_parts(
                self.name_ptr,
                self.name_length,
            )))
        }
    }

    /// Borrows the backend handle for a custom I/O callback.
    ///
    /// # Safety
    /// `F` must be the exact file type used to open this live SQLite file.
    /// The handle must be valid and not mutably borrowed for this borrow.
    ///
    /// ```compile_fail
    /// use rsqlite_vfs::SQLiteVfsFile;
    /// fn extend<F: 'static>(file: &SQLiteVfsFile) -> &'static F {
    ///     unsafe { file.handle::<F>() }
    /// }
    /// ```
    pub unsafe fn handle<F>(&self) -> &F {
        unsafe { &*self.handle_ptr.cast::<F>() }
    }

    /// Mutably borrows the backend handle for a custom I/O callback.
    ///
    /// # Safety
    /// `F` must be the exact file type used to open this live SQLite file.
    /// The handle must be valid and exclusively accessible for this borrow.
    ///
    /// ```compile_fail
    /// use rsqlite_vfs::SQLiteVfsFile;
    /// fn extend<F: 'static>(file: &mut SQLiteVfsFile) -> &'static mut F {
    ///     unsafe { file.handle_mut::<F>() }
    /// }
    /// ```
    pub unsafe fn handle_mut<F>(&mut self) -> &mut F {
        unsafe { &mut *self.handle_ptr.cast::<F>() }
    }

    /// Returns a pointer to the SQLite file header from an exclusive borrow.
    /// The pointer must not be used after this file is moved or freed, or while
    /// conflicting references exist.
    pub fn sqlite3_file(&mut self) -> *mut sqlite3_file {
        core::ptr::from_mut(self).cast()
    }
}

/// Errors from VFS lookup, registration or unregistration.
/// Match variants rather than the human-readable display text.
#[derive(thiserror::Error, Debug)]
pub enum RegisterVfsError {
    #[error("VFS name must not be empty")]
    EmptyName,
    #[error("VFS name contains a NUL byte")]
    ToCStr,
    #[error("failed to register VFS (SQLite error {0})")]
    RegisterVfs(VfsErrorCode),
    #[error("failed to unregister VFS (SQLite error {0})")]
    UnregisterVfs(VfsErrorCode),
    #[error("VFS name is already registered: {0:?}")]
    NameConflict(String),
}

/// Looks up a VFS without retaining it. May initialize SQLite.
///
/// # Safety
/// Obey the linked SQLite library's global initialization and threading rules.
/// In single-thread mode, serialize this call with all other SQLite use.
/// Coordinate with VFS owners to prevent freeing a registration during lookup
/// or while using the returned pointer.
pub unsafe fn registered_vfs(vfs_name: &str) -> Result<Option<*mut sqlite3_vfs>, RegisterVfsError> {
    let name = CString::new(vfs_name).map_err(|_| RegisterVfsError::ToCStr)?;
    let vfs = unsafe { sqlite3_vfs_find(name.as_ptr()) };
    Ok((!vfs.is_null()).then_some(vfs))
}

/// Owns a VFS registration. Drop leaves it and its data alive for SQLite.
/// Keep the handle for explicit [`Self::unregister`], or use [`Self::into_raw`]
/// for process-lifetime registration.
#[must_use = "keep the registration for explicit cleanup, or call into_raw for permanent registration"]
pub struct VfsRegistration<T> {
    allocations: core::mem::ManuallyDrop<VfsAllocations<T>>,
}

// Own raw allocations from the moment any pointer is exposed to SQLite or a
// custom constructor. Moving/reborrowing a Box afterwards would invalidate
// pointers derived from an earlier exclusive borrow under Rust's alias rules.
// Drop also handles constructor panic and registration failure.
struct VfsAllocations<T> {
    vfs: *mut sqlite3_vfs,
    app_data: *mut VfsAppData<T>,
    name: *mut core::ffi::c_char,
}

impl<T> Drop for VfsAllocations<T> {
    fn drop(&mut self) {
        unsafe {
            // Reconstitute all owners before dropping backend data, so the
            // structure/name are reclaimed even if its destructor unwinds.
            let _name = CString::from_raw(self.name);
            let _vfs = (!self.vfs.is_null()).then(|| Box::from_raw(self.vfs));
            let _app_data = Box::from_raw(self.app_data);
        }
    }
}

impl<T> core::fmt::Debug for VfsRegistration<T> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("VfsRegistration")
            .field("vfs", &self.allocations.vfs)
            .finish_non_exhaustive()
    }
}

impl<T> VfsRegistration<T> {
    /// Returns the SQLite pointer without transferring allocation ownership.
    /// It must not be freed separately or used after successful `unregister`.
    pub fn as_ptr(&self) -> *mut sqlite3_vfs {
        self.allocations.vfs
    }

    /// Gives up managed cleanup and retains all allocations. The returned
    /// pointer remains valid for the process lifetime unless manually freed.
    /// Calling SQLite's raw unregister function alone will not free it.
    pub fn into_raw(self) -> *mut sqlite3_vfs {
        self.allocations.vfs
    }

    /// Unregisters and frees the VFS, name and data. Failure returns the intact
    /// handle and error for retry.
    ///
    /// # Safety
    /// Close all connections/files and retire callbacks, saved pointers and
    /// delegating wrappers. Serialize with VFS lookup, registration and use;
    /// obey SQLite initialization and backend threading rules. Owned allocations
    /// must not have been freed or replaced; prior raw unregistration is allowed.
    pub unsafe fn unregister(self) -> Result<(), (Self, RegisterVfsError)> {
        unsafe {
            let code = sqlite3_vfs_unregister(self.allocations.vfs);
            if code != SQLITE_OK {
                return Err((
                    self,
                    RegisterVfsError::UnregisterVfs(
                        VfsErrorCode::from_raw(code)
                            .expect("SQLite must return a valid error code"),
                    ),
                ));
            }
        }
        drop(core::mem::ManuallyDrop::into_inner(self.allocations));
        Ok(())
    }
}

/// Registers a VFS, rejecting empty or occupied names.
/// The returned handle requires explicit cleanup; drop does not unregister it.
///
/// # Safety
///
/// `V::vfs`, `IO::METHODS` and callbacks must agree on versions, file size/layout
/// and app-data types. The constructor must preserve supplied name/app-data
/// pointers without taking ownership. Callback resources must outlive all uses,
/// including open files after raw unregistration.
///
/// Obey SQLite/backend threading and aliasing rules; single-thread mode requires
/// serialization with all SQLite use. In every mode, serialize this entire
/// lookup-and-register operation against same-name registrations, including C
/// callers. The constructor must not reentrantly register that name.
pub unsafe fn register_vfs<IO: SQLiteIoMethods, V: SQLiteVfs<IO>>(
    vfs_name: &str,
    app_data: <IO::Store as VfsStore>::AppData,
    default_vfs: bool,
) -> Result<VfsRegistration<<IO::Store as VfsStore>::AppData>, RegisterVfsError> {
    if vfs_name.is_empty() {
        return Err(RegisterVfsError::EmptyName);
    }
    let name = CString::new(vfs_name).map_err(|_| RegisterVfsError::ToCStr)?;
    if !unsafe { sqlite3_vfs_find(name.as_ptr()) }.is_null() {
        return Err(RegisterVfsError::NameConflict(vfs_name.into()));
    }
    let mut allocations = VfsAllocations {
        vfs: core::ptr::null_mut(),
        app_data: Box::into_raw(Box::new(VfsAppData::new(app_data))),
        name: name.into_raw(),
    };
    // SAFETY: The allocations are live and retained for SQLite. The caller
    // guarantees that the custom constructor and callbacks satisfy the contract.
    allocations.vfs = Box::into_raw(Box::new(unsafe {
        V::vfs(allocations.name, allocations.app_data)
    }));
    let ret = unsafe { sqlite3_vfs_register(allocations.vfs, i32::from(default_vfs)) };

    if ret != SQLITE_OK {
        return Err(RegisterVfsError::RegisterVfs(
            VfsErrorCode::from_raw(ret).expect("SQLite must return a valid error code"),
        ));
    }

    Ok(VfsRegistration {
        allocations: core::mem::ManuallyDrop::new(allocations),
    })
}

/// A container for VFS-specific errors, holding both an error code and a descriptive message.
/// Use [`Self::code`] for error handling; messages are human-readable diagnostics,
/// not a stable format for programmatic matching.
#[derive(thiserror::Error, Debug, Clone)]
#[error("{message} (SQLite error {code})")]
pub struct VfsError {
    code: VfsErrorCode,
    message: Cow<'static, str>,
    system_error: Option<SystemErrorCode>,
}

impl VfsError {
    pub fn new(code: VfsErrorCode, message: Cow<'static, str>) -> Self {
        VfsError {
            code,
            message,
            system_error: None,
        }
    }

    /// Attaches the underlying OS error without changing the SQLite result code.
    pub fn with_system_error(mut self, code: SystemErrorCode) -> Self {
        self.system_error = Some(code);
        self
    }

    /// Returns the OS error, if any, for `xGetLastError` / `sqlite3_system_errno`.
    pub fn system_error(&self) -> Option<SystemErrorCode> {
        self.system_error
    }

    /// Returns the SQLite result code, including any extended error code.
    pub fn code(&self) -> VfsErrorCode {
        self.code
    }

    /// Explicit SQLite interoperability, preserving extended error codes.
    pub fn raw_code(&self) -> i32 {
        self.code.as_raw()
    }

    /// Returns the backend's error message without additional formatting.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// A specialized `Result` type for VFS operations.
pub type VfsResult<T> = Result<T, VfsError>;

fn no_memory() -> VfsError {
    VfsError::new(
        VfsErrorCode::NoMemory,
        "unable to allocate memory for file data".into(),
    )
}

/// The handle and access mode actually obtained by the backend. A read-write
/// request may fall back to read-only; SQLite must be told through `pOutFlags`.
pub struct OpenedFile<F> {
    pub file: F,
    pub access: OpenAccess,
}

/// Owns `sqlite3_vfs::pAppData`. Threading and synchronization follow `T`
/// and the backend; this wrapper adds neither.
pub struct VfsAppData<T> {
    data: T,
}

impl<T> VfsAppData<T> {
    /// Borrows app data without borrowing the SQLite-owned VFS structure.
    /// SQLite may independently mutate the registry's `pNext` field.
    ///
    /// # Safety
    /// `vfs` must be valid and aligned, with `pAppData` unchanged during this
    /// call. It must point to an aligned `VfsAppData<T>` of exactly this type,
    /// live for the caller-chosen `'a`. Obey Rust shared-reference rules and
    /// backend threading requirements.
    pub unsafe fn get<'a>(vfs: *const sqlite3_vfs) -> &'a Self {
        unsafe { &*core::ptr::addr_of!((*vfs).pAppData).read().cast() }
    }

    pub fn new(t: T) -> Self {
        VfsAppData { data: t }
    }

    /// Transfers ownership to a raw pointer suitable for `sqlite3_vfs::pAppData`.
    /// Reclaim it with [`Self::from_raw`] only after all users have stopped.
    pub fn leak(self) -> *mut Self {
        Box::into_raw(Box::new(self))
    }

    /// Reclaims the allocation created by [`Self::leak`], returning its contents.
    ///
    /// # Safety
    ///
    /// Takes ownership of a pointer returned by `leak`, exactly once. No references
    /// to the allocation may remain in use, including through SQLite callbacks.
    pub unsafe fn from_raw(t: *mut Self) -> VfsAppData<T> {
        unsafe { *Box::from_raw(t) }
    }

    fn store_err<S: VfsStore<AppData = T>>(&self, err: VfsError) -> i32 {
        let code = err.raw_code();
        S::record_error(&self.data, err);
        code
    }
}

/// Provides shared access to application data. Synchronization is the backend's responsibility.
impl<T> Deref for VfsAppData<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

/// I/O on an independently opened handle; underlying file data may be shared.
/// Offsets/sizes are byte counts independent of pointer width. Reject values
/// beyond backend limits rather than truncating them; buffers remain address-space limited.
pub trait VfsFile {
    /// Optional preallocation hint, never a request to shrink the file.
    /// Return true when handled, false when unsupported. This is only an
    /// optimization; do not assume SQLite always sends it before writing.
    fn size_hint(&mut self, _size: u64) -> VfsResult<bool> {
        Ok(false)
    }

    /// Minimum write unit which can disturb neighboring bytes.
    fn sector_size(&self) -> SectorSize {
        SectorSize::DEFAULT
    }

    /// Only advertise guarantees actually provided by this file's storage.
    fn device_characteristics(&self) -> DeviceCharacteristics {
        DeviceCharacteristics::NONE
    }

    /// Reads at most `buf.len()` bytes, returning the number written to `buf`.
    ///
    /// Fill as much as available; return zero at/beyond EOF or for an empty buffer.
    /// Short reads mean EOF and are not retried. Return `Ok(count)`, never
    /// `IoShortRead`: `xRead` needs the count to zero-fill the unread tail and
    /// report `SQLITE_IOERR_SHORT_READ`.
    fn read(&mut self, buf: &mut [u8], offset: u64) -> VfsResult<usize>;

    /// Writes the entire buffer or returns an error; partial success must not
    /// be reported as `Ok(())`.
    fn write(&mut self, buf: &[u8], offset: u64) -> VfsResult<()>;

    /// Truncates the logical file to `size` bytes, as requested by `xTruncate`.
    /// Return an error if the requested size cannot be applied.
    fn truncate(&mut self, size: u64) -> VfsResult<()>;

    /// Synchronizes prior writes according to SQLite's requested semantics.
    /// `Full` is not `PRAGMA synchronous=FULL`; see `SyncMode`.
    fn sync(&mut self, options: SyncOptions) -> VfsResult<()>;

    /// Returns the logical file length in bytes, not its allocated storage size.
    fn size(&self) -> VfsResult<u64>;

    /// Upgrades to `level`, leaving an already higher lock unchanged. Return
    /// `Busy` for contention; failed upgrades may retain a PENDING lock.
    /// SQLite never requests `None` here.
    fn lock(&mut self, level: LockLevel) -> VfsResult<()>;

    /// Downgrades to Shared or None, leaving an already lower lock unchanged.
    fn unlock(&mut self, level: LockLevel) -> VfsResult<()>;

    /// Whether any connection (including this one) holds Reserved, Pending, or
    /// Exclusive on this file. This must include other processes where relevant.
    fn check_reserved_lock(&self) -> VfsResult<bool>;
}

/// Opens backend handles and manages the file namespace. I/O uses the returned
/// handle directly, without looking up its filename again.
pub trait VfsStore {
    /// Backend handle retained from xOpen until xClose.
    type File: VfsFile + 'static;
    type AppData: 'static;

    /// Records optional diagnostics for `xGetLastError`. Concurrent backends must
    /// associate diagnostics with the calling thread, not just serialize access
    /// to a shared last-error slot. The default discards diagnostics;
    /// callback result codes are still returned to SQLite unchanged.
    fn record_error(_data: &Self::AppData, _error: VfsError) {}

    /// Returns the calling thread's snapshot without consuming it. Override with `record_error`
    /// to support `xGetLastError`; the default reports no additional diagnostic.
    fn last_error(_data: &Self::AppData) -> Option<VfsError> {
        None
    }

    /// Returns a fresh handle for each `xOpen`. Create only with CREATE;
    /// CREATE | EXCLUSIVE must reject existing files. Report actual access in
    /// `OpenedFile`; read-only fallback is allowed, read-write upgrades are not.
    /// Keep the resource alive until close, regardless of namespace changes.
    /// Clean up failed opens here: no close follows.
    fn open_file(
        data: &Self::AppData,
        request: OpenRequest<'_>,
    ) -> VfsResult<OpenedFile<Self::File>>;

    /// Closes the handle even on error. DELETEONCLOSE must remove the opened
    /// resource, not a same-name replacement; unlinking at open is allowed.
    /// `name` is absent for anonymous opens; `options` is the original request,
    /// not the actual access reported by `OpenedFile`.
    fn close_file(
        data: &Self::AppData,
        name: Option<&str>,
        file: Self::File,
        options: OpenOptions,
    ) -> VfsResult<()>;

    /// Checks existence or permissions as requested, without opening the file.
    fn access(data: &Self::AppData, name: &str, mode: AccessMode) -> VfsResult<bool>;

    /// Returns a canonical, NUL-free UTF-8 name. Flat namespaces can return
    /// `name` unchanged; filesystem backends must resolve aliases/relative paths.
    fn full_pathname(data: &Self::AppData, name: &str) -> VfsResult<String>;

    /// Deletes a file. If `sync_dir` is true, synchronize its containing
    /// directory/namespace before returning success.
    fn delete_file(data: &Self::AppData, name: &str, sync_dir: bool) -> VfsResult<()>;
}

/// Platform services used by the default VFS callbacks.
pub trait OsCallback {
    /// Suspends execution for at least `dur`. The default `xSleep` reports this
    /// requested duration to SQLite; a no-op is not a conforming sleep service.
    fn sleep(&self, dur: Duration);

    /// Returns the actual number of random bytes written, at most `buf.len()`.
    /// Return zero when randomness is unavailable.
    fn random(&self, buf: &mut [u8]) -> usize;

    /// Returns UTC milliseconds since 1970-01-01 00:00:00 (the Unix epoch).
    /// Negative values represent earlier times. Return an error if the clock
    /// is unavailable; the default callbacks convert this value to Julian time.
    fn epoch_timestamp_in_ms(&self) -> VfsResult<i64>;
}

/// SQLite VFS callbacks, delegating to typed `VfsStore` methods by default.
/// See the [SQLite VFS contract](https://www.sqlite.org/c3ref/vfs.html).
///
/// # Raw callback safety
/// Callers must satisfy the SQLite contract for each callback: pointers must
/// have the required lifetime, alignment and readable/writable extent. The VFS
/// must carry the matching store's app data, and `xOpen` needs storage for
/// [`SQLiteVfsFile`]. Shared app data requires the backend's synchronization;
/// mutable buffers must not alias other active accesses.
/// Backend panics are not converted to SQLite errors and cannot unwind through
/// these `extern "C"` callbacks. Return [`VfsResult`] errors for recoverable failures.
#[allow(clippy::missing_safety_doc)]
pub trait SQLiteVfs<IO: SQLiteIoMethods> {
    /// Platform services used by the default callbacks.
    type Os: OsCallback + ?Sized;

    /// Borrows the services for this VFS instance. Threading follows the same
    /// requirements as the backend app data; no global service is required.
    fn os(data: &<IO::Store as VfsStore>::AppData) -> &Self::Os;

    /// SQLite `sqlite3_vfs.iVersion`, not a crate or backend version.
    /// Version 2 adds the integer time callback; version 3 adds system-call hooks.
    const VERSION: ::core::ffi::c_int;
    /// Maximum pathname length in bytes advertised as `sqlite3_vfs.mxPathname`.
    const MAX_PATH_SIZE: ::core::ffi::c_int = 1024;

    /// Builds the raw SQLite VFS structure without registering it.
    ///
    /// # Safety
    ///
    /// `vfs_name` must be NUL-terminated and `app_data` must point to a valid
    /// `VfsAppData<<IO::Store as VfsStore>::AppData>`; both must outlive the VFS
    /// and open files. Versions, method table, layout and callbacks must agree.
    /// Obey their threading/aliasing rules and [`register_vfs`]'s ownership contract.
    unsafe fn vfs(
        vfs_name: *const ::core::ffi::c_char,
        app_data: *mut VfsAppData<<IO::Store as VfsStore>::AppData>,
    ) -> sqlite3_vfs {
        sqlite3_vfs {
            iVersion: Self::VERSION,
            szOsFile: core::mem::size_of::<SQLiteVfsFile>() as i32,
            mxPathname: Self::MAX_PATH_SIZE,
            pNext: core::ptr::null_mut(),
            zName: vfs_name,
            pAppData: app_data.cast(),
            xOpen: Some(Self::xOpen),
            xDelete: Some(Self::xDelete),
            xAccess: Some(Self::xAccess),
            xFullPathname: Some(Self::xFullPathname),
            xDlOpen: Some(Self::xDlOpen),
            xDlError: Some(Self::xDlError),
            xDlSym: None,
            xDlClose: None,
            xRandomness: Some(Self::xRandomness),
            xSleep: Some(Self::xSleep),
            xCurrentTime: Some(Self::xCurrentTime),
            xGetLastError: Some(Self::xGetLastError),
            xCurrentTimeInt64: Some(Self::xCurrentTimeInt64),
            xSetSystemCall: None,
            xGetSystemCall: None,
            xNextSystemCall: None,
        }
    }

    unsafe extern "C" fn xOpen(
        pVfs: *mut sqlite3_vfs,
        zName: sqlite3_filename,
        pFile: *mut sqlite3_file,
        flags: ::core::ffi::c_int,
        pOutFlags: *mut ::core::ffi::c_int,
    ) -> ::core::ffi::c_int {
        unsafe {
            // SQLite must not call xClose when opening fails before a handle exists.
            (*pFile).pMethods = core::ptr::null();
            let app_data = VfsAppData::<<IO::Store as VfsStore>::AppData>::get(pVfs);

            let options = match OpenOptions::from_raw_flags(flags) {
                Ok(options) => options,
                Err(err) => return app_data.store_err::<IO::Store>(err),
            };

            let filename = if zName.is_null() {
                None
            } else {
                let raw = CStr::from_ptr(zName);
                let path = match raw.to_str() {
                    Ok(path) => path,
                    Err(_) => {
                        return app_data.store_err::<IO::Store>(VfsError::new(
                            VfsErrorCode::CantOpen,
                            "filename is not valid UTF-8".into(),
                        ));
                    }
                };
                Some(VfsFilename::from_sqlite(
                    path,
                    matches!(
                        options.kind(),
                        Some(FileKind::MainDb | FileKind::MainJournal | FileKind::Wal)
                    )
                    .then_some(zName),
                ))
            };

            let opened = match IO::Store::open_file(app_data, OpenRequest { filename, options }) {
                Ok(handle) => handle,
                Err(err) => return app_data.store_err::<IO::Store>(err),
            };

            let vfs_file = pFile.cast::<SQLiteVfsFile>();
            (*vfs_file).vfs = pVfs;
            (*vfs_file).flags = flags;
            (*vfs_file).name_ptr = zName.cast();
            (*vfs_file).name_length = filename.map_or(0, |name| name.path().len());
            (*vfs_file).handle_ptr = Box::into_raw(Box::new(opened.file)).cast();

            (*pFile).pMethods = &IO::METHODS;

            if !pOutFlags.is_null() {
                *pOutFlags = (flags & !(SQLITE_OPEN_READONLY | SQLITE_OPEN_READWRITE))
                    | match opened.access {
                        OpenAccess::ReadOnly => SQLITE_OPEN_READONLY,
                        OpenAccess::ReadWrite => SQLITE_OPEN_READWRITE,
                    };
            }

            SQLITE_OK
        }
    }

    unsafe extern "C" fn xDelete(
        pVfs: *mut sqlite3_vfs,
        zName: *const ::core::ffi::c_char,
        syncDir: ::core::ffi::c_int,
    ) -> ::core::ffi::c_int {
        unsafe {
            let app_data = VfsAppData::<<IO::Store as VfsStore>::AppData>::get(pVfs);
            if zName.is_null() {
                return app_data.store_err::<IO::Store>(VfsError::new(
                    VfsErrorCode::IoDelete,
                    "filename is null".into(),
                ));
            }
            let s = match CStr::from_ptr(zName).to_str() {
                Ok(name) => name,
                Err(_) => {
                    return app_data.store_err::<IO::Store>(VfsError::new(
                        VfsErrorCode::IoDelete,
                        "filename is not valid UTF-8".into(),
                    ));
                }
            };
            if let Err(err) = IO::Store::delete_file(app_data, s, syncDir != 0) {
                app_data.store_err::<IO::Store>(err)
            } else {
                SQLITE_OK
            }
        }
    }

    /// Delegates the requested existence/permission check without opening a file.
    unsafe extern "C" fn xAccess(
        pVfs: *mut sqlite3_vfs,
        zName: *const ::core::ffi::c_char,
        flags: ::core::ffi::c_int,
        pResOut: *mut ::core::ffi::c_int,
    ) -> ::core::ffi::c_int {
        unsafe {
            *pResOut = 0;
            let app_data = VfsAppData::<<IO::Store as VfsStore>::AppData>::get(pVfs);
            if zName.is_null() {
                return app_data.store_err::<IO::Store>(VfsError::new(
                    VfsErrorCode::IoAccess,
                    "filename is null".into(),
                ));
            }
            let Some(mode) = AccessMode::from_raw(flags) else {
                return app_data.store_err::<IO::Store>(VfsError::new(
                    VfsErrorCode::IoAccess,
                    "invalid access mode".into(),
                ));
            };
            let file = match CStr::from_ptr(zName).to_str() {
                Ok(name) => name,
                Err(_) => {
                    return app_data.store_err::<IO::Store>(VfsError::new(
                        VfsErrorCode::IoAccess,
                        "filename is not valid UTF-8".into(),
                    ));
                }
            };
            let exist = match IO::Store::access(app_data, file, mode) {
                Ok(exist) => exist,
                Err(err) => return app_data.store_err::<IO::Store>(err),
            };
            *pResOut = i32::from(exist);

            SQLITE_OK
        }
    }

    /// Copies the backend's canonical name into SQLite's buffer, including NUL.
    unsafe extern "C" fn xFullPathname(
        pVfs: *mut sqlite3_vfs,
        zName: *const ::core::ffi::c_char,
        nOut: ::core::ffi::c_int,
        zOut: *mut ::core::ffi::c_char,
    ) -> ::core::ffi::c_int {
        unsafe {
            let app_data = VfsAppData::<<IO::Store as VfsStore>::AppData>::get(pVfs);
            if zName.is_null() {
                return app_data.store_err::<IO::Store>(VfsError::new(
                    VfsErrorCode::CantOpen,
                    "filename is null".into(),
                ));
            }
            if zOut.is_null() || nOut <= 0 {
                return app_data.store_err::<IO::Store>(VfsError::new(
                    VfsErrorCode::CantOpen,
                    "pathname output buffer is missing or empty".into(),
                ));
            }
            let name = match CStr::from_ptr(zName).to_str() {
                Ok(name) => name,
                Err(_) => {
                    return app_data.store_err::<IO::Store>(VfsError::new(
                        VfsErrorCode::CantOpen,
                        "filename is not valid UTF-8".into(),
                    ));
                }
            };
            let full = match IO::Store::full_pathname(app_data, name) {
                Ok(full) => full,
                Err(err) => return app_data.store_err::<IO::Store>(err),
            };
            if full.as_bytes().contains(&0) {
                return app_data.store_err::<IO::Store>(VfsError::new(
                    VfsErrorCode::CantOpen,
                    "canonical filename contains a NUL byte".into(),
                ));
            }
            if full.len() >= nOut as usize {
                return app_data.store_err::<IO::Store>(VfsError::new(
                    VfsErrorCode::CantOpen,
                    "pathname output buffer is too small".into(),
                ));
            }
            full.as_ptr().copy_to(zOut.cast(), full.len());
            zOut.add(full.len()).write(0);
            SQLITE_OK
        }
    }

    /// Reports the last backend diagnostic without consuming it. The return value
    /// is its OS error number, or zero when absent, never its SQLite result code.
    /// SQLite may query just the number with a null output buffer.
    unsafe extern "C" fn xGetLastError(
        pVfs: *mut sqlite3_vfs,
        nOut: ::core::ffi::c_int,
        zOut: *mut ::core::ffi::c_char,
    ) -> ::core::ffi::c_int {
        unsafe {
            let app_data = VfsAppData::<<IO::Store as VfsStore>::AppData>::get(pVfs);
            let Some(error) = IO::Store::last_error(app_data) else {
                write_message(zOut, nOut, "");
                return SQLITE_OK;
            };
            write_message(zOut, nOut, error.message());
            error.system_error().map_or(0, SystemErrorCode::as_raw)
        }
    }

    /// Dynamic extension loading is unsupported by default. Returning null
    /// lets SQLite report an error instead of calling a null method pointer.
    unsafe extern "C" fn xDlOpen(
        _pVfs: *mut sqlite3_vfs,
        _zFilename: *const core::ffi::c_char,
    ) -> *mut core::ffi::c_void {
        core::ptr::null_mut()
    }

    /// Override together with `xDlOpen` and the method table's `xDlSym` and
    /// `xDlClose` entries when supporting dynamic extensions.
    unsafe extern "C" fn xDlError(
        _pVfs: *mut sqlite3_vfs,
        nByte: i32,
        zErrMsg: *mut core::ffi::c_char,
    ) {
        unsafe { write_message(zErrMsg, nByte, "dynamic extension loading is not supported") }
    }

    /// Fills the output through [`OsCallback::random`] and returns its byte count,
    /// or zero if no randomness is available or the backend reports an invalid count.
    unsafe extern "C" fn xRandomness(
        pVfs: *mut sqlite3_vfs,
        nByte: ::core::ffi::c_int,
        zOut: *mut ::core::ffi::c_char,
    ) -> ::core::ffi::c_int {
        unsafe {
            if nByte <= 0 || zOut.is_null() {
                return 0;
            }
            let data = VfsAppData::<<IO::Store as VfsStore>::AppData>::get(pVfs);
            zOut.cast::<u8>().write_bytes(0, nByte as usize);
            let slice = core::slice::from_raw_parts_mut(zOut.cast(), nByte as usize);
            let count = Self::os(data).random(slice);
            if count > slice.len() {
                0
            } else {
                count as i32
            }
        }
    }

    /// Reports the platform clock as a Julian day number, including its fraction.
    unsafe extern "C" fn xCurrentTime(
        pVfs: *mut sqlite3_vfs,
        pTimeOut: *mut f64,
    ) -> ::core::ffi::c_int {
        unsafe {
            *pTimeOut = 0.0;
            let data = VfsAppData::<<IO::Store as VfsStore>::AppData>::get(pVfs);
            match Self::os(data).epoch_timestamp_in_ms() {
                Ok(time) => {
                    *pTimeOut = 2440587.5 + (time as f64 / 86400000.0);
                    SQLITE_OK
                }
                Err(error) => data.store_err::<IO::Store>(error),
            }
        }
    }

    /// Reports the Julian day number multiplied by 86,400,000 as an integer.
    /// Returns an error if the clock is unavailable or the epoch conversion overflows.
    unsafe extern "C" fn xCurrentTimeInt64(
        pVfs: *mut sqlite3_vfs,
        pOut: *mut sqlite3_int64,
    ) -> ::core::ffi::c_int {
        unsafe {
            *pOut = 0;
            let data = VfsAppData::<<IO::Store as VfsStore>::AppData>::get(pVfs);
            let time = match Self::os(data).epoch_timestamp_in_ms() {
                Ok(time) => time,
                Err(error) => return data.store_err::<IO::Store>(error),
            };
            *pOut = match time.checked_add(210_866_760_000_000) {
                Some(time) => time,
                None => {
                    return data.store_err::<IO::Store>(VfsError::new(
                        VfsErrorCode::Error,
                        "Julian timestamp exceeds the signed 64-bit range".into(),
                    ));
                }
            };
            SQLITE_OK
        }
    }

    unsafe extern "C" fn xSleep(
        pVfs: *mut sqlite3_vfs,
        microseconds: ::core::ffi::c_int,
    ) -> ::core::ffi::c_int {
        if microseconds <= 0 {
            return 0;
        }
        let dur = Duration::from_micros(microseconds as u64);
        unsafe {
            let data = VfsAppData::<<IO::Store as VfsStore>::AppData>::get(pVfs);
            Self::os(data).sleep(dur);
        }
        microseconds
    }
}

/// SQLite I/O callbacks, delegating to `VfsFile` by default.
/// The backend supplies locking/sync; WAL requires shared-memory overrides.
/// See the [SQLite I/O contract](https://www.sqlite.org/c3ref/io_methods.html).
///
/// # Raw callback safety
/// File pointers must refer to a live [`SQLiteVfsFile`] initialized by a
/// successful matching `xOpen`, with its VFS and backend data still alive.
/// Call `xClose` only once. Buffers and other pointers must satisfy the SQLite
/// contract for each method, including their extent, alignment and lifetime.
/// Serialize callbacks on each handle where needed to preserve Rust's exclusive
/// access rules, and obey the backend's threading requirements. As with
/// [`SQLiteVfs`], backend panics are not converted to errors.
#[allow(clippy::missing_safety_doc)]
pub trait SQLiteIoMethods {
    type Store: VfsStore;

    /// SQLite `sqlite3_io_methods.iVersion`. Version 2 permits shared-memory
    /// callbacks; version 3 permits fetch/unfetch. Raising this value alone
    /// does not implement those capabilities.
    const VERSION: ::core::ffi::c_int;

    /// Method table installed by the default `xOpen`. Shared-memory methods are
    /// absent; fetch/unfetch decline memory mapping. Overrides must keep the
    /// version, file layout and backend types consistent with the VFS.
    const METHODS: sqlite3_io_methods = sqlite3_io_methods {
        iVersion: Self::VERSION,
        xClose: Some(Self::xClose),
        xRead: Some(Self::xRead),
        xWrite: Some(Self::xWrite),
        xTruncate: Some(Self::xTruncate),
        xSync: Some(Self::xSync),
        xFileSize: Some(Self::xFileSize),
        xLock: Some(Self::xLock),
        xUnlock: Some(Self::xUnlock),
        xCheckReservedLock: Some(Self::xCheckReservedLock),
        xFileControl: Some(Self::xFileControl),
        xSectorSize: Some(Self::xSectorSize),
        xDeviceCharacteristics: Some(Self::xDeviceCharacteristics),
        xShmMap: None,
        xShmLock: None,
        xShmBarrier: None,
        xShmUnmap: None,
        xFetch: Some(Self::xFetch),
        xUnfetch: Some(Self::xUnfetch),
    };

    unsafe extern "C" fn xClose(pFile: *mut sqlite3_file) -> ::core::ffi::c_int {
        unsafe {
            let vfs_file = &mut *SQLiteVfsFile::from_file(pFile);
            let app_data = VfsAppData::<<Self::Store as VfsStore>::AppData>::get(vfs_file.vfs);
            let handle = *Box::from_raw(
                vfs_file
                    .handle_ptr
                    .cast::<<Self::Store as VfsStore>::File>(),
            );
            let name = if vfs_file.name_ptr.is_null() {
                None
            } else {
                Some(core::str::from_utf8_unchecked(core::slice::from_raw_parts(
                    vfs_file.name_ptr,
                    vfs_file.name_length,
                )))
            };
            vfs_file.handle_ptr = core::ptr::null_mut();
            vfs_file.name_ptr = core::ptr::null();
            vfs_file.name_length = 0;
            vfs_file.io_methods.pMethods = core::ptr::null();
            let options = match OpenOptions::from_raw_flags(vfs_file.flags) {
                Ok(options) => options,
                Err(err) => return app_data.store_err::<Self::Store>(err),
            };
            // The backend consumes the handle even if close/delete fails.
            match Self::Store::close_file(app_data, name, handle, options) {
                Ok(()) => SQLITE_OK,
                Err(err) => app_data.store_err::<Self::Store>(err),
            }
        }
    }

    unsafe extern "C" fn xRead(
        pFile: *mut sqlite3_file,
        zBuf: *mut ::core::ffi::c_void,
        iAmt: ::core::ffi::c_int,
        iOfst: sqlite3_int64,
    ) -> ::core::ffi::c_int {
        unsafe {
            let vfs_file = &mut *SQLiteVfsFile::from_file(pFile);
            let app_data = VfsAppData::<<Self::Store as VfsStore>::AppData>::get(vfs_file.vfs);

            let f = |file: &mut <Self::Store as VfsStore>::File| {
                let size = usize::try_from(iAmt).map_err(|_| {
                    VfsError::new(VfsErrorCode::IoRead, "negative read length".into())
                })?;
                let offset = u64::try_from(iOfst).map_err(|_| {
                    VfsError::new(VfsErrorCode::IoRead, "negative file offset".into())
                })?;
                if size == 0 {
                    return Ok(SQLITE_OK);
                }
                // SQLite may supply uninitialized output memory. Safe backend
                // code is allowed to inspect any byte of its Rust slice.
                zBuf.cast::<u8>().write_bytes(0, size);
                let slice = core::slice::from_raw_parts_mut(zBuf.cast::<u8>(), size);
                let n_read = match file.read(slice, offset) {
                    Ok(count) => count,
                    Err(err) => {
                        // An error has no byte count, so we cannot identify the
                        // unread tail. Fail rather than report an EOF containing
                        // uninitialized bytes or discard a valid data prefix.
                        if err.raw_code() == SQLITE_IOERR_SHORT_READ {
                            return Err(VfsError::new(
                                VfsErrorCode::IoRead,
                                "backend must return a byte count for short reads".into(),
                            ));
                        }
                        return Err(err);
                    }
                };
                if n_read > size {
                    return Err(VfsError::new(
                        VfsErrorCode::IoRead,
                        "read count exceeds buffer length".into(),
                    ));
                }
                if n_read < size {
                    slice[n_read..].fill(0);
                    return Err(VfsError::new(
                        VfsErrorCode::IoShortRead,
                        "short read at end of file".into(),
                    ));
                }
                Ok(SQLITE_OK)
            };

            match f(vfs_file.handle_mut::<<Self::Store as VfsStore>::File>()) {
                Ok(code) => code,
                Err(err) => app_data.store_err::<Self::Store>(err),
            }
        }
    }

    unsafe extern "C" fn xWrite(
        pFile: *mut sqlite3_file,
        zBuf: *const ::core::ffi::c_void,
        iAmt: ::core::ffi::c_int,
        iOfst: sqlite3_int64,
    ) -> ::core::ffi::c_int {
        unsafe {
            let vfs_file = &mut *SQLiteVfsFile::from_file(pFile);
            let app_data = VfsAppData::<<Self::Store as VfsStore>::AppData>::get(vfs_file.vfs);

            let f = |file: &mut <Self::Store as VfsStore>::File| {
                let size = usize::try_from(iAmt).map_err(|_| {
                    VfsError::new(VfsErrorCode::IoWrite, "negative write length".into())
                })?;
                let offset = u64::try_from(iOfst).map_err(|_| {
                    VfsError::new(VfsErrorCode::IoWrite, "negative file offset".into())
                })?;
                if size == 0 {
                    return Ok(SQLITE_OK);
                }
                let slice = core::slice::from_raw_parts(zBuf.cast::<u8>(), size);
                file.write(slice, offset)?;
                Ok(SQLITE_OK)
            };

            match f(vfs_file.handle_mut::<<Self::Store as VfsStore>::File>()) {
                Ok(code) => code,
                Err(err) => app_data.store_err::<Self::Store>(err),
            }
        }
    }

    unsafe extern "C" fn xTruncate(
        pFile: *mut sqlite3_file,
        size: sqlite3_int64,
    ) -> ::core::ffi::c_int {
        unsafe {
            let vfs_file = &mut *SQLiteVfsFile::from_file(pFile);
            let app_data = VfsAppData::<<Self::Store as VfsStore>::AppData>::get(vfs_file.vfs);

            let f = |file: &mut <Self::Store as VfsStore>::File| {
                let size = u64::try_from(size).map_err(|_| {
                    VfsError::new(VfsErrorCode::IoTruncate, "negative file size".into())
                })?;
                file.truncate(size)?;
                Ok(SQLITE_OK)
            };

            match f(vfs_file.handle_mut::<<Self::Store as VfsStore>::File>()) {
                Ok(code) => code,
                Err(err) => app_data.store_err::<Self::Store>(err),
            }
        }
    }

    unsafe extern "C" fn xSync(
        pFile: *mut sqlite3_file,
        flags: ::core::ffi::c_int,
    ) -> ::core::ffi::c_int {
        unsafe {
            let vfs_file = &mut *SQLiteVfsFile::from_file(pFile);
            let app_data = VfsAppData::<<Self::Store as VfsStore>::AppData>::get(vfs_file.vfs);

            let Some(options) = SyncOptions::from_raw_flags(flags) else {
                return app_data.store_err::<Self::Store>(VfsError::new(
                    VfsErrorCode::IoSync,
                    "invalid sync flags".into(),
                ));
            };

            let f = |file: &mut <Self::Store as VfsStore>::File| {
                file.sync(options)?;
                Ok(SQLITE_OK)
            };

            match f(vfs_file.handle_mut::<<Self::Store as VfsStore>::File>()) {
                Ok(code) => code,
                Err(err) => app_data.store_err::<Self::Store>(err),
            }
        }
    }

    unsafe extern "C" fn xFileSize(
        pFile: *mut sqlite3_file,
        pSize: *mut sqlite3_int64,
    ) -> ::core::ffi::c_int {
        unsafe {
            *pSize = 0;
            let vfs_file = &*SQLiteVfsFile::from_file(pFile);
            let app_data = VfsAppData::<<Self::Store as VfsStore>::AppData>::get(vfs_file.vfs);

            let f = |file: &<Self::Store as VfsStore>::File| {
                let size = sqlite3_int64::try_from(file.size()?).map_err(|_| {
                    VfsError::new(
                        VfsErrorCode::IoStat,
                        "file size exceeds the signed 64-bit range".into(),
                    )
                })?;
                *pSize = size;
                Ok(SQLITE_OK)
            };

            match f(vfs_file.handle::<<Self::Store as VfsStore>::File>()) {
                Ok(code) => code,
                Err(err) => app_data.store_err::<Self::Store>(err),
            }
        }
    }

    unsafe extern "C" fn xLock(
        pFile: *mut sqlite3_file,
        eLock: ::core::ffi::c_int,
    ) -> ::core::ffi::c_int {
        unsafe {
            let file = &mut *SQLiteVfsFile::from_file(pFile);
            let data = VfsAppData::<<Self::Store as VfsStore>::AppData>::get(file.vfs);
            let level = match LockLevel::from_raw(eLock) {
                Some(level) if level != LockLevel::None => level,
                _ => {
                    return data.store_err::<Self::Store>(VfsError::new(
                        VfsErrorCode::IoLock,
                        "invalid lock level".into(),
                    ));
                }
            };
            match file
                .handle_mut::<<Self::Store as VfsStore>::File>()
                .lock(level)
            {
                Ok(()) => SQLITE_OK,
                Err(err) => data.store_err::<Self::Store>(err),
            }
        }
    }

    unsafe extern "C" fn xUnlock(
        pFile: *mut sqlite3_file,
        eLock: ::core::ffi::c_int,
    ) -> ::core::ffi::c_int {
        unsafe {
            let file = &mut *SQLiteVfsFile::from_file(pFile);
            let data = VfsAppData::<<Self::Store as VfsStore>::AppData>::get(file.vfs);
            let level = match LockLevel::from_raw(eLock) {
                Some(level @ (LockLevel::None | LockLevel::Shared)) => level,
                _ => {
                    return data.store_err::<Self::Store>(VfsError::new(
                        VfsErrorCode::IoUnlock,
                        "invalid unlock level".into(),
                    ));
                }
            };
            match file
                .handle_mut::<<Self::Store as VfsStore>::File>()
                .unlock(level)
            {
                Ok(()) => SQLITE_OK,
                Err(err) => data.store_err::<Self::Store>(err),
            }
        }
    }

    unsafe extern "C" fn xCheckReservedLock(
        pFile: *mut sqlite3_file,
        pResOut: *mut ::core::ffi::c_int,
    ) -> ::core::ffi::c_int {
        unsafe {
            *pResOut = 0;
            let file = &*SQLiteVfsFile::from_file(pFile);
            let data = VfsAppData::<<Self::Store as VfsStore>::AppData>::get(file.vfs);
            match file
                .handle::<<Self::Store as VfsStore>::File>()
                .check_reserved_lock()
            {
                Ok(held) => {
                    *pResOut = i32::from(held);
                    SQLITE_OK
                }
                Err(err) => data.store_err::<Self::Store>(err),
            }
        }
    }

    unsafe extern "C" fn xFileControl(
        pFile: *mut sqlite3_file,
        op: ::core::ffi::c_int,
        pArg: *mut ::core::ffi::c_void,
    ) -> ::core::ffi::c_int {
        if op != SQLITE_FCNTL_SIZE_HINT {
            return SQLITE_NOTFOUND;
        }
        unsafe {
            let file = &mut *SQLiteVfsFile::from_file(pFile);
            let data = VfsAppData::<<Self::Store as VfsStore>::AppData>::get(file.vfs);
            let Ok(size) = u64::try_from(*pArg.cast::<i64>()) else {
                return data.store_err::<Self::Store>(VfsError::new(
                    VfsErrorCode::Io,
                    "negative file size hint".into(),
                ));
            };
            match file
                .handle_mut::<<Self::Store as VfsStore>::File>()
                .size_hint(size)
            {
                Ok(true) => SQLITE_OK,
                Ok(false) => SQLITE_NOTFOUND,
                Err(error) => data.store_err::<Self::Store>(error),
            }
        }
    }

    /// Uses SQLite's usual fallback sector size. Override when the backend's
    /// minimum write unit that can disturb neighboring bytes differs.
    unsafe extern "C" fn xSectorSize(pFile: *mut sqlite3_file) -> ::core::ffi::c_int {
        unsafe {
            (&*SQLiteVfsFile::from_file(pFile))
                .handle::<<Self::Store as VfsStore>::File>()
                .sector_size()
                .bytes() as i32
        }
    }

    unsafe extern "C" fn xDeviceCharacteristics(pFile: *mut sqlite3_file) -> ::core::ffi::c_int {
        unsafe {
            (&*SQLiteVfsFile::from_file(pFile))
                .handle::<<Self::Store as VfsStore>::File>()
                .device_characteristics()
                .as_raw()
        }
    }

    /// Declines memory mapping; SQLite falls back to `xRead`. Required as a
    /// callable entry when `VERSION` is 3, even if this backend never maps files.
    unsafe extern "C" fn xFetch(
        _pFile: *mut sqlite3_file,
        _iOfst: sqlite3_int64,
        _iAmt: i32,
        pp: *mut *mut core::ffi::c_void,
    ) -> i32 {
        unsafe { *pp = core::ptr::null_mut() };
        SQLITE_OK
    }

    /// Nothing to release for the default non-mapping `xFetch`.
    unsafe extern "C" fn xUnfetch(
        _pFile: *mut sqlite3_file,
        _iOfst: sqlite3_int64,
        _p: *mut core::ffi::c_void,
    ) -> i32 {
        SQLITE_OK
    }
}

/// Database signature, size or page-layout validation errors, not a complete
/// integrity check. Match variants rather than the human-readable display text.
#[derive(thiserror::Error, Debug)]
pub enum ImportDbError {
    #[error("invalid database size or page alignment")]
    InvalidDbSize,
    #[error("invalid SQLite database signature")]
    InvalidHeader,
    #[error("page size must be a power of two between 512 and 65536 bytes")]
    InvalidPageSize,
}

/// Validates the database signature, page size and file alignment, returning
/// the page size. This does not validate database contents or integrity.
pub fn check_import_db(bytes: &[u8]) -> Result<usize, ImportDbError> {
    let length = bytes.len();

    if length < 512 || length % 512 != 0 {
        return Err(ImportDbError::InvalidDbSize);
    }

    if !bytes.starts_with(SQLITE3_HEADER.as_bytes()) {
        return Err(ImportDbError::InvalidHeader);
    }

    // The database page size in bytes.
    // Must be a power of two between 512 and 32768 inclusive, or the value 1 representing a page size of 65536.
    let page_size = u16::from_be_bytes([bytes[16], bytes[17]]);
    let page_size = if page_size == 1 {
        65536
    } else {
        usize::from(page_size)
    };

    check_db_and_page_size(length, page_size)?;
    Ok(page_size)
}

/// Validates byte counts: the page size must be a power of two from 512 through
/// 65536, and the database size a multiple of it. An empty database is allowed;
/// this function does not inspect any file contents.
pub fn check_db_and_page_size(db_size: usize, page_size: usize) -> Result<(), ImportDbError> {
    if !(page_size.is_power_of_two() && (512..=65536).contains(&page_size)) {
        return Err(ImportDbError::InvalidPageSize);
    }
    if db_size % page_size != 0 {
        return Err(ImportDbError::InvalidDbSize);
    }
    Ok(())
}

/// Reusable checks for custom VFS implementations.
#[doc(hidden)]
pub mod test_suite;

#[cfg(test)]
mod test_support {
    use super::{OsCallback, VfsResult};

    pub(crate) struct CallbackOs<const NOW: i64>;
    impl<const NOW: i64> OsCallback for CallbackOs<NOW> {
        fn sleep(&self, _: core::time::Duration) {}

        fn random(&self, buf: &mut [u8]) -> usize {
            assert!(!buf.is_empty());
            buf.fill(42);
            buf.len()
        }

        fn epoch_timestamp_in_ms(&self) -> VfsResult<i64> {
            Ok(NOW)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MemChunksFile, VfsErrorCode, VfsFile};

    #[test]
    fn test_chunks_file() {
        // Force a capacity overflow without relying on actual system exhaustion.
        let mut small = MemChunksFile::new(1);
        small.write(&[7], 0).unwrap();
        assert_eq!(
            small
                .write(&[9], (usize::MAX / 2) as u64)
                .unwrap_err()
                .code(),
            VfsErrorCode::NoMemory
        );
        assert_eq!(small.size().unwrap(), 1);
        let mut retained = [0];
        assert_eq!(small.read(&mut retained, 0).unwrap(), 1);
        assert_eq!(retained, [7]);

        let mut file = MemChunksFile::new(512);
        file.write(&[], 0).unwrap();
        assert!(file.size().unwrap() == 0);

        let mut buffer = [1; 2];
        let ret = file.read(&mut buffer, 0).unwrap();
        assert_eq!(ret, 0);
        assert_eq!([1; 2], buffer);

        file.write(&[1], 0).unwrap();
        assert!(file.size().unwrap() == 1);
        for size in [2, 513, u64::MAX] {
            assert_eq!(
                file.truncate(size).unwrap_err().code(),
                VfsErrorCode::IoTruncate
            );
            assert_eq!(file.size().unwrap(), 1);
        }
        let mut buffer = [2; 2];
        let ret = file.read(&mut buffer, 0).unwrap();
        assert_eq!(ret, 1);
        assert_eq!([1, 2], buffer);

        let mut file = MemChunksFile::new(512);
        file.write(&[1; 512], 0).unwrap();
        assert!(file.size().unwrap() == 512);
        assert!(file.chunks.len() == 1);

        file.truncate(512).unwrap();
        assert!(file.size().unwrap() == 512);
        assert!(file.chunks.len() == 1);

        file.write(&[41, 42, 43], 511).unwrap();
        assert!(file.size().unwrap() == 514);
        assert!(file.chunks.len() == 2);

        let mut buffer = [0; 3];
        let ret = file.read(&mut buffer, 511).unwrap();
        assert_eq!(ret, 3);
        assert_eq!(buffer, [41, 42, 43]);

        file.truncate(513).unwrap();
        assert!(file.size().unwrap() == 513);
        assert!(file.chunks.len() == 2);

        file.write(&[1], 2048).unwrap();
        assert!(file.size().unwrap() == 2049);
        assert!(file.chunks.len() == 5);

        file.truncate(0).unwrap();
        assert!(file.size().unwrap() == 0);
        assert!(file.chunks.is_empty());
    }

    #[test]
    fn test_chunks_file_read_past_eof() {
        let mut file = MemChunksFile::new(512);

        file.write(&[41, 42], 511).unwrap();

        file.chunks[1][1..].fill(0xAA);

        let mut buf = [99; 512];
        let ret = file.read(&mut buf, 512).unwrap();
        assert_eq!(ret, 1);
        assert_eq!(buf[0], 42);
        assert_eq!(&buf[1..], &[99; 511]);

        let mut buf = [99; 3];
        let ret = file.read(&mut buf, 511).unwrap();
        assert_eq!(ret, 2);
        assert_eq!(buf, [41, 42, 99]);

        for offset in [513, 514, 1u64 << 32, u64::MAX] {
            let mut buf = [99; 3];
            assert_eq!(file.read(&mut buf, offset).unwrap(), 0);
            assert_eq!(buf, [99; 3]);
        }
        for offset in [0, 511, 513, u64::MAX] {
            assert_eq!(file.read(&mut [], offset).unwrap(), 0);
        }
    }
}
