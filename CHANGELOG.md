# `sqlite-wasm-rs` Change Log
--------------------------------------------------------------------------------

## Unreleased

### Added

### Fixed

### Changed

--------------------------------------------------------------------------------

## [0.4.8](https://github.com/Spxg/sqlite-wasm-rs/compare/0.4.7...0.4.8)

### Changed

* Bump SQLite Version to 3.51.1 and SQLite3MC Version to 2.2.6
  [#124](https://github.com/Spxg/sqlite-wasm-rs/pull/144)

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
