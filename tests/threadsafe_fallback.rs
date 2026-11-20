//! SQLite initialization with `threadsafe` when `cfg` lacks `atomics`, as on stable rustc.
#![cfg(not(target_feature = "atomics"))]

use js_sys::{ArrayBuffer, WebAssembly};
use sqlite_wasm_rs::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn initialization_refuses_shared_memory() {
    let memory = wasm_bindgen::memory().unchecked_into::<WebAssembly::Memory>();
    let shared = !memory.buffer().is_instance_of::<ArrayBuffer>();
    let expected = if shared { SQLITE_MISUSE } else { SQLITE_OK };
    // SAFETY: no other thread uses SQLite in this test binary.
    assert_eq!(unsafe { sqlite3_initialize() }, expected);
}
