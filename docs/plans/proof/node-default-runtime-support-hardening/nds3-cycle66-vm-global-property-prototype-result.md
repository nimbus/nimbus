# NDS3 Cycle 66 - VM Global Property Prototype

Date: 2026-06-13  
Branch: `codex/node-default-runtime-support-hardening`  
PR: #10 (draft)  
Deno fork tag: `v2.8.3-nimbus.19` (`fde88bd9879405c2c7ebf66d2c5b101d6b6b77ff`)  
rusty_v8: stock `v149.4.0-nimbus.1`

## Fixture

- `test/parallel/test-vm-global-property-prototype.js` (node22 + node24)

## Baseline Failure

Published tag `v2.8.3-nimbus.18` failed node24 with:

```text
node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
AssertionError [ERR_ASSERTION]: Expected values to be loosely deep-equal:
diagnostic artifact: /private/tmp/nimbus-nds-cycle66-vm-global-property-prototype-node24/vm/node24__test_parallel_test_vm_global_property_prototype_js.json
```

Focused instrumentation of a synthetic copy of the fixture (the official fixture was not edited) showed that outer sandbox prototype properties were incorrectly reported by query traps:

```text
Object.hasOwn(this, "onOuterProto") -> true
"onOuterProto" in this -> true
```

Node expects those query paths to exclude the outer sandbox prototype chain, while ordinary reads still resolve through the sandbox getter trap.

## Root Cause And Fork Fix

In `ext/node/ops/vm.rs`, `property_query` used `sandbox.has(scope, property_value)`, which walks the sandbox's outer prototype chain. V8 query traps back both `Object.hasOwn()` and `in`, so the inner global incorrectly treated outer sandbox prototype properties as present.

Fix in `nimbus/deno`:

- Use `sandbox.has_own_property(scope, property)` in the query trap.
- Use `sandbox.get_real_named_property_attributes(scope, property)` for attribute lookup.
- Keep read resolution unchanged so outer prototype values remain readable through the getter path.
- Update the Deno unit test to assert that `"addEventListener" in window` is false for outer prototype properties while direct reads still work.

Fork checks:

```text
git diff --check
cargo fmt -p deno_node --check
deno fmt --check tests/unit_node/vm_test.ts
```

Result: all passed. The fork was committed, tagged, and pushed as `v2.8.3-nimbus.19`.

## Dynamic Proof

Local fork override proof:

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle66-vm-global-property-prototype-local-clean-node24
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-global-property-prototype.js
NIMBUS_RECENSUS_LANE=node24
NIMBUS_RECENSUS_EXTRA_DIRS=test/common
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

Result:

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 865 filtered out; finished in 1.99s
```

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle66-vm-global-property-prototype-local-node22
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-global-property-prototype.js
NIMBUS_RECENSUS_LANE=node22
NIMBUS_RECENSUS_EXTRA_DIRS=test/common
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

Result:

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 865 filtered out; finished in 1.91s
```

Immutable tag proof after repinning Nimbus to `v2.8.3-nimbus.19`:

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle66-vm-global-property-prototype-tag-node24
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-global-property-prototype.js
NIMBUS_RECENSUS_LANE=node24
NIMBUS_RECENSUS_EXTRA_DIRS=test/common
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

Result:

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 865 filtered out; finished in 2.02s
```

```text
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle66-vm-global-property-prototype-tag-node22
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-vm-global-property-prototype.js
NIMBUS_RECENSUS_LANE=node22
NIMBUS_RECENSUS_EXTRA_DIRS=test/common
cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

Result:

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 865 filtered out; finished in 2.02s
```

Promotion guards:

```text
cargo test -p nimbus-runtime --lib node24_default_lane_executes_cycle66_vm_global_property_prototype_batch -- --nocapture
```

```text
summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 866 filtered out; finished in 2.04s
```

```text
cargo test -p nimbus-runtime --lib node22_supported_lane_executes_cycle66_vm_global_property_prototype_batch -- --nocapture
```

```text
summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 866 filtered out; finished in 2.04s
```

Existing VM regression guards:

```text
cargo test -p nimbus-runtime --lib node24_default_lane_executes_loader_context_vm_promoted_batch_fixture -- --nocapture
```

```text
summary: selected=51, passed=51, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 866 filtered out; finished in 103.48s
```

```text
cargo test -p nimbus-runtime --lib node22_supported_lane_executes_loader_context_vm_promoted_batch_fixture -- --nocapture
```

```text
summary: selected=49, passed=49, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 866 filtered out; finished in 98.95s
```

## Non-Promoted Adjacent Fixture

`test/parallel/test-vm-global-property-interceptors.js` was checked under the local fork patch and still failed on a different assertion:

```text
node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
Missing expected exception
```

It was not promoted.

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
node22 {'gaps': 29, 'pass_rate_percent': 98.77, 'passed': 2337, 'total': 2366}
node24 {'gaps': 36, 'pass_rate_percent': 98.5, 'passed': 2367, 'total': 2403}
```

The gate remains red and honest.

Verifier:

```text
bash scripts/verify-node-default-runtime-support-hardening.sh
Summary: 13 passed, 21 failed
Step 9: FAIL - V8-isolate-required fixtures not proven green
```
