#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(clippy::type_complexity)]
mod libsqlite3;

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
mod r#impl;

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
mod vfs;

/// These exported APIs are stable and will not have breaking changes.
pub mod export {
    // Some sqlite types copied from libsqlite3-sys
    pub use super::libsqlite3::*;
    pub use super::vfs::sahpool::{
        install_opfs_sahpool, OpfsSAHError, OpfsSAHPoolCfg, OpfsSAHPoolCfgBuilder, OpfsSAHPoolUtil,
    };

    #[cfg(feature = "custom-libc")]
    pub use sqlite_wasm_libc;

    /// Make it behave the same as when wrapper features are enabled
    pub struct SQLite;

    impl SQLite {
        /// Register `opfs-sahpool` vfs and return a utility object which can be used
        /// to perform basic administration of the file pool
        #[deprecated = "use install_opfs_sahpool directly in shim feature"]
        pub async fn install_opfs_sahpool(
            &self,
            options: Option<&OpfsSAHPoolCfg>,
        ) -> Result<OpfsSAHPoolUtil, OpfsSAHError> {
            install_opfs_sahpool(options, false).await
        }
    }

    /// Empty implementation
    #[deprecated = "init_sqlite() is not needed in shim feature"]
    pub async fn init_sqlite() -> Result<SQLite, ()> {
        Ok(SQLite)
    }
}
