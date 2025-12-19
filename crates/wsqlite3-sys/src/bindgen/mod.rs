#[cfg(feature = "sqlite3mc")]
mod sqlite3mc;

#[cfg(not(feature = "sqlite3mc"))]
mod sqlite3;

#[cfg(feature = "sqlite3mc")]
pub use sqlite3mc::*;

#[cfg(not(feature = "sqlite3mc"))]
pub use sqlite3::*;
