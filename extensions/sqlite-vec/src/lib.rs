#![doc = include_str!("../README.md")]
#![cfg_attr(not(test), no_std)]

#[link(name = "wsqlite_vec0")]
extern "C" {
    pub fn sqlite3_vec_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    use sqlite_wasm_rs::*;
    use std::{
        ffi::{c_char, c_int, CStr},
        ptr,
    };

    #[wasm_bindgen_test::wasm_bindgen_test]
    fn test_auto_extension() {
        unsafe {
            let init = std::mem::transmute::<
                *const (),
                unsafe extern "C" fn(
                    *mut sqlite3,
                    *mut *mut c_char,
                    *const sqlite3_api_routines,
                ) -> c_int,
            >(sqlite3_vec_init as *const ());
            assert_eq!(sqlite3_auto_extension(Some(init)), SQLITE_OK);

            let mut db = ptr::null_mut();
            assert_eq!(sqlite3_open(c":memory:".as_ptr(), &mut db), SQLITE_OK);

            let mut stmt = ptr::null_mut();
            assert_eq!(
                sqlite3_prepare_v2(
                    db,
                    c"SELECT vec_version()".as_ptr(),
                    -1,
                    &mut stmt,
                    ptr::null_mut(),
                ),
                SQLITE_OK
            );
            assert_eq!(sqlite3_step(stmt), SQLITE_ROW);

            let version = sqlite3_column_text(stmt, 0);
            assert!(!version.is_null());
            assert!(CStr::from_ptr(version.cast()).to_bytes().starts_with(b"v"));

            assert_eq!(sqlite3_step(stmt), SQLITE_DONE);
            assert_eq!(sqlite3_finalize(stmt), SQLITE_OK);
            assert_eq!(sqlite3_close(db), SQLITE_OK);
            sqlite3_reset_auto_extension();
        }
    }
}
