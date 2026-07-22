# NDS3 cycle 70 - vm module referrer realm

Date: 2026-06-14

## Summary

Promoted `test/parallel/test-vm-module-referrer-realm.mjs` for both node22 and
node24.

This burned one required gap in each lane:

- node22: 26 -> 25 gaps, 2341 / 2366, 98.94%
- node24: 33 -> 32 gaps, 2371 / 2403, 98.67%

The Deno fork fix is tag `v2.8.3-nimbus.22`
(`7c0004b48eee868969174ff52262bbc2f7e0e7a1`). It records a context-level
`node:vm` dynamic-import marker when `createContext()` receives
`importModuleDynamically`, and `deno_core` uses that marker only when V8 reports
an indirect dynamic import with empty host-defined options and an undefined
referrer. This matches Node's no-JS-stack fallback without widening the sandbox
or changing normal module loading.

## Fork Proof

Deno fork files changed:

- `ext/node/polyfills/vm.js`
- `ext/node/ops/vm.rs`
- `libs/core/runtime/host_defined_options.rs`
- `libs/core/runtime/bindings.rs`
- `libs/core/lib.rs`
- `tests/unit_node/vm_test.ts`

Fork checks:

```text
cargo fmt -p deno_core -p deno_node --check
deno fmt --check tests/unit_node/vm_test.ts
git diff --check
env CARGO_ENCODED_RUSTFLAGS= cargo check -p deno_core -p deno_node

Result: all passed. cargo check finished dev profile; only Deno's existing
bench-profile warning was emitted.
```

The system `deno` binary was not used for behavior proof because it is 2.7.12
and no local fork `target/{debug,release}/deno` binary existed. The behavioral
proof below is the Nimbus fixture harness against the local fork and then the
published immutable tag.

## Dynamic Proof

Baseline failure on the previous published tag:

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle70-vm-module-referrer-realm-tag-node24-rerun3
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-module-referrer-realm.mjs
NIMBUS_RECENSUS_LANE=node24
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
runtime JavaScript error: Error: invalid runtime referrer `undefined`
test result: FAILED. 0 passed; 1 failed; 0 ignored; 873 filtered out; finished in 2.00s
```

Local fork override proofs:

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle70-vm-module-referrer-realm-local-node24-a
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-module-referrer-realm.mjs
NIMBUS_RECENSUS_LANE=node24
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 873 filtered out; finished in 2.03s
```

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle70-vm-module-referrer-realm-local-node22-a
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-module-referrer-realm.mjs
NIMBUS_RECENSUS_LANE=node22
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 873 filtered out; finished in 1.93s
```

Published tag proofs after removing the local path override and repinning
`Cargo.toml` / `Cargo.lock` to `v2.8.3-nimbus.22`:

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle70-vm-module-referrer-realm-tag-node24-a
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-module-referrer-realm.mjs
NIMBUS_RECENSUS_LANE=node24
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 873 filtered out; finished in 2.05s
```

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle70-vm-module-referrer-realm-tag-node22-a
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-module-referrer-realm.mjs
NIMBUS_RECENSUS_LANE=node22
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 873 filtered out; finished in 1.97s
```

## Promotion Proof

Added:

- `crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle70_wave1.rs`
- include in `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`

The scratch `nds_probe.rs` include and file were deleted before commit.

```text
cargo test -p nimbus-runtime --lib node24_default_lane_executes_cycle70_vm_module_referrer_realm_batch -- --nocapture

node_compat node24-default-lane-executes-cycle70-vm-module-referrer-realm-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 874 filtered out; finished in 2.03s
```

```text
cargo test -p nimbus-runtime --lib node22_supported_lane_executes_cycle70_vm_module_referrer_realm_batch -- --nocapture

node_compat node22-supported-lane-executes-cycle70-vm-module-referrer-realm-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 874 filtered out; finished in 1.96s
```

Regression guards for the VM loader/context surface:

```text
cargo test -p nimbus-runtime --lib node24_default_lane_executes_loader_context_vm_promoted_batch_fixture -- --nocapture

node_compat node24-default-lane-executes-loader-context-vm-promoted-batch node24 summary: selected=51, passed=51, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 874 filtered out; finished in 105.04s
```

```text
cargo test -p nimbus-runtime --lib node22_supported_lane_executes_loader_context_vm_promoted_batch_fixture -- --nocapture

node_compat node22-supported-lane-executes-loader-context-vm-promoted-batch node22 summary: selected=49, passed=49, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 874 filtered out; finished in 100.43s
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

`docs/private/architecture/runtime/node-default-support-posture.json` now reports:

```text
node22 25 gaps, 98.94%, 2341 / 2366
node24 32 gaps, 98.67%, 2371 / 2403
```

The classification diff removed
`test/parallel/test-vm-module-referrer-realm.mjs` from both node22 and node24
required-gap catalogs.

## Gate State

The gate remains red, honestly:

- node22: 25 gaps, 98.94%
- node24: 32 gaps, 98.67%

Verifier:

```text
bash scripts/verify-node-default-runtime-support-hardening.sh

Summary: 13 passed, 21 failed
Step 9: FAIL - V8-isolate-required fixtures not proven green
```

Remaining high-yield clusters are crypto-provider, ESM loader, promise-hooks, and
hang-timeout/event-loop. The previous vm-semantics residual is now closed.
