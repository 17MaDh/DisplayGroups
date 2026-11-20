## What this changes

<!-- What and why, in a few sentences. Link the issue it fixes, for example "Fixes #12". -->

## How it was tested

<!--
For changes to switching, say what you switched on: the GPU and driver, the displays and how
they are connected, the Windows build or Ubuntu version, and the groups you switched between
(CONTRIBUTING.md, "Testing on real displays").
-->

## Checklist

- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass.
- [ ] `npm run typecheck` passes in `apps/settings` (if the settings UI changed).
- [ ] New UI strings are in `locales/en.json` and `locales/ar.json`.
- [ ] The rules in CONTRIBUTING.md still hold: no keyboard hooks, no network access, and every switch can be undone.
