//! Some tools for implementing VFS

use crate::libsqlite3::*;

use fragile::Fragile;
use js_sys::{Date, Math, Number, Uint8Array};
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    ffi::{CStr, CString},
    ops::{Deref, DerefMut},
};
use wasm_array_cp::ArrayBufferCopy;

/// The header of the SQLite file is used to determine whether the imported file is legal.
pub const SQLITE3_HEADER: &str = "SQLite format 3";

/// A [`FragileComfirmed<T>`] wraps a non sendable `T` to be safely send to other threads.
///
/// Once the value has been wrapped it can be sent to other threads but access
/// to the value on those threads will fail.
pub struct FragileComfirmed<T> {
    fragile: Fragile<T>,
}

unsafe impl<T> Send for FragileComfirmed<T> {}
unsafe impl<T> Sync for FragileComfirmed<T> {}

impl<T> FragileComfirmed<T> {
    pub fn new(t: T) -> Self {
        FragileComfirmed {
            fragile: Fragile::new(t),
        }
    }
}

impl<T> Deref for FragileComfirmed<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.fragile.get()
    }
}

impl<T> DerefMut for FragileComfirmed<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.fragile.get_mut()
    }
}

/// get random name if zFileName is null and other cases
pub fn get_random_name() -> String {
    let random = Number::from(Math::random()).to_string(36).unwrap();
    random.slice(2, random.length()).as_string().unwrap()
}

/// Copy `Uint8Array` and return new `Vec<u8>`
pub fn copy_to_vec(src: &Uint8Array) -> Vec<u8> {
    ArrayBufferCopy::to_vec(src)
}

/// Copy `Uint8Array` to `slice`
pub fn copy_to_slice(src: &Uint8Array, dst: &mut [u8]) {
    ArrayBufferCopy::copy_to(src, dst);
}

/// Copy `slice` and return new `Uint8Array`
pub fn copy_to_uint8_array(src: &[u8]) -> Uint8Array {
    ArrayBufferCopy::from_slice(src)
}

/// Copy `slice` to `Unit8Array`
pub fn copy_to_uint8_array_subarray(src: &[u8], dst: &Uint8Array) {
    ArrayBufferCopy::copy_from(dst, src)
}

/// Return error code if expr is true.
///
/// The default error code is SQLITE_ERROR.
#[macro_export]
macro_rules! bail {
    ($ex:expr) => {
        bail!($ex, SQLITE_ERROR);
    };
    ($ex:expr, $code: expr) => {
        if $ex {
            return $code;
        }
    };
}

/// Unpack Option<T>.
///
/// If it is None, return an error code.
///
/// The default error code is SQLITE_ERROR.
#[macro_export]
macro_rules! check_option {
    ($ex:expr) => {
        check_option!($ex, SQLITE_ERROR)
    };
    ($ex:expr, $code: expr) => {
        if let Some(v) = $ex {
            v
        } else {
            return $code;
        }
    };
}

/// Unpack Ok<T>.
///
/// If it is Err, return an error code.
///
/// The default err code is SQLITE_ERROR.
#[macro_export]
macro_rules! check_result {
    ($ex:expr) => {
        check_result!($ex, SQLITE_ERROR)
    };
    ($ex:expr, $code: expr) => {
        if let Ok(v) = $ex {
            v
        } else {
            return $code;
        }
    };
}

/// Mark unused parameter
#[macro_export]
macro_rules! unused {
    ($ex:expr) => {
        let _ = $ex;
    };
}

/// The actual pFile type in Vfs.
///
/// `szOsFile` must be set to the size of `SQLiteVfsFile`.
#[repr(C)]
pub struct SQLiteVfsFile {
    /// The first field must be of type sqlite_file.
    /// In C layout, the pointer to SQLiteVfsFile is the pointer to io_methods.
    pub io_methods: sqlite3_file,
    /// The vfs where the file is located, usually used to manage files.
    pub vfs: *mut sqlite3_vfs,
    /// Flags used to open the database.
    pub flags: i32,
    /// The pointer to the file name.
    /// If it is a leaked static pointer, you need to drop it manually when xClose it.
    pub name_ptr: *const u8,
    /// Length of the file name, on wasm32 platform, usize is u32.
    pub name_length: usize,
}

impl SQLiteVfsFile {
    /// Convert a `sqlite3_file` pointer to a `SQLiteVfsFile` pointer.
    ///
    /// # Safety
    ///
    /// You must ensure that the pointer passed in is `SQLiteVfsFile`
    pub unsafe fn from_file(file: *mut sqlite3_file) -> &'static SQLiteVfsFile {
        &*file.cast::<Self>()
    }

    /// Get the file name.
    ///
    /// # Safety
    ///
    /// When xClose, you can free the memory by `drop(Box::from_raw(ptr));`.
    ///
    /// Do not use again after free.
    pub unsafe fn name(&self) -> &'static mut str {
        // emm, `from_raw_parts_mut` is unstable
        std::str::from_utf8_unchecked_mut(std::slice::from_raw_parts_mut(
            self.name_ptr.cast_mut(),
            self.name_length,
        ))
    }

    /// Convert a `&'static SQLiteVfsFile` pointer to `*mut sqlite3_file` pointer.
    pub fn sqlite3_file(&'static self) -> *mut sqlite3_file {
        self as *const SQLiteVfsFile as *mut sqlite3_file
    }
}

/// Possible errors when registering Vfs
#[derive(thiserror::Error, Debug)]
pub enum RegisterVfsError {
    #[error("An error occurred converting the given vfs name to a CStr")]
    ToCStr,
    #[error("An error occurred while registering vfs with sqlite")]
    RegisterVfs,
}

/// Register vfs general method
pub fn register_vfs<IO: SQLiteIoMethods, V: SQLiteVfs<IO>>(
    vfs_name: &str,
    app_data: IO::AppData,
    default_vfs: bool,
) -> Result<*mut sqlite3_vfs, RegisterVfsError> {
    let name = CString::new(vfs_name).map_err(|_| RegisterVfsError::ToCStr)?;
    let name_ptr = name.into_raw();
    let app_data = VfsAppData::new(app_data).leak();

    let vfs = Box::leak(Box::new(V::vfs(name_ptr, app_data.cast())));
    let ret = unsafe { sqlite3_vfs_register(vfs, i32::from(default_vfs)) };

    if ret != SQLITE_OK {
        unsafe {
            drop(Box::from_raw(vfs));
            drop(CString::from_raw(name_ptr));
            drop(VfsAppData::from_raw(app_data));
        }
        return Err(RegisterVfsError::RegisterVfs);
    }

    Ok(vfs as *mut sqlite3_vfs)
}

/// Generic function for reading by page (block)
pub fn page_read<T, G: Fn(usize) -> Option<T>, R: Fn(T, &mut [u8], (usize, usize))>(
    buf: &mut [u8],
    page_size: usize,
    file_size: usize,
    offset: usize,
    get_page: G,
    read_fn: R,
) -> i32 {
    if page_size == 0 || file_size == 0 {
        buf.fill(0);
        return SQLITE_IOERR_SHORT_READ;
    }

    let mut bytes_read = 0;
    let mut p_data_offset = 0;
    let p_data_length = buf.len();
    let i_offset = offset;

    while p_data_offset < p_data_length {
        let file_offset = i_offset + p_data_offset;
        let page_idx = file_offset / page_size;
        let page_offset = file_offset % page_size;
        let page_addr = page_idx * page_size;

        let Some(page) = get_page(page_addr) else {
            break;
        };

        let page_length = (page_size - page_offset).min(p_data_length - p_data_offset);
        read_fn(
            page,
            &mut buf[p_data_offset..p_data_offset + page_length],
            (page_offset, page_offset + page_length),
        );

        p_data_offset += page_length;
        bytes_read += page_length;
    }

    if bytes_read < p_data_length {
        buf[bytes_read..].fill(0);
        return SQLITE_IOERR_SHORT_READ;
    }

    SQLITE_OK
}

/// Chunks storage in memory, used for temporary file
pub struct MemChunksFile {
    chunks: HashMap<usize, Vec<u8>>,
    chunk_size: Option<usize>,
    file_size: usize,
}

impl MemChunksFile {
    pub fn new(chunk_size: usize) -> Self {
        assert!(chunk_size != 0, "chunk size can't be zero");
        MemChunksFile {
            chunks: HashMap::new(),
            chunk_size: Some(chunk_size),
            file_size: 0,
        }
    }

    pub fn waiting_for_write() -> Self {
        MemChunksFile {
            chunks: HashMap::new(),
            chunk_size: None,
            file_size: 0,
        }
    }
}

impl Default for MemChunksFile {
    fn default() -> Self {
        MemChunksFile::new(512)
    }
}

impl VfsFile for MemChunksFile {
    fn read(&self, buf: &mut [u8], offset: usize) -> VfsResult<i32> {
        let Some(chunk_size) = self.chunk_size else {
            buf.fill(0);
            return Ok(SQLITE_IOERR_SHORT_READ);
        };

        if self.file_size <= offset {
            buf.fill(0);
            return Ok(SQLITE_IOERR_SHORT_READ);
        }

        if chunk_size == buf.len() && offset % chunk_size == 0 {
            if let Some(chunk) = self.chunks.get(&offset) {
                buf.copy_from_slice(chunk);
            }
            Ok(SQLITE_OK)
        } else {
            let mut size = buf.len();
            let chunk_idx = offset / chunk_size;
            let mut remaining_idx = offset % chunk_size;
            let mut offset = 0;

            for key in (chunk_idx..).map(|idx| idx * chunk_size) {
                if let Some(chunk) = self.chunks.get(&key) {
                    let n = std::cmp::min(chunk_size.min(self.file_size) - remaining_idx, size);
                    buf[offset..offset + n]
                        .copy_from_slice(&chunk[remaining_idx..remaining_idx + n]);
                    offset += n;
                    size -= n;
                    remaining_idx = 0;
                } else {
                    break;
                }

                if size == 0 {
                    break;
                }
            }

            if offset < buf.len() {
                buf[offset..].fill(0);
                Ok(SQLITE_IOERR_SHORT_READ)
            } else {
                Ok(SQLITE_OK)
            }
        }
    }

    fn write(&mut self, buf: &[u8], offset: usize) -> VfsResult<()> {
        if buf.is_empty() {
            return Ok(());
        }

        let chunk_size = if let Some(chunk_size) = self.chunk_size {
            chunk_size
        } else {
            let size = buf.len();
            self.chunk_size = Some(size);
            size
        };

        let new_length = self.file_size.max(offset + buf.len());

        if chunk_size == buf.len() && offset % chunk_size == 0 {
            for fill in (self.chunks.len() * chunk_size..offset).step_by(chunk_size) {
                self.chunks.insert(fill, vec![0; chunk_size]);
            }
            if let Some(buffer) = self.chunks.get_mut(&offset) {
                buffer.copy_from_slice(buf);
            } else {
                self.chunks.insert(offset, buf.to_vec());
            }
        } else {
            let mut size = buf.len();
            let chunk_start_idx = offset / chunk_size;
            let chunk_end_idx = (offset + size - 1) / chunk_size;
            let chunks_length = self.chunks.len();

            for key in (chunks_length..=chunk_end_idx).map(|idx| idx * chunk_size) {
                self.chunks.insert(key, vec![0; chunk_size]);
            }

            let mut remaining_idx = offset % chunk_size;
            let mut offset = 0;

            for key in (chunk_start_idx..=chunk_end_idx).map(|idx| idx * chunk_size) {
                let n = std::cmp::min(chunk_size - remaining_idx, size);
                self.chunks.get_mut(&key).unwrap()[remaining_idx..remaining_idx + n]
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

    fn truncate(&mut self, size: usize) -> VfsResult<()> {
        if let Some(chunk_size) = self.chunk_size {
            if size == 0 {
                std::mem::take(&mut self.chunks);
            } else if size % chunk_size == 0 {
                for offset in (size..self.file_size).step_by(chunk_size) {
                    self.chunks.remove(&offset);
                }
            } else {
                let idx = ((size - 1) / chunk_size) + 1;
                for idx in idx..self.chunks.len() {
                    self.chunks.remove(&(idx * chunk_size));
                }
            }
        } else if size != 0 {
            assert_eq!(self.file_size, 0);
            return Err(VfsError::new(SQLITE_IOERR, "failed to truncate".into()));
        }
        self.file_size = size;
        Ok(())
    }

    fn flush(&mut self) -> VfsResult<()> {
        Ok(())
    }

    fn size(&self) -> VfsResult<usize> {
        Ok(self.file_size)
    }
}

/// Linear storage in memory, used for temporary DB
#[derive(Default)]
pub struct MemLinearFile(Vec<u8>);

impl VfsFile for MemLinearFile {
    fn read(&self, buf: &mut [u8], offset: usize) -> VfsResult<i32> {
        let size = buf.len();
        let end = size + offset;
        if self.0.len() <= offset {
            buf.fill(0);
            return Ok(SQLITE_IOERR_SHORT_READ);
        }

        let read_end = end.min(self.0.len());
        let read_size = read_end - offset;
        buf[..read_size].copy_from_slice(&self.0[offset..read_end]);

        if read_size < size {
            buf[read_size..].fill(0);
            return Ok(SQLITE_IOERR_SHORT_READ);
        }
        Ok(SQLITE_OK)
    }

    fn write(&mut self, buf: &[u8], offset: usize) -> VfsResult<()> {
        let end = buf.len() + offset;
        if end > self.0.len() {
            self.0.resize(end, 0);
        }
        self.0[offset..end].copy_from_slice(buf);
        Ok(())
    }

    fn truncate(&mut self, size: usize) -> VfsResult<()> {
        self.0.truncate(size);
        Ok(())
    }

    fn flush(&mut self) -> VfsResult<()> {
        Ok(())
    }

    fn size(&self) -> VfsResult<usize> {
        Ok(self.0.len())
    }
}

/// Used to log and retrieve Vfs errors
#[derive(Debug)]
pub struct VfsError {
    code: i32,
    message: String,
}

impl VfsError {
    pub fn new(code: i32, message: String) -> Self {
        VfsError { code, message }
    }
}

/// Wrapper for `Result`
pub type VfsResult<T> = Result<T, VfsError>;

/// Wrapper for `pAppData`
pub struct VfsAppData<T> {
    data: T,
    last_err: Mutex<Option<(i32, String)>>,
}

impl<T> VfsAppData<T> {
    pub fn new(t: T) -> Self {
        VfsAppData {
            data: t,
            last_err: Mutex::new(None),
        }
    }

    /// Leak, then pAppData can be set to VFS
    pub fn leak(self) -> *mut Self {
        Box::into_raw(Box::new(self))
    }

    /// # Safety
    ///
    /// You have to make sure the pointer is correct
    pub unsafe fn from_raw(t: *mut Self) -> VfsAppData<T> {
        *Box::from_raw(t)
    }

    /// Pop vfs last errcode and errmsg
    pub fn pop_err(&self) -> Option<(i32, String)> {
        self.last_err.lock().take()
    }

    /// Store errcode and errmsg
    pub fn store_err(&self, err: VfsError) -> i32 {
        let VfsError { code, message } = err;
        self.last_err.lock().replace((code, message));
        code
    }
}

/// Deref only, returns immutable reference, avoids race conditions
impl<T> Deref for VfsAppData<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

/// Some basic capabilities of file
pub trait VfsFile {
    /// Abstraction of `xRead`, returns `SQLITE_OK` or `SQLITE_IOERR_SHORT_READ`
    fn read(&self, buf: &mut [u8], offset: usize) -> VfsResult<i32>;
    /// Abstraction of `xWrite`
    fn write(&mut self, buf: &[u8], offset: usize) -> VfsResult<()>;
    /// Abstraction of `xTruncate`
    fn truncate(&mut self, size: usize) -> VfsResult<()>;
    /// Abstraction of `xSync`
    fn flush(&mut self) -> VfsResult<()>;
    /// Abstraction of `xFileSize`
    fn size(&self) -> VfsResult<usize>;
}

/// Make changes to files
pub trait VfsStore<File, AppData> {
    /// Convert pAppData to the type we need
    ///
    /// # Safety
    ///
    /// As long as it is set through the abstract VFS interface, it is safe
    unsafe fn app_data(vfs: *mut sqlite3_vfs) -> &'static VfsAppData<AppData> {
        &*(*vfs).pAppData.cast()
    }
    /// Get file path, use for `xOpen`
    fn name2path(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<String> {
        unused!(vfs);
        Ok(file.into())
    }
    /// Adding files to the Store, use for `xOpen` and `xAccess`
    fn add_file(vfs: *mut sqlite3_vfs, file: &str, flags: i32) -> VfsResult<()>;
    /// Checks if the specified file exists in the Store, use for `xOpen` and `xAccess`
    fn contains_file(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<bool>;
    /// Delete the specified file in the Store, use for `xClose` and `xDelete`
    fn delete_file(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<()>;
    /// Read the file contents, use for `xRead`, `xFileSize`
    fn with_file<F: Fn(&File) -> i32>(vfs_file: &SQLiteVfsFile, f: F) -> VfsResult<i32>;
    /// Write the file contents, use for `xWrite`, `xTruncate` and `xSync`
    fn with_file_mut<F: Fn(&mut File) -> i32>(vfs_file: &SQLiteVfsFile, f: F) -> VfsResult<i32>;
}

/// Abstraction of SQLite vfs
#[allow(clippy::missing_safety_doc)]
pub trait SQLiteVfs<IO: SQLiteIoMethods> {
    const VERSION: ::std::os::raw::c_int;
    const MAX_PATH_SIZE: ::std::os::raw::c_int = 1024;

    fn vfs(
        vfs_name: *const ::std::os::raw::c_char,
        app_data: *mut VfsAppData<IO::AppData>,
    ) -> sqlite3_vfs {
        sqlite3_vfs {
            iVersion: Self::VERSION,
            szOsFile: std::mem::size_of::<SQLiteVfsFile>() as i32,
            mxPathname: Self::MAX_PATH_SIZE,
            pNext: std::ptr::null_mut(),
            zName: vfs_name,
            pAppData: app_data.cast(),
            xOpen: Some(Self::xOpen),
            xDelete: Some(Self::xDelete),
            xAccess: Some(Self::xAccess),
            xFullPathname: Some(Self::xFullPathname),
            xDlOpen: None,
            xDlError: None,
            xDlSym: None,
            xDlClose: None,
            xRandomness: Some(x_methods_shim::xRandomness),
            xSleep: Some(x_methods_shim::xSleep),
            xCurrentTime: Some(x_methods_shim::xCurrentTime),
            xGetLastError: Some(Self::xGetLastError),
            xCurrentTimeInt64: Some(x_methods_shim::xCurrentTimeInt64),
            xSetSystemCall: None,
            xGetSystemCall: None,
            xNextSystemCall: None,
        }
    }

    unsafe extern "C" fn xOpen(
        pVfs: *mut sqlite3_vfs,
        zName: sqlite3_filename,
        pFile: *mut sqlite3_file,
        flags: ::std::os::raw::c_int,
        pOutFlags: *mut ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        let app_data = IO::Store::app_data(pVfs);

        let name = if zName.is_null() {
            get_random_name()
        } else {
            check_result!(CStr::from_ptr(zName).to_str()).into()
        };

        let name = match IO::Store::name2path(pVfs, &name) {
            Ok(name) => name,
            Err(err) => return app_data.store_err(err),
        };

        let exist = match IO::Store::contains_file(pVfs, &name) {
            Ok(exist) => exist,
            Err(err) => return app_data.store_err(err),
        };

        if !exist {
            if flags & SQLITE_OPEN_CREATE == 0 {
                return app_data.store_err(VfsError::new(
                    SQLITE_CANTOPEN,
                    format!("file not found: {name}"),
                ));
            }
            if let Err(err) = IO::Store::add_file(pVfs, &name, flags) {
                return app_data.store_err(err);
            }
        }

        let leak = name.leak();
        let vfs_file = pFile.cast::<SQLiteVfsFile>();
        (*vfs_file).vfs = pVfs;
        (*vfs_file).flags = flags;
        (*vfs_file).name_ptr = leak.as_ptr();
        (*vfs_file).name_length = leak.len();

        (*pFile).pMethods = &IO::METHODS;

        if !pOutFlags.is_null() {
            *pOutFlags = flags;
        }

        SQLITE_OK
    }

    unsafe extern "C" fn xDelete(
        pVfs: *mut sqlite3_vfs,
        zName: *const ::std::os::raw::c_char,
        syncDir: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        unused!(syncDir);

        let app_data = IO::Store::app_data(pVfs);
        bail!(zName.is_null(), SQLITE_IOERR_DELETE);
        let s = check_result!(CStr::from_ptr(zName).to_str());
        if let Err(err) = IO::Store::delete_file(pVfs, s) {
            app_data.store_err(err)
        } else {
            SQLITE_OK
        }
    }

    unsafe extern "C" fn xAccess(
        pVfs: *mut sqlite3_vfs,
        zName: *const ::std::os::raw::c_char,
        flags: ::std::os::raw::c_int,
        pResOut: *mut ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        unused!(flags);

        *pResOut = if zName.is_null() {
            0
        } else {
            let app_data = IO::Store::app_data(pVfs);
            let file = check_result!(CStr::from_ptr(zName).to_str());
            let exist = match IO::Store::contains_file(pVfs, file) {
                Ok(exist) => exist,
                Err(err) => return app_data.store_err(err),
            };
            i32::from(exist)
        };

        SQLITE_OK
    }

    unsafe extern "C" fn xFullPathname(
        pVfs: *mut sqlite3_vfs,
        zName: *const ::std::os::raw::c_char,
        nOut: ::std::os::raw::c_int,
        zOut: *mut ::std::os::raw::c_char,
    ) -> ::std::os::raw::c_int {
        unused!(pVfs);
        bail!(zName.is_null() || zOut.is_null(), SQLITE_CANTOPEN);
        let len = CStr::from_ptr(zName).to_bytes_with_nul().len();
        bail!(len > nOut as usize, SQLITE_CANTOPEN);
        zName.copy_to(zOut, len);
        SQLITE_OK
    }

    unsafe extern "C" fn xGetLastError(
        pVfs: *mut sqlite3_vfs,
        nOut: ::std::os::raw::c_int,
        zOut: *mut ::std::os::raw::c_char,
    ) -> ::std::os::raw::c_int {
        let app_data = IO::Store::app_data(pVfs);
        let Some((code, msg)) = app_data.pop_err() else {
            return SQLITE_OK;
        };
        if !zOut.is_null() {
            let nOut = nOut as usize;
            let count = msg.len().min(nOut);
            msg.as_ptr().copy_to(zOut.cast(), count);
            let zero = match nOut.cmp(&msg.len()) {
                std::cmp::Ordering::Less | std::cmp::Ordering::Equal => nOut,
                std::cmp::Ordering::Greater => msg.len() + 1,
            };
            if zero > 0 {
                std::ptr::write(zOut.add(zero - 1), 0);
            }
        }
        code
    }
}

/// Abstraction of SQLite vfs's io methods
#[allow(clippy::missing_safety_doc)]
pub trait SQLiteIoMethods {
    type File: VfsFile;
    type AppData: 'static;
    type Store: VfsStore<Self::File, Self::AppData>;

    const VERSION: ::std::os::raw::c_int;

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
        xFetch: None,
        xUnfetch: None,
    };

    unsafe extern "C" fn xClose(pFile: *mut sqlite3_file) -> ::std::os::raw::c_int {
        let vfs_file = SQLiteVfsFile::from_file(pFile);
        let app_data = Self::Store::app_data(vfs_file.vfs);

        if vfs_file.flags & SQLITE_OPEN_DELETEONCLOSE != 0 {
            if let Err(err) = Self::Store::delete_file(vfs_file.vfs, vfs_file.name()) {
                return app_data.store_err(err);
            }
        }

        drop(Box::from_raw(vfs_file.name()));

        SQLITE_OK
    }

    unsafe extern "C" fn xRead(
        pFile: *mut sqlite3_file,
        zBuf: *mut ::std::os::raw::c_void,
        iAmt: ::std::os::raw::c_int,
        iOfst: sqlite3_int64,
    ) -> ::std::os::raw::c_int {
        let vfs_file = SQLiteVfsFile::from_file(pFile);
        let app_data = Self::Store::app_data(vfs_file.vfs);

        let f = |file: &Self::File| {
            let size = iAmt as usize;
            let offset = iOfst as usize;
            let slice = std::slice::from_raw_parts_mut(zBuf.cast::<u8>(), size);
            match file.read(slice, offset) {
                Ok(code) => code,
                Err(err) => app_data.store_err(err),
            }
        };

        match Self::Store::with_file(vfs_file, f) {
            Ok(code) => code,
            Err(err) => app_data.store_err(err),
        }
    }

    unsafe extern "C" fn xWrite(
        pFile: *mut sqlite3_file,
        zBuf: *const ::std::os::raw::c_void,
        iAmt: ::std::os::raw::c_int,
        iOfst: sqlite3_int64,
    ) -> ::std::os::raw::c_int {
        let vfs_file = SQLiteVfsFile::from_file(pFile);
        let app_data = Self::Store::app_data(vfs_file.vfs);

        let f = |file: &mut Self::File| {
            let (offset, size) = (iOfst as usize, iAmt as usize);
            let slice = std::slice::from_raw_parts(zBuf.cast::<u8>(), size);
            if let Err(err) = file.write(slice, offset) {
                app_data.store_err(err)
            } else {
                SQLITE_OK
            }
        };

        match Self::Store::with_file_mut(vfs_file, f) {
            Ok(code) => code,
            Err(err) => app_data.store_err(err),
        }
    }

    unsafe extern "C" fn xTruncate(
        pFile: *mut sqlite3_file,
        size: sqlite3_int64,
    ) -> ::std::os::raw::c_int {
        let vfs_file = SQLiteVfsFile::from_file(pFile);
        let app_data = Self::Store::app_data(vfs_file.vfs);

        let f = |file: &mut Self::File| {
            if let Err(err) = file.truncate(size as usize) {
                app_data.store_err(err)
            } else {
                SQLITE_OK
            }
        };

        match Self::Store::with_file_mut(vfs_file, f) {
            Ok(code) => code,
            Err(err) => app_data.store_err(err),
        }
    }

    unsafe extern "C" fn xSync(
        pFile: *mut sqlite3_file,
        flags: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        unused!(flags);

        let vfs_file = SQLiteVfsFile::from_file(pFile);
        let app_data = Self::Store::app_data(vfs_file.vfs);

        let f = |file: &mut Self::File| {
            if let Err(err) = file.flush() {
                app_data.store_err(err)
            } else {
                SQLITE_OK
            }
        };

        match Self::Store::with_file_mut(vfs_file, f) {
            Ok(code) => code,
            Err(err) => app_data.store_err(err),
        }
    }

    unsafe extern "C" fn xFileSize(
        pFile: *mut sqlite3_file,
        pSize: *mut sqlite3_int64,
    ) -> ::std::os::raw::c_int {
        let vfs_file = SQLiteVfsFile::from_file(pFile);
        let app_data = Self::Store::app_data(vfs_file.vfs);

        let f = |file: &Self::File| match file.size() {
            Ok(size) => {
                *pSize = size as sqlite3_int64;
                SQLITE_OK
            }
            Err(err) => app_data.store_err(err),
        };

        match Self::Store::with_file(vfs_file, f) {
            Ok(code) => code,
            Err(err) => app_data.store_err(err),
        }
    }

    unsafe extern "C" fn xLock(
        pFile: *mut sqlite3_file,
        eLock: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        unused!((pFile, eLock));
        SQLITE_OK
    }

    unsafe extern "C" fn xUnlock(
        pFile: *mut sqlite3_file,
        eLock: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        unused!((pFile, eLock));
        SQLITE_OK
    }

    unsafe extern "C" fn xCheckReservedLock(
        pFile: *mut sqlite3_file,
        pResOut: *mut ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        unused!(pFile);
        *pResOut = 0;
        SQLITE_OK
    }

    unsafe extern "C" fn xFileControl(
        pFile: *mut sqlite3_file,
        op: ::std::os::raw::c_int,
        pArg: *mut ::std::os::raw::c_void,
    ) -> ::std::os::raw::c_int {
        unused!((pFile, op, pArg));
        SQLITE_NOTFOUND
    }

    unsafe extern "C" fn xSectorSize(pFile: *mut sqlite3_file) -> ::std::os::raw::c_int {
        unused!(pFile);
        512
    }

    unsafe extern "C" fn xDeviceCharacteristics(pFile: *mut sqlite3_file) -> ::std::os::raw::c_int {
        unused!(pFile);
        0
    }
}

/// Some x methods simulated using JS
#[allow(clippy::missing_safety_doc)]
pub mod x_methods_shim {
    use super::*;

    /// thread::sleep is available when atomics is enabled
    #[cfg(target_feature = "atomics")]
    pub unsafe extern "C" fn xSleep(
        _pVfs: *mut sqlite3_vfs,
        microseconds: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        use std::{thread, time::Duration};
        thread::sleep(Duration::from_micros(microseconds as u64));
        SQLITE_OK
    }

    #[cfg(not(target_feature = "atomics"))]
    pub unsafe extern "C" fn xSleep(
        _pVfs: *mut sqlite3_vfs,
        _microseconds: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        SQLITE_OK
    }

    /// https://github.com/sqlite/sqlite/blob/fb9e8e48fd70b463fb7ba6d99e00f2be54df749e/ext/wasm/api/sqlite3-vfs-opfs.c-pp.js#L951
    pub unsafe extern "C" fn xRandomness(
        _pVfs: *mut sqlite3_vfs,
        nByte: ::std::os::raw::c_int,
        zOut: *mut ::std::os::raw::c_char,
    ) -> ::std::os::raw::c_int {
        for i in 0..nByte as usize {
            *zOut.add(i) = (Math::random() * 255000.0) as _;
        }
        nByte
    }

    /// https://github.com/sqlite/sqlite/blob/fb9e8e48fd70b463fb7ba6d99e00f2be54df749e/ext/wasm/api/sqlite3-vfs-opfs.c-pp.js#L870
    pub unsafe extern "C" fn xCurrentTime(
        _pVfs: *mut sqlite3_vfs,
        pTimeOut: *mut f64,
    ) -> ::std::os::raw::c_int {
        *pTimeOut = 2440587.5 + (Date::new_0().get_time() / 86400000.0);
        SQLITE_OK
    }

    /// https://github.com/sqlite/sqlite/blob/fb9e8e48fd70b463fb7ba6d99e00f2be54df749e/ext/wasm/api/sqlite3-vfs-opfs.c-pp.js#L877
    pub unsafe extern "C" fn xCurrentTimeInt64(
        _pVfs: *mut sqlite3_vfs,
        pOut: *mut sqlite3_int64,
    ) -> ::std::os::raw::c_int {
        *pOut = ((2440587.5 * 86400000.0) + Date::new_0().get_time()) as sqlite3_int64;
        SQLITE_OK
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ImportDbError {
    #[error("Byte array size is invalid for an SQLite db.")]
    InvalidDbSize,
    #[error("Input does not contain an SQLite database header.")]
    InvalidHeader,
    #[error("Page size must be a power of two between 512 and 65536 inclusive")]
    InvalidPageSize,
}

/// Simple verification when importing db, and return page size;
pub fn check_import_db(bytes: &[u8]) -> Result<usize, ImportDbError> {
    let length = bytes.len();

    if length < 512 || length % 512 != 0 {
        return Err(ImportDbError::InvalidDbSize);
    }

    if SQLITE3_HEADER
        .as_bytes()
        .iter()
        .zip(bytes)
        .any(|(x, y)| x != y)
    {
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

    Ok(page_size)
}

/// Check db and page size, page size must be a power of two between 512 and 65536 inclusive, db size must be a multiple of page size.
pub fn check_db_and_page_size(db_size: usize, page_size: usize) -> Result<(), ImportDbError> {
    if !(page_size.is_power_of_two() && (512..=65536).contains(&page_size)) {
        return Err(ImportDbError::InvalidPageSize);
    }
    if db_size % page_size != 0 {
        return Err(ImportDbError::InvalidDbSize);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        vfs::utils::{copy_to_slice, copy_to_uint8_array_subarray},
        SQLITE_IOERR_SHORT_READ,
    };

    use super::{copy_to_uint8_array, copy_to_vec, MemChunksFile, VfsFile};
    use js_sys::Uint8Array;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn test_js_utils() {
        let buf1 = vec![1, 2, 3, 4];
        let uint8 = copy_to_uint8_array(&buf1);
        let buf2 = copy_to_vec(&uint8);
        assert_eq!(buf1, buf2);

        let mut buf3 = vec![0u8; 2];
        copy_to_slice(&uint8.subarray(0, 2), &mut buf3);
        assert_eq!(buf3, vec![1, 2]);

        let buf4 = Uint8Array::new_with_length(3);
        copy_to_uint8_array_subarray(&buf3, &buf4.subarray(1, 3));
        assert!(buf4.get_index(0) == 0);
        assert!(buf4.get_index(1) == 1);
        assert!(buf4.get_index(2) == 2);
    }

    #[wasm_bindgen_test]
    fn test_chunks_file() {
        let mut file = MemChunksFile::new(512);
        file.write(&[], 0).unwrap();
        assert!(file.size().unwrap() == 0);

        let mut buffer = [1; 2];
        let ret = file.read(&mut buffer, 0).unwrap();
        assert_eq!(ret, SQLITE_IOERR_SHORT_READ);
        assert_eq!([0; 2], buffer);

        file.write(&[1], 0).unwrap();
        assert!(file.size().unwrap() == 1);
        let mut buffer = [2; 2];
        let ret = file.read(&mut buffer, 0).unwrap();
        assert_eq!(ret, SQLITE_IOERR_SHORT_READ);
        assert_eq!([1, 0], buffer);

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
        assert_eq!(ret, 0);
        assert_eq!(buffer, [41, 42, 43]);

        file.truncate(513).unwrap();
        assert!(file.size().unwrap() == 513);
        assert!(file.chunks.len() == 2);

        file.write(&[1], 2048).unwrap();
        assert!(file.size().unwrap() == 2049);
        assert!(file.chunks.len() == 5);

        file.truncate(0).unwrap();
        assert!(file.size().unwrap() == 0);
        assert!(file.chunks.len() == 0);
    }
}
