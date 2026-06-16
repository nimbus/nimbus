# NDS3 cycle 36 result - CommonJS main uncaughtException stack

Date: 2026-06-13

## Scope

Fixed and promoted the required fixture on both default lanes:

- `test/parallel/test-events-uncaught-exception-stack.js` (node22, node24)

Fork state:

- No Deno fork change.
- No rusty_v8 change.
- Nimbus remains pinned to `nimbus/deno` `v2.8.3-nimbus.5` and `nimbus/rusty_v8`
  `v149.4.0-nimbus.1`.

Nimbus changes:

- The node-compat harness now loads this fixture as a CommonJS main module with
  `Module._load(..., isMain=true)`.
- The generated bundle still uses the normal guarded import-error path when the
  lane requests top-level skip capture.
- A regression test asserts this fixture is not loaded through ESM import or a
  nested `createRequire(...)` call.
- A non-ignored cycle-36 guard promotes the fixture for node22 and node24.

## Root Cause

The fixture installs an `uncaughtException` listener and then emits an unhandled
`EventEmitter` error at CommonJS top level:

```js
process.on('uncaughtException', common.mustCall((err) => {
  const [firstLine, ...lines] = err.stack.split('\n');
  assert.strictEqual(firstLine, 'Error');
  lines.forEach((line) => {
    assert.match(line, /^ {4}at/);
  });
}));

new EventEmitter().emit('error', new Error());
```

The prior harness side-loaded the fixture through the ESM wrapper, so Deno saw
the throw as a module-load failure rather than a CommonJS main-script fatal
exception. Deno already has a CJS main path in `Module._load` that invokes
`process._fatalException(err)` when `isMain && parent === null`; the harness was
not reaching that path for this main-script-shaped fixture.

Pre-fix focused census:

```bash
gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE="test/parallel/test-events-uncaught-exception-stack.js" \
  NIMBUS_RECENSUS_LANE=node24 NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
test/parallel/test-events-uncaught-exception-stack.js: upstream node_compat fixture `test/parallel/test-events-uncaught-exception-stack.js` should execute: runtime JavaScript error: Error
    at Object.<anonymous> (/private/tmp/nvx-NuwQky/app/.nimbus/convex/test/parallel/test-events-uncaught-exception-stack.js:16:34)
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 822 filtered out; finished in 2.03s
```

Diagnostic roots:

- `target/node-compat/diagnostics/batch/node24__nds_probe__summary.json`
- `target/node-compat/diagnostics/general/node24__test_parallel_test_events_uncaught_exception_stack_js.json`

## Proof

Focused post-fix censuses before promotion:

```bash
gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE="test/parallel/test-events-uncaught-exception-stack.js" \
  NIMBUS_RECENSUS_LANE=node24 NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 822 filtered out; finished in 1.98s
```

```bash
gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE="test/parallel/test-events-uncaught-exception-stack.js" \
  NIMBUS_RECENSUS_LANE=node22 NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 822 filtered out; finished in 1.92s
```

Harness regression:

```bash
gtimeout -s KILL 90 cargo test -p nimbus-runtime --lib \
  node_compat_uncaught_exception_stack_fixture_uses_commonjs_main_entry -- --nocapture
```

```text
test runtime::tests::node_compat::node_compat_uncaught_exception_stack_fixture_uses_commonjs_main_entry ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 824 filtered out; finished in 0.01s
```

Promoted non-ignored guards:

```bash
gtimeout -s KILL 120 cargo test -p nimbus-runtime --lib \
  cycle36_events_uncaught_exception_stack -- --nocapture
```

```text
node_compat node22-supported-lane-executes-cycle36-events-uncaught-exception-stack-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
test runtime::tests::node_compat::node22_supported_lane_executes_cycle36_events_uncaught_exception_stack_batch ... ok
node_compat node24-default-lane-executes-cycle36-events-uncaught-exception-stack-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test runtime::tests::node_compat::node24_default_lane_executes_cycle36_events_uncaught_exception_stack_batch ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 823 filtered out; finished in 3.88s
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
/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py sync
/opt/homebrew/bin/python3.12 scripts/runtime/node/status.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/dashboard.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/trends.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/publish_evidence.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py
```

```text
node22 required gaps: 63
node24 required gaps: 73
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
node22 required gaps: 63
node22 required pass rate: 97.35
node24 required gaps: 73
node24 required pass rate: 96.97
unique required fixtures remaining: 75
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
