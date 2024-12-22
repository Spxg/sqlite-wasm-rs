wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);

use sqlite_wasm_rs::{MemoryOpts, SQLite, SQLiteOpts, ffi};
use wasm_bindgen::{JsValue, prelude::Closure};
use wasm_bindgen_test::{console_log, wasm_bindgen_test};

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_wrong_memory_range() {
    assert!(
        SQLite::new(SQLiteOpts {
            memory: MemoryOpts {
                initial: 500,
                maximum: 256,
            },
        })
        .await
        .is_err(),
    );
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_sqlite_version() {
    let sqlite = SQLite::default().await.unwrap();
    console_log!("{:#?}", sqlite.version());
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_open_and_exec() {
    let sqlite = SQLite::default().await.unwrap();
    let capi = sqlite.capi();
    let wasm = sqlite.wasm();

    let ptr = wasm.alloc_ptr(1, true);
    let ret = capi.sqlite3_open("test_open_and_exec.db", ptr as *mut _);
    assert_eq!(capi.SQLITE_OK(), ret);

    let mut db: *mut ffi::Sqlite3DbHandle = std::ptr::null_mut();
    wasm.copy_to_rust(ptr as _, &mut db);

    let sql = "CREATE TABLE IF NOT EXISTS COMPANY(
                        ID INT PRIMARY KEY     NOT NULL,
                        NAME           TEXT    NOT NULL );";

    let err_msg = wasm.alloc_ptr(1, true);
    let ret = capi.sqlite3_exec(db, sql, None, std::ptr::null_mut(), err_msg as *mut _);
    assert_eq!(capi.SQLITE_OK(), ret);

    let sql = "INSERT INTO COMPANY (ID,NAME) VALUES (1, 'John Doe');";
    let ret = capi.sqlite3_exec(db, sql, None, std::ptr::null_mut(), err_msg as *mut _);
    assert_eq!(capi.SQLITE_OK(), ret);

    let sql = "SELECT * FROM COMPANY;";
    let f = |arg1: Vec<JsValue>, arg2: Vec<String>| -> i32 {
        let mut iter = arg2.into_iter().zip(arg1);
        assert_eq!(("ID".into(), JsValue::from("1")), iter.next().unwrap());
        assert_eq!(
            ("NAME".into(), JsValue::from("John Doe")),
            iter.next().unwrap()
        );
        0
    };
    let ret = capi.sqlite3_exec(
        db,
        sql,
        Some(&Closure::new(f)),
        std::ptr::null_mut(),
        err_msg as *mut _,
    );
    assert_eq!(capi.SQLITE_OK(), ret);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_open_v2_and_exec_opfs() {
    let sqlite = SQLite::default().await.unwrap();
    let capi = sqlite.capi();
    let wasm = sqlite.wasm();

    let ptr = wasm.alloc_ptr(1, true);
    let ret = capi.sqlite3_open_v2(
        "test_open_v2_and_exec_opfs.db",
        ptr as *mut _,
        capi.SQLITE_OPEN_READWRITE() | capi.SQLITE_OPEN_CREATE(),
        "opfs",
    );
    assert_eq!(capi.SQLITE_OK(), ret);

    let mut db: *mut ffi::Sqlite3DbHandle = std::ptr::null_mut();
    wasm.copy_to_rust(ptr as _, &mut db);

    let err_msg = wasm.alloc_ptr(1, true);

    // drop first
    let sql = "DROP TABLE COMPANY;";
    let ret = capi.sqlite3_exec(db, sql, None, std::ptr::null_mut(), err_msg as *mut _);
    if (capi.SQLITE_OK() == ret) {
        console_log!("test_open_v2_and_exec_opfs: table exist before test, dropped");
    }

    let sql = "CREATE TABLE IF NOT EXISTS COMPANY(
                        ID INT PRIMARY KEY     NOT NULL,
                        NAME           TEXT    NOT NULL );";

    let ret = capi.sqlite3_exec(db, sql, None, std::ptr::null_mut(), err_msg as *mut _);
    assert_eq!(capi.SQLITE_OK(), ret);

    let sql = "INSERT INTO COMPANY (ID,NAME) VALUES (1, 'John Doe');";
    let ret = capi.sqlite3_exec(db, sql, None, std::ptr::null_mut(), err_msg as *mut _);
    assert_eq!(capi.SQLITE_OK(), ret);

    let sql = "SELECT * FROM COMPANY;";
    let f = |arg1: Vec<JsValue>, arg2: Vec<String>| -> i32 {
        let mut iter = arg2.into_iter().zip(arg1);
        assert_eq!(("ID".into(), JsValue::from("1")), iter.next().unwrap());
        assert_eq!(
            ("NAME".into(), JsValue::from("John Doe")),
            iter.next().unwrap()
        );
        0
    };
    let ret = capi.sqlite3_exec(
        db,
        sql,
        Some(&Closure::new(f)),
        std::ptr::null_mut(),
        err_msg as *mut _,
    );
    assert_eq!(capi.SQLITE_OK(), ret);
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_sqlite3_close_v2() {
    let sqlite = SQLite::default().await.unwrap();
    let capi = sqlite.capi();
    let wasm = sqlite.wasm();

    let ptr = wasm.alloc_ptr(1, true);
    let ret = capi.sqlite3_open("test_sqlite3_close_v2.db", ptr as _);
    assert_eq!(capi.SQLITE_OK(), ret);

    let mut db: *mut ffi::Sqlite3DbHandle = std::ptr::null_mut();
    wasm.copy_to_rust(ptr, &mut db);

    let ret = capi.sqlite3_close_v2(db);
    assert_eq!(capi.SQLITE_OK(), ret);

    let ret = capi.sqlite3_close_v2(db);
    assert_eq!(capi.SQLITE_MISUSE(), ret);
}
