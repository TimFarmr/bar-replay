//! Databento: CME futures and equities, bring-your-own-key.
//!
//! BYOK is strictly an upgrade path (spec §4). Nothing here is reachable
//! without a key the user pasted themselves, the two free adapters keep
//! working when it is absent, and a missing key produces a sentence telling
//! them where to add one — never a panic and never a hang.
//!
//! # The wire format below is NOT verified against the live API
//!
//! Everything in this module about Databento's HTTP surface — the endpoint,
//! the parameter names, HTTP Basic with the key as username and an empty
//! password, newline-delimited JSON records, `hd.ts_event` in nanoseconds and
//! prices as integers scaled by 1e-9 — is written from Databento's published
//! documentation. Calling it needs a paid subscription, so it has never been
//! run against the real service. [`parse_ohlcv`] is therefore a pure function
//! tested against a hand-written sample of the documented shape: the tests
//! prove the parser matches the docs, not that the docs match the server.
//!
//! One detail in particular is unresolved: `instruments()` offers "NQ" and
//! "ES", which are product roots rather than tradable contract symbols. A real
//! request probably needs either a specific contract ("NQZ5") or a continuous
//! symbol with `stype_in=continuous` ("NQ.c.0"). Which one is right has to be
//! settled against the live API before this adapter ships.

use crate::{http, keychain};
use chrono_tz::Tz;
use replay_core::{Bar, Error, Instrument, Market, Provider, Result, SpreadMode, Timestamp};
use serde_json::Value;

pub const ID: &str = "databento";

const BASE: &str = "hist.databento.com/v0/timeseries.get_range";

/// CME Globex, the dataset holding both instruments below.
const DATASET: &str = "GLBX.MDP3";

/// Databento publishes prices as fixed-point integers with nine decimal
/// places, so 20000.25 arrives as 20000250000000.
const PRICE_SCALE: f64 = 1e-9;

pub struct Databento;

/// The contracts this adapter knows about without a symbology lookup.
///
/// Multipliers are the exchange's own contract sizes and drive P&L directly,
/// so a drift here is a wrong answer in dollars: NQ is $20 per index point,
/// ES is $50.
fn future(symbol: &str, multiplier: f64) -> Instrument {
    Instrument {
        provider: ID.into(),
        symbol: symbol.into(),
        price_decimals: 2,
        point: 0.25,
        multiplier,
        quote_currency: "USD".into(),
        // Spec §4 names America/New_York as the session zone for CME futures.
        session_tz: Tz::America__New_York,
        // ohlcv-1m is built from trade prints, so there is no historical
        // spread here and the UI must say so (§5.3).
        spread_mode: SpreadMode::Synthetic,
        // CME's daily maintenance halt and holiday calendar are not modelled
        // yet, so every gap will read as a data gap (§5.4). Adding a variant to
        // `Market` is a core change and belongs in its own piece of work;
        // claiming `FxWeek` here would be worse, because the CME week is not
        // the FX week.
        market: Market::Continuous,
    }
}

fn builtin() -> Vec<Instrument> {
    vec![future("NQ", 20.0), future("ES", 50.0)]
}

/// Databento's error bodies quote the request, and our request carries the key
/// as HTTP Basic userinfo, so every message leaving this module is scrubbed.
/// Spec §4: keys are never logged.
fn redact(message: String, key: &str) -> String {
    message.replace(key, "<api key>")
}

fn rfc3339(ts: Timestamp, what: &str) -> Result<String> {
    chrono::DateTime::from_timestamp_millis(ts.0)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .ok_or_else(|| Error::Provider(format!("{what} {} is not a real date", ts.0)))
}

/// Databento renders integers as JSON numbers, but its own examples show some
/// 64-bit fields as strings, because a nanosecond timestamp does not survive a
/// JSON parser that uses doubles. Accept both rather than guess.
fn int(record: &Value, field: &str, record_no: usize) -> Result<i64> {
    let cell = record
        .get(field)
        .ok_or_else(|| Error::Data(format!("record {record_no} has no {field:?} field")))?;
    let parsed = match cell {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse::<i64>().ok(),
        _ => None,
    };
    parsed.ok_or_else(|| {
        Error::Data(format!(
            "record {record_no}: {field} is not a whole number: {cell}"
        ))
    })
}

/// Parse a newline-delimited `ohlcv-1m` response into bars with
/// `from <= ts < to`, ascending and unique.
///
/// Pure on purpose: see the module note above. This is the only part of the
/// adapter that can be tested without a paid key.
pub fn parse_ohlcv(text: &str, from: Timestamp, to: Timestamp) -> Result<Vec<Bar>> {
    let mut out: Vec<Bar> = Vec::new();
    for (i, line) in text.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        let record_no = i + 1;
        let record: Value = serde_json::from_str(line)
            .map_err(|e| Error::Data(format!("record {record_no} is not JSON: {e}")))?;
        let header = record
            .get("hd")
            .ok_or_else(|| Error::Data(format!("record {record_no} has no \"hd\" header")))?;
        // ts_event is nanoseconds since the epoch; the whole app works in
        // milliseconds, and a 1-minute bar always opens on a whole second, so
        // this division loses nothing.
        let ts = Timestamp(int(header, "ts_event", record_no)? / 1_000_000);
        let px =
            |field: &str| Ok::<f64, Error>(int(&record, field, record_no)? as f64 * PRICE_SCALE);
        out.push(Bar {
            ts,
            open: px("open")?,
            high: px("high")?,
            low: px("low")?,
            close: px("close")?,
            volume: int(&record, "volume", record_no)? as f64,
        });
    }
    out.retain(|b| from <= b.ts && b.ts < to);
    out.sort_by_key(|b| b.ts);
    out.dedup_by_key(|b| b.ts);
    Ok(out)
}

impl Provider for Databento {
    fn id(&self) -> &'static str {
        ID
    }

    /// Listing what the adapter *could* serve needs no key: the settings screen
    /// has to be able to show a trader what a key would buy them.
    fn instruments(&self) -> Result<Vec<Instrument>> {
        Ok(builtin())
    }

    fn minutes(&self, inst: &Instrument, from: Timestamp, to: Timestamp) -> Result<Vec<Bar>> {
        let key = keychain::get(ID).ok_or_else(|| {
            Error::Provider(
                "no Databento API key is stored. Databento is optional: add your key under \
                 Settings > Data providers to use CME futures, or pick a Dukascopy or Binance \
                 instrument, which need no key at all."
                    .into(),
            )
        })?;

        // HTTP Basic with the key as the username and an empty password, sent
        // as URL userinfo so the request goes through `http`, which already
        // retries the 429s and 503s a historical API hands out under load. The
        // key therefore lives inside `url`, and `http` quotes the URL in its
        // error text, so everything that can escape this call is redacted.
        let url = format!(
            "https://{key}:@{BASE}?dataset={DATASET}&symbols={}&schema=ohlcv-1m\
             &start={}&end={}&encoding=json",
            inst.symbol,
            rfc3339(from, "start")?,
            rfc3339(to, "end")?,
        );

        let body = match http::get_string(&url) {
            Ok(Some(body)) => body,
            // `http` turns 404 into "the provider has no file there", which for
            // a REST endpoint means our path is wrong, not that the range is
            // empty.
            Ok(None) => {
                return Err(Error::Provider(format!(
                    "Databento returned no {DATASET} endpoint for {}; this adapter may be \
                     pointed at an out-of-date API version",
                    inst.symbol
                )))
            }
            Err(e) => return Err(Error::Provider(redact(e.to_string(), &key))),
        };

        parse_ohlcv(&body, from, to).map_err(|e| match e {
            Error::Data(m) => Error::Data(redact(m, &key)),
            other => other,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-written from Databento's documented `ohlcv-1m` JSON shape. It has
    /// never round-tripped through the live API; see the module note.
    const SAMPLE: &str = r#"
{"hd":{"ts_event":"1704189600000000000","rtype":32,"publisher_id":1,"instrument_id":42},"open":"20000250000000","high":"20010000000000","low":"19995500000000","close":"20005750000000","volume":"1234"}
{"hd":{"ts_event":1704189660000000000,"rtype":32,"publisher_id":1,"instrument_id":42},"open":20005750000000,"high":20020000000000,"low":20001000000000,"close":20018250000000,"volume":987}
"#;

    const OPEN: Timestamp = Timestamp(1_704_189_600_000);
    const WIDE: (Timestamp, Timestamp) = (Timestamp(0), Timestamp(i64::MAX));

    #[test]
    fn documented_records_parse_whether_fields_are_strings_or_numbers() {
        let bars = parse_ohlcv(SAMPLE, WIDE.0, WIDE.1).unwrap();
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].ts, OPEN);
        assert_eq!(bars[0].open, 20000.25);
        assert_eq!(bars[0].high, 20010.0);
        assert_eq!(bars[0].low, 19995.5);
        assert_eq!(bars[0].close, 20005.75);
        assert_eq!(bars[0].volume, 1234.0);
        assert_eq!(bars[1].ts.0 - bars[0].ts.0, Timestamp::MINUTE);
        assert_eq!(bars[1].open, 20005.75);
        assert_eq!(bars[1].volume, 987.0);
    }

    #[test]
    fn the_range_is_half_open() {
        let bars = parse_ohlcv(SAMPLE, OPEN, Timestamp(OPEN.0 + Timestamp::MINUTE)).unwrap();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].ts, OPEN);
        assert!(parse_ohlcv("", WIDE.0, WIDE.1).unwrap().is_empty());
    }

    #[test]
    fn a_malformed_record_is_reported_with_its_position_not_skipped() {
        let e = parse_ohlcv("{not json}", WIDE.0, WIDE.1).unwrap_err();
        assert!(e.to_string().contains("record 1"), "{e}");

        let no_header = r#"{"open":1,"high":1,"low":1,"close":1,"volume":1}"#;
        assert!(parse_ohlcv(no_header, WIDE.0, WIDE.1)
            .unwrap_err()
            .to_string()
            .contains("\"hd\""));

        let bad_price = r#"{"hd":{"ts_event":1704189600000000000},"open":"20000.25","high":1,"low":1,"close":1,"volume":1}"#;
        let e = parse_ohlcv(bad_price, WIDE.0, WIDE.1).unwrap_err();
        assert!(e.to_string().contains("open"), "{e}");
    }

    #[test]
    fn contract_multipliers_are_the_exchange_values() {
        let insts = builtin();
        let nq = insts.iter().find(|i| i.symbol == "NQ").unwrap();
        let es = insts.iter().find(|i| i.symbol == "ES").unwrap();
        assert_eq!(nq.multiplier, 20.0);
        assert_eq!(es.multiplier, 50.0);
        for i in &insts {
            assert_eq!(i.provider, ID);
            assert_eq!(i.point, 0.25);
            assert_eq!(i.price_decimals, 2);
            assert_eq!(i.session_tz, Tz::America__New_York);
        }
    }

    #[test]
    fn a_redacted_message_cannot_carry_the_key() {
        let key = "db-0123456789abcdef";
        let leaky = format!("https://{key}:@{BASE}?dataset=GLBX.MDP3: HTTP 401");
        let safe = redact(leaky, key);
        assert!(!safe.contains(key), "{safe}");
        assert!(safe.contains("<api key>"), "{safe}");
    }

    /// Without a key the adapter must explain itself and return, not panic and
    /// not reach the network. Skipped on a machine that actually has a key
    /// stored, because there the call would be a real paid request.
    #[test]
    fn a_missing_key_is_a_friendly_refusal() {
        if keychain::get(ID).is_some() {
            return;
        }
        let inst = builtin().remove(0);
        let e = Databento
            .minutes(&inst, OPEN, Timestamp(OPEN.0 + Timestamp::MINUTE))
            .unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("no Databento API key"), "{msg}");
        assert!(msg.contains("Settings"), "{msg}");
    }
}
