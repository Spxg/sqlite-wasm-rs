use crate::vfs::check_persistent;
use sqlite_wasm_rs::export::*;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn test_memory_vfs() {
    let mut db1 = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"file:mem.db?vfs=memvfs".as_ptr().cast(),
            &mut db1 as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    let state = check_persistent(db1);

    let ret = unsafe { sqlite3_close(db1) };
    assert_eq!(SQLITE_OK, ret);

    let mut db2 = std::ptr::null_mut();
    // is equivalent to the above
    let ret = unsafe {
        sqlite3_open_v2(
            c"mem.db".as_ptr().cast(),
            &mut db2 as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            c"memvfs".as_ptr().cast(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    assert_eq!(!state, check_persistent(db2));
}
