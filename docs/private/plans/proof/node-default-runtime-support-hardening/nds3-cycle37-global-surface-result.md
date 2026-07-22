# NDS3 cycle 37 result - Node global enumerable surface

Date: 2026-06-13

## Scope

Fixed and promoted the required fixture on both default lanes:

- `test/parallel/test-global.js` (node22, node24)

Fork state:

- No Deno fork change.
- No rusty_v8 change.
- Nimbus remains pinned to `nimbus/deno` `v2.8.3-nimbus.5` and `nimbus/rusty_v8`
  `v149.4.0-nimbus.1`.

Nimbus changes:

- The Node bootstrap now seeds `atob` and `btoa` from Deno's web base64 helpers
  and normalizes Node's expected enumerable global surface after bootstrap.
- Nimbus host/runtime helper functions are lexical bindings instead of
  enumerable global function declarations.
- Harness-only globals (`__nimbusNodeCompatLane`,
  `__nimbusInternalTestBindingState`, and `gc`) stay available but are
  non-enumerable.
- A non-ignored cycle-37 guard promotes `test-global.js` for node22 and node24.

## Root Cause

`test-global.js` asserts Node's exact enumerable `globalThis` key set, then
checks sloppy-global linkage and the `[object global]` tag. Nimbus exposed
internal bootstrap/test helpers as enumerable globals and did not expose
`atob`/`btoa` as Node-visible enumerable globals.

Pre-fix focused censuses:

```bash
gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE=test/parallel/test-global.js \
  NIMBUS_RECENSUS_LANE=node24 NIMBUS_RECENSUS_EXTRA_DIRS=test/common \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
runtime JavaScript error: AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal
+ Set(30) { '__nimbusAsyncHostValue', ..., 'gc', 'onunhandledrejection', 'reportError' }
- Set(15) { 'atob', 'btoa', ..., 'structuredClone' }
```

```bash
gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE=test/parallel/test-global.js \
  NIMBUS_RECENSUS_LANE=node22 NIMBUS_RECENSUS_EXTRA_DIRS=test/common \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node22 summary: selected=1, passed=0, skipped=0, failed=1
runtime JavaScript error: AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal
```

The first post-fix run reached the fixture helper import and failed only because
the scratch census omitted `test/fixtures/global`; the promotion includes that
official fixture helper directory.

## Proof

Focused post-fix censuses before promotion:

```bash
gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE=test/parallel/test-global.js \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS=test/common:test/fixtures/global \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 825 filtered out; finished in 1.91s
```

```bash
gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE=test/parallel/test-global.js \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS=test/common:test/fixtures/global \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 825 filtered out; finished in 1.91s
```

Promoted non-ignored guards:

```bash
gtimeout -s KILL 120 cargo test -p nimbus-runtime --lib \
  cycle37_global_surface -- --nocapture
```

```text
node_compat node22-supported-lane-executes-cycle37-global-surface-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle37-global-surface-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 825 filtered out; finished in 3.92s
```

Regression guards for touched global/GC surfaces:

```bash
gtimeout -s KILL 120 cargo test -p nimbus-runtime --lib cycle12d -- --nocapture
```

```text
node_compat node24-default-lane-executes-cycle12d-promoted-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 826 filtered out; finished in 1.91s
```

```bash
gtimeout -s KILL 180 cargo test -p nimbus-runtime --lib \
  parallel_js_platform_promoted_batch_fixture -- --nocapture
```

```text
node_compat node24-default-lane-executes-parallel-js-platform-promoted-batch node24 summary: selected=33, passed=33, skipped=0, failed=0
node_compat node22-supported-lane-executes-parallel-js-platform-promoted-batch node22 summary: selected=34, passed=34, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 825 filtered out; finished in 150.37s
```

Formatting:

```bash
cargo fmt --all --check
```

```text
passed with no diff
```

Generated pipeline:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do \
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null; \
done
```

Generated checks:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all --check
/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py validate
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py --check
/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py --check
```

```text
classifications node20/node22/node24/node26 are up to date
validated node-compat watchpoint catalog: 134 entries
node default support posture: pass
node required-surface blocker inventory: pass
```

Generated posture after classification sync and evidence regeneration:

```text
node22 required gaps: 62
node22 required pass rate: 97.39
node24 required gaps: 72
node24 required pass rate: 97.01
unique required fixtures remaining: 74
```

## Guardrails

- No V8 or rusty_v8 source changes.
- No Deno fork changes or tags.
- No official fixture or checker edits.
- No hand-edited false-green JSON.
- Temporary scratch `nds_probe` include/file was removed before promotion.
- The fixture was removed from the node22/node24 required blocker inventory only
  by `classifications.py sync` after non-ignored green guards landed.
- PR #10 remains draft; the gate is still red and honest.
