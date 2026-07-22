# NDS3 Cycle 41: stream Writable same-callback TickObject isolation

Date: 2026-06-13

## Scope

- Fixture: `test/parallel/test-stream-writable-samecb-singletick.js`
- Lanes: node22, node24
- Owner: Nimbus harness
- Deno fork: unchanged, clean at `v2.8.3-nimbus.7`
- rusty_v8 fork: unchanged, pinned to `v149.4.0-nimbus.1`

## Root Cause

The fixture enables an `async_hooks` init hook and asserts that 100 synchronous
`Console(...).log()` calls allocate exactly one `TickObject`. The stream
implementation already scheduled exactly one Writable `afterWriteTick`, but the
Nimbus node-compat harness ran its invocation/drain `process.nextTick()` work
while the fixture hook was still enabled. Those harness-only TickObjects do not
exist in a direct `node test-stream-writable-samecb-singletick.js` run and were
counted as extra fixture resources.

Cycle 41 extends the existing async-hooks harness suppression path to this
fixture even though it does not live under `test/async-hooks/`, keeping harness
nextTick drains invisible while preserving fixture-owned TickObject init.

## Focused Proof

Published-tag focused rebuild:

```text
set -o pipefail; cargo test -p nimbus-runtime --lib nds_probe --no-run 2>&1 | grep -iE 'error\[|Finished|warning:|failed to'
Finished `test` profile [unoptimized + debuginfo] target(s) in 22.66s
```

Focused node24 probe:

```text
gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE="test/parallel/test-stream-writable-samecb-singletick.js" NIMBUS_RECENSUS_LANE=node24 NIMBUS_RECENSUS_EXTRA_DIRS="test/common" cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 833 filtered out; finished in 2.03s
```

Focused node22 probe:

```text
gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE="test/parallel/test-stream-writable-samecb-singletick.js" NIMBUS_RECENSUS_LANE=node22 NIMBUS_RECENSUS_EXTRA_DIRS="test/common" cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 833 filtered out; finished in 2.03s
```

Promoted non-ignored guard:

```text
gtimeout -s KILL 120 cargo test -p nimbus-runtime --lib cycle41_stream_writable_samecb -- --nocapture
node_compat node22-supported-lane-executes-cycle41-stream-writable-samecb-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle41-stream-writable-samecb-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 833 filtered out; finished in 3.93s
```

## Regeneration Checks

```text
cargo fmt --all --check
```

Passed with no output.

```text
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py --check
node default support posture: pass
/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py --check
node required-surface blocker inventory: pass
/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py validate
validated node-compat watchpoint catalog: 134 entries
```

Generated posture after regeneration:

```text
node22 58 97.56
node24 68 97.18
```

## Notes

- No Deno fork tag was published; temporary diagnostics in
  `/Users/jack/src/github.com/nimbus/deno` were removed, leaving the fork clean.
- The temporary `.cargo/config.toml` path override was removed before the final
  published-tag proof.
- The scratch `nds_probe` file/include was removed before promotion.
