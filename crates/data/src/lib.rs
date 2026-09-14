//! Provider adapters and the local Parquet/DuckDB store (M1).
//!
//! Everything that touches the network or the disk lives here. The engine
//! above it only ever sees data at or before the replay cursor (spec §5.1).

pub mod catalog;
pub mod fetch;
pub mod http;
pub mod keychain;
pub mod paths;
pub mod providers;
pub mod store;

pub use store::Store;
