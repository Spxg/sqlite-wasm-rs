//! Some tools for implementing VFS

use crate::libsqlite3::*;

use fragile::Fragile;
use js_sys::{Date, Math, Number, Uint8Array, WebAssembly};
use std::{
    collections::HashMap,
    ffi::{CStr, CString},
    ops::{Deref, DerefMut},
};
use wasm_bindgen::{prelude::wasm_bindgen, JsCast};

/// The header of the SQLite file is used to determine whether the imported file is legal.
pub const SQLITE3_HEADER: &str = "SQLite format 3";

/// Wrap the pVfs pointer, which is often used in VFS implementation.
///
/// Use vfs pointer as the map key to find the corresponding vfs handle, such as `OpfsSAHPool`.
#[derive(Hash, PartialEq, Eq)]
pub struct VfsPtr(pub *mut sqlite3_vfs);

unsafe impl Send for VfsPtr {}
unsafe impl Sync for VfsPtr {}

/// Wrap the pFile pointer, which is often used in VFS implementation.
///
/// Use file pointer as the map key to find the corresponding file handle, such as `MemFile`.
#[derive(Hash, PartialEq, Eq)]
pub struct FilePtr(pub *mut sqlite3_file);

unsafe impl Send for FilePtr {}
unsafe impl Sync for FilePtr {}

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

/// Directly using copy_from and copy_to to convert Uint8Array and Vec<u8> is risky.
/// There is a possibility that the memory will grow and the buffer will be detached during copy.
/// So here we convert on the js side.
///
/// Related issues:
///
/// <https://github.com/rustwasm/wasm-bindgen/issues/4395>
///
/// <https://github.com/rustwasm/wasm-bindgen/issues/4392>
#[wasm_bindgen(module = "/src/vfs/utils.js")]
extern "C" {
    type JSUtils;

    #[wasm_bindgen(static_method_of = JSUtils, js_name = toSlice)]
    fn to_slice(memory: &WebAssembly::Memory, buffer: &Uint8Array, dst: *mut u8, len: usize);

    #[wasm_bindgen(static_method_of = JSUtils, js_name = toUint8Array)]
    fn to_uint8_array(memory: &WebAssembly::Memory, src: *const u8, len: usize, dst: &Uint8Array);
}

/// Copy `Uint8Array` and return new `Vec<u8>`
pub fn copy_to_vec(src: &Uint8Array) -> Vec<u8> {
    let mut vec = vec![0u8; src.length() as usize];
    copy_to_slice(src, vec.as_mut_slice());
    vec
}

/// Copy `Uint8Array` to `slice`
pub fn copy_to_slice(src: &Uint8Array, dst: &mut [u8]) {
    assert!(
        src.length() as usize == dst.len(),
        "Unit8Array and slice have different sizes"
    );

    let buf = wasm_bindgen::memory();
    let mem = buf.unchecked_ref::<WebAssembly::Memory>();
    JSUtils::to_slice(mem, src, dst.as_mut_ptr(), dst.len());
}

/// Copy `slice` and return new `Uint8Array`
pub fn copy_to_uint8_array(src: &[u8]) -> Uint8Array {
    let uint8 = Uint8Array::new_with_length(src.len() as u32);
    copy_to_uint8_array_subarray(src, &uint8);
    uint8
}

/// Copy `slice` to `Unit8Array`
pub fn copy_to_uint8_array_subarray(src: &[u8], dst: &Uint8Array) {
    assert!(
        src.len() == dst.length() as _,
        "Unit8Array and slice have different sizes"
    );
    let buf = wasm_bindgen::memory();
    let mem = buf.unchecked_ref::<WebAssembly::Memory>();
    JSUtils::to_uint8_array(mem, src.as_ptr(), src.len(), dst)
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

    /// Get the file name. When xClose, you can release the memory by `drop(Box::from_raw(ptr));`.
    ///
    /// # Safety
    ///
    /// You must ensure that the pointer passed in is `SQLiteVfsFile`
    pub unsafe fn name(&self) -> &'static mut str {
        // emm, `from_raw_parts_mut` is unstable
        std::str::from_utf8_unchecked_mut(std::slice::from_raw_parts_mut(
            self.name_ptr.cast_mut(),
            self.name_length,
        ))
    }
}

/// Possible errors when registering Vfs
#[derive(thiserror::Error, Debug)]
pub enum VfsError {
    #[error("An error occurred converting the given vfs name to a CStr")]
    ToCStr,
    #[error("An error occurred while registering vfs with sqlite")]
    RegisterVfs,
}

/// FIXME: delete later
pub fn register_vfs_legacy(
    vfs_name: &str,
    default_vfs: bool,
    register_vfs: fn(*const ::std::os::raw::c_char) -> sqlite3_vfs,
) -> Result<*mut sqlite3_vfs, VfsError> {
    let name = CString::new(vfs_name).map_err(|_| VfsError::ToCStr)?;
    let name_ptr = name.into_raw();
    let vfs = Box::leak(Box::new(register_vfs(name_ptr)));
    let ret = unsafe { sqlite3_vfs_register(vfs, i32::from(default_vfs)) };

    if ret != SQLITE_OK {
        unsafe {
            drop(Box::from_raw(vfs));
            drop(CString::from_raw(name_ptr));
        }
        return Err(VfsError::RegisterVfs);
    }

    Ok(vfs as *mut sqlite3_vfs)
}

/// Register vfs general method
pub fn register_vfs<T, IO: SQLiteIoMethods, V: SQLiteVfs<IO>>(
    vfs_name: &str,
    app_data: IO::AppData,
    default_vfs: bool,
) -> Result<*mut sqlite3_vfs, VfsError> {
    let name = CString::new(vfs_name).map_err(|_| VfsError::ToCStr)?;
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
        return Err(VfsError::RegisterVfs);
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

/// Some basic capabilities of Store
pub trait VfsStore {
    /// Abstraction of `xRead`, returns `SQLITE_OK` or `SQLITE_IOERR_SHORT_READ`
    fn read(&self, buf: &mut [u8], offset: usize) -> i32;
    /// Abstraction of `xWrite`
    fn write(&mut self, buf: &[u8], offset: usize);
    /// Abstraction of `xTruncate`
    fn truncate(&mut self, size: usize);
    /// Get file size
    fn size(&self) -> usize;
}

/// Linear storage in memory, used for temporary DB
#[derive(Default)]
pub struct MemLinearStore(Vec<u8>);

impl VfsStore for MemLinearStore {
    fn read(&self, buf: &mut [u8], offset: usize) -> i32 {
        let size = buf.len();
        let end = size + offset;
        if self.0.len() <= offset {
            buf.fill(0);
            return SQLITE_IOERR_SHORT_READ;
        }

        let read_end = end.min(self.0.len());
        let read_size = read_end - offset;
        buf[..read_size].copy_from_slice(&self.0[offset..read_end]);

        if read_size < size {
            buf[read_size..].fill(0);
            return SQLITE_IOERR_SHORT_READ;
        }
        SQLITE_OK
    }

    fn write(&mut self, buf: &[u8], offset: usize) {
        let end = buf.len() + offset;
        if end > self.0.len() {
            self.0.resize(end, 0);
        }
        self.0[offset..end].copy_from_slice(buf);
    }

    fn truncate(&mut self, size: usize) {
        self.0.truncate(size);
    }

    fn size(&self) -> usize {
        self.0.len()
    }
}

/// Memory storage structure by page (block)
///
/// Used by memory vfs and relaxed-idb vfs
#[derive(Default)]
pub struct MemPageStore {
    pages: HashMap<usize, Vec<u8>>,
    file_size: usize,
    page_size: usize,
}

impl VfsStore for MemPageStore {
    fn read(&self, buf: &mut [u8], offset: usize) -> i32 {
        page_read(
            buf,
            self.page_size,
            self.file_size,
            offset,
            |addr| self.pages.get(&addr),
            |page, buf, (start, end)| {
                buf.copy_from_slice(&page[start..end]);
            },
        )
    }

    fn write(&mut self, buf: &[u8], offset: usize) {
        let page_size = buf.len();

        for fill in (self.file_size..offset).step_by(page_size) {
            self.pages.insert(fill, vec![0; page_size]);
        }
        if let Some(buffer) = self.pages.get_mut(&offset) {
            buffer.copy_from_slice(buf);
        } else {
            self.pages.insert(offset, buf.to_vec());
        }

        self.page_size = page_size;
        self.file_size = self.file_size.max(offset + page_size);
    }

    fn truncate(&mut self, size: usize) {
        for offset in size..self.file_size {
            self.pages.remove(&offset);
        }
        self.file_size = size;
    }

    fn size(&self) -> usize {
        self.file_size
    }
}

/// Wrapper for `pAppData`
pub struct VfsAppData<T>(T);

impl<T> VfsAppData<T> {
    pub fn new(t: T) -> Self {
        VfsAppData(t)
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
}

/// Deref only, returns immutable reference, avoids race conditions
impl<T> Deref for VfsAppData<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Control the Store and make changes to files
pub trait StoreControl<S, A> {
    /// Convert pAppData to the type we need
    ///
    /// # Safety
    ///
    /// As long as it is set through the abstract VFS interface, it is safe
    unsafe fn app_data(vfs: *mut sqlite3_vfs) -> &'static VfsAppData<A> {
        &*(*vfs).pAppData.cast()
    }
    /// Adding files to the Store, use for `xOpen`
    fn add_file(vfs: *mut sqlite3_vfs, file: &str, flags: i32);
    /// Checks if the specified file exists in the Store, use for `xOpen`
    fn contains_file(vfs: *mut sqlite3_vfs, file: &str) -> bool;
    /// Delete the specified file in the Store, use for `xClose` and `xDelete`
    fn delete_file(vfs: *mut sqlite3_vfs, file: &str) -> Option<S>;
    /// Read the file contents, use for `xRead`, `xFileSize`
    fn with_file<F: Fn(&S) -> i32>(vfs_file: &SQLiteVfsFile, f: F) -> Option<i32>;
    /// Write the file contents, use for `xWrite`, `xTruncate`
    fn with_file_mut<F: Fn(&mut S) -> i32>(vfs_file: &SQLiteVfsFile, f: F) -> Option<i32>;
}

/// Abstraction of SQLite vfs
#[allow(clippy::missing_safety_doc)]
pub trait SQLiteVfs<IO: SQLiteIoMethods> {
    const VERSION: ::std::os::raw::c_int;

    fn vfs(
        vfs_name: *const ::std::os::raw::c_char,
        app_data: *mut VfsAppData<IO::AppData>,
    ) -> sqlite3_vfs {
        sqlite3_vfs {
            iVersion: Self::VERSION,
            szOsFile: std::mem::size_of::<SQLiteVfsFile>() as i32,
            mxPathname: 1024,
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
        let name = if zName.is_null() {
            get_random_name()
        } else {
            check_result!(CStr::from_ptr(zName).to_str()).into()
        };

        if !IO::StoreControl::contains_file(pVfs, &name) {
            if flags & SQLITE_OPEN_CREATE == 0 {
                return SQLITE_CANTOPEN;
            }
            IO::StoreControl::add_file(pVfs, &name, flags);
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
        _syncDir: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        bail!(zName.is_null(), SQLITE_IOERR_DELETE);
        let s = check_result!(CStr::from_ptr(zName).to_str());
        IO::StoreControl::delete_file(pVfs, s);
        SQLITE_OK
    }

    unsafe extern "C" fn xAccess(
        pVfs: *mut sqlite3_vfs,
        zName: *const ::std::os::raw::c_char,
        _flags: ::std::os::raw::c_int,
        pResOut: *mut ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        *pResOut = if zName.is_null() {
            0
        } else {
            let s = check_result!(CStr::from_ptr(zName).to_str());
            i32::from(IO::StoreControl::contains_file(pVfs, s))
        };

        SQLITE_OK
    }

    unsafe extern "C" fn xFullPathname(
        _pVfs: *mut sqlite3_vfs,
        zName: *const ::std::os::raw::c_char,
        nOut: ::std::os::raw::c_int,
        zOut: *mut ::std::os::raw::c_char,
    ) -> ::std::os::raw::c_int {
        bail!(zName.is_null() || zOut.is_null(), SQLITE_CANTOPEN);
        let len = CStr::from_ptr(zName).count_bytes() + 1;
        bail!(len > nOut as usize, SQLITE_CANTOPEN);
        zName.copy_to(zOut, len);
        SQLITE_OK
    }

    unsafe extern "C" fn xGetLastError(
        _pVfs: *mut sqlite3_vfs,
        _nOut: ::std::os::raw::c_int,
        _zOut: *mut ::std::os::raw::c_char,
    ) -> ::std::os::raw::c_int {
        SQLITE_OK
    }
}

/// Abstraction of SQLite vfs's io methods
#[allow(clippy::missing_safety_doc)]
pub trait SQLiteIoMethods {
    type Store: VfsStore;
    type AppData;
    type StoreControl: StoreControl<Self::Store, Self::AppData>;

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
        if vfs_file.flags & SQLITE_OPEN_DELETEONCLOSE != 0 {
            check_option!(Self::StoreControl::delete_file(
                vfs_file.vfs,
                vfs_file.name()
            ));
        }
        drop(unsafe { Box::from_raw(vfs_file.name()) });
        SQLITE_OK
    }

    unsafe extern "C" fn xRead(
        pFile: *mut sqlite3_file,
        zBuf: *mut ::std::os::raw::c_void,
        iAmt: ::std::os::raw::c_int,
        iOfst: sqlite3_int64,
    ) -> ::std::os::raw::c_int {
        let vfs_file = SQLiteVfsFile::from_file(pFile);
        let f = |store: &Self::Store| {
            let size = iAmt as usize;
            let offset = iOfst as usize;
            let slice = std::slice::from_raw_parts_mut(zBuf.cast::<u8>(), size);
            store.read(slice, offset)
        };
        check_option!(Self::StoreControl::with_file(vfs_file, f))
    }

    unsafe extern "C" fn xWrite(
        pFile: *mut sqlite3_file,
        zBuf: *const ::std::os::raw::c_void,
        iAmt: ::std::os::raw::c_int,
        iOfst: sqlite3_int64,
    ) -> ::std::os::raw::c_int {
        let vfs_file = SQLiteVfsFile::from_file(pFile);
        let f = |store: &mut Self::Store| {
            let (offset, size) = (iOfst as usize, iAmt as usize);
            let slice = std::slice::from_raw_parts(zBuf.cast::<u8>(), size);
            store.write(slice, offset);
            SQLITE_OK
        };
        check_option!(Self::StoreControl::with_file_mut(vfs_file, f))
    }

    unsafe extern "C" fn xTruncate(
        pFile: *mut sqlite3_file,
        size: sqlite3_int64,
    ) -> ::std::os::raw::c_int {
        let vfs_file = SQLiteVfsFile::from_file(pFile);
        let f = |store: &mut Self::Store| {
            store.truncate(size as usize);
            SQLITE_OK
        };
        check_option!(Self::StoreControl::with_file_mut(vfs_file, f))
    }

    unsafe extern "C" fn xSync(
        _pFile: *mut sqlite3_file,
        _flags: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        SQLITE_OK
    }

    unsafe extern "C" fn xFileSize(
        pFile: *mut sqlite3_file,
        pSize: *mut sqlite3_int64,
    ) -> ::std::os::raw::c_int {
        let vfs_file = SQLiteVfsFile::from_file(pFile);
        let f = |store: &Self::Store| {
            *pSize = store.size() as sqlite3_int64;
            SQLITE_OK
        };
        check_option!(Self::StoreControl::with_file(vfs_file, f))
    }

    unsafe extern "C" fn xLock(
        _pFile: *mut sqlite3_file,
        _eLock: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        SQLITE_OK
    }

    unsafe extern "C" fn xUnlock(
        _pFile: *mut sqlite3_file,
        _eLock: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        SQLITE_OK
    }

    unsafe extern "C" fn xCheckReservedLock(
        _pFile: *mut sqlite3_file,
        pResOut: *mut ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        *pResOut = 0;
        SQLITE_OK
    }

    unsafe extern "C" fn xFileControl(
        _pFile: *mut sqlite3_file,
        _op: ::std::os::raw::c_int,
        _pArg: *mut ::std::os::raw::c_void,
    ) -> ::std::os::raw::c_int {
        SQLITE_NOTFOUND
    }

    unsafe extern "C" fn xSectorSize(_pFile: *mut sqlite3_file) -> ::std::os::raw::c_int {
        512
    }

    unsafe extern "C" fn xDeviceCharacteristics(_arg1: *mut sqlite3_file) -> ::std::os::raw::c_int {
        0
    }
}

/// Some x methods simulated using JS
#[allow(clippy::missing_safety_doc)]
pub mod x_methods_shim {
    use super::*;

    /// thread::sleep is available when atomics are enabled
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
        for i in 0..nByte {
            *zOut.offset(i as isize) = (Math::random() * 255000.0) as _;
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

#[cfg(test)]
mod tests {
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    use crate::vfs::utils::{copy_to_slice, copy_to_uint8_array_subarray};

    use super::{copy_to_uint8_array, copy_to_vec};
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
}
