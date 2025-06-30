wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);

use sqlite_wasm_rs::{
    self as ffi,
    sahpool_vfs::{install as install_opfs_sahpool, OpfsSAHPoolCfg},
};
use wasm_bindgen_test::wasm_bindgen_test;

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(clippy::type_complexity)]
mod shim;

#[wasm_bindgen_test]
async fn usage() {
    // open with memory vfs
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        ffi::sqlite3_open_v2(
            c"mem.db".as_ptr().cast(),
            &mut db as *mut _,
            ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(ffi::SQLITE_OK, ret);

    // install opfs-sahpool persistent vfs and set as default vfs
    install_opfs_sahpool(&OpfsSAHPoolCfg::default(), true)
        .await
        .unwrap();

    // open with opfs-sahpool vfs
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        ffi::sqlite3_open_v2(
            c"opfs-sahpool.db".as_ptr().cast(),
            &mut db as *mut _,
            ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(ffi::SQLITE_OK, ret);
}
