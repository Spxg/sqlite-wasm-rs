## VFS

We have written several tests to make it easier for you to know how to use it.
Go to [`here`](https://github.com/Spxg/sqlite-wasm-rs/tree/master/sqlite-wasm-rs/tests/allin/vfs) to check it out.

### MemoryVFS

Data is stored in memory, this is the default vfs. Reading and writing are very fast, after all, in memory.

Refresh the page and data will be lost. You also need to pay attention to the memory size limit of the browser page.

### SyncAccessHandlePoolVFS

Ported from sqlite-wasm, see [`opfs-sahpool`](https://sqlite.org/wasm/doc/trunk/persistence.md#vfs-opfs-sahpool) for details. Install the [`opfs-explorer`](https://chromewebstore.google.com/detail/opfs-explorer/acndjpgkpaclldomagafnognkcgjignd) plugin to browse files.

The VFS is based on [`FileSystemSyncAccessHandle`](https://developer.mozilla.org/en-US/docs/Web/API/FileSystemSyncAccessHandle) read and write. The Dedicated Worker is required.

### RelaxedIdbVFS

`relaxed-idb` feature is required.

Inspired by wa-sqlite's [`IDBMirrorVFS`](https://github.com/rhashimoto/wa-sqlite/blob/master/src/examples/IDBMirrorVFS.js), this is an VFS used in a synchronization context.

The principle is to preload the db into memory before xOpen, and then all operations are synchronous. When sqlite calls sync, it asynchronously writes the changed blocks to the indexed db through the indexed transaction. The difference from IDBMirrorVFS is that `RelaxedIdbVFS` does only support pragma `synchronous=off`, **therefore, it is not recommended to use in a production environment.**

As for performance, since both reading and writing are done in memory, the performance is very good. However, we need to pay attention to the performance of preload the database, because the database is divided into multiple blocks and stored in the indexed db, and it takes some time to read all of them into memory. After my test, when page_size is 64k, the loading speed is the fastest.

The db page_size can be set via `pragma page_size=SIZE;` before creating a table in db. Once the table is created, it cannot be changed.

As with MemoryVFS, you also need to pay attention to the memory size limit of the browser page.

It is particularly important to note that using it on multiple pages may cause DB corruption. It is recommended to use it in SharedWorker.

## VFS Comparison

||MemoryVFS|SyncAccessHandlePoolVFS|RelaxedIdbVFS|
|-|-|-|-|
|Storage|RAM|OPFS|IndexedDB|
|Contexts|All|Dedicated Worker|All|
|Multiple connections|:x:|:x:|:x:|
|Full durability|✅|✅|:x:|
|Relaxed durability|:x:|:x:|✅|
|Multi-database transactions|✅|✅|✅|
|No COOP/COEP requirements|✅|✅|✅|

## How to implement a VFS

Here is an example showing how to use `sqlite-wasm-rs` to implement a simple in-memory VFS, see [`implement-a-vfs`](https://github.com/Spxg/sqlite-wasm-rs/tree/master/examples/implement-a-vfs) example.

