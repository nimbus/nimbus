# SEQ8 PITR Export Import

status: done

## Scope

SEQ8 adds a typed point-in-time restore archive on top of the durable tenant
journal and the MVCC retention floors from SEQ7. The archive supports
restore-to-sequence and restore-to-timestamp targets, validates retention
eligibility, stores current storage-format markers, and carries a canonical
logical snapshot fingerprint so imports prove that replay produced the exact
historical target state.

The implementation intentionally restores through the existing durable-journal
replay path. redb and SQLite use the materialized snapshot rebuild helper
directly. Postgres, MySQL, and libSQL expose the same export/import API in
production code and import by appending the archive tail into an empty tenant
then running normal durable recovery. This keeps document writes, indexes,
schema/table identity events, scheduled execution metadata, and version rows on
the already-tested recovery path instead of adding backend-specific restore
shortcuts.

## Read-Before-Edit Checklist

- `docs/plans/storage-engine-quality-and-mvcc-plan.md`
- `crates/nimbus-storage/src/store/journal_snapshot.rs`
- `crates/nimbus-storage/src/store.rs`
- `crates/nimbus-storage/src/lib.rs`
- `crates/nimbus-storage/src/sqlite/journal.rs`
- `crates/nimbus-storage/src/postgres/read.rs`
- `crates/nimbus-storage/src/postgres/write.rs`
- `crates/nimbus-storage/src/mysql/read.rs`
- `crates/nimbus-storage/src/mysql/write.rs`
- `crates/nimbus-storage/src/libsql/read.rs`
- `crates/nimbus-storage/src/libsql/write.rs`
- `crates/nimbus-storage/src/store/journal_snapshot/tests.rs`
- `crates/nimbus-storage/src/tests/sqlite_foundation/snapshot.rs`

## Implementation Evidence

| Area | Evidence |
| --- | --- |
| Typed archive contract | `PointInTimeRestoreTarget` supports `Sequence` and `Timestamp`; `PointInTimeRestoreArchive` stores archive version, resolved target sequence/timestamp, base snapshot, journal tail, storage-format versions, document/index version-format versions, and `target_fingerprint`. |
| Canonical fingerprint | `MaterializedJournalSnapshot::canonical_fingerprint()` validates the snapshot, sorts table identities, documents, and scheduled execution IDs, and hashes the canonical JSON payload with SHA-256. |
| Shared export semantics | `build_point_in_time_restore_archive(...)` resolves sequence/timestamp targets, rejects targets older than the document-version retention floor with `HistoricalReadErrorKind::RetentionExpired`, builds a sequence-0 base snapshot, slices the durable journal tail through the target, and computes the target fingerprint through replay. |
| Fail-closed import validation | `PointInTimeRestoreArchive::validate()` rejects unsupported archive/storage/document-version/index-version formats, non-contiguous journal tails, tails beyond target, and archives missing the target sequence. Provider replay imports also require an empty sequence-0 archive base and an empty destination tenant before mutating the durable log. |
| Embedded restore path | redb and SQLite export/import archives and validate restored fingerprints through `rebuild_materialized_journal_from_snapshot(...)`. |
| Provider restore path | Postgres, MySQL, and libSQL export/import archives in production code. Imports append the validated archive journal tail into an empty tenant and call `recover_durable_journal()`, then compare the restored materialized snapshot fingerprint with the archive target fingerprint. |
| Public storage surface | `PointInTimeRestoreArchive` and `PointInTimeRestoreTarget` are re-exported from `crates/nimbus-storage/src/lib.rs`; backend-specific methods remain storage-layer APIs until SEQ12/SEQ14 define adapter exposure and support-state docs. |

## Verification Evidence

| Command | Result |
| --- | --- |
| `cargo test -p nimbus-storage point_in_time -- --nocapture` | Passed: `4 passed, 0 failed`, `291 filtered out`. Covers redb sequence and timestamp export, redb expired-retention rejection, replay-to-sequence rebuild, and SQLite sequence/timestamp export/import with index-state verification. |
| `cargo test -p nimbus-storage journal_snapshot -- --nocapture` | Passed: `6 passed, 0 failed`, `289 filtered out`. Covers snapshot validation, tail rebuild, point-in-time sequence stop, archive import, retention-expired PITR rejection, and incomplete-tail rejection. |
| `cargo check -p nimbus-storage` | Passed. Confirms redb, SQLite, Postgres, MySQL, and libSQL PITR APIs compile in production code. |

## External Fixture State

SEQ8 landed before the final Docker-backed provider closeout. Postgres/MySQL/
libSQL PITR source surfaces compile and reuse their normal durable recovery
paths. Later SEQ3/SEQ4 closeout runs supplied the live MySQL/libSQL
document/index evidence required before SEQ14.

## SEQ8 Closeout

SEQ8 is complete for the storage-layer PITR/export/import contract. Nimbus now
has a typed archive format, sequence/timestamp target resolution, retention
eligibility checks, storage-format fail-closed validation, canonical restored
snapshot fingerprints, embedded behavioral tests, SQLite backend tests, and
all-provider production API coverage through the durable journal replay path.
