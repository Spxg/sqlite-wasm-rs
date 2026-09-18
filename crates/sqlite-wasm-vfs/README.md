[![Crates.io](https://img.shields.io/crates/v/sqlite-wasm-vfs.svg)](https://crates.io/crates/sqlite-wasm-vfs)

Some experimental VFS implementations.

## Features

The OPFS SAH pool implementation is enabled by default:

| Feature | Module | Storage |
| --- | --- | --- |
| `sahpool` | `sqlite_wasm_vfs::sahpool` | Origin Private File System (OPFS), using a pool of `SyncAccessHandle`s |

To use the OPFS SAH pool:

```toml
[dependencies]
sqlite-wasm-vfs = "0.2"
```

Set `default-features = false` to disable the implementation and its `web-sys`
filesystem bindings. Enable `features = ["sahpool"]` to opt back in explicitly.
