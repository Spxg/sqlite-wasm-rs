#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
pub mod libsqlite3_sys;

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
pub mod c;

pub mod wasm;

use js_sys::{Object, WebAssembly::Memory};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt::Display, result::Result};
use tokio::sync::OnceCell;
use wasm::{CApi, Wasm};
use wasm_bindgen::JsValue;

static SQLITE: OnceCell<SQLite> = OnceCell::const_new();

pub async fn init_sqlite() -> Result<&'static SQLite, SQLiteError> {
    SQLITE.get_or_try_init(|| SQLite::default()).await
}

pub async fn init_sqlite_with(opts: SQLiteOpts) -> Result<&'static SQLite, SQLiteError> {
    SQLITE.get_or_try_init(|| SQLite::new(opts)).await
}

pub fn sqlite() -> Option<&'static SQLite> {
    SQLITE.get()
}

const WASM: &[u8] = include_bytes!("jswasm/sqlite3.wasm");

#[derive(Serialize)]
pub struct InitOpts {
    #[serde(rename = "wasmBinary")]
    pub wasm_binary: &'static [u8],
    #[serde(with = "serde_wasm_bindgen::preserve", rename = "wasmMemory")]
    pub wasm_memory: Memory,
    #[serde(rename = "proxyUri")]
    pub proxy_uri: String,
}

#[derive(Serialize)]
pub struct MemoryOpts {
    /// The initial size of the WebAssembly Memory, in units of WebAssembly pages.
    pub initial: usize,
    /// The maximum size the WebAssembly Memory is allowed to grow to, in units of WebAssembly pages.
    pub maximum: usize,
}

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

#[derive(Debug)]
pub enum SQLiteError {
    Memory(JsValue),
    Module(JsValue),
    Serde(serde_wasm_bindgen::Error),
}

impl Display for SQLiteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Memory(msg) => f.debug_tuple("Memory").field(msg).finish(),
            Self::Module(msg) => f.debug_tuple("Module").field(msg).finish(),
            Self::Serde(msg) => f.debug_tuple("Serde").field(msg).finish(),
        }
    }
}

impl Error for SQLiteError {}

pub struct SQLiteOpts {
    pub memory: MemoryOpts,
}

pub struct SQLite {
    ffi: wasm::SQLite,
    version: Version,
}

#[allow(unsafe_code)]
unsafe impl Sync for SQLite {}
#[allow(unsafe_code)]
unsafe impl Send for SQLite {}

impl SQLite {
    pub const DEFAULT_OPTIONS: SQLiteOpts = SQLiteOpts {
        memory: MemoryOpts {
            initial: 256,
            maximum: 32768,
        },
    };

    /// # Errors
    ///
    /// same as `SQLite::new()`
    ///
    pub async fn default() -> Result<Self, SQLiteError> {
        Self::new(Self::DEFAULT_OPTIONS).await
    }

    /// # Errors
    ///
    /// `SQLiteError::Memory`: the wrong range is configured
    ///
    /// `SQLiteError::Module`: error in initializing module
    ///
    /// `SQLiteError::Serde`: serialization and deserialization errors
    ///
    pub async fn new(opts: SQLiteOpts) -> Result<Self, SQLiteError> {
        let proxy_uri = wasm_bindgen::link_to!(module = "/src/jswasm/sqlite3-opfs-async-proxy.js");

        let wasm_memory = serde_wasm_bindgen::to_value(&opts.memory).map_err(SQLiteError::Serde)?;
        let wasm_memory = Memory::new(&Object::from(wasm_memory)).map_err(SQLiteError::Memory)?;

        let opts = InitOpts {
            wasm_binary: WASM,
            wasm_memory,
            proxy_uri,
        };

        let opts = serde_wasm_bindgen::to_value(&opts).map_err(SQLiteError::Serde)?;
        let module = wasm::SQLite::init(&Object::from(opts))
            .await
            .map_err(SQLiteError::Module)?;

        let sqlite = wasm::SQLite::new(module);

        let version =
            serde_wasm_bindgen::from_value(sqlite.version()).map_err(SQLiteError::Serde)?;

        let sqlite = Self {
            ffi: sqlite,
            version,
        };

        Ok(sqlite)
    }

    #[must_use]
    pub fn version(&self) -> &Version {
        &self.version
    }

    #[must_use]
    pub fn capi(&self) -> CApi {
        self.ffi.handle().capi()
    }

    #[must_use]
    pub fn wasm(&self) -> Wasm {
        self.ffi.handle().wasm()
    }
}
