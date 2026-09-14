//! Gap taxonomy (spec §5.4).
//!
//! Missing data is always rendered as a gap and never interpolated, but not
//! all gaps mean the same thing:
//!
//! * a **calendar** gap is the market being shut — a weekend or a holiday. It
//!   is expected, and drawn the way any chart skips non-trading time.
//! * a **data** gap is the provider missing bars while the market was open.
//!   That is a defect in the data and the chart must say so, because a trader
//!   replaying across it is looking at a hole, not at quiet trading.
//!
//! Conflating the two would let a broken download masquerade as a quiet
//! Sunday, which is exactly the false confidence this project exists to avoid.

use chrono::{DateTime, Datelike, Timelike, Weekday};
use chrono_tz::Tz;
use replay_core::{Market, Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapKind {
    Calendar,
    Data,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Gap {
    /// First missing minute.
    pub from: Timestamp,
    /// Last missing minute (inclusive), so a one-minute gap has `from == to`.
    pub to: Timestamp,
    pub kind: GapKind,
    /// Shown to the user; the UI never invents its own wording for a gap.
    pub reason: &'static str,
    /// Minutes inside the gap during which the market was expected to be open.
    pub missing_open_minutes: i64,
}

/// The FX week opens Sunday 17:00 and closes Friday 17:00 in New York, which
/// is the convention every FX venue uses. Expressed in New York local time so
/// chrono-tz handles DST rather than a hand-rolled offset (spec §4).
const FX_OPEN_HOUR: u32 = 17;
const FX_TZ: Tz = Tz::America__New_York;

/// A gap missing at least this much open time is treated as a market holiday
/// rather than a data defect.
///
/// This is a deliberate heuristic, not a holiday calendar: shipping one would
/// mean a dependency that goes stale every year. Half a session missing is far
/// more likely to be a closed market than a provider dropping bars, and the
/// reason string says which judgement was applied so the user can disagree.
const HOLIDAY_MINUTES: i64 = 12 * 60;

pub fn is_open(ts: Timestamp, market: Market) -> bool {
    match market {
        Market::Continuous => true,
        Market::FxWeek => {
            let local: DateTime<Tz> = DateTime::from_timestamp_millis(ts.0)
                .expect("timestamp in range")
                .with_timezone(&FX_TZ);
            match local.weekday() {
                Weekday::Sat => false,
                Weekday::Fri => local.hour() < FX_OPEN_HOUR,
                Weekday::Sun => local.hour() >= FX_OPEN_HOUR,
                _ => true,
            }
        }
    }
}

/// Every stretch of missing minutes between consecutive base bars.
///
/// `times` are base-bar open times, ascending. Nothing outside `[first, last]`
/// is considered: a session says nothing about data it never asked for.
pub fn find(times: &[Timestamp], market: Market) -> Vec<Gap> {
    let mut out = Vec::new();
    for pair in times.windows(2) {
        let (prev, next) = (pair[0], pair[1]);
        if next.0 - prev.0 <= Timestamp::MINUTE {
            continue;
        }
        let from = Timestamp(prev.0 + Timestamp::MINUTE);
        let to = Timestamp(next.0 - Timestamp::MINUTE);
        let open = open_minutes(from, to, market);
        let (kind, reason) = if open == 0 {
            (GapKind::Calendar, "market closed")
        } else if open >= HOLIDAY_MINUTES {
            (GapKind::Calendar, "no data for a whole session (holiday?)")
        } else {
            (GapKind::Data, "data missing while the market was open")
        };
        out.push(Gap {
            from,
            to,
            kind,
            reason,
            missing_open_minutes: open,
        });
    }
    out
}

/// Minutes in `[from, to]` during which the market was expected to be open.
fn open_minutes(from: Timestamp, to: Timestamp, market: Market) -> i64 {
    if market == Market::Continuous {
        return (to.0 - from.0) / Timestamp::MINUTE + 1;
    }
    let mut open = 0;
    let mut t = from;
    while t <= to {
        if is_open(t, market) {
            open += 1;
        }
        t = Timestamp(t.0 + Timestamp::MINUTE);
    }
    open
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2024-01-05 21:59 UTC — Friday, just before the FX week closes at
    /// 17:00 New York (22:00 UTC in January).
    const FRI_CLOSE_UTC: i64 = 1_704_492_000_000; // 2024-01-05 22:00 UTC
    const SUN_OPEN_UTC: i64 = 1_704_664_800_000; // 2024-01-07 22:00 UTC

    #[test]
    fn the_fx_week_closes_friday_evening_and_reopens_sunday_evening() {
        assert!(is_open(
            Timestamp(FRI_CLOSE_UTC - Timestamp::MINUTE),
            Market::FxWeek
        ));
        assert!(!is_open(Timestamp(FRI_CLOSE_UTC), Market::FxWeek));
        assert!(!is_open(
            Timestamp(SUN_OPEN_UTC - Timestamp::MINUTE),
            Market::FxWeek
        ));
        assert!(is_open(Timestamp(SUN_OPEN_UTC), Market::FxWeek));
    }

    #[test]
    fn crypto_is_never_closed() {
        for ts in [FRI_CLOSE_UTC, SUN_OPEN_UTC, FRI_CLOSE_UTC + Timestamp::DAY] {
            assert!(is_open(Timestamp(ts), Market::Continuous));
        }
    }

    /// Spec §6: a weekend gap must render differently from an injected
    /// missing-data gap during trading hours.
    #[test]
    fn a_weekend_and_a_hole_during_trading_hours_are_classified_differently() {
        let weekend = vec![
            Timestamp(FRI_CLOSE_UTC - Timestamp::MINUTE),
            Timestamp(SUN_OPEN_UTC),
        ];
        let gaps = find(&weekend, Market::FxWeek);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].kind, GapKind::Calendar);
        assert_eq!(gaps[0].reason, "market closed");
        assert_eq!(gaps[0].missing_open_minutes, 0);

        // Thirty minutes vanishing on a Wednesday morning is a defect.
        let wednesday = FRI_CLOSE_UTC - 2 * Timestamp::DAY;
        let outage = vec![
            Timestamp(wednesday),
            Timestamp(wednesday + 31 * Timestamp::MINUTE),
        ];
        let gaps = find(&outage, Market::FxWeek);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].kind, GapKind::Data);
        assert_eq!(gaps[0].missing_open_minutes, 30);
        assert_ne!(
            gaps[0].kind,
            GapKind::Calendar,
            "a provider outage must never be dressed up as a closed market"
        );
    }

    #[test]
    fn a_whole_missing_session_is_reported_as_a_holiday_not_an_outage() {
        let wednesday = FRI_CLOSE_UTC - 2 * Timestamp::DAY;
        let gaps = find(
            &[Timestamp(wednesday), Timestamp(wednesday + Timestamp::DAY)],
            Market::FxWeek,
        );
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].kind, GapKind::Calendar);
        assert!(gaps[0].reason.contains("holiday"));
    }

    #[test]
    fn consecutive_bars_produce_no_gap_at_all() {
        let t = Timestamp(FRI_CLOSE_UTC - 10 * Timestamp::DAY);
        let times: Vec<Timestamp> = (0..5)
            .map(|i| Timestamp(t.0 + i * Timestamp::MINUTE))
            .collect();
        assert!(find(&times, Market::FxWeek).is_empty());
        assert!(find(&times, Market::Continuous).is_empty());
    }

    #[test]
    fn a_gap_in_crypto_is_always_a_data_gap_because_the_market_never_shuts() {
        let t = Timestamp(FRI_CLOSE_UTC);
        let gaps = find(
            &[t, Timestamp(t.0 + 6 * Timestamp::MINUTE)],
            Market::Continuous,
        );
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].kind, GapKind::Data);
        assert_eq!(gaps[0].missing_open_minutes, 5);
        assert_eq!(gaps[0].from, Timestamp(t.0 + Timestamp::MINUTE));
        assert_eq!(gaps[0].to, Timestamp(t.0 + 5 * Timestamp::MINUTE));
    }
}
