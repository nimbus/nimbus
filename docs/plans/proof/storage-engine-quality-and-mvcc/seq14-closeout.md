# SEQ14 Closeout Evidence

status: done

## Summary

`SEQ14` closes the storage-engine-quality-and-mvcc plan by making the completed
architecture visible outside the plan, rerunning the reusable verifier after
the live-provider gates, recording the one live bug found during closeout, and
pushing the branch with a draft pull request.

## Branch And PR State

- Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/storage-engine-quality-and-mvcc`
- Branch: `codex/storage-engine-quality-and-mvcc`
- Base recorded by SEQ0: `main@4a9e6a77bcd3c51ef14018d1e34c3e2dfd199d38`
- First pushed commit: `4e99e45b6fea6e01b7df23945e4e807c681d624d`
- Draft PR URL: `https://github.com/nimbus/nimbus/pull/13`

## Post-Closeout Architecture Challenge

The durable follow-up audit prompt is
`docs/plans/prompts/storage-engine-quality-and-mvcc-post-closeout-architecture-review.md`.
It is not a resume prompt for SEQ implementation. It is the next architecture
challenge gate for a fresh reviewer to verify the PR head, re-check local
Convex/CockroachDB/TigerBeetle/ElectricSQL/ExtendDB source refs, inspect the
final code/proof/verifier evidence, and decide whether the completed storage
architecture is genuinely enterprise-ready.

## Architecture Documentation Updates

The completed architecture is no longer hidden only in this plan:

- `ARCHITECTURE.md` now describes the latest-row plus version-history storage
  architecture, `nimbus-core` MVCC/read-shape types, `nimbus-storage`
  document-version, index-version, PITR, CDC, retention, and diagnostic modules,
  and the fail-closed historical read boundaries.
- `docs/architecture/storage/persistence-engine-baseline.md` now describes the
  current backend layouts, `document_versions`, `index_versions`, external
  provider history layouts, historical planning through `HistoricalReadShape`,
  PITR, CDC, retention GC, storage-format gates, diagnostics, and the serving
  snapshot boundary.
- `docs/operating/storage-backends.md` now records latest-row current reads,
  retained document/index versions, historical reads, PITR, CDC/changefeed, and
  updated `StorageHealthDiagnostic` fields.
- Adapter docs now state how the shared storage guarantees are inherited:
  `docs/adapters/convex/compatibility.md`,
  `docs/adapters/firebase/compatibility.md`,
  `docs/adapters/cloud-functions/compatibility.md`,
  `docs/adapters/mongodb/operations.md`,
  `docs/adapters/dynamodb/enterprise-readiness.md`, and
  `docs/adapters/native/README.md`.
- `docs/plans/README.md` routes active work to this plan and records the
  completed SEQ0-SEQ13 evidence plus the SEQ14 closeout requirement.

## Live Provider Closeout Evidence

Docker Desktop was started and the Docker daemon was available for the
external-provider fixtures.

- `cargo test -p nimbus-storage document_versions -- --nocapture`
  - result: `17 passed, 0 failed`
- `cargo test -p nimbus-storage index_versions -- --nocapture`
  - initial result: failed in
    `tests::libsql_provider::libsql_index_versions_are_materialized_during_durable_recovery`
  - confirmed root cause: `LibsqlReplicaTenantStore::table_identity_diagnostics()`
    read `active_cache_store()` without a freshness barrier after
    `replace_table_schema(...)`, so diagnostics could see a stale local cache.
  - fix: `crates/nimbus-storage/src/libsql/read.rs` now routes
    `table_identity_diagnostics()` through
    `self.current_query_cache_store()?.read_snapshot()?`.
- `cargo test -p nimbus-storage libsql_index_versions_are_materialized_during_durable_recovery -- --nocapture`
  - result after fix: `1 passed, 0 failed`
- `cargo test -p nimbus-storage index_versions -- --nocapture`
  - result after fix: `12 passed, 0 failed`
- `cargo test -p nimbus-storage historical_index -- --nocapture`
  - result after fix: `10 passed, 0 failed`
- `cargo check -p nimbus-storage`
  - result after fix: passed

## Performance Evidence

`SEQ13` added
`redb_storage_engine_quality_performance_budget_covers_latest_historical_cdc_pitr_and_gc`
as a deterministic redb smoke budget for the new storage engine surfaces.

- `cargo test -p nimbus-storage redb_storage_engine_quality_performance_budget -- --nocapture`
  - result: `1 passed, 0 failed`
  - latest point reads: `1.283209ms <= 200ms`
  - historical point reads: `2.257625ms <= 300ms`
  - historical index pagination: `23.009417ms <= 500ms`
  - CDC stream: `10.979417ms <= 300ms`
  - PITR export/import: `264.958375ms <= 1s`
  - retention compaction: `1.386792ms <= 500ms`

## Additional Closeout Finding

The final verifier exposed one nondeterministic SQLite PITR test assumption:
`sqlite_point_in_time_archive_restores_sequence_and_timestamp_targets` used the
system clock, so adjacent commits could share the same timestamp and make a
timestamp-target restore choose the later update sequence instead of the second
insert sequence. The test now opens SQLite with a `ManualClock` and assigns
distinct timestamps to schema creation, first insert, second insert, and update
commits.

- `cargo test -p nimbus-storage sqlite_point_in_time_archive_restores_sequence_and_timestamp_targets -- --nocapture`
  - result after fix: `1 passed, 0 failed`

## Post-Review Audit Fixes

The local autoreview skill and manual follow-up audit found additional
closeout issues after the draft PR was opened. The fixes are now part of this
branch rather than deferred:

- `HistoricalReadErrorKind::SnapshotUnavailable` is mapped by
  `crates/nimbus-server/src/error_envelope.rs` to HTTP `503 Service
  Unavailable`.
- Timestamp-target PITR now rejects non-monotonic commit timelines with
  `SnapshotUnavailable` instead of choosing a sequence prefix that could
  include commits timestamped after the requested point.
- MongoDB `findAndModify` with `new: true` now reads from the active
  transaction overlay for both update and upsert return paths.
- Document/index history storage-format admission now reports
  `HistoricalReadErrorKind::FormatMismatch` instead of generic internal errors.
- Storage diagnostics no longer report stale MySQL/libSQL
  `external_evidence_pending` states after SEQ14 closeout, and native
  HTTP/WebSocket historical reads, PITR, and changefeed remain
  `UnsupportedAdapter` until public native routes exist.
- Storage diagnostics now expose a derived capability profile beside the typed
  per-feature support matrix, so backends can report `enterprise_complete`
  while adapters with no public historical/PITR/changefeed routes remain
  `latest_only`.
- Historical index cursors now bind the read-shape policy snapshot and storage
  format generation, so pagination cannot resume across policy or format drift.
- Retention GC watermarks now route pins by resource dependency instead of
  copying one active-pin floor into every resource family.
- MongoDB `$changeStream` now fails closed with `CommandNotSupported` until the
  adapter is backed by the durable SEQ changefeed bootstrap and cursor model.
- SQL-family historical index planning is now centralized in
  `crates/nimbus-storage/src/index/history_scan.rs`, reducing SQLite/Postgres/MySQL
  drift risk while leaving physical row fetching and document hydration
  backend-owned.
- MySQL durable-journal stream/bootstrap now derives `cursor_floor` from the
  retained `commit_log` floor, aligning CDC retention semantics with
  redb/SQLite/Postgres instead of reporting a permanent zero floor.
- SQL-family production roots now stay below the repo's 1,500-line review
  threshold by extracting concept-owned helpers:
  `crates/nimbus-storage/src/mysql/table_catalog.rs`,
  `crates/nimbus-storage/src/mysql/query_helpers.rs`,
  `crates/nimbus-storage/src/postgres/query_helpers.rs`, and
  `crates/nimbus-storage/src/postgres/write_schema_events.rs`. Post-extraction
  line counts are `mysql/backend.rs` 1329, `postgres/backend.rs` 1470, and
  `postgres/write.rs` 1476.
- `docs/technical-debt.md` now marks A-021, A-022, and O-007 done.
- The owning plan records the narrow modularity exception for the provider
  conformance roots and the embedded redb composition root.

Focused verification after these fixes:

- `cargo test -p nimbus-core mvcc -- --nocapture`
  - result: `11 passed, 0 failed`
- `cargo test -p nimbus-storage format -- --nocapture`
  - result: `15 passed, 0 failed`
- `cargo test -p nimbus-storage diagnostic -- --nocapture`
  - result: `15 passed, 0 failed`
- `cargo test -p nimbus-storage historical_index -- --nocapture`
  - result: `10 passed, 0 failed`
- `cargo test -p nimbus-storage durable_journal_stream -- --nocapture`
  - result: `2 passed, 0 failed`
- `cargo check -p nimbus-storage`
  - result: passed after SQL-family production-root modularity extraction
- `cargo test -p nimbus-mongodb find_and_modify -- --nocapture`
  - result: `9 passed, 0 failed`
- `cargo test -p nimbus-storage point_in_time -- --nocapture`
  - result: `4 passed, 0 failed`
- `cargo test -p nimbus-storage generated_history -- --nocapture`
  - result: `9 passed, 2 ignored`
- `npm run build -w nimbus-ui`
  - result: passed after `packages/nimbus-ui` codegen switched from stale
    `convex codegen --app .` to the repo-owned `@nimbus/codegen` CLI.
- `cargo test -p nimbus-bridge runtime_host_error_envelope_preserves_historical_read_kind -- --nocapture`
  - result: `1 passed, 0 failed`
- `cargo test -p nimbus-convex historical_read_error_round_trips_through_runtime_encoding -- --nocapture`
  - result: `1 passed, 0 failed`
- `cargo test -p nimbus-server snapshot_unavailable_historical_read_maps_to_service_unavailable -- --nocapture`
  - result: `1 passed, 0 failed`

## Final Local Verification

- `bash scripts/verify-storage-engine-quality-and-mvcc.sh`
  - result: `20 passed, 0 failed`
- `cargo fmt --all --check`
  - result: passed
- `cargo check -p nimbus-core`
  - result: passed
- `cargo check -p nimbus-mongodb`
  - result: passed
- `cargo check -p nimbus-storage`
  - result: passed
- `npm run docs:validate-refs:strict`
  - result: `docs reference validation: pass (242 working-tree Markdown files)`
- `git diff --check`
  - result: passed

## Closeout Notes

- No SEQ phase remains in `external_evidence_pending`.
- The live provider closeout promoted SEQ3 and SEQ4 to `done` and verified
  MySQL/libSQL fixture execution without skip paths.
- The libSQL diagnostics freshness bug is fixed in the implementation rather
  than waived in the proof.
- Branch push and draft PR creation are complete.
