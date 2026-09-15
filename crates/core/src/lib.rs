//! Core model, timeframe math and provider interface for the bar-replay
//! backtester. No UI and no I/O live here: adapters implement
//! [`Provider`], the store persists [`Bar`]s, and the engine drives a
//! cursor over them.

pub mod aggregate;
pub mod provider;
pub mod timeframe;

pub use aggregate::{aggregate, Candle};
pub use provider::{Provider, Stub};
pub use timeframe::Timeframe;

use serde::{Deserialize, Serialize};

/// UTC milliseconds since the Unix epoch. The replay cursor is one of these,
/// never an array index (invariant I1: no lookahead).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(transparent)]
pub struct Timestamp(pub i64);

impl Timestamp {
    pub const MINUTE: i64 = 60_000;
    pub const HOUR: i64 = 3_600_000;
    pub const DAY: i64 = 86_400_000;

    /// Start of the `step`-ms bucket containing `self` (`floor(T, tf)` in I2).
    /// Euclidean so pre-1970 timestamps floor downward too.
    pub fn floor(self, step: i64) -> Timestamp {
        Timestamp(self.0.div_euclid(step) * step)
    }
}

impl std::fmt::Display for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match chrono::DateTime::from_timestamp_millis(self.0) {
            Some(dt) => write!(f, "{}", dt.format("%Y-%m-%d %H:%M:%S")),
            None => write!(f, "ts({})", self.0),
        }
    }
}

/// One base-resolution (1-minute) bar. `ts` is its **open** time.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bar {
    pub ts: Timestamp,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// One quote. Providers that only publish trade prints set `bid == ask` and
/// declare [`SpreadMode::Synthetic`] so the UI can say so (invariant I3).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tick {
    pub ts: Timestamp,
    pub bid: f64,
    pub ask: f64,
}

/// When an instrument is expected to be trading. Used to tell a calendar gap
/// (weekend or holiday — normal) from a data gap (the provider is missing bars
/// during hours the market was open), which invariant I4 requires be rendered
/// differently and never conflated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Market {
    /// Never closes: crypto.
    Continuous,
    /// The FX week: opens Sunday 17:00 and closes Friday 17:00, New York time,
    /// which is what every FX venue means by "the week" regardless of DST.
    FxWeek,
}

/// Where the spread in a session comes from. Always surfaced in the UI: a
/// trader must never mistake a synthetic spread for historical fact (I3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpreadMode {
    /// Provider publishes real bid/ask (Dukascopy).
    Historical,
    /// Trade-only provider (Binance); spread is a user-configured constant.
    Synthetic,
}

/// A tradable symbol on one provider. Fields beyond `provider`/`symbol` are
/// display and P&L parameters; see docs/schema.md.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Instrument {
    /// Adapter that serves it: "dukascopy", "binance", "csv", "databento".
    pub provider: String,
    /// Provider-native symbol: "EURUSD", "BTCUSDT".
    pub symbol: String,
    /// Display and export precision.
    pub price_decimals: u32,
    /// Smallest price increment.
    pub point: f64,
    /// P&L per 1 unit per 1.0 of price move: 1 for FX/crypto, 20 for NQ.
    pub multiplier: f64,
    pub quote_currency: String,
    /// Session/daily/weekly boundaries are computed against this zone and it is
    /// always shown in the UI, never silently assumed.
    pub session_tz: chrono_tz::Tz,
    pub spread_mode: SpreadMode,
    pub market: Market,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    /// The provider, network or file said no.
    Provider(String),
    /// The data itself is unusable (bad CSV row, corrupt payload).
    Data(String),
    Io(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Provider(m) => write!(f, "provider: {m}"),
            Error::Data(m) => write!(f, "data: {m}"),
            Error::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_rounds_down_to_bucket_start() {
        assert_eq!(
            Timestamp(125_000).floor(Timestamp::MINUTE),
            Timestamp(120_000)
        );
        assert_eq!(
            Timestamp(120_000).floor(Timestamp::MINUTE),
            Timestamp(120_000)
        );
        assert_eq!(Timestamp(-1).floor(Timestamp::MINUTE), Timestamp(-60_000));
    }
}
