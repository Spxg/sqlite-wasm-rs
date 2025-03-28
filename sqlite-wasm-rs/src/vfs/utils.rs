//! Some tools for implementing VFS

use crate::libsqlite3::{sqlite3_file, sqlite3_vfs};

use fragile::Fragile;
use js_sys::{Math, Number, Uint8Array, WebAssembly};
use std::ops::{Deref, DerefMut};
use wasm_bindgen::{prelude::wasm_bindgen, JsCast};

/// Wrap the pVfs pointer, which is often used in VFS implementation.
///
/// Use vfs pointer as the map key to find the corresponding vfs handle, such as `OpfsSAHPool`.
#[derive(Hash, PartialEq, Eq)]
pub struct VfsPtr(pub *mut sqlite3_vfs);

unsafe impl Send for VfsPtr {}
unsafe impl Sync for VfsPtr {}

/// Wrap the pFile pointer, which is often used in VFS implementation.
///
/// Use file pointer as the map key to find the corresponding file handle, such as `MemFile`.
#[derive(Hash, PartialEq, Eq)]
pub struct FilePtr(pub *mut sqlite3_file);

unsafe impl Send for FilePtr {}
unsafe impl Sync for FilePtr {}

/// A [`FragileComfirmed<T>`] wraps a non sendable `T` to be safely send to other threads.
///
/// Once the value has been wrapped it can be sent to other threads but access
/// to the value on those threads will fail.
pub struct FragileComfirmed<T> {
    fragile: Fragile<T>,
}

unsafe impl<T> Send for FragileComfirmed<T> {}
unsafe impl<T> Sync for FragileComfirmed<T> {}

impl<T> FragileComfirmed<T> {
    pub fn new(t: T) -> Self {
        FragileComfirmed {
            fragile: Fragile::new(t),
        }
    }
}

impl<T> Deref for FragileComfirmed<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.fragile.get()
    }
}

impl<T> DerefMut for FragileComfirmed<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.fragile.get_mut()
    }
}

/// get random name if zFileName is null and other cases
pub fn get_random_name() -> String {
    let random = Number::from(Math::random()).to_string(36).unwrap();
    random.slice(2, random.length()).as_string().unwrap()
}

/// Directly using copy_from and copy_to to convert Uint8Array and Vec<u8> is risky.
/// There is a possibility that the memory will grow and the buffer will be detached during copy.
/// So here we convert on the js side.
///
/// Related issues:
///
/// <https://github.com/rustwasm/wasm-bindgen/issues/4395>
///
/// <https://github.com/rustwasm/wasm-bindgen/issues/4392>
#[wasm_bindgen(module = "/src/vfs/utils.js")]
extern "C" {
    type JSUtils;

    #[wasm_bindgen(static_method_of = JSUtils, js_name = toSlice)]
    fn to_slice(memory: &WebAssembly::Memory, buffer: &Uint8Array, dst: *mut u8, len: usize);

    #[wasm_bindgen(static_method_of = JSUtils, js_name = toUint8Array)]
    fn to_uint8_array(memory: &WebAssembly::Memory, src: *const u8, len: usize, dst: &Uint8Array);
}

/// Copy `Uint8Array` and return new `Vec<u8>`
pub fn copy_to_vec(src: &Uint8Array) -> Vec<u8> {
    let mut vec = vec![0u8; src.length() as usize];
    copy_to_slice(src, vec.as_mut_slice());
    vec
}

/// Copy `Uint8Array` to `slice`
pub fn copy_to_slice(src: &Uint8Array, dst: &mut [u8]) {
    assert!(
        src.length() as usize == dst.len(),
        "Unit8Array and slice have different sizes"
    );

    let buf = wasm_bindgen::memory();
    let mem = buf.unchecked_ref::<WebAssembly::Memory>();
    JSUtils::to_slice(mem, src, dst.as_mut_ptr(), dst.len());
}

/// Copy `slice` and return new `Uint8Array`
pub fn copy_to_uint8_array(src: &[u8]) -> Uint8Array {
    let uint8 = Uint8Array::new_with_length(src.len() as u32);
    copy_to_uint8_array_subarray(src, &uint8);
    uint8
}

/// Copy `slice` to `Unit8Array`
pub fn copy_to_uint8_array_subarray(src: &[u8], dst: &Uint8Array) {
    assert!(
        src.len() == dst.length() as _,
        "Unit8Array and slice have different sizes"
    );
    let buf = wasm_bindgen::memory();
    let mem = buf.unchecked_ref::<WebAssembly::Memory>();
    JSUtils::to_uint8_array(mem, src.as_ptr(), src.len(), dst)
}

/// Return error code if expr is true.
///
/// The default error code is SQLITE_ERROR.
#[macro_export]
macro_rules! bail {
    ($ex:expr) => {
        bail!($ex, SQLITE_ERROR);
    };
    ($ex:expr, $code: expr) => {
        if $ex {
            return $code;
        }
    };
}

/// Unpack Option<T>.
///
/// If it is None, return an error code.
///
/// The default error code is SQLITE_ERROR.
#[macro_export]
macro_rules! check_option {
    ($ex:expr) => {
        check_option!($ex, SQLITE_ERROR)
    };
    ($ex:expr, $code: expr) => {
        if let Some(v) = $ex {
            v
        } else {
            return $code;
        }
    };
}

/// Unpack Ok<T>.
///
/// If it is Err, return an error code.
///
/// The default err code is SQLITE_ERROR.
#[macro_export]
macro_rules! check_result {
    ($ex:expr) => {
        check_result!($ex, SQLITE_ERROR)
    };
    ($ex:expr, $code: expr) => {
        if let Ok(v) = $ex {
            v
        } else {
            return $code;
        }
    };
}

#[cfg(test)]
mod tests {
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    use crate::vfs::utils::{copy_to_slice, copy_to_uint8_array_subarray};

    use super::{copy_to_uint8_array, copy_to_vec};
    use js_sys::Uint8Array;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn test_js_utils() {
        let buf1 = vec![1, 2, 3, 4];
        let uint8 = copy_to_uint8_array(&buf1);
        let buf2 = copy_to_vec(&uint8);
        assert_eq!(buf1, buf2);

        let mut buf3 = vec![0u8; 2];
        copy_to_slice(&uint8.subarray(0, 2), &mut buf3);
        assert_eq!(buf3, vec![1, 2]);

        let buf4 = Uint8Array::new_with_length(3);
        copy_to_uint8_array_subarray(&buf3, &buf4.subarray(1, 3));
        assert!(buf4.get_index(0) == 0);
        assert!(buf4.get_index(1) == 1);
        assert!(buf4.get_index(2) == 2);
    }
}
