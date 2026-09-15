# ADR 0003: React + TypeScript for the UI

Status: accepted (locked by docs/adr)

## Context
Contributor familiarity outweighs framework preference for an OSS project.

## Decision
React + TypeScript inside the Tauri webview. No additional UI framework until
a concrete need appears.

## Consequences
The UI is a display client only: it receives cursor-filtered data from the
Rust backend and never sees the future (ADR 0005, invariant I1).
