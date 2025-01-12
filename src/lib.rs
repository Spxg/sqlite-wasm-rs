#![doc = include_str!("../README.md")]

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
mod libsqlite3;

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
mod c;
mod fragile;
mod wasm;

/// These exported APIs are stable and will not have breaking changes.
pub mod export {
    /// C interface definition of sqlite
    pub use super::c::*;
    /// Some sqlite types copied from libsqlite3-sys
    pub use super::libsqlite3::*;
    /// See <https://sqlite.org/wasm/doc/trunk/persistence.md#vfs-opfs-sahpool>
    pub use super::wasm::OpfsSAHPoolUtil;
    /// `async fn init_sqlite()`: before using C-API, you must initialize sqlite, once.
    ///
    /// `fn sqlite()`: get the sqlite instance
    ///
    /// `OpfsSAHPoolCfg`: see <https://sqlite.org/wasm/doc/trunk/persistence.md#vfs-opfs-sahpool>
    ///
    /// `SQLite`: sqlite-wasm instance, only can be created by `init_sqlite`
    ///
    /// `SQLiteError`: initializing instance and other possible errors
    ///
    /// `Version`: sqlite-wasm version
    pub use super::{init_sqlite, sqlite, OpfsSAHPoolCfg, SQLite, SQLiteError, Version};
}

use fragile::FragileComfirmed;
use js_sys::{Object, WebAssembly};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt::Display, result::Result};
use tokio::sync::OnceCell;
use wasm::{CApi, OpfsSAHPoolUtil, Wasm};
use wasm_bindgen::JsCast;

/// Sqlite only needs to be initialized once
static SQLITE: OnceCell<SQLite> = OnceCell::const_new();

/// Initialize sqlite and opfs vfs
pub async fn init_sqlite() -> Result<&'static SQLite, SQLiteError> {
    SQLITE.get_or_try_init(SQLite::new).await
}

/// Get the current sqlite global instance
pub fn sqlite() -> Option<&'static SQLite> {
    SQLITE.get()
}

/// "Inline" sqlite wasm binary
const WASM: &[u8] = include_bytes!("jswasm/sqlite3.wasm");

/// Initialize sqlite parameters
///
/// Currently, only memory can be configured
#[derive(Serialize)]
struct InitOpts {
    /// sqlite wasm binary
    #[serde(rename = "wasmBinary")]
    wasm_binary: &'static [u8],
    /// opfs proxy uri
    #[serde(rename = "proxyUri")]
    proxy_uri: String,
}

/// SQLite version info
#[derive(Deserialize, Clone, Debug)]
pub struct Version {
    #[serde(rename = "libVersion")]
    pub lib_version: String,
    #[serde(rename = "libVersionNumber")]
    pub lib_version_number: u32,
    #[serde(rename = "sourceId")]
    pub source_id: String,
    #[serde(rename = "downloadVersion")]
    pub download_version: u32,
}

/// See <https://sqlite.org/wasm/doc/trunk/persistence.md#vfs-opfs-sahpool>
#[derive(Serialize, Clone, Debug)]
pub struct OpfsSAHPoolCfg {
    // If truthy, contents and filename mapping are removed from each SAH
    // as it is acquired during initalization of the VFS, leaving the VFS's
    // storage in a pristine state. Use this only for databases which need not
    // survive a page reload.
    #[serde(rename = "clearOnInit")]
    pub clear_on_init: bool,
    // Specifies the default capacity of the VFS, i.e. the number of files
    // it may contain.
    #[serde(rename = "initialCapacity")]
    pub initial_capacity: u32,
    // Specifies the OPFS directory name in which to store metadata for the VFS.
    pub directory: String,
    // Sets the name to register this VFS under.
    pub name: String,
    // It is an opt-in workaround for a particular browser quirk which can cause
    // initialization of this VFS to fail on its first attempt but to succeed if
    // a second attempt is tried a short time later
    #[serde(rename = "forceReinitIfPreviouslyFailed")]
    pub force_reinit_if_previously_failed: bool,
}

/// Possible errors in initializing sqlite
#[derive(Debug)]
pub enum SQLiteError {
    /// error in initializing module
    InitModule(js_sys::Error),
    /// serialization and deserialization errors
    Serde(serde_wasm_bindgen::Error),
    /// error in installing opfs sah pool vfs
    InstallOpfsSAHPoolVfs(js_sys::Error),
}

impl Display for SQLiteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InitModule(msg) => f.debug_tuple("InitModule").field(msg).finish(),
            Self::Serde(msg) => f.debug_tuple("Serde").field(msg).finish(),
            Self::InstallOpfsSAHPoolVfs(msg) => {
                f.debug_tuple("InstallOpfsSAHPoolVfs").field(msg).finish()
            }
        }
    }
}

impl Error for SQLiteError {}

/// Wrapped sqlite instance
///
/// It is not sure about the multi-thread support of sqlite-wasm,
/// so use `Fragile` to limit it to one thread.
pub struct SQLite {
    ffi: FragileComfirmed<wasm::SQLite>,
    version: Version,
}

impl SQLite {
    /// # Errors
    ///
    /// `SQLiteError::Module`: error in initializing module
    ///
    /// `SQLiteError::Serde`: serialization and deserialization errors
    async fn new() -> Result<Self, SQLiteError> {
        let proxy_uri = wasm_bindgen::link_to!(module = "/src/jswasm/sqlite3-opfs-async-proxy.js");

        let opts = InitOpts {
            wasm_binary: WASM,
            proxy_uri,
        };

        let opts = serde_wasm_bindgen::to_value(&opts).map_err(SQLiteError::Serde)?;
        let module = wasm::SQLite::init(&Object::from(opts))
            .await
            .map_err(SQLiteError::InitModule)?;

        let sqlite = wasm::SQLite::new(module);

        let version =
            serde_wasm_bindgen::from_value(sqlite.version()).map_err(SQLiteError::Serde)?;

        let sqlite = Self {
            ffi: FragileComfirmed::new(sqlite),
            version,
        };

        Ok(sqlite)
    }

    /// install_opfs_sahpool() returns a Promise which resolves, on success, to a utility object
    /// which can be used to perform basic administration of the file pool (colloquially known as `PoolUtil`).
    ///
    /// # Errors
    ///
    /// `SQLiteError::Serde`: serialization and deserialization errors
    ///
    /// `SQLiteError::InstallOpfsSAHPoolVfs`: error in installing opfs sah pool vfs
    pub async fn install_opfs_sahpool(
        &self,
        cfg: Option<&OpfsSAHPoolCfg>,
    ) -> Result<OpfsSAHPoolUtil, SQLiteError> {
        let cfg = cfg
            .map(|cfg| {
                serde_wasm_bindgen::to_value(&cfg)
                    .map_err(SQLiteError::Serde)
                    .map(Object::from)
            })
            .transpose()?;
        self.ffi
            .handle()
            .install_opfs_sahpool(cfg.as_ref())
            .await
            .map_err(SQLiteError::InstallOpfsSAHPoolVfs)
    }

    /// SQLite version
    #[must_use]
    pub fn version(&self) -> &Version {
        &self.version
    }

    /// SQLite CAPI
    #[must_use]
    fn capi(&self) -> CApi {
        self.ffi.handle().capi()
    }

    /// SQLite memeory manager
    #[must_use]
    fn wasm(&self) -> Wasm {
        self.ffi.handle().wasm()
    }
}

/// Peek and Poke on the JS side
///
/// See <https://github.com/rustwasm/wasm-bindgen/issues/4395>
///
/// See <https://github.com/rustwasm/wasm-bindgen/issues/4392>
impl SQLite {
    unsafe fn poke_buf(&self, src: &[u8], dst: *mut u8) {
        let buf = wasm_bindgen::memory();
        let mem = buf.unchecked_ref::<WebAssembly::Memory>();
        self.ffi.poke_buf(mem, src.as_ptr(), dst, src.len() as u32)
    }

    unsafe fn peek<T>(&self, from: *mut u8, dst: &mut T) {
        let dst = std::ptr::from_ref(dst) as *mut u8;
        let slice = unsafe { std::slice::from_raw_parts_mut(dst, size_of::<T>()) };
        self.peek_buf(from, slice);
    }

    unsafe fn peek_buf(&self, src: *const u8, dst: &mut [u8]) {
        let buf = wasm_bindgen::memory();
        let mem = buf.unchecked_ref::<WebAssembly::Memory>();
        self.ffi
            .peek_buf(mem, src, dst.as_mut_ptr(), dst.len() as u32)
    }
}
