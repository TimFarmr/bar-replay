# ADR 0001: Desktop app with signed installers

Status: accepted (locked by spec §3)

## Context
The loss function: a non-technical trader does a first honest replay within
5 minutes, with no account, no key, no terminal. A web app would need hosting,
which is a server we operate (forbidden by ADR 0007), and browser storage is
too small for years of minute bars.

## Decision
Native desktop app. Signed installers for Windows, macOS and Linux.

## Consequences
Code-signing certificates are needed before M5. Updates ship as new installers;
auto-update is deferred. There is never a server component.
