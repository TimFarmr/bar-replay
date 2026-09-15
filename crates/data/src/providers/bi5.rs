//! Decoding for Dukascopy `.bi5` payloads, kept apart from the HTTP adapter so
//! it can be tested against committed fixtures with no network.

use chrono::{DateTime, Datelike, Utc};
use replay_core::{Bar, Error, Result, Tick, Timestamp};

const MINUTE_REC: usize = 24;
const TICK_REC: usize = 20;

fn utc(ts: Timestamp) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ts.0).expect("timestamp in range")
}

pub fn ymd(ts: Timestamp) -> (i32, u32, u32) {
    let d = utc(ts);
    (d.year(), d.month(), d.day())
}

/// Every UTC midnight whose day overlaps `[from, to)`.
pub fn days_covering(from: Timestamp, to: Timestamp) -> Vec<Timestamp> {
    step_covering(from, to, Timestamp::DAY)
}

/// Every UTC hour start whose hour overlaps `[from, to)`.
pub fn hours_covering(from: Timestamp, to: Timestamp) -> Vec<Timestamp> {
    step_covering(from, to, Timestamp::HOUR)
}

fn step_covering(from: Timestamp, to: Timestamp, step: i64) -> Vec<Timestamp> {
    let mut out = Vec::new();
    let mut t = from.floor(step);
    while t < to {
        out.push(t);
        t = Timestamp(t.0 + step);
    }
    out
}

/// `.bi5` bodies are raw LZMA1 streams. An empty body means "no data here",
/// which Dukascopy also uses for closed sessions.
fn inflate(raw: &[u8], what: &str) -> Result<Vec<u8>> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    lzma_rs::lzma_decompress(&mut std::io::Cursor::new(raw), &mut out)
        .map_err(|e| Error::Data(format!("{what}: corrupt LZMA payload: {e}")))?;
    Ok(out)
}

fn be_i32(b: &[u8], at: usize) -> i32 {
    i32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn be_f32(b: &[u8], at: usize) -> f32 {
    f32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Decode one day of minute bars. `day` is the UTC midnight the file covers.
///
/// Dukascopy pads minutes that had no ticks with a flat zero-volume record
/// (open == high == low == close). Those are forward-fill, not data, so they
/// are dropped: a minute with no trading is a gap (invariant I4).
pub fn decode_minutes(raw: &[u8], day: Timestamp, point: f64) -> Result<Vec<Bar>> {
    let body = inflate(raw, "minute file")?;
    if body.len() % MINUTE_REC != 0 {
        return Err(Error::Data(format!(
            "minute file: {} bytes is not a multiple of {MINUTE_REC}",
            body.len()
        )));
    }
    let mut out = Vec::with_capacity(body.len() / MINUTE_REC);
    for rec in body.as_chunks::<MINUTE_REC>().0 {
        let volume = be_f32(rec, 20) as f64;
        if volume == 0.0 {
            continue;
        }
        let secs = be_i32(rec, 0) as i64;
        out.push(Bar {
            ts: Timestamp(day.0 + secs * 1000),
            open: be_i32(rec, 4) as f64 * point,
            close: be_i32(rec, 8) as f64 * point,
            low: be_i32(rec, 12) as f64 * point,
            high: be_i32(rec, 16) as f64 * point,
            volume,
        });
    }
    Ok(out)
}

/// Decode one hour of ticks. `hour` is the UTC hour start the file covers.
pub fn decode_ticks(raw: &[u8], hour: Timestamp, point: f64) -> Result<Vec<Tick>> {
    let body = inflate(raw, "tick file")?;
    if body.len() % TICK_REC != 0 {
        return Err(Error::Data(format!(
            "tick file: {} bytes is not a multiple of {TICK_REC}",
            body.len()
        )));
    }
    let mut out = Vec::with_capacity(body.len() / TICK_REC);
    for rec in body.as_chunks::<TICK_REC>().0 {
        out.push(Tick {
            ts: Timestamp(hour.0 + be_i32(rec, 0) as i64),
            ask: be_i32(rec, 4) as f64 * point,
            bid: be_i32(rec, 8) as f64 * point,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_payload_is_no_data_not_an_error() {
        assert!(decode_minutes(&[], Timestamp(0), 1e-5).unwrap().is_empty());
        assert!(decode_ticks(&[], Timestamp(0), 1e-5).unwrap().is_empty());
    }

    #[test]
    fn days_and_hours_cover_partial_ranges_at_both_ends() {
        let from = Timestamp(Timestamp::DAY + 5 * Timestamp::HOUR);
        let to = Timestamp(2 * Timestamp::DAY + Timestamp::HOUR);
        assert_eq!(
            days_covering(from, to),
            vec![Timestamp(Timestamp::DAY), Timestamp(2 * Timestamp::DAY)]
        );
        // hours 05:00-23:00 of the first day, plus 00:00 of the second.
        assert_eq!(hours_covering(from, to).len(), 20);
    }

    #[test]
    fn a_truncated_record_is_rejected_rather_than_guessed() {
        // 13-byte LZMA-alone header with a body that cannot decode.
        let junk = [
            0x5du8, 0, 0, 0x80, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00,
        ];
        assert!(decode_minutes(&junk, Timestamp(0), 1e-5).is_err());
    }
}
