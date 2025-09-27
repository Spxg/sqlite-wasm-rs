pub mod memory;
pub mod sahpool;
pub mod utils;

#[cfg(feature = "relaxed-idb")]
pub mod relaxed_idb;

#[no_mangle]
pub unsafe extern "C" fn sqlite3_os_init() -> std::ffi::c_int {
    memory::install()
}
