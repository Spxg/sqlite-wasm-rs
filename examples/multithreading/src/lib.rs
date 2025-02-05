#![allow(deprecated)]
use std::{ffi::CString, time::Duration};

use sqlite_wasm_rs::export::{
    sqlite3, sqlite3_close, sqlite3_column_int, sqlite3_exec, sqlite3_finalize,
    sqlite3_memory_highwater, sqlite3_memory_used, sqlite3_open_v2, sqlite3_prepare_v3,
    sqlite3_step, sqlite3_stmt, SQLITE_OK, SQLITE_OPEN_CREATE, SQLITE_OPEN_READWRITE, SQLITE_ROW,
};
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};
use wasm_bindgen_test::console_log;
use web_sys::js_sys;

pub fn spawn(f: impl FnOnce() + Send + 'static) -> Result<web_sys::Worker, JsValue> {
    let worker = web_sys::Worker::new("worker.js")?;

    let msg = js_sys::Array::new();
    msg.push(&wasm_bindgen::module());
    msg.push(&wasm_bindgen::memory());
    worker.post_message(&msg)?;

    let msg = js_sys::Array::new();
    let ptr = Box::into_raw(Box::new(Box::new(f) as Box<dyn FnOnce()>));
    msg.push(&JsValue::from(ptr as u32));
    worker.post_message(&msg)?;
    Ok(worker)
}

#[wasm_bindgen]
pub fn child_entry_point(addr: u32) {
    let closure = unsafe { Box::from_raw(addr as *mut Box<dyn FnOnce()>) };
    (*closure)();
}

#[wasm_bindgen]
pub async fn test_spawn() {
    let sqlite3 = sqlite_wasm_rs::export::init_sqlite().await.unwrap();
    console_log!(
        "thread {:?}: init sqlite success",
        std::thread::current().id()
    );

    sqlite3.install_opfs_sahpool(None).await.unwrap();
    console_log!(
        "thread {:?}: install opfs-sahpool success",
        std::thread::current().id()
    );
    create_table();

    // memory monitior
    spawn(move || {
        for i in 0.. {
            std::thread::sleep(Duration::from_secs(1));
            let current = unsafe { sqlite3_memory_used() };
            let highwater = unsafe { sqlite3_memory_highwater(0) };
            console_log!(
                "thread {:?}: [{i}] memory_used: {current}, memory_highwater: {highwater}",
                std::thread::current().id()
            );
        }
    })
    .unwrap();

    spawn(move || {
        for i in 0.. {
            std::thread::sleep(Duration::from_secs(1));
            insert_record();
            console_log!(
                "thread {:?}: [{i}] insert record success",
                std::thread::current().id()
            );
        }
    })
    .unwrap();

    spawn(move || {
        for i in 0.. {
            std::thread::sleep(Duration::from_secs(1));
            let count = count_record();
            console_log!(
                "thread {:?}: [{i}] table record size is {count}",
                std::thread::current().id()
            );
        }
    })
    .unwrap();
}

fn connection() -> *mut sqlite3 {
    let mut db = std::ptr::null_mut();
    let f = CString::new("file:multithreading?vfs=opfs-sahpool").unwrap();
    let ret = unsafe {
        sqlite3_open_v2(
            f.as_ptr(),
            &mut db as _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(ret, SQLITE_OK);
    console_log!(
        "thread {:?}: connect db success",
        std::thread::current().id()
    );

    db
}

fn insert_record() {
    let conn = connection();
    let sql = CString::new("INSERT INTO COMPANY (NAME) VALUES ('FAKE');").unwrap();

    let ret = unsafe {
        sqlite3_exec(
            conn,
            sql.as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    unsafe { sqlite3_close(conn) };
}

fn count_record() -> i32 {
    let conn = connection();
    let sql = CString::new("SELECT COUNT(*) FROM COMPANY;").unwrap();
    let mut stmt: *mut sqlite3_stmt = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_prepare_v3(
            conn,
            sql.as_ptr(),
            -1,
            0,
            &mut stmt as _,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(ret, SQLITE_OK);

    let rc = unsafe { sqlite3_step(stmt) };
    assert_eq!(rc, SQLITE_ROW);
    let count = unsafe { sqlite3_column_int(stmt, 0) };
    unsafe {
        sqlite3_finalize(stmt);
    }
    unsafe { sqlite3_close(conn) };

    count
}

fn create_table() {
    let conn = connection();
    let sql = CString::new(
        "CREATE TABLE IF NOT EXISTS COMPANY(
            ID INTEGER PRIMARY KEY AUTOINCREMENT,
            NAME           TEXT    NOT NULL );",
    )
    .unwrap();
    let ret = unsafe {
        sqlite3_exec(
            conn,
            sql.as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    unsafe { sqlite3_close(conn) };

    console_log!(
        "thread {:?}: create table success",
        std::thread::current().id()
    );
}
