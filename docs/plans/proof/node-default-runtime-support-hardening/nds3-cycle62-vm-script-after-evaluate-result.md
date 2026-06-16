# NDS3 cycle 62 - VM script afterEvaluate microtask queue

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

The following fixture was dynamically promoted for node22 and node24:

- `test/parallel/test-vm-script-after-evaluate.js`

Gate movement from the generated private posture:

- node22: 34 -> 33 gaps, 98.61% pass rate
- node24: 41 -> 40 gaps, 98.34% pass rate
- unique remaining required fixtures: 42

Deno fork tag: `v2.8.3-nimbus.16` (`ee32c71874`). Nimbus was repinned to the
published tag after local proof; the temporary local Deno path override was
removed before immutable-tag verification.

## Root Cause

`test-vm-script-after-evaluate.js` creates VM contexts with
`microtaskMode: "afterEvaluate"` and expects Promises created inside that VM to
use the context's separate microtask queue. The Deno Node domain polyfill patched
every VM context's `Promise.prototype.then` at context creation time. That patch
installs an outer-realm wrapper so domain propagation can work for ordinary VM
Promises, but in an `afterEvaluate` context it crosses the queue boundary and
lets Promise continuations run when Node expects them to remain isolated.

The fork fix keeps the existing domain Promise patch for normal VM contexts, but
skips `patchDomainPromiseContext(...)` when the VM context was created with an
`afterEvaluate` microtask queue. The same fork commit also aligns thrown
`domain.bind()` / thrown `domain.intercept()` callback errors with Node's source:
thrown callback errors are domain-thrown errors and should not be marked as
`domainBound`.

The fix stays in `deno_node` TypeScript/JavaScript polyfills:

- `ext/node/polyfills/vm.js`
- `ext/node/polyfills/domain.ts`

No V8/rusty_v8 changes were made.

## Verification

Local-fork focused proof before publishing the Deno tag:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle62-vm-script-after-evaluate-local \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-vm-script-after-evaluate.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 856 filtered out; finished in 1.98s

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle62-vm-script-after-evaluate-local \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-vm-script-after-evaluate.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 856 filtered out; finished in 1.89s
```

Domain/VM regression proof for the queue-boundary fix:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle62-vm-script-after-evaluate-local \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-domain-vm-promise-isolation.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 856 filtered out; finished in 2.26s
```

Domain regression guards:

```bash
/opt/homebrew/bin/gtimeout -s KILL 150 \
  cargo test -p nimbus-runtime --lib node24_node_tools_domain_foundation_batch_fixture -- --nocapture
# node_compat node24-node-tools-domain-foundation-batch node24 -> test/parallel/test-domain-bind-timeout.js
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 856 filtered out; finished in 30.20s

/opt/homebrew/bin/gtimeout -s KILL 150 \
  cargo test -p nimbus-runtime --lib node22_node_tools_domain_foundation_batch_fixture -- --nocapture
# node_compat node22-node-tools-domain-foundation-batch node22 -> test/parallel/test-domain-bind-timeout.js
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 856 filtered out; finished in 30.46s

/opt/homebrew/bin/gtimeout -s KILL 120 \
  cargo test -p nimbus-runtime --lib node22_node_tools_domain_promise_watchpoint -- --nocapture
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 856 filtered out; finished in 1.99s

/opt/homebrew/bin/gtimeout -s KILL 120 \
  cargo test -p nimbus-runtime --lib nds3_domain_fork_promoted_batch_fixture -- --nocapture
# node_compat node22-supported-lane-executes-nds3-domain-fork-promoted-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
# node_compat node24-default-lane-executes-nds3-domain-fork-promoted-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 855 filtered out; finished in 3.90s

/opt/homebrew/bin/gtimeout -s KILL 120 \
  cargo test -p nimbus-runtime --lib cycle60_domain_capture_after_load_batch -- --nocapture
# node_compat node22-supported-lane-executes-cycle60-domain-capture-after-load-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
# node_compat node24-default-lane-executes-cycle60-domain-capture-after-load-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 855 filtered out; finished in 3.89s
```

Deno fork publish:

```bash
git add ext/node/polyfills/domain.ts ext/node/polyfills/vm.js
git commit -m "node(vm): preserve afterEvaluate microtask queues"
# [nimbus/v2.8.3 ee32c71874] node(vm): preserve afterEvaluate microtask queues
#  2 files changed, 8 insertions(+), 6 deletions(-)

git tag v2.8.3-nimbus.16
git push origin HEAD
# f31e3cdd80..ee32c71874  HEAD -> nimbus/v2.8.3

git push origin v2.8.3-nimbus.16
# * [new tag]               v2.8.3-nimbus.16 -> v2.8.3-nimbus.16
```

Immutable-tag preparation:

```bash
cargo update -p deno_node
# deno_node v0.189.0: v2.8.3-nimbus.15#f31e3cdd -> v2.8.3-nimbus.16#ee32c718

cargo clean -p deno_node
# Removed 541 files, 361.1MiB total
```

Immutable-tag focused probes after removing the local path override and
repinning Nimbus to `v2.8.3-nimbus.16`:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle62-vm-script-after-evaluate-tag \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-vm-script-after-evaluate.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 856 filtered out; finished in 2.07s

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle62-vm-script-after-evaluate-tag \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-vm-script-after-evaluate.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 856 filtered out; finished in 1.95s
```

Immutable-tag regression checks:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle62-vm-script-after-evaluate-tag \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-domain-vm-promise-isolation.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 856 filtered out; finished in 2.30s

/opt/homebrew/bin/gtimeout -s KILL 150 \
  cargo test -p nimbus-runtime --lib node24_node_tools_domain_foundation_batch_fixture -- --nocapture
# node_compat node24-node-tools-domain-foundation-batch node24 -> test/parallel/test-domain-bind-timeout.js
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 856 filtered out; finished in 30.84s
```

Real promotion guard:

```bash
/opt/homebrew/bin/gtimeout -s KILL 120 \
  cargo test -p nimbus-runtime --lib cycle62_vm_script_after_evaluate_batch -- --nocapture
# node_compat node24-default-lane-executes-cycle62-vm-script-after-evaluate-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
# node_compat node22-supported-lane-executes-cycle62-vm-script-after-evaluate-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 856 filtered out; finished in 3.91s
```

Regenerated lightweight posture/evidence pipeline:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null
done
```

Generated private posture counts:

```text
node22 33 98.61
node24 40 98.34
unique remaining required fixtures: 42
```

The checked-in public evidence summaries moved one manifested fixture per lane:

```text
node22 documented_manifested_green_count: 2332 -> 2333
node24 documented_manifested_green_count: 2362 -> 2363
total documented green fixtures: 6620 -> 6622
```

NDS verifier:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 13 passed, 21 failed
# Step 9 remains red because the generated posture is node22=33 / node24=40, not 0/0.
```
