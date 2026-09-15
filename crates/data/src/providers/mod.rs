//! Provider adapters. Each one talks to its provider directly (ADR 0007).

pub mod bi5;
pub mod binance;
pub mod csv;
pub mod databento;
pub mod dukascopy;

use replay_core::{Error, Provider, Result};

/// What a caller needs to know about an adapter without constructing one.
///
/// This is the single list of adapters. The CLI, the settings screen and the
/// error messages all read it, so adding a provider means editing one place
/// rather than hunting for every spot that restated the list and drifted.
pub struct ProviderInfo {
    pub id: &'static str,
    /// Shown to the user.
    pub label: &'static str,
    /// Needs a BYOK API key before it can fetch anything.
    pub needs_key: bool,
    /// Usable with no configuration at all, which the data policy requires of the
    /// providers offered on first launch.
    pub free: bool,
    /// Built from a file the user picked rather than from an id alone, so it
    /// cannot be constructed by [`by_id`].
    pub from_file: bool,
}

pub fn registry() -> &'static [ProviderInfo] {
    &[
        ProviderInfo {
            id: dukascopy::ID,
            label: "Dukascopy — FX and gold, real bid/ask",
            needs_key: false,
            free: true,
            from_file: false,
        },
        ProviderInfo {
            id: binance::ID,
            label: "Binance — crypto, trades only",
            needs_key: false,
            free: true,
            from_file: false,
        },
        ProviderInfo {
            id: databento::ID,
            label: "Databento — CME futures and equities",
            needs_key: true,
            free: false,
            from_file: false,
        },
        ProviderInfo {
            id: csv::ID,
            label: "CSV — your own file",
            needs_key: false,
            free: false,
            from_file: true,
        },
    ]
}

pub fn info(id: &str) -> Option<&'static ProviderInfo> {
    registry().iter().find(|p| p.id == id)
}

/// Every adapter that works with no configuration at all.
pub fn free() -> Vec<Box<dyn Provider>> {
    registry()
        .iter()
        .filter(|p| p.free)
        .filter_map(|p| by_id(p.id).ok())
        .collect()
}

pub fn by_id(id: &str) -> Result<Box<dyn Provider>> {
    match id {
        dukascopy::ID => Ok(Box::new(dukascopy::Dukascopy)),
        binance::ID => Ok(Box::new(binance::Binance)),
        databento::ID => Ok(Box::new(databento::Databento)),
        // `csv` is deliberately absent: it is built from a path and a symbol
        // the user chose, so it cannot be conjured from an id alone.
        other => Err(Error::Provider(format!(
            "unknown provider {other:?}; available: {}",
            constructible().join(", ")
        ))),
    }
}

fn constructible() -> Vec<&'static str> {
    registry()
        .iter()
        .filter(|p| !p.from_file)
        .map(|p| p.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_constructible_provider_can_actually_be_constructed() {
        for id in constructible() {
            assert!(by_id(id).is_ok(), "{id} is listed but cannot be built");
        }
    }

    #[test]
    fn a_file_backed_provider_is_not_offered_as_constructible() {
        assert!(!constructible().contains(&csv::ID));
        assert!(by_id(csv::ID).is_err());
    }

    /// The error message is generated from the registry, so it cannot drift
    /// out of date the way a hand-written list would.
    #[test]
    fn an_unknown_provider_is_told_what_is_available() {
        // `unwrap_err` would need the Ok type to be Debug, which a trait
        // object is not.
        let Err(err) = by_id("nope") else {
            panic!("an unknown provider must not resolve");
        };
        let msg = err.to_string();
        for id in constructible() {
            assert!(msg.contains(id), "{msg} should mention {id}");
        }
    }

    #[test]
    fn the_free_providers_are_the_ones_needing_no_configuration() {
        let ids: Vec<&str> = free().iter().map(|p| p.id()).collect();
        assert_eq!(ids, vec![dukascopy::ID, binance::ID]);
        for p in registry().iter().filter(|p| p.free) {
            assert!(!p.needs_key, "{} is free but wants a key", p.id);
        }
    }
}
