# NDS3 cycle 71 - WebCrypto promise prototype pollution

Date: 2026-06-14

## Summary

Promoted `test/parallel/test-webcrypto-promise-prototype-pollution.mjs` for
node24.

This burned one required gap:

- node22: 25 gaps, 2341 / 2366, 98.94% (unchanged)
- node24: 32 -> 31 gaps, 2372 / 2403, 98.71%

The Deno fork fix is tag `v2.8.3-nimbus.23`
(`08e20a0bdc7388482a1ecb5f75d4ec3c9ebdc751`). It keeps WebCrypto AES
`generateKey()` on the native op await path instead of adopting an intermediate
JS async helper promise after user code mutates `Promise.prototype.then`.

## Fork Proof

Deno fork file changed:

- `ext/crypto/00_crypto.js`

Fork checks:

```text
git diff --check

Result: passed.
```

```text
env CARGO_ENCODED_RUSTFLAGS= cargo check -p deno_crypto

Result: passed. cargo check finished dev profile; only Deno's existing
bench-profile warning was emitted.
```

`deno fmt --check ext/crypto/00_crypto.js` was intentionally not used as a
gate: the file is wrapped in an extension IIFE and Deno fmt wants to reindent
the entire file. The whitespace guard was `git diff --check`.

## Dynamic Proof

Baseline failure on the previous published tag:

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle71-webcrypto-promise-pollution-node24-a
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-webcrypto-promise-prototype-pollution.mjs
NIMBUS_RECENSUS_LANE=node24
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
runtime JavaScript error: AssertionError [ERR_ASSERTION]: Promise.prototype.then
test result: FAILED. 0 passed; 1 failed; 0 ignored; 875 filtered out; finished in 2.13s
```

Local fork proof:

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle71-webcrypto-promise-pollution-local-node24-d
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-webcrypto-promise-prototype-pollution.mjs
NIMBUS_RECENSUS_LANE=node24
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 875 filtered out; finished in 2.09s
```

Published tag proof after removing the local path override and repinning
`Cargo.toml` / `Cargo.lock` to `v2.8.3-nimbus.23`:

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle71-webcrypto-promise-pollution-tag-node24-a
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-webcrypto-promise-prototype-pollution.mjs
NIMBUS_RECENSUS_LANE=node24
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 875 filtered out; finished in 2.24s
```

Adjacent same-cluster probe that remained red and was not promoted:

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle71-webcrypto-deduplicate-usages-local-node24-a
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-webcrypto-deduplicate-usages.js
NIMBUS_RECENSUS_LANE=node24
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
runtime JavaScript error: NotSupportedError: Unrecognized algorithm name
```

That probe peeled past duplicate-usage ordering and reached the fixture's KMAC
branch. No classification or promotion was made for that fixture.

## Promotion Proof

Added:

- `crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle71_wave1.rs`
- include in `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`

The scratch `nds_probe.rs` include and file were deleted before commit.

```text
cargo test -p nimbus-runtime --lib node24_default_lane_executes_cycle71_webcrypto_promise_prototype_pollution_batch -- --nocapture

node_compat node24-default-lane-executes-cycle71-webcrypto-promise-prototype-pollution-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 876 filtered out; finished in 2.08s
```

## Regeneration

Ran:

```text
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
/opt/homebrew/bin/python3.12 scripts/runtime/node/status.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/dashboard.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/trends.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/publish_evidence.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py
```

`docs/private/architecture/runtime/node-default-support-posture.json` now
reports:

```text
node22 25 gaps, 98.94%, 2341 / 2366
node24 31 gaps, 98.71%, 2372 / 2403
```

The classification diff removed
`test/parallel/test-webcrypto-promise-prototype-pollution.mjs` from the node24
required-gap catalog.

## Gate State

The gate remains red, honestly:

- node22: 25 gaps, 98.94%
- node24: 31 gaps, 98.71%

Remaining high-yield clusters are crypto-provider, ESM loader, promise-hooks,
and hang-timeout/event-loop.
