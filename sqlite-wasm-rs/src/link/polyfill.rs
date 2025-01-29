use super::export::{sqlite3_int64, sqlite3_vfs, sqlite3_vfs_register};
use js_sys::{Date, Math};

/// Copy from sqlite-wasm
#[no_mangle]
pub unsafe extern "C" fn emscripten_get_now() -> std::os::raw::c_double {
    let window = web_sys::window().expect("should have a window in this context");
    let performance = window
        .performance()
        .expect("performance should be available");
    performance.now()
}

#[no_mangle]
pub unsafe extern "C" fn sqlite3_os_init() -> std::os::raw::c_int {
    let vfs = sqlite3_vfs {
        iVersion: 1,
        szOsFile: 0,
        mxPathname: 512,
        pNext: std::ptr::null_mut(),
        zName: "none\0".as_ptr() as *const std::os::raw::c_char,
        pAppData: std::ptr::null_mut(),
        xOpen: None,
        xDelete: None,
        xAccess: None,
        xFullPathname: None,
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
    };

    sqlite3_vfs_register(Box::leak(Box::new(vfs)), 0)
}

#[no_mangle]
pub unsafe extern "C" fn sqlite3_os_end() -> std::os::raw::c_int {
    0
}

unsafe extern "C" fn xSleep(
    _arg1: *mut sqlite3_vfs,
    _microseconds: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    // https://github.com/sqlite/sqlite/blob/fb9e8e48fd70b463fb7ba6d99e00f2be54df749e/ext/wasm/api/sqlite3-vfs-opfs-sahpool.c-pp.js#L416
    0
}

unsafe extern "C" fn xRandomness(
    _arg1: *mut sqlite3_vfs,
    nByte: ::std::os::raw::c_int,
    zOut: *mut ::std::os::raw::c_char,
) -> ::std::os::raw::c_int {
    // https://github.com/sqlite/sqlite/blob/fb9e8e48fd70b463fb7ba6d99e00f2be54df749e/ext/wasm/api/sqlite3-vfs-opfs.c-pp.js#L951
    for i in 0..nByte {
        *zOut.offset(i as isize) = ((Math::random() * 255000.0) as u8 & 0xFF) as _;
    }
    0
}

unsafe extern "C" fn xCurrentTime(
    _arg1: *mut sqlite3_vfs,
    arg2: *mut f64,
) -> ::std::os::raw::c_int {
    // https://github.com/sqlite/sqlite/blob/fb9e8e48fd70b463fb7ba6d99e00f2be54df749e/ext/wasm/api/sqlite3-vfs-opfs.c-pp.js#L870
    *arg2 = 2440587.5 + (Date::new_0().get_time() / 86400000.0) + 2440587.5;
    0
}

unsafe extern "C" fn xGetLastError(
    _arg1: *mut sqlite3_vfs,
    _arg2: ::std::os::raw::c_int,
    _arg3: *mut ::std::os::raw::c_char,
) -> ::std::os::raw::c_int {
    // https://github.com/sqlite/sqlite/blob/fb9e8e48fd70b463fb7ba6d99e00f2be54df749e/ext/wasm/api/sqlite3-vfs-opfs.c-pp.js#L895
    0
}

unsafe extern "C" fn xCurrentTimeInt64(
    _arg1: *mut sqlite3_vfs,
    arg2: *mut sqlite3_int64,
) -> ::std::os::raw::c_int {
    // https://github.com/sqlite/sqlite/blob/fb9e8e48fd70b463fb7ba6d99e00f2be54df749e/ext/wasm/api/sqlite3-vfs-opfs.c-pp.js#L877
    *arg2 = ((2440587.5 * 86400000.0) + Date::new_0().get_time()) as sqlite3_int64;
    0
}

const ALIGN: usize = 8;

#[no_mangle]
pub unsafe extern "C" fn malloc(size: usize) -> *mut u8 {
    let layout = match std::alloc::Layout::from_size_align(size + ALIGN, ALIGN) {
        Ok(layout) => layout,
        Err(_) => return std::ptr::null_mut(),
    };

    let ptr = std::alloc::alloc(layout);
    if ptr.is_null() {
        return std::ptr::null_mut();
    }

    *(ptr as *mut usize) = size;
    ptr.offset(ALIGN as isize)
}

#[no_mangle]
pub unsafe extern "C" fn free(ptr: *mut u8) {
    let ptr = ptr.offset(-(ALIGN as isize));
    let size = *(ptr as *mut usize);
    let layout = std::alloc::Layout::from_size_align_unchecked(size + ALIGN, ALIGN);

    std::alloc::dealloc(ptr, layout);
}

#[no_mangle]
pub unsafe extern "C" fn realloc(ptr: *mut u8, new_size: usize) -> *mut u8 {
    let ptr = ptr.offset(-(ALIGN as isize));
    let size = *(ptr as *mut usize);
    let layout = std::alloc::Layout::from_size_align_unchecked(size + ALIGN, ALIGN);

    let ptr = std::alloc::realloc(ptr, layout, new_size + ALIGN);
    if ptr.is_null() {
        return std::ptr::null_mut();
    }

    *(ptr as *mut usize) = new_size;
    ptr.offset(ALIGN as isize)
}
