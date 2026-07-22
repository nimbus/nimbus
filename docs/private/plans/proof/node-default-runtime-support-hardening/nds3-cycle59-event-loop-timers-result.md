# NDS3 cycle 59 - event-loop timer throw/domain ordering

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

The following fixtures were dynamically promoted for node22 and node24:

- `test/parallel/test-timers-immediate-queue-throw.js`
- `test/parallel/test-timers-reset-process-domain-on-throw.js`

Gate movement:

- node22: 41 -> 39 gaps, 98.35% pass rate
- node24: 48 -> 46 gaps, 98.09% pass rate
- unique remaining required fixtures: 48

Deno fork tag: `v2.8.3-nimbus.13` (`a470e7d569`). Nimbus was repinned to the
published tag after local proof; the temporary local Deno path override was
removed before immutable-tag verification.

## Root Cause

`test-timers-immediate-queue-throw.js` exposed two interacting ordering gaps:

- Nimbus' bootstrap `setImmediate` compatibility wrapper caught callback errors
  and called `process._fatalException()` before the underlying Deno immediate
  wrapper observed the throw.
- Deno core drained `process.nextTick()` between immediate callbacks even after a
  setImmediate callback reported an uncaught exception. Node finishes the current
  immediate snapshot before running the nextTick scheduled by the throwing
  immediate.

The fork fix marks exceptions reported from immediate callbacks and suppresses
inter-immediate nextTick drains for the rest of that snapshot. Nimbus' wrapper
still preserves callback `this` binding to the returned immediate handle, but no
longer swallows the throw before Deno's immediate wrapper can report it.

`test-timers-reset-process-domain-on-throw.js` exposed a related timer/domain
sentinel gap:

- `domain.run()` correctly exits to `process.domain === undefined` for its
  synchronous error handler path.
- A later timer callback with no domain association should enter with Node's
  inactive-domain sentinel, `process.domain === null`.
- A first broad fix in `domain` async hooks regressed
  `test-domain-emit-error-handler-stack.js`; the final fix scopes the
  `undefined` -> `null` normalization to timer callback entry only.

The fork fix stays in `deno_core` / `deno_node` and does not touch
V8/rusty_v8.

## Verification

Local-fork focused probes before publishing the Deno tag:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-immediate-queue-throw.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-immediate-queue-throw.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-reset-process-domain-on-throw.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-reset-process-domain-on-throw.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
```

Local-fork regression guards:

```bash
cargo test -p nimbus-runtime --lib nds3_domain_fork_promoted_batch_fixture -- --nocapture
# node_compat node24-default-lane-executes-nds3-domain-fork-promoted-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
# node_compat node22-supported-lane-executes-nds3-domain-fork-promoted-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 849 filtered out; finished in 3.94s

cargo test -p nimbus-runtime --lib process_timers_promoted_batch_fixture -- --nocapture
# node_compat node22-supported-lane-executes-process-timers-promoted-batch node22 summary: selected=23, passed=23, skipped=0, failed=0
# node_compat node24-default-lane-executes-process-timers-promoted-batch node24 summary: selected=32, passed=32, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 849 filtered out; finished in 117.50s
```

Deno fork publish:

```bash
git add ext/node/polyfills/domain.ts ext/node/polyfills/internal/timers.mjs libs/core/01_core.js
git commit -m "node(timers): preserve domain and immediate throw ordering"
# [nimbus/v2.8.3 a470e7d569] node(timers): preserve domain and immediate throw ordering
#  3 files changed, 22 insertions(+), 5 deletions(-)

git tag v2.8.3-nimbus.13
git push origin nimbus/v2.8.3
# a7777aeece..a470e7d569  nimbus/v2.8.3 -> nimbus/v2.8.3

git push origin v2.8.3-nimbus.13
# * [new tag]               v2.8.3-nimbus.13 -> v2.8.3-nimbus.13
```

Immutable-tag focused probes after removing the local path override and
repinning Nimbus to `v2.8.3-nimbus.13`:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-immediate-queue-throw.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-immediate-queue-throw.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-reset-process-domain-on-throw.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-reset-process-domain-on-throw.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
```

Immutable-tag regression guards:

```bash
cargo test -p nimbus-runtime --lib nds3_domain_fork_promoted_batch_fixture -- --nocapture
# node_compat node24-default-lane-executes-nds3-domain-fork-promoted-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
# node_compat node22-supported-lane-executes-nds3-domain-fork-promoted-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 849 filtered out; finished in 3.84s

cargo test -p nimbus-runtime --lib process_timers_promoted_batch_fixture -- --nocapture
# node_compat node22-supported-lane-executes-process-timers-promoted-batch node22 summary: selected=23, passed=23, skipped=0, failed=0
# node_compat node24-default-lane-executes-process-timers-promoted-batch node24 summary: selected=32, passed=32, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 849 filtered out; finished in 121.35s
```

Real promotion guard:

```bash
cargo test -p nimbus-runtime --lib cycle59_event_loop_timers_batch -- --nocapture
# node_compat node24-default-lane-executes-cycle59-event-loop-timers-batch node24 summary: selected=2, passed=2, skipped=0, failed=0
# node_compat node22-supported-lane-executes-cycle59-event-loop-timers-batch node22 summary: selected=2, passed=2, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 850 filtered out; finished in 8.02s
```

Regenerated lightweight posture/evidence pipeline:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null
done
```

Generated counts:

```text
node22 39 98.35
node24 46 98.09
unique remaining required fixtures: 48
```

NDS verifier:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 13 passed, 21 failed
# Step 9 remains red because the generated posture is node22=39 / node24=46, not 0/0.
```

Safety checks:

```bash
grep -c '^paths =' .cargo/config.toml
# 0

git diff --check -- ext/node/polyfills/domain.ts ext/node/polyfills/internal/timers.mjs libs/core/01_core.js
# pass

cargo fmt --all --check
# pass

git diff --check
# pass
```
