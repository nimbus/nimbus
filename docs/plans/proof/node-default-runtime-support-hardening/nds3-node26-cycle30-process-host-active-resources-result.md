# NDS3 Node26 Cycle 30: process active-resource promotion

This checkpoint promotes the six Node26 Current `process.getActiveResourcesInfo()`
fixtures from the process-host gap set into the non-ignored Node26 support lane.
It also carries the Deno fork fix needed to keep the existing
`test-process-get-builtin.mjs` process-host fixture green on the immutable Deno
tag.

## Scope

Promoted Node26 fixtures:

- `test/parallel/test-process-getactiveresources-track-active-handles.js`
- `test/parallel/test-process-getactiveresources-track-active-requests.js`
- `test/parallel/test-process-getactiveresources-track-interval-lifetime.js`
- `test/parallel/test-process-getactiveresources-track-multiple-timers.js`
- `test/parallel/test-process-getactiveresources-track-timer-lifetime.js`
- `test/parallel/test-process-getactiveresources.js`

The Deno fork change is:

```text
0931af4604 node: preserve getBuiltinModule CJS identity
tag: v2.8.3-nimbus.61
```

Nimbus is pinned back to immutable `nimbus/deno` tag `v2.8.3-nimbus.61`.
`rusty_v8` remains unchanged at `v149.4.0-nimbus.2`. No local Deno path pin
remains.

## Proof Commands

Local Deno worktree proof before tagging:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave30-process-host-local-deno-promoted2 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_process_host_promoted_batch_fixture -- --nocapture
# selected=33 passed=33 failed=0 skipped=0

NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave30-process-host-local-deno-broad2 \
  cargo test -p nimbus-runtime --lib node26_current_lane_process_host_watchpoint -- --ignored --nocapture
# selected=6 passed=6 failed=0 skipped=0
```

Immutable tag proof after `v2.8.3-nimbus.61` was pushed and Nimbus was repinned:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave30-process-host-tag61-promoted1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_process_host_promoted_batch_fixture -- --nocapture
# selected=33 passed=33 failed=0 skipped=0

NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave30-process-host-tag61-broad1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_process_host_watchpoint -- --ignored --nocapture
# selected=6 passed=6 failed=0 skipped=0
```

Generated evidence refresh:

```bash
python3 -B scripts/runtime/node/classifications.py sync --lane all
# wrote node20, node22, node24, node26 classification catalogs

python3 -B scripts/runtime/node/status.py
# wrote target/node-compat/status/status-summary.{json,md}

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
```

Integrity checks:

```bash
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

Verifier:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 14 passed, 20 failed
# [9] Node22/Node24 V8-isolate-required green: PASS
```

The verifier remains red honestly because Node26 still has required gaps and
the final NDS closeout rows are incomplete.

## Posture Movement

Before this checkpoint:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`, `2363 / 2363`.
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`, `2400 / 2400`.
- Node26 `v8_isolate_required`: `34` gaps, `98.37%`, `2058 / 2092`.

After this checkpoint:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`, `2363 / 2363`.
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`, `2400 / 2400`.
- Node26 `v8_isolate_required`: `28` gaps, `98.66%`, `2064 / 2092`.

The six active-resource fixture paths are gone from
`tests/runtime/node/classifications/node26.json` and from
`docs/private/plans/proof/node-default-runtime-support-hardening/nds3-required-surface-blockers.json`.

## Diagnostics

Retained diagnostic roots:

- `/private/tmp/nds-node26-wave30-process-host-promoted1`
- `/private/tmp/nds-node26-wave30-process-host-local-deno-promoted2`
- `/private/tmp/nds-node26-wave30-process-host-local-deno-broad2`
- `/private/tmp/nds-node26-wave30-process-host-tag61-promoted1`
- `/private/tmp/nds-node26-wave30-process-host-tag61-broad1`

Summary artifacts:

- `/private/tmp/nds-node26-wave30-process-host-local-deno-promoted2/batch/node26__node26_current_lane_executes_process_host_promoted_batch__summary.json`
- `/private/tmp/nds-node26-wave30-process-host-local-deno-broad2/batch/node26__node26_current_lane_process_host_watchpoint__summary.json`
- `/private/tmp/nds-node26-wave30-process-host-tag61-promoted1/batch/node26__node26_current_lane_executes_process_host_promoted_batch__summary.json`
- `/private/tmp/nds-node26-wave30-process-host-tag61-broad1/batch/node26__node26_current_lane_process_host_watchpoint__summary.json`

## Remaining Node26 Required Gaps

Node26 now has `28` `v8_isolate_required` gaps:

- `11` `node-compat/unpromoted-surface`
- `7` `runtime/v8`
- `5` `streams-local-io/fs-host-io`
- `4` `core-semantics/console`
- `1` `loader-context/vm`

The next high-yield wave should start with a fresh broad run over the remaining
required blockers, then prefer coherent implementation clusters: async DNS and
async lifecycle residuals, console core semantics, fs-host-io, and the remaining
WebStreams/Blob/structuredClone residuals. Keep `runtime/v8` separate unless a
prebuilt rusty_v8 export or non-native implementation route is available.
