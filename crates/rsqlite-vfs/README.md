[![Crates.io](https://img.shields.io/crates/v/rsqlite-vfs.svg)](https://crates.io/crates/rsqlite-vfs)

# `rsqlite-vfs`

Traits and utilities for implementing SQLite VFS in Rust, including the platform-independent `memvfs`.

Safe memory VFS access requires `MemVfsContext::try_new()` with `threadsafe` or non-atomic Wasm. Threaded construction requires compile-option diagnostics, verifies real mutexes and active core locking, and rejects C `SQLITE_DEBUG` builds. Unchecked access is available for caller-enforced contracts.

Contexts and derived safe objects must retire before foreign SQLite shutdown or threading reconfiguration. Memory files can outlive VFS uninstallation.

See [implement-a-vfs](https://github.com/Spxg/sqlite-wasm-rs/blob/master/crates/rsqlite-vfs/examples/implement-a-vfs.rs) for a VFS implementation and [memvfs_context_workers](https://github.com/Spxg/sqlite-wasm-rs/blob/master/crates/rsqlite-vfs/examples/memvfs_context_workers.rs) for a native safe-access consumer.
