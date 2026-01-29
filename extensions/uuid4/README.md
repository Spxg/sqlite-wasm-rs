# sqlite-wasm-uuid4

`wasm32-unknown-unknown` bindings to the SQLite `uuid` extension (v4).

Exports `sqlite3_uuid4_init` for usage with `sqlite3_auto_extension`.

## Testing

This crate is configured to run tests in a NodeJS environment using `wasm-pack`.

```bash
wasm-pack test --node
```

