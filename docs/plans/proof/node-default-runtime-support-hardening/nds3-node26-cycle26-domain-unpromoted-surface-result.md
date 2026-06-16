# NDS3 node26 cycle 26 - Domain and unpromoted-surface promotion

Date: 2026-06-15
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

This wave burns 32 Node26 Current required gaps by adding missing Node26
watchpoint coverage and promoting fixtures that were already green under the
current Deno/rusty_v8 foundation.

Node26 `v8_isolate_required` posture moved from `99` gaps / `95.28%`
(`1998 / 2097`) to `67` gaps / `96.8%` (`2030 / 2097`). Node22 and Node24
remain green at `0` gaps / `100.0%`.

No Deno fork change was needed. Nimbus remains pinned to immutable
`https://github.com/nimbus/deno`, tag `v2.8.3-nimbus.58`
(`cf321f2394ffd51ca56fffe7636f52beb7174f2a`). rusty_v8 was unchanged at
`v149.4.0-nimbus.2`.

No V8 or rusty_v8 changes were made. No official upstream Node fixture or
checker was edited. No generated JSON was hand-edited to fake a green. No
`git add -A` was used.

## Harness Changes

This wave adds Nimbus-local watchpoint coverage only:

- `node26_current_lane_loader_context_domain_watchpoint`, an ignored broad
  Node26 domain required-gap inventory beside the existing Node22/Node24 domain
  inventories.
- `node26_current_lane_executes_loader_context_domain_promoted_batch_fixture`,
  a non-ignored Node26 domain promoted batch covering 18 proven domain paths.
- `node26_current_lane_unpromoted_surface_required_gap_watchpoint`, an ignored
  owner-wide `node-compat/unpromoted-surface` Node26 inventory used for the
  mixed residual broad pass.
- `node26_current_lane_executes_unpromoted_surface_promoted_batch_fixture`, a
  non-ignored 14-fixture Node26 promoted batch for the clean subset found by the
  broad pass.

## Domain Proof

The fresh Node26 domain broad batch selected every remaining domain required
gap and passed:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave26-domain-broad1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_loader_context_domain_watchpoint -- --ignored --nocapture
# selected=18, passed=18, skipped=0, failed=0
```

After promotion, the non-ignored domain batch passed:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave26-domain-promoted1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_loader_context_domain_promoted_batch_fixture -- --nocapture
# selected=18, passed=18, skipped=0, failed=0
```

After regeneration, the ignored domain watchpoint drained:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave26-domain-drained1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_loader_context_domain_watchpoint -- --ignored --nocapture
# selected=0, passed=0, skipped=0, failed=0
```

Promoted domain paths:

- `test/parallel/test-domain-async-id-map-leak.js`
- `test/parallel/test-domain-crypto.js`
- `test/parallel/test-domain-emit-error-handler-stack.js`
- `test/parallel/test-domain-error-types.js`
- `test/parallel/test-domain-fs-enoent-stream.js`
- `test/parallel/test-domain-http-server.js`
- `test/parallel/test-domain-implicit-fs.js`
- `test/parallel/test-domain-multi.js`
- `test/parallel/test-domain-nested-throw.js`
- `test/parallel/test-domain-promise.js`
- `test/parallel/test-domain-safe-exit.js`
- `test/parallel/test-domain-set-uncaught-exception-capture-after-load.js`
- `test/parallel/test-domain-stack-empty-in-process-uncaughtexception.js`
- `test/parallel/test-domain-stack.js`
- `test/parallel/test-domain-thrown-error-handler-stack.js`
- `test/parallel/test-domain-timers-uncaught-exception.js`
- `test/parallel/test-domain-top-level-error-handler-clears-stack.js`
- `test/parallel/test-domain-vm-promise-isolation.js`

## Unpromoted-Surface Proof

The older filtered `node26_current_lane_unpromoted_parallel_discovery_watchpoint`
is now stale and intentionally panics because it selects only 2 fixtures, below
its historical reviewability floor:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave26-unpromoted-parallel-broad1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_unpromoted_parallel_discovery_watchpoint -- --ignored --nocapture
# panic: unpromoted parallel discovery selector should stay reviewable; selected 2 fixtures
```

The new owner-wide broad batch selected all 36 remaining
`node-compat/unpromoted-surface` gaps. It was interrupted after
`test/parallel/test-webstreams-clone-unref.js` exceeded the 35s per-fixture
diagnostic timeout and the outer Rust test stayed live:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave26-unpromoted-surface-broad1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_unpromoted_surface_required_gap_watchpoint -- --ignored --nocapture
# selected=36
# interrupted with ctrl-c after test/parallel/test-webstreams-clone-unref.js hung
```

The broad diagnostic root retained per-fixture diagnostics for the failing and
hanging subgroups:

- DNS async-resource accounting:
  `test/async-hooks/test-getaddrinforeqwrap.js`,
  `test/async-hooks/test-getnameinforeqwrap.js`,
  `test/async-hooks/test-querywrap.js`.
- Async lifecycle residuals:
  `test/parallel/test-async-hooks-fatal-error.js`,
  `test/parallel/test-async-local-storage-weak-asyncwrap-leak.js`.
- FFI/embedding helper or host-surface gaps:
  `test/ffi/test-ffi-module.js`,
  `test/ffi/test-ffi-shared-buffer.js`,
  `test/embedding/test-embedding-snapshot-vm.js`.
- Error-shape/API gaps:
  `test/parallel/test-blob-file-backed.js`,
  `test/parallel/test-permission-diagnostics-channel.js`,
  `test/parallel/test-stream2-basic.js`,
  `test/parallel/test-structuredClone-global.js`,
  `test/parallel/test-trace-events-api.js`.
- URLPattern constructor/brand gaps:
  `test/parallel/test-urlpattern-invalidthis.js`,
  `test/parallel/test-urlpattern-types.js`,
  `test/parallel/test-urlpattern.js`.
- WebStreams hang:
  `test/parallel/test-webstreams-clone-unref.js`.

Two self-skips were observed but not promoted in this wave:

- `test/embedding/test-shared-embedding-v8.js`: skipped because it only applies
  to test builds linked against the Node.js shared library.
- `test/parallel/test-webcrypto-derivebits-argon2.js`: skipped because it
  requires OpenSSL >= 3.2.

The clean subset was promoted and proved in a focused non-ignored batch. The
first 13-fixture run passed:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave26-unpromoted-surface-promoted1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_unpromoted_surface_promoted_batch_fixture -- --nocapture
# selected=13, passed=13, skipped=0, failed=0
```

An exploratory 15-fixture run added
`test/parallel/test-whatwg-encoding-singlebyte.mjs` and
`test/parallel/test-whatwg-webstreams-transform-stream-members.js`, but the
transform-stream fixture hung after the encoding fixture completed:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave26-unpromoted-surface-promoted2 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_unpromoted_surface_promoted_batch_fixture -- --nocapture
# selected=15
# interrupted with ctrl-c after test/parallel/test-whatwg-webstreams-transform-stream-members.js hung
```

The transform-stream fixture was removed from the promoted set. The final
14-fixture promoted batch passed:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave26-unpromoted-surface-promoted3 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_unpromoted_surface_promoted_batch_fixture -- --nocapture
# selected=14, passed=14, skipped=0, failed=0
```

Promoted unpromoted-surface paths:

- `test/async-hooks/test-async-exec-resource-http-32060.js`
- `test/async-hooks/test-async-exec-resource-http-agent.js`
- `test/async-hooks/test-async-exec-resource-http.js`
- `test/parallel/test-blob-createobjecturl.js`
- `test/parallel/test-diagnostic-channel-http-request-created.js`
- `test/parallel/test-diagnostic-channel-http-response-created.js`
- `test/parallel/test-dns-channel-cancel-promise.js`
- `test/parallel/test-dns-lookup-promises-options-deprecated.js`
- `test/parallel/test-dns-lookup-promises.js`
- `test/parallel/test-dns-perf_hooks.js`
- `test/parallel/test-dns-promises-exists.js`
- `test/parallel/test-gc-tls-external-memory.js`
- `test/parallel/test-heapdump-async-hooks-init-promise.js`
- `test/parallel/test-whatwg-encoding-singlebyte.mjs`

## Generator and Integrity Checks

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
# wrote node20, node22, node24, node26 classification catalogs

/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py sync
# wrote tests/runtime/node/expectations/rust-watchpoints.json

/opt/homebrew/bin/python3.12 scripts/runtime/node/status.py
# wrote target/node-compat/status/status-summary.{json,md}

/opt/homebrew/bin/python3.12 scripts/runtime/node/dashboard.py
# wrote target/node-compat/dashboard/dashboard-summary.{json,md}

/opt/homebrew/bin/python3.12 scripts/runtime/node/trends.py
# wrote target/node-compat/trends/trend-summary.{json,md}

/opt/homebrew/bin/python3.12 scripts/runtime/node/publish_evidence.py
# published tests/runtime/node/compat/node-compat-evidence/latest/*

/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py
# wrote private and public node-default-support-posture artifacts

/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py
# node22 required gaps: 0
# node24 required gaps: 0

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/classifications.py sync --preserve-existing --check
# node20.json, node22.json, node24.json, node26.json are up to date

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/default_support_posture.py --check
# node default support posture: pass

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/required_surface_blockers.py --check
# node required-surface blocker inventory: pass

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 153 entries

cargo fmt --all --check
# no output

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/docs_guard.py
# Node LTS docs guard passed: public docs avoid stale pass-rate, support-priority, and host-heavy overclaim prose

git diff --check
# no output
```

Regenerated public posture:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`, `2363 / 2363`.
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`, `2400 / 2400`.
- Node26 `v8_isolate_required`: `67` gaps, `96.8%`, `2030 / 2097`.

Verifier checkpoint:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 14 passed, 20 failed
```

Step 9 remains green for Node22/Node24. The verifier remains red honestly
because the broader NDS closeout proof rows are incomplete and Node26 still has
`67` required gaps.

## Remaining Node26 Required Gaps

After this wave:

- `loader-context/vm`: 23
- `node-compat/unpromoted-surface`: 22
- `runtime/v8`: 7
- `process-and-timing/process-host`: 6
- `streams-local-io/fs-host-io`: 5
- `core-semantics/console`: 4

Recommended next action: attack the 22 remaining `node-compat/unpromoted-surface`
fixtures by fixing the smaller error-shape groups first (`structuredClone`,
Blob clone error code, permission diagnostics, URLPattern), then separately
handle async-resource DNS accounting and the WebStreams hangs. Keep VM-module
work separate because it includes the known `HasTopLevelAwait`/import-meta
blockers.
