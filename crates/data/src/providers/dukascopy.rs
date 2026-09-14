//! Dukascopy: free FX minute bars and real bid/ask ticks, no key, no signup.
//!
//! Wire format (verified against live files, not documentation):
//! `.bi5` payloads are raw LZMA1 ("alone") streams. Decompressed,
//! * `BID_candles_min_1.bi5` is one day of 24-byte big-endian records:
//!   `i32 seconds-from-midnight, i32 open, i32 close, i32 low, i32 high, f32 volume`
//! * `{hh}h_ticks.bi5` is one hour of 20-byte records:
//!   `i32 ms-from-hour, i32 ask, i32 bid, f32 ask-volume, f32 bid-volume`
//!
//! Integer prices are scaled by the instrument's `point`.

use super::bi5;
use crate::catalog;
use crate::http;
use replay_core::{Bar, Instrument, Provider, Result, Tick, Timestamp};

const BASE: &str = "https://datafeed.dukascopy.com/datafeed";
pub const ID: &str = "dukascopy";

pub struct Dukascopy;

/// Dukascopy numbers months from zero.
fn day_url(symbol: &str, y: i32, m: u32, d: u32) -> String {
    format!(
        "{BASE}/{symbol}/{y:04}/{:02}/{d:02}/BID_candles_min_1.bi5",
        m - 1
    )
}

fn hour_url(symbol: &str, y: i32, m: u32, d: u32, h: u32) -> String {
    format!(
        "{BASE}/{symbol}/{y:04}/{:02}/{d:02}/{h:02}h_ticks.bi5",
        m - 1
    )
}

impl Provider for Dukascopy {
    fn id(&self) -> &'static str {
        ID
    }

    /// The public datafeed throttles by IP: a burst earns connection resets
    /// and 503s for minutes afterwards, so history is fetched gently.
    fn max_concurrency(&self) -> usize {
        2
    }

    fn instruments(&self) -> Result<Vec<Instrument>> {
        Ok(catalog::builtin()
            .into_iter()
            .filter(|i| i.provider == ID)
            .collect())
    }

    fn minutes(&self, inst: &Instrument, from: Timestamp, to: Timestamp) -> Result<Vec<Bar>> {
        let mut out = Vec::new();
        for day in bi5::days_covering(from, to) {
            let (y, m, d) = bi5::ymd(day);
            let Some(raw) = http::get_bytes(&day_url(&inst.symbol, y, m, d))? else {
                continue; // no file: weekend or before listing
            };
            for bar in bi5::decode_minutes(&raw, day, inst.point)? {
                if from <= bar.ts && bar.ts < to {
                    out.push(bar);
                }
            }
        }
        out.sort_by_key(|b| b.ts);
        out.dedup_by_key(|b| b.ts);
        Ok(out)
    }

    fn ticks(
        &self,
        inst: &Instrument,
        from: Timestamp,
        to: Timestamp,
    ) -> Result<Option<Vec<Tick>>> {
        let mut out = Vec::new();
        for hour in bi5::hours_covering(from, to) {
            let (y, m, d) = bi5::ymd(hour);
            let h = ((hour.0 % Timestamp::DAY) / Timestamp::HOUR) as u32;
            let Some(raw) = http::get_bytes(&hour_url(&inst.symbol, y, m, d, h))? else {
                continue;
            };
            for tick in bi5::decode_ticks(&raw, hour, inst.point)? {
                if from <= tick.ts && tick.ts < to {
                    out.push(tick);
                }
            }
        }
        out.sort_by_key(|t| t.ts);
        Ok(Some(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_use_zero_based_months() {
        assert_eq!(
            day_url("EURUSD", 2024, 1, 2),
            "https://datafeed.dukascopy.com/datafeed/EURUSD/2024/00/02/BID_candles_min_1.bi5"
        );
        assert_eq!(
            hour_url("EURUSD", 2024, 12, 31, 23),
            "https://datafeed.dukascopy.com/datafeed/EURUSD/2024/11/31/23h_ticks.bi5"
        );
    }
}
