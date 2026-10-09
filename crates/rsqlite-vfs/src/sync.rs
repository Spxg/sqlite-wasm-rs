//! Sharing primitives for memvfs, single-threaded by default and guarded by SQLite's mutexes with `threadsafe`.
//!
//! SQLite's mutexes keep the crate `no_std` and let the host decide how a waiting thread blocks.

#[cfg(not(feature = "threadsafe"))]
mod imp {
    use crate::{VfsError, VfsResult};
    use alloc::rc::Rc;
    use core::cell::{RefCell, RefMut};

    pub(crate) use alloc::rc::Rc as Shared;

    /// A value borrowed exclusively on one thread.
    pub(crate) struct Locked<T>(RefCell<T>);

    impl<T> Locked<T> {
        pub(crate) fn new(value: T) -> VfsResult<Self> {
            Ok(Self(RefCell::new(value)))
        }

        pub(crate) fn lock(&self) -> RefMut<'_, T> {
            self.0.borrow_mut()
        }
    }

    /// The last VFS error, shared by every handle to one memvfs.
    #[derive(Clone)]
    pub(crate) struct LastError(Rc<RefCell<Option<VfsError>>>);

    impl LastError {
        pub(crate) fn new() -> Self {
            Self(Rc::default())
        }

        pub(crate) fn set(&self, error: VfsError) {
            self.0.replace(Some(error));
        }

        pub(crate) fn get(&self) -> Option<VfsError> {
            self.0.borrow().clone()
        }
    }
}

#[cfg(feature = "threadsafe")]
mod imp {
    use crate::ffi::{
        sqlite3_mutex, sqlite3_mutex_alloc, sqlite3_mutex_enter, sqlite3_mutex_free,
        sqlite3_mutex_leave, SQLITE_MUTEX_FAST,
    };
    use crate::{VfsError, VfsErrorCode, VfsResult};
    use core::cell::{RefCell, UnsafeCell};
    use core::marker::PhantomData;
    use core::ops::{Deref, DerefMut};
    use core::ptr::NonNull;

    pub(crate) use alloc::sync::Arc as Shared;

    /// A value guarded by a `SQLITE_MUTEX_FAST` mutex from SQLite's configured mutex methods.
    pub(crate) struct Locked<T> {
        mutex: NonNull<sqlite3_mutex>,
        value: UnsafeCell<T>,
    }

    // SAFETY: `value` is only reachable through `lock`, which holds `mutex`, so sharing or
    // moving a `Locked<T>` hands the value between threads, which `T: Send` permits.
    unsafe impl<T: Send> Send for Locked<T> {}
    // SAFETY: as for `Send`, every access to `value` is serialized by `mutex`.
    unsafe impl<T: Send> Sync for Locked<T> {}

    impl<T> Locked<T> {
        pub(crate) fn new(value: T) -> VfsResult<Self> {
            // SAFETY: allocating a fast mutex has no precondition, SQLite initializes itself first if needed.
            let mutex = NonNull::new(unsafe { sqlite3_mutex_alloc(SQLITE_MUTEX_FAST) })
                .ok_or_else(|| {
                    VfsError::new(VfsErrorCode::NoMemory, "unable to allocate a mutex".into())
                })?;
            Ok(Self {
                mutex,
                value: UnsafeCell::new(value),
            })
        }

        pub(crate) fn lock(&self) -> Guard<'_, T> {
            // SAFETY: `mutex` came from `sqlite3_mutex_alloc` and lives until `drop`.
            unsafe { sqlite3_mutex_enter(self.mutex.as_ptr()) };
            Guard {
                locked: self,
                _not_send: PhantomData,
            }
        }
    }

    impl<T> Drop for Locked<T> {
        fn drop(&mut self) {
            // SAFETY: `&mut self` proves no guard is alive, and the mutex is freed exactly once.
            unsafe { sqlite3_mutex_free(self.mutex.as_ptr()) };
        }
    }

    /// Holds the mutex until dropped on the thread that entered it.
    pub(crate) struct Guard<'a, T> {
        locked: &'a Locked<T>,
        // SQLite requires the entering thread to leave the mutex.
        _not_send: PhantomData<*const ()>,
    }

    impl<T> Deref for Guard<'_, T> {
        type Target = T;

        fn deref(&self) -> &T {
            // SAFETY: this guard holds the mutex, so no other reference to the value exists.
            unsafe { &*self.locked.value.get() }
        }
    }

    impl<T> DerefMut for Guard<'_, T> {
        fn deref_mut(&mut self) -> &mut T {
            // SAFETY: this guard holds the mutex and `&mut self` excludes other borrows through it.
            unsafe { &mut *self.locked.value.get() }
        }
    }

    impl<T> Drop for Guard<'_, T> {
        fn drop(&mut self) {
            // SAFETY: this thread entered the mutex in `Locked::lock` and leaves it once.
            unsafe { sqlite3_mutex_leave(self.locked.mutex.as_ptr()) };
        }
    }

    std::thread_local! {
        static LAST_ERROR: RefCell<Option<VfsError>> = const { RefCell::new(None) };
    }

    /// The calling thread's last VFS error, since `xGetLastError` names no connection.
    #[derive(Clone)]
    pub(crate) struct LastError;

    impl LastError {
        pub(crate) fn new() -> Self {
            Self
        }

        pub(crate) fn set(&self, error: VfsError) {
            LAST_ERROR.with(|slot| slot.replace(Some(error)));
        }

        pub(crate) fn get(&self) -> Option<VfsError> {
            LAST_ERROR.with(|slot| slot.borrow().clone())
        }
    }
}

pub(crate) use imp::{LastError, Locked, Shared};
