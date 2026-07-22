# NDS3 cycle 63 - VM module afterEvaluate microtask queue

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

The following fixture was dynamically promoted for node22 and node24:

- `test/parallel/test-vm-module-after-evaluate.js`

Gate movement from the generated private posture:

- node22: 33 -> 32 gaps, 98.65% pass rate
- node24: 40 -> 39 gaps, 98.38% pass rate
- unique remaining required fixtures: 41

No new fork tag was needed for this cycle. The fixture was already fixed by the
cycle62 Deno fork tag `v2.8.3-nimbus.16` (`ee32c71874`), which preserved VM
`microtaskMode: "afterEvaluate"` queue isolation while keeping normal domain/VM
Promise propagation.

## Root Cause

`test-vm-module-after-evaluate.js` exercises the same VM afterEvaluate
microtask-queue boundary as cycle62, but through `vm.SourceTextModule`
evaluation rather than `vm.Script`. Once the Deno fork stopped patching
afterEvaluate VM contexts with the domain Promise wrapper, module evaluation
kept its Promise continuations on the expected VM-owned microtask queue.

The promotion is a no-fork follow-up to cycle62. No V8/rusty_v8 changes were
made.

## Verification

Focused probes on the already-pinned `v2.8.3-nimbus.16` tag:

```bash
/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle63-vm-module-after-evaluate \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-vm-module-after-evaluate.js" \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 858 filtered out; finished in 2.00s

/opt/homebrew/bin/gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nimbus-nds-cycle63-vm-module-after-evaluate \
  NIMBUS_RECENSUS_FIXTURE="test/parallel/test-vm-module-after-evaluate.js" \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_EXTRA_DIRS="test/common" \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
# node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 858 filtered out; finished in 1.89s
```

Real promotion guard:

```bash
/opt/homebrew/bin/gtimeout -s KILL 120 \
  cargo test -p nimbus-runtime --lib cycle63_vm_module_after_evaluate_batch -- --nocapture
# node_compat node22-supported-lane-executes-cycle63-vm-module-after-evaluate-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
# node_compat node24-default-lane-executes-cycle63-vm-module-after-evaluate-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
# test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 858 filtered out; finished in 3.84s
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
node22 32 98.65
node24 39 98.38
unique remaining required fixtures: 41
```

The checked-in public evidence summaries moved one manifested fixture per lane:

```text
node22 documented_manifested_green_count: 2333 -> 2334
node24 documented_manifested_green_count: 2363 -> 2364
total documented green fixtures: 6622 -> 6624
```

NDS verifier:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 13 passed, 21 failed
# Step 9 remains red because the generated posture is node22=32 / node24=39, not 0/0.
```
