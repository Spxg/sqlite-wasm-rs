//! Memory VFS, used as the default VFS

use crate::shim::libsqlite3::*;
use crate::shim::vfs::utils::get_random_name;
use js_sys::{Date, Math};
use once_cell::sync::Lazy;
use parking_lot::{Mutex, MutexGuard, RwLock};
use std::{collections::HashMap, ffi::CStr, sync::Arc};

/// thread::sleep is available when atomics are enabled
#[cfg(target_feature = "atomics")]
unsafe extern "C" fn xSleep(
    _pVfs: *mut sqlite3_vfs,
    microseconds: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    use std::{thread, time::Duration};

    thread::sleep(Duration::from_micros(microseconds as u64));
    SQLITE_OK
}

#[cfg(not(target_feature = "atomics"))]
unsafe extern "C" fn xSleep(
    _pVfs: *mut sqlite3_vfs,
    _microseconds: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    SQLITE_OK
}

/// https://github.com/sqlite/sqlite/blob/fb9e8e48fd70b463fb7ba6d99e00f2be54df749e/ext/wasm/api/sqlite3-vfs-opfs.c-pp.js#L951
unsafe extern "C" fn xRandomness(
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
unsafe extern "C" fn xCurrentTime(
    _pVfs: *mut sqlite3_vfs,
    pTimeOut: *mut f64,
) -> ::std::os::raw::c_int {
    *pTimeOut = 2440587.5 + (Date::new_0().get_time() / 86400000.0);
    SQLITE_OK
}

/// https://github.com/sqlite/sqlite/blob/fb9e8e48fd70b463fb7ba6d99e00f2be54df749e/ext/wasm/api/sqlite3-vfs-opfs.c-pp.js#L877
unsafe extern "C" fn xCurrentTimeInt64(
    _pVfs: *mut sqlite3_vfs,
    pOut: *mut sqlite3_int64,
) -> ::std::os::raw::c_int {
    *pOut = ((2440587.5 * 86400000.0) + Date::new_0().get_time()) as sqlite3_int64;
    SQLITE_OK
}

/// pFile -> mem_file
fn file2file() -> MutexGuard<'static, HashMap<usize, Arc<RwLock<MemFile>>>> {
    static PFILE: Lazy<Mutex<HashMap<usize, Arc<RwLock<MemFile>>>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));

    PFILE.lock()
}

/// filename -> mem_file
fn name2file() -> MutexGuard<'static, HashMap<String, Arc<RwLock<MemFile>>>> {
    static NAME: Lazy<Mutex<HashMap<String, Arc<RwLock<MemFile>>>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));

    NAME.lock()
}

/// An open file
struct MemFile {
    // filename
    name: String,
    /// flags
    flags: i32,
    /// content of the file
    data: Vec<u8>,
}

unsafe extern "C" fn xOpen(
    _pVfs: *mut sqlite3_vfs,
    zName: sqlite3_filename,
    pFile: *mut sqlite3_file,
    flags: ::std::os::raw::c_int,
    pOutFlags: *mut ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    let name = if zName.is_null() {
        get_random_name()
    } else {
        let Ok(s) = CStr::from_ptr(zName).to_str() else {
            return SQLITE_ERROR;
        };
        s.into()
    };

    let mut name2file = name2file();
    let mem_file = if let Some(mem_file) = name2file.get(&name) {
        Arc::clone(mem_file)
    } else {
        if flags & SQLITE_OPEN_CREATE == 0 {
            return SQLITE_CANTOPEN;
        }
        let file = Arc::new(RwLock::new(MemFile {
            name: name.clone(),
            flags,
            data: Vec::new(),
        }));
        name2file.insert(name, Arc::clone(&file));
        file
    };

    file2file().insert(pFile as usize, mem_file);

    (*pFile).pMethods = &IO_METHODS;

    if !pOutFlags.is_null() {
        *pOutFlags = flags;
    }

    SQLITE_OK
}

unsafe extern "C" fn xDelete(
    _pVfs: *mut sqlite3_vfs,
    zName: *const ::std::os::raw::c_char,
    _syncDir: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    if zName.is_null() {
        return SQLITE_IOERR_DELETE;
    }
    let Ok(s) = CStr::from_ptr(zName).to_str() else {
        return SQLITE_ERROR;
    };
    name2file().remove(s);
    SQLITE_OK
}

unsafe extern "C" fn xAccess(
    _pVfs: *mut sqlite3_vfs,
    zName: *const ::std::os::raw::c_char,
    _flags: ::std::os::raw::c_int,
    pResOut: *mut ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    *pResOut = if zName.is_null() {
        0
    } else {
        let Ok(s) = CStr::from_ptr(zName).to_str() else {
            return SQLITE_ERROR;
        };
        i32::from(name2file().contains_key(s))
    };
    SQLITE_OK
}

unsafe extern "C" fn xFullPathname(
    _pVfs: *mut sqlite3_vfs,
    zName: *const ::std::os::raw::c_char,
    nOut: ::std::os::raw::c_int,
    zOut: *mut ::std::os::raw::c_char,
) -> ::std::os::raw::c_int {
    if zName.is_null() || zOut.is_null() {
        return SQLITE_CANTOPEN;
    }
    let len = CStr::from_ptr(zName).count_bytes() + 1;

    if len > nOut as usize {
        return SQLITE_CANTOPEN;
    }
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

unsafe extern "C" fn xClose(pFile: *mut sqlite3_file) -> ::std::os::raw::c_int {
    if let Some(file) = file2file().remove(&(pFile as usize)) {
        let file = file.write();
        if file.flags & SQLITE_OPEN_DELETEONCLOSE != 0 {
            name2file().remove(&file.name);
        }
    }
    SQLITE_OK
}

unsafe extern "C" fn xRead(
    pFile: *mut sqlite3_file,
    zBuf: *mut ::std::os::raw::c_void,
    iAmt: ::std::os::raw::c_int,
    iOfst: sqlite3_int64,
) -> ::std::os::raw::c_int {
    let Some(file) = file2file().get(&(pFile as usize)).cloned() else {
        return SQLITE_ERROR;
    };
    let file = file.read();
    let data = &file.data;

    let end = iOfst as usize + iAmt as usize;
    let slice = std::slice::from_raw_parts_mut(zBuf.cast::<u8>(), iAmt as usize);

    if data.len() <= iOfst as usize {
        slice.fill(0);
        return SQLITE_IOERR_SHORT_READ;
    }

    let read_size = end.min(data.len()) - iOfst as usize;
    slice[..read_size].copy_from_slice(&data[iOfst as usize..end.min(data.len())]);

    if read_size < iAmt as usize {
        slice[read_size..iAmt as usize].fill(0);
        return SQLITE_IOERR_SHORT_READ;
    }

    SQLITE_OK
}

unsafe extern "C" fn xWrite(
    pFile: *mut sqlite3_file,
    zBuf: *const ::std::os::raw::c_void,
    iAmt: ::std::os::raw::c_int,
    iOfst: sqlite3_int64,
) -> ::std::os::raw::c_int {
    let Some(file) = file2file().get(&(pFile as usize)).cloned() else {
        return SQLITE_ERROR;
    };
    let end = iOfst as usize + iAmt as usize;
    let mut file = file.write();
    let data = &mut file.data;

    if end > data.len() {
        data.resize(end, 0);
    }
    let slice = std::slice::from_raw_parts(zBuf.cast::<u8>(), iAmt as usize);

    data[iOfst as usize..end].copy_from_slice(slice);

    SQLITE_OK
}

unsafe extern "C" fn xTruncate(
    pFile: *mut sqlite3_file,
    size: sqlite3_int64,
) -> ::std::os::raw::c_int {
    let Some(file) = file2file().get(&(pFile as usize)).cloned() else {
        return SQLITE_ERROR;
    };
    let mut file = file.write();
    let now = file.data.len();
    file.data.truncate(now.min(size as usize));
    SQLITE_OK
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
    let Some(file) = file2file().get(&(pFile as usize)).cloned() else {
        return SQLITE_ERROR;
    };
    *pSize = file.read().data.len() as sqlite3_int64;
    SQLITE_OK
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

static IO_METHODS: sqlite3_io_methods = sqlite3_io_methods {
    iVersion: 1,
    xClose: Some(xClose),
    xRead: Some(xRead),
    xWrite: Some(xWrite),
    xTruncate: Some(xTruncate),
    xSync: Some(xSync),
    xFileSize: Some(xFileSize),
    xLock: Some(xLock),
    xUnlock: Some(xUnlock),
    xCheckReservedLock: Some(xCheckReservedLock),
    xFileControl: Some(xFileControl),
    xSectorSize: Some(xSectorSize),
    xDeviceCharacteristics: Some(xDeviceCharacteristics),
    xShmMap: None,
    xShmLock: None,
    xShmBarrier: None,
    xShmUnmap: None,
    xFetch: None,
    xUnfetch: None,
};

fn vfs() -> sqlite3_vfs {
    sqlite3_vfs {
        iVersion: 1,
        szOsFile: std::mem::size_of::<sqlite3_file>() as i32,
        mxPathname: 1024,
        pNext: std::ptr::null_mut(),
        zName: c"memvfs".as_ptr().cast(),
        pAppData: std::ptr::null_mut(),
        xOpen: Some(xOpen),
        xDelete: Some(xDelete),
        xAccess: Some(xAccess),
        xFullPathname: Some(xFullPathname),
        xDlOpen: None,
        xDlError: None,
        xDlSym: None,
        xDlClose: None,
        xRandomness: Some(xRandomness),
        xSleep: Some(xSleep),
        xCurrentTime: Some(xCurrentTime),
        xGetLastError: Some(xGetLastError),
        xCurrentTimeInt64: Some(xCurrentTimeInt64),
        xSetSystemCall: None,
        xGetSystemCall: None,
        xNextSystemCall: None,
    }
}

pub(crate) fn install_memory_vfs() -> ::std::os::raw::c_int {
    unsafe { sqlite3_vfs_register(Box::leak(Box::new(vfs())), 1) }
}
