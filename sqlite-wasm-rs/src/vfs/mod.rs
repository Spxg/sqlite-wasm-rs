#![doc = include_str!("README.md")]

#[cfg(feature = "relaxed-idb")]
pub mod relaxed_idb;

pub mod memory;
pub mod sahpool;
pub mod utils;

#[no_mangle]
pub unsafe extern "C" fn sqlite3_os_init() -> std::os::raw::c_int {
    memory::install()
}
