#[test]
fn app_data_access_does_not_borrow_registry_links() {
    use rsqlite_vfs::{VfsAppData, ffi::sqlite3_vfs};
    use std::boxed::Box;

    struct Shared(*mut sqlite3_vfs);
    // SAFETY: Only the writer accesses pNext; readers use immutable pAppData
    // and its Sync payload. Both allocations outlive the joined threads.
    unsafe impl Sync for Shared {}
    impl Shared {
        fn read(&self) {
            for _ in 0..40 {
                assert_eq!(**unsafe { VfsAppData::<usize>::get(self.0) }, 42);
                std::thread::yield_now();
            }
        }

        fn write(&self) {
            for _ in 0..40 {
                unsafe { core::ptr::addr_of_mut!((*self.0).pNext).write(self.0) };
                std::thread::yield_now();
            }
        }
    }
    let data = Box::into_raw(Box::new(VfsAppData::new(42usize)));
    let mut vfs: sqlite3_vfs = unsafe { core::mem::zeroed() };
    vfs.pAppData = data.cast();
    let shared = Shared(Box::into_raw(Box::new(vfs)));
    std::thread::scope(|scope| {
        scope.spawn(|| shared.read());
        scope.spawn(|| shared.write());
    });
    unsafe {
        drop(Box::from_raw(shared.0));
        drop(Box::from_raw(data));
    }
}
