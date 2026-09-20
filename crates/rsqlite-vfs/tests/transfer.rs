use std::cell::RefCell;

use rsqlite_vfs::transfer::{
    DbExport, DbImport, ExportSource, ImportDbError, ImportMode, ImportTarget, TransferError,
};

struct Target<'a> {
    published: &'a RefCell<Option<Vec<u8>>>,
    bytes: Vec<u8>,
    fail_write: bool,
}

impl ImportTarget for Target<'_> {
    type Error = TransferError;

    fn write_at(&mut self, offset: u64, bytes: &[u8]) -> Result<(), Self::Error> {
        if self.fail_write {
            return Err(TransferError::OutOfMemory);
        }
        let start = offset as usize;
        self.bytes
            .resize(self.bytes.len().max(start + bytes.len()), 0);
        self.bytes[start..start + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    fn commit(self) -> Result<(), Self::Error> {
        self.published.replace(Some(self.bytes));
        Ok(())
    }

    fn abort(self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn abort_with_error(self, error: Self::Error) -> Self::Error {
        error
    }
}

#[test]
fn test_imports_validate_before_publication_and_poison_failed_writes() {
    let published = RefCell::new(None);
    let target = |fail_write| Target {
        published: &published,
        bytes: Vec::new(),
        fail_write,
    };
    let mut bytes = [0; 512];
    bytes[..16].copy_from_slice(b"SQLite format 3\0");
    bytes[16..20].copy_from_slice(&[2, 0, 2, 2]);

    let mut import = DbImport::new(target(false), 512, ImportMode::Checked).unwrap();
    import.write(&bytes[..17]).unwrap();
    import.write(&bytes[17..]).unwrap();
    assert!(published.borrow().is_none());
    import.finish().unwrap();
    let mut expected = bytes;
    expected[18..20].copy_from_slice(&[1, 1]);
    assert_eq!(published.take().unwrap(), expected);

    let mut import = DbImport::new(target(false), 512, ImportMode::Unchecked).unwrap();
    import.write(&bytes).unwrap();
    import.finish().unwrap();
    assert_eq!(published.take().unwrap(), bytes);

    let mut import = DbImport::new(target(false), 512, ImportMode::Checked).unwrap();
    assert!(matches!(
        import.write(&[0; 18]),
        Err(TransferError::ImportDb(ImportDbError::InvalidHeader))
    ));
    assert!(matches!(import.finish(), Err(TransferError::ImportFailed)));

    let mut import = DbImport::new(target(true), 512, ImportMode::Checked).unwrap();
    assert!(matches!(
        import.write(&bytes),
        Err(TransferError::OutOfMemory)
    ));
    assert!(matches!(
        import.write(&bytes),
        Err(TransferError::ImportFailed)
    ));
    assert!(matches!(import.finish(), Err(TransferError::ImportFailed)));

    let import = DbImport::new(target(false), (1u64 << 32) + 512, ImportMode::Checked).unwrap();
    assert!(matches!(
        import.finish(),
        Err(TransferError::SizeMismatch { expected, actual: 0 }) if expected == (1u64 << 32) + 512
    ));
    assert!(published.borrow().is_none());
}

struct Source<'a> {
    bytes: &'a [u8],
    size: u64,
}

impl ExportSource for Source<'_> {
    type Error = TransferError;

    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let bytes = &self.bytes[offset as usize..];
        let count = bytes.len().min(buf.len()).min(3);
        buf[..count].copy_from_slice(&bytes[..count]);
        Ok(count)
    }
}

#[test]
fn test_exports_retry_partial_reads_but_reject_premature_eof() {
    let bytes = b"a database image";
    let mut export = DbExport::new(Source {
        bytes,
        size: bytes.len() as u64,
    });
    let mut prefix = [0; 5];
    assert_eq!(export.read(&mut prefix).unwrap(), prefix.len());
    assert_eq!(prefix, bytes[..5]);
    assert_eq!(export.read_to_vec().unwrap(), bytes[5..]);

    let export = DbExport::new(Source {
        bytes,
        size: bytes.len() as u64 + 1,
    });
    assert!(matches!(
        export.read_to_vec(),
        Err(TransferError::ShortRead { actual: 0, .. })
    ));

    let export = DbExport::new(Source {
        bytes: &[],
        size: u64::MAX,
    });
    assert!(matches!(
        export.read_to_vec(),
        Err(TransferError::FileTooLarge)
    ));
}
