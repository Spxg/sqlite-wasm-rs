use std::ffi::c_char;
use std::ffi::c_int;
use std::ffi::c_void;
use std::ptr;

#[link(name = "sqlite_uuid4", kind = "static")]
extern "C" {
    pub fn sqlite3_uuid4_init(
        db: *mut c_void,
        pzErrMsg: *mut *mut c_char,
        pApi: *const c_void,
    ) -> c_int;
}

pub fn register(db: *mut c_void) -> c_int {
    unsafe { sqlite3_uuid4_init(db, ptr::null_mut(), ptr::null()) }
}
