use js_sys::{Date, Number};
use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
extern "C" {
    // performance.now()
    #[wasm_bindgen(js_namespace = ["globalThis", "performance"], js_name = now)]
    fn performance_now() -> Option<f64>;
}

pub type time_t = std::os::raw::c_longlong;

/// https://github.com/emscripten-core/emscripten/blob/df69e2ccc287beab6f580f33b33e6b5692f5d20b/system/lib/libc/musl/include/time.h#L40
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

/// https://github.com/emscripten-core/emscripten/blob/df69e2ccc287beab6f580f33b33e6b5692f5d20b/system/lib/libc/emscripten_internal.h#L42
///
/// https://github.com/sqlite/sqlite-wasm/blob/7c1b309c3bd07d8e6d92f82344108cebbd14f161/sqlite-wasm/jswasm/sqlite3-bundler-friendly.mjs#L3404
#[no_mangle]
pub unsafe extern "C" fn _localtime_js(t: time_t, tm: *mut tm) {
    let date = Date::new(&Number::from((t * 1000) as f64).into());

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

    (*tm).tm_gmtoff = -(date.get_timezone_offset() * 60.0) as _;
}

/// https://github.com/emscripten-core/emscripten/blob/df69e2ccc287beab6f580f33b33e6b5692f5d20b/system/lib/libc/emscripten_internal.h#L45
///
/// https://github.com/sqlite/sqlite-wasm/blob/7c1b309c3bd07d8e6d92f82344108cebbd14f161/sqlite-wasm/jswasm/sqlite3-bundler-friendly.mjs#L3460
#[no_mangle]
pub unsafe extern "C" fn _tzset_js(
    timezone: *mut std::os::raw::c_long,
    daylight: *mut std::os::raw::c_int,
    std_name: *mut std::os::raw::c_char,
    dst_name: *mut std::os::raw::c_char,
) {
    unsafe fn set_name(name: String, dst: *mut std::os::raw::c_char) {
        for (idx, byte) in name.bytes().enumerate() {
            *dst.add(idx) = byte as _;
        }
        *dst.add(name.len()) = 0;
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

/// https://github.com/sqlite/sqlite-wasm/blob/7c1b309c3bd07d8e6d92f82344108cebbd14f161/sqlite-wasm/jswasm/sqlite3-bundler-friendly.mjs#L3496
#[no_mangle]
pub unsafe extern "C" fn emscripten_get_now() -> std::os::raw::c_double {
    performance_now().expect("performance should be available")
}

// https://github.com/alexcrichton/dlmalloc-rs/blob/fb116603713825b43b113cc734bb7d663cb64be9/src/dlmalloc.rs#L141
const ALIGN: usize = std::mem::size_of::<usize>() * 2;

#[no_mangle]
pub unsafe extern "C" fn malloc(size: usize) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align_unchecked(size + ALIGN, ALIGN);
    let ptr = std::alloc::alloc(layout);

    if ptr.is_null() {
        return std::ptr::null_mut();
    }
    *ptr.cast::<usize>() = size;

    ptr.add(ALIGN)
}

#[no_mangle]
pub unsafe extern "C" fn free(ptr: *mut u8) {
    let ptr = ptr.sub(ALIGN);
    let size = *(ptr.cast::<usize>());

    let layout = std::alloc::Layout::from_size_align_unchecked(size + ALIGN, ALIGN);
    std::alloc::dealloc(ptr, layout);
}

#[no_mangle]
pub unsafe extern "C" fn realloc(ptr: *mut u8, new_size: usize) -> *mut u8 {
    let ptr = ptr.sub(ALIGN);
    let size = *(ptr.cast::<usize>());

    let layout = std::alloc::Layout::from_size_align_unchecked(size + ALIGN, ALIGN);
    let ptr = std::alloc::realloc(ptr, layout, new_size + ALIGN);

    if ptr.is_null() {
        return std::ptr::null_mut();
    }
    *ptr.cast::<usize>() = new_size;

    ptr.add(ALIGN)
}
