# NDS3 cycle 64 - domain async-id weak retention

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

The following fixture was dynamically promoted for node22 and node24:

- `test/parallel/test-domain-async-id-map-leak.js`

Gate movement from the generated private posture:

- node22: 32 -> 31 gaps, 98.69% pass rate
- node24: 39 -> 38 gaps, 98.42% pass rate
- unique remaining required fixtures: 40

The fix landed in the Deno fork as tag `v2.8.3-nimbus.17`
(`5c0197035b`, `node(domain): avoid retaining domains from async id map`).
Nimbus was repinned from `v2.8.3-nimbus.16` to `v2.8.3-nimbus.17` and the
fixture was re-proven from the published tag.

## Root Cause

`test-domain-async-id-map-leak.js` creates a `domain`, an `AsyncResource`, and an
`EventEmitter`, then removes all userland strong references and waits for all
three objects to be garbage collected. Deno's `ext/node/polyfills/domain.ts`
stored `asyncId -> process.domain` in a strong `SafeMap`. That strong map entry
kept the domain/resource/emitter graph reachable, so the `AsyncResource`
finalizer could not emit the async destroy hook that would delete the map entry.

Node's `lib/domain.js` stores `asyncId -> process.domain[kWeak]`, where `kWeak`
is a `WeakReference` wrapper. The Deno fork now mirrors that shape: each
`Domain` owns a `WeakReference`, async init stores that weak wrapper in the
pairing map, and async before/after temporarily increments/decrements the strong
reference while entering/exiting the domain.

No V8/rusty_v8 changes were made.

## Non-Promotion Note

The related fixture
`test/parallel/test-domain-stack-empty-in-process-uncaughtexception.js` was also
probed under the same local fork patch, but it still failed with `Error: boom`.
That is a distinct top-level uncaught-exception/domain stack cleanup issue and
remains in `NDS-GATE-BLOCKER.md`.

## Verification

Initial local probes on the temporary Cargo path override to
`/Users/jack/src/github.com/nimbus/deno/ext/node`:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle64-domain-async-id-minimal-node24 \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-domain-async-id-map-leak.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 860 filtered out; finished in 2.00s

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle64-domain-async-id-minimal-node22 \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-domain-async-id-map-leak.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 860 filtered out; finished in 1.91s
```

Related sibling probe, intentionally not promoted:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle64-domain-stack-local-v2-node24 \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-domain-stack-empty-in-process-uncaughtexception.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
# runtime JavaScript error: Error: boom
```

Focused domain regression guards on the local fork patch:

```bash
cargo test -p nimbus-runtime --lib node24_node_tools_domain_foundation_batch_fixture
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 860 filtered out; finished in 30.09s

cargo test -p nimbus-runtime --lib node22_node_tools_domain_foundation_batch_fixture
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 860 filtered out; finished in 30.32s

cargo test -p nimbus-runtime --lib node22_node_tools_domain_promise_watchpoint
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 860 filtered out; finished in 1.89s

cargo test -p nimbus-runtime --lib node24_default_lane_executes_nds3_domain_fork_promoted_batch_fixture
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 860 filtered out; finished in 1.90s

cargo test -p nimbus-runtime --lib node22_supported_lane_executes_nds3_domain_fork_promoted_batch_fixture
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 860 filtered out; finished in 1.89s

cargo test -p nimbus-runtime --lib node24_default_lane_executes_cycle60_domain_capture_after_load_batch
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 860 filtered out; finished in 1.90s

cargo test -p nimbus-runtime --lib node22_supported_lane_executes_cycle60_domain_capture_after_load_batch
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 860 filtered out; finished in 1.88s

cargo test -p nimbus-runtime --lib node24_default_lane_executes_loader_context_domain_promoted_batch_fixture
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 860 filtered out; finished in 27.53s

cargo test -p nimbus-runtime --lib node22_supported_lane_executes_loader_context_domain_promoted_batch_fixture
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 860 filtered out; finished in 27.59s

cargo test -p nimbus-runtime --lib node24_default_lane_executes_cycle59_event_loop_timers_batch
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 860 filtered out; finished in 3.84s

cargo test -p nimbus-runtime --lib node22_supported_lane_executes_cycle59_event_loop_timers_batch
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 860 filtered out; finished in 3.91s
```

Immutable tag probes after publishing `v2.8.3-nimbus.17` and repinning
`Cargo.toml`/`Cargo.lock`:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle64-domain-async-id-tag-node24 \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-domain-async-id-map-leak.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 860 filtered out; finished in 2.05s

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle64-domain-async-id-tag-node22 \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-domain-async-id-map-leak.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 860 filtered out; finished in 1.97s
```

Real promotion guards on the published tag:

```bash
cargo test -p nimbus-runtime --lib node24_default_lane_executes_cycle64_domain_async_id_batch -- --nocapture
# node_compat node24-default-lane-executes-cycle64-domain-async-id-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 861 filtered out; finished in 2.02s

cargo test -p nimbus-runtime --lib node22_supported_lane_executes_cycle64_domain_async_id_batch -- --nocapture
# node_compat node22-supported-lane-executes-cycle64-domain-async-id-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 861 filtered out; finished in 1.97s
```

Regenerated lightweight posture/evidence pipeline:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
/opt/homebrew/bin/python3.12 scripts/runtime/node/status.py >/dev/null
/opt/homebrew/bin/python3.12 scripts/runtime/node/dashboard.py >/dev/null
/opt/homebrew/bin/python3.12 scripts/runtime/node/trends.py >/dev/null
/opt/homebrew/bin/python3.12 scripts/runtime/node/publish_evidence.py >/dev/null
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py >/dev/null
/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py >/dev/null
```

Generated private posture counts:

```text
node22 31 98.69
node24 38 98.42
unique remaining required fixtures: 40
```

The checked-in public evidence summaries moved one manifested fixture per lane:

```text
node22 documented_manifested_green_count: 2334 -> 2335
node24 documented_manifested_green_count: 2364 -> 2365
```

NDS verifier:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 13 passed, 21 failed
# Step 9 remains red because the generated posture is node22=31 / node24=38, not 0/0.
```
