# Storage Review Repairs

Status: `active` | Owner: this plan | Created: 2026-08-26
Baseline: main @ `b57a2d680891de852d5576e65ccaea787b005431`
Proof root: `proof/storage-review-repairs/`

Next action: run the final repository gates and fresh Nimbus and Opus reviews

## Outcome

> Nimbus verifies every materialized field, imports each nonzero-base restore as
> one recoverable state change, and uses proof gates that fail on missing backend
> or performance evidence.

## Architecture

Before:

```text
[journal snapshot]
  -> [position: schema + documents + schedules]
  -> [restore current state] -> commit
  -> [install checkpoint and floors] -> commit

[proof scripts] -> [aggregate source search or relative-only benchmark]
```

After:

```text
[journal snapshot]
  -> [position: schema + documents + schedules + bindings + trigger cursor]
  -> [validated restore transaction]
       {current state + MVCC anchors + tail + checkpoint + floors}

[proof scripts] -> [per-backend requirements + absolute measured limits]
                -> [negative mutation tests]
```

## Scope

- Owns: the five confirmed findings from the 2026-08-26 Opus 5 aggregate review.
- Owns: tests and small cleanup needed to make each repaired contract durable.
- Does not own: Band SA rows that already closed in the architecture review plan.
- Does not own: Blob Lifecycle Integrity scope or horizontal scaling authority.
- Non-goal: change any behavior based on the four rejected review claims.
- Non-goal: add a compatibility layer for pre-launch storage formats.

## Invariants

1. One canonical materialized identity covers all state rebuilt from the journal.
2. A successful nonzero-base import is restart-safe and immediately retryable.
3. A base checkpoint read returns the snapshot state through document and index MVCC paths.
4. A verifier claim about every backend requires evidence from every named backend.
5. A performance verdict requires measured candidate limits that do not depend on a slow baseline.
6. The three client mutation paths and storage atomicity rules remain unchanged.

## Findings ledger

| ID | Classification | Evidence | Owning task |
|---|---|---|---|
| F1 | confirmed | `CanonicalMaterializedState` omits `resource_path_bindings` and `trigger_delivery_cursor`. | SRR1 |
| F2 | confirmed | Embedded PITR import restores state before it installs the checkpoint and floors. | SRR2 |
| F3 | confirmed | A nonzero-base restore installs MVCC floors without snapshot anchors at the base sequence. | SRR2 |
| F4 | confirmed | The retention verifier uses one `rg` result as evidence for multiple backend paths. | SRR3 |
| F5 | confirmed | IMV7 accepts static and censored relative evidence without measured absolute candidate bounds. | SRR4 |
| F6 | rejected | Tokio `Notified` retains `notify_waiters` permits after future creation and before polling. | none |
| F7 | rejected | The retention controller runs on its dedicated two-thread background executor. | none |
| F8 | rejected | libSQL rejects gaps and forces a full snapshot after same-process retention. | none |
| F9 | rejected | Historical SMR proof transcripts match the verifier version at each proof commit. | none |

## Status ledger

| ID | Task | Status | Evidence |
|---|---|---|---|
| SRR0 | Pin and adjudicate the aggregate review baseline. | `done` | Opus 5 reviewed 169 files in three passes. Independent checks confirmed five findings and rejected four. See `proof/storage-review-repairs/srr0-baseline.md`. |
| SRR1 | Extend canonical materialized identity through full and incremental verification. | `done` | Commit `0a4ba6119`; storage 395 passed and 3 ignored; focused storage, engine, CLI, and server checks passed. See `proof/storage-review-repairs/srr1-materialized-identity.md`. |
| SRR2 | Make nonzero-base PITR import atomic and seed MVCC anchors. | `done` | Commit `1a553ac87`; 397 storage tests passed with 4 planned skips; strict all-feature Clippy passed. See `proof/storage-review-repairs/srr2-atomic-pitr-import.md`. |
| SRR3 | Require per-backend retention verifier evidence and test its failure behavior. | `done` | Commit `e83899824`; five helper groups and 18 omission mutations passed; the main verifier reports `18 passed, 0 failed`. See `proof/storage-review-repairs/srr3-retention-verifier.md`. |
| SRR4 | Replace the IMV7 relative-only gate with measured absolute limits and negative tests. | `done` | Commit `132343e37`; the mutation helper passes 6 checks and the complete IMV verifier passes all 16 conditions. See `proof/storage-review-repairs/srr4-imv-performance-gate.md`. |
| SRR5 | Run repository gates and a fresh Nimbus autoreview. | `in_progress` | |
| SRR9 | Clean up this plan after the repair pull request merges. | `todo` | Trigger: merge of the final repair pull request. |

## Tasks

### SRR0 Pin and adjudicate the baseline

- Problem: the aggregate review mixed valid findings with four false positives.
- Owning seam and paths: this plan and `proof/storage-review-repairs/`.
- Steps: record the reviewed range, direct-code adjudication, and accepted scope.
- Acceptance: every review candidate has a confirmed or rejected verdict with source evidence.
- Fail-before: Opus reported nine candidates without an orchestrator verdict.
- Verification: inspect the recorded source paths against baseline `b57a2d680`.

### SRR1 Extend canonical materialized identity

- Problem: resource bindings and the trigger cursor can drift without changing verification identity.
- Owning seam and paths: `crates/nimbus-storage/src/materialized_position.rs`, `materialized_verification.rs`, and engine verification.
- Steps:
  1. Add deterministic canonical encodings for resource bindings and the trigger cursor.
  2. Include both fields in full position calculation and incremental delta tracking.
  3. Update the pre-launch position codec version and dependent container diagnostics.
  4. Add drift and full-versus-incremental regression tests.
- Acceptance: either omitted field changes the position and incremental roots equal full recomputation.
- Fail-before: new binding-only and cursor-only drift tests fail against the baseline.
- Verification: run focused `nimbus-storage` and `nimbus-engine` materialized verification tests.

### SRR2 Make nonzero-base PITR import atomic

- Problem: embedded imports can expose restored state before the checkpoint and floors exist.
- Owning seam and paths: storage journal snapshot, retention, document versions, and index versions.
- Steps:
  1. Validate the complete archive before the first state mutation.
  2. Restore the snapshot, seed base-sequence document and index anchors, apply the tail, and install retention metadata in one backend transaction.
  3. Preserve the in-memory backend's equivalent one-state-change contract.
  4. Add injected-failure, retry, restart, document-history, and index-history tests.
- Acceptance: a failed import leaves the prior tenant state unchanged and can retry the same archive.
- Acceptance: reads at the imported base sequence return the checkpoint document and index state.
- Fail-before: failure-window and base-history regressions fail against the baseline.
- Verification: run focused PITR import tests for memory, redb, and SQLite.

### SRR3 Repair universal retention checks

- Problem: one matching backend can satisfy a verifier claim about all backends.
- Owning seam and paths: `scripts/verify-storage-metadata-retention.sh` and its test helper.
- Steps:
  1. Add a helper that requires a match in each named path.
  2. Replace every aggregate search that claims per-backend evidence.
  3. Add mutations that remove one backend's evidence and require verifier failure.
- Acceptance: each named backend omission makes the matching verifier condition fail.
- Fail-before: a fixture with one missing backend still passes the baseline helper.
- Verification: run the verifier test helper and the retention verifier at `18 passed, 0 failed`.

### SRR4 Repair the IMV7 performance gate

- Problem: the gate can accept a candidate from static estimates and censored relative comparisons.
- Owning seam and paths: `docs/private/plans/proof/incremental-materialized-verification/verify.sh` and IMV7 proof data.
- Steps:
  1. Require measured candidate status, nonempty samples, and true percentile calculation.
  2. Enforce ratified absolute latency and resident-memory limits for each decisive rung.
  3. Keep baseline comparisons as diagnostic evidence only.
  4. Add malformed, empty, censored-candidate, slow-candidate, and high-memory negative tests.
- Acceptance: each negative fixture fails without a traceback and the accepted proof data passes.
- Fail-before: at least one invalid fixture passes the baseline gate.
- Verification: run the IMV proof helper tests and the complete IMV verifier.

### SRR5 Run final gates and review

- Problem: the repair set crosses durable formats, recovery, and proof tooling.
- Owning seam and paths: the complete branch.
- Steps:
  1. Run format, strict Clippy, focused tests, proof verifiers, docs gates, and `make ci`.
  2. Commit the final branch state.
  3. Run `nimbus-autoreview --gate pre-pr --mode auto` and resolve every accepted finding.
  4. Prepare the pull request evidence and record any hosted-only checks.
- Acceptance: all local required gates pass and autoreview reports no accepted P0 through P2 finding.
- Fail-before: not applicable because this task aggregates earlier red tests.
- Verification: use the repository commands in `AGENTS.md` and the owning proof scripts.

### SRR9 Cleanup

- Problem: a merged plan must not remain an active control plane.
- Owning seam and paths: this plan, its proof root, and the plans index.
- Steps: confirm the final merge, then archive or delete the plan per repository convention.
- Acceptance: the plans index has no active entry for this completed work.
- Fail-before: not applicable because the merge triggers this task.
- Verification: search the repository for `storage-review-repairs-plan` and confirm the final routing.

## Goal

```text
Execute docs/private/plans/storage-review-repairs-plan.md to completion. This is
a whole-plan goal, not a single-task goal. Read the plan fully, then read
AGENTS.md, docs/private/operating/verification.md, the archived IMV and SMR
plans, and each task's owning source and tests. Work in
/Users/jack/src/github.com/nimbus/nimbus on branch
codex/storage-review-repairs. Chat history is not progress state. Resume from
the status ledger, the execution log, and git state. If compaction happens,
continue from the plan and git state rather than restarting. Loop: keep one
task in_progress, implement at the owning seam, capture fail-before evidence,
run the verification commands, commit the work per the commit policy, write
the proof file, append the execution log with the work commit, mark the task
terminal with evidence, commit the plan update the same way, then advance to
the next task. Decide rather than ask. Mark a wrong or already-satisfied task
no-action with a one-line reason. Record a blocker and continue with the next
eligible task. Binding constraints: preserve the six invariants and do not act
on F6 through F9. Commit policy: make one local work commit and one plan-state
commit for each completed task. Do not push or open a pull request without
owner approval. Stop only at a valid stop state from the plans skill. Before
you stop, update the ledger and the log, and record the next action in the
status line. The goal is met when SRR0 through SRR5 are terminal, every
confirmed finding has focused regression evidence, the required repository
gates pass, and SRR9 waits only for the final merge.
```

## Execution log

| Date | Item | Action | Evidence |
|---|---|---|---|
| 2026-08-26 | SRR0 | completed | Pinned `b57a2d680`; Opus 5 reviewed 169 files in three passes; direct checks confirmed F1 through F5 and rejected F6 through F9. No production behavior changed. |
| 2026-08-26 | SRR1 | started | Accepted canonical materialized identity as the first implementation task. |
| 2026-08-26 | SRR1 | completed | Commit `0a4ba6119` covers bindings and trigger progress in position v3 and verification root v2. Full storage tests passed 395 with three planned ignores. |
| 2026-08-26 | SRR2 | started | Accepted atomic nonzero-base PITR import and base-sequence MVCC anchors as one recovery contract. |
| 2026-08-26 | SRR2 | completed | Commit `1a553ac87` stages the embedded restore, anchors, tail, checkpoint, and floors behind one visibility boundary. Memory, redb, and SQLite retry after a staged fault; redb and SQLite preserve base history across restart. |
| 2026-08-26 | SRR3 | started | Accepted per-backend source evidence and mutation-tested verifier failures as the proof-tooling contract. |
| 2026-08-26 | SRR3 | completed | Commit `e83899824` replaces aggregate path searches with explicit per-path checks. Five helper groups prove that each of 18 individual omissions fails closed; the main verifier stays green at 18 of 18 conditions. |
| 2026-08-26 | SRR4 | started | Accepted absolute measured candidate limits, robust proof parsing, and malformed-proof negative tests as the IMV closeout contract. |
| 2026-08-26 | SRR4 | completed | Commit `132343e37` measures production candidate churn at 100,000 and 1,000,000 leaves. Five invalid-proof mutations fail cleanly, and the complete IMV verifier passes 16 of 16 conditions. |
| 2026-08-26 | SRR5 | started | Started complete repository gates and the required fresh Nimbus and Opus review of the integrated repair branch. |
