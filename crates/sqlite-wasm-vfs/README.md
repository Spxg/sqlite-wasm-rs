# sqlite-wasm-vfs

SQLite VFS implementations for `wasm32-unknown-unknown`.

`sahpool` stores databases in OPFS using sync access handles.

```toml
[dependencies]
sqlite-wasm-rs = { version = "0.6", features = ["wasm-bindgen"] }
sqlite-wasm-vfs = { version = "0.3", features = ["sahpool"] }
```

Install it as SQLite's default VFS:

```rust
use sqlite_wasm_rs::WasmOsCallback;
use sqlite_wasm_vfs::sahpool::{install, OpfsSAHPoolCfg};

async fn install_vfs() {
    install::<WasmOsCallback>(&OpfsSAHPoolCfg::default(), true)
        .await
        .unwrap();
}
```

Requires a secure context and a dedicated worker. Concurrent connections to the same database and WAL are not supported.

[API documentation](https://docs.rs/sqlite-wasm-vfs)
