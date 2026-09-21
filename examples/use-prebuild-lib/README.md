# `Use prebuild libsqlite3.a`

This example shows how to link sqlite3 with prebuild libsqlite3.a

Do not enable the `sqlite-wasm-rs/bindgen` feature with this configuration.
Overriding the build script also skips binding generation, so this example uses
the checked-in bindings. The static library must match the selected SQLite or
SQLite3MC configuration.

## Usage

```sh
RUSTFLAGS="-L $(pwd)" CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner cargo test --target wasm32-unknown-unknown
```

1. Build `sqlite-wasm-rs` and find the library:

```sh
# change opt-level
CARGO_PROFILE_RELEASE_OPT_LEVEL="z" cargo build --target wasm32-unknown-unknown --release
# find library
fd -IH libwsqlite3.a
```

2. Copy library to your ld search path

3. Add config to `.cargo/config.toml`

```toml
[target.wasm32-unknown-unknown.wsqlite3]
rustc-link-lib = ["wsqlite3"]
```
