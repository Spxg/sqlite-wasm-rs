use std::ffi::CString;

use super::{cstr, memory_db};
use sqlite_wasm_rs::export::*;
use wasm_bindgen_test::{console_log, wasm_bindgen_test};

#[wasm_bindgen_test]
#[allow(unused)]
fn test_aux() {
    let db = memory_db();

    unsafe extern "C" fn free(ptr: *mut ::std::os::raw::c_void) {
        drop(CString::from_raw(ptr.cast()));
    }

    unsafe extern "C" fn set_and_get_aux(
        arg1: *mut sqlite3_context,
        arg2: ::std::os::raw::c_int,
        arg3: *mut *mut sqlite3_value,
    ) {
        console_log!("set_and_get_aux");
        let s = CString::new("aux").unwrap().into_raw();
        sqlite3_set_auxdata(arg1, 1, s.cast(), Some(free));
        let s = sqlite3_get_auxdata(arg1, 1);
        let s = CString::from_raw(s.cast());
        assert_eq!(s.to_str().unwrap(), "aux");
        s.into_raw();
    }

    unsafe {
        assert_eq!(
            sqlite3_create_function_v2(
                db,
                cstr("x_set_get_aux").as_ptr(),
                0,
                SQLITE_UTF8,
                std::ptr::null_mut(),
                Some(set_and_get_aux),
                None,
                None,
                None,
            ),
            SQLITE_OK
        );
        assert_eq!(
            sqlite3_exec(
                db,
                cstr("SELECT x_set_get_aux();").as_ptr(),
                None,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ),
            SQLITE_OK
        );
    }
}
