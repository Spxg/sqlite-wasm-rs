pub type sqlite3_int64 = ::core::ffi::c_longlong;
pub type sqlite3_filename = *const ::core::ffi::c_char;
pub type sqlite3_syscall_ptr = ::core::option::Option<unsafe extern "C" fn()>;

pub const SQLITE_CANTOPEN: i32 = 14;
pub const SQLITE_ERROR: i32 = 1;
pub const SQLITE_IOERR: i32 = 10;
pub const SQLITE_IOERR_SHORT_READ: i32 = 522;
pub const SQLITE_IOERR_DELETE: i32 = 2570;
pub const SQLITE_OK: i32 = 0;
pub const SQLITE_OPEN_DELETEONCLOSE: i32 = 8;
pub const SQLITE_OPEN_MAIN_DB: i32 = 256;
pub const SQLITE_OPEN_READWRITE: i32 = 2;
pub const SQLITE_OPEN_CREATE: i32 = 4;
pub const SQLITE_NOTFOUND: i32 = 12;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct sqlite3_file {
    pub pMethods: *const sqlite3_io_methods,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct sqlite3_io_methods {
    pub iVersion: ::core::ffi::c_int,
    pub xClose: ::core::option::Option<
        unsafe extern "C" fn(arg1: *mut sqlite3_file) -> ::core::ffi::c_int,
    >,
    pub xRead: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_file,
            arg2: *mut ::core::ffi::c_void,
            iAmt: ::core::ffi::c_int,
            iOfst: sqlite3_int64,
        ) -> ::core::ffi::c_int,
    >,
    pub xWrite: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_file,
            arg2: *const ::core::ffi::c_void,
            iAmt: ::core::ffi::c_int,
            iOfst: sqlite3_int64,
        ) -> ::core::ffi::c_int,
    >,
    pub xTruncate: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_file,
            size: sqlite3_int64,
        ) -> ::core::ffi::c_int,
    >,
    pub xSync: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_file,
            flags: ::core::ffi::c_int,
        ) -> ::core::ffi::c_int,
    >,
    pub xFileSize: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_file,
            pSize: *mut sqlite3_int64,
        ) -> ::core::ffi::c_int,
    >,
    pub xLock: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_file,
            arg2: ::core::ffi::c_int,
        ) -> ::core::ffi::c_int,
    >,
    pub xUnlock: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_file,
            arg2: ::core::ffi::c_int,
        ) -> ::core::ffi::c_int,
    >,
    pub xCheckReservedLock: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_file,
            pResOut: *mut ::core::ffi::c_int,
        ) -> ::core::ffi::c_int,
    >,
    pub xFileControl: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_file,
            op: ::core::ffi::c_int,
            pArg: *mut ::core::ffi::c_void,
        ) -> ::core::ffi::c_int,
    >,
    pub xSectorSize: ::core::option::Option<
        unsafe extern "C" fn(arg1: *mut sqlite3_file) -> ::core::ffi::c_int,
    >,
    pub xDeviceCharacteristics: ::core::option::Option<
        unsafe extern "C" fn(arg1: *mut sqlite3_file) -> ::core::ffi::c_int,
    >,
    pub xShmMap: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_file,
            iPg: ::core::ffi::c_int,
            pgsz: ::core::ffi::c_int,
            arg2: ::core::ffi::c_int,
            arg3: *mut *mut ::core::ffi::c_void,
        ) -> ::core::ffi::c_int,
    >,
    pub xShmLock: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_file,
            offset: ::core::ffi::c_int,
            n: ::core::ffi::c_int,
            flags: ::core::ffi::c_int,
        ) -> ::core::ffi::c_int,
    >,
    pub xShmBarrier: ::core::option::Option<
        unsafe extern "C" fn(arg1: *mut sqlite3_file),
    >,
    pub xShmUnmap: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_file,
            deleteFlag: ::core::ffi::c_int,
        ) -> ::core::ffi::c_int,
    >,
    pub xFetch: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_file,
            iOfst: sqlite3_int64,
            iAmt: ::core::ffi::c_int,
            pp: *mut *mut ::core::ffi::c_void,
        ) -> ::core::ffi::c_int,
    >,
    pub xUnfetch: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_file,
            iOfst: sqlite3_int64,
            p: *mut ::core::ffi::c_void,
        ) -> ::core::ffi::c_int,
    >,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct sqlite3_vfs {
    pub iVersion: ::core::ffi::c_int,
    pub szOsFile: ::core::ffi::c_int,
    pub mxPathname: ::core::ffi::c_int,
    pub pNext: *mut sqlite3_vfs,
    pub zName: *const ::core::ffi::c_char,
    pub pAppData: *mut ::core::ffi::c_void,
    pub xOpen: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_vfs,
            zName: sqlite3_filename,
            arg2: *mut sqlite3_file,
            flags: ::core::ffi::c_int,
            pOutFlags: *mut ::core::ffi::c_int,
        ) -> ::core::ffi::c_int,
    >,
    pub xDelete: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_vfs,
            zName: *const ::core::ffi::c_char,
            syncDir: ::core::ffi::c_int,
        ) -> ::core::ffi::c_int,
    >,
    pub xAccess: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_vfs,
            zName: *const ::core::ffi::c_char,
            flags: ::core::ffi::c_int,
            pResOut: *mut ::core::ffi::c_int,
        ) -> ::core::ffi::c_int,
    >,
    pub xFullPathname: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_vfs,
            zName: *const ::core::ffi::c_char,
            nOut: ::core::ffi::c_int,
            zOut: *mut ::core::ffi::c_char,
        ) -> ::core::ffi::c_int,
    >,
    pub xDlOpen: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_vfs,
            zFilename: *const ::core::ffi::c_char,
        ) -> *mut ::core::ffi::c_void,
    >,
    pub xDlError: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_vfs,
            nByte: ::core::ffi::c_int,
            zErrMsg: *mut ::core::ffi::c_char,
        ),
    >,
    pub xDlSym: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_vfs,
            arg2: *mut ::core::ffi::c_void,
            zSymbol: *const ::core::ffi::c_char,
        ) -> ::core::option::Option<
            unsafe extern "C" fn(
                arg1: *mut sqlite3_vfs,
                arg2: *mut ::core::ffi::c_void,
                zSymbol: *const ::core::ffi::c_char,
            ),
        >,
    >,
    pub xDlClose: ::core::option::Option<
        unsafe extern "C" fn(arg1: *mut sqlite3_vfs, arg2: *mut ::core::ffi::c_void),
    >,
    pub xRandomness: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_vfs,
            nByte: ::core::ffi::c_int,
            zOut: *mut ::core::ffi::c_char,
        ) -> ::core::ffi::c_int,
    >,
    pub xSleep: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_vfs,
            microseconds: ::core::ffi::c_int,
        ) -> ::core::ffi::c_int,
    >,
    pub xCurrentTime: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_vfs,
            arg2: *mut f64,
        ) -> ::core::ffi::c_int,
    >,
    pub xGetLastError: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_vfs,
            arg2: ::core::ffi::c_int,
            arg3: *mut ::core::ffi::c_char,
        ) -> ::core::ffi::c_int,
    >,
    pub xCurrentTimeInt64: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_vfs,
            arg2: *mut sqlite3_int64,
        ) -> ::core::ffi::c_int,
    >,
    pub xSetSystemCall: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_vfs,
            zName: *const ::core::ffi::c_char,
            arg2: sqlite3_syscall_ptr,
        ) -> ::core::ffi::c_int,
    >,
    pub xGetSystemCall: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_vfs,
            zName: *const ::core::ffi::c_char,
        ) -> sqlite3_syscall_ptr,
    >,
    pub xNextSystemCall: ::core::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_vfs,
            zName: *const ::core::ffi::c_char,
        ) -> *const ::core::ffi::c_char,
    >,
}

unsafe extern "C" {
    pub fn sqlite3_vfs_find(zVfsName: *const ::core::ffi::c_char) -> *mut sqlite3_vfs;
}

unsafe extern "C" {
    pub fn sqlite3_vfs_register(
        arg1: *mut sqlite3_vfs,
        makeDflt: ::core::ffi::c_int,
    ) -> ::core::ffi::c_int;
}

unsafe extern "C" {
    pub fn sqlite3_vfs_unregister(arg1: *mut sqlite3_vfs) -> ::core::ffi::c_int;
}
