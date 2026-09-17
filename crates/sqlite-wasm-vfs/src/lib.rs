#![doc = include_str!("../README.md")]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

/// IndexedDB VFS implementation with relaxed durability guarantees.
#[cfg(feature = "relaxed-idb")]
pub mod relaxed_idb;

/// Origin Private File System (OPFS) VFS implementation using `SyncAccessHandle`.
#[cfg(feature = "sahpool")]
pub mod sahpool;

// OPFS offsets and IndexedDB numeric keys use JavaScript Numbers.
#[cfg(any(feature = "sahpool", feature = "relaxed-idb"))]
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

#[cfg(any(feature = "sahpool", feature = "relaxed-idb"))]
fn check_js_file_size(size: u64, code: i32) -> rsqlite_vfs::VfsResult<()> {
    if size > MAX_SAFE_INTEGER {
        return Err(rsqlite_vfs::VfsError::new(
            code,
            "File offset or size exceeds JavaScript's safe integer range".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);
