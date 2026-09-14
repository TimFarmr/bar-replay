# ADR 0007: Market data flows client → provider directly

Status: accepted (locked by spec §3)

## Context
The user is the data subscriber; this project must never redistribute market
data. Any proxy, cache, or "convenience endpoint" we operate would.

## Decision
The app fetches directly from each provider. No infrastructure operated by
this project ever touches market data. BYOK keys live in the OS keychain
only: never in config files, never logged, never sent anywhere but the
provider's own API.

## Consequences
Network access happens in the Rust backend (no CORS games in the webview).
Rate limits are the user's. No telemetry of any kind.
