# `sqlite-wasm-rs` Change Log
--------------------------------------------------------------------------------

## Unreleased

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
