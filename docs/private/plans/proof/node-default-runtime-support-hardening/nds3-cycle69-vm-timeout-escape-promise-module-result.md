# NDS3 cycle 69 - vm timeout escape promise module

Date: 2026-06-13

## Summary

Promoted `test/parallel/test-vm-timeout-escape-promise-module.js` for both
node22 and node24.

This burned one required gap in each lane:

- node22: 27 -> 26 gaps, 2340 / 2366, 98.90%
- node24: 34 -> 33 gaps, 2370 / 2403, 98.63%

No new fork tag was needed. The fixture was unlocked by the cycle68 Deno fork
tag `v2.8.3-nimbus.21`, which added `SourceTextModule.evaluate({ timeout })`
parity.

## Dynamic Proof

Scratch probe diagnostics were retained under:

- `/private/tmp/nimbus-nds-cycle69-vm-timeout-escape-promise-module-tag-node24`
- `/private/tmp/nimbus-nds-cycle69-vm-timeout-escape-promise-module-tag-node22`

Node24 probe on the published tag:

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle69-vm-timeout-escape-promise-module-tag-node24
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-timeout-escape-promise-module.js
NIMBUS_RECENSUS_LANE=node24
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 871 filtered out; finished in 1.98s
```

Node22 probe on the published tag:

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle69-vm-timeout-escape-promise-module-tag-node22
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-timeout-escape-promise-module.js
NIMBUS_RECENSUS_LANE=node22
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 871 filtered out; finished in 1.90s
```

## Promotion Proof

Added:

- `crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle69_wave1.rs`
- include in `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`

The scratch `nds_probe.rs` include and file were deleted before commit.

```text
cargo test -p nimbus-runtime --lib node24_default_lane_executes_cycle69_vm_timeout_escape_promise_module_batch -- --nocapture

node_compat node24-default-lane-executes-cycle69-vm-timeout-escape-promise-module-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 873 filtered out; finished in 1.97s
```

```text
cargo test -p nimbus-runtime --lib node22_supported_lane_executes_cycle69_vm_timeout_escape_promise_module_batch -- --nocapture

node_compat node22-supported-lane-executes-cycle69-vm-timeout-escape-promise-module-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 873 filtered out; finished in 1.89s
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

`required_surface_blockers.py` reported:

```text
node22 required gaps: 26
node24 required gaps: 33
```

The classification diff removed
`test/parallel/test-vm-timeout-escape-promise-module.js` from both node22 and
node24 required-gap catalogs.

## Gate State

The gate remains red, honestly:

- node22: 26 gaps, 98.90%
- node24: 33 gaps, 98.63%

Verifier:

```text
bash scripts/verify-node-default-runtime-support-hardening.sh

Summary: 13 passed, 21 failed
Step 9: FAIL - V8-isolate-required fixtures not proven green
```

Next recommended VM-semantics target:

- `test/parallel/test-vm-module-referrer-realm.mjs`
