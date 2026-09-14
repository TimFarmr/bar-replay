//! The one interface every market-data adapter implements.
//!
//! Adapters talk to the provider directly from the user's machine; no
//! infrastructure operated by this project ever sees market data (ADR 0007).

use crate::{Bar, Instrument, Market, Result, SpreadMode, Tick, Timestamp};

/// A market-data source. Implementations return what the provider has and
/// nothing else: no interpolation, no forward-fill, no synthesized bars (§5.4).
pub trait Provider: Send + Sync {
    /// Stable adapter id, matching [`Instrument::provider`].
    fn id(&self) -> &'static str;

    /// Symbols this adapter can serve without configuration.
    fn instruments(&self) -> Result<Vec<Instrument>>;

    /// How many requests this provider tolerates at once. The user is the
    /// subscriber and their rate limit is not ours to spend, so adapters that
    /// know their provider is fragile say so rather than letting the caller
    /// guess.
    fn max_concurrency(&self) -> usize {
        4
    }

    /// Completed 1-minute bars with `from <= ts < to`, ascending by `ts`,
    /// unique. Missing minutes are simply absent.
    fn minutes(&self, instrument: &Instrument, from: Timestamp, to: Timestamp) -> Result<Vec<Bar>>;

    /// Ticks with `from <= ts < to`, ascending. `Ok(None)` means this provider
    /// has no tick data at all; the fill model then assumes stop-loss first
    /// (§5.3, ADR 0013).
    fn ticks(
        &self,
        _instrument: &Instrument,
        _from: Timestamp,
        _to: Timestamp,
    ) -> Result<Option<Vec<Tick>>> {
        Ok(None)
    }
}

/// Deterministic fixture for tests. Never offered in the UI: real candles come
/// from real providers only (§5.4).
pub struct Stub;

impl Stub {
    pub fn instrument() -> Instrument {
        Instrument {
            provider: "stub".into(),
            symbol: "STUB".into(),
            price_decimals: 2,
            point: 0.01,
            multiplier: 1.0,
            quote_currency: "USD".into(),
            session_tz: chrono_tz::UTC,
            spread_mode: SpreadMode::Synthetic,
            market: Market::Continuous,
        }
    }
}

impl Provider for Stub {
    fn id(&self) -> &'static str {
        "stub"
    }

    fn instruments(&self) -> Result<Vec<Instrument>> {
        Ok(vec![Stub::instrument()])
    }

    fn minutes(&self, _: &Instrument, from: Timestamp, to: Timestamp) -> Result<Vec<Bar>> {
        let mut ts = from.floor(Timestamp::MINUTE);
        if ts < from {
            ts.0 += Timestamp::MINUTE;
        }
        let mut bars = Vec::new();
        while ts < to {
            let p = 100.0 + (ts.0 / Timestamp::MINUTE % 10) as f64;
            bars.push(Bar {
                ts,
                open: p,
                high: p + 1.0,
                low: p - 1.0,
                close: p + 0.5,
                volume: 1.0,
            });
            ts.0 += Timestamp::MINUTE;
        }
        Ok(bars)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_minutes_are_aligned_ascending_and_in_range() {
        let (from, to) = (Timestamp(90_000), Timestamp(400_000));
        let bars = Stub.minutes(&Stub::instrument(), from, to).unwrap();
        assert_eq!(bars.len(), 5);
        for b in &bars {
            assert!(from <= b.ts && b.ts < to);
            assert_eq!(b.ts.floor(Timestamp::MINUTE), b.ts);
            assert!(b.low <= b.open.min(b.close) && b.open.max(b.close) <= b.high);
        }
        for w in bars.windows(2) {
            assert_eq!(w[1].ts.0 - w[0].ts.0, Timestamp::MINUTE);
        }
        assert!(Stub.ticks(&Stub::instrument(), from, to).unwrap().is_none());
    }
}
