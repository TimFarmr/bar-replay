//! Local export of a session's results to CSV and JSON.
//!
//! Everything is written with fixed precision — prices to the instrument's own
//! decimals, money to two — so "the same session exports byte-identically"
//! is a statement that actually means something (ADR 0010). It is also what
//! lets the determinism test diff two runs.
//!
//! Exports are files on the user's disk. Nothing is uploaded anywhere.

use crate::journal::JournalEntry;
use crate::order::{RoundTrip, Trade};
use crate::stats::Summary;
use replay_core::Timestamp;

fn when(ts: Timestamp) -> String {
    chrono::DateTime::from_timestamp_millis(ts.0)
        .map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| ts.0.to_string())
}

/// `-0.00` is arithmetically zero but reads like a loss. Normalise it so a
/// commission-free entry does not look like it cost something.
fn zeroed(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

/// Wrap a field for CSV only when it could otherwise break the row.
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// The fill-by-fill ledger, one row per fill (ADR 0009).
pub fn trades_csv(trades: &[Trade], decimals: usize) -> String {
    let mut out = String::from(
        "seq,time_utc,order,side,qty,price,role,reason,assumption,commission,realised\n",
    );
    for t in trades {
        out.push_str(&format!(
            "{},{},{},{:?},{:.4},{:.*},{:?},{:?},{:?},{:.2},{:.2}\n",
            t.seq,
            when(t.cursor),
            t.order,
            t.side,
            t.qty,
            decimals,
            t.price,
            t.role,
            t.reason,
            t.assumption,
            zeroed(t.commission),
            zeroed(t.realised),
        ));
    }
    out.to_lowercase()
}

/// Completed entry-to-exit trades, which is what a trader reviews.
pub fn round_trips_csv(trips: &[RoundTrip], decimals: usize) -> String {
    let mut out =
        String::from("opened_utc,closed_utc,side,qty,entry,exit,realised,r_multiple,assumption\n");
    for t in trips {
        out.push_str(&format!(
            "{},{},{:?},{:.4},{:.*},{:.*},{:.2},{},{:?}\n",
            when(t.opened),
            when(t.closed),
            t.side,
            t.qty,
            decimals,
            t.entry,
            decimals,
            t.exit,
            zeroed(t.realised),
            t.r_multiple.map_or(String::new(), |r| format!("{r:.3}")),
            t.assumption,
        ));
    }
    out.to_lowercase()
}

pub fn journal_csv(entries: &[JournalEntry]) -> String {
    let mut out = String::from("time_utc,trade,tags,text\n");
    for e in entries {
        out.push_str(&format!(
            "{},{},{},{}\n",
            when(e.cursor),
            e.trade.map_or(String::new(), |t| t.to_string()),
            csv_field(&e.tags.join(" ")),
            csv_field(&e.text),
        ));
    }
    out
}

/// Everything in one document, for a trader who would rather keep a single
/// file or feed it to their own tools.
pub fn session_json(
    summary: &Summary,
    trades: &[Trade],
    trips: &[RoundTrip],
    journal: &[JournalEntry],
) -> Result<String, String> {
    let doc = serde_json::json!({
        "summary": summary,
        "trades": trades,
        "round_trips": trips,
        "journal": journal,
    });
    serde_json::to_string_pretty(&doc).map_err(|e| format!("encoding export: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::{Assumption, Reason, Role, Side};

    fn trade() -> Trade {
        Trade {
            seq: 1,
            cursor: Timestamp(1_704_067_200_000), // 2024-01-01T00:00:00Z
            order: 7,
            side: Side::Buy,
            qty: 1.5,
            price: 1.234_56,
            role: Role::Entry,
            reason: Reason::Market,
            assumption: Assumption::None,
            commission: 0.5,
            realised: -0.5,
            risk_per_unit: Some(0.002),
        }
    }

    #[test]
    fn a_trade_row_carries_fixed_precision_and_a_readable_time() {
        let csv = trades_csv(&[trade()], 5);
        let row = csv.lines().nth(1).unwrap();
        assert_eq!(
            row,
            "1,2024-01-01t00:00:00z,7,buy,1.5000,1.23456,entry,market,none,0.50,-0.50"
        );
    }

    #[test]
    fn the_same_ledger_exports_byte_identically_twice() {
        let trades = [trade(), trade()];
        assert_eq!(trades_csv(&trades, 5), trades_csv(&trades, 5));
    }

    #[test]
    fn a_missing_r_multiple_is_blank_rather_than_a_fake_number() {
        let trip = RoundTrip {
            opened: Timestamp(0),
            closed: Timestamp(60_000),
            side: Side::Buy,
            qty: 1.0,
            entry: 100.0,
            exit: 101.0,
            realised: 1.0,
            r_multiple: None,
            assumption: Assumption::None,
        };
        let row = round_trips_csv(&[trip], 2);
        assert!(row.lines().nth(1).unwrap().ends_with("1.00,,none"), "{row}");
    }

    #[test]
    fn notes_with_commas_and_quotes_survive_the_csv_round_trip() {
        let entry = JournalEntry {
            id: 1,
            cursor: Timestamp(0),
            text: "chased it, again \"badly\"".into(),
            tags: vec!["fomo".into()],
            trade: Some(3),
        };
        let csv = journal_csv(&[entry]);
        let row = csv.lines().nth(1).unwrap();
        assert!(row.contains("\"chased it, again \"\"badly\"\"\""), "{row}");
        // The header plus exactly one record: the embedded newline-free text
        // must not have split the row.
        assert_eq!(csv.lines().count(), 2);
    }
}
