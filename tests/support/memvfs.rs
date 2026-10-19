use sqlite_wasm_rs::vfs::memvfs::MemVfsUtil;

pub fn get() -> MemVfsUtil {
    #[cfg(any(
        feature = "threadsafe",
        all(target_family = "wasm", not(target_feature = "atomics"))
    ))]
    {
        let context = sqlite_wasm_rs::vfs::memvfs::MemVfsContext::try_new().unwrap();
        MemVfsUtil::get(&context).unwrap()
    }
    #[cfg(not(any(
        feature = "threadsafe",
        all(target_family = "wasm", not(target_feature = "atomics"))
    )))]
    {
        // SAFETY: this suite excludes concurrent SQLite use and teardown.
        unsafe { MemVfsUtil::get_unchecked().unwrap() }
    }
}
