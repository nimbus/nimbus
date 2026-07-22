# NDS3 node26 cycle 24 - Current residual completion

Date: 2026-06-15
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

This wave completes the 15-fixture Node26 Current residual catalog left by
cycle 23 and promotes those fixtures into the non-ignored Node26 Current
residual promoted batch.

Node26 `v8_isolate_required` posture moved from `121` gaps / `94.23%` to
`106` gaps / `94.95%` (`1991 / 2097`). Node22 and Node24 remain green at
`0` gaps / `100.0%`.

The Deno fork was advanced from `v2.8.3-nimbus.56` to published lightweight tag
`v2.8.3-nimbus.57`:

- Deno branch: `/Users/jack/src/github.com/nimbus/deno`, `nimbus/v2.8.3`
- Deno commit: `6194437d9736a8a4a9f8a8586a298d4b7314de59`
- Deno commit subject: `Improve Node26 polyfill compatibility`
- Published tag: `v2.8.3-nimbus.57`

Nimbus was repinned from the temporary local Deno path pin back to immutable
`https://github.com/nimbus/deno`, tag `v2.8.3-nimbus.57`. `Cargo.lock` records
`#6194437d9736a8a4a9f8a8586a298d4b7314de59`.

No V8 or rusty_v8 changes were made. No official upstream Node fixture or
checker was edited. No generated JSON was hand-edited to fake a green. No
`git add -A` was used.

## Cleanup

Before continuing the wave, disk pressure was checked and only the active PR
worktree build cache was cleaned:

```bash
df -h /System/Volumes/Data /Users/jack/src/github.com/nimbus/nimbus /private/tmp
# /System/Volumes/Data available before cleanup: 129Gi

du -sh /Users/jack/src/github.com/nimbus/nimbus/target \
  /Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target \
  /Users/jack/src/github.com/nimbus/deno/target \
  /Users/jack/src/github.com/nimbus/rusty_v8/target
# main nimbus target: 22G
# PR worktree target: 5.5G
# deno target: absent
# rusty_v8 target: absent

cargo clean
# Removed 20461 files, 6.3GiB total

df -h /System/Volumes/Data
# /System/Volumes/Data available after cleanup: 135Gi
```

The main repo target was left untouched because it is outside the active PR #10
worktree. NDS diagnostic roots were retained because all `nds-*` roots under
`/private/tmp` were only `8.1M` total.

## Local Deno-Path Proof

The local Deno-path broad batch proved the implementation before tagging:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-current-residual-local-cycle24-broad14 \
  cargo test -p nimbus-runtime --lib node26_current_lane_broad_residual_watchpoint -- --ignored --nocapture
# selected=15, passed=13, skipped=0, failed=2
```

The remaining two failures were:

- `test/parallel/test-http2-options-max-headers-exceeds-nghttp2.js`
- `test/parallel/test-https-agent-session-reuse.js`

After the final HTTP2/TLS fixes:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-current-residual-local-cycle24-broad15 \
  cargo test -p nimbus-runtime --lib node26_current_lane_broad_residual_watchpoint -- --ignored --nocapture
# selected=15, passed=15, skipped=0, failed=0
```

The Node22/Node24 compatibility regression checks were:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-current-residual-local-cycle24-older-fs-glob \
  cargo test -p nimbus-runtime --lib fs_glob_fixture -- --nocapture
# 2 passed, 0 failed

NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-current-residual-local-cycle24-older-tls-session \
  cargo test -p nimbus-runtime --lib node22_networking_https_tls_session_batch_fixture -- --nocapture
# 1 passed, 0 failed, 1 ignored

NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-current-residual-local-cycle24-older-http2-node22-rerun \
  cargo test -p nimbus-runtime --lib node22_supported_lane_executes_http2_diagnostic_core_promoted_batch_fixture -- --nocapture
# selected=129, passed=129, skipped=0, failed=0

NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-current-residual-local-cycle24-older-http2-node24 \
  cargo test -p nimbus-runtime --lib node24_default_lane_executes_http2_diagnostic_core_promoted_batch_fixture -- --nocapture
# selected=129, passed=129, skipped=0, failed=0
```

After tightening the Node26-only HTTP2 behavior gate, the local Node26 broad
batch was rerun:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-current-residual-local-cycle24-broad16 \
  cargo test -p nimbus-runtime --lib node26_current_lane_broad_residual_watchpoint -- --ignored --nocapture
# selected=15, passed=15, skipped=0, failed=0
```

## Immutable-Tag Proof

After Deno commit/tag/push and Nimbus repin:

```bash
cargo check -p nimbus-runtime --lib
# Finished dev profile; lock resolved Deno crates to v2.8.3-nimbus.57#6194437d

NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-current-residual-tag57-broad1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_broad_residual_watchpoint -- --ignored --nocapture
# selected=15, passed=15, skipped=0, failed=0
```

The 15 green residual fixtures were promoted into
`NODE26_CURRENT_RESIDUAL_PROMOTED_PATHS`, alongside the prior 13 promoted paths:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-current-residual-tag57-promoted1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_current_residual_promoted_batch_fixture -- --nocapture
# selected=28, passed=28, skipped=0, failed=0

NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-current-residual-tag57-drained1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_broad_residual_watchpoint -- --ignored --nocapture
# selected=0, passed=0, skipped=0, failed=0
```

Promoted paths from this wave:

- `test/parallel/test-buffer-indexof.js`
- `test/parallel/test-crypto-dh.js`
- `test/parallel/test-crypto-gcm-implicit-short-tag.js`
- `test/parallel/test-crypto-scrypt.js`
- `test/parallel/test-fs-glob.mjs`
- `test/parallel/test-http2-misbehaving-flow-control-paused.js`
- `test/parallel/test-http2-misbehaving-flow-control.js`
- `test/parallel/test-http2-options-max-headers-exceeds-nghttp2.js`
- `test/parallel/test-https-agent-session-reuse.js`
- `test/parallel/test-process-load-env-file.js`
- `test/parallel/test-runner-get-test-context.js`
- `test/parallel/test-stream-duplex.js`
- `test/parallel/test-trace-events-dynamic-enable.js`
- `test/parallel/test-url-parse-invalid-input.js`
- `test/parallel/test-util-parse-env.js`

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
# validated node-compat watchpoint catalog: 151 entries

cargo fmt --all --check
# no output

git diff --check
# no output
```

Regenerated public posture:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`, `2363 / 2363`.
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`, `2400 / 2400`.
- Node26 `v8_isolate_required`: `106` gaps, `94.95%`, `1991 / 2097`.

Verifier checkpoint:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 14 passed, 20 failed
```

Step 9 remains green for Node22/Node24. The verifier remains red honestly
because the broader NDS closeout proof rows are incomplete and Node26 still has
`106` required gaps.

## Diagnostics

Retained diagnostic roots:

- `/private/tmp/nds-node26-current-residual-local-cycle24-broad14`
- `/private/tmp/nds-node26-current-residual-local-cycle24-broad15`
- `/private/tmp/nds-node26-current-residual-local-cycle24-broad16`
- `/private/tmp/nds-node26-current-residual-local-cycle24-older-http2-node22-rerun`
- `/private/tmp/nds-node26-current-residual-local-cycle24-older-http2-node24`
- `/private/tmp/nds-node26-current-residual-tag57-broad1`
- `/private/tmp/nds-node26-current-residual-tag57-promoted1`
- `/private/tmp/nds-node26-current-residual-tag57-drained1`

Summary artifacts:

- `/private/tmp/nds-node26-current-residual-local-cycle24-broad14/batch/node26__node26_current_lane_broad_residual_watchpoint__summary.json`
- `/private/tmp/nds-node26-current-residual-local-cycle24-broad15/batch/node26__node26_current_lane_broad_residual_watchpoint__summary.json`
- `/private/tmp/nds-node26-current-residual-local-cycle24-broad16/batch/node26__node26_current_lane_broad_residual_watchpoint__summary.json`
- `/private/tmp/nds-node26-current-residual-local-cycle24-older-http2-node22-rerun/batch/node22__node22_supported_lane_executes_http2_diagnostic_core_promoted_batch__summary.json`
- `/private/tmp/nds-node26-current-residual-local-cycle24-older-http2-node24/batch/node24__node24_default_lane_executes_http2_diagnostic_core_promoted_batch__summary.json`
- `/private/tmp/nds-node26-current-residual-tag57-broad1/batch/node26__node26_current_lane_broad_residual_watchpoint__summary.json`
- `/private/tmp/nds-node26-current-residual-tag57-promoted1/batch/node26__node26_current_lane_executes_current_residual_promoted_batch__summary.json`
- `/private/tmp/nds-node26-current-residual-tag57-drained1/batch/node26__node26_current_lane_broad_residual_watchpoint__summary.json`

## Remaining Node26 Required Gaps

After regeneration, Node26 has `106` required gaps:

- `40` `node-compat/unpromoted-surface`
- `23` `loader-context/vm`
- `18` `loader-context/domain`
- `7` `runtime/v8`
- `6` `process-and-timing/process-host`
- `5` `streams-local-io/fs-host-io`
- `4` `core-semantics/console`
- `2` `core-semantics/assert`
- `1` `core-semantics/url`

Recommended next action: run a fresh ROI scan over these nine remaining
owners. Prefer another coherent implementation wave over singleton cleanup.
The likely highest-yield implementation targets are the `node-compat/unpromoted-
surface` leftovers, `loader-context/vm`, and `loader-context/domain`; keep
`runtime/v8` separate because some entries may still depend on native V8 or
deno_core boundary work.

## Integrity

- No V8 or rusty_v8 source was changed.
- No official upstream Node fixture or checker was edited.
- No generated JSON was hand-edited to fake a green.
- No local Deno path pin remains in `Cargo.toml` or `Cargo.lock`.
- `measure_ah.sh` and other scratch files remain untracked.
