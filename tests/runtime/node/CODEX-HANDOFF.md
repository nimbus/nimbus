# Codex Handoff - NDS gate closeout state (PR #10)

Read this file once, then verify from current state before acting. This file is
tracked because agents are expected to read it first.

## Mission

The Nimbus node-default-runtime-support gate was to drive both required lanes to:

- `v8_isolate_required.gaps == 0`
- `v8_isolate_required.pass_rate_percent == 100`

That literal required-gap gate is now green after cycle 98.

## Current State

Worktree:

- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
- Branch: `codex/node-default-runtime-support-hardening`
- PR: #10, ready for review, not merged
- Current pushed head after the gate fix: `477af0e8d` (`Promote vm module import.meta fixture`)

Fork pins in Nimbus:

- nimbus/deno `v2.8.3-nimbus.46` (`d3f650c2fa`)
- nimbus/rusty_v8 `v149.4.0-nimbus.2` (`8f70a59`)

No local Cargo `paths = [...]` override is present in the committed branch.

Generated private posture at the cycle-98 checkpoint:

```text
node22 v8_isolate_required: gaps=0, pass_rate_percent=100.0, passed=2363, total=2363
node24 v8_isolate_required: gaps=0, pass_rate_percent=100.0, passed=2400, total=2400
```

Tracked blocker note:

- `tests/runtime/node/NDS-GATE-BLOCKER.md`

Cycle proof:

- `docs/plans/proof/node-default-runtime-support-hardening/nds3-cycle98-vm-module-import-meta-result.md`

## Latest Verification

Focused immutable-tag promotion guard:

```text
cargo test -p nimbus-runtime cycle98_vm_module_import_meta -- --nocapture
node_compat node22-supported-lane-executes-cycle98-vm-module-import-meta node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle98-vm-module-import-meta node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 915 filtered out
```

Verifier:

```text
bash scripts/verify-node-default-runtime-support-hardening.sh
[9] Node22/Node24 V8-isolate-required green
  PASS  Node22 and Node24 V8-isolate-required fixtures are 100%
```

The full verifier still reports unrelated private closeout/proof-corpus failures
in this checkout. Step 9 is the required-gap gate named by this burndown.

## What Changed In The Final Cycles

Cycle 97 resolved the Node24 top-level-await blocker by publishing:

- nimbus/rusty_v8 `v149.4.0-nimbus.2`
- nimbus/deno `v2.8.3-nimbus.45`

Cycle 98 resolved the last VM import-meta blocker by publishing:

- nimbus/deno `v2.8.3-nimbus.46`

`test/parallel/test-vm-module-import-meta.js` now dynamically passes in both
node22 and node24 required lanes through the committed promotion guard.

## Current PR Blocker

The fixture gate is green, but PR #10 cannot currently merge:

```text
mergeable=CONFLICTING
mergeStateStatus=DIRTY
statusCheckRollup=[]
```

Do not merge `origin/main` into this branch without explicit owner approval. The
base has a large history/scrub divergence, and this handoff intentionally leaves
base reconciliation to the owner.

## Honesty Contract

A fixture may leave the `v8_isolate_required` gap set only by:

1. A real dynamic green guard where the batch summary reports
   `passed>=1, skipped=0, failed=0`.
2. A source-confirmed structural reclassification where the fixture genuinely
   needs a host capability outside the multi-tenant isolate contract.

Never skip, weaken/delete assertions, edit upstream fixtures/checkers, or
hand-edit generated posture/classification JSON for a false green. A skip is
not a pass. The Rust test result is not enough by itself; use the fixture
summary line.

## Remaining Work

There are no remaining required-surface fixture gaps in node22 or node24.

If continuing toward merge, the next task is PR/base reconciliation and CI, not
NDS fixture burndown. Keep `measure_ah.sh` and the older scratch/census files
untracked; never use `git add -A` in this worktree.
