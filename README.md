# SQLite Wasm Rust

Wrap the official [`sqlite-wasm`](https://github.com/sqlite/sqlite-wasm), and expect to provide a usable C-like API.

Currently, this project is just a toy (and may be for a long time), and currently only the following APIs are implemented and tested:

* [`sqlite3_open`](https://www.sqlite.org/c3ref/open.html)
* [`sqlite3_open_v2`](https://www.sqlite.org/c3ref/open.html)
* [`sqlite3_exec`](https://www.sqlite.org/c3ref/exec.html)
* [`sqlite3_close_v2`](https://www.sqlite.org/c3ref/close.html)
* [`sqlite3_errmsg`](https://www.sqlite.org/c3ref/errcode.html)

## Installation

```bash
npm install
```

```bash
rollup -c
```

```bash
cargo install wasm-pack
```

## Test

```bash
wasm-pack test --chrome
```

## Related Project

* [`sqlite-wasm`](https://github.com/sqlite/sqlite-wasm)
* [`sqlite-web-rs`](https://github.com/xmtp/sqlite-web-rs)
* [`rusqlite`](https://github.com/rusqlite/rusqlite)
