# NDS3 cycle 72 - ESM CJS named export errors

Date: 2026-06-14

## Summary

Promoted `test/es-module/test-esm-cjs-named-error.mjs` for node22 and node24.

This burned two required gaps:

- node22: 25 -> 24 gaps, 2342 / 2366, 98.99%
- node24: 31 -> 30 gaps, 2373 / 2403, 98.75%

The Deno fork fix is tag `v2.8.3-nimbus.24`
(`28a7f584b3cb3280199700c651904c160b498a42`). It aligns CommonJS named
export analysis and generated CJS-wrapper missing-export diagnostics with
Node's ESM loader behavior for direct `module.exports = { ... }` object
assignments.

## Fork Proof

Deno fork files changed:

- `libs/resolver/cjs/analyzer/deno_ast.rs`
- `libs/core/modules/map.rs`
- `libs/core/modules/mod.rs`
- `libs/core/modules/module_map_data.rs`

Fork checks:

```text
cargo fmt

Result: passed.
```

```text
env CARGO_ENCODED_RUSTFLAGS= cargo check -p deno_core -p deno_resolver

Result: passed. cargo check finished dev profile.
```

```text
git diff --check HEAD~1..HEAD

Result: passed.
```

The fork commit and tag were pushed:

```text
28a7f584b3 (HEAD -> nimbus/v2.8.3, tag: v2.8.3-nimbus.24, origin/nimbus/v2.8.3) node(esm): align CJS named export errors
```

## Dynamic Proof

Local fork proof before publishing `v2.8.3-nimbus.24`:

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle72-esm-cjs-named-local-node24-clean
NIMBUS_RECENSUS_FIXTURE=test/es-module/test-esm-cjs-named-error.mjs
NIMBUS_RECENSUS_LANE=node24
NIMBUS_RECENSUS_EXTRA_DIRS=test/common:test/fixtures/es-modules
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 876 filtered out; finished in 2.11s
```

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle72-esm-cjs-named-local-node22-clean
NIMBUS_RECENSUS_FIXTURE=test/es-module/test-esm-cjs-named-error.mjs
NIMBUS_RECENSUS_LANE=node22
NIMBUS_RECENSUS_EXTRA_DIRS=test/common:test/fixtures/es-modules
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 876 filtered out; finished in 2.04s
```

Published tag proof after removing the local path override and repinning
`Cargo.toml` / `Cargo.lock` to `v2.8.3-nimbus.24`:

```text
cargo test -p nimbus-runtime --lib nds_probe_node24 -- --ignored --nocapture

node_compat nds-probe-node24 node24 summary: selected=1, passed=1, skipped=0, failed=0
node_compat nds-probe-node24 node24 summary artifact: target/node-compat/diagnostics/batch/node24__nds_probe_node24__summary.json
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 877 filtered out; finished in 2.11s
```

```text
cargo test -p nimbus-runtime --lib nds_probe_node22 -- --ignored --nocapture

node_compat nds-probe-node22 node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat nds-probe-node22 node22 summary artifact: target/node-compat/diagnostics/batch/node22__nds_probe_node22__summary.json
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 877 filtered out; finished in 2.03s
```

The published-tag proof used hardcoded temporary probe tests because the
managed sandbox denied Cargo target-lock access when the command was wrapped
with `env ... cargo test`. The scratch probe file and include were deleted
before promotion.

## Promotion Proof

Added:

- `crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle72_esm_cjs_named_error.rs`
- include in `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`

The scratch `nds_probe.rs` include and file were deleted before commit.

```text
cargo test -p nimbus-runtime --lib cycle72_esm_cjs_named_error -- --nocapture

node_compat node22-supported-lane-executes-cycle72-esm-cjs-named-error-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle72-esm-cjs-named-error-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 876 filtered out; finished in 4.08s
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

The managed sandbox denied generated-file writes for these scripts, so they
were rerun with tool escalation through the same checked-in generators. No
generated JSON was hand-edited.

`docs/private/architecture/runtime/node-default-support-posture.json` now
reports:

```text
node22 24 gaps, 98.99%, 2342 / 2366
node24 30 gaps, 98.75%, 2373 / 2403
```

The classification diff removed
`test/es-module/test-esm-cjs-named-error.mjs` from the node22 and node24
required-gap catalogs.

## Gate State

The gate remains red, honestly:

- node22: 24 gaps, 98.99%
- node24: 30 gaps, 98.75%

`bash scripts/verify-node-default-runtime-support-hardening.sh` was rerun after
regeneration. It reported 13 passed / 21 failed in this local tree; step 9
remains red because gaps are not yet zero, and several private proof-doc rows
are absent/ignored in this checkout.

Remaining high-yield clusters include ESM/module loader residuals, async
lifecycle, crypto/provider coverage, networking, WebStreams/encoding, and the
remaining fs/stream surface.
