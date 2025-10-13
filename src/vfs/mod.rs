pub mod memory;
pub mod sahpool;
pub mod utils;

#[cfg(feature = "relaxed-idb")]
pub mod relaxed_idb;

#[no_mangle]
pub unsafe extern "C" fn sqlite3_os_init() -> std::ffi::c_int {
    use crate::libsqlite3::SQLITE_OK;
    memory::install();
    SQLITE_OK
}

#[no_mangle]
pub unsafe extern "C" fn sqlite3_os_end() -> std::ffi::c_int {
    use crate::libsqlite3::SQLITE_OK;
    memory::uninstall();
    SQLITE_OK
}

#[cfg(test)]
mod tests {
    use crate::{sqlite3_initialize, sqlite3_shutdown, SQLITE_OK};
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn test_initialize_shutdown() {
        unsafe {
            assert_eq!(sqlite3_initialize(), SQLITE_OK, "failed to initialize");
            assert_eq!(sqlite3_shutdown(), SQLITE_OK, "failed to shutdown");
        }
    }
}
