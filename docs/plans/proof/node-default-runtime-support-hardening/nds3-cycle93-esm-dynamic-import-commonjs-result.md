# NDS3 Cycle 93 - ESM dynamic CommonJS import ordering

Date: 2026-06-14

## Summary

Cycle 93 promotes `test/es-module/test-esm-dynamic-import-commonjs.js` in both required lanes.

The fork fix was already published before this Nimbus checkpoint was finalized:

- Deno fork: `/Users/jack/src/github.com/nimbus/deno`
- Branch/tag: `nimbus/v2.8.3` / `v2.8.3-nimbus.40`
- Commit: `41f65d2c7f node: defer nextTick during traced CJS dynamic imports`
- Files changed in the fork tag: `ext/node/polyfills/01_require.js`, `libs/core/01_core.js`

Nimbus is repinned from `v2.8.3-nimbus.39` to `v2.8.3-nimbus.40` in `Cargo.toml` and `Cargo.lock`.
No local Cargo `paths = [...]` override is present in the final Nimbus state.

## Dynamic Proof

The focused scratch probe was run after repinning to the immutable tag, not against a local Deno path.

Compile proof:

```bash
cargo test -p nimbus-runtime --lib nds_probe --no-run 2>&1 \
  | grep -iE 'error\[|Finished|Compiling deno_(core|node)|Compiling nimbus-runtime'
```

Result:

```text
Compiling deno_node v0.189.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.40#41f65d2c)
Compiling deno_core v0.404.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.40#41f65d2c)
Compiling deno_node_crypto v0.21.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.40#41f65d2c)
Compiling deno_node_sqlite v0.21.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.40#41f65d2c)
Compiling nimbus-runtime v0.1.33 (/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/crates/nimbus-runtime)
Finished `test` profile [unoptimized + debuginfo] target(s) in 46.44s
```

node24 immutable-tag probe:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle93-dynamic-import-commonjs-tag40-node24-1 \
  NIMBUS_RECENSUS_FIXTURE=test/es-module/test-esm-dynamic-import-commonjs.js \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS=test/common:test/fixtures:test/fixtures/es-modules \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture 2>&1 \
  | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|AssertionError|tickDuringCJSImport'
```

Result:

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 906 filtered out; finished in 3.10s
```

node22 immutable-tag probe:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle93-dynamic-import-commonjs-tag40-node22-1 \
  NIMBUS_RECENSUS_FIXTURE=test/es-module/test-esm-dynamic-import-commonjs.js \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS=test/common:test/fixtures:test/fixtures/es-modules \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture 2>&1 \
  | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|AssertionError|tickDuringCJSImport'
```

Result:

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 906 filtered out; finished in 2.86s
```

## Promotion Guards

Permanent promotion file:

- `crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle93_esm_dynamic_import_commonjs.rs`

node22 promotion guard:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle93-dynamic-import-commonjs-promotion-node22-1 \
  cargo test -p nimbus-runtime --lib node22_default_lane_executes_cycle93_esm_dynamic_import_commonjs -- --nocapture 2>&1 \
  | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|AssertionError|tickDuringCJSImport'
```

Result:

```text
node_compat node22-default-lane-executes-cycle93-esm-dynamic-import-commonjs node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 907 filtered out; finished in 2.88s
```

node24 promotion guard:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle93-dynamic-import-commonjs-promotion-node24-1 \
  cargo test -p nimbus-runtime --lib node24_default_lane_executes_cycle93_esm_dynamic_import_commonjs -- --nocapture 2>&1 \
  | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|AssertionError|tickDuringCJSImport'
```

Result:

```text
node_compat node24-default-lane-executes-cycle93-esm-dynamic-import-commonjs node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 907 filtered out; finished in 2.85s
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
node22 = 5
node24 = 6
```

Remaining generated gap list:

```text
node22:
  test/es-module/test-esm-dynamic-import-commonjs.mjs
  test/es-module/test-esm-dynamic-import.js
  test/es-module/test-esm-loader-mock.mjs
  test/es-module/test-esm-virtual-json.mjs
  test/parallel/test-vm-module-import-meta.js

node24:
  test/es-module/test-esm-dynamic-import-commonjs.mjs
  test/es-module/test-esm-dynamic-import.js
  test/es-module/test-esm-loader-mock.mjs
  test/es-module/test-esm-virtual-json.mjs
  test/parallel/test-vm-module-hastoplevelawait.js
  test/parallel/test-vm-module-import-meta.js
```

The companion `test/es-module/test-esm-dynamic-import-commonjs.mjs` was not promoted; prior focused probes kept it red, so this cycle only claims the dynamically proven `.js` fixture.
