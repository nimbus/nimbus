# NDS3 Cycle 29: process.getBuiltinModule Identity

Date: 2026-06-13

## Scope

This checkpoint promotes `test/parallel/test-process-get-builtin.mjs` for both
required lanes after the Deno v2.8.3 / rusty_v8 v149.4.0 foundation bump.

The fixture is a pure Node builtin identity and loader-contract fixture. The fix
does not add host process, signal, subprocess, cwd-mutation, filesystem, or
native capability. It keeps the sandbox boundary unchanged.

## Fork State

`nimbus/deno`:

- Branch: `nimbus/v2.8.3`
- Commit: `e4de17df418471d084caa8314b5863828240f12c`
- Tag: `v2.8.3-nimbus.2`
- Fork commit: `node: align builtin module identity`

`nimbus/rusty_v8`:

- Branch: `nimbus/v149.4.0`
- Tag: `v149.4.0-nimbus.1`
- No V8 or rusty_v8 native binding changes were made in this cycle.

Nimbus now pins Deno-family crates to immutable tag `v2.8.3-nimbus.2` and keeps
`v8` on `v149.4.0-nimbus.1`.

## Root Cause

The fixture exercises `process.getBuiltinModule()`, `require()`, ESM namespace
objects, `Module.builtinModules`, and load-hooked builtin identities. After the
foundation bump, several builtin paths were individually usable but not
reference-identical across all Node loader surfaces.

Failures grouped into one loader identity cluster:

- `process.getBuiltinModule()` used a direct builtin cache path instead of the
  active CommonJS/load-hook path.
- Affected Deno ESM wrappers exposed fresh default objects instead of the active
  builtin object for `tls`, `perf_hooks`, `vm`, `dgram`, `os`, and `readline`.
- Nimbus wrapper builtins cloned `tty` / `readline` / `readline/promises`,
  breaking identity with the cached Deno builtin object.
- Nimbus `process` installation wrapped Deno's process object, so Deno-owned
  builtin lookup and Nimbus-owned lane metadata were split.
- Node 22's `Module.builtinModules` shape needed to exclude `node:`-prefixed
  names while still allowing explicit `process.getBuiltinModule("node:test")`.

## Fix Summary

Deno fork:

- Route `process.getBuiltinModule()` through the active CJS require path so
  load-hooked builtin overrides remain visible.
- Tag hook-overridden builtin CJS modules so the cache can distinguish active
  overrides from stale default builtin entries.
- Align Deno ESM default exports for affected builtins with the active builtin
  object.
- Make `process.release` configurable/enumerable for Nimbus lane metadata.
- Avoid recursive `node:fs` loading during `node:repl` bootstrap.

Nimbus:

- Mutate Deno's process object in place instead of installing a proxy wrapper.
- Mutate Deno-owned `tty`, `readline`, and `readline/promises` builtin objects
  in place so CJS, ESM, and `process.getBuiltinModule()` preserve identity.
- Keep `node:fs/promises` bound to the Deno builtin object for identity.
- Prune `node:`-prefixed `Module.builtinModules` entries for the Node 22 lane.
- Add a static two-lane cycle promotion test for
  `test/parallel/test-process-get-builtin.mjs`.

## Proof Commands

Local Deno worktree proof before tagging:

```text
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-process-get-builtin.mjs \
NIMBUS_RECENSUS_LANE=node24 \
NIMBUS_RECENSUS_EXTRA_DIRS=test/common \
gtimeout -s KILL 90 cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

Result:

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 810 filtered out; finished in 2.08s
```

```text
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-process-get-builtin.mjs \
NIMBUS_RECENSUS_LANE=node22 \
NIMBUS_RECENSUS_EXTRA_DIRS=test/common \
gtimeout -s KILL 90 cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

Result:

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 810 filtered out; finished in 2.07s
```

Immutable tag proof after publishing `v2.8.3-nimbus.2` and repinning Nimbus:

```text
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-process-get-builtin.mjs \
NIMBUS_RECENSUS_LANE=node24 \
NIMBUS_RECENSUS_EXTRA_DIRS=test/common \
gtimeout -s KILL 90 cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

Result:

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 810 filtered out; finished in 2.09s
```

```text
NIMBUS_RECENSUS_FIXTURE=test/parallel/test-process-get-builtin.mjs \
NIMBUS_RECENSUS_LANE=node22 \
NIMBUS_RECENSUS_EXTRA_DIRS=test/common \
gtimeout -s KILL 90 cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

Result:

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 810 filtered out; finished in 1.99s
```

Static promotion proof:

```text
gtimeout -s KILL 90 cargo test -p nimbus-runtime --lib cycle29_get_builtin -- --nocapture
```

Result:

```text
node_compat node24-default-lane-executes-cycle29-get-builtin-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node22-supported-lane-executes-cycle29-get-builtin-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 810 filtered out; finished in 3.95s
```

Generated evidence refresh:

```text
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for script in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do /opt/homebrew/bin/python3.12 "scripts/runtime/node/${script}.py" >/dev/null || exit $?; done
```

Result:

- `tests/runtime/node/classifications/node22.json`: removed
  `test/parallel/test-process-get-builtin.mjs` from required gaps.
- `tests/runtime/node/classifications/node24.json`: removed
  `test/parallel/test-process-get-builtin.mjs` from required gaps.
- `docs/private/architecture/runtime/node-default-support-posture.json`:
  `node22.v8_isolate_required` = 70 gaps, 97.05% pass rate.
- `docs/private/architecture/runtime/node-default-support-posture.json`:
  `node24.v8_isolate_required` = 78 gaps, 96.77% pass rate.

## Guardrails

- No official Node fixture or checker edits.
- No false-green hand edits to generated JSON.
- No V8 or rusty_v8 native binding changes.
- No local Deno path pin left in Nimbus.
- Scratch probe include and temporary files were removed before promotion.
- PR #10 remains draft and unmerged.

## Recommended Next Action

Continue with broad, high-yield cluster waves. The largest remaining levers are
module loader/ESM/CJS residuals, async lifecycle, crypto/networking,
WebStreams/encoding, and remaining fs/stream fixtures. Keep using local Deno
path pins only during fork-owner development, then tag, repin, and reprove on an
immutable fork tag before promotion.
