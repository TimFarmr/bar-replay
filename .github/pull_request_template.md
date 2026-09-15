## What this changes

<!-- And why. If it fixes an issue, link it. -->

## Correctness

<!--
If this touches the engine or the data layer, which invariant does it bear on?
See docs/invariants.md (I1-I6) and the verification table.
Delete this section for docs-only or UI-only changes.
-->

- [ ] I read the invariant(s) this affects
- [ ] A test covers the behaviour (or I have said why none is possible)

## Checks

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cd ui && npm run build`
- [ ] No new dependency, or I justified it and checked its license is AGPL-compatible
