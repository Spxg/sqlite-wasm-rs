/* Makes SQLite's default mutex the atomics mutex from src/mutex.rs. */
struct sqlite3_mutex_methods;
const struct sqlite3_mutex_methods *rust_sqlite_wasm_mutex_methods(void);

/* The call `sqlite3DefaultMutex()` reaches Rust, while the no-op definition
 * `sqlite3DefaultMutex(void)` compiles under an unused name. */
#define sqlite3DefaultMutex(...) SQLITE_WASM_RS_DEFAULT_MUTEX_##__VA_ARGS__()
#define SQLITE_WASM_RS_DEFAULT_MUTEX_ rust_sqlite_wasm_mutex_methods
#define SQLITE_WASM_RS_DEFAULT_MUTEX_void sqlite3NoopDefaultMutex
