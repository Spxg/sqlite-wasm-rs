## VFS

We have written several tests to make it easier for you to know how to use it.
Please go to [`here`](https://github.com/Spxg/sqlite-wasm-rs/tree/master/sqlite-wasm-rs/tests) to check it out.

### MemoryVFS

Data is stored in memory, this is the default vfs.

### SyncAccessHandlePoolVFS

Persistent vfs, ported from sqlite-wasm, see [`opfs-sahpool`](https://sqlite.org/wasm/doc/trunk/persistence.md#vfs-opfs-sahpool) for details.

Install the [`opfs-explorer`](https://chromewebstore.google.com/detail/opfs-explorer/acndjpgkpaclldomagafnognkcgjignd) plugin to browse files.

### IndexedPoolVFS

Inspired by wa-sqlite's [`IDBMirrorVFS`](https://github.com/rhashimoto/wa-sqlite/blob/master/src/examples/IDBMirrorVFS.js), this is an VFS used in a synchronization context.

The file system is Relaxed durability, for sqlite it is `pragma synchronous=off;`.

The page_size can be set via `pragma page_size=SIZE` before creating a table in db. Once the table is created, it cannot be changed.

## VFS Comparison

||MemoryVFS|SyncAccessHandlePoolVFS|IndexedPoolVFS|
|-|-|-|-|
|Storage|RAM|OPFS|IndexedDB|
|Contexts|All|Worker|All|
|Multiple connections|:x:|:x:|:x:|
|Full durability|✅|✅|:x:|
|Relaxed durability|:x:|:x:|✅|
|Multi-database transactions|✅|✅|✅|
|No COOP/COEP requirements|✅|✅|✅|
