This PR introduces **Pure Rust** implementations of SQLite UUID extensions, specifically **UUIDv4 (Random)** and **UUIDv7 (Time-ordered)**. These extensions are implemented as separate crates within the workspace (`extensions/uuid4`, `extensions/uuid7`) using `sqlite-wasm-rs` bindings and the native [`uuid`](https://crates.io/crates/uuid) crate.

This allows users to generate and manipulate [RFC-9562 UUIDs](https://www.rfc-editor.org/rfc/rfc9562) directly within SQL queries in a WASM environment.

## Details

### Architecture
- **Rust Implementation**: The extensions are written in Rust using `sqlite-wasm-rs` bindings to interface with the SQLite API.
- **Modular Design**: Extensions reside in the `extensions/` directory as independent crates.
- **Dependencies**: Leverages the popular `uuid` crate for generation and parsing logic.

### Extensions

#### UUIDv4 (Random)
Based on the standard SQLite [`uuid.c`](https://sqlite.org/src/file/ext/misc/uuid.c) API but implemented in Rust.
- `uuid()`: Generates a random 36-char string.
- `uuid_str(X)`: Standardizes input (Blob/Text) to 36-char string.
- `uuid_blob(X)`: Converts input to 16-byte BLOB.
- `uuid_blob()`: **(Added)** Generates a new random 16-byte BLOB directly.

#### UUIDv7 (Time-Ordered)
A new extension for timestamp-based UUIDs, providing better index locality for direct SQLite use.
- `uuid7()`: Generates a time-ordered 36-char string.
- `uuid7_blob(X)`: Converts input to 16-byte BLOB.
- `uuid7_blob()`: Generates a new time-ordered 16-byte BLOB directly.

### Features
- **WASM Compatible**: Fully compatible with `wasm32-unknown-unknown` target.
- **Testing**: Comprehensive headless browser testing via `wasm-pack test` verifying:
    - Generation formats (Text vs Blob).
    - Usage as `DEFAULT` column values in schemas.
    - Sorting/Monotonicity for UUIDv7.
    - Round-trip conversions between String and BLOB formats.

## Testing

Tests are located in each extension's `src/lib.rs` and run via `wasm-pack test --headless --firefox`.
- **Validation**: Verified correct UUID version bits (v4 vs v7).
- **Parity**: Ensured `uuid7_blob` matches the flexible generation/conversion API of `uuid4`'s `uuid_blob`.

## Resources
- [RFC 9562 - Universally Unique IDentifiers (UUID)](https://www.rfc-editor.org/rfc/rfc9562)
- [uuid Crate (Rust)](https://crates.io/crates/uuid)
- [SQLite uuid.c Source](https://sqlite.org/src/file/ext/misc/uuid.c)