use rsqlite_vfs::{test_suite::test_vfs_store, *};

#[test]
fn random_name_is_valid() {
    fn random(buf: &mut [u8]) -> usize {
        rand::fill(buf);
        buf.len()
    }
    let name_1 = random_name(random).unwrap();
    let name_2 = random_name(random).unwrap();
    assert!(name_1.is_ascii(), "Expected an ascii-name: `{name_1}`");
    assert!(name_2.is_ascii(), "Expected an ascii-name: `{name_2}`");
    assert_ne!(name_1, name_2);
    assert!(random_name(|_| 0).is_err());
    assert!(random_name(|buf| buf.len() - 1).is_err());
    assert!(random_name(|buf| buf.len() + 1).is_err());
}

#[test]
fn failed_store_checks_close_and_remove_the_created_file() {
    use core::cell::Cell;
    use rsqlite_vfs::{MemChunksFile, OpenRequest, OpenedFile};
    use std::{rc::Rc, string::String};

    #[derive(Clone, Copy, Default, PartialEq)]
    enum Behavior {
        #[default]
        AccessError,
        AccessPanic,
        ReopenError,
        WrongAccess,
        UnexpectedOpen,
        UnlinkAtOpen,
        IgnoreDeleteOnClose,
    }

    #[derive(Default)]
    struct State {
        opened: Cell<bool>,
        closes: Cell<usize>,
        deletes: Cell<usize>,
        behavior: Behavior,
    }
    struct Store;
    impl VfsStore for Store {
        type AppData = Rc<State>;
        type File = MemChunksFile;

        fn open_file(
            data: &Rc<State>,
            request: OpenRequest<'_>,
        ) -> VfsResult<OpenedFile<Self::File>> {
            if data.behavior == Behavior::ReopenError && data.closes.get() != 0 {
                return Err(
                    VfsError::new(VfsErrorCode::CantOpen, "reopen failed".into())
                        .with_system_error(SystemErrorCode::from_raw(13).unwrap()),
                );
            }
            data.opened.set(data.behavior != Behavior::UnlinkAtOpen);
            Ok(OpenedFile {
                file: MemChunksFile::default(),
                access: if data.behavior == Behavior::WrongAccess {
                    OpenAccess::ReadOnly
                } else {
                    request.options.access()
                },
            })
        }

        fn close_file(
            data: &Rc<State>,
            _: Option<&str>,
            _: Self::File,
            options: OpenOptions,
        ) -> VfsResult<()> {
            data.closes.set(data.closes.get() + 1);
            if matches!(data.behavior, Behavior::AccessError | Behavior::AccessPanic) {
                return Err(VfsError::new(
                    VfsErrorCode::IoClose,
                    "cleanup failed".into(),
                ));
            }
            if options.delete_on_close() && data.behavior != Behavior::IgnoreDeleteOnClose {
                data.opened.set(false);
            }
            Ok(())
        }

        fn access(data: &Rc<State>, _: &str, _: AccessMode) -> VfsResult<bool> {
            if !data.opened.get() {
                return Ok(false);
            }
            assert!(
                data.behavior != Behavior::AccessPanic,
                "injected check panic"
            );
            if data.behavior == Behavior::AccessError {
                Err(VfsError::new(VfsErrorCode::IoAccess, "check failed".into()))
            } else {
                Ok(true)
            }
        }

        fn full_pathname(_: &Rc<State>, name: &str) -> VfsResult<String> {
            Ok(name.into())
        }

        fn delete_file(data: &Rc<State>, _: &str, _: bool) -> VfsResult<()> {
            data.deletes.set(data.deletes.get() + 1);
            data.opened.set(false);
            Ok(())
        }
    }
    for behavior in [Behavior::AccessError, Behavior::AccessPanic] {
        let state = Rc::new(State {
            behavior,
            ..State::default()
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            test_vfs_store::<Store>(VfsAppData::new(state.clone()))
        }));
        if behavior == Behavior::AccessPanic {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap().unwrap_err().code(), VfsErrorCode::IoAccess);
        }
        assert_eq!(state.closes.get(), 1);
        assert_eq!(state.deletes.get(), 1);
        assert!(!state.opened.get());
    }

    use rsqlite_vfs::test_suite::{
        test_vfs_store_delete_on_close, test_vfs_store_open_modes, test_vfs_store_reopen,
    };
    // Cleanup must survive a closed handle followed by a failed reopen, and
    // context must not erase the backend's original error codes.
    let state = Rc::new(State {
        behavior: Behavior::ReopenError,
        ..State::default()
    });
    let error = test_vfs_store_reopen::<Store>(&state).unwrap_err();
    assert_eq!(error.code(), VfsErrorCode::CantOpen);
    assert_eq!(error.system_error().unwrap().as_raw(), 13);
    assert!(error.message().contains("reopen failed"));
    assert_eq!(
        (state.closes.get(), state.deletes.get(), state.opened.get()),
        (1, 1, false)
    );

    // Even a successful open with the wrong mode or unexpected success must
    // have its handle closed and newly created name cleaned up.
    for behavior in [Behavior::WrongAccess, Behavior::UnexpectedOpen] {
        let state = Rc::new(State {
            behavior,
            ..State::default()
        });
        let error = if behavior == Behavior::WrongAccess {
            test_vfs_store::<Store>(VfsAppData::new(state.clone())).unwrap_err()
        } else {
            test_vfs_store_open_modes::<Store>(&state).unwrap_err()
        };
        assert_eq!(error.code(), VfsErrorCode::Io);
        assert!(error.message().contains("expected"));
        assert!(error.message().contains("got"));
        assert_eq!(
            (state.closes.get(), state.deletes.get(), state.opened.get()),
            (1, 1, false)
        );
    }

    // A valid early unlink must not cause a second delete; an ignored
    // DELETEONCLOSE must be detected before fallback cleanup hides the defect.
    for behavior in [Behavior::UnlinkAtOpen, Behavior::IgnoreDeleteOnClose] {
        let state = Rc::new(State {
            behavior,
            ..State::default()
        });
        let result = test_vfs_store_delete_on_close::<Store>(&state);
        if behavior == Behavior::UnlinkAtOpen {
            result.unwrap();
            assert_eq!(state.deletes.get(), 0);
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.code(), VfsErrorCode::Io);
            assert!(error.message().contains("DELETEONCLOSE"));
            assert_eq!(state.deletes.get(), 1);
        }
        assert_eq!(state.closes.get(), 1);
        assert!(!state.opened.get());
    }
}
