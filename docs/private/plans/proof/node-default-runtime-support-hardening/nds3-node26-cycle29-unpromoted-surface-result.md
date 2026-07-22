# NDS3 node26 cycle 29 - unpromoted-surface reclass and HTTP parser promotion

Date: 2026-06-16
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

This wave burns 7 Node26 Current required gaps from
`node-compat/unpromoted-surface`:

- 2 fixtures were dynamically promoted after a non-ignored Node26 batch proved
  them green.
- 5 fixtures were honestly reclassified out of the V8-isolate-required
  denominator with source-confirmed host/native/platform evidence.

Node26 `v8_isolate_required` posture moved from `41` gaps / `98.04%`
(`2056 / 2097`) to `34` gaps / `98.37%` (`2058 / 2092`). Node22 and Node24
remain green at `0` gaps / `100.0%`.

Deno was unchanged at `v2.8.3-nimbus.60`
(`d7edcf7ab9b49c317849601cbe359e8db1939cdf`). rusty_v8 was unchanged at
`v149.4.0-nimbus.2`.

No V8 or rusty_v8 changes were made. No official upstream Node fixture or
checker was edited. No generated JSON was hand-edited to fake a green. No
`git add -A` was used.

## Broad Batch Before Promotion

The in-flight broad unpromoted-surface run before this checkpoint used:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave29-unpromoted-surface-broad1 \
  /opt/homebrew/bin/gtimeout -s KILL 300 \
  cargo test -p nimbus-runtime --lib node26_current_lane_unpromoted_surface_required_gap_watchpoint -- --ignored --nocapture
# selected=18
# killed by hard timeout after reaching test/parallel/test-webstreams-clone-unref.js
```

Diagnostics were retained under:

```text
/private/tmp/nds-node26-wave29-unpromoted-surface-broad1
```

That run grouped the cluster into async lifecycle, native/embedder, provider
boundary, Blob/structuredClone, stream/WebStreams, and trace-events root causes.

## Dynamic Promotions

The promoted batch added these two Node26 fixtures to
`NODE26_UNPROMOTED_SURFACE_PROMOTED_PATHS`:

- `test/async-hooks/test-httpparser-reuse.js`
- `test/parallel/test-async-hooks-http-parser-destroy.js`

The non-ignored promoted batch passed on the immutable Deno tag:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave29-unpromoted-surface-promoted1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_unpromoted_surface_promoted_batch_fixture -- --nocapture
# selected=20, passed=20, skipped=0, failed=0
```

Summary artifact:

```text
/private/tmp/nds-node26-wave29-unpromoted-surface-promoted1/batch/node26__node26_current_lane_executes_unpromoted_surface_promoted_batch__summary.json
```

## Structural Reclassifications

The following Node26 fixtures moved out of the required isolate denominator via
`scripts/runtime/node/classifications.py`. Each classification is lane-specific
and source-confirmed.

- `test/embedding/test-embedding-snapshot-vm.js`:
  source resolves `common.resolveBuiltBinary('embedtest')`, spawns that host
  helper, writes an `--embedder-snapshot-blob`, and spawns the helper again to
  reload it. This is host embedder-binary/snapshot coverage, not a
  multi-tenant isolate support claim.
- `test/embedding/test-shared-embedding-v8.js`:
  source self-skips unless `common.usesSharedLibrary` is true, then resolves
  and spawns `shared_embedtest`. This is shared-library embedder test-build
  coverage.
- `test/ffi/test-ffi-module.js`:
  source runs under `--experimental-ffi`, imports `node:ffi`, and validates
  subprocess-gated native FFI behavior. Default Nimbus isolates must not expose
  ambient host FFI/dlopen.
- `test/ffi/test-ffi-shared-buffer.js`:
  source runs under `--experimental-ffi --expose-internals`, loads
  `internal/test/binding('ffi')`, and exercises dlopen-backed shared-buffer
  calls against a native test library.
- `test/parallel/test-webcrypto-derivebits-argon2.js`:
  source self-skips unless `hasOpenSSL(3, 2)` is true. The current provider
  boundary does not expose OpenSSL 3.2 Argon2 WebCrypto support.

Representative diagnostics from the pre-wave broad root:

```text
test/embedding/test-embedding-snapshot-vm.js:
  TypeError: common.resolveBuiltBinary is not a function

test/ffi/test-ffi-module.js:
  TypeError: common.skipIfFFIMissing is not a function

test/parallel/test-webcrypto-derivebits-argon2.js:
  Skipped: requires OpenSSL >= 3.2
```

## Broad Batch After Promotion

After regeneration, the same broad watchpoint selected only the 11 remaining
unpromoted-surface required gaps:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave29-unpromoted-surface-broad2 \
  /opt/homebrew/bin/gtimeout -s KILL 180 \
  cargo test -p nimbus-runtime --lib node26_current_lane_unpromoted_surface_required_gap_watchpoint -- --ignored --nocapture
# selected=11
# exit=137 from the hard timeout after test/parallel/test-webstreams-clone-unref.js
```

No cargo/rustc/nextest/nimbus_runtime/gtimeout process remained after the
timeout.

Diagnostics were retained under:

```text
/private/tmp/nds-node26-wave29-unpromoted-surface-broad2
```

Remaining root-cause grouping from that broad root:

- DNS async resource lifecycle:
  `test/async-hooks/test-getaddrinforeqwrap.js`,
  `test/async-hooks/test-getnameinforeqwrap.js`,
  `test/async-hooks/test-querywrap.js` all fail `0 !== 1` assertions in their
  lookup/query callbacks through `ext:deno_node/dns.ts` and
  `internal_binding/cares_wrap.ts`.
- Async destroy / async-local-storage lifecycle:
  `test/parallel/test-async-hooks-fatal-error.js` fails `destroy 0 !== 1`;
  `test/parallel/test-async-local-storage-weak-asyncwrap-leak.js` fails
  `[] !== 0`.
- Blob / structuredClone error shape:
  `test/parallel/test-blob-file-backed.js` returns DOMException code `25`
  instead of Node `ERR_INVALID_STATE`;
  `test/parallel/test-structuredClone-global.js` still has the Node26 spelling
  mismatch, `can not` vs `cannot`.
- Stream/WebStreams behavior:
  `test/parallel/test-stream2-basic.js` fails chunk ordering/length
  expectations; `test/parallel/test-webstreams-clone-unref.js` exceeds the
  35s fixture timeout and sticks the broad run until the outer hard timeout.
- Trace-events output:
  `test/parallel/test-trace-events-api.js` fails `assert(fs.existsSync(file))`
  for the expected trace output file.

## Generator And Integrity Checks

```bash
python3 scripts/runtime/node/watchpoints.py sync
# wrote tests/runtime/node/expectations/rust-watchpoints.json

python3 scripts/runtime/node/classifications.py sync --lane all
# wrote node20, node22, node24, node26 classification catalogs

python3 scripts/runtime/node/status.py
# wrote target/node-compat/status/status-summary.{json,md}

python3 scripts/runtime/node/dashboard.py
# wrote target/node-compat/dashboard/dashboard-summary.{json,md}

python3 scripts/runtime/node/trends.py
# wrote target/node-compat/trends/trend-summary.{json,md}

python3 scripts/runtime/node/publish_evidence.py
# published tests/runtime/node/compat/node-compat-evidence/latest/*

python3 scripts/runtime/node/default_support_posture.py
# wrote private and public node-default-support-posture artifacts

python3 scripts/runtime/node/required_surface_blockers.py
# node22 required gaps: 0
# node24 required gaps: 0

python3 -B scripts/runtime/node/classifications.py sync --preserve-existing --check
# node20.json, node22.json, node24.json, node26.json are up to date

python3 -B scripts/runtime/node/default_support_posture.py --check
# node default support posture: pass

python3 -B scripts/runtime/node/required_surface_blockers.py --check
# node required-surface blocker inventory: pass

python3 -B scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 153 entries

python3 -B scripts/runtime/node/docs_guard.py
# Node LTS docs guard passed

cargo fmt --all --check
# pass

git diff --check
# pass

bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 14 passed, 20 failed
# [9] Node22/Node24 V8-isolate-required green: PASS
```

Generated posture after the wave:

```text
node22 v8_isolate_required.gaps = 0, pass_rate_percent = 100.0
node24 v8_isolate_required.gaps = 0, pass_rate_percent = 100.0
node26 v8_isolate_required.gaps = 34, pass_rate_percent = 98.37
```

## Recommended Next Wave

Stay in the remaining `node-compat/unpromoted-surface` cluster only if attacking
a coherent implementation group. The highest-yield local subgroups are:

- DNS async lifecycle in `deno_node` cares/dns wrappers: 3 fixtures.
- Blob + structuredClone Node26 error shape: 2 fixtures, but keep Node22/Node24
  spelling compatibility in view.
- Trace-events output file creation/flush: 1 fixture.
- Stream/WebStreams behavior: 3 fixtures, with `test-webstreams-clone-unref.js`
  still requiring a hang-safe proof strategy.

If switching clusters, the remaining global required-gap counts are:

```text
node-compat/unpromoted-surface: 11
runtime/v8: 7
process-and-timing/process-host: 6
streams-local-io/fs-host-io: 5
core-semantics/console: 4
loader-context/vm: 1
```
