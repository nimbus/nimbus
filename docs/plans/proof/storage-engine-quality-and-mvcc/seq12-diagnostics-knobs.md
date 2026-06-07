# SEQ12 Diagnostics And Knobs

status: done

## Summary

`SEQ12` turns storage diagnostics into an operator-facing MVCC support snapshot
instead of a thin backend/head report. The existing fields remain present, and
the diagnostic now also exposes versioned index storage, MVCC operator state,
historical-query admission, retention pressure, backend capability profile,
backend support state, adapter capability profiles, adapter support state, and
backend-parity comparison state.

## Implementation Anchors

- `crates/nimbus-storage/src/diagnostics.rs`
  - `IndexVersionStorageDiagnostic`
  - `MvccOperatorDiagnostic`
  - `MvccVersionCountsDiagnostic`
  - `HistoricalQueryAdmissionRequest`
  - `HistoricalQueryAdmissionDiagnostic`
  - `StoragePressureDiagnostic`
  - `StorageCapabilityProfile`
  - `BackendParityDiagnostic`
  - `AdapterSupportDiagnostic`
  - `storage_health_diagnostic_with_retention_config(...)`
- `crates/nimbus-storage/src/store/index_versions.rs`
  - redb `index_version_storage_diagnostic`
- `crates/nimbus-storage/src/sqlite/index_versions.rs`
  - SQLite `index_version_storage_diagnostic`
- `crates/nimbus-storage/src/postgres/index_versions.rs`
  - Postgres `index_version_storage_diagnostic`
- `crates/nimbus-storage/src/mysql/index_versions.rs`
  - MySQL `index_version_storage_diagnostic`
- `crates/nimbus-storage/src/libsql/index_versions.rs`
  - libSQL `index_version_storage_diagnostic`
- `crates/nimbus-core/src/error.rs`
  - `HistoricalReadErrorKind` is serializable for typed operator states.

## Operator States Covered

- Healthy: default empty embedded store reports nominal pressure and healthy
  MVCC state.
- Compacting: a shorter retention-config diagnostic reports
  `CompactionRecommended` and `StorageOperatorState::Compacting` without
  mutating storage.
- Lagging: pressure classification reports `ReplayLagging` and maps to
  `StorageOperatorState::Lagging`.
- Expired: historical admission returns `RetentionExpired` for sequences older
  than the retained MVCC window.
- Unsupported: historical admission and adapter matrices expose typed
  unsupported backend/adapter states while adapter capability profiles remain
  `LatestOnly` until public historical/PITR/changefeed routes ship.
- Format mismatch: historical admission returns `FormatMismatch`.
- Policy gated: historical admission returns `PolicySnapshotMissing`.
- Backend divergence: `BackendParityDiagnostic::compare(...)` reports
  `BackendDivergence` when operator-visible MVCC counts or heads differ.
- SEQ14 review correction: backend feature support no longer reports stale
  `external_evidence_pending` states after live MySQL/libSQL closeout evidence,
  and native HTTP/WebSocket historical reads, PITR, and changefeed remain
  `UnsupportedAdapter` until public native routes are documented and shipped.

## Verification

- `cargo test -p nimbus-storage diagnostic -- --nocapture`
  - result: `15 passed, 0 failed`
  - note: focused provider diagnostic tests that match the filter still report
    skipped live fixture setup when no explicit provider URL exists and Docker
    daemon access is unavailable. Later SEQ3/SEQ4 closeout runs supplied the
    Docker-backed live MySQL/libSQL document/index evidence required before
    SEQ14.
- `cargo check -p nimbus-storage`
  - result: passed
