#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
mod libsqlite3;

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
mod fill;

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
mod vfs;

/// These exported APIs are stable and will not have breaking changes.
pub mod export {

    // Some sqlite types copied from libsqlite3-sys
    pub use super::libsqlite3::*;
    pub use super::vfs::sahpool::{
        install_opfs_sahpool, OpfsSAHError, OpfsSAHPoolCfg, OpfsSAHPoolUtil,
    };

    /// Make it behave the same as when wrapper features are enabled
    pub struct SQLite;

    impl SQLite {
        #[deprecated = "use install_opfs_sahpool directly in polyfill feature"]
        pub async fn install_opfs_sahpool(
            &self,
            options: Option<&OpfsSAHPoolCfg>,
        ) -> Result<OpfsSAHPoolUtil, OpfsSAHError> {
            install_opfs_sahpool(options, false).await
        }
    }

    #[deprecated = "init_sqlite() is not needed in polyfill feature"]
    pub async fn init_sqlite() -> Result<SQLite, ()> {
        Ok(SQLite)
    }
}
