//! Timeframes and candle bucket boundaries.
//!
//! Intraday buckets are a plain floor over UTC milliseconds. Daily and weekly
//! buckets open at local midnight in the instrument's session timezone, with
//! DST handled by chrono-tz, never by a manual offset (spec §4).

use crate::Timestamp;
use chrono::{Datelike, Duration, LocalResult, NaiveDate, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Timeframe {
    M1,
    M5,
    M15,
    M30,
    H1,
    H4,
    D1,
    W1,
}

impl Timeframe {
    pub const ALL: [Timeframe; 8] = [
        Timeframe::M1,
        Timeframe::M5,
        Timeframe::M15,
        Timeframe::M30,
        Timeframe::H1,
        Timeframe::H4,
        Timeframe::D1,
        Timeframe::W1,
    ];

    /// Fixed bucket width in ms, or `None` for calendar timeframes whose width
    /// varies across DST transitions.
    pub fn fixed_ms(self) -> Option<i64> {
        use Timeframe::*;
        Some(match self {
            M1 => Timestamp::MINUTE,
            M5 => 5 * Timestamp::MINUTE,
            M15 => 15 * Timestamp::MINUTE,
            M30 => 30 * Timestamp::MINUTE,
            H1 => Timestamp::HOUR,
            H4 => 4 * Timestamp::HOUR,
            D1 | W1 => return None,
        })
    }

    pub fn label(self) -> &'static str {
        use Timeframe::*;
        match self {
            M1 => "1m",
            M5 => "5m",
            M15 => "15m",
            M30 => "30m",
            H1 => "1h",
            H4 => "4h",
            D1 => "1d",
            W1 => "1w",
        }
    }

    pub fn parse(s: &str) -> Option<Timeframe> {
        Timeframe::ALL
            .into_iter()
            .find(|tf| tf.label().eq_ignore_ascii_case(s))
    }

    /// Open time of the bucket containing `ts`.
    pub fn bucket_start(self, ts: Timestamp, tz: Tz) -> Timestamp {
        match self.fixed_ms() {
            Some(ms) => ts.floor(ms),
            None => {
                let local = tz.from_utc_datetime(
                    &chrono::DateTime::from_timestamp_millis(ts.0)
                        .expect("timestamp in range")
                        .naive_utc(),
                );
                let mut date = local.date_naive();
                if self == Timeframe::W1 {
                    // Weeks open Monday, matching ISO-8601 and every venue's
                    // weekly candle.
                    date -= Duration::days(local.weekday().num_days_from_monday() as i64);
                }
                local_midnight(date, tz)
            }
        }
    }

    /// Every bucket open time needed to cover `[from, to]`, ascending.
    ///
    /// The store joins against this list instead of asking the database to do
    /// timezone maths, so the SQL and [`crate::aggregate`] cannot disagree
    /// about where a day begins.
    pub fn bucket_starts(self, from: Timestamp, to: Timestamp, tz: Tz) -> Vec<Timestamp> {
        let mut out = Vec::new();
        if to < from {
            return out;
        }
        let mut t = self.bucket_start(from, tz);
        while t <= to {
            out.push(t);
            let next = self.next_bucket(t, tz);
            debug_assert!(next > t, "buckets must advance");
            t = next;
        }
        out
    }

    /// Open time of the bucket after the one containing `ts`. Used to decide
    /// whether the newest candle is still forming (§5.2).
    pub fn next_bucket(self, ts: Timestamp, tz: Tz) -> Timestamp {
        let start = self.bucket_start(ts, tz);
        match self.fixed_ms() {
            Some(ms) => Timestamp(start.0 + ms),
            None => {
                let step = if self == Timeframe::W1 { 7 } else { 1 };
                let local_date = tz
                    .from_utc_datetime(
                        &chrono::DateTime::from_timestamp_millis(start.0)
                            .expect("timestamp in range")
                            .naive_utc(),
                    )
                    .date_naive();
                local_midnight(local_date + Duration::days(step), tz)
            }
        }
    }
}

/// The UTC instant of local midnight on `date`.
///
/// Midnight does not exist in every zone on every date: some zones (e.g.
/// America/Santiago) spring forward at 00:00, and a few fall back across it.
/// Ambiguous midnights take the earliest instant; skipped ones take the first
/// instant that does exist. Both keep buckets strictly increasing, which is
/// what the aggregator relies on.
fn local_midnight(date: NaiveDate, tz: Tz) -> Timestamp {
    let naive = date.and_hms_opt(0, 0, 0).expect("valid midnight");
    let dt = match tz.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(earliest, _) => earliest,
        LocalResult::None => first_valid_after(naive, tz),
    };
    Timestamp(dt.timestamp_millis())
}

fn first_valid_after(naive: NaiveDateTime, tz: Tz) -> chrono::DateTime<Tz> {
    // DST jumps are at most a few hours; minute steps find the edge exactly.
    for minutes in 1..=(6 * 60) {
        let candidate = naive + Duration::minutes(minutes);
        match tz.from_local_datetime(&candidate) {
            LocalResult::Single(dt) => return dt,
            LocalResult::Ambiguous(earliest, _) => return earliest,
            LocalResult::None => continue,
        }
    }
    unreachable!("no valid local time within 6h of midnight in {tz}")
}
