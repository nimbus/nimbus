status: done
date: 2026-05-27
phase: CST9

# CST9 Closeout

## Result

Convex-Informed Storage Trust Hardening is complete.

Nimbus now has:

- Stable logical table identity through `TableId` plus a backend-owned physical
  layout.
- Explicit `TableState` lifecycle for active, hidden, and deleting identities.
- Convex-compatible table-aware document ID validation without forcing that ID
  shape onto every adapter.
- Read dependencies, subscription invalidation, and durable mutation
  intersection keyed by stable table identity.
- Stable `IndexId` plus `IndexState` lifecycle for maintained/queryable index
  behavior.
- An explicit history posture: `intentionally_latest_row`, backed by the
  durable logical commit log, materialized snapshot plus journal-tail replay,
  and pinned transaction-session reads.
- Read-only table identity diagnostics instead of public mutable catalog
  constructors.
- Cross-backend conformance evidence for redb, SQLite, Postgres, MySQL, and
  libSQL.

## Final Verification

- `cargo fmt --all --check` passed.
- `cargo check -p nimbus-storage --all-targets` passed.
- `cargo check -p nimbus-core -p nimbus-storage -p nimbus-engine -p nimbus-server --all-targets` passed.
- `cargo test -p nimbus-core --lib`: 95 passed, 0 failed.
- `cargo test -p nimbus-core schema --lib`: 11 passed, 0 failed.
- `cargo test -p nimbus-core dependency --lib`: 9 passed, 0 failed.
- `cargo test -p nimbus-storage index --lib`: 25 passed, 0 failed.
- `cargo test -p nimbus-storage materialized_snapshot --lib`: 5 passed, 0 failed.
- `cargo test -p nimbus-storage durable_journal_recovery --lib`: 3 passed, 0 failed.
- `cargo test -p nimbus-storage execution_unit_batch_persists --lib`: 3 passed, 0 failed.
- `cargo test -p nimbus-storage table_identity_diagnostics --lib`: 2 passed, 0 failed.
- `cargo test -p nimbus-storage table_lifecycle --lib`: 5 passed, 0 failed.
- `cargo test -p nimbus-storage mysql_schema_write --lib`: 1 passed, 0 failed.
- `cargo test -p nimbus-storage canceled_async_write_after_commit_still_reports_committed --lib`: 1 passed, 0 failed.
- `cargo test -p nimbus-storage --lib`: 222 passed, 0 failed, 2 ignored.
- `cargo test -p nimbus-server read_tracking --lib`: 5 passed, 0 failed.
- `cargo test -p nimbus-engine transaction_session_point_reads_stay_on_the_begin_snapshot --lib`: 1 passed, 0 failed.
- `npm run typecheck` passed; TanStack route generation emitted existing non-route warnings and exited 0.

Final aggregate verifier:

```text
bash scripts/verify-convex-storage-trust-hardening.sh
Summary: 10 passed, 0 failed
```

## Debt And Docs

- `docs/technical-debt.md` has the CST-owned rows closed or routed.
- `docs/architecture/storage/table-identity.md` is the canonical identity,
  lifecycle, diagnostics, and physical-layout reference.
- `docs/architecture/storage/persistence-engine-baseline.md` records the
  chosen `intentionally_latest_row` history posture.
- `docs/plans/archive/convex-storage-trust-hardening-plan.md` records CST0-CST9
  completion and proof links.
