## VFS

We have written several tests to make it easier for you to know how to use it.

Go to [`here`](https://github.com/Spxg/sqlite-wasm-rs/tree/master/sqlite-wasm-rs/tests/full/vfs) to check it out.

### MemoryVFS

```rust
use sqlite_wasm_rs as ffi;

fn open_db() {
    // open with memory vfs
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        ffi::sqlite3_open_v2(
            c"mem.db".as_ptr().cast(),
            &mut db as *mut _,
            ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
            std::ptr::null()
        )
    };
    assert_eq!(ffi::SQLITE_OK, ret);
}
```

Data is stored in memory, this is the default vfs, and reading and writing are very fast, after all, in memory.

Refresh the page and data will be lost, and you also need to pay attention to the memory size limit of the browser page.

### SyncAccessHandlePoolVFS

```rust
use sqlite_wasm_rs::{
    self as ffi,
    sahpool_vfs::{install as install_opfs_sahpool, OpfsSAHPoolCfg},
};

async fn open_db() {
    // install opfs-sahpool persistent vfs and set as default vfs
    install_opfs_sahpool(&OpfsSAHPoolCfg::default(), true)
        .await
        .unwrap();

    // open with opfs-sahpool vfs
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        ffi::sqlite3_open_v2(
            c"opfs-sahpool.db".as_ptr().cast(),
            &mut db as *mut _,
            ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
            std::ptr::null()
        )
    };
    assert_eq!(ffi::SQLITE_OK, ret);
}
```

Ported from sqlite-wasm, see [`opfs-sahpool`](https://sqlite.org/wasm/doc/trunk/persistence.md#vfs-opfs-sahpool) for details. 

The VFS is based on [`FileSystemSyncAccessHandle`](https://developer.mozilla.org/en-US/docs/Web/API/FileSystemSyncAccessHandle) read and write, and you can install the [`opfs-explorer`](https://chromewebstore.google.com/detail/opfs-explorer/acndjpgkpaclldomagafnognkcgjignd) plugin to browse files.

### RelaxedIdbVFS

**The `relaxed-idb` feature is required, and it is not recommended to use in a production environment.**

```rust
use sqlite_wasm_rs::{
    self as ffi,
    relaxed_idb_vfs::{install as install_idb_vfs, RelaxedIdbCfg},
};

async fn open_db() {
    // install relaxed-idb persistent vfs and set as default vfs
    install_idb_vfs(&RelaxedIdbCfg::default(), true)
        .await
        .unwrap();

    // open with relaxed-idb vfs
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        ffi::sqlite3_open_v2(
            c"relaxed-idb.db".as_ptr().cast(),
            &mut db as *mut _,
            ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
            std::ptr::null()
        )
    };
    assert_eq!(ffi::SQLITE_OK, ret);
}
```

Inspired by wa-sqlite's [`IDBMirrorVFS`](https://github.com/rhashimoto/wa-sqlite/blob/master/src/examples/IDBMirrorVFS.js), this is an VFS used in a synchronization context.

The principle is to preload the db into memory before xOpen, and then all operations are synchronous. When sqlite calls sync, it asynchronously writes the changed blocks to the indexed db through the indexed transaction. The difference from IDBMirrorVFS is that `RelaxedIdbVFS` does only support pragma `synchronous=off`.

As for performance, since both reading and writing are done in memory, the performance is very good. However, we need to pay attention to the performance of preload the database, because the database is divided into multiple blocks and stored in the indexed db, and it takes some time to read all of them into memory. After my test, when page_size is 64k, the loading speed is the fastest.

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

