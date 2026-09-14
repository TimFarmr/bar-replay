//! Parquet storage and cursor-filtered queries (ADR 0004).
//!
//! DuckDB runs in-process and both writes and reads the Parquet files, so no
//! separate Parquet library is needed and there is no server anywhere.
//!
//! **The no-lookahead filter lives here** (spec §5.1): `ts <= cursor` is part
//! of every query, so data past the cursor never reaches the engine, let alone
//! the chart.

use chrono_tz::Tz;
use duckdb::Connection;
use replay_core::{Bar, Candle, Error, Result, Timeframe, Timestamp};
use std::path::Path;

pub struct Store {
    conn: Connection,
}

/// What a cached base series actually holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coverage {
    pub from: Timestamp,
    pub to: Timestamp,
    pub bars: i64,
}

fn db(context: &str, e: duckdb::Error) -> Error {
    Error::Data(format!("{context}: {e}"))
}

/// Parquet paths are interpolated, not bound, because DuckDB only accepts a
/// constant there. Single quotes are doubled so a path can never terminate the
/// literal; paths come from our own layout, never from provider input.
fn sql_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").replace('\'', "''")
}

impl Store {
    pub fn open() -> Result<Store> {
        let conn = Connection::open_in_memory().map_err(|e| db("opening DuckDB", e))?;
        Ok(Store { conn })
    }

    /// Merge `bars` into the Parquet file at `path`, keeping it sorted and
    /// unique by `ts`. Incoming bars win on conflict so a provider correction
    /// replaces the stale row. Returns the total row count afterwards.
    pub fn upsert_bars(&self, path: &Path, bars: &[Bar]) -> Result<i64> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        self.conn
            .execute_batch(
                "CREATE OR REPLACE TEMP TABLE incoming
                 (ts BIGINT, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, volume DOUBLE);",
            )
            .map_err(|e| db("creating staging table", e))?;

        {
            let mut app = self
                .conn
                .appender("incoming")
                .map_err(|e| db("opening appender", e))?;
            for b in bars {
                app.append_row(duckdb::params![
                    b.ts.0, b.open, b.high, b.low, b.close, b.volume
                ])
                .map_err(|e| db("appending bar", e))?;
            }
        }

        let p = sql_path(path);
        let merged = if path.exists() {
            format!(
                "SELECT * FROM read_parquet('{p}') old
                 WHERE NOT EXISTS (SELECT 1 FROM incoming i WHERE i.ts = old.ts)
                 UNION ALL SELECT * FROM incoming"
            )
        } else {
            "SELECT * FROM incoming".to_string()
        };

        // DuckDB cannot COPY onto a file it is reading, so write beside it and
        // swap. The swap is also what keeps a crash mid-write from truncating
        // an existing cache.
        let tmp = path.with_extension("parquet.tmp");
        let tp = sql_path(&tmp);
        self.conn
            .execute_batch(&format!(
                "COPY ({merged} ORDER BY ts) TO '{tp}' (FORMAT PARQUET);"
            ))
            .map_err(|e| db("writing parquet", e))?;
        std::fs::rename(&tmp, path)?;

        self.coverage(path).map(|c| c.map_or(0, |c| c.bars))
    }

    pub fn coverage(&self, path: &Path) -> Result<Option<Coverage>> {
        if !path.exists() {
            return Ok(None);
        }
        let p = sql_path(path);
        let row = self
            .conn
            .query_row(
                &format!("SELECT min(ts), max(ts), count(*) FROM read_parquet('{p}')"),
                [],
                |r| {
                    Ok((
                        r.get::<_, Option<i64>>(0)?,
                        r.get::<_, Option<i64>>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            )
            .map_err(|e| db("reading coverage", e))?;
        Ok(match row {
            (Some(from), Some(to), bars) => Some(Coverage {
                from: Timestamp(from),
                to: Timestamp(to),
                bars,
            }),
            _ => None,
        })
    }

    /// Base bars with `from <= ts <= cursor`, ascending.
    pub fn bars(&self, path: &Path, from: Timestamp, cursor: Timestamp) -> Result<Vec<Bar>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let p = sql_path(path);
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT ts, open, high, low, close, volume FROM read_parquet('{p}')
                 WHERE ts >= ? AND ts <= ? ORDER BY ts"
            ))
            .map_err(|e| db("preparing bar scan", e))?;
        let rows = stmt
            .query_map(duckdb::params![from.0, cursor.0], |r| {
                Ok(Bar {
                    ts: Timestamp(r.get(0)?),
                    open: r.get(1)?,
                    high: r.get(2)?,
                    low: r.get(3)?,
                    close: r.get(4)?,
                    volume: r.get(5)?,
                })
            })
            .map_err(|e| db("scanning bars", e))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| db("reading bars", e))
    }

    /// Aggregate the base series into `tf` candles covering `[from, cursor]`.
    ///
    /// Fixed-width timeframes bucket arithmetically. Daily and weekly join
    /// against boundaries computed by [`Timeframe::bucket_starts`], so DuckDB
    /// never does timezone maths and cannot disagree with the engine.
    pub fn candles(
        &self,
        path: &Path,
        tf: Timeframe,
        tz: Tz,
        from: Timestamp,
        cursor: Timestamp,
    ) -> Result<Vec<Candle>> {
        if !path.exists() || cursor < from {
            return Ok(Vec::new());
        }
        let p = sql_path(path);
        // §5.1: the cursor filter is part of the scan, not a later step.
        let src = format!(
            "(SELECT * FROM read_parquet('{p}') WHERE ts >= {} AND ts <= {})",
            tf.bucket_start(from, tz).0,
            cursor.0
        );
        let sql = match tf.fixed_ms() {
            // Exact integer floor for any sign, unlike `//` which truncates.
            Some(ms) => format!(
                "SELECT s.ts - ((s.ts % {ms}) + {ms}) % {ms} AS bucket,
                        first(s.open ORDER BY s.ts) AS open, max(s.high) AS high,
                        min(s.low) AS low, last(s.close ORDER BY s.ts) AS close,
                        sum(s.volume) AS volume
                 FROM {src} s GROUP BY bucket ORDER BY bucket"
            ),
            None => {
                let starts = tf.bucket_starts(from, cursor, tz);
                if starts.is_empty() {
                    return Ok(Vec::new());
                }
                let values: Vec<String> = starts
                    .iter()
                    .map(|s| format!("({},{})", s.0, tf.next_bucket(*s, tz).0))
                    .collect();
                format!(
                    "WITH buckets(start, stop) AS (VALUES {})
                     SELECT b.start AS bucket,
                            first(s.open ORDER BY s.ts) AS open, max(s.high) AS high,
                            min(s.low) AS low, last(s.close ORDER BY s.ts) AS close,
                            sum(s.volume) AS volume
                     FROM {src} s JOIN buckets b ON s.ts >= b.start AND s.ts < b.stop
                     GROUP BY b.start ORDER BY b.start",
                    values.join(",")
                )
            }
        };

        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| db("preparing aggregation", e))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Candle {
                    ts: Timestamp(r.get(0)?),
                    open: r.get(1)?,
                    high: r.get(2)?,
                    low: r.get(3)?,
                    close: r.get(4)?,
                    volume: r.get(5)?,
                    complete: true,
                })
            })
            .map_err(|e| db("aggregating", e))?;
        let mut out: Vec<Candle> = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| db("reading candles", e))?;

        // Only the newest candle can still be forming (§5.2).
        if let Some(last) = out.last_mut() {
            last.complete = tf.next_bucket(last.ts, tz).0 <= cursor.0 + Timestamp::MINUTE;
        }
        Ok(out)
    }
}
