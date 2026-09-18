[![Crates.io](https://img.shields.io/crates/v/sqlite-wasm-vec.svg)](https://crates.io/crates/sqlite-wasm-vec)

`wasm32-unknown-unknown` bindings to the [sqlite-vec](https://github.com/asg017/sqlite-vec) extension.

## Usage

```toml
[dependencies]
sqlite-wasm-vec = "0.1"
sqlite-wasm-rs = "0.6"
```

Register the extension before opening a database:

```rust
use sqlite_wasm_vec::sqlite3_vec_init;
use sqlite_wasm_rs::{sqlite3_auto_extension, SQLITE_OK};

unsafe {
    assert_eq!(
        sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_vec_init as *const ()))),
        SQLITE_OK
    );
}
```
