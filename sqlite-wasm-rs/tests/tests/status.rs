use sqlite_wasm_rs::export::*;
use wasm_bindgen_test::{console_log, wasm_bindgen_test};

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_memory_used() {
    init_sqlite().await.unwrap();
    unsafe {
        let used = sqlite3_memory_used();
        console_log!("memory used: {used}");
        assert!(used >= 0);
    }
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_highwater() {
    init_sqlite().await.unwrap();
    unsafe {
        let highwater = sqlite3_memory_highwater(0);
        console_log!("memory highwater: {highwater}");
        assert!(highwater >= 0);
    }
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_status() {
    init_sqlite().await.unwrap();
    unsafe {
        let mut used = 0;
        let mut highwater = 0;
        let ret = sqlite3_status(
            SQLITE_STATUS_MEMORY_USED,
            &mut used as _,
            &mut highwater as _,
            0,
        );
        assert_eq!(ret, SQLITE_OK);
        console_log!("memory status: used: {used} highwater: {highwater}");
        assert!(used >= 0);
        assert!(highwater >= 0);
    }
}
