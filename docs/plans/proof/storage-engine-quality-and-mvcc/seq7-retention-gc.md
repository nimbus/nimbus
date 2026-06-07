# SEQ7 Retention GC

status: done

## Scope

SEQ7 adds explicit retention compaction and garbage collection for the MVCC
document and index histories introduced by SEQ3 and SEQ4. The implementation
uses the existing storage `RetentionFloor` rather than introducing a second
pin registry. Compaction computes separate document, index, registry,
read-policy, CDC, PITR, materializer, replica, and transaction-session
watermarks from resource-specific participant routing. That keeps pruning
conservative where resources truly depend on the same pin while giving
operators precise active-pin counts and floors for the resource that is
actually holding history.

## Read-Before-Edit Checklist

- `docs/plans/storage-engine-quality-and-mvcc-plan.md`
- `crates/nimbus-storage/src/retention.rs`
- `crates/nimbus-storage/src/diagnostics.rs`
- `crates/nimbus-storage/src/store/document_versions.rs`
- `crates/nimbus-storage/src/store/index_versions.rs`
- `crates/nimbus-storage/src/sqlite/document_versions.rs`
- `crates/nimbus-storage/src/sqlite/index_versions.rs`
- `crates/nimbus-storage/src/postgres/document_versions.rs`
- `crates/nimbus-storage/src/postgres/index_versions.rs`
- `crates/nimbus-storage/src/mysql/document_versions.rs`
- `crates/nimbus-storage/src/mysql/index_versions.rs`
- `crates/nimbus-storage/src/libsql/document_versions.rs`
- `crates/nimbus-storage/src/libsql/index_versions.rs`
- `crates/nimbus-storage/src/tests/crud_and_journal.rs`
- `crates/nimbus-storage/src/tests/sqlite_foundation/journal.rs`

## Implementation Evidence

| Area | Evidence |
| --- | --- |
| Typed GC contract | `RetentionGcConfig`, `RetentionGcResource`, `RetentionGcWatermark`, `RetentionGcWatermarks`, and `RetentionGcSummary` define the configured history window, per-resource watermarks, active pin count, safe prune floor, and exact document/index prune counts. |
| Conservative enterprise default | `RetentionGcConfig::retain_all()` is the default diagnostic posture, so Nimbus does not prune history unless an operator or caller supplies an explicit positive history window. |
| Existing pin registry reused | `RetentionFloor::gc_watermarks(...)` derives every resource watermark from the existing retention pins and routes pins by participant/resource dependency. SQLite, Postgres, MySQL, and libSQL now expose the same `pin_retention_participant(...)` helper that redb already exposed. |
| Document anchor preservation | redb, SQLite, Postgres, MySQL, and libSQL document-version compaction delete rows below the safe floor except the latest anchor at or before the floor for each `(table_id, document_id)`. This preserves reads at the oldest retained sequence even when the visible state began before the floor. |
| Index interval pruning | redb, SQLite, Postgres, MySQL, and libSQL index-version compaction removes only closed intervals with `visible_until <= safe_prune_before`; open intervals and intervals visible at the floor remain. |
| Atomic backend maintenance | redb and SQLite compact inside a write transaction. Postgres, MySQL, and libSQL compact through their existing write transaction lifecycles, tenant locks, and rollback/commit behavior, and the maintenance transaction emits no logical mutation commit. |
| Diagnostics | `StorageHealthDiagnostic` now reports active `retention_pins` and computed `retention_gc` watermarks alongside the existing document-version counts and storage heads. |

## Verification Evidence

| Command | Result |
| --- | --- |
| `cargo test -p nimbus-storage retention_gc -- --nocapture` | Passed: `3 passed, 0 failed`. Covers redb and SQLite pin-blocked compaction, release-advanced compaction, document anchor preservation, closed index interval pruning, active pin diagnostics, resource-specific watermark routing, and exact prune summaries. |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo test -p nimbus-storage document_versions -- --nocapture` | Passed at SEQ7 checkpoint: `17 passed, 0 failed`, `275 filtered out`. Final Docker-backed live provider evidence was later added in SEQ3 closeout: `cargo test -p nimbus-storage document_versions -- --nocapture` passed `17 passed, 0 failed`. |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo test -p nimbus-storage index_versions -- --nocapture` | Passed at SEQ7 checkpoint: `12 passed, 0 failed`, `280 filtered out`. Final Docker-backed live provider evidence was later added in SEQ4 closeout: `cargo test -p nimbus-storage index_versions -- --nocapture` passed `12 passed, 0 failed`. |
| `cargo check -p nimbus-storage` | Passed. Confirms the runtime provider code, including Postgres/MySQL/libSQL compaction wrappers, compiles outside test-only cfg. |

## SEQ7 Closeout

SEQ7 is complete for the current implemented GC scope. Retention compaction now
has typed watermarks, exact summaries, active pin diagnostics, safe document
anchor preservation, closed index interval pruning, and backend parity across
redb, SQLite, Postgres, MySQL, and libSQL. Final live MySQL/libSQL provider
fixture evidence is complete through the later SEQ3/SEQ4 closeout runs.
