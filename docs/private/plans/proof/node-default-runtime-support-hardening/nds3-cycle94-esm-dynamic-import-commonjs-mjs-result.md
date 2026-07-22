# NDS3 Cycle 94 - ESM-origin dynamic CommonJS import ordering

Date: 2026-06-14

## Summary

Cycle 94 promotes `test/es-module/test-esm-dynamic-import-commonjs.mjs` in both required lanes.

Fork fix:

- Deno fork: `/Users/jack/src/github.com/nimbus/deno`
- Branch/tag: `nimbus/v2.8.3` / `v2.8.3-nimbus.41`
- Commit: `5c8d394b76 core: defer nextTick for ESM CJS dynamic imports`
- Files changed in the fork tag:
  - `libs/core/01_core.js`
  - `libs/core/modules/map.rs`
  - `libs/core/runtime/bindings.rs`
  - `libs/core/runtime/jsrealm.rs`
  - `libs/core/runtime/jsruntime.rs`

Nimbus is repinned from `v2.8.3-nimbus.40` to `v2.8.3-nimbus.41` in `Cargo.toml` and `Cargo.lock`.
No local Cargo `paths = [...]` override is present in the final Nimbus state.

## Root Cause

Cycle 93 fixed CommonJS-origin dynamic imports by deferring nextTick draining from JS while the traced import promise settled.
The `.mjs` fixture starts from top-level ESM:

```js
let tickDuringCJSImport = false;
process.nextTick(() => { tickDuringCJSImport = true; });
await import(fixtures.fileURL('empty.cjs'));
assert(!tickDuringCJSImport);
```

That path enters deno_core's dynamic-import host callback directly, so no CommonJS JS tracer runs. The fix shares the nextTick deferral counter through the existing `tick_info` buffer and lets deno_core hold the deferral for `.cjs` evaluation imports until after the import promise's microtasks are flushed.

## Dynamic Proof

Local fork proof with temporary Cargo path override to `/Users/jack/src/github.com/nimbus/deno/libs/core`:

node24 local proof:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle94-dynamic-import-commonjs-mjs-local-node24-2 \
  NIMBUS_RECENSUS_FIXTURE=test/es-module/test-esm-dynamic-import-commonjs.mjs \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS=test/common:test/fixtures:test/fixtures/es-modules \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture 2>&1 \
  | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|AssertionError|tickDuringCJSImport|actual|expected|test-esm-dynamic-import-commonjs'
```

Result:

```text
node_compat nds-probe node24 -> test/es-module/test-esm-dynamic-import-commonjs.mjs
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 908 filtered out; finished in 3.06s
```

node22 local proof:

```text
node_compat nds-probe node22 -> test/es-module/test-esm-dynamic-import-commonjs.mjs
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 908 filtered out; finished in 2.87s
```

Fork hygiene before publishing:

```bash
git diff --check
cargo fmt --all --check
```

Both commands passed.

Immutable tag proof after repinning Nimbus to `v2.8.3-nimbus.41`:

node24 tag proof:

```text
node_compat nds-probe node24 -> test/es-module/test-esm-dynamic-import-commonjs.mjs
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 908 filtered out; finished in 5.11s
```

node22 tag proof:

```text
node_compat nds-probe node22 -> test/es-module/test-esm-dynamic-import-commonjs.mjs
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 908 filtered out; finished in 2.89s
```

## Promotion Guards

Permanent promotion file:

- `crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle94_esm_dynamic_import_commonjs_mjs.rs`

node22 promotion guard:

```text
node_compat node22-default-lane-executes-cycle94-esm-dynamic-import-commonjs-mjs node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 909 filtered out; finished in 2.90s
```

node24 promotion guard:

```text
node_compat node24-default-lane-executes-cycle94-esm-dynamic-import-commonjs-mjs node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 909 filtered out; finished in 2.83s
```

The scratch `nds_probe.rs` include/file was removed before checkpointing.

## Regeneration

Commands:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null
done
```

Generated posture after regeneration:

```text
node22 = 4
node24 = 5
```

Remaining generated gap list:

```text
node22:
  test/es-module/test-esm-dynamic-import.js
  test/es-module/test-esm-loader-mock.mjs
  test/es-module/test-esm-virtual-json.mjs
  test/parallel/test-vm-module-import-meta.js

node24:
  test/es-module/test-esm-dynamic-import.js
  test/es-module/test-esm-loader-mock.mjs
  test/es-module/test-esm-virtual-json.mjs
  test/parallel/test-vm-module-hastoplevelawait.js
  test/parallel/test-vm-module-import-meta.js
```
