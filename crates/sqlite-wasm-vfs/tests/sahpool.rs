#![cfg(feature = "sahpool")]

use sqlite_wasm_rs as ffi;
use sqlite_wasm_vfs::sahpool::{install, OpfsSAHError, OpfsSAHPoolCfg, OpfsSAHPoolCfgBuilder};
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);

fn config(name: &str) -> OpfsSAHPoolCfg {
    OpfsSAHPoolCfgBuilder::new()
        .vfs_name(name)
        .directory(name)
        .initial_capacity(3)
        .clear_on_init(true)
        .build()
}

// Faults are scoped to one test operation and restored by Drop on normal exit.
// Wasm panics abort, so restoration is not guaranteed after a failed assertion.
// No production-only testing hooks.
#[wasm_bindgen(inline_js = "
let saved, mode, fired, flushes, closes, started, release, acquired;

export function fault(value) {
    mode = value;
    fired = false;
    flushes = 0;
    closes = 0;

    const p = FileSystemSyncAccessHandle.prototype;
    const f = FileSystemFileHandle.prototype;
    const d = FileSystemDirectoryHandle.prototype;
    saved = {
        read: p.read,
        write: p.write,
        flush: p.flush,
        close: p.close,
        size: p.getSize,
        truncate: p.truncate,
        acquire: f.createSyncAccessHandle,
        remove: d.removeEntry
    };
    acquired = new Promise(resolve => started = resolve);

    p.write = function(bytes, options) {
        if (mode === 'all-writes' || (!fired && mode === 'body-write' && options.at >= 4096)) {
            fired = true;
            throw new DOMException('injected quota failure', 'QuotaExceededError');
        }

        if (!fired && ((mode === 'header-short' && options.at === 0) || (mode === 'wal-short' && options.at === 4114))) {
            fired = true;
            return bytes.byteLength - 1;
        }

        return saved.write.call(this, bytes, options);
    };

    p.read = function(bytes, options) {
        if (!fired && mode === 'header-read' && options.at === 0) {
            fired = true;
            return 0;
        }

        return saved.read.call(this, bytes, options);
    };

    p.flush = function() {
        ++flushes;
        let fail = mode === 'flush' || mode === 'initialize-flush';
        if (mode === 'main-flush') {
            const header = new Uint8Array(512);
            saved.read.call(this, header, {at: 0});
            const name = new TextDecoder().decode(header.subarray(0, header.indexOf(0)));
            fail = name === 'main.db';
        }

        if (!fired && fail) {
            fired = true;
            throw new DOMException('injected flush failure', 'UnknownError');
        }

        return saved.flush.call(this);
    };

    p.close = function() {
        ++closes;
        return saved.close.call(this);
    };

    p.getSize = function() {
        return mode === 'large-size' ? 4096 + 2147483648 : saved.size.call(this);
    };

    p.truncate = function(size) {
        if (!fired && (mode === 'initialize-truncate' || mode === 'initialize-cleanup')
            && saved.size.call(this) === 0) {
            fired = true;
            throw new DOMException('injected allocation failure', 'QuotaExceededError');
        }

        return saved.truncate.call(this, size);
    };

    d.removeEntry = async function(...args) {
        if ((!fired && mode === 'remove') || mode === 'initialize-cleanup') {
            fired = true;
            throw new DOMException('injected removal failure', 'UnknownError');
        }

        return saved.remove.apply(this, args);
    };

    f.createSyncAccessHandle = async function(...args) {
        const handle = await saved.acquire.apply(this, args);
        if (mode === 'acquire') {
            const wait = new Promise(resolve => release = resolve);
            started();
            await wait;
        }

        return handle;
    };
}

export function restore() {
    const p = FileSystemSyncAccessHandle.prototype;
    Object.assign(p, {
        read: saved.read,
        write: saved.write,
        flush: saved.flush,
        close: saved.close,
        getSize: saved.size,
        truncate: saved.truncate
    });
    FileSystemFileHandle.prototype.createSyncAccessHandle = saved.acquire;
    FileSystemDirectoryHandle.prototype.removeEntry = saved.remove;
}

export function flushCount() {
    return flushes;
}

export function closeCount() {
    return closes;
}

export function waitAcquired() {
    return acquired;
}

export async function finishAcquire() {
    release();
    await new Promise(resolve => setTimeout(resolve, 0));
}

export async function canAcquireLease(directory) {
    const root = await navigator.storage.getDirectory();
    const file = await (await root.getDirectoryHandle(directory)).getFileHandle('.lock');

    try {
        const handle = await file.createSyncAccessHandle();
        handle.close();
        return true;
    } catch {
        return false;
    }
}

export async function corrupt(directory, kind) {
    const root = await navigator.storage.getDirectory();
    const dir = await (await root.getDirectoryHandle(directory)).getDirectoryHandle('.opaque');
    const handles = [];

    if (kind === 'incomplete') {
        const file = await dir.getFileHandle('incomplete-slot', {create: true});
        const handle = await file.createSyncAccessHandle();
        try {
            handle.truncate(516);
            handle.flush();
        } finally {
            handle.close();
        }
        return;
    }

    try {
        for await (const [, file] of dir.entries()) handles.push(await file.createSyncAccessHandle());

        const headers = handles.map(handle => {
            const bytes = new Uint8Array(516);
            handle.read(bytes, {at: 0});
            return bytes;
        });
        const i = headers.findIndex(bytes => bytes[0] !== 0);

        if (kind === 'short') handles[i].truncate(100);
        if (kind === 'empty') handles[i].truncate(0);
        if (kind === 'utf8') handles[i].write(new Uint8Array([255]), {at: 0});
        if (kind === 'flags') handles[i].write(new Uint8Array(4), {at: 512});
        if (kind === 'conflicting-flags') {
            handles[i].write(new Uint8Array([0, 0, 3, 0]), {at: 512});
        }
        if (kind === 'duplicate') {
            const j = headers.findIndex(bytes => bytes[0] === 0);
            handles[j].write(headers[i], {at: 0});
        }

        for (const handle of handles) handle.flush();
    } finally {
        for (const handle of handles) handle.close();
    }
}
")]
extern "C" {
    fn fault(mode: &str);
    fn restore();

    #[wasm_bindgen(js_name = flushCount)]
    fn flush_count() -> u32;

    #[wasm_bindgen(js_name = closeCount)]
    fn close_count() -> u32;

    #[wasm_bindgen(js_name = waitAcquired)]
    async fn wait_acquired();

    #[wasm_bindgen(js_name = finishAcquire)]
    async fn finish_acquire();

    #[wasm_bindgen(js_name = canAcquireLease)]
    async fn can_acquire_lease(directory: &str) -> JsValue;

    async fn corrupt(directory: &str, kind: &str);
}

struct Fault;

impl Fault {
    fn new(mode: &str) -> Self {
        fault(mode);
        Self
    }
}

impl Drop for Fault {
    fn drop(&mut self) {
        restore();
    }
}

#[wasm_bindgen_test]
async fn imports_are_flushed_and_failures_do_not_publish_files() {
    let cfg = config("test-import-failures");
    let pool = install::<ffi::WasmOsCallback>(&cfg, false).await.unwrap();

    for name in ["", "nul\0suffix", &"x".repeat(512)] {
        assert!(matches!(
            pool.import_db_unchecked(name, b"data"),
            Err(OpfsSAHError::InvalidFilename(_))
        ));
        assert_eq!(pool.count(), 0);
        assert_eq!(pool.capacity(), 3);
    }

    let mut db = [0; 512];
    db[..16].copy_from_slice(b"SQLite format 3\0");
    db[16..18].copy_from_slice(&512u16.to_be_bytes());

    for mode in ["body-write", "header-short", "wal-short", "flush"] {
        let guard = Fault::new(mode);
        let error = pool.import_db("failed.db", &db).unwrap_err();
        if mode == "body-write" {
            assert!(error.to_string().contains("QuotaExceededError"));
            assert!(error.to_string().contains("injected quota failure"));
        }

        drop(guard);
        assert!(!pool.exists("failed.db"));
        assert_eq!(pool.capacity(), 3);
        pool.import_db("retry.db", &db).unwrap();
        pool.delete_db("retry.db").unwrap();
    }

    let guard = Fault::new("observe");
    pool.import_db("ok.db", &db).unwrap();
    let imported = flush_count();
    assert!(imported > 0);
    pool.delete_db("ok.db").unwrap();
    assert!(flush_count() > imported);
    drop(guard);

    let guard = Fault::new("all-writes");
    assert!(matches!(
        pool.import_db("failed.db", &db),
        Err(OpfsSAHError::Cleanup { .. })
    ));
    drop(guard);
    assert_eq!(pool.capacity(), 3);
    assert!(!pool.exists("failed.db"));
    assert!(matches!(
        pool.import_db("blocked.db", &db),
        Err(OpfsSAHError::NeedsRecovery)
    ));

    pool.clear_all().await.unwrap();
    pool.import_db("recovered.db", &db).unwrap();

    let guard = Fault::new("large-size");
    assert!(matches!(
        pool.export_db("recovered.db"),
        Err(OpfsSAHError::FileTooLarge)
    ));
    drop(guard);

    unsafe {
        pool.uninstall().unwrap();
    }
}

#[wasm_bindgen_test]
async fn lifecycle_reuses_paused_pool_and_reclaims_registration() {
    let cfg = config("test-pool-lifecycle");
    let pool = install::<ffi::WasmOsCallback>(&cfg, false).await.unwrap();
    pool.import_db_unchecked("keep.db", b"keep").unwrap();

    let original_default = unsafe { ffi::sqlite3_vfs_find(std::ptr::null()) };
    install::<ffi::WasmOsCallback>(&cfg, true).await.unwrap();
    assert_eq!(unsafe { ffi::sqlite3_vfs_find(std::ptr::null()) }, unsafe {
        ffi::sqlite3_vfs_find(c"test-pool-lifecycle".as_ptr())
    });

    pool.pause().unwrap();
    assert!(matches!(
        pool.add_capacity(1).await,
        Err(OpfsSAHError::Paused)
    ));
    assert!(matches!(pool.clear_all().await, Err(OpfsSAHError::Paused)));

    let reused = install::<ffi::WasmOsCallback>(&cfg, false).await.unwrap();
    assert!(reused.is_paused());

    reused.resume().await.unwrap();
    assert!(!pool.is_paused());
    assert_eq!(pool.export_db("keep.db").unwrap(), b"keep");

    let mut wrong = config("test-pool-lifecycle");
    wrong.directory = "another-directory".into();
    assert!(matches!(
        install::<ffi::WasmOsCallback>(&wrong, false).await,
        Err(OpfsSAHError::ConfigurationMismatch)
    ));

    unsafe {
        pool.uninstall().unwrap();
    }
    assert!(reused.is_uninstalled());
    assert!(matches!(
        reused.resume().await,
        Err(OpfsSAHError::Uninstalled)
    ));

    let mut reopen = cfg;
    reopen.clear_on_init = false;
    let replacement = install::<ffi::WasmOsCallback>(&reopen, false)
        .await
        .unwrap();
    assert_eq!(replacement.export_db("keep.db").unwrap(), b"keep");

    unsafe {
        replacement.uninstall().unwrap();
        ffi::sqlite3_vfs_register(original_default, 1);
    }
}

#[wasm_bindgen_test]
async fn invalid_configuration_does_not_touch_storage() {
    let guard = Fault::new("observe");
    let mut cfg = config("test-invalid-config");
    cfg.vfs_name.clear();
    assert!(matches!(
        install::<ffi::WasmOsCallback>(&cfg, false).await,
        Err(OpfsSAHError::Vfs(_))
    ));
    assert_eq!(flush_count(), 0);
    assert_eq!(close_count(), 0);
    drop(guard);
}

#[wasm_bindgen_test]
async fn damaged_headers_fail_without_retaining_handles() {
    for kind in [
        "short",
        "utf8",
        "duplicate",
        "header-read",
        "flags",
        "conflicting-flags",
    ] {
        let cfg = config(&format!("test-header-{kind}"));
        let pool = install::<ffi::WasmOsCallback>(&cfg, false).await.unwrap();
        pool.import_db_unchecked("keep.db", b"keep").unwrap();
        pool.pause().unwrap();

        let guard = if kind == "header-read" {
            Some(Fault::new(kind))
        } else {
            corrupt(&cfg.directory, kind).await;
            None
        };
        assert!(matches!(
            pool.resume().await,
            Err(OpfsSAHError::InvalidHeader { .. } | OpfsSAHError::DuplicateFilename(_))
        ));
        assert!(pool.is_paused());
        assert_eq!(pool.capacity(), 0);
        drop(guard);

        unsafe {
            pool.uninstall().unwrap();
        }

        // Explicit destructive reinitialization can acquire every handle again.
        let clean = install::<ffi::WasmOsCallback>(&cfg, false).await.unwrap();
        assert_eq!(clean.count(), 0);

        unsafe {
            clean.uninstall().unwrap();
        }
    }

    let cfg = config("test-empty-header");
    let pool = install::<ffi::WasmOsCallback>(&cfg, false).await.unwrap();
    pool.import_db_unchecked("stale.db", b"data").unwrap();

    pool.pause().unwrap();

    corrupt(&cfg.directory, "empty").await;

    pool.resume().await.unwrap();
    assert!(!pool.exists("stale.db"));
    assert_eq!(pool.capacity(), 3);

    unsafe {
        pool.uninstall().unwrap();
    }
}

#[wasm_bindgen_test]
async fn empty_pool_still_owns_its_directory() {
    let mut cfg = config("test-empty-pool-lease");
    cfg.initial_capacity = 0;
    let pool = install::<ffi::WasmOsCallback>(&cfg, false).await.unwrap();
    assert_eq!(pool.capacity(), 0);
    assert_eq!(
        can_acquire_lease(&cfg.directory).await.as_bool(),
        Some(false)
    );

    pool.pause().unwrap();
    assert_eq!(
        can_acquire_lease(&cfg.directory).await.as_bool(),
        Some(true)
    );

    pool.resume().await.unwrap();
    assert_eq!(
        can_acquire_lease(&cfg.directory).await.as_bool(),
        Some(false)
    );

    unsafe {
        pool.uninstall().unwrap();
    }
    assert_eq!(
        can_acquire_lease(&cfg.directory).await.as_bool(),
        Some(true)
    );
}

#[wasm_bindgen_test]
async fn management_cancellation_and_partial_removal_release_handles() {
    let cfg = config("test-management-cancellation");
    let pool = install::<ffi::WasmOsCallback>(&cfg, false).await.unwrap();

    let guard = Fault::new("acquire");
    let pending = match futures_util::future::select(
        Box::pin(pool.add_capacity(1)),
        Box::pin(wait_acquired()),
    )
    .await
    {
        futures_util::future::Either::Right(((), pending)) => pending,
        _ => panic!("acquisition should be held by the injected promise"),
    };
    assert!(matches!(pool.pause(), Err(OpfsSAHError::Busy)));
    assert!(matches!(pool.clear_all().await, Err(OpfsSAHError::Busy)));

    drop(pending);

    pool.pause().unwrap();
    let closed = close_count();
    finish_acquire().await;
    assert_eq!(close_count(), closed + 1);
    drop(guard);

    pool.resume().await.unwrap();

    let capacity = pool.capacity();

    let guard = Fault::new("remove");
    assert!(pool.reduce_capacity(3).await.is_err());
    assert_eq!(close_count(), 1);
    assert_eq!(pool.capacity(), capacity - 1);
    drop(guard);

    pool.pause().unwrap();

    pool.resume().await.unwrap();
    assert_eq!(pool.capacity(), capacity);

    unsafe {
        pool.uninstall().unwrap();
    }
}

#[wasm_bindgen_test]
async fn incomplete_slots_do_not_block_existing_databases() {
    let cfg = config("test-incomplete-slots");
    let pool = install::<ffi::WasmOsCallback>(&cfg, false).await.unwrap();
    pool.import_db_unchecked("keep.db", b"keep").unwrap();

    for mode in [
        "initialize-truncate",
        "initialize-flush",
        "initialize-cleanup",
    ] {
        let guard = Fault::new(mode);
        let error = pool.add_capacity(1).await.unwrap_err();
        drop(guard);

        if mode == "initialize-cleanup" {
            assert!(matches!(error, OpfsSAHError::Cleanup { .. }));
        } else {
            assert!(matches!(error, OpfsSAHError::Opfs { .. }));
        }

        assert_eq!(pool.capacity(), 3);
        assert_eq!(pool.export_db("keep.db").unwrap(), b"keep");
        pool.pause().unwrap();
        pool.resume().await.unwrap();

        let expected = if mode == "initialize-cleanup" { 4 } else { 3 };
        assert_eq!(pool.capacity(), expected);
        assert_eq!(pool.export_db("keep.db").unwrap(), b"keep");
    }

    pool.pause().unwrap();
    corrupt(&cfg.directory, "incomplete").await;
    pool.resume().await.unwrap();
    assert_eq!(pool.capacity(), 5);
    assert_eq!(pool.export_db("keep.db").unwrap(), b"keep");

    unsafe {
        pool.uninstall().unwrap();
    }
}

struct Db(*mut ffi::sqlite3);

impl Db {
    fn open(name: &str, vfs: &str) -> Result<Self, i32> {
        let name = std::ffi::CString::new(name).unwrap();
        let vfs = std::ffi::CString::new(vfs).unwrap();
        let mut db = std::ptr::null_mut();
        let code = unsafe {
            ffi::sqlite3_open_v2(
                name.as_ptr(),
                &mut db,
                ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
                vfs.as_ptr(),
            )
        };
        let db = Self(db);

        if code == ffi::SQLITE_OK {
            Ok(db)
        } else {
            Err(code)
        }
    }

    fn exec(&self, sql: &std::ffi::CStr) -> i32 {
        unsafe {
            ffi::sqlite3_exec(
                self.0,
                sql.as_ptr(),
                None,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        }
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        assert_eq!(unsafe { ffi::sqlite3_close(self.0) }, ffi::SQLITE_OK);
    }
}

#[wasm_bindgen_test]
async fn multi_database_rollback_reclaims_super_journals() {
    let mut cfg = config("test-multidb-rollback");
    cfg.initial_capacity = 6;
    let pool = install::<ffi::WasmOsCallback>(&cfg, false).await.unwrap();
    let db = Db::open("main.db", &cfg.vfs_name).unwrap();
    assert_eq!(
        db.exec(
            c"ATTACH 'aux.db' AS aux;
        CREATE TABLE main.t(n); CREATE TABLE aux.t(n);
        INSERT INTO main.t VALUES(1); INSERT INTO aux.t VALUES(1);
        CREATE TEMP TABLE verify(n CHECK(n=1));"
        ),
        ffi::SQLITE_OK
    );

    for _ in 0..3 {
        assert_eq!(
            db.exec(c"BEGIN; UPDATE main.t SET n=2; UPDATE aux.t SET n=2;"),
            ffi::SQLITE_OK
        );

        let guard = Fault::new("main-flush");
        let code = db.exec(c"COMMIT;");
        drop(guard);

        assert_eq!(code, ffi::SQLITE_IOERR);
        assert_eq!(
            db.exec(
                c"INSERT INTO verify SELECT n FROM main.t;
            INSERT INTO verify SELECT n FROM aux.t;"
            ),
            ffi::SQLITE_OK
        );
        assert_eq!(pool.count(), 2);
        assert!(!pool.list().iter().any(|name| name.contains("-mj")));
    }

    assert_eq!(
        db.exec(
            c"BEGIN; UPDATE main.t SET n=3;
        UPDATE aux.t SET n=3; COMMIT;"
        ),
        ffi::SQLITE_OK
    );
    drop(db);

    assert_eq!(pool.count(), 2);
    unsafe {
        pool.uninstall().unwrap();
    }
}

#[wasm_bindgen_test]
async fn database_names_reserve_room_for_all_journals() {
    let mut cfg = config("test-database-name-boundary");
    cfg.initial_capacity = 6;
    let pool = install::<ffi::WasmOsCallback>(&cfg, false).await.unwrap();
    let name = "é".repeat(249) + "x";
    let db = Db::open(&name, &cfg.vfs_name).unwrap();
    assert_eq!(
        db.exec(
            c"ATTACH 'aux.db' AS aux;
        CREATE TABLE main.t(n); CREATE TABLE aux.t(n);
        BEGIN; INSERT INTO main.t VALUES(1); INSERT INTO aux.t VALUES(1);
        COMMIT;"
        ),
        ffi::SQLITE_OK
    );
    drop(db);

    let bytes = pool.export_db(&name).unwrap();
    pool.delete_db(&name).unwrap();
    for checked in [true, false] {
        let import = |name: &str| {
            if checked {
                pool.import_db(name, &bytes)
            } else {
                pool.import_db_unchecked(name, &bytes)
            }
        };
        import(&name).unwrap();
        assert_eq!(pool.export_db(&name).unwrap(), bytes);
        pool.delete_db(&name).unwrap();

        for length in [500, 504, 511, 512] {
            let invalid = "x".repeat(length);
            assert!(matches!(
                import(&invalid),
                Err(OpfsSAHError::InvalidFilename(_))
            ));
            assert!(matches!(
                Db::open(&invalid, &cfg.vfs_name),
                Err(ffi::SQLITE_CANTOPEN)
            ));
            assert!(!pool.exists(&invalid));
        }
    }

    unsafe {
        pool.uninstall().unwrap();
    }
}
