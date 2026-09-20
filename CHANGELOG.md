# `sqlite-wasm-rs` Change Log
--------------------------------------------------------------------------------

## Unreleased

Changes since 0.5.5, covering `sqlite-wasm-rs` 0.6.0, `rsqlite-vfs` 0.2.0
and `sqlite-wasm-vfs` 0.3.0.

### Breaking changes

* No features are enabled by default. Enable `wasm-bindgen` in `sqlite-wasm-rs`
  for the JavaScript host adapter, and `sahpool` in `sqlite-wasm-vfs` for OPFS.
  Without `wasm-bindgen`, applications must supply the C ABI host hooks.

* Remove `relaxed_idb` support. Its asynchronous persistence did not meet
  SQLite's synchronous durability requirements.

* Replace `sqlite_wasm_rs::utils` with `sqlite_wasm_rs::vfs`. Access `MemVfsUtil`
  and `MemVfsError` through `vfs::memvfs` instead of the crate root.

* Redesign `VfsStore` around owned per-open handles, associated `File`/`AppData`
  types and typed open options. `open_file` takes `OpenRequest` and returns
  `OpenedFile`; backends implement close, access, path resolution and deletion.

* Use `u64` for file offsets and lengths. `VfsFile::read` now takes `&mut self`
  and returns a byte count. Replace `flush` with `sync(SyncOptions)` and require
  lock delegates; deletion receives `sync_dir`.

* Make `OsCallback` instance-based through `SQLiteVfs::Os` and `os`. Clocks
  return a result and randomness reports the number of bytes filled.
  Backends own diagnostic storage and synchronization; `VfsError` uses typed
  SQLite/OS codes and borrowed or owned messages.

* Make raw VFS construction, lookup and registration unsafe. `register_vfs`
  returns `VfsRegistration` with explicit unsafe `unregister`; dropping the
  handle leaves the VFS registered. Raw file access no longer returns static references.

* Separate memory VFS installation from management: use fallible
  `memvfs::install(os, default_vfs)` and non-generic `MemVfsUtil::get()`.
  Installation, lookup and uninstallation require unsafe, same-thread access.

* Move management methods to `VfsFilesManager`: `remove`, `clear`, `contains`,
  `names`, `len` and `is_empty`, all returning `Result`. Memory VFS management
  uses `Infallible`; SAH queries reject paused, uninstalled, busy or
  recovery-required states.
  Move import/export methods to `transfer::DbTransfer`; import these traits to
  call their methods. Unchecked imports preserve all bytes and no longer take `clear_wal`.

* Make registration, memory VFS and transfer errors non-exhaustive.

* Rename SAH pool methods to `capacity`, `ensure_capacity`, `pause` and `resume`;
  capacity values use `usize`. Add unsafe `uninstall`; `install` now requires
  `OsCallback + Default + 'static`. Limit database names to 499 UTF-8 bytes for
  SAH pools and 1012 for memvfs, reserving space for journal suffixes.

* Remove `xOpenImpl`, `xCloseImpl`, `memvfs::MemFile`, public helper macros,
  `random_name`, `SQLITE3_HEADER`, `check_import_db` and `check_db_and_page_size`.
  Move `ImportDbError` to `transfer`.

### Added

* C ABI host adapters, with `host-js` and `host-c` examples for environments
  without wasm-bindgen.

* `SQLITE_WASM_RS_SOURCE_DIR` for custom SQLite or SQLite3MC amalgamations.
  Enable `bindgen` to generate bindings from the selected headers.

* Reusable chunked import/export through `DbTransfer`, implemented by memvfs
  and SAH pools. Transfers use `u64` lengths without requiring a full-image buffer;
  unfinished imports are discarded. Memory and backend limits still apply.

### Changed

* Update SQLite to 3.53.4, SQLite3MC to 2.5.1 and printf to 6.4.0.

* Default `SQLiteVfs::VERSION` to 2 and `SQLiteIoMethods::VERSION` to 1.
  Add typed delegates for size hints, sector size and device characteristics.

* Remove `hashbrown` from `rsqlite-vfs`, retaining `no_std` support, and replace
  Tokio with `futures-util` in `sqlite-wasm-vfs`.

* Use SQLite's own error messages in `code_to_str`. Expand the doc-hidden
  `test_suite` for custom VFS implementations and provide a native VFS example.

### Fixed

* Correct default VFS callback buffer handling, short reads, time conversion,
  diagnostics and unsupported operations. Preserve SQLite filename URI metadata
  and fix registration ownership, cleanup and reinstallation.

* Fix memory-file truncation, sparse writes and allocation failure handling.
  Preserve open-file identity after deletion/recreation and enforce exclusive creation.

* Fix SAH hot-journal recovery, persistent namespace updates and header validation.
  Recover resources after failures or cancellation, prevent overlapping directory
  ownership and preserve invalid files for recovery. Transfers reject open files
  and nonempty journal/WAL sidecars; the on-disk pool format is unchanged.

* Fix C allocation overflow/null handling, entropy buffer initialization and
  invalid local-time handling. Split Web Crypto requests at 64 KiB. Correct the
  extension symbol callback signature and refresh SQLite configuration constants.

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
