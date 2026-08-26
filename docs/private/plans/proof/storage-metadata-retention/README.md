# Storage Metadata Retention Proof

This directory holds the fail-before, implementation, provider, benchmark, and
closeout evidence for
[`storage-metadata-retention-plan.md`](../../storage-metadata-retention-plan.md).

Promotion baseline: `cc7ae36a3c21bf7aa093c013f3025d074c679438`.

At promotion, Nimbus already had resource-specific retention watermarks,
pin-aware MVCC compaction, anchor preservation, index-interval pruning,
trimmed-cursor checks, and retention diagnostics. The missing production
contract was checkpoint-backed commit-log pruning plus an Engine lifecycle that
invokes the existing and new maintenance safely. SMR0 captures the exact red
verifier and ratifies the bounded shipped profile before production behavior
changes.

Evidence:

- `smr0-baseline.md`: pinned fail-before inventory and bounded-profile
  calibration.
- `smr1-embedded-checkpoint.md`: provider-neutral checkpoint, embedded atomic
  compaction, nonzero-base PITR, tests, review, and routed follow-up evidence.
