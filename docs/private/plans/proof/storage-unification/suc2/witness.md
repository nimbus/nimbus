# SUC2.1 — Single Transcription Of The Queued Durable-Commit Sequence

## What Changed

`crates/nimbus-engine/src/engine/mutations/durable_batch.rs` now owns the one
transcription of the queued commit sequence: arm write-log guard → fenced
provider persist (fallback durable append) → durable-head advance → disarm →
`on_durable` → `JournalDurableAppendBeforeApply` fault (fallback route) →
`DURABLE_BEFORE_PUBLISH` window → apply-or-recover → publish frontier →
cache invalidation → applied-head watermark. The ordered publisher
(`publisher.rs::persist_assigned_batch_once`) and the serial reference arm
(`journal.rs::process_serial_queued_mutation_batch`) both delegate to
`persist_and_apply_assigned_batch`; neither carries its own copy any more.
Transcription count = 1 (compiler-linked).

Callers keep route-specific concerns: sequence validation (already one shared
function), error classification, response plumbing, fan-out, metrics.

## Drift Fixed (canonical semantics = ordered publisher)

1. The serial arm skipped the `DURABLE_BEFORE_PUBLISH` fault window when a
   fenced provider applied the batch; the window now applies on every route.
2. The serial arm discarded the apply error on recovery; the recovery-failure
   message now preserves both the apply and recovery error contexts.

## Regression Caught During Verification

First cut moved the serial arm's deferred durability acknowledgements into the
core's `on_durable` closure unconditionally, so a persistence failure returned
`deferred: Vec::new()` and same-batch dependents received the raw append error
instead of a retryable-conflict rewrite.
`serial_discard_rewrites_same_batch_conflict_before_retry` failed (RED);
fixed by handing the acknowledgements to `on_durable` through an `Option` the
failure path reclaims. Both discard-rewrite tests pass (GREEN).

## Decision U5 — Provider-Side Witness Rides With SUC3

The CommitTransaction witness half of SUC2.1 (a type threading
document/version/index/journal/watermark effects so a provider omitting one
cannot compile) requires touching all four providers' apply paths — the exact
triplicated code SUC3.1 deletes. The facade's single apply function, taking
every effect as a required argument, *is* that witness. Building a separate
witness first would be written against code scheduled for deletion (same
rationale as U4). SUC2.1 therefore ships the engine transcription unification;
the witness lands as the facade signature in SUC3.1.

## Verification

- `cargo nextest run -p nimbus-engine` (fixtures opt-out): 655 passed, 5 skipped
  (includes journal/publisher/direct/execution-unit/fan-out and both
  kill-switch discard-rewrite equivalence tests)
- `cargo nextest run -p nimbus-storage` (fixtures opt-out): 435 passed, 2 skipped
- `cargo clippy -p nimbus-engine -p nimbus-storage --all-targets -- -D warnings`: clean
- `cargo fmt --all --check`: clean
- All runs under `set -o pipefail`; the first (non-pipefail) battery masked the
  RED discard-rewrite test — rerun confirmed and fixed before commit.
