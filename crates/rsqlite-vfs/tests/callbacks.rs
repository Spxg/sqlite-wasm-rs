use rsqlite_vfs::{ffi::*, *};
use std::{cell::RefCell, ffi::CStr, rc::Rc, time::Duration};

#[derive(Default)]
struct State {
    hints: Vec<u64>,
    actual_access: Option<OpenAccess>,
    error: Option<VfsError>,
    opens: Vec<OpenOptions>,
    locks: Vec<LockLevel>,
    unlocks: Vec<LockLevel>,
    syncs: Vec<SyncOptions>,
    accesses: Vec<AccessMode>,
    deletes: Vec<bool>,
    failure: Option<VfsErrorCode>,
    system_error: Option<SystemErrorCode>,
    drops: usize,
    read_count: usize,
    reads: Vec<u64>,
    writes: Vec<(u64, Vec<u8>)>,
    truncates: Vec<u64>,
    size: u64,
    names: Vec<Option<String>>,
    closes: Vec<(Option<String>, OpenOptions)>,
    now: i64,
    random_count: Option<usize>,
    sleeps: Vec<Duration>,
    reserved: bool,
}

impl State {
    fn result(&self) -> VfsResult<()> {
        match self.failure {
            Some(code) => {
                let mut error = VfsError::new(code, "a中b".into());

                if let Some(system) = self.system_error {
                    error = error.with_system_error(system);
                }

                Err(error)
            }
            None => Ok(()),
        }
    }
}

struct Backend(RefCell<State>);

impl core::ops::Deref for Backend {
    type Target = RefCell<State>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

type Data = Rc<Backend>;

struct File(Data);

impl Drop for File {
    fn drop(&mut self) {
        self.0.borrow_mut().drops += 1;
    }
}

impl VfsFile for File {
    fn size_hint(&mut self, size: u64) -> VfsResult<bool> {
        self.0.borrow_mut().hints.push(size);
        self.0.borrow().result()?;
        Ok(true)
    }

    fn sector_size(&self) -> SectorSize {
        SectorSize::new(8192).unwrap()
    }

    fn device_characteristics(&self) -> DeviceCharacteristics {
        DeviceCharacteristics::SAFE_APPEND | DeviceCharacteristics::UNDELETABLE_WHEN_OPEN
    }

    fn read(&mut self, buf: &mut [u8], offset: u64) -> VfsResult<usize> {
        let mut state = self.0.borrow_mut();
        state.reads.push(offset);
        state.result()?;

        assert!(buf.iter().all(|&byte| byte == 0));

        let count = state.read_count.min(buf.len());
        buf[..count].fill(42);
        Ok(state.read_count)
    }

    fn write(&mut self, bytes: &[u8], offset: u64) -> VfsResult<()> {
        self.0.borrow_mut().writes.push((offset, bytes.to_vec()));
        self.0.borrow().result()
    }

    fn truncate(&mut self, size: u64) -> VfsResult<()> {
        self.0.borrow_mut().truncates.push(size);
        self.0.borrow().result()?;

        self.0.borrow_mut().size = size;
        Ok(())
    }

    fn size(&self) -> VfsResult<u64> {
        self.0.borrow().result()?;
        Ok(self.0.borrow().size)
    }

    fn sync(&mut self, options: SyncOptions) -> VfsResult<()> {
        self.0.borrow_mut().syncs.push(options);
        self.0.borrow().result()
    }

    fn lock(&mut self, level: LockLevel) -> VfsResult<()> {
        self.0.borrow_mut().locks.push(level);
        self.0.borrow().result()
    }

    fn unlock(&mut self, level: LockLevel) -> VfsResult<()> {
        self.0.borrow_mut().unlocks.push(level);
        self.0.borrow().result()
    }

    fn check_reserved_lock(&self) -> VfsResult<bool> {
        self.0.borrow().result()?;
        Ok(self.0.borrow().reserved)
    }
}

struct Store;

impl VfsStore for Store {
    type File = File;

    fn close_file(
        data: &Data,
        name: Option<&str>,
        file: File,
        options: OpenOptions,
    ) -> VfsResult<()> {
        data.borrow_mut()
            .closes
            .push((name.map(str::to_owned), options));
        drop(file);
        data.borrow().result()
    }

    type AppData = Data;

    fn record_error(data: &Data, error: VfsError) {
        data.borrow_mut().error = Some(error);
    }

    fn last_error(data: &Data) -> Option<VfsError> {
        data.borrow().error.clone()
    }

    fn open_file(data: &Data, request: OpenRequest<'_>) -> VfsResult<OpenedFile<File>> {
        let options = request.options;
        data.borrow_mut().opens.push(options);
        data.borrow_mut()
            .names
            .push(request.filename.map(|name| name.path().to_owned()));
        data.borrow().result()?;

        Ok(OpenedFile {
            file: File(data.clone()),
            access: data.borrow().actual_access.unwrap_or(options.access()),
        })
    }

    fn access(data: &Data, name: &str, mode: AccessMode) -> VfsResult<bool> {
        data.borrow_mut().accesses.push(mode);
        data.borrow().result()?;
        Ok(name == "exists")
    }

    fn full_pathname(data: &Data, name: &str) -> VfsResult<String> {
        data.borrow().result()?;
        Ok(if name == "nul" {
            "bad\0name".into()
        } else {
            name.into()
        })
    }

    fn delete_file(data: &Data, _: &str, sync_dir: bool) -> VfsResult<()> {
        data.borrow_mut().deletes.push(sync_dir);
        data.borrow().result()
    }
}

struct Io;

impl SQLiteIoMethods for Io {
    type Store = Store;

    const VERSION: i32 = 3;
}

struct Vfs;

impl SQLiteVfs<Io> for Vfs {
    type Os = Backend;

    const VERSION: i32 = 3;

    fn os(data: &Data) -> &Self::Os {
        data
    }
}

impl OsCallback for Backend {
    fn sleep(&self, duration: Duration) {
        self.borrow_mut().sleeps.push(duration);
    }

    fn random(&self, buf: &mut [u8]) -> usize {
        assert!(!buf.is_empty());

        let count = self.borrow().random_count.unwrap_or(buf.len());
        let written = count.min(buf.len());
        buf[..written].fill(42);
        count
    }

    fn epoch_timestamp_in_ms(&self) -> VfsResult<i64> {
        self.borrow().result()?;
        Ok(self.borrow().now)
    }
}

fn with_vfs(test: impl FnOnce(*mut sqlite3_vfs, &Data)) {
    let state = Rc::new(Backend(RefCell::new(State::default())));
    let mut data = VfsAppData::new(state.clone());
    let mut vfs = unsafe { Vfs::vfs(c"callbacks".as_ptr(), &mut data) };

    // Keep the VFS and its data at stable addresses while handles are open.
    test(core::ptr::from_mut(&mut vfs), &state);
}

unsafe fn open(vfs: *mut sqlite3_vfs) -> SQLiteVfsFile {
    let mut file: SQLiteVfsFile = core::mem::zeroed();
    assert_eq!(
        Vfs::xOpen(
            vfs,
            c"file".as_ptr(),
            file.sqlite3_file(),
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_TEMP_DB,
            core::ptr::null_mut()
        ),
        SQLITE_OK
    );
    file
}

#[test]
fn test_open_close() {
    with_vfs(|vfs_ptr, state| unsafe {
        let mut file: SQLiteVfsFile = core::mem::zeroed();
        let flags = SQLITE_OPEN_READWRITE
            | SQLITE_OPEN_CREATE
            | SQLITE_OPEN_EXCLUSIVE
            | SQLITE_OPEN_DELETEONCLOSE
            | SQLITE_OPEN_TEMP_DB;
        let mut out_flags = -1;
        assert_eq!(
            Vfs::xOpen(
                vfs_ptr,
                c"file".as_ptr(),
                file.sqlite3_file(),
                flags,
                &mut out_flags
            ),
            SQLITE_OK
        );
        assert_eq!(out_flags, flags);

        assert_eq!(file.vfs(), vfs_ptr);
        assert_eq!(file.name(), Some("file"));
        assert_eq!(file.options().raw_flags(), flags);
        let opened = state.borrow().opens[0];
        assert_eq!(opened.kind(), Some(FileKind::TempDb));
        assert_eq!(opened.access(), OpenAccess::ReadWrite);
        assert!(opened.create() && opened.exclusive() && opened.delete_on_close());

        state.borrow_mut().failure = Some(VfsErrorCode::IoClose);
        assert_eq!(Io::xClose(file.sqlite3_file()), SQLITE_IOERR_CLOSE);
        assert!((*file.sqlite3_file()).pMethods.is_null());
        assert_eq!(state.borrow().drops, 1);
        assert_eq!(state.borrow().closes[0].0.as_deref(), Some("file"));
        assert_eq!(state.borrow().closes[0].1.raw_flags(), flags);
        state.borrow_mut().failure = None;

        let open_count = state.borrow().opens.len();
        for invalid in [
            0,
            SQLITE_OPEN_READONLY | SQLITE_OPEN_READWRITE,
            SQLITE_OPEN_READONLY | SQLITE_OPEN_CREATE,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_EXCLUSIVE,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_DELETEONCLOSE,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_MAIN_DB | SQLITE_OPEN_WAL,
        ] {
            out_flags = -1;
            assert_eq!(
                Vfs::xOpen(
                    vfs_ptr,
                    c"file".as_ptr(),
                    file.sqlite3_file(),
                    invalid,
                    &mut out_flags
                ),
                SQLITE_CANTOPEN
            );
            assert!((*file.sqlite3_file()).pMethods.is_null());
            assert_eq!(out_flags, -1);
        }
        assert_eq!(state.borrow().opens.len(), open_count);

        state.borrow_mut().failure = Some(VfsErrorCode::CantOpen);
        assert_eq!(
            Vfs::xOpen(
                vfs_ptr,
                c"file".as_ptr(),
                file.sqlite3_file(),
                flags,
                &mut out_flags
            ),
            SQLITE_CANTOPEN
        );
        assert!((*file.sqlite3_file()).pMethods.is_null());
        assert_eq!(state.borrow().drops, 1);
        state.borrow_mut().failure = None;

        state.borrow_mut().actual_access = Some(OpenAccess::ReadOnly);
        assert_eq!(
            Vfs::xOpen(
                vfs_ptr,
                c"readonly".as_ptr(),
                file.sqlite3_file(),
                SQLITE_OPEN_READWRITE | SQLITE_OPEN_MAIN_DB,
                &mut out_flags
            ),
            SQLITE_OK
        );
        assert_eq!(
            out_flags & (SQLITE_OPEN_READONLY | SQLITE_OPEN_READWRITE),
            SQLITE_OPEN_READONLY
        );
        assert_eq!(Io::xClose(file.sqlite3_file()), SQLITE_OK);

        state.borrow_mut().actual_access = None;
        assert_eq!(
            Vfs::xOpen(
                vfs_ptr,
                core::ptr::null(),
                file.sqlite3_file(),
                flags,
                &mut out_flags
            ),
            SQLITE_OK
        );
        assert_eq!(file.name(), None);
        assert!(state.borrow().names.last().unwrap().is_none());
        assert_eq!(Io::xClose(file.sqlite3_file()), SQLITE_OK);
        assert!(state.borrow().closes.last().unwrap().0.is_none());
        assert_eq!(state.borrow().drops, 3);
    });
}

#[test]
fn test_read() {
    with_vfs(|vfs_ptr, state| unsafe {
        let mut file = open(vfs_ptr);

        for (count, code, expected) in [
            (4, SQLITE_OK, [42; 4]),
            (2, SQLITE_IOERR_SHORT_READ, [42, 42, 0, 0]),
            (0, SQLITE_IOERR_SHORT_READ, [0; 4]),
            (5, SQLITE_IOERR_READ, [42; 4]),
        ] {
            state.borrow_mut().read_count = count;
            let mut buf = [99u8; 4];
            assert_eq!(
                Io::xRead(file.sqlite3_file(), buf.as_mut_ptr().cast(), 4, 1 << 32),
                code
            );
            assert_eq!(buf, expected);
            assert_eq!(state.borrow().reads.last(), Some(&(1 << 32)));
        }

        let calls = state.borrow().reads.len();
        for (length, offset, code) in [
            (-1, 0, SQLITE_IOERR_READ),
            (1, -1, SQLITE_IOERR_READ),
            (0, 0, SQLITE_OK),
        ] {
            assert_eq!(
                Io::xRead(file.sqlite3_file(), core::ptr::null_mut(), length, offset),
                code
            );
        }
        assert_eq!(state.borrow().reads.len(), calls);

        for code in [VfsErrorCode::IoRead, VfsErrorCode::IoShortRead] {
            state.borrow_mut().failure = Some(code);
            let mut buf = [99u8; 4];
            assert_eq!(
                Io::xRead(file.sqlite3_file(), buf.as_mut_ptr().cast(), 4, 0),
                SQLITE_IOERR_READ
            );
            assert_eq!(
                state.borrow().error.as_ref().unwrap().code(),
                VfsErrorCode::IoRead
            );
        }
        state.borrow_mut().failure = None;
        assert_eq!(Io::xClose(file.sqlite3_file()), SQLITE_OK);
    });
}

#[test]
fn test_write_truncate_size() {
    with_vfs(|vfs_ptr, state| unsafe {
        let mut file = open(vfs_ptr);
        let offset = (1i64 << 32) + 7;
        assert_eq!(
            Io::xWrite(file.sqlite3_file(), b"abc".as_ptr().cast(), 3, offset),
            SQLITE_OK
        );
        assert_eq!(
            state.borrow().writes.as_slice(),
            &[(offset as u64, b"abc".to_vec())]
        );
        assert_eq!(Io::xTruncate(file.sqlite3_file(), offset), SQLITE_OK);
        assert_eq!(state.borrow().truncates.as_slice(), &[offset as u64]);

        let mut size = -1;
        assert_eq!(Io::xFileSize(file.sqlite3_file(), &mut size), SQLITE_OK);
        assert_eq!(size, offset);

        for (length, offset, code) in [
            (-1, 0, SQLITE_IOERR_WRITE),
            (1, -1, SQLITE_IOERR_WRITE),
            (0, 0, SQLITE_OK),
        ] {
            assert_eq!(
                Io::xWrite(file.sqlite3_file(), core::ptr::null(), length, offset),
                code
            );
        }
        assert_eq!(
            Io::xTruncate(file.sqlite3_file(), -1),
            SQLITE_IOERR_TRUNCATE
        );
        assert_eq!(state.borrow().writes.len(), 1);
        assert_eq!(state.borrow().truncates.len(), 1);

        state.borrow_mut().size = u64::MAX;
        assert_eq!(
            Io::xFileSize(file.sqlite3_file(), &mut size),
            SQLITE_IOERR_FSTAT
        );
        assert_eq!(size, 0);

        state.borrow_mut().failure = Some(VfsErrorCode::Full);
        assert_eq!(
            Io::xWrite(file.sqlite3_file(), b"x".as_ptr().cast(), 1, 0),
            SQLITE_FULL
        );
        assert_eq!(Io::xTruncate(file.sqlite3_file(), 0), SQLITE_FULL);
        assert_eq!(Io::xFileSize(file.sqlite3_file(), &mut size), SQLITE_FULL);
        assert_eq!(size, 0);
        state.borrow_mut().failure = None;

        let mut hint = 1i64 << 32;
        assert_eq!(
            Io::xFileControl(
                file.sqlite3_file(),
                SQLITE_FCNTL_SIZE_HINT,
                core::ptr::from_mut(&mut hint).cast()
            ),
            SQLITE_OK
        );
        assert_eq!(state.borrow().hints.as_slice(), &[1u64 << 32]);
        assert_eq!(
            Io::xFileControl(file.sqlite3_file(), -1, core::ptr::null_mut()),
            SQLITE_NOTFOUND
        );
        assert_eq!(Io::xSectorSize(file.sqlite3_file()), 8192);
        assert_eq!(
            Io::xDeviceCharacteristics(file.sqlite3_file()),
            SQLITE_IOCAP_SAFE_APPEND | SQLITE_IOCAP_UNDELETABLE_WHEN_OPEN
        );
        assert_eq!(Io::xClose(file.sqlite3_file()), SQLITE_OK);
    });
}

#[test]
fn test_lock_sync() {
    with_vfs(|vfs_ptr, state| unsafe {
        let mut file = open(vfs_ptr);

        for (raw, typed) in [
            (SQLITE_LOCK_SHARED, LockLevel::Shared),
            (SQLITE_LOCK_RESERVED, LockLevel::Reserved),
            (SQLITE_LOCK_PENDING, LockLevel::Pending),
            (SQLITE_LOCK_EXCLUSIVE, LockLevel::Exclusive),
        ] {
            assert_eq!(Io::xLock(file.sqlite3_file(), raw), SQLITE_OK);
            assert_eq!(state.borrow().locks.last(), Some(&typed));
        }
        for raw in [SQLITE_LOCK_NONE, -1, 99] {
            assert_eq!(Io::xLock(file.sqlite3_file(), raw), SQLITE_IOERR_LOCK);
        }
        assert_eq!(state.borrow().locks.len(), 4);

        for (raw, typed) in [
            (SQLITE_LOCK_SHARED, LockLevel::Shared),
            (SQLITE_LOCK_NONE, LockLevel::None),
        ] {
            assert_eq!(Io::xUnlock(file.sqlite3_file(), raw), SQLITE_OK);
            assert_eq!(state.borrow().unlocks.last(), Some(&typed));
        }
        for raw in [
            SQLITE_LOCK_RESERVED,
            SQLITE_LOCK_PENDING,
            SQLITE_LOCK_EXCLUSIVE,
            -1,
        ] {
            assert_eq!(Io::xUnlock(file.sqlite3_file(), raw), SQLITE_IOERR_UNLOCK);
        }
        assert_eq!(state.borrow().unlocks.len(), 2);

        let mut result = -1;
        for held in [false, true] {
            state.borrow_mut().reserved = held;
            assert_eq!(
                Io::xCheckReservedLock(file.sqlite3_file(), &mut result),
                SQLITE_OK
            );
            assert_eq!(result, i32::from(held));
        }

        for (raw, mode) in [
            (SQLITE_SYNC_NORMAL, SyncMode::Normal),
            (SQLITE_SYNC_FULL, SyncMode::Full),
        ] {
            for data_only in [false, true] {
                assert_eq!(
                    Io::xSync(
                        file.sqlite3_file(),
                        raw | if data_only { SQLITE_SYNC_DATAONLY } else { 0 }
                    ),
                    SQLITE_OK
                );
                assert_eq!(
                    state.borrow().syncs.last(),
                    Some(&SyncOptions { mode, data_only })
                );
            }
        }
        for raw in [0, 1, SQLITE_SYNC_DATAONLY, SQLITE_SYNC_NORMAL | 0x100, -1] {
            assert_eq!(Io::xSync(file.sqlite3_file(), raw), SQLITE_IOERR_FSYNC);
        }
        assert_eq!(state.borrow().syncs.len(), 4);
        state.borrow_mut().failure = Some(VfsErrorCode::Busy);
        assert_eq!(
            Io::xLock(file.sqlite3_file(), SQLITE_LOCK_EXCLUSIVE),
            SQLITE_BUSY
        );
        state.borrow_mut().failure = Some(VfsErrorCode::IoCheckReservedLock);
        assert_eq!(
            Io::xCheckReservedLock(file.sqlite3_file(), &mut result),
            SQLITE_IOERR_CHECKRESERVEDLOCK
        );
        assert_eq!(result, 0);
        state.borrow_mut().failure = None;
        assert_eq!(Io::xClose(file.sqlite3_file()), SQLITE_OK);
    });
}

#[test]
fn test_path_access_delete() {
    with_vfs(|vfs_ptr, state| unsafe {
        let mut output = [99u8; 6];
        assert_eq!(
            Vfs::xFullPathname(vfs_ptr, c"file".as_ptr(), 5, output.as_mut_ptr().cast()),
            SQLITE_OK
        );
        assert_eq!(&output, b"file\0c");
        for capacity in [-1, 0, 4] {
            output.fill(99);
            assert_eq!(
                Vfs::xFullPathname(
                    vfs_ptr,
                    c"file".as_ptr(),
                    capacity,
                    output.as_mut_ptr().cast()
                ),
                SQLITE_CANTOPEN
            );
            assert_eq!(output, [99; 6]);
        }

        for (flag, mode) in [
            (SQLITE_ACCESS_EXISTS, AccessMode::Exists),
            (SQLITE_ACCESS_READ, AccessMode::Read),
            (SQLITE_ACCESS_READWRITE, AccessMode::ReadWrite),
        ] {
            for (name, expected) in [(c"exists", 1), (c"missing", 0)] {
                let mut result = -1;
                assert_eq!(
                    Vfs::xAccess(vfs_ptr, name.as_ptr(), flag, &mut result),
                    SQLITE_OK
                );
                assert_eq!(result, expected);
                assert_eq!(state.borrow().accesses.last(), Some(&mode));
            }
        }
        for (name, flag) in [
            (c"exists".as_ptr(), -1),
            (core::ptr::null(), SQLITE_ACCESS_EXISTS),
            (c"\xff".as_ptr(), SQLITE_ACCESS_EXISTS),
        ] {
            let mut result = -1;
            assert_eq!(
                Vfs::xAccess(vfs_ptr, name, flag, &mut result),
                SQLITE_IOERR_ACCESS
            );
            assert_eq!(result, 0);
        }

        assert_eq!(state.borrow().accesses.len(), 6);
        for sync_dir in [0, 1] {
            assert_eq!(Vfs::xDelete(vfs_ptr, c"file".as_ptr(), sync_dir), SQLITE_OK);
        }
        assert_eq!(state.borrow().deletes.as_slice(), &[false, true]);

        assert_eq!(
            Vfs::xDelete(vfs_ptr, c"\xff".as_ptr(), 0),
            SQLITE_IOERR_DELETE
        );

        assert_eq!(
            Vfs::xFullPathname(vfs_ptr, c"\xff".as_ptr(), 6, output.as_mut_ptr().cast()),
            SQLITE_CANTOPEN
        );

        let mut file: SQLiteVfsFile = core::mem::zeroed();
        assert_eq!(
            Vfs::xOpen(
                vfs_ptr,
                c"\xff".as_ptr(),
                file.sqlite3_file(),
                SQLITE_OPEN_READWRITE,
                core::ptr::null_mut()
            ),
            SQLITE_CANTOPEN
        );
        assert!((*file.sqlite3_file()).pMethods.is_null());
        assert_eq!(
            Vfs::xFullPathname(vfs_ptr, c"nul".as_ptr(), 6, output.as_mut_ptr().cast()),
            SQLITE_CANTOPEN
        );
    });
}

#[test]
fn test_os_callbacks() {
    with_vfs(|vfs_ptr, state| unsafe {
        for length in [-1, 0] {
            assert_eq!(Vfs::xRandomness(vfs_ptr, length, core::ptr::null_mut()), 0);
        }
        for (count, expected) in [
            (4, [42, 42, 42, 42, 99]),
            (2, [42, 42, 0, 0, 99]),
            (0, [0, 0, 0, 0, 99]),
        ] {
            state.borrow_mut().random_count = Some(count);
            let mut bytes = [99u8; 5];
            assert_eq!(
                Vfs::xRandomness(vfs_ptr, 4, bytes.as_mut_ptr().cast()),
                count as i32
            );
            assert_eq!(bytes, expected);
        }

        for micros in [-1, 0, 1234] {
            assert_eq!(Vfs::xSleep(vfs_ptr, micros), micros.max(0));
        }
        assert_eq!(
            state.borrow().sleeps.as_slice(),
            &[Duration::from_micros(1234)]
        );

        let mut value = 0;
        for now in [-1, (1i64 << 53) | 1] {
            state.borrow_mut().now = now;
            assert_eq!(Vfs::xCurrentTimeInt64(vfs_ptr, &mut value), SQLITE_OK);
            assert_eq!(value, 210_866_760_000_000 + now);
        }

        state.borrow_mut().now = 0;
        let mut days = 0.0;
        assert_eq!(Vfs::xCurrentTime(vfs_ptr, &mut days), SQLITE_OK);
        assert_eq!(days, 2440587.5);

        state.borrow_mut().now = i64::MAX;
        assert_eq!(Vfs::xCurrentTimeInt64(vfs_ptr, &mut value), SQLITE_ERROR);
        assert_eq!(value, 0);

        state.borrow_mut().failure = Some(VfsErrorCode::Error);
        assert_eq!(Vfs::xCurrentTime(vfs_ptr, &mut days), SQLITE_ERROR);
        assert_eq!(days, 0.0);
        assert_eq!(Vfs::xCurrentTimeInt64(vfs_ptr, &mut value), SQLITE_ERROR);
        assert_eq!(value, 0);
    });
}

#[test]
fn test_error_reporting() {
    with_vfs(|vfs_ptr, state| unsafe {
        let mut file = open(vfs_ptr);
        let mut output = [99u8; 12];
        let mut result = -1;
        let system_code = SystemErrorCode::from_raw(1234).unwrap();
        state.borrow_mut().system_error = Some(system_code);
        let extended = VfsErrorCode::from_raw(SQLITE_IOERR_AUTH).unwrap();
        state.borrow_mut().failure = Some(extended);
        assert_eq!(
            Io::xSync(file.sqlite3_file(), SQLITE_SYNC_FULL),
            SQLITE_IOERR_AUTH
        );
        assert_eq!(state.borrow_mut().error.take().unwrap().code(), extended);
        assert_eq!(
            Io::xUnlock(file.sqlite3_file(), SQLITE_LOCK_NONE),
            SQLITE_IOERR_AUTH
        );
        assert_eq!(
            Vfs::xAccess(vfs_ptr, c"file".as_ptr(), SQLITE_ACCESS_EXISTS, &mut result),
            SQLITE_IOERR_AUTH
        );
        assert_eq!(result, 0);
        assert_eq!(
            Vfs::xFullPathname(vfs_ptr, c"file".as_ptr(), 8, output.as_mut_ptr().cast()),
            SQLITE_IOERR_AUTH
        );
        assert_eq!(
            Vfs::xDelete(vfs_ptr, c"file".as_ptr(), 1),
            SQLITE_IOERR_AUTH
        );
        for (capacity, expected) in [
            (1, ""),
            (2, "a"),
            (3, "a"),
            (4, "a"),
            (5, "a中"),
            (6, "a中b"),
            (11, "a中b"),
        ] {
            output.fill(99);
            assert_eq!(
                Vfs::xGetLastError(vfs_ptr, capacity, output.as_mut_ptr().cast()),
                system_code.as_raw()
            );
            assert_eq!(
                CStr::from_ptr(output.as_ptr().cast()).to_str().unwrap(),
                expected
            );
            assert_eq!(output[capacity as usize], 99);
        }
        for capacity in [-1, 0] {
            output.fill(99);
            assert_eq!(
                Vfs::xGetLastError(vfs_ptr, capacity, output.as_mut_ptr().cast()),
                system_code.as_raw()
            );
            assert_eq!(output, [99; 12]);
        }
        assert_eq!(state.borrow_mut().error.take().unwrap().message(), "a中b");
        assert_eq!(
            Vfs::xGetLastError(vfs_ptr, 12, output.as_mut_ptr().cast()),
            SQLITE_OK
        );
        assert_eq!(output[0], 0);

        // A new failure must replace stale OS diagnostics.
        assert_eq!(
            Vfs::xAccess(vfs_ptr, c"file".as_ptr(), -1, &mut result),
            SQLITE_IOERR_ACCESS
        );
        assert_eq!(state.borrow().error.as_ref().unwrap().system_error(), None);
        assert_eq!(Vfs::xGetLastError(vfs_ptr, 0, core::ptr::null_mut()), 0);

        state.borrow_mut().failure = None;
        assert_eq!(Io::xClose(file.sqlite3_file()), SQLITE_OK);
    });
}

#[test]
fn test_optional_callbacks() {
    with_vfs(|vfs_ptr, _| unsafe {
        let vfs = &*vfs_ptr;
        assert_eq!(vfs.iVersion, 3);
        assert_eq!(Io::METHODS.iVersion, 3);
        assert!(Io::METHODS.xShmMap.is_none());
        assert!(vfs.xSetSystemCall.is_none());

        let mut opened = open(vfs_ptr);
        let file = opened.sqlite3_file();
        assert_eq!(
            Io::xFileControl(file, -1, core::ptr::null_mut()),
            SQLITE_NOTFOUND
        );
        assert!(vfs.xDlOpen.unwrap()(vfs_ptr, c"extension".as_ptr()).is_null());

        let mut message = [99u8; 64];
        vfs.xDlError.unwrap()(vfs_ptr, 64, message.as_mut_ptr().cast());
        assert_eq!(
            CStr::from_ptr(message.as_ptr().cast()).to_str().unwrap(),
            "dynamic extension loading is not supported"
        );

        let mut mapped = core::ptr::NonNull::dangling().as_ptr();
        assert_eq!(
            Io::METHODS.xFetch.unwrap()(file, 0, 512, &mut mapped),
            SQLITE_OK
        );
        assert!(mapped.is_null());
        assert_eq!(Io::METHODS.xUnfetch.unwrap()(file, 0, mapped), SQLITE_OK);
        assert!(vfs.xDlSym.is_none());

        assert_eq!(Io::xClose(file), SQLITE_OK);
    });
}
