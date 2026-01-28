# Adding Static Extensions

This library supports statically linking SQLite extensions. This is necessary because WebAssembly environments typically do not support dynamic library loading (`dlopen`), so all extensions must be compiled and linked directly into the binary.

## Structure

We maintain a simple structure for single-file C extensions:

- `sqlite3/ext/`: Directory for C extension source files.

## Step-by-Step Guide

### 1. Add the Source File

Place your extension's C source code in `sqlite3/ext/`.

For example, for the `uuid` extension:

```bash
cp path/to/uuid.c sqlite3/ext/uuid.c
```

### 2. Define a Feature Flag

Add a new feature to `Cargo.toml` to allow users to opt-in to this extension.

```toml
[features]
# ...
uuid = []
```

### 3. Register in `build.rs`

Update `build.rs` to compile your extension when the feature is enabled. Looking for the `extensions` array in the `compile()` function:

```rust
    // Extensions
    // (feature_name, source_file)
    let extensions = [
        #[cfg(feature = "uuid")]
        ("uuid", "sqlite3/ext/uuid.c"),
        // Add your new extension here:
        #[cfg(feature = "your_feature")]
        ("your_feature", "sqlite3/ext/your_extension.c"),
    ];
```

The build script automatically handles:

- Compiling the C file.
- Adding `sqlite3/` to the include path (so `#include "sqlite3ext.h"` works).
- Linking it into the final `wsqlite3` library.

### 4. Expose Initialization in Rust

In `src/lib.rs`, allow users to register the extension.

First, declare the extension's initialization function. This name is defined in the C file (usually `sqlite3_EXTENSION_init`).

```rust
#[cfg(feature = "your_feature")]
extern "C" {
    pub fn sqlite3_your_extension_init(
        db: *mut sqlite3,
        pzErrMsg: *mut *mut core::ffi::c_char,
        pApi: *const sqlite3_api_routines,
    ) -> core::ffi::c_int;
}
```

Then, provide a safe helper to register it using `sqlite3_auto_extension`:

```rust
#[cfg(feature = "your_feature")]
pub fn register_your_extension() {
    unsafe {
        sqlite3_auto_extension(Some(core::mem::transmute(
            sqlite3_your_extension_init as *const (),
        )));
    }
}
```

### Usage

Users can now use your extension by enabling the feature and registering it at startup:

```toml
[dependencies]
sqlite-wasm-rs = { version = "...", features = ["uuid"] }
```

```rust
fn main() {
    sqlite_wasm_rs::register_uuid_extension();
    
    // Open database and use uuid functions...
}
```
