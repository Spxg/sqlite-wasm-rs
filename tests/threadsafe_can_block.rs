//! The `can_block` host hook on a dedicated worker, in builds with and without `--shared-memory`.

use js_sys::{SharedArrayBuffer, WebAssembly};
// Links the crate that defines the hook.
use sqlite_wasm_rs as _;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_dedicated_worker);

extern "C" {
    fn rust_sqlite_wasm_host_can_block() -> i32;
}

/// `memory.atomic.wait32` traps on unshared memory, so a worker may block exactly when the module's memory is shared.
#[wasm_bindgen_test]
fn workers_block_only_on_shared_memory() {
    let memory = wasm_bindgen::memory().unchecked_into::<WebAssembly::Memory>();
    let shared = memory.buffer().is_instance_of::<SharedArrayBuffer>();
    // SAFETY: the hook takes no arguments and only inspects the calling thread and the module's memory.
    let can_block = unsafe { rust_sqlite_wasm_host_can_block() } != 0;
    assert_eq!(can_block, shared);
}
