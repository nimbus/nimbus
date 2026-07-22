# NDS3 cycle 65 - domain uncaughtException stack clearing

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

The following fixture was dynamically promoted for node22 and node24:

- `test/parallel/test-domain-stack-empty-in-process-uncaughtexception.js`

Gate movement from the generated private posture:

- node22: 31 -> 30 gaps, 98.73% pass rate
- node24: 38 -> 37 gaps, 98.46% pass rate
- unique remaining required fixtures: 39

The fix landed in the Deno fork as tag `v2.8.3-nimbus.18`
(`053abe6e3a`, `node(domain): clear stack before uncaughtException listeners`).
Nimbus was repinned from `v2.8.3-nimbus.17` to `v2.8.3-nimbus.18` and the
fixture was re-proven from the published tag. No V8/rusty_v8 changes were made.

## Root Cause

`test-domain-stack-empty-in-process-uncaughtexception.js` throws from inside
`domain.create().run(...)` and asserts that the process-level
`uncaughtException` listener observes an empty domain stack. Node's domain module
prepends an internal uncaught-exception listener that clears the domain stack and
sets `process.domain` to `null` before user listeners run.

Deno's `ext/node/polyfills/domain.ts` did not install that clear listener, so the
fixture either escaped as a raw top-level error or reached the user listener with
the wrong `process.domain` state. The Deno fork now lazily installs the internal
clear listener after `process` exists, keeps it prepended ahead of user
`uncaughtException` listeners, and removes it when it is the only listener left.

The Nimbus harness also needed to run this official fixture through the
CommonJS-main `Module._load(..., isMain=true)` path, matching the existing cycle36
uncaught-exception stack fixture, so Deno's main-module fatal-exception path can
call `process._fatalException(err)`. The fixture additionally uses a single
`beforeExit` `mustCall`, so it was added to the lifecycle-drain postlude table.

## Verification

Baseline published-tag probe before the cycle65 changes:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle65-domain-stack-baseline-node24 \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-domain-stack-empty-in-process-uncaughtexception.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
# runtime JavaScript error: Error: boom
```

Intermediate proof while developing the fix showed the layers separately:

- after the CommonJS-main harness path, the fixture reached the
  `uncaughtException` handler but failed because `process.domain` was `undefined`
  instead of Node's `null`;
- after the Deno domain clear listener, the fixture reached the final lifecycle
  assertion but failed because the harness did not yet emit the single
  `beforeExit` required by the fixture's `mustCall`.

Local probes on the temporary Cargo path override to
`/Users/jack/src/github.com/nimbus/deno/ext/node` after all three pieces:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle65-domain-stack-local-v2-node24 \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-domain-stack-empty-in-process-uncaughtexception.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 863 filtered out; finished in 1.97s

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle65-domain-stack-local-node22 \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-domain-stack-empty-in-process-uncaughtexception.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 863 filtered out; finished in 1.90s
```

Immutable tag probes after publishing `v2.8.3-nimbus.18`, removing the local
Cargo path override, and running `cargo clean -p deno_node`:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle65-domain-stack-tag-node24 \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-domain-stack-empty-in-process-uncaughtexception.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 863 filtered out; finished in 2.04s

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle65-domain-stack-tag-node22 \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-domain-stack-empty-in-process-uncaughtexception.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 863 filtered out; finished in 1.94s
```

Real promotion guards on the published tag:

```bash
cargo test -p nimbus-runtime --lib node24_default_lane_executes_cycle65_domain_stack_batch -- --nocapture
# node_compat node24-default-lane-executes-cycle65-domain-stack-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 864 filtered out; finished in 2.00s

cargo test -p nimbus-runtime --lib node22_supported_lane_executes_cycle65_domain_stack_batch -- --nocapture
# node_compat node22-supported-lane-executes-cycle65-domain-stack-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 864 filtered out; finished in 1.91s
```

Focused domain regression subset on the published tag:

```bash
/opt/homebrew/bin/gtimeout -s KILL 240 cargo test -p nimbus-runtime --lib domain -- --nocapture --test-threads=1
# node20 cycle16 domain summary: selected=1, passed=1, skipped=0, failed=0
# node22 cycle16/cycle60/cycle64/cycle65/loader-context/nds3-domain summaries: all skipped=0, failed=0
# node24 cycle16/cycle60/cycle64/cycle65/loader-context/nds3-domain summaries: all skipped=0, failed=0
# node26 cycle16 domain summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 19 passed; 0 failed; 2 ignored; 0 measured; 844 filtered out; finished in 173.65s
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
node22 30 98.73
node24 37 98.46
unique remaining required fixtures: 39
```

NDS verifier:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 13 passed, 21 failed
# Step 9 remains red because the generated posture is node22=30 / node24=37, not 0/0.
```
