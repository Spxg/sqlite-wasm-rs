use super::export::{sqlite3_int64, sqlite3_vfs, sqlite3_vfs_register};
use js_sys::{Date, Math};
use wasm_bindgen::JsCast;
use web_sys::{SharedWorkerGlobalScope, WorkerGlobalScope};

pub type time_t = std::os::raw::c_longlong;

#[repr(C)]
pub struct tm {
    pub tm_sec: std::os::raw::c_int,
    pub tm_min: std::os::raw::c_int,
    pub tm_hour: std::os::raw::c_int,
    pub tm_mday: std::os::raw::c_int,
    pub tm_mon: std::os::raw::c_int,
    pub tm_year: std::os::raw::c_int,
    pub tm_wday: std::os::raw::c_int,
    pub tm_yday: std::os::raw::c_int,
    pub tm_isdst: std::os::raw::c_int,
    pub tm_gmtoff: std::os::raw::c_long,
    pub tm_zone: *mut std::os::raw::c_char,
}

const INT53_MAX: time_t = 9007199254740992;
const INT53_MIN: time_t = -9007199254740992;

fn yday_from_date(date: &Date) -> u32 {
    const MONTH_DAYS_LEAP_CUMULATIVE: [u32; 12] =
        [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];

    const MONTH_DAYS_REGULAR_CUMULATIVE: [u32; 12] =
        [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];

    let year = date.get_full_year();
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);

    let month_days_cumulative = if leap {
        MONTH_DAYS_LEAP_CUMULATIVE
    } else {
        MONTH_DAYS_REGULAR_CUMULATIVE
    };
    month_days_cumulative[date.get_month() as usize] + date.get_date() - 1
}

#[no_mangle]
pub unsafe extern "C" fn _localtime_js(t: time_t, tm: *mut tm) {
    assert!(t < INT53_MIN || t > INT53_MAX, "wrong time range");

    let date = Date::new(&t.into());
    (*tm).tm_sec = date.get_seconds() as _;
    (*tm).tm_min = date.get_minutes() as _;
    (*tm).tm_hour = date.get_hours() as _;
    (*tm).tm_mday = date.get_date() as _;
    (*tm).tm_mon = date.get_month() as _;
    (*tm).tm_year = (date.get_full_year() - 1900) as _;
    (*tm).tm_wday = date.get_day() as _;
    (*tm).tm_yday = yday_from_date(&date) as _;

    let start = Date::new_with_year_month_day(date.get_full_year(), 0, 1);
    let summer_offset =
        Date::new_with_year_month_day(date.get_full_year(), 6, 1).get_timezone_offset();
    let winter_offset = start.get_timezone_offset();
    (*tm).tm_isdst = i32::from(
        summer_offset != winter_offset
            && date.get_timezone_offset() == winter_offset.min(summer_offset),
    );

    (*tm).tm_gmtoff = (date.get_timezone_offset() * 60.0) as _;
}

#[no_mangle]
pub unsafe extern "C" fn _tzset_js(
    timezone: *mut std::os::raw::c_longlong,
    daylight: *mut std::os::raw::c_int,
    std_name: *mut std::os::raw::c_char,
    dst_name: *mut std::os::raw::c_char,
) {
    unsafe fn set_name(name: String, dst: *mut std::os::raw::c_char) {
        for (idx, byte) in name.bytes().enumerate() {
            *dst.offset(idx as _) = byte as _;
        }
        *dst.offset(name.len() as _) = 0;
    }

    fn extract_zone(timezone_offset: f64) -> String {
        let sign = if timezone_offset >= 0.0 { '-' } else { '+' };
        let offset = timezone_offset.abs();
        let hours = format!("{:02}", (offset / 60.0).floor() as i32);
        let minutes = format!("{:02}", (offset % 60.0) as i32);
        format!("UTC{sign}{hours}{minutes}")
    }

    let current_year = Date::new_0().get_full_year();
    let winter = Date::new_with_year_month_day(current_year, 0, 1);
    let summer = Date::new_with_year_month_day(current_year, 6, 1);
    let winter_offset = winter.get_timezone_offset();
    let summer_offset = summer.get_timezone_offset();

    let std_timezone_offset = winter_offset.max(summer_offset);
    *timezone = (std_timezone_offset * 60.0) as _;
    *daylight = i32::from(winter_offset != summer_offset);

    let winter_name = extract_zone(winter_offset);
    let summer_name = extract_zone(summer_offset);

    if summer_offset < winter_offset {
        set_name(winter_name, std_name);
        set_name(summer_name, dst_name);
    } else {
        set_name(winter_name, dst_name);
        set_name(summer_name, std_name);
    }
}

/// Copy from sqlite-wasm
#[no_mangle]
pub unsafe extern "C" fn emscripten_get_now() -> std::os::raw::c_double {
    let performance = if let Some(window) = web_sys::window() {
        window.performance()
    } else if let Ok(worker) = js_sys::global().dyn_into::<WorkerGlobalScope>() {
        worker.performance()
    } else if let Ok(worker) = js_sys::global().dyn_into::<SharedWorkerGlobalScope>() {
        worker.performance()
    } else {
        panic!("sqlite not run in main_thread, dedicated worker or shared worker");
    }
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
        zName: "none\0".as_ptr().cast(),
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
