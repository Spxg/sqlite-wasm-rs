use std::cell::{Cell, RefCell};

use rsqlite_vfs::transfer::{DbTransfer, ExportSource, ImportDbError, ImportTarget, TransferError};

#[derive(Debug)]
enum Error {
    Transfer(TransferError),
    Write,
    Commit,
    Cleanup,
    WithCleanup(Box<Error>),
}

impl From<TransferError> for Error {
    fn from(error: TransferError) -> Self {
        Self::Transfer(error)
    }
}

#[derive(Default)]
struct Store {
    published: RefCell<Option<Vec<u8>>>,
    events: RefCell<Vec<&'static str>>,
    fail_write: Cell<bool>,
    fail_commit: Cell<bool>,
    fail_cleanup: Cell<bool>,
    export_size: Cell<Option<u64>>,
}

impl DbTransfer for Store {
    type Error = Error;
    type Target<'a> = Target<'a>;
    type Source<'a> = Source;

    fn create_import(&self, _: &str, _: u64) -> Result<Target<'_>, Error> {
        Ok(Target {
            store: self,
            bytes: Vec::new(),
        })
    }

    fn open_export(&self, _: &str) -> Result<Source, Error> {
        let bytes = self.published.borrow().as_ref().unwrap().clone();
        let size = self.export_size.get().unwrap_or(bytes.len() as u64);
        Ok(Source { bytes, size })
    }
}

struct Target<'a> {
    store: &'a Store,
    bytes: Vec<u8>,
}

impl Drop for Target<'_> {
    fn drop(&mut self) {
        self.store.events.borrow_mut().push("drop");
    }
}

impl ImportTarget for Target<'_> {
    type Error = Error;

    fn write_at(&mut self, offset: u64, bytes: &[u8]) -> Result<(), Error> {
        if self.store.fail_write.get() {
            return Err(Error::Write);
        }

        let start = offset as usize;
        self.bytes
            .resize(self.bytes.len().max(start + bytes.len()), 0);
        self.bytes[start..start + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    fn commit(mut self) -> Result<(), Error> {
        self.store.events.borrow_mut().push("commit");
        if self.store.fail_commit.get() {
            return Err(Error::Commit);
        }

        self.store
            .published
            .replace(Some(core::mem::take(&mut self.bytes)));
        Ok(())
    }

    fn abort(self) -> Result<(), Error> {
        self.store.events.borrow_mut().push("abort");
        if self.store.fail_cleanup.get() {
            Err(Error::Cleanup)
        } else {
            Ok(())
        }
    }

    fn abort_with_error(self, error: Error) -> Error {
        if self.abort().is_err() {
            Error::WithCleanup(Box::new(error))
        } else {
            error
        }
    }
}

struct Source {
    bytes: Vec<u8>,
    size: u64,
}

impl ExportSource for Source {
    type Error = Error;

    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, Error> {
        let bytes = &self.bytes[offset as usize..];
        let count = bytes.len().min(buf.len()).min(3);
        buf[..count].copy_from_slice(&bytes[..count]);
        Ok(count)
    }
}

fn image() -> [u8; 512] {
    let mut bytes = [0; 512];
    bytes[..16].copy_from_slice(b"SQLite format 3\0");
    bytes[16..20].copy_from_slice(&[2, 0, 2, 2]);
    bytes
}

#[test]
fn test_checked_import() {
    let store = Store::default();
    let bytes = image();
    let mut expected = bytes;
    expected[18..20].copy_from_slice(&[1, 1]);

    for chunk_size in [1, 17, 18, 19, 512] {
        let mut import = store.begin_import("test.db", 512).unwrap();

        for chunk in bytes.chunks(chunk_size) {
            import.write(chunk).unwrap();
        }

        assert!(store.published.borrow().is_none());
        import.finish().unwrap();
        assert_eq!(store.published.take().unwrap(), expected);
    }

    store.import_db("test.db", &bytes).unwrap();
    assert_eq!(store.published.take().unwrap(), expected);
    assert_eq!(bytes[18..20], [2, 2]);
}

#[test]
fn test_unchecked_import() {
    let store = Store::default();

    for bytes in [&image()[..], b"encrypted image", b""] {
        let mut import = store
            .begin_import_unchecked("test.db", bytes.len() as u64)
            .unwrap();

        for chunk in bytes.chunks(3) {
            import.write(chunk).unwrap();
        }

        import.finish().unwrap();
        assert_eq!(store.published.take().unwrap(), bytes);

        store.import_db_unchecked("test.db", bytes).unwrap();
        assert_eq!(store.published.take().unwrap(), bytes);
    }
}

#[test]
fn test_failed_import() {
    let store = Store::default();
    let mut import = store.begin_import("test.db", 512).unwrap();
    assert!(matches!(
        import.write(&[0; 18]),
        Err(Error::Transfer(TransferError::ImportDb(
            ImportDbError::InvalidHeader
        )))
    ));
    assert!(matches!(
        import.write(&image()),
        Err(Error::Transfer(TransferError::ImportFailed))
    ));
    assert!(matches!(
        import.finish(),
        Err(Error::Transfer(TransferError::ImportFailed))
    ));

    let mut import = store.begin_import_unchecked("test.db", 2).unwrap();
    assert!(matches!(
        import.write(b"abc"),
        Err(Error::Transfer(TransferError::SizeMismatch {
            expected: 2,
            actual: 3
        }))
    ));
    assert!(matches!(
        import.finish(),
        Err(Error::Transfer(TransferError::ImportFailed))
    ));

    store.fail_write.set(true);
    let mut import = store.begin_import("test.db", 512).unwrap();
    assert!(matches!(import.write(&image()), Err(Error::Write)));
    assert!(matches!(
        import.write(&image()),
        Err(Error::Transfer(TransferError::ImportFailed))
    ));
    assert!(matches!(
        import.finish(),
        Err(Error::Transfer(TransferError::ImportFailed))
    ));
    store.fail_write.set(false);

    let size = (1u64 << 32) + 512;
    let mut import = store.begin_import("test.db", size).unwrap();
    import.write(&image()).unwrap();
    assert!(matches!(import.finish(),
        Err(Error::Transfer(TransferError::SizeMismatch { expected, actual: 512 })) if expected == size));
    assert!(store.published.borrow().is_none());
    assert_eq!(
        store.events.borrow().as_slice(),
        &["abort", "drop", "abort", "drop", "abort", "drop", "abort", "drop"]
    );
}

#[test]
fn test_import_cleanup() {
    let store = Store::default();
    let mut import = store.begin_import("test.db", 512).unwrap();
    import.write(&image()[..17]).unwrap();
    drop(import);
    assert_eq!(store.events.take(), ["drop"]);

    store.begin_import("test.db", 512).unwrap().abort().unwrap();
    assert_eq!(store.events.take(), ["abort", "drop"]);

    store.fail_cleanup.set(true);
    assert!(matches!(
        store.begin_import("test.db", 512).unwrap().abort(),
        Err(Error::Cleanup)
    ));
    assert_eq!(store.events.take(), ["abort", "drop"]);

    store.fail_write.set(true);
    let error = store.import_db("test.db", &image()).unwrap_err();
    assert!(matches!(error, Error::WithCleanup(error) if matches!(*error, Error::Write)));
    assert_eq!(store.events.take(), ["abort", "drop"]);
    store.fail_cleanup.set(false);
    store.fail_write.set(false);

    // Header reset can fail after all chunks were accepted.
    let mut import = store.begin_import("test.db", 512).unwrap();
    import.write(&image()).unwrap();
    store.fail_write.set(true);
    assert!(matches!(import.finish(), Err(Error::Write)));
    assert_eq!(store.events.take(), ["abort", "drop"]);
    store.fail_write.set(false);

    store.fail_commit.set(true);
    assert!(matches!(
        store.import_db("test.db", &image()),
        Err(Error::Commit)
    ));
    assert_eq!(store.events.take(), ["commit", "drop"]);
    assert!(store.published.borrow().is_none());
}

#[test]
fn test_export() {
    let bytes = b"a database image";
    let store = Store::default();
    store.published.replace(Some(bytes.to_vec()));

    let mut export = store.begin_export("test.db").unwrap();
    assert_eq!(export.size(), bytes.len() as u64);
    assert_eq!(export.read(&mut []).unwrap(), 0);

    let mut prefix = [0; 5];
    assert_eq!(export.read(&mut prefix).unwrap(), prefix.len());
    assert_eq!(prefix, bytes[..5]);
    assert_eq!(export.read_to_vec().unwrap(), bytes[5..]);

    let mut export = store.begin_export("test.db").unwrap();
    let mut output = [99; 32];
    assert_eq!(export.read(&mut output).unwrap(), bytes.len());
    assert_eq!(&output[..bytes.len()], bytes);
    assert!(output[bytes.len()..].iter().all(|&byte| byte == 99));
    assert_eq!(export.read(&mut output).unwrap(), 0);
    assert_eq!(store.export_db("test.db").unwrap(), bytes);

    store.published.replace(Some(Vec::new()));
    assert!(store.export_db("test.db").unwrap().is_empty());
}

#[test]
fn test_export_limits() {
    let bytes = b"a database image";
    let store = Store::default();
    store.published.replace(Some(bytes.to_vec()));
    store.export_size.set(Some(bytes.len() as u64 + 1));
    let mut export = store.begin_export("test.db").unwrap();
    let mut output = [0; 32];
    assert!(matches!(
        export.read(&mut output),
        Err(Error::Transfer(TransferError::ShortRead { actual: 0, .. }))
    ));

    // Failed reads leave the cursor at its original position.
    let mut prefix = [0; 5];
    assert_eq!(export.read(&mut prefix).unwrap(), 5);
    assert_eq!(prefix, bytes[..5]);

    store.export_size.set(Some(u64::MAX));
    assert!(matches!(
        store.export_db("test.db"),
        Err(Error::Transfer(TransferError::FileTooLarge))
    ));
}
