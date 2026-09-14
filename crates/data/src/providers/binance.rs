//! Binance: free full crypto kline history, no key, no signup.
//!
//! `GET /api/v3/klines` returns at most 1000 rows per call, so a range is
//! walked forward in pages. Minutes with no trades are simply absent from the
//! response, which is exactly the gap semantics we want (spec §5.4).
//!
//! Klines are trade prints with no bid/ask, so instruments served here are
//! [`SpreadMode::Synthetic`](replay_core::SpreadMode::Synthetic).

use crate::catalog;
use crate::http;
use replay_core::{Bar, Error, Instrument, Provider, Result, Timestamp};
use serde_json::Value;

const BASE: &str = "https://api.binance.com/api/v3/klines";
const PAGE: i64 = 1000;
pub const ID: &str = "binance";

pub struct Binance;

fn num(v: &Value, idx: usize, what: &str) -> Result<f64> {
    let cell = v
        .get(idx)
        .ok_or_else(|| Error::Data(format!("kline row is missing {what}")))?;
    // Binance sends prices as strings and timestamps as numbers.
    let parsed = match cell {
        Value::String(s) => s.parse::<f64>().ok(),
        Value::Number(n) => n.as_f64(),
        _ => None,
    };
    parsed.ok_or_else(|| Error::Data(format!("kline {what} is not a number: {cell}")))
}

fn parse_page(body: &str) -> Result<Vec<Bar>> {
    let rows: Value = serde_json::from_str(body)
        .map_err(|e| Error::Data(format!("klines response is not JSON: {e}")))?;
    let rows = rows
        .as_array()
        .ok_or_else(|| Error::Data(format!("klines response is not an array: {rows}")))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(Bar {
            ts: Timestamp(num(row, 0, "open time")? as i64),
            open: num(row, 1, "open")?,
            high: num(row, 2, "high")?,
            low: num(row, 3, "low")?,
            close: num(row, 4, "close")?,
            volume: num(row, 5, "volume")?,
        });
    }
    Ok(out)
}

impl Provider for Binance {
    fn id(&self) -> &'static str {
        ID
    }

    fn instruments(&self) -> Result<Vec<Instrument>> {
        Ok(catalog::builtin()
            .into_iter()
            .filter(|i| i.provider == ID)
            .collect())
    }

    fn minutes(&self, inst: &Instrument, from: Timestamp, to: Timestamp) -> Result<Vec<Bar>> {
        let mut out: Vec<Bar> = Vec::new();
        let mut start = from;
        while start < to {
            let url = format!(
                "{BASE}?symbol={}&interval=1m&startTime={}&endTime={}&limit={PAGE}",
                inst.symbol,
                start.0,
                to.0 - 1
            );
            let Some(body) = http::get_string(&url)? else {
                return Err(Error::Provider(format!("unknown symbol {}", inst.symbol)));
            };
            let page = parse_page(&body)?;
            let Some(last) = page.last().map(|b| b.ts) else {
                break; // no more data in range
            };
            out.extend(page.into_iter().filter(|b| from <= b.ts && b.ts < to));
            // Always advance, even if the page ended early, so a stretch with
            // no trades cannot spin forever.
            start = Timestamp(last.0 + Timestamp::MINUTE);
        }
        out.sort_by_key(|b| b.ts);
        out.dedup_by_key(|b| b.ts);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[[1789377240000,"77732.01000000","77740.00000000","77703.47000000","77708.01000000","3.09043000",1789377299999,"240184.17","1165","0.66","51851.39","0"]]"#;

    #[test]
    fn a_kline_row_parses_into_a_bar() {
        let bars = parse_page(SAMPLE).unwrap();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].ts, Timestamp(1_789_377_240_000));
        assert_eq!(bars[0].open, 77732.01);
        assert_eq!(bars[0].high, 77740.0);
        assert_eq!(bars[0].low, 77703.47);
        assert_eq!(bars[0].close, 77708.01);
        assert_eq!(bars[0].volume, 3.09043);
    }

    #[test]
    fn an_empty_page_is_not_an_error() {
        assert!(parse_page("[]").unwrap().is_empty());
    }

    #[test]
    fn a_malformed_row_is_reported_not_skipped() {
        assert!(parse_page(r#"[[1789377240000,"not-a-price"]]"#).is_err());
        assert!(parse_page(r#"{"code":-1121}"#).is_err());
    }
}
