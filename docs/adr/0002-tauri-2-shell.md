# ADR 0002: Tauri 2 as the desktop shell

Status: accepted (locked by docs/adr)

## Context
Electron bundles Chromium (~150 MB installers). Tauri uses the OS webview and
a Rust backend (~10 MB).

## Decision
Tauri 2. The Rust backend hosts the data layer (adapters, Parquet, DuckDB,
replay engine); the webview hosts the React UI.

## Consequences
Contributors need a Rust toolchain. Heavy data work lives in Rust, never in
the webview. Webview differences across OSes need manual checks each milestone.
