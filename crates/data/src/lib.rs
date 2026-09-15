//! Provider adapters and the local Parquet/DuckDB store.
//!
//! Everything that touches the network or the disk lives here. The engine
//! above it only ever sees data at or before the replay cursor (invariant I1).

pub mod catalog;
pub mod fetch;
pub mod http;
pub mod keychain;
pub mod paths;
pub mod providers;
pub mod store;

pub use store::Store;
