# NDS3 node26 cycle 31 - DNS async hooks promotion

Date: 2026-06-16
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

This checkpoint burns 3 Node26 Current required gaps from the DNS async-hooks
subcluster:

- `test/async-hooks/test-getaddrinforeqwrap.js`
- `test/async-hooks/test-getnameinforeqwrap.js`
- `test/async-hooks/test-querywrap.js`

The implementation lives in the Nimbus Deno fork:

- Repo: `/Users/jack/src/github.com/nimbus/deno`
- Branch: `nimbus/v2.8.3`
- Commit: `1066b3a6dd573c00cc354d1cb952c27130640b0f`
- Tag: `v2.8.3-nimbus.62`
- Commit subject: `node: emit async hooks for DNS requests`
- Changed file: `ext/node/polyfills/internal_binding/cares_wrap.ts`

Nimbus is repinned from immutable Deno tag `v2.8.3-nimbus.61` to
`v2.8.3-nimbus.62`. `rusty_v8` is unchanged at `v149.4.0-nimbus.2`.

Node26 `v8_isolate_required` posture moved from `28` gaps / `98.66%`
(`2064 / 2092`) to `25` gaps / `98.8%` (`2067 / 2092`). Node22 and Node24
remain green at `0` gaps / `100.0%`.

No V8 or rusty_v8 changes were made. No official upstream Node fixture or
checker was edited. No generated JSON was hand-edited to fake a green. No
`git add -A` was used.

## Deno Fork Change

`ext/node/polyfills/internal_binding/cares_wrap.ts` now emits async-hooks
lifecycle events for DNS request wrappers:

- allocates a JS-side async id with `newAsyncId()`;
- records `executionAsyncId()` as the trigger id;
- emits `init` before dispatch;
- emits `before`, `after`, and `destroy` around completion callbacks;
- clears the stored async ids after completion or cancellation.

The first local attempt used the native `req.getAsyncId()` path and failed
because these request objects are not native `AsyncWrap`s in this embedded path:

```text
/private/tmp/nds-node26-cycle31-dns-async-hooks-local-deno-focused2
# selected=3, passed=0, skipped=0, failed=3
# representative detail: TypeError: expected AsyncWrap
```

The final fix follows the existing JS-side resource-id pattern used by other
Deno Node polyfills.

`deno fmt --check ext/node/polyfills/internal_binding/cares_wrap.ts` was not
used as a proof gate for this checkpoint because the upstream file is an
IIFE-wrapped polyfill and the formatter wants to reindent the whole file. The
Nimbus checkpoint instead relies on `git diff --check`, Rust formatting, and
fixture execution against the compiled embedded sources.

## Proof Commands

Local Deno proof before publishing the fork tag:

```bash
env NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle31-dns-async-hooks-local-deno-focused3 \
  cargo test -p nimbus-runtime --lib node26_current_lane_dns_async_hooks_required_gap_watchpoint -- --ignored --nocapture
# selected=3, passed=3, skipped=0, failed=0
```

```bash
env NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle31-dns-async-hooks-local-deno-promoted1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_async_hooks_promoted_batch_fixture -- --nocapture
# selected=73, passed=73, skipped=0, failed=0
```

Immutable tag proof after publishing `v2.8.3-nimbus.62`:

```bash
git ls-remote --tags origin v2.8.3-nimbus.62
# 1066b3a6dd573c00cc354d1cb952c27130640b0f refs/tags/v2.8.3-nimbus.62
```

```bash
env NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle31-dns-async-hooks-tag62-focused1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_dns_async_hooks_required_gap_watchpoint -- --ignored --nocapture
# selected=3, passed=3, skipped=0, failed=0
```

```bash
env NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle31-dns-async-hooks-tag62-promoted1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_async_hooks_promoted_batch_fixture -- --nocapture
# selected=73, passed=73, skipped=0, failed=0
```

After the three DNS fixtures were moved into
`ASYNC_HOOKS_PROMOTED_NODE26_PATHS`, the durable non-ignored promoted batch
proved the full Node26 async-hooks promoted set on the immutable tag:

```bash
env NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-cycle31-dns-async-hooks-tag62-promoted2 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_async_hooks_promoted_batch_fixture -- --nocapture
# selected=76, passed=76, skipped=0, failed=0
```

Summary artifact:

```text
/private/tmp/nds-node26-cycle31-dns-async-hooks-tag62-promoted2/batch/node26__node26_current_lane_executes_async_hooks_promoted_batch__summary.json
```

That promoted run emitted noisy child stderr from unrelated fixtures after the
summary, but Cargo exited `0` and the summary artifact records
`selected=76`, `passed=76`, `failed=0`.

The temporary focused Rust watchpoint used for development was removed before
this checkpoint. The durable committed proof is the non-ignored promoted
async-hooks batch.

## Generator And Integrity Checks

```bash
python3 -B scripts/runtime/node/watchpoints.py sync
# wrote tests/runtime/node/expectations/rust-watchpoints.json

python3 -B scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 153 entries

python3 -B scripts/runtime/node/status.py
# wrote target/node-compat/status/status-summary.{json,md}

python3 -B scripts/runtime/node/classifications.py sync --lane all
# wrote node20, node22, node24, node26 classification catalogs

python3 -B scripts/runtime/node/dashboard.py
# wrote target/node-compat/dashboard/dashboard-summary.{json,md}

python3 -B scripts/runtime/node/trends.py
# wrote target/node-compat/trends/trend-summary.{json,md}

python3 -B scripts/runtime/node/publish_evidence.py
# published tests/runtime/node/compat/node-compat-evidence/latest/*

python3 -B scripts/runtime/node/default_support_posture.py
# wrote private and public node-default-support-posture artifacts

python3 -B scripts/runtime/node/required_surface_blockers.py
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
```

Additional docs reference check:

```bash
npm run docs:validate-refs:strict
# failed: 32 broken references
```

The broken references are existing private/staging doc-link issues such as
`../../docs/private/adapters/...` and
`../../staging/runtimes/nodejs/evidence/...`; they are not introduced by this
DNS async-hooks checkpoint.

Verifier:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 14 passed, 20 failed
# [9] Node22/Node24 V8-isolate-required green: PASS
```

The overall verifier remains red honestly because the broader NDS closeout
proof rows are incomplete and Node26 still has `25` Current-lane required gaps.
PR #10 remains draft and unmerged.

## Current Posture

Generated `docs/architecture/runtime/node-default-support-posture.json` after
this checkpoint:

```text
node22 v8_isolate_required.gaps = 0, pass_rate_percent = 100.0
node24 v8_isolate_required.gaps = 0, pass_rate_percent = 100.0
node26 v8_isolate_required.gaps = 25, pass_rate_percent = 98.8
```

Remaining Node26 required gaps by generated owner:

```text
node-compat/unpromoted-surface: 8
runtime/v8: 7
streams-local-io/fs-host-io: 5
core-semantics/console: 4
loader-context/vm: 1
```

Remaining exact Node26 required fixtures:

```text
test/parallel/test-async-hooks-fatal-error.js
test/parallel/test-async-local-storage-weak-asyncwrap-leak.js
test/parallel/test-blob-file-backed.js
test/parallel/test-console-diagnostics-channels.js
test/parallel/test-console-issue-43095.js
test/parallel/test-console-with-frozen-intrinsics.js
test/parallel/test-console.js
test/parallel/test-fs-promises-watch-ignore-invalid.mjs
test/parallel/test-fs-promises-watch.js
test/parallel/test-fs-sir-writes-alot.js
test/parallel/test-fs-stat-temporal.mjs
test/parallel/test-fs-write-buffer-large.js
test/parallel/test-stream2-basic.js
test/parallel/test-structuredClone-global.js
test/parallel/test-trace-events-api.js
test/parallel/test-v8-collect-gc-profile-exit-before-stop.js
test/parallel/test-v8-collect-gc-profile-using.js
test/parallel/test-v8-collect-gc-profile.js
test/parallel/test-v8-getheapsnapshot-twice.js
test/parallel/test-v8-global-setter.js
test/parallel/test-v8-heap-profile.js
test/parallel/test-v8-string-is-one-byte-representation.js
test/parallel/test-vm-module-evaluate-while-evaluating.js
test/parallel/test-webstreams-clone-unref.js
test/parallel/test-whatwg-webstreams-transform-stream-members.js
```

## Recommended Next Wave

Start from the remaining 25-gap posture and pick the broadest coherent
implementation cluster, not a singleton:

- `runtime/v8` has the largest count at 7 fixtures, but may include native/V8
  API constraints and must avoid rusty_v8 edits unless a separate prebuilt
  release path is intentionally opened.
- `streams-local-io/fs-host-io` has 5 fixtures and should begin with a broad
  ignored batch to distinguish virtual-fs semantics from host-policy tests.
- `core-semantics/console` has 4 app-visible fixtures and is likely a good
  implementation wave if the current V8/host clusters prove lower ROI.
- The remaining `node-compat/unpromoted-surface` group is mixed; batch it only
  if the selected fixtures share a root cause.

Do not undraft or merge PR #10 until Node22, Node24, and Node26 all reach
`v8_isolate_required.gaps == 0` and `pass_rate_percent == 100.0` with the full
gate verified.
