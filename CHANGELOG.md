# `sqlite-wasm-rs` Change Log
--------------------------------------------------------------------------------

## Unreleased

### Added

* C ABI host hooks for custom `wasm32-unknown-unknown` environments. The
  default `wasm-bindgen` feature preserves the JavaScript adapter; disabling it
  removes the core library's `wasm-bindgen` and `js-sys` dependencies. A custom
  host example runs SQLite with ordinary Wasm imports and no generated glue.

* Public, doc-hidden `test_suite` with reusable file and store conformance checks
  for custom VFS implementations.

* Default-enabled `sahpool` feature in `sqlite-wasm-vfs`, allowing the OPFS
  implementation and its filesystem bindings to be disabled.

### Changed

* **Breaking:** Replace `sqlite_wasm_rs::utils` with `sqlite_wasm_rs::vfs`,
  re-exporting the complete `rsqlite-vfs` API. Access `MemVfsError` and
  `MemVfsUtil` through `sqlite_wasm_rs::vfs::memvfs` instead of the crate root.

* Align `code_to_str` with `libsqlite3-sys` by using SQLite's own error messages.

* **Breaking:** Redesign VFS backend traits around owned per-open handles and
  typed options/errors. `VfsStore` owns the `File` and `AppData` types;
  `open_file` receives `OpenRequest` and returns `OpenedFile` with the actual
  access mode. Backends implement close/delete-on-close, access, path resolution,
  synchronization and locking; safe delegates also cover storage hints and
  device capabilities. `sync` replaces `flush`, and deletion receives `sync_dir`.

* **Breaking:** File offsets and sizes use `u64`, including on wasm32.
  `VfsFile::read` takes `&mut self` and returns a byte count; the default `xRead`
  handles zero-filling and short-read errors. Memory and backend limits still apply.

* **Breaking:** Platform services use instance-based `OsCallback` methods through
  `SQLiteVfs::Os` and `os`. Clocks are fallible, randomness reports bytes filled,
  and `random_name` rejects incomplete random input.

* **Breaking:** Backends own diagnostic storage and synchronization instead of
  relying on a built-in `RefCell`. `VfsError` uses validated `VfsErrorCode` values,
  optional `SystemErrorCode` diagnostics and borrowed or owned messages, and
  implements `Display` and `core::error::Error`.

* **Breaking:** Raw VFS construction, lookup and registration are explicitly
  unsafe. `register_vfs` rejects empty/conflicting names and returns an owned
  `VfsRegistration` with explicit unsafe unregistration; dropping it keeps the
  registration alive. Raw file/app-data access no longer exposes static references.

* **Breaking:** Separate memory VFS installation from management access:
  `memvfs::install(os, default_vfs)` is fallible, `MemVfsUtil` is non-generic,
  and `get` only acquires an installed instance. Lifecycle operations require
  unsafe, serialized same-thread access; management errors are typed.
  Align management names and return types, including SAH pool `capacity`,
  `ensure_capacity`, `pause` and `resume`.

* Remove the `hashbrown` dependency from `rsqlite-vfs` while retaining `no_std`
  support; reduce memory-file allocation overhead and handle bulk allocation
  failures without corrupting existing data.

### Fixed

* Handle null pointers and allocation-size overflow in the C allocation shim;
  initialize entropy output buffers safely and split Web Crypto requests to
  avoid unnecessary fallback for buffers larger than 64 KiB.

* Align default callbacks with SQLite's buffer, short-read, time, diagnostic and
  unsupported-operation contracts. Preserve URI metadata for database, journal
  and WAL opens, and keep OS error numbers separate from SQLite result codes.
  The no-atomics wasm sleep limitation is unchanged.

* Correct VFS pointer ownership and cleanup across registration failures,
  unregistration and reinstallation. Keep memory management handles valid after
  uninstall, reject foreign same-name registrations, and release file handles
  even when delete-on-close fails.

* Correct memory-file truncation and sparse-write behavior; validate imported
  filenames and SQLite headers. Preserve open memory-file identity after
  deletion/recreation, enforce exclusive creation, and prevent SAH pool removal
  or pausing while file handles remain open.

* Correct the SQLite extension symbol callback signature and update database
  configuration constants to match the bundled SQLite headers.

### Removed

* **Breaking:** Remove `xOpenImpl`, `xCloseImpl`, `memvfs::MemFile` and the public
  `bail!`, `check_result!`, `check_option!` and `unused!` helper macros.

* **Breaking:** Remove the `relaxed_idb` module, `relaxed-idb` feature and
  `indexed_db_futures` dependency. Its asynchronous persistence did not provide
  SQLite's synchronous durability guarantees; use `sahpool` for persistent storage.

--------------------------------------------------------------------------------

## [0.5.5](https://github.com/Spxg/sqlite-wasm-rs/compare/0.5.4...0.5.5)

* Minimal `cc` version.
  [#175](https://github.com/Spxg/sqlite-wasm-rs/pull/175)

--------------------------------------------------------------------------------

## [0.5.4](https://github.com/Spxg/sqlite-wasm-rs/compare/0.5.3...0.5.4)

### Changed

* Make crates compile on 1.81.0.
  [#172](https://github.com/Spxg/sqlite-wasm-rs/pull/172)

--------------------------------------------------------------------------------

## [0.5.3](https://github.com/Spxg/sqlite-wasm-rs/compare/0.5.2...0.5.3)

### Changed

* Bump SQLite Version to 3.53.0 and SQLite3MC Version to 2.3.3.
  [#171](https://github.com/Spxg/sqlite-wasm-rs/pull/171)

--------------------------------------------------------------------------------

## [0.5.2](https://github.com/Spxg/sqlite-wasm-rs/compare/0.5.1...0.5.2)

### Added

* Introduced the `rsqlite-vfs` crate.
  [#156](https://github.com/Spxg/sqlite-wasm-rs/pull/156)

### Changed

* Bump SQLite Version to 3.51.2 and SQLite3MC Version to 2.2.7.
  [#168](https://github.com/Spxg/sqlite-wasm-rs/pull/168)

--------------------------------------------------------------------------------

## [0.5.1](https://github.com/Spxg/sqlite-wasm-rs/compare/0.5.0...0.5.1)

### Changed

* Removed emcc requirement.
  [#157](https://github.com/Spxg/sqlite-wasm-rs/pull/157)

--------------------------------------------------------------------------------

## [0.5.0](https://github.com/Spxg/sqlite-wasm-rs/compare/0.4.8...0.5.0)

### Added

* New crate `sqlite-wasm-vfs`: some experimental VFS implementations.
  [#146](https://github.com/Spxg/sqlite-wasm-rs/pull/146)

* Added `no_std` support for `sqlite-wasm-rs`.
  [#149](https://github.com/Spxg/sqlite-wasm-rs/pull/149)

### Changed

* Moved `relaxed-idb` vfs to `sqlite-wasm-vfs`.
  [#146](https://github.com/Spxg/sqlite-wasm-rs/pull/146)

* Removed `relaxed-idb`, `precompiled`, `custom-libc`, `bundled` features.
  [#146](https://github.com/Spxg/sqlite-wasm-rs/pull/146)

* Renamed `buildtime-bindgen` feature to `bindgen`.
  [#146](https://github.com/Spxg/sqlite-wasm-rs/pull/146)

* Bump MSRV to 1.82.0.
  [#148](https://github.com/Spxg/sqlite-wasm-rs/pull/148)

* Moved `opfs-sahpool` vfs to `sqlite-wasm-vfs`.
  [#149](https://github.com/Spxg/sqlite-wasm-rs/pull/149)

--------------------------------------------------------------------------------

## [0.4.8](https://github.com/Spxg/sqlite-wasm-rs/compare/0.4.7...0.4.8)

### Changed

* Bump SQLite Version to 3.51.1 and SQLite3MC Version to 2.2.6
  [#124](https://github.com/Spxg/sqlite-wasm-rs/pull/145)

--------------------------------------------------------------------------------

## [0.4.7](https://github.com/Spxg/sqlite-wasm-rs/compare/0.4.6...0.4.7)

### Added

* Added comment about incorrect implementation of `Send` + `Sync`.
  [#125](https://github.com/Spxg/sqlite-wasm-rs/pull/125)

* Added `sqlite-vec` extension support.
  [#130](https://github.com/Spxg/sqlite-wasm-rs/pull/130)

### Changed

* Reduced the use of `JsValue` in opfs-sahpool VFS.
  [#124](https://github.com/Spxg/sqlite-wasm-rs/pull/124)

--------------------------------------------------------------------------------

## [0.4.6](https://github.com/Spxg/sqlite-wasm-rs/compare/0.4.5...0.4.6)

### Added

* Added `sqlite3_os_end` C interface.
  [#117](https://github.com/Spxg/sqlite-wasm-rs/pull/117)

* Added `pause_vfs`, `unpause_vfs`, and `is_paused` to `opfs-sahpool` VFS.
  [#121](https://github.com/Spxg/sqlite-wasm-rs/pull/121)

--------------------------------------------------------------------------------

## [0.4.5](https://github.com/Spxg/sqlite-wasm-rs/compare/0.4.4...0.4.5)

### Changed

* Moved VFS documentation to source files.
  [#112](https://github.com/Spxg/sqlite-wasm-rs/pull/112)

* Removed unnecessary `thread_local` used.
  [#113](https://github.com/Spxg/sqlite-wasm-rs/pull/113)
