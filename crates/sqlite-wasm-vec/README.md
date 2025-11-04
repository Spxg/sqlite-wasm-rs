Provide sqlite-vec extension solution for `wasm32-unknown-unknown` target.

## Usage

```toml
[dependencies]
sqlite-wasm-vec = "0.1"
sqlite-wasm-rs = "0.4"
```

```rust
use std::ffi::CStr;
use sqlite_wasm_vec::sqlite3_vec_init;
use sqlite_wasm_rs::{
    sqlite3_auto_extension, sqlite3_close, sqlite3_column_count, sqlite3_column_text,
    sqlite3_column_type, sqlite3_finalize, sqlite3_open_v2, sqlite3_prepare_v3, sqlite3_step,
    SQLITE_OK, SQLITE_OPEN_CREATE, SQLITE_OPEN_READWRITE, SQLITE_ROW, SQLITE_TEXT,
};

fn vec_version() -> String {
    unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_vec_init as *const ())));
    }

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c":memory:".as_ptr().cast(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(ret, SQLITE_OK);

    let sql = c"select vec_version();";
    let mut stmt = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_prepare_v3(
            db,
            sql.as_ptr().cast(),
            -1,
            0,
            &mut stmt as *mut _,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(ret, SQLITE_OK);

    unsafe {
        assert_eq!(sqlite3_step(stmt), SQLITE_ROW);
        let count = sqlite3_column_count(stmt);
        assert_eq!(count, 1);
        let ty = sqlite3_column_type(stmt, 0);
        assert_eq!(ty, SQLITE_TEXT);
        let s = CStr::from_ptr(sqlite3_column_text(stmt, 0).cast())
            .to_str()
            .unwrap();
        assert!(s.starts_with('v'));
        sqlite3_finalize(stmt);
        sqlite3_close(db);
        s.to_string()
    }
}

```
