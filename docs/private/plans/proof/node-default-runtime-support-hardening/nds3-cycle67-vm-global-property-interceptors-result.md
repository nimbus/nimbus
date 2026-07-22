# NDS3 Cycle 67 - VM Global Property Interceptors

Date: 2026-06-13  
Branch: `codex/node-default-runtime-support-hardening`  
PR: #10 (draft)  
Deno fork tag: `v2.8.3-nimbus.20` (`23941445d0aad713c44497e9bd5c79acda5844dc`)  
rusty_v8: stock `v149.4.0-nimbus.1`

## Fixture

- `test/parallel/test-vm-global-property-interceptors.js` (node22 + node24)

## Baseline Failure

Published tag `v2.8.3-nimbus.19` failed node24 with:

```text
node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
AssertionError [ERR_ASSERTION]: Missing expected exception.
at ...test-vm-global-property-interceptors.js:126:8
diagnostic artifact: /private/tmp/nimbus-nds-cycle67-vm-global-property-interceptors-node24/vm/node24__test_parallel_test_vm_global_property_interceptors_js.json
```

The failing source window:

```text
Object.defineProperty(sandbox, { f: {} });
assert.throws(() => vm.runInContext(`
'use strict';
Object.defineProperty(this, 'f', { value: 'newF' });
`, ctx), /TypeError: Cannot redefine property: f/);
```

## Root Cause And Fork Fix

`ext/node/ops/vm.rs` `property_definer` forwarded contextified-global
`Object.defineProperty(...)` calls to the sandbox object, but ignored the boolean
result from `sandbox.define_property(...)`. Redefining a non-configurable sandbox
property therefore failed internally but was reported to V8 as successfully
intercepted, so no TypeError reached the fixture.

Fix in `nimbus/deno`:

- Check the `sandbox.define_property(...)` result in the VM definer trap.
- Throw `TypeError: Cannot redefine property: <name>` when the sandbox rejects
  the definition.
- Add a Deno unit regression for a non-configurable sandbox property.

Fork checks:

```text
git diff --check
cargo fmt -p deno_node --check
deno fmt --check tests/unit_node/vm_test.ts
```

Result: all passed. The fork was committed, tagged, and pushed as
`v2.8.3-nimbus.20`.

## Dynamic Proof

Local fork override proof:

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle67-vm-global-property-interceptors-local-node24
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-global-property-interceptors.js
NIMBUS_RECENSUS_LANE=node24
NIMBUS_RECENSUS_EXTRA_DIRS=test/common
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 867 filtered out; finished in 1.97s
```

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle67-vm-global-property-interceptors-local-node22
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-global-property-interceptors.js
NIMBUS_RECENSUS_LANE=node22
NIMBUS_RECENSUS_EXTRA_DIRS=test/common
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 867 filtered out; finished in 1.89s
```

Immutable tag proof after repinning Nimbus to `v2.8.3-nimbus.20`:

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle67-vm-global-property-interceptors-tag-node24
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-global-property-interceptors.js
NIMBUS_RECENSUS_LANE=node24
NIMBUS_RECENSUS_EXTRA_DIRS=test/common
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 867 filtered out; finished in 2.02s
```

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle67-vm-global-property-interceptors-tag-node22
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-global-property-interceptors.js
NIMBUS_RECENSUS_LANE=node22
NIMBUS_RECENSUS_EXTRA_DIRS=test/common
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 867 filtered out; finished in 2.02s
```

Promotion guards:

```text
cargo test -p nimbus-runtime --lib node24_default_lane_executes_cycle67_vm_global_property_interceptors_batch -- --nocapture
```

```text
summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 868 filtered out; finished in 2.02s
```

```text
cargo test -p nimbus-runtime --lib node22_supported_lane_executes_cycle67_vm_global_property_interceptors_batch -- --nocapture
```

```text
summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 868 filtered out; finished in 2.02s
```

Existing VM regression guards:

```text
cargo test -p nimbus-runtime --lib node24_default_lane_executes_loader_context_vm_promoted_batch_fixture -- --nocapture
```

```text
summary: selected=51, passed=51, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 868 filtered out; finished in 103.55s
```

```text
cargo test -p nimbus-runtime --lib node22_supported_lane_executes_loader_context_vm_promoted_batch_fixture -- --nocapture
```

```text
summary: selected=49, passed=49, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 868 filtered out; finished in 98.83s
```

## Generated Counts

Regeneration commands:

```text
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
/opt/homebrew/bin/python3.12 scripts/runtime/node/status.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/dashboard.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/trends.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/publish_evidence.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py
```

Generated posture:

```text
node22 {'gaps': 28, 'pass_rate_percent': 98.82, 'passed': 2338, 'total': 2366}
node24 {'gaps': 35, 'pass_rate_percent': 98.54, 'passed': 2368, 'total': 2403}
unique required fixtures: 37
```

The gate remains red and honest.

Verifier:

```text
bash scripts/verify-node-default-runtime-support-hardening.sh
Summary: 13 passed, 21 failed
Step 9: FAIL - V8-isolate-required fixtures not proven green
```
