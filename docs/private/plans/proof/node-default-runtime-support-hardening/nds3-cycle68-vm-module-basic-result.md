# NDS3 cycle 68 - vm module basic

Date: 2026-06-13

## Summary

Promoted `test/parallel/test-vm-module-basic.js` for both node22 and node24.

This burned one required gap in each lane:

- node22: 28 -> 27 gaps, 2339 / 2366, 98.86%
- node24: 35 -> 34 gaps, 2369 / 2403, 98.59%

The Deno fork fix is published as `v2.8.3-nimbus.21`
(`b810e8b6629c2cdf0ed0bb507fc4101d2d9c7dea`).

## Root Cause

`test-vm-module-basic.js` exposed several missing `node:vm` module semantics in
the fork:

- `SourceTextModule.evaluate({ timeout })` validated `breakOnSigint` but ignored
  `timeout`, so `while (true) {}` hung until the outer harness timeout.
- Auto-generated module identifiers used one global counter. Node scopes default
  identifiers per VM context.
- `util.inspect()` exposed private wrapper symbols instead of the public module
  surface `{ status, identifier, context }`.
- Constructing the abstract `Module` base class threw the fork's
  `ERR_INVALID_ARG_TYPE` message instead of Node's plain
  `TypeError: Module is not a constructor`.

## Deno Fork Changes

Edited:

- `/Users/jack/src/github.com/nimbus/deno/ext/node/ops/vm.rs`
- `/Users/jack/src/github.com/nimbus/deno/ext/node/polyfills/vm.js`
- `/Users/jack/src/github.com/nimbus/deno/tests/unit_node/vm_test.ts`

Behavior added:

- plumb `options.timeout` from `Module.evaluate()` into
  `op_vm_module_evaluate`;
- reuse the existing isolate termination pattern used by script execution
  timeout and throw `ERR_SCRIPT_EXECUTION_TIMEOUT`;
- scope generated module identifiers by context with a `SafeWeakMap`;
- add Node-style custom inspect output for module instances;
- match Node's direct `new Module()` TypeError;
- add focused Deno unit coverage for the four parity layers.

Fork checks:

```text
cargo fmt -p deno_node --check
deno fmt --check tests/unit_node/vm_test.ts
git diff --check
```

All passed. `deno fmt --check ext/node/polyfills/vm.js` was intentionally not
used as a gate because that whole file is not Deno-fmt-shaped.

Fork publication:

```text
git commit -m "node(vm): align module evaluate basics"
git tag v2.8.3-nimbus.21
git push origin HEAD
git push origin v2.8.3-nimbus.21
```

Push result:

```text
23941445d0..b810e8b662  HEAD -> nimbus/v2.8.3
* [new tag]               v2.8.3-nimbus.21 -> v2.8.3-nimbus.21
```

## Dynamic Proof

Scratch probe target:

```text
cargo test -p nimbus-runtime --lib nds_probe --no-run
Finished `test` profile [unoptimized + debuginfo] target(s) in 46.81s
```

Baseline / peel diagnostics were retained under:

- `/private/tmp/nimbus-nds-cycle68-vm-module-basic-local-node24`
- `/private/tmp/nimbus-nds-cycle68-vm-module-basic-local2-node24`
- `/private/tmp/nimbus-nds-cycle68-vm-module-basic-local3-node24`
- `/private/tmp/nimbus-nds-cycle68-vm-module-basic-local4-node24`
- `/private/tmp/nimbus-nds-cycle68-vm-module-basic-local-node22`

The failing layers moved from harness timeout, to identifier assertion, to
inspect assertion, to constructor assertion, then passed dynamically.

Local Deno override proof after the final fix:

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle68-vm-module-basic-local4-node24
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-module-basic.js
NIMBUS_RECENSUS_LANE=node24
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 869 filtered out; finished in 2.49s
```

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle68-vm-module-basic-local-node22
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-module-basic.js
NIMBUS_RECENSUS_LANE=node22
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 869 filtered out; finished in 2.39s
```

## Immutable Tag Proof

Nimbus was repinned from `v2.8.3-nimbus.20` to `v2.8.3-nimbus.21`, then
`cargo update -p deno_node` updated `Cargo.lock` to:

```text
git+https://github.com/nimbus/deno?tag=v2.8.3-nimbus.21#b810e8b6629c2cdf0ed0bb507fc4101d2d9c7dea
```

The local `.cargo/config.toml` path override was removed before immutable tag
proof. `cargo clean -p deno_node` removed 542 files / 362.6 MiB.

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle68-vm-module-basic-tag-node24
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-module-basic.js
NIMBUS_RECENSUS_LANE=node24
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 869 filtered out; finished in 2.56s
```

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle68-vm-module-basic-tag-node22
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-module-basic.js
NIMBUS_RECENSUS_LANE=node22
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 869 filtered out; finished in 2.46s
```

## Promotion Proof

Added:

- `crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle68_wave1.rs`
- include in `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`

The scratch `nds_probe.rs` include and file were deleted before commit.

```text
cargo test -p nimbus-runtime --lib node24_default_lane_executes_cycle68_vm_module_basic_batch -- --nocapture

node_compat node24-default-lane-executes-cycle68-vm-module-basic-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 871 filtered out; finished in 2.52s
```

```text
cargo test -p nimbus-runtime --lib node22_supported_lane_executes_cycle68_vm_module_basic_batch -- --nocapture

node_compat node22-supported-lane-executes-cycle68-vm-module-basic-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 871 filtered out; finished in 2.43s
```

## Regression Proof

Because the fork edit touched shared VM module behavior, the existing promoted VM
family guards were rerun:

```text
cargo test -p nimbus-runtime --lib node24_default_lane_executes_loader_context_vm_promoted_batch_fixture -- --nocapture

node_compat node24-default-lane-executes-loader-context-vm-promoted-batch node24 summary: selected=51, passed=51, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 870 filtered out; finished in 103.57s
```

```text
cargo test -p nimbus-runtime --lib node22_supported_lane_executes_loader_context_vm_promoted_batch_fixture -- --nocapture

node_compat node22-supported-lane-executes-loader-context-vm-promoted-batch node22 summary: selected=49, passed=49, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 870 filtered out; finished in 99.77s
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
node22 required gaps: 27
node24 required gaps: 34
```

The classification diff removed `test/parallel/test-vm-module-basic.js` from
both node22 and node24 required-gap catalogs.

## Gate State

The gate remains red, honestly:

- node22: 27 gaps, 98.86%
- node24: 34 gaps, 98.59%

Verifier:

```text
bash scripts/verify-node-default-runtime-support-hardening.sh

Summary: 13 passed, 21 failed
Step 9: FAIL - V8-isolate-required fixtures not proven green
```

Next recommended VM-semantics targets:

- `test/parallel/test-vm-module-referrer-realm.mjs`
- `test/parallel/test-vm-timeout-escape-promise-module.js`
