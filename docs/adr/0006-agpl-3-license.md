# ADR 0006: AGPL-3.0-only

Status: accepted (locked by spec §3)

## Decision
The project is licensed AGPL-3.0-only to prevent a closed SaaS fork.

## Consequences
Every dependency must be AGPL-compatible (MIT, Apache-2.0, BSD are). Each new
dependency is justified in its PR with what it replaces and its license
(spec §8).
