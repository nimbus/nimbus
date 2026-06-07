# SEQ13 Performance Evidence

status: done

## Summary

`SEQ13` adds a focused redb performance regression smoke gate for the new MVCC
paths while preserving the existing benchmark reports as the broader provider
baseline. This proof is intentionally a budget guard for latest reads,
historical reads, historical pagination, CDC, PITR, retention compaction, and
bounded write amplification; it does not replace the existing provider
benchmark harnesses.

## Existing Benchmark Inputs

- `docs/plans/research/sqlite-storage-benchmark-report.md`
- `docs/plans/proof/storage-engine-quality-and-mvcc/seq0-embedded-point-read-baseline.md`
- `docs/plans/research/postgres-provider-benchmark-report.md`
- `docs/plans/research/mysql-provider-benchmark-report.md`
- `docs/plans/research/sqlite-replica-provider-benchmark-report.md`

The external-provider reports remain the baseline inputs for provider RTT and
throughput posture. Later SEQ3/SEQ4 closeout runs supplied Docker-backed live
MySQL/libSQL document/index evidence; SEQ13's focused redb budget remains a
regression smoke gate rather than a replacement for provider benchmark refresh.

## Implementation Anchor

- `crates/nimbus-storage/src/tests/crud_and_journal.rs`
  - `redb_storage_engine_quality_performance_budget_covers_latest_historical_cdc_pitr_and_gc`
  - `assert_seq13_budget`

The test seeds 64 documents, updates 16 of them, then measures latest point
reads, historical point reads, historical index pagination, CDC streaming, PITR
export/import, and retention compaction against explicit smoke budgets. It also
asserts bounded write amplification: document-version rows stay at no more than
one row per document write, and index-version rows stay within close/open rows
per indexed write.

## Current Budget Evidence

- `cargo test -p nimbus-storage redb_storage_engine_quality_performance_budget -- --nocapture`
  - result: `1 passed, 0 failed`
  - latest point reads: `1.283209ms <= 200ms`
  - historical point reads: `2.257625ms <= 300ms`
  - historical index pagination: `23.009417ms <= 500ms`
  - CDC stream: `10.979417ms <= 300ms`
  - PITR export/import: `264.958375ms <= 1s`
  - retention compaction: `1.386792ms <= 500ms`
  - output marker: `seq13 performance budget`
- `cargo check -p nimbus-storage`
  - result: passed

## Scope Note

The focused redb budget gate is a deterministic regression tripwire for the
new MVCC machinery on the embedded path. Provider-level performance remains
covered by the recorded benchmark reports and must be refreshed with the
provider harnesses when explicit Postgres/MySQL/libSQL fixture credentials are
available.
