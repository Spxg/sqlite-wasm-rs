[![Crates.io](https://img.shields.io/crates/v/sqlite-wasm-vec.svg)](https://crates.io/crates/sqlite-wasm-vec)

`wasm32-unknown-unknown` bindings to the [sqlite-vec](https://github.com/asg017/sqlite-vec) extension.

## Usage

```toml
[dependencies]
sqlite-wasm-vec = "0.1"
rusqlite = "0.38.0"
```

```rust
use sqlite_wasm_vec::sqlite3_vec_init;
use rusqlite::{ffi::sqlite3_auto_extension, Connection};

#[test]
fn vec_version() -> String {
    unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_vec_init as *const ())));
    }

    let conn = Connection::open_in_memory().unwrap();

    conn.query_row("select vec_version()", [], |x| x.get(0))
        .unwrap()
}
```
