# NDS3 cycle 35 result - prepareStackTrace source-map self-exec

Date: 2026-06-13

## Scope

Fixed and promoted the required fixture on both default lanes:

- `test/parallel/test-error-prepare-stack-trace.js` (node22, node24)

Deno fork release:

- `nimbus/deno` commit `7ec6b93296`
- tag `v2.8.3-nimbus.5`

Fork changes:

- `ext/node/polyfills/internal/errors.ts` now exposes a Node-shaped default
  `Error.prepareStackTrace` function and preserves override behavior.
- `ext/node/polyfills/internal_binding/node_options.ts` now parses
  `--enable-source-maps` and `--no-enable-source-maps`.

Nimbus changes:

- The node-compat subprocess parser accepts the source-map flags used by the
  fixture's self-spawn.
- Script-mode self-exec from a copied source bundle rewrites the main script path
  into the child bundle root, so helper temp writes stay inside the child runtime
  grants instead of pointing at the parent bundle root.
- The fixture receives the existing narrow `$runtime_self_exec` grant and is
  promoted into a non-ignored cycle-35 guard for node22 and node24.

## Proof

Local-fork focused census after a temporary Cargo path override to
`/Users/jack/src/github.com/nimbus/deno/ext/node`:

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 820 filtered out

node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 820 filtered out
```

Immutable-tag focused census after publishing `v2.8.3-nimbus.5`, removing the
Cargo path override, and repinning `Cargo.toml`/`Cargo.lock`:

```bash
gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE="test/parallel/test-error-prepare-stack-trace.js" \
  NIMBUS_RECENSUS_LANE=node24 NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 820 filtered out; finished in 4.02s
```

```bash
gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE="test/parallel/test-error-prepare-stack-trace.js" \
  NIMBUS_RECENSUS_LANE=node22 NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 820 filtered out; finished in 3.90s
```

Promoted non-ignored guards:

```bash
gtimeout -s KILL 120 cargo test -p nimbus-runtime --lib cycle35_error_prepare_stack_trace -- --nocapture
```

```text
node_compat node22-supported-lane-executes-cycle35-error-prepare-stack-trace-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
test runtime::tests::node_compat::node22_supported_lane_executes_cycle35_error_prepare_stack_trace_batch ... ok
node_compat node24-default-lane-executes-cycle35-error-prepare-stack-trace-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test runtime::tests::node_compat::node24_default_lane_executes_cycle35_error_prepare_stack_trace_batch ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 820 filtered out; finished in 7.81s
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
node22 required gaps: 64
node24 required gaps: 74
node22 64 97.3
node24 74 96.93
unique required fixtures remaining: 76
```

## Guardrails

- No V8 or rusty_v8 source changes.
- No official fixture or checker edits.
- No hand-edited false-green JSON.
- Temporary Cargo path override was removed before immutable-tag proof.
- Scratch `nds_probe` include/file was removed before promotion.
- PR #10 remains draft; the gate is still red and honest.
