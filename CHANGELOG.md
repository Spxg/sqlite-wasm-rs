# `sqlite-wasm-rs` Change Log
--------------------------------------------------------------------------------

## Unreleased

### Added

* Added support for the SQLite `uuid` extension, enabled via the `uuid` feature flag.

### Fixed

### Changed

--------------------------------------------------------------------------------

## [0.5.2](https://github.com/Spxg/sqlite-wasm-rs/compare/0.5.1...0.5.2)

### Added

* Introduced the `rsqlite-vfs` crate.
  [#156](https://github.com/Spxg/sqlite-wasm-rs/pull/156)

### Fixed

### Changed

* Bump SQLite Version to 3.51.2 and SQLite3MC Version to 2.2.7.
  [#168](https://github.com/Spxg/sqlite-wasm-rs/pull/168)

--------------------------------------------------------------------------------

## [0.5.1](https://github.com/Spxg/sqlite-wasm-rs/compare/0.5.0...0.5.1)

### Added

### Fixed

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

### Fixed

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
