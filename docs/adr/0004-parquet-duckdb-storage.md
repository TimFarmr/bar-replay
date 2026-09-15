# ADR 0004: Local Parquet files + DuckDB for queries

Status: accepted (locked by docs/adr)

## Context
Years of 1-minute bars are tens of millions of rows. Timeframe aggregation
must be fast and happen on demand (the data policy: never store higher timeframes).
No server is allowed.

## Decision
One Parquet file per instrument holds the base 1-minute series. DuckDB,
bundled in-process, runs range scans and aggregation as SQL. DuckDB also
writes the Parquet files, so no separate Parquet library is needed.

## Consequences
DuckDB (MIT) is the one large dependency. Schema changes to cached data mean
rewriting Parquet files; the cache is disposable, session copies are not
(ADR 0012).
