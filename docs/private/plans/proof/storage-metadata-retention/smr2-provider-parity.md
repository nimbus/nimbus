# SMR2 Provider Parity Proof

Date: 2026-08-26.
Implementation commit: `e990cc7c8`.
Pull request: #317.
Merge commit: `f97b2db67fcbea5c6b6c3dee395c361c210d1c75`.

## Outcome

SMR2 implements the retained-history contract on PostgreSQL, MySQL, and
libSQL. Each provider persists the materialized checkpoint and exact physical
floor, validates the committer owner, epoch, and durable sequence in the same
transaction, rechecks the prepared checkpoint state, then prunes document
versions, index versions, and the journal prefix before it publishes the new
checkpoint and floor.

The provider transaction keeps `latest_sequence` and `applied_sequence`
independent from the physical minimum journal sequence. A stale Engine
generation receives `CommitterLeaseError::Fenced` before any delete. A
checkpoint that raced another compactor or exceeds the applied head fails
closed.

libSQL treats the remote primary as authority. Retention publication marks the
replica cache for a full snapshot refresh, so a local cache never tries to
incrementally replay a prefix that the remote primary deleted. The refreshed
snapshot carries the checkpoint, exact floor, materialized state, and retained
journal tail.

## Contract Evidence

- `SqlWriteTransactionCore` owns the transaction-local retention operations:
  metadata load, applied-head load, MVCC pruning, journal deletion, checkpoint
  publication, and fault injection.
- `fenced_compact_retained_history` prepares the candidate outside the write
  transaction, then validates the lease, durable sequence, prior checkpoint
  blob, prior physical floor, and applied head inside the transaction before
  any delete.
- PostgreSQL and libSQL store checkpoint and floor as metadata blobs. MySQL's
  pre-launch metadata table now supports either scalar or blob values; its
  retention keys reject a missing blob as corruption.
- The floor uses the same big-endian `u64` encoding on all three providers.
- Provider-specific SQL errors pass through the normal driver classifier.
  PostgreSQL and MySQL trigger failures remain `Storage(Other)`; libSQL's
  Hrana transport maps the remote SQLite constraint to
  `Storage(Unavailable)`. None becomes retention expiry, fencing, or
  corruption.
- The storage writer ownership matrix declares checkpoint publication,
  journal-prefix deletion, MVCC pruning, lease fencing, and materialized-root
  neutrality. Its mutation test fails if any effect is omitted.

## Verification

| Command or gate | Result |
| --- | --- |
| Focused live PostgreSQL retention lane | 3 passed. Covered stale lease, exact restart floor, injected pre-commit rollback, real SQL rollback, MVCC pruning, and retry. |
| Focused live MySQL retention lane | 3 passed with the same contract. |
| Focused live libSQL retention lane | 3 passed. Also proved a checkpoint-compatible full cache rebuild after remote pruning. |
| Full live PostgreSQL provider lane before final test strengthening | 82 passed. |
| Full live MySQL provider lane before final test strengthening | 52 passed. |
| Full live libSQL provider lane before final test strengthening | 56 passed and 1 pre-existing scheduler-probe failure. The failing case reproduced unchanged on clean `main` at `748d9630f`; all retention cases passed. |
| `cargo test -p nimbus-storage --all-features` with implicit external fixtures disabled | 539 passed, 3 ignored, and one timing-budget outlier. The PITR budget measured 1.047 seconds against 1 second during the full run; the immediate isolated rerun passed at 0.390 seconds. |
| Storage ownership effect gate | 4 passed, including the omission mutation test. |
| `cargo clippy -p nimbus-storage --all-targets --no-default-features` | Passed. Vendored dependency warnings only. |
| `cargo clippy -p nimbus-storage --all-targets --all-features` | Passed. Vendored dependency warnings only. |
| `cargo fmt --all --check` and `git diff --check` | Passed. |
| `bash scripts/verify-storage-metadata-retention.sh` | Expected intermediate state: `Summary: 13 passed, 5 failed`. The provider-retention condition is green; SMR3 through SMR5 own the five remaining conditions. |
| Nimbus autoreview, `pre-pr` | Clean. No accepted or actionable findings. |
| Pull request #317 | Merged as `f97b2db67`. |

## Remaining Boundary

SMR2 exposes the safe provider operation but does not call it from production.
SMR3 owns the bounded profile, single-flight Engine lifecycle, cancellation,
retry, diagnostics, metrics, and operator-triggered execution. SMR4 owns
post-page floor validation for readers that can race this new physical prune.
