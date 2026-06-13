# NDS3 cycle 31 result - console symbol inspect parity

Date: 2026-06-13

## Scope

Promoted the node22-only required fixture:

- `test/parallel/test-console.js`

This was a fork-owner fix in `nimbus/deno`'s `deno_web` console formatter, then a normal Nimbus repin to the published immutable tag.

## Root cause

`console.dir()` with `customInspect: false` exposed symbol-keyed properties without Node's bracketed symbol property label. Node expects:

```text
[Symbol(nodejs.util.inspect.custom)]: [Function: [nodejs.util.inspect.custom]]
```

The Deno formatter printed:

```text
Symbol(nodejs.util.inspect.custom): [Function: [nodejs.util.inspect.custom]]
```

The fix wraps the symbol label in brackets in `ext/web/01_console.js`.

## Fork state

- Deno branch: `nimbus/v2.8.3`
- Deno commit/tag: `7bd83bd7de` / `v2.8.3-nimbus.3`
- Deno change: `web: align symbol property inspect labels`
- rusty_v8: unchanged, still `v149.4.0-nimbus.1`

Nimbus is pinned back to the immutable Deno tag in `Cargo.toml` and `Cargo.lock`; no local Deno path override remains.

## Proof

Initial focused probe, before the fork fix:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-console.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture 2>&1 \
  | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|deep-equal|\+ actual|- expected|at async.*test-console|AssertionError|Error:'
```

Result:

```text
node_compat nds-probe node22 summary: selected=1, passed=0, skipped=0, failed=1
actual:   '  Symbol(nodejs.util.inspect.custom): [Function: [nodejs.util.inspect.custom]]\n'
expected: '  [Symbol(nodejs.util.inspect.custom)]: [Function: [nodejs.util.inspect.custom]]\n'
```

Focused proof with temporary local Deno path override:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-console.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture 2>&1 \
  | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|failed='
```

Result:

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 815 filtered out
```

Published fork tag and repin:

```bash
git add ext/web/01_console.js
git commit -m "web: align symbol property inspect labels"
git tag v2.8.3-nimbus.3
git push origin nimbus/v2.8.3
git push origin v2.8.3-nimbus.3
cargo update -p deno_web
cargo clean -p deno_web
```

Immutable-tag proof after removing the local Deno path override:

```bash
gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-console.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture 2>&1 \
  | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|failed='
```

Result:

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 815 filtered out
```

Promoted non-ignored guard:

```bash
gtimeout -s KILL 90 cargo test -p nimbus-runtime --lib cycle31_console -- --nocapture 2>&1 \
  | grep -iE 'summary: selected|test result|should execute|error\[|FAILED|failed='
```

Result:

```text
node_compat node22-supported-lane-executes-cycle31-console-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 815 filtered out
```

Generated posture after classification sync and evidence regeneration:

```text
node22: v8_isolate_required.gaps = 68, pass_rate_percent = 97.14
node24: v8_isolate_required.gaps = 77, pass_rate_percent = 96.81
unique required fixtures remaining: 80
```

## Guardrails

- No V8 or rusty_v8 changes.
- No official fixture or checker edits.
- No hand-edited false-green JSON.
- Temporary local Deno path override was removed before immutable-tag proof.
- Scratch `nds_probe` include/file was removed before promotion.
- PR #10 remains draft; the gate is still red and honest.
