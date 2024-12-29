# SQLite Wasm Rust

Wrap the official [`sqlite-wasm`](https://github.com/sqlite/sqlite-wasm), and expect to provide a usable C-like API. 

And currently  the following APIs are implemented and tested:

| 1                                                            | 2                                                            | 3                                                            | 4                                                            | 5                                                            | 6                                                            |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | :----------------------------------------------------------- |
| [`sqlite3_open_v2`](https://www.sqlite.org/c3ref/open.html)  | [`sqlite3_exec`](https://www.sqlite.org/c3ref/exec.html)     | [`sqlite3_close`](https://www.sqlite.org/c3ref/close.html)   | [`sqlite3_close_v2`](https://www.sqlite.org/c3ref/close.html) | [`sqlite3_changes`](https://www.sqlite.org/c3ref/changes.html) | [`sqlite3_deserialize`](https://www.sqlite.org/c3ref/deserialize.html) |
| [`sqlite3_serialize`](https://www.sqlite.org/c3ref/serialize.html) | [`sqlite3_free`](https://www.sqlite.org/c3ref/free.html)     | [`sqlite3_create_function_v2`](https://www.sqlite.org/c3ref/create_function.html) | [`sqlite3_result_text`](https://www.sqlite.org/c3ref/result_blob.html) | [`sqlite3_result_blob`](https://www.sqlite.org/c3ref/result_blob.html) | [`sqlite3_result_int`](https://www.sqlite.org/c3ref/result_blob.html) |
| [`sqlite3_result_int64`](https://www.sqlite.org/c3ref/result_blob.html) | [`sqlite3_result_double`](https://www.sqlite.org/c3ref/result_blob.html) | [`sqlite3_result_null`](https://www.sqlite.org/c3ref/result_blob.html) | [`sqlite3_column_value`](https://www.sqlite.org/c3ref/column_blob.html) | [`sqlite3_column_count`](https://www.sqlite.org/c3ref/column_count.html) | [`sqlite3_column_name`](https://www.sqlite.org/c3ref/column_name.html) |
| [`sqlite3_bind_null`](https://www.sqlite.org/c3ref/bind_blob.html) | [`sqlite3_bind_blob`](https://www.sqlite.org/c3ref/bind_blob.html) | [`sqlite3_bind_text`](https://www.sqlite.org/c3ref/bind_blob.html) | [`sqlite3_value_free`](https://www.sqlite.org/c3ref/value_dup.html) | [`sqlite3_value_bytes`](https://www.sqlite.org/c3ref/value_blob.html) | [`sqlite3_value_text`](https://www.sqlite.org/c3ref/value_blob.html) |
| [`sqlite3_value_blob`](https://www.sqlite.org/c3ref/value_blob.html) | [`sqlite3_value_int`](https://www.sqlite.org/c3ref/value_blob.html) | [`sqlite3_value_int64`](https://www.sqlite.org/c3ref/value_blob.html) | [`sqlite3_value_double`](https://www.sqlite.org/c3ref/value_blob.html) | [`sqlite3_value_type`](https://www.sqlite.org/c3ref/value_blob.html) | [`sqlite3_value_dup`](https://www.sqlite.org/c3ref/value_dup.html) |
| [`sqlite3_bind_double`](https://www.sqlite.org/c3ref/bind_blob.html) | [`sqlite3_bind_int`](https://www.sqlite.org/c3ref/bind_blob.html) | [`sqlite3_bind_int64`](https://www.sqlite.org/c3ref/bind_blob.html) | [`sqlite3_create_collation_v2`](https://www.sqlite.org/c3ref/create_collation.html) | [`sqlite3_extended_errcode`](https://www.sqlite.org/c3ref/errcode.html) | [`sqlite3_finalize`](https://www.sqlite.org/c3ref/finalize.html) |
| [`sqlite3_step`](https://www.sqlite.org/c3ref/step.html)     | [`sqlite3_errmsg`](https://www.sqlite.org/c3ref/errcode.html) | [`sqlite3_db_handle`](https://www.sqlite.org/c3ref/db_handle.html) | [`sqlite3_reset`](https://www.sqlite.org/c3ref/reset.html)   | [`sqlite3_prepare_v3`](https://www.sqlite.org/c3ref/prepare.html) | [`sqlite3_context_db_handle`](https://www.sqlite.org/c3ref/context_db_handle.html) |
| [`sqlite3_user_data`](https://www.sqlite.org/c3ref/user_data.html) | [`sqlite3_aggregate_context`](https://www.sqlite.org/c3ref/aggregate_context.html) | [`sqlite3_result_error`](https://www.sqlite.org/c3ref/result_blob.html) |                                                              |                                                              |                                                              |

## Usage

```rust
use sqlite_wasm_rs::c as ffi;
use std::ffi::CString;

async fn open_db() -> anyhow::Result<()> {
    // Before using CAPI, you must initialize the database. 
    // Initializing the database is a one-time operation during 
    // the life of the program.
    ffi::init_sqlite().await?;
  
    let mut db = std::ptr::null_mut();
    let filename = CString::new("mydb").unwrap();
    // Persistent Storage is supported, use opfs vfs.
    // This support is only available when sqlite is loaded from a 
    // Worker thread, whether it's loaded in its own dedicated worker 
    // or in a worker together with client code. 
    //
    // See <https://sqlite.org/wasm/doc/trunk/persistence.md#opfs>
    let vfs = CString::new("opfs").unwrap();
    let ret = unsafe {
        ffi::sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
            // Using std::ptr::null() is a memory DB
            vfs.as_ptr(),
        )
    };
    assert_eq!(ffi::SQLITE_OK, ret);
}
```

## About TEST

This project was successfully used in [`diesel`](https://github.com/diesel-rs/diesel), and diesel's integration tests and unit tests all run successfully (except for a few tests that required `std::fs::*` ), see [`sqlitest.gif`](https://raw.githubusercontent.com/Spxg/Spxg/refs/heads/master/resources/sqlitest.gif).

## Related Project

* [`sqlite-wasm`](https://github.com/sqlite/sqlite-wasm): SQLite Wasm conveniently wrapped as an ES Module.
* [`sqlite-web-rs`](https://github.com/xmtp/sqlite-web-rs): A SQLite WebAssembly backend for Diesel.
* [`rusqlite`](https://github.com/rusqlite/rusqlite): Ergonomic bindings to SQLite for Rust.