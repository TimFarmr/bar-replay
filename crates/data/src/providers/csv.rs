//! CSV: candles the user already has, imported from a file.
//!
//! Spec §4 fixes the schema: required columns `timestamp, open, high, low,
//! close`, optional `volume`, timestamps as ISO-8601 or epoch, and rows
//! **strictly ascending** by timestamp — "out-of-order or duplicate rows are
//! rejected with a report, never silently sorted or deduped".
//!
//! That last rule is the reason this module refuses rather than repairs: a
//! silent `sort_by_key` would turn a broken export into a plausible-looking
//! chart, and the trader would backtest against a series that never happened.
//! Every rejection names the 1-based line number in the file the user is
//! looking at. Fields are split on commas and trimmed; quoting and embedded
//! commas are not supported, because every column here is a number or a date.

use chrono_tz::Tz;
use replay_core::{Bar, Error, Instrument, Market, Provider, Result, SpreadMode, Timestamp};
use std::path::PathBuf;

pub const ID: &str = "csv";

/// Epoch timestamps arrive as either seconds or milliseconds and the file does
/// not say which, so they are told apart by magnitude. 1e11 is the only
/// boundary that is unambiguous in practice: as milliseconds it is March 1973,
/// as seconds it is the year 5138, and no candle file contains either.
const EPOCH_MS_FLOOR: i64 = 100_000_000_000;

/// One user-supplied file, served as one instrument.
///
/// A CSV carries no symbol of its own, so the caller supplies the name the
/// trader will see on the chart.
pub struct Csv {
    pub path: PathBuf,
    pub symbol: String,
}

/// Where each schema column sits in this particular file. Columns may appear in
/// any order and their names are matched case-insensitively, because exports
/// from TradingView, MetaTrader and a hand-rolled script all disagree.
struct Columns {
    timestamp: usize,
    open: usize,
    high: usize,
    low: usize,
    close: usize,
    /// `None` when the file has no volume column: §4 makes it optional, and an
    /// absent volume is 0.0 rather than a reason to refuse the file.
    volume: Option<usize>,
    width: usize,
}

fn split(line: &str) -> Vec<&str> {
    line.split(',').map(str::trim).collect()
}

impl Columns {
    fn from_header(header: &str, line: usize) -> Result<Columns> {
        let names = split(header);
        let find = |want: &str| names.iter().position(|n| n.eq_ignore_ascii_case(want));
        let required = |want: &str| {
            find(want).ok_or_else(|| {
                Error::Data(format!(
                    "line {line}: the header has no {want:?} column; a CSV must start with a header row naming timestamp, open, high, low and close (volume is optional), in any order"
                ))
            })
        };
        Ok(Columns {
            timestamp: required("timestamp")?,
            open: required("open")?,
            high: required("high")?,
            low: required("low")?,
            close: required("close")?,
            volume: find("volume"),
            width: names.len(),
        })
    }
}

/// ISO-8601 with an explicit offset, or an epoch count.
///
/// A timestamp with no zone is refused rather than assumed to be UTC: §4 says
/// the session timezone is always shown and never silently assumed, and
/// guessing here would shift every candle in the file by the user's offset
/// without telling them.
fn timestamp(raw: &str, line: usize) -> Result<Timestamp> {
    if let Ok(n) = raw.parse::<i64>() {
        let ms = if n.abs() < EPOCH_MS_FLOOR {
            n * 1_000
        } else {
            n
        };
        return Ok(Timestamp(ms));
    }
    match chrono::DateTime::parse_from_rfc3339(raw) {
        Ok(dt) => Ok(Timestamp(dt.timestamp_millis())),
        Err(_) => Err(Error::Data(format!(
            "line {line}: timestamp {raw:?} is not a date this importer understands; use ISO-8601 with a timezone (2024-01-02T10:00:00Z) or an epoch number in seconds or milliseconds"
        ))),
    }
}

fn number(cells: &[&str], idx: usize, what: &str, line: usize) -> Result<f64> {
    let raw = cells[idx];
    match raw.parse::<f64>() {
        Ok(v) if v.is_finite() => Ok(v),
        _ => Err(Error::Data(format!(
            "line {line}: {what} {raw:?} is not a number"
        ))),
    }
}

/// Parse a whole file. Kept separate from [`Csv`] so the rules above can be
/// tested against a string literal without writing a fixture to disk.
///
/// Returns every bar in file order. Ascending order is a property of the file,
/// checked here, never imposed afterwards.
pub fn parse(text: &str) -> Result<Vec<Bar>> {
    // A UTF-8 BOM is what Excel writes; without this the first column would be
    // named "\u{feff}timestamp" and the file would be rejected for a missing
    // header column, which is a baffling thing to tell a user.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);

    // Line numbers count every physical line so they match the user's editor,
    // but blank lines carry no row: a trailing newline is not an empty candle.
    let mut rows = text
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim_end_matches('\r')))
        .filter(|(_, l)| !l.trim().is_empty());

    let (header_line, header) = rows.next().ok_or_else(|| {
        Error::Data("the file is empty; it needs a header row and at least one candle".into())
    })?;
    let cols = Columns::from_header(header, header_line)?;

    let mut bars: Vec<Bar> = Vec::new();
    let mut previous: Option<(usize, String, Timestamp)> = None;
    for (line, row) in rows {
        let cells = split(row);
        if cells.len() != cols.width {
            return Err(Error::Data(format!(
                "line {line}: this row has {} fields but the header has {}; every row must have one value per column",
                cells.len(),
                cols.width
            )));
        }

        let raw_ts = cells[cols.timestamp].to_string();
        let ts = timestamp(&raw_ts, line)?;
        let open = number(&cells, cols.open, "open", line)?;
        let high = number(&cells, cols.high, "high", line)?;
        let low = number(&cells, cols.low, "low", line)?;
        let close = number(&cells, cols.close, "close", line)?;
        let volume = match cols.volume {
            Some(i) => number(&cells, i, "volume", line)?,
            None => 0.0,
        };

        // An open or close outside the bar's own range is not a rounding
        // artefact, it is a broken export, and it would make every high/low
        // stop in the fill model fire at a price that never traded (§5.3).
        if low > open.min(close) || open.max(close) > high {
            return Err(Error::Data(format!(
                "line {line}: open {open}, high {high}, low {low}, close {close} is not a valid candle; the low must be the lowest price and the high the highest"
            )));
        }

        if let Some((prev_line, prev_raw, prev_ts)) = &previous {
            if ts == *prev_ts {
                return Err(Error::Data(format!(
                    "line {line}: timestamp {raw_ts} repeats line {prev_line} ({prev_raw}); duplicate rows are rejected, never merged"
                )));
            }
            if ts < *prev_ts {
                return Err(Error::Data(format!(
                    "line {line}: timestamp {raw_ts} is not after the previous row ({prev_raw}); rows must be strictly ascending"
                )));
            }
        }

        previous = Some((line, raw_ts, ts));
        bars.push(Bar {
            ts,
            open,
            high,
            low,
            close,
            volume,
        });
    }

    if bars.is_empty() {
        return Err(Error::Data("the file has a header but no candles".into()));
    }
    Ok(bars)
}

impl Provider for Csv {
    fn id(&self) -> &'static str {
        ID
    }

    /// The file is local, so there is no rate limit to respect and no reason to
    /// describe one.
    fn instruments(&self) -> Result<Vec<Instrument>> {
        Ok(vec![Instrument {
            provider: ID.into(),
            symbol: self.symbol.clone(),
            // A CSV says nothing about what it holds, so the import keeps the
            // finest precision we ship anywhere (FX, 5 dp). Showing a trailing
            // zero is harmless; rounding a user's own prices away is not.
            price_decimals: 5,
            point: 1e-5,
            multiplier: 1.0,
            quote_currency: "USD".into(),
            // Nothing in the file tells us the venue or its hours, so the
            // import claims no calendar: every gap is a data gap (§5.4).
            session_tz: Tz::UTC,
            // A CSV of candles has no bid/ask; the spread is a user constant
            // and the UI must say so (§5.3).
            spread_mode: SpreadMode::Synthetic,
            market: Market::Continuous,
        }])
    }

    fn minutes(&self, _inst: &Instrument, from: Timestamp, to: Timestamp) -> Result<Vec<Bar>> {
        let text = std::fs::read_to_string(&self.path)
            .map_err(|e| Error::Provider(format!("could not read {}: {e}", self.path.display())))?;
        Ok(parse(&text)?
            .into_iter()
            .filter(|b| from <= b.ts && b.ts < to)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "\
timestamp,open,high,low,close,volume
2024-01-02T10:00:00Z,1.1,1.2,1.0,1.15,100
2024-01-02T10:01:00Z,1.15,1.25,1.1,1.2,120
";

    fn err(text: &str) -> String {
        match parse(text) {
            Ok(bars) => panic!("expected a rejection, got {} bars", bars.len()),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn a_well_formed_file_parses_in_file_order() {
        let bars = parse(GOOD).unwrap();
        assert_eq!(bars[0].ts, Timestamp(1_704_189_600_000));
        assert_eq!(bars[0].open, 1.1);
        assert_eq!(bars[0].high, 1.2);
        assert_eq!(bars[0].low, 1.0);
        assert_eq!(bars[0].close, 1.15);
        assert_eq!(bars[0].volume, 100.0);
        assert_eq!(bars[1].ts.0 - bars[0].ts.0, Timestamp::MINUTE);
    }

    #[test]
    fn columns_may_be_named_in_any_case_and_any_order() {
        let text = "\
Close,LOW,High,Open,TimeStamp
1.15,1.0,1.2,1.1,2024-01-02T10:00:00Z
";
        let bars = parse(text).unwrap();
        assert_eq!(bars[0].open, 1.1);
        assert_eq!(bars[0].close, 1.15);
        assert_eq!(bars[0].low, 1.0);
        assert_eq!(bars[0].high, 1.2);
    }

    #[test]
    fn volume_is_optional_and_defaults_to_zero() {
        let text = "\
timestamp,open,high,low,close
2024-01-02T10:00:00Z,1.1,1.2,1.0,1.15
";
        assert_eq!(parse(text).unwrap()[0].volume, 0.0);
    }

    #[test]
    fn epoch_seconds_and_milliseconds_are_told_apart_by_magnitude() {
        let secs = "timestamp,open,high,low,close\n1704189600,1,1,1,1\n";
        let ms = "timestamp,open,high,low,close\n1704189600000,1,1,1,1\n";
        assert_eq!(parse(secs).unwrap()[0].ts, Timestamp(1_704_189_600_000));
        assert_eq!(parse(ms).unwrap()[0].ts, Timestamp(1_704_189_600_000));
    }

    #[test]
    fn an_iso_timestamp_with_an_offset_is_converted_to_utc() {
        let text = "timestamp,open,high,low,close\n2024-01-02T11:00:00+01:00,1,1,1,1\n";
        assert_eq!(parse(text).unwrap()[0].ts, Timestamp(1_704_189_600_000));
    }

    #[test]
    fn a_timestamp_without_a_timezone_is_refused_not_assumed_to_be_utc() {
        let msg = err("timestamp,open,high,low,close\n2024-01-02 10:00:00,1,1,1,1\n");
        assert!(msg.contains("line 2"), "{msg}");
        assert!(msg.contains("ISO-8601"), "{msg}");
    }

    #[test]
    fn an_out_of_order_row_is_rejected_and_never_sorted() {
        let text = "\
timestamp,open,high,low,close
2024-01-02T10:00:00Z,1,1,1,1
2024-01-02T09:00:00Z,1,1,1,1
";
        let msg = err(text);
        assert!(msg.contains("line 3"), "{msg}");
        assert!(msg.contains("strictly ascending"), "{msg}");
        assert!(msg.contains("2024-01-02T09:00:00Z"), "{msg}");
        assert!(msg.contains("2024-01-02T10:00:00Z"), "{msg}");
    }

    #[test]
    fn a_duplicate_timestamp_is_rejected_and_never_deduped() {
        let text = "\
timestamp,open,high,low,close
2024-01-02T10:00:00Z,1,1,1,1
2024-01-02T10:00:00Z,2,2,2,2
";
        let msg = err(text);
        assert!(msg.contains("line 3"), "{msg}");
        assert!(msg.contains("duplicate"), "{msg}");
    }

    #[test]
    fn a_missing_required_column_names_the_column() {
        let msg = err("timestamp,open,high,low\n2024-01-02T10:00:00Z,1,1,1\n");
        assert!(msg.contains("line 1"), "{msg}");
        assert!(msg.contains("\"close\""), "{msg}");
    }

    #[test]
    fn a_malformed_number_names_the_line_and_the_field() {
        let msg = err("timestamp,open,high,low,close\n2024-01-02T10:00:00Z,1,1,oops,1\n");
        assert!(msg.contains("line 2"), "{msg}");
        assert!(msg.contains("low"), "{msg}");
        assert!(msg.contains("oops"), "{msg}");
    }

    #[test]
    fn a_row_with_the_wrong_field_count_is_rejected() {
        let msg = err("timestamp,open,high,low,close\n2024-01-02T10:00:00Z,1,1,1\n");
        assert!(msg.contains("line 2"), "{msg}");
        assert!(msg.contains("4 fields"), "{msg}");
    }

    #[test]
    fn a_price_outside_the_bars_own_range_is_rejected() {
        for row in ["1.1,1.2,1.0,1.9", "0.5,1.2,1.0,1.1"] {
            let msg = err(&format!(
                "timestamp,open,high,low,close\n2024-01-02T10:00:00Z,{row}\n"
            ));
            assert!(msg.contains("line 2"), "{msg}");
            assert!(msg.contains("valid candle"), "{msg}");
        }
    }

    #[test]
    fn an_excel_byte_order_mark_does_not_hide_the_first_column() {
        let bars = parse(&format!("\u{feff}{GOOD}")).unwrap();
        assert_eq!(bars.len(), 2);
    }

    #[test]
    fn a_file_with_no_candles_is_rejected_rather_than_imported_empty() {
        assert!(err("timestamp,open,high,low,close\n").contains("no candles"));
        assert!(err("").contains("empty"));
    }

    #[test]
    fn minutes_returns_only_the_half_open_range() {
        let dir = std::env::temp_dir().join("replay-csv-range-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bars.csv");
        std::fs::write(&path, GOOD).unwrap();
        let csv = Csv {
            path,
            symbol: "MYDATA".into(),
        };
        let inst = csv.instruments().unwrap().remove(0);
        assert_eq!(inst.symbol, "MYDATA");
        assert_eq!(inst.provider, ID);

        let first = Timestamp(1_704_189_600_000);
        let bars = csv
            .minutes(&inst, first, Timestamp(first.0 + Timestamp::MINUTE))
            .unwrap();
        assert_eq!(bars.len(), 1, "`to` is exclusive");
        assert_eq!(bars[0].ts, first);
    }
}
