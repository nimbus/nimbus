# SEQ5 Serving Snapshot Manager

status: done

## Scope

SEQ5 extends the existing materialized `ServingSnapshotManager` boundary with a
read-shape pin for MVCC reads. It does not add a second serving cache or a
parallel snapshot manager. A pinned serving read handle now carries the SEQ2
`HistoricalReadShape` beside the immutable serving snapshot, validates that the
snapshot covers the requested historical sequence, and fails closed if the table
is not present in the serving snapshot.

The current SEQ5 implementation is intentionally scoped to the serving snapshot
boundary. Full historical point/scan/index storage reads are already routed
through SEQ3/SEQ4 storage history. SEQ6 owns transaction-session pending-write
overlay and OCC integration.

## Read-Before-Edit Checklist

- `docs/plans/storage-engine-quality-and-mvcc-plan.md`
- `crates/nimbus-core/src/visibility.rs`
- `crates/nimbus-core/src/versioned_registry.rs`
- `crates/nimbus-core/src/error.rs`
- `crates/nimbus-engine/src/tenant/materialized_reads/snapshot.rs`
- `crates/nimbus-engine/src/tenant/materialized_reads/backend/publication.rs`
- `crates/nimbus-engine/src/tenant/materialized_reads/backend/loading.rs`
- `crates/nimbus-engine/src/tests/materialized_serving/retention.rs`
- `crates/nimbus-engine/src/service/queries/materialized.rs`
- `crates/nimbus-engine/src/service/queries/planner/mod.rs`

## Implementation Evidence

| Area | Evidence |
| --- | --- |
| Existing boundary reused | `ServingSnapshotManager` remains the single retained serving snapshot manager. SEQ5 adds `PinnedServingReadSnapshot` to `crates/nimbus-engine/src/tenant/materialized_reads/snapshot.rs` instead of creating a second cache. |
| Read-shape bundle carried | `ServingSnapshot::pin_read_shape(...)` accepts the SEQ2 `HistoricalReadShape` and stores it inside the pinned handle, preserving stable `TableId`, schema/index identity, read policy, and read snapshot metadata with the immutable serving view. |
| Engine service seam | `Service::pin_serving_read_shape(...)` loads or reuses the existing materialized serving snapshot for the read-shape table and sequence, then pins the read-shape bundle through the same `ServingSnapshotManager` boundary. |
| Coverage validation | `pin_read_shape(...)` compares the serving snapshot `covered_sequence()` with `read_shape.read_snapshot().sequence().sequence()` and rejects snapshots that do not cover the requested historical read sequence. |
| Table validation | `pin_read_shape(...)` rejects shapes whose table is absent from the materialized serving snapshot, preventing a read handle from silently falling through to a partial serving view. |
| Typed fail-closed error | `HistoricalReadErrorKind::SnapshotUnavailable` reports serving-snapshot coverage failures separately from retention expiration, unsupported backends, cursor mismatch, and policy-snapshot failures. |
| Immutable handle API | `PinnedServingReadSnapshot` exposes `covered_sequence`, `read_shape`, `table_id`, `table_documents`, and `document` without mutable access to the underlying serving snapshot. |

## Verification Evidence

| Command | Result |
| --- | --- |
| `cargo test -p nimbus-engine pinned_serving_read_shape -- --nocapture` | Passed: `2 passed, 0 failed`, `270 filtered out`. |

## Test Coverage

- `pinned_serving_read_shape_handle_preserves_identity_and_documents_after_later_applies`
  warms the existing materialized serving surface, pins a SEQ2 read-shape bundle
  over the retained snapshot, verifies stable `TableId`/index identity and
  document access, writes a later document, and proves the pinned handle does
  not see the later write while the current serving snapshot does.
- `pinned_serving_read_shape_handle_fails_closed_when_snapshot_does_not_cover_shape`
  builds a newer read-shape requirement than the pinned serving snapshot covers
  and verifies a typed `SnapshotUnavailable` historical-read error.

## SEQ5 Closeout

SEQ5 is complete for the serving snapshot boundary. The implementation keeps
latest-row materialized serving behavior intact, carries resolved MVCC
read-shape identity through the pinned handle, and fails closed for unavailable
serving snapshots. SEQ6 must build on this by integrating MVCC visibility with
transaction sessions, pending writes, and existing OCC conflict checks.
