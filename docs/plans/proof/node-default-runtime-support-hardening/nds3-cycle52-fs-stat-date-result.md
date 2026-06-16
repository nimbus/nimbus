# NDS3 cycle 52 - fs stat date timestamp parity

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

`test/parallel/test-fs-stat-date.mjs` was dynamically promoted for node22 and
node24.

Gate movement:

- node22: 50 -> 49 gaps, 97.93% pass rate
- node24: 58 -> 57 gaps, 97.63% pass rate

No Deno fork tag was needed. A temporary local Deno path override was used during
diagnosis, then removed; the final proof rebuilt and ran against immutable
`v2.8.3-nimbus.8`.

## Root Cause

The fixture calls `fsPromises.utimes()` with pre-epoch `Date` values, then
asserts `fs.stat()` and `fs.statSync()` preserve the negative millisecond
timestamps.

Stock Node on this host preserves those values:

```text
atimeMs: -40691
mtimeMs: -355
```

Nimbus' runtime-local filesystem bridge had two gaps:

- `runtimeFsToUnixTimeFromEpoch()` normalized already-validated negative numeric
  seconds through Node's `toUnixTimestamp()` helper, whose Node-facing semantics
  map negative numeric inputs to the current time.
- `system_time_from_unix_parts()` rejected negative seconds, and
  `system_time_to_unix_millis()` dropped pre-epoch metadata.

The fix keeps Node's public validation path intact while allowing the
runtime-local Deno.utime bridge to accept normalized negative numeric seconds and
round-trip pre-epoch `SystemTime` metadata.

## Verification

Focused census before the fix:

```text
node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
AssertionError [ERR_ASSERTION]: expected -40691 +/- 1.0000000000000002, got 1781358360084
```

Immutable-tag focused probes after the fix:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-fs-stat-date.mjs" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-fs-stat-date.mjs" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
```

Real promotion guard:

```bash
cargo test -p nimbus-runtime --lib cycle52_fs_stat_date -- --nocapture
# node_compat node22-supported-lane-executes-cycle52-fs-stat-date-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
# node_compat node24-default-lane-executes-cycle52-fs-stat-date-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 836 filtered out
```

Regenerated lightweight posture/evidence pipeline:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null
done
```

Checks:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py --check
# node default support posture: pass

/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py --check
# node required-surface blocker inventory: pass

/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 134 entries
```

Generated counts:

```text
node22 49 97.93
node24 57 97.63
```
