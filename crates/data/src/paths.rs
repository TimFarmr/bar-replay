//! Where the app keeps its files. Layout is fixed by docs/schema.md.

use std::path::{Path, PathBuf};

/// Root of all app data. Overridable so tests and the CLI never touch the
/// user's real cache.
pub fn root() -> PathBuf {
    if let Ok(dir) = std::env::var("BAR_REPLAY_HOME") {
        return PathBuf::from(dir);
    }
    let base = dirs_next_home().unwrap_or_else(|| PathBuf::from("."));
    base.join(".bar-replay")
}

fn dirs_next_home() -> Option<PathBuf> {
    // The OS crates disagree on Windows; the env vars do not.
    std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

pub fn instrument_dir(root: &Path, provider: &str, symbol: &str) -> PathBuf {
    root.join("instruments").join(provider).join(symbol)
}

/// The cached base series for an instrument (disposable; ADR 0012).
pub fn base_parquet(root: &Path, provider: &str, symbol: &str) -> PathBuf {
    instrument_dir(root, provider, symbol).join("1m.parquet")
}

pub fn session_dir(root: &Path, id: &str) -> PathBuf {
    root.join("sessions").join(id)
}
