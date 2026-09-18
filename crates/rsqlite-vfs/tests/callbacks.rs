use rsqlite_vfs::{ffi::*, *};
use std::{cell::RefCell, ffi::CStr};

struct CallbackOs<const NOW: i64>;
impl<const NOW: i64> OsCallback for CallbackOs<NOW> {
    fn sleep(&self, _: core::time::Duration) {}

    fn random(&self, buf: &mut [u8]) -> usize {
        assert!(!buf.is_empty());
        buf.fill(42);
        buf.len()
    }

    fn epoch_timestamp_in_ms(&self) -> VfsResult<i64> {
        Ok(NOW)
    }
}

struct CallbackStore;
impl rsqlite_vfs::VfsStore for CallbackStore {
    type File = MemChunksFile;

    fn close_file(
        _: &Self::AppData,
        _: Option<&str>,
        _: Self::File,
        _: OpenOptions,
    ) -> rsqlite_vfs::VfsResult<()> {
        Ok(())
    }

    type AppData = RefCell<Option<rsqlite_vfs::VfsError>>;

    fn record_error(data: &Self::AppData, error: rsqlite_vfs::VfsError) {
        data.replace(Some(error));
    }

    fn last_error(data: &Self::AppData) -> Option<rsqlite_vfs::VfsError> {
        data.borrow().clone()
    }

    fn open_file(
        _: &RefCell<Option<rsqlite_vfs::VfsError>>,
        request: rsqlite_vfs::OpenRequest<'_>,
    ) -> rsqlite_vfs::VfsResult<rsqlite_vfs::OpenedFile<MemChunksFile>> {
        Ok(rsqlite_vfs::OpenedFile {
            file: MemChunksFile::new(512),
            access: request.options.access(),
        })
    }

    fn access(
        _: &RefCell<Option<rsqlite_vfs::VfsError>>,
        name: &str,
        _mode: AccessMode,
    ) -> rsqlite_vfs::VfsResult<bool> {
        if name == "error" {
            Err(rsqlite_vfs::VfsError::new(
                VfsErrorCode::IoAccess,
                "access failed".into(),
            ))
        } else {
            Ok(name == "exists")
        }
    }

    fn full_pathname(
        _: &RefCell<Option<rsqlite_vfs::VfsError>>,
        name: &str,
    ) -> rsqlite_vfs::VfsResult<std::string::String> {
        Ok(name.into())
    }

    fn delete_file(
        _: &RefCell<Option<rsqlite_vfs::VfsError>>,
        _: &str,
        _: bool,
    ) -> rsqlite_vfs::VfsResult<()> {
        Err(rsqlite_vfs::VfsError::new(
            VfsErrorCode::IoDelete,
            "delete failed".into(),
        ))
    }
}

struct CallbackIo;
impl rsqlite_vfs::SQLiteIoMethods for CallbackIo {
    type Store = CallbackStore;
    const VERSION: i32 = 3;
}

struct CallbackVfs<const NOW: i64 = 0>;
impl<const NOW: i64> rsqlite_vfs::SQLiteVfs<CallbackIo> for CallbackVfs<NOW> {
    type Os = CallbackOs<NOW>;

    fn os(_: &RefCell<Option<rsqlite_vfs::VfsError>>) -> &Self::Os {
        &CallbackOs
    }

    const VERSION: i32 = 3;
}

#[test]
fn callbacks_delegate_typed_requests_and_preserve_backend_errors() {
    use std::rc::Rc;

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
        reserved: bool,
    }
    impl State {
        fn result(&self) -> VfsResult<()> {
            match self.failure {
                Some(code) => Err(VfsError::new(code, "backend error".into())),
                None => Ok(()),
            }
        }
    }

    type Data = Rc<RefCell<State>>;
    struct File(Data);
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

        fn read(&mut self, _: &mut [u8], _: u64) -> VfsResult<usize> {
            unreachable!()
        }

        fn write(&mut self, _: &[u8], _: u64) -> VfsResult<()> {
            unreachable!()
        }

        fn truncate(&mut self, _: u64) -> VfsResult<()> {
            unreachable!()
        }

        fn size(&self) -> VfsResult<u64> {
            unreachable!()
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
            drop(file);
            if options.delete_on_close() {
                Self::delete_file(data, name.unwrap(), false)?;
            }
            Ok(())
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
            data.borrow().result()?;
            Ok(OpenedFile {
                file: File(data.clone()),
                access: data.borrow().actual_access.unwrap_or(options.access()),
            })
        }

        fn access(data: &Data, _: &str, mode: AccessMode) -> VfsResult<bool> {
            data.borrow_mut().accesses.push(mode);
            data.borrow().result()?;
            Ok(mode != AccessMode::ReadWrite)
        }

        fn full_pathname(data: &Data, name: &str) -> VfsResult<String> {
            data.borrow().result()?;
            Ok(if name == "nul" {
                "bad\0name".into()
            } else {
                std::format!("/{name}")
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
        const VERSION: i32 = 1;
    }
    struct Vfs;
    impl SQLiteVfs<Io> for Vfs {
        type Os = CallbackOs<0>;

        fn os(_: &Data) -> &Self::Os {
            &CallbackOs
        }

        const VERSION: i32 = 1;
    }

    let state = Rc::new(RefCell::new(State::default()));
    let mut data = VfsAppData::new(state.clone());
    unsafe {
        let mut vfs = Vfs::vfs(c"delegates".as_ptr(), &mut data);
        // Open files retain this pointer. Reuse it without creating new
        // exclusive references that invalidate the stored pointer.
        let vfs_ptr = core::ptr::from_mut(&mut vfs);
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
        let opened = state.borrow().opens[0];
        assert_eq!(opened.kind(), Some(FileKind::TempDb));
        assert_eq!(opened.access(), OpenAccess::ReadWrite);
        assert!(opened.create() && opened.exclusive() && opened.delete_on_close());

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
        for (raw, typed, expected) in [
            (SQLITE_ACCESS_EXISTS, AccessMode::Exists, 1),
            (SQLITE_ACCESS_READ, AccessMode::Read, 1),
            (SQLITE_ACCESS_READWRITE, AccessMode::ReadWrite, 0),
        ] {
            assert_eq!(
                Vfs::xAccess(vfs_ptr, c"file".as_ptr(), raw, &mut result),
                SQLITE_OK
            );
            assert_eq!(result, expected);
            assert_eq!(state.borrow().accesses.last(), Some(&typed));
        }
        let mut output = [99u8; 8];
        assert_eq!(
            Vfs::xFullPathname(vfs_ptr, c"file".as_ptr(), 8, output.as_mut_ptr().cast()),
            SQLITE_OK
        );
        assert_eq!(CStr::from_ptr(output.as_ptr().cast()), c"/file");
        assert_eq!(
            Vfs::xFullPathname(vfs_ptr, c"file".as_ptr(), 5, output.as_mut_ptr().cast()),
            SQLITE_CANTOPEN
        );
        assert_eq!(
            Vfs::xFullPathname(vfs_ptr, c"nul".as_ptr(), 8, output.as_mut_ptr().cast()),
            SQLITE_CANTOPEN
        );
        for sync_dir in [0, 1] {
            assert_eq!(Vfs::xDelete(vfs_ptr, c"file".as_ptr(), sync_dir), SQLITE_OK);
        }

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
        let extended = VfsErrorCode::from_raw(SQLITE_IOERR_AUTH).unwrap();
        state.borrow_mut().failure = Some(extended);
        assert_eq!(
            Io::xSync(file.sqlite3_file(), SQLITE_SYNC_FULL),
            SQLITE_IOERR_AUTH
        );
        assert_eq!(data.borrow_mut().error.take().unwrap().code(), extended);
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
        state.borrow_mut().failure = None;
        assert_eq!(Io::xClose(file.sqlite3_file()), SQLITE_OK);
        assert_eq!(
            state.borrow().deletes.as_slice(),
            &[false, true, true, false]
        );

        let open_count = state.borrow().opens.len();
        for invalid in [
            0,
            SQLITE_OPEN_READONLY | SQLITE_OPEN_CREATE,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_EXCLUSIVE,
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
            assert!(file.io_methods.pMethods.is_null());
            assert_eq!(out_flags, -1);
        }
        assert_eq!(state.borrow().opens.len(), open_count);
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
    }
}

#[test]
fn default_optional_methods_decline_unsupported_features() {
    // A custom loader must be assignable with SQLite's exact C signature.
    unsafe extern "C" fn find_symbol(
        _: *mut sqlite3_vfs,
        _: *mut core::ffi::c_void,
        _: *const core::ffi::c_char,
    ) -> Option<unsafe extern "C" fn()> {
        None
    }
    let mut data = VfsAppData::new(RefCell::new(None));
    let mut vfs = unsafe { CallbackVfs::<0>::vfs(c"callbacks".as_ptr(), &mut data) };
    let vfs_ptr = core::ptr::from_mut(&mut vfs);
    let file = core::ptr::null_mut();
    unsafe {
        assert!(vfs.xDlOpen.unwrap()(vfs_ptr, c"extension".as_ptr()).is_null());
        let mut message = [99u8; 64];
        vfs.xDlError.unwrap()(vfs_ptr, 64, message.as_mut_ptr().cast());
        assert_eq!(
            CStr::from_ptr(message.as_ptr().cast()).to_str().unwrap(),
            "dynamic extension loading is not supported"
        );
        let mut mapped = core::ptr::dangling_mut();
        assert_eq!(
            CallbackIo::METHODS.xFetch.unwrap()(file, 0, 512, &mut mapped),
            SQLITE_OK
        );
        assert!(mapped.is_null());
        assert_eq!(
            CallbackIo::METHODS.xUnfetch.unwrap()(file, 0, mapped),
            SQLITE_OK
        );
        vfs.xDlSym = Some(find_symbol);
        assert!(vfs.xDlSym.unwrap()(vfs_ptr, core::ptr::null_mut(), c"entry".as_ptr()).is_none());
    }
}

#[test]
fn pathname_and_access_follow_flat_namespace_contract() {
    let data = VfsAppData::new(RefCell::new(None));
    let mut vfs = unsafe {
        CallbackVfs::<0>::vfs(c"callbacks".as_ptr(), core::ptr::from_ref(&data).cast_mut())
    };
    let seed_error = || {
        CallbackStore::record_error(
            &data,
            VfsError::new(VfsErrorCode::IoRead, "previous read failure".into())
                .with_system_error(SystemErrorCode::from_raw(13).unwrap()),
        )
    };
    let check_error = |code| {
        let error = CallbackStore::last_error(&data).unwrap();
        assert_eq!(error.code(), code);
        assert_eq!(error.system_error(), None);
        assert_ne!(error.message(), "previous read failure");
    };
    let mut output = [99u8; 6];
    unsafe {
        assert_eq!(
            CallbackVfs::<0>::xFullPathname(
                &mut vfs,
                c"file".as_ptr(),
                5,
                output.as_mut_ptr().cast()
            ),
            SQLITE_OK
        );
        assert_eq!(&output, b"file\0c");
        for capacity in [-1, 0, 4] {
            seed_error();
            output.fill(99);
            assert_eq!(
                CallbackVfs::<0>::xFullPathname(
                    &mut vfs,
                    c"file".as_ptr(),
                    capacity,
                    output.as_mut_ptr().cast()
                ),
                SQLITE_CANTOPEN
            );
            assert_eq!(output, [99; 6]);
            check_error(VfsErrorCode::CantOpen);
        }
        for flag in [
            SQLITE_ACCESS_EXISTS,
            SQLITE_ACCESS_READ,
            SQLITE_ACCESS_READWRITE,
        ] {
            for (name, expected) in [(c"exists", 1), (c"missing", 0)] {
                let mut result = -1;
                assert_eq!(
                    CallbackVfs::<0>::xAccess(&mut vfs, name.as_ptr(), flag, &mut result),
                    SQLITE_OK
                );
                assert_eq!(result, expected);
            }
        }
        for (name, flag) in [
            (c"error".as_ptr(), SQLITE_ACCESS_EXISTS),
            (c"exists".as_ptr(), -1),
            (core::ptr::null(), SQLITE_ACCESS_EXISTS),
            (c"\xff".as_ptr(), SQLITE_ACCESS_EXISTS),
        ] {
            seed_error();
            let mut result = -1;
            assert_eq!(
                CallbackVfs::<0>::xAccess(&mut vfs, name, flag, &mut result),
                SQLITE_IOERR_ACCESS
            );
            assert_eq!(result, 0);
            check_error(VfsErrorCode::IoAccess);
        }
        assert_eq!(
            CallbackVfs::<0>::xDelete(&mut vfs, c"error".as_ptr(), 1),
            SQLITE_IOERR_DELETE
        );
        assert_eq!(data.borrow_mut().take().unwrap().message(), "delete failed");
        seed_error();
        assert_eq!(
            CallbackVfs::<0>::xDelete(&mut vfs, c"\xff".as_ptr(), 0),
            SQLITE_IOERR_DELETE
        );
        check_error(VfsErrorCode::IoDelete);
        seed_error();
        assert_eq!(
            CallbackVfs::<0>::xFullPathname(
                &mut vfs,
                c"\xff".as_ptr(),
                6,
                output.as_mut_ptr().cast()
            ),
            SQLITE_CANTOPEN
        );
        check_error(VfsErrorCode::CantOpen);
        seed_error();
        let mut file: SQLiteVfsFile = core::mem::zeroed();
        assert_eq!(
            CallbackVfs::<0>::xOpen(
                &mut vfs,
                c"\xff".as_ptr(),
                file.sqlite3_file(),
                SQLITE_OPEN_READWRITE,
                core::ptr::null_mut()
            ),
            SQLITE_CANTOPEN
        );
        assert!(file.io_methods.pMethods.is_null());
        check_error(VfsErrorCode::CantOpen);
    }
}

#[test]
fn last_error_queries_preserve_utf8_and_do_not_consume_the_error() {
    let data = VfsAppData::new(RefCell::new(None));
    let mut vfs = unsafe {
        CallbackVfs::<0>::vfs(c"callbacks".as_ptr(), core::ptr::from_ref(&data).cast_mut())
    };
    let mut output = [99u8; 12];
    unsafe {
        assert_eq!(
            CallbackVfs::<0>::xGetLastError(&mut vfs, 12, output.as_mut_ptr().cast()),
            SQLITE_OK
        );
        assert_eq!(output[0], 0);
        CallbackStore::record_error(&data, VfsError::new(VfsErrorCode::IoRead, "a中b".into()));
        assert_eq!(
            CallbackVfs::<0>::xGetLastError(&mut vfs, 0, core::ptr::null_mut()),
            0
        );
        // Attaching an OS error changes diagnostics, not the SQLite result.
        let system_code = SystemErrorCode::from_raw(1234).unwrap();
        let error =
            VfsError::new(VfsErrorCode::IoRead, "a中b".into()).with_system_error(system_code);
        assert_eq!(error.raw_code(), SQLITE_IOERR_READ);
        assert_eq!(error.system_error(), Some(system_code));
        assert_eq!(SystemErrorCode::from_raw(0), None);
        CallbackStore::record_error(&data, error);
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
                CallbackVfs::<0>::xGetLastError(&mut vfs, capacity, output.as_mut_ptr().cast()),
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
                CallbackVfs::<0>::xGetLastError(&mut vfs, capacity, output.as_mut_ptr().cast()),
                system_code.as_raw()
            );
            assert_eq!(output, [99; 12]);
        }
        assert_eq!(data.borrow_mut().take().unwrap().message(), "a中b");
        assert_eq!(
            CallbackVfs::<0>::xGetLastError(&mut vfs, 12, output.as_mut_ptr().cast()),
            SQLITE_OK
        );
        assert_eq!(output[0], 0);
    }
}

#[test]
fn randomness_handles_empty_buffers_and_time_preserves_integer_precision() {
    let mut data = VfsAppData::new(RefCell::new(None));
    let mut vfs = unsafe { CallbackVfs::<0>::vfs(c"callbacks".as_ptr(), &mut data) };
    let ptr = core::ptr::from_mut(&mut vfs);
    unsafe {
        for length in [-1, 0] {
            assert_eq!(
                CallbackVfs::<0>::xRandomness(ptr, length, core::ptr::null_mut()),
                0
            );
        }
        let mut bytes = [99u8; 5];
        assert_eq!(
            CallbackVfs::<0>::xRandomness(ptr, 4, bytes.as_mut_ptr().cast()),
            4
        );
        assert_eq!(bytes, [42, 42, 42, 42, 99]);
        let mut value = 0;
        assert_eq!(
            CallbackVfs::<-1>::xCurrentTimeInt64(ptr, &mut value),
            SQLITE_OK
        );
        assert_eq!(value, 210_866_759_999_999);
        const LARGE: i64 = 1i64 << 53 | 1;
        assert_eq!(
            CallbackVfs::<LARGE>::xCurrentTimeInt64(ptr, &mut value),
            SQLITE_OK
        );
        assert_eq!(value, 210_866_760_000_000 + LARGE);
        assert_eq!(
            CallbackVfs::<{ i64::MAX }>::xCurrentTimeInt64(ptr, &mut value),
            SQLITE_ERROR
        );
        assert_eq!(value, 0);
    }
}

#[test]
fn xread_handles_counts_and_errors() {
    // Deliberately leaves the unread tail untouched, and can report an
    // invalid count or an error to exercise the common callback.
    struct ReadFile(Result<usize, i32>);
    impl VfsFile for ReadFile {
        fn read(&mut self, buf: &mut [u8], offset: u64) -> VfsResult<usize> {
            assert_eq!(offset, 1u64 << 32);
            assert!(buf.iter().all(|&byte| byte == 0));
            let count = self.0.map_err(|code| {
                VfsError::new(VfsErrorCode::from_raw(code).unwrap(), "read failed".into())
            })?;
            let written = count.min(buf.len());
            buf[..written].fill(42);
            Ok(count)
        }

        fn write(&mut self, _: &[u8], _: u64) -> VfsResult<()> {
            unreachable!()
        }

        fn truncate(&mut self, _: u64) -> VfsResult<()> {
            unreachable!()
        }

        fn sync(&mut self, _: SyncOptions) -> VfsResult<()> {
            unreachable!()
        }

        fn lock(&mut self, _: LockLevel) -> VfsResult<()> {
            unreachable!()
        }

        fn unlock(&mut self, _: LockLevel) -> VfsResult<()> {
            unreachable!()
        }

        fn check_reserved_lock(&self) -> VfsResult<bool> {
            unreachable!()
        }

        fn size(&self) -> VfsResult<u64> {
            unreachable!()
        }
    }

    struct ReadStore;
    impl VfsStore for ReadStore {
        type File = ReadFile;

        fn close_file(
            _: &Self::AppData,
            _: Option<&str>,
            _: Self::File,
            _: OpenOptions,
        ) -> VfsResult<()> {
            unreachable!()
        }

        type AppData = RefCell<Option<VfsError>>;

        fn record_error(data: &Self::AppData, error: VfsError) {
            data.replace(Some(error));
        }

        fn last_error(data: &Self::AppData) -> Option<VfsError> {
            data.borrow().clone()
        }

        fn open_file(_: &Self::AppData, _: OpenRequest<'_>) -> VfsResult<OpenedFile<ReadFile>> {
            unreachable!()
        }

        fn access(_: &Self::AppData, _: &str, _: AccessMode) -> VfsResult<bool> {
            unreachable!()
        }

        fn full_pathname(_: &Self::AppData, _: &str) -> VfsResult<String> {
            unreachable!()
        }

        fn delete_file(_: &Self::AppData, _: &str, _: bool) -> VfsResult<()> {
            unreachable!()
        }
    }

    struct ReadIo;
    impl SQLiteIoMethods for ReadIo {
        type Store = ReadStore;
        const VERSION: i32 = 1;
    }

    let read = |result, buf: &mut [u8], expected| {
        let mut data = VfsAppData::new(RefCell::new((expected != SQLITE_OK).then(|| {
            VfsError::new(VfsErrorCode::IoRead, "previous read failure".into())
                .with_system_error(SystemErrorCode::from_raw(13).unwrap())
        })));
        let mut handle = ReadFile(result);
        let mut vfs: sqlite3_vfs = unsafe { core::mem::zeroed() };
        vfs.pAppData = core::ptr::from_mut(&mut data).cast();
        let mut file = SQLiteVfsFile {
            io_methods: sqlite3_file {
                pMethods: &ReadIo::METHODS,
            },
            vfs: &mut vfs,
            flags: 0,
            name_ptr: b"read.db".as_ptr(),
            name_length: 7,
            handle_ptr: core::ptr::from_mut(&mut handle).cast(),
        };
        let code = unsafe {
            ReadIo::xRead(
                file.sqlite3_file(),
                buf.as_mut_ptr().cast(),
                buf.len() as i32,
                1i64 << 32,
            )
        };
        assert_eq!(code, expected);
        let error = data.borrow_mut().take();
        if let Some(error) = &error {
            assert_eq!(error.system_error(), None);
        }
        error
    };

    for (count, code, expected) in [
        (4, SQLITE_OK, [42; 4]),
        (2, SQLITE_IOERR_SHORT_READ, [42, 42, 0, 0]),
        (0, SQLITE_IOERR_SHORT_READ, [0; 4]),
    ] {
        let mut buf = [99; 4];
        let error = read(Ok(count), &mut buf, code);
        if code == SQLITE_OK {
            assert!(error.is_none());
        } else {
            assert_eq!(error.unwrap().code(), VfsErrorCode::IoShortRead);
        }
        assert_eq!(buf, expected);
    }
    assert!(read(Ok(0), &mut [], SQLITE_OK).is_none());
    let error = read(Ok(5), &mut [99; 4], SQLITE_IOERR_READ).unwrap();
    assert_eq!(error.code(), VfsErrorCode::IoRead);
    let error = read(Err(SQLITE_IOERR_READ), &mut [99; 4], SQLITE_IOERR_READ).unwrap();
    assert_eq!(error.code(), VfsErrorCode::IoRead);
    assert_eq!(error.message(), "read failed");
    let mut buf = [99; 4];
    let error = read(Err(SQLITE_IOERR_SHORT_READ), &mut buf, SQLITE_IOERR_READ).unwrap();
    assert_eq!(error.code(), VfsErrorCode::IoRead);
}
