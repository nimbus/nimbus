# NDS3 cycle 58 - timers immediate unref liveness

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

The following fixtures were dynamically promoted for node22 and node24:

- `test/parallel/test-timers-immediate-unref-simple.js`
- `test/parallel/test-timers-immediate-unref.js`
- `test/parallel/test-timers-immediate-unref-nested-once.js`

Gate movement:

- node22: 44 -> 41 gaps, 98.27% pass rate
- node24: 51 -> 48 gaps, 98.00% pass rate

Deno fork tag: `v2.8.3-nimbus.12` (`a7777aeece`). Nimbus was repinned to the
published tag after local proof; the temporary local Deno path override was
removed before immutable-tag verification.

## Root Cause

Deno core's event-loop check phase treated broad `dispatched_ops` as loop-
keeping work. That allowed unrefed immediates to run when only unrefed or
otherwise non-liveness work existed, so fixtures such as
`setImmediate(common.mustNotCall()).unref()` fired instead of letting the event
loop settle.

The fork fix stays in `deno_core` and does not touch V8/rusty_v8:

- changes `dispatch_event_loop_tick()` to return both `dispatched_ops` and
  `completed_refed_ops`
- computes `completed_refed_ops` by removing completed promise ids from
  `context_state.unrefed_ops`; completions found there do not keep the loop
  alive
- gates unrefed immediate execution on
  `has_refed || did_work || completed_refed_ops || uv_did_io`

`test/parallel/test-timers-immediate-queue-throw.js` was intentionally not
promoted in this cycle; it still fails with `AssertionError: 0 !== 1` and needs
a separate queue-throw semantics fix.

## Verification

Local-fork focused probes before publishing the Deno tag:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-immediate-unref-simple.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-immediate-unref.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-immediate-unref-nested-once.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-immediate-unref-simple.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-immediate-unref.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-immediate-unref-nested-once.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
```

The adjacent queue-throw fixture remained red during local probing:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-immediate-queue-throw.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
# AssertionError [ERR_ASSERTION]: Expected values to be strictly equal: 0 !== 1
```

Deno fork checks:

```bash
rustfmt --check libs/core/runtime/jsruntime.rs
# pass

cargo fmt --all --check
# failed on pre-existing formatting drift in libs/core/lib.rs and
# libs/core/runtime/bindings.rs; the touched file passed rustfmt.
```

Immutable-tag focused probes after repinning to `v2.8.3-nimbus.12`:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-immediate-unref-simple.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-immediate-unref.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-immediate-unref-nested-once.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-immediate-unref-simple.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-immediate-unref.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-timers-immediate-unref-nested-once.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
```

Real promotion guard:

```bash
cargo test -p nimbus-runtime --lib cycle58_timers_immediate_unref_batch -- --nocapture
# node_compat node22-supported-lane-executes-cycle58-timers-immediate-unref-batch node22 summary: selected=3, passed=3, skipped=0, failed=0
# node_compat node24-default-lane-executes-cycle58-timers-immediate-unref-batch node24 summary: selected=3, passed=3, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 848 filtered out; finished in 11.99s
```

Focused regression guard on the published tag:

```bash
cargo test -p nimbus-runtime --lib process_timers_promoted_batch_fixture -- --nocapture
# node22 process_timers_promoted_batch summary: selected=23, passed=23, skipped=0, failed=0
# node24 process_timers_promoted_batch summary: selected=32, passed=32, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 848 filtered out; finished in 123.80s
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
node22 41 98.27
node24 48 98.00
unique remaining required fixtures: 50
```

The verifier-owned private posture artifact reports the same gate values:

```bash
/opt/homebrew/bin/python3.12 - <<'PY'
import json
d = json.load(open("docs/private/architecture/runtime/node-default-support-posture.json"))
for lane in ("node22", "node24"):
    m = d["lanes"][lane]["v8_isolate_required"]
    print(lane, m["gaps"], m["pass_rate_percent"])
PY
# node22 41 98.27
# node24 48 98.0
```

Checks:

```bash
cargo fmt --all --check
# pass

git diff --check
# pass

/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 134 entries

rg -n 'paths = \["/Users/jack/src/github.com/nimbus/deno|v2\.8\.3-nimbus\.11|git\+file' .cargo/config.toml Cargo.toml Cargo.lock
# no matches

bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 13 passed, 21 failed
# Step 9 remains red because the generated posture is node22=41 / node24=48, not 0/0.
```
