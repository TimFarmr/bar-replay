//! Reference aggregation of base (1-minute) bars into any timeframe.
//!
//! This is the definition the store's SQL is tested against (the verification checklist:
//! "assert the last candle equals the manual aggregate of 1m bars"). Keeping a
//! dependency-free implementation here means the invariant can be checked
//! without a database.

use crate::{Bar, Timeframe, Timestamp};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Candle {
    /// Bucket open time, not the first bar's timestamp: a candle whose opening
    /// minutes are missing still sits in its own slot (I4, no shifting).
    pub ts: Timestamp,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    /// False while the bucket is still being revealed by the cursor. Only the
    /// newest candle can be incomplete (I2).
    pub complete: bool,
}

/// Aggregate ascending base `bars` into `tf` candles as of `cursor`.
///
/// Bars after `cursor` are ignored rather than trusted: the no-lookahead filter
/// belongs in the data layer (I1), and this is the last line of defence.
/// Buckets with no bars produce no candle — gaps stay gaps (I4).
pub fn aggregate(bars: &[Bar], tf: Timeframe, tz: Tz, cursor: Timestamp) -> Vec<Candle> {
    let mut out: Vec<Candle> = Vec::new();

    for bar in bars.iter().filter(|b| b.ts <= cursor) {
        let start = tf.bucket_start(bar.ts, tz);
        match out.last_mut() {
            Some(c) if c.ts == start => {
                c.high = c.high.max(bar.high);
                c.low = c.low.min(bar.low);
                c.close = bar.close;
                c.volume += bar.volume;
            }
            _ => out.push(Candle {
                ts: start,
                open: bar.open,
                high: bar.high,
                low: bar.low,
                close: bar.close,
                volume: bar.volume,
                complete: true,
            }),
        }
    }

    // A bucket is fully revealed once the cursor's bar covers its final minute.
    // The cursor bar spans [cursor, cursor + 1m).
    if let Some(last) = out.last_mut() {
        let end = tf.next_bucket(last.ts, tz);
        last.complete = end.0 <= cursor.0 + Timestamp::MINUTE;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Tz::{America__New_York, UTC};

    fn bar(minute: i64, o: f64, h: f64, l: f64, c: f64) -> Bar {
        Bar {
            ts: Timestamp(minute * Timestamp::MINUTE),
            open: o,
            high: h,
            low: l,
            close: c,
            volume: 1.0,
        }
    }

    #[test]
    fn hourly_candle_is_first_max_min_last_over_its_minutes() {
        let bars: Vec<Bar> = (0..60)
            .map(|m| bar(m, m as f64, m as f64 + 2.0, m as f64 - 1.0, m as f64 + 1.0))
            .collect();
        let c = aggregate(&bars, Timeframe::H1, UTC, Timestamp(59 * Timestamp::MINUTE));
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].ts, Timestamp(0));
        assert_eq!(c[0].open, 0.0);
        assert_eq!(c[0].high, 61.0);
        assert_eq!(c[0].low, -1.0);
        assert_eq!(c[0].close, 60.0);
        assert_eq!(c[0].volume, 60.0);
        assert!(c[0].complete, "cursor covers the final minute of the hour");
    }

    #[test]
    fn newest_candle_is_in_progress_until_its_last_minute_is_revealed() {
        let bars: Vec<Bar> = (0..60).map(|m| bar(m, 1.0, 2.0, 0.5, 1.5)).collect();
        for cursor_min in 0..59 {
            let cursor = Timestamp(cursor_min * Timestamp::MINUTE);
            let c = aggregate(&bars, Timeframe::H1, UTC, cursor);
            assert_eq!(c.len(), 1);
            assert!(
                !c[0].complete,
                "minute {cursor_min} should still be forming"
            );
            assert_eq!(c[0].volume, cursor_min as f64 + 1.0);
        }
    }

    #[test]
    fn bars_after_the_cursor_are_never_aggregated() {
        let bars: Vec<Bar> = (0..120).map(|m| bar(m, 1.0, 2.0, 0.5, 1.5)).collect();
        let cursor = Timestamp(30 * Timestamp::MINUTE);
        let c = aggregate(&bars, Timeframe::H1, UTC, cursor);
        assert_eq!(c.len(), 1, "the second hour is entirely in the future");
        assert_eq!(c[0].volume, 31.0);
    }

    #[test]
    fn a_missing_bucket_produces_no_candle_rather_than_a_filled_one() {
        let bars = vec![bar(0, 1.0, 1.0, 1.0, 1.0), bar(180, 2.0, 2.0, 2.0, 2.0)];
        let c = aggregate(
            &bars,
            Timeframe::H1,
            UTC,
            Timestamp(240 * Timestamp::MINUTE),
        );
        assert_eq!(c.len(), 2, "the two empty hours are a gap, not candles");
        assert_eq!(c[0].ts, Timestamp(0));
        assert_eq!(c[1].ts, Timestamp(3 * Timestamp::HOUR));
    }

    #[test]
    fn daily_buckets_open_at_local_midnight_not_utc_midnight() {
        // 2024-03-04 05:00 UTC is 00:00 in New York (UTC-5).
        let ny_midnight = Timestamp(1_709_528_400_000);
        let ts = Timestamp(ny_midnight.0 + 3 * Timestamp::HOUR);
        assert_eq!(
            Timeframe::D1.bucket_start(ts, America__New_York),
            ny_midnight
        );
        assert_eq!(
            Timeframe::D1.bucket_start(ts, UTC),
            ts.floor(Timestamp::DAY)
        );
    }

    #[test]
    fn a_dst_day_is_23_hours_long_and_buckets_stay_increasing() {
        // US DST started 2024-03-10.
        let before = Timestamp(1_709_960_400_000); // 2024-03-09 00:00 NY
        let start = Timeframe::D1.bucket_start(before, America__New_York);
        let next = Timeframe::D1.next_bucket(start, America__New_York);
        let after = Timeframe::D1.next_bucket(next, America__New_York);
        assert_eq!(next.0 - start.0, Timestamp::DAY, "2024-03-09 is 24h");
        assert_eq!(after.0 - next.0, 23 * Timestamp::HOUR, "2024-03-10 is 23h");
        assert!(start < next && next < after);
    }

    #[test]
    fn weekly_buckets_open_on_monday() {
        // 2024-03-06 is a Wednesday.
        let wed = Timestamp(1_709_683_200_000);
        let start = Timeframe::W1.bucket_start(wed, UTC);
        assert_eq!(start, Timestamp(1_709_510_400_000)); // Mon 2024-03-04
        assert_eq!(
            Timeframe::W1.next_bucket(start, UTC).0 - start.0,
            7 * Timestamp::DAY
        );
    }
}
