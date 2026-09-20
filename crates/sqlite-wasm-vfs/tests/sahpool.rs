use rsqlite_vfs::{transfer::DbTransfer, VfsFilesManager};
use sqlite_wasm_rs::*;
use sqlite_wasm_vfs::sahpool::{install, OpfsSAHError, OpfsSAHPoolCfgBuilder, OpfsSAHPoolUtil};
use std::{
    ffi::{CStr, CString},
    ptr,
};
use wasm_bindgen_test::wasm_bindgen_test;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);

fn config(name: &str) -> OpfsSAHPoolCfgBuilder {
    OpfsSAHPoolCfgBuilder::new()
        .vfs_name(name)
        .directory(&format!("sqlite-wasm-vfs-tests/{name}"))
}

async fn pool(name: &str, capacity: usize) -> OpfsSAHPoolUtil {
    install::<WasmOsCallback>(
        &config(name)
            .clear_on_init(true)
            .initial_capacity(capacity)
            .build(),
        false,
    )
    .await
    .unwrap()
}

struct Db(*mut sqlite3);

impl Db {
    fn try_open(vfs: &str, name: &str) -> Result<Self, i32> {
        let vfs = CString::new(vfs).unwrap();
        let name = CString::new(name).unwrap();
        let mut raw = ptr::null_mut();
        let code = unsafe {
            sqlite3_open_v2(
                name.as_ptr(),
                &mut raw,
                SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
                vfs.as_ptr(),
            )
        };

        if code != SQLITE_OK {
            if !raw.is_null() {
                assert_eq!(unsafe { sqlite3_close(raw) }, SQLITE_OK);
            }

            return Err(code);
        }

        Ok(Self(raw))
    }

    fn open(vfs: &str, name: &str) -> Self {
        Self::try_open(vfs, name).unwrap()
    }

    fn exec(&self, sql: &CStr) {
        let code =
            unsafe { sqlite3_exec(self.0, sql.as_ptr(), None, ptr::null_mut(), ptr::null_mut()) };

        assert_eq!(code, SQLITE_OK, "{}", unsafe {
            CStr::from_ptr(sqlite3_errmsg(self.0)).to_string_lossy()
        });
    }

    fn scalar(&self, sql: &CStr) -> i64 {
        let mut statement = ptr::null_mut();

        unsafe {
            assert_eq!(
                sqlite3_prepare_v2(self.0, sql.as_ptr(), -1, &mut statement, ptr::null_mut()),
                SQLITE_OK
            );
            assert_eq!(sqlite3_step(statement), SQLITE_ROW);

            let value = sqlite3_column_int64(statement, 0);
            assert_eq!(sqlite3_step(statement), SQLITE_DONE);
            assert_eq!(sqlite3_finalize(statement), SQLITE_OK);

            value
        }
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        assert_eq!(unsafe { sqlite3_close(self.0) }, SQLITE_OK);
    }
}

#[wasm_bindgen_test]
async fn test_registration() {
    let util = pool("registration", 2).await;
    util.import_db_unchecked("kept", b"retained bytes").unwrap();

    let reused = pool("registration", 8).await;
    assert_eq!(reused.capacity(), 2);
    assert_eq!(reused.export_db("kept").unwrap(), b"retained bytes");

    assert!(matches!(
        install::<WasmOsCallback>(
            &config("registration")
                .directory("sqlite-wasm-vfs-tests/other")
                .build(),
            false,
        )
        .await,
        Err(OpfsSAHError::ConfigurationMismatch)
    ));
    assert!(matches!(
        install::<WasmOsCallback>(
            &config("registration")
                .vfs_name("registration-other")
                .build(),
            false,
        )
        .await,
        Err(OpfsSAHError::DirectoryInUse(_))
    ));

    util.pause().unwrap();
    let reused = pool("registration", 8).await;
    assert!(reused.is_paused());
    reused.resume().await.unwrap();
    assert_eq!(util.export_db("kept").unwrap(), b"retained bytes");

    unsafe { util.uninstall().unwrap() };
    assert!(reused.is_uninstalled());
    assert!(matches!(
        reused.contains("kept"),
        Err(OpfsSAHError::Uninstalled)
    ));
    assert!(matches!(reused.names(), Err(OpfsSAHError::Uninstalled)));
    assert!(matches!(reused.len(), Err(OpfsSAHError::Uninstalled)));
    assert!(matches!(reused.is_empty(), Err(OpfsSAHError::Uninstalled)));
    assert!(matches!(
        reused.resume().await,
        Err(OpfsSAHError::Uninstalled)
    ));
    assert!(matches!(reused.clear(), Err(OpfsSAHError::Uninstalled)));
    unsafe { reused.uninstall().unwrap() };

    let replacement = install::<WasmOsCallback>(
        &config("registration")
            .vfs_name("registration-other")
            .initial_capacity(2)
            .build(),
        false,
    )
    .await
    .unwrap();
    assert_eq!(replacement.export_db("kept").unwrap(), b"retained bytes");
    assert!(util.is_uninstalled());

    unsafe { replacement.uninstall().unwrap() };
}

#[wasm_bindgen_test]
async fn test_persistence() {
    let util = pool("persistence", 3).await;
    let db = Db::open("persistence", "main.db");

    db.exec(c"PRAGMA journal_mode=DELETE; CREATE TABLE t(n); INSERT INTO t VALUES(10);");
    db.exec(c"BEGIN; INSERT INTO t VALUES(20); COMMIT;");
    db.exec(c"BEGIN; DELETE FROM t; INSERT INTO t VALUES(99); ROLLBACK;");
    assert_eq!(db.scalar(c"SELECT sum(n) FROM t"), 30);
    drop(db);

    util.pause().unwrap();
    util.pause().unwrap();
    assert!(util.is_paused());
    assert_eq!(util.capacity(), 0);
    assert!(matches!(
        util.contains("main.db"),
        Err(OpfsSAHError::Paused)
    ));
    assert!(matches!(util.names(), Err(OpfsSAHError::Paused)));
    assert!(matches!(util.len(), Err(OpfsSAHError::Paused)));
    assert!(matches!(util.is_empty(), Err(OpfsSAHError::Paused)));
    assert!(unsafe { sqlite3_vfs_find(c"persistence".as_ptr()) }.is_null());
    assert!(matches!(
        util.export_db("main.db"),
        Err(OpfsSAHError::Paused)
    ));

    util.resume().await.unwrap();
    util.resume().await.unwrap();
    assert!(!util.is_paused());
    assert!(util.contains("main.db").unwrap());
    assert_eq!(util.capacity(), 3);

    let db = Db::open("persistence", "main.db");
    assert_eq!(db.scalar(c"SELECT sum(n) FROM t"), 30);
    drop(db);
    unsafe { util.uninstall().unwrap() };

    let reinstalled =
        install::<WasmOsCallback>(&config("persistence").initial_capacity(3).build(), false)
            .await
            .unwrap();
    let db = Db::open("persistence", "main.db");
    assert_eq!(db.scalar(c"SELECT sum(n) FROM t"), 30);
    drop(db);

    unsafe { reinstalled.uninstall().unwrap() };
}

#[wasm_bindgen_test]
async fn test_file_management() {
    let util = pool("management", 3).await;
    util.import_db_unchecked("a", b"first").unwrap();
    util.import_db_unchecked("b", b"second").unwrap();

    let mut names = util.names().unwrap();
    names.sort();
    assert_eq!(names, ["a", "b"]);
    assert_eq!(util.len().unwrap(), 2);
    assert!(util.contains("a").unwrap());
    assert!(!util.contains("missing").unwrap());

    assert_eq!(util.reduce_capacity(usize::MAX).await.unwrap(), 1);
    assert_eq!(util.capacity(), 2);
    assert_eq!(util.export_db("a").unwrap(), b"first");
    assert_eq!(util.export_db("b").unwrap(), b"second");
    assert!(matches!(
        util.import_db_unchecked("c", b"third"),
        Err(OpfsSAHError::NoCapacity)
    ));

    assert_eq!(util.add_capacity(2).await.unwrap(), 4);
    util.ensure_capacity(3).await.unwrap();
    assert_eq!(util.capacity(), 4);
    util.import_db_unchecked("c", b"third").unwrap();

    assert!(util.remove("b").unwrap());
    assert!(!util.remove("b").unwrap());
    util.import_db_unchecked("d", b"reused").unwrap();
    assert_eq!(util.export_db("d").unwrap(), b"reused");
    assert_eq!(util.len().unwrap(), 3);

    util.clear().unwrap();
    assert!(util.is_empty().unwrap());
    assert!(util.names().unwrap().is_empty());
    assert_eq!(util.capacity(), 4);
    assert_eq!(util.reduce_capacity(usize::MAX).await.unwrap(), 4);
    assert_eq!(util.capacity(), 0);

    util.ensure_capacity(2).await.unwrap();
    assert_eq!(util.capacity(), 2);
    util.import_db_unchecked("a", b"new contents").unwrap();
    assert_eq!(util.export_db("a").unwrap(), b"new contents");

    unsafe { util.uninstall().unwrap() };
}

#[wasm_bindgen_test]
async fn test_transfer_roundtrip() {
    let util = pool("transfer", 4).await;
    let db = Db::open("transfer", "source.db");
    db.exec(c"CREATE TABLE t(n, data); INSERT INTO t VALUES(42, zeroblob(16384));");
    drop(db);

    let mut export = util.begin_export("source.db").unwrap();
    let size = export.size();
    let mut bytes = Vec::new();
    let mut buffer = [0; 257];

    loop {
        let count = export.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }

        bytes.extend_from_slice(&buffer[..count]);
    }
    drop(export);
    assert_eq!(bytes.len() as u64, size);
    assert_eq!(util.export_db("source.db").unwrap(), bytes);

    let mut import = util.begin_import("copy.db", size).unwrap();

    for chunk in bytes.chunks(509) {
        import.write(chunk).unwrap();
    }

    assert!(matches!(util.contains("copy.db"), Err(OpfsSAHError::Busy)));
    import.finish().unwrap();
    assert_eq!(util.export_db("copy.db").unwrap(), bytes);

    let db = Db::open("transfer", "copy.db");
    assert_eq!(db.scalar(c"SELECT n FROM t"), 42);
    assert_eq!(db.scalar(c"SELECT length(data) FROM t"), 16384);
    assert_eq!(
        db.scalar(c"SELECT count(*) FROM pragma_integrity_check WHERE integrity_check != 'ok'"),
        0
    );
    drop(db);

    let opaque = b"opaque\0non-SQLite bytes";
    util.import_db_unchecked("opaque", opaque).unwrap();
    assert_eq!(util.export_db("opaque").unwrap(), opaque);

    unsafe { util.uninstall().unwrap() };
}

#[wasm_bindgen_test]
async fn test_import_cleanup() {
    enum Finish {
        Drop,
        Abort,
        InvalidHeader,
        Incomplete,
    }

    let util = pool("cleanup", 1).await;

    for finish in [
        Finish::Drop,
        Finish::Abort,
        Finish::InvalidHeader,
        Finish::Incomplete,
    ] {
        match finish {
            Finish::Drop | Finish::Abort => {
                let mut import = util.begin_import_unchecked("pending", 8).unwrap();
                import.write(b"partial").unwrap();

                if matches!(finish, Finish::Drop) {
                    drop(import);
                } else {
                    import.abort().unwrap();
                }
            }
            Finish::InvalidHeader => {
                let mut import = util.begin_import("pending", 512).unwrap();
                assert!(matches!(
                    import.write(&[0; 18]),
                    Err(OpfsSAHError::ImportDb(_))
                ));
                assert!(matches!(import.finish(), Err(OpfsSAHError::ImportFailed)));
            }
            Finish::Incomplete => {
                let mut import = util.begin_import_unchecked("pending", 8).unwrap();
                import.write(b"short").unwrap();
                assert!(matches!(
                    import.finish(),
                    Err(OpfsSAHError::ImportSizeMismatch {
                        expected: 8,
                        actual: 5
                    })
                ));
            }
        }

        assert!(!util.contains("pending").unwrap());
        assert_eq!(util.capacity(), 1);
        util.import_db_unchecked("reused", b"replacement").unwrap();
        assert_eq!(util.export_db("reused").unwrap(), b"replacement");
        assert!(util.remove("reused").unwrap());
        assert!(util.is_empty().unwrap());
    }

    util.pause().unwrap();
    util.resume().await.unwrap();
    assert!(util.is_empty().unwrap());
    assert_eq!(util.capacity(), 1);

    unsafe { util.uninstall().unwrap() };
}

#[wasm_bindgen_test]
async fn test_busy_guards() {
    let util = pool("busy", 4).await;
    let db = Db::open("busy", "main.db");
    db.exec(c"CREATE TABLE t(n); INSERT INTO t VALUES(42);");

    assert!(matches!(
        util.remove("main.db"),
        Err(OpfsSAHError::FileInUse(_))
    ));
    assert!(matches!(
        util.begin_export("main.db"),
        Err(OpfsSAHError::FileInUse(_))
    ));
    assert!(matches!(util.clear(), Err(OpfsSAHError::FilesInUse)));
    assert!(matches!(util.pause(), Err(OpfsSAHError::FilesInUse)));
    assert!(!util.is_paused());
    assert_eq!(db.scalar(c"SELECT n FROM t"), 42);
    drop(db);

    let import = util.begin_import_unchecked("pending", 3).unwrap();
    assert!(matches!(util.contains("main.db"), Err(OpfsSAHError::Busy)));
    assert!(matches!(util.names(), Err(OpfsSAHError::Busy)));
    assert!(matches!(util.len(), Err(OpfsSAHError::Busy)));
    assert!(matches!(util.is_empty(), Err(OpfsSAHError::Busy)));
    assert!(matches!(util.pause(), Err(OpfsSAHError::Busy)));
    assert!(matches!(util.clear(), Err(OpfsSAHError::Busy)));
    assert!(matches!(
        util.add_capacity(1).await,
        Err(OpfsSAHError::Busy)
    ));
    assert!(matches!(util.remove("main.db"), Err(OpfsSAHError::Busy)));
    assert!(matches!(
        Db::try_open("busy", "other.db"),
        Err(SQLITE_CANTOPEN)
    ));
    drop(import);

    let mut export = util.begin_export("main.db").unwrap();
    let mut buffer = [0; 1024];

    while export.read(&mut buffer).unwrap() != 0 {}

    assert!(matches!(util.pause(), Err(OpfsSAHError::Busy)));
    assert!(matches!(util.remove("main.db"), Err(OpfsSAHError::Busy)));
    assert!(matches!(
        Db::try_open("busy", "other.db"),
        Err(SQLITE_CANTOPEN)
    ));
    drop(export);

    util.pause().unwrap();
    util.resume().await.unwrap();

    let db = Db::open("busy", "main.db");
    assert_eq!(db.scalar(c"SELECT n FROM t"), 42);
    drop(db);

    unsafe { util.uninstall().unwrap() };
}

#[wasm_bindgen_test]
async fn test_sidecar_guard() {
    let util = pool("sidecar", 3).await;
    let db = Db::open("sidecar", "main.db");
    db.exec(c"PRAGMA journal_mode=PERSIST; CREATE TABLE t(n); INSERT INTO t VALUES(42);");
    drop(db);

    assert!(util.contains("main.db-journal").unwrap());
    assert!(matches!(
        util.begin_export("main.db"),
        Err(OpfsSAHError::RecoveryRequired(_))
    ));

    // This journal belongs to a successfully committed, closed connection.
    assert!(util.remove("main.db-journal").unwrap());

    let bytes = util.export_db("main.db").unwrap();
    util.import_db("copy.db", &bytes).unwrap();

    let db = Db::open("sidecar", "copy.db");
    assert_eq!(db.scalar(c"SELECT n FROM t"), 42);
    drop(db);

    unsafe { util.uninstall().unwrap() };
}

#[wasm_bindgen_test]
async fn test_filename_limits() {
    let util = pool("filenames", 3).await;

    for name in ["a".repeat(499), format!("{}x", "界".repeat(166))] {
        assert_eq!(name.len(), 499);

        let db = Db::open("filenames", &name);
        db.exec(c"CREATE TABLE t(n); BEGIN; INSERT INTO t VALUES(42); COMMIT;");
        drop(db);

        let db = Db::open("filenames", &name);
        assert_eq!(db.scalar(c"SELECT n FROM t"), 42);
        drop(db);
        assert!(util.contains(&name).unwrap());

        let too_long = format!("{name}x");
        assert!(matches!(
            Db::try_open("filenames", &too_long),
            Err(SQLITE_CANTOPEN)
        ));
        assert!(matches!(
            util.import_db_unchecked(&too_long, b"data"),
            Err(OpfsSAHError::InvalidFilename(_))
        ));
        assert!(util.remove(&name).unwrap());
    }

    for name in ["", "has\0nul"] {
        assert!(matches!(
            util.import_db_unchecked(name, b"data"),
            Err(OpfsSAHError::InvalidFilename(_))
        ));
    }
    assert!(util.is_empty().unwrap());

    unsafe { util.uninstall().unwrap() };
}
