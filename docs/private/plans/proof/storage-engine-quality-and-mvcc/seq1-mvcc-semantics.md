---
status: done
phase: SEQ1
plan: docs/plans/storage-engine-quality-and-mvcc-plan.md
updated: 2026-06-06
---

# SEQ1 MVCC Semantics

SEQ1 defines the typed MVCC semantic contract in `nimbus-core` before storage
layouts start changing. It owns commit/read timestamps, retention windows,
historical-read eligibility, cursor identity, and fail-closed unsupported or
expired states.

## Scope

SEQ1 added typed core concepts and tests for:

- `CommitSequence`
- `CommitTimestamp`
- `ReadTimestamp`
- `HistoricalReadSnapshot`
- `RetentionFloor`
- `HistoryWindow`
- historical read eligibility
- historical cursor identity
- historical authorization policy selection
- pending-vs-committed write visibility
- unsupported-backend, unsupported-adapter, expired-retention, and
  format-mismatch errors

SEQ1 does not implement document/index version storage. SEQ3 and SEQ4 own the
physical layouts after SEQ1 and SEQ2 settle the semantics.

## Implementation

| Area | Evidence |
| --- | --- |
| MVCC scalar contract | `crates/nimbus-core/src/mvcc.rs` defines `CommitSequence`, `CommitTimestamp`, `ReadTimestamp`, `HistoricalReadSnapshot`, `RetentionFloor`, and `HistoryWindow` with serde-ready newtypes and explicit retained-window checks. |
| Timestamp resolution | `HistoricalReadSnapshot::resolve_at_or_before` resolves product timestamps to the latest durable commit at or before the read timestamp, breaks same-timestamp ties by the highest `CommitSequence`, and rejects non-monotonic commit timelines with `SnapshotUnavailable` so timestamp-target PITR cannot replay a sequence prefix containing commits timestamped after the requested point. |
| Historical cursor identity | `HistoricalCursorIdentity` binds the resolved read snapshot, `TableId`, full-scan/index query shape, `PolicySnapshotId`, `RetentionFloor`, backend/adapter support identity, and storage format generation. Resume drift returns a typed fail-closed cursor mismatch. |
| Historical authorization | `HistoricalAuthorization` requires a policy snapshot for the requested read timestamp. Missing policy snapshots fail closed with `PolicySnapshotMissing`. |
| Pending writes | `HistoricalVersionVisibility::Pending` is never visible to historical readers; committed versions are visible only at or after their commit sequence. |
| Unsupported and expired states | `HistoricalReadErrorKind` and `Error::HistoricalRead` add typed fail-closed states for unsupported backends, unsupported adapters, expired retention, timestamp out of range, cursor mismatch, policy snapshot missing, and storage format mismatch. |
| Public error mapping | `crates/nimbus-server/src/error_envelope.rs` maps typed historical-read errors into structured public envelopes and HTTP statuses. Unsupported backend/adapter reports `501 Not Implemented`; expired/cursor/format/timestamp problems report `400 Bad Request`; missing policy snapshots report `403 Forbidden`; unavailable serving snapshots report `503 Service Unavailable`. |
| Sandbox forwarding | `crates/nimbus-bin/src/machine/backend.rs` classifies historical-read failures as invalid forwarded specs if they ever cross the machine API path. |

## Read-Before-Edit Checklist

Before editing `nimbus-core`, read:

- `crates/nimbus-core/src/lib.rs`
- current core error/result definitions
- current timestamp/sequence/table/index identity definitions
- storage and engine callers that already expose `SequenceNumber`,
  `ReadVisibility`, or retention-floor concepts
- tests around table/index identity and read visibility

## Starting Decisions From SEQ0

| Topic | Starting decision |
| --- | --- |
| Historical authorization | Resolve the read policy as of the read timestamp; missing or expired policy snapshots fail closed. |
| Historical cursor identity | Bind cursors to read timestamp, table id, index/full-scan shape, query shape, policy snapshot, retention floor, backend support state, and format generation. |
| Storage format | Unknown old/future MVCC layout versions fail closed. |
| Retention | Prune floor is the minimum safe floor across all readers, consumers, replicas, materializers, PITR points, and transaction/session pins. |
| CDC handoff | Snapshot cut plus next ordered event sequence; no missed or duplicated logical events. |

## Verification Evidence

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed. |
| `cargo test -p nimbus-core mvcc -- --nocapture` | Passed: `11 passed, 0 failed`, `114 filtered out`. Covers timestamp-to-sequence tie handling, non-monotonic commit timestamp rejection, before-first-commit rejection, retention bounds, historical policy snapshot requirement, cursor identity drift, format mismatch, unsupported backend/adapter typed errors, and pending-write invisibility. |
| `cargo test -p nimbus-core historical_read -- --nocapture` | Passed: `2 passed, 0 failed`, `106 filtered out`. Covers the public historical-read error helper and a filtered MVCC visibility regression. |
| `cargo check -p nimbus-core` | Passed. |
| `npm run build -w nimbus-ui` | Passed after the UI package codegen script was corrected to use the repo-owned `@nimbus/codegen` CLI instead of stale `convex codegen --app .`. Produces `packages/nimbus-ui/dist/index.html` for `nimbus-assets`. |
| `cargo test -p nimbus-server snapshot_unavailable_historical_read_maps_to_service_unavailable -- --nocapture` | Passed: `1 passed, 0 failed`, `410 filtered out`. Confirms `HistoricalReadErrorKind::SnapshotUnavailable` maps to HTTP `503 Service Unavailable` once the UI asset prerequisite exists. |

The public server mapping that was originally blocked by the UI asset
prerequisite is now verified.

## SEQ1 Closeout

SEQ1 is complete. `docs/plans/storage-engine-quality-and-mvcc-plan.md` now
marks `SEQ1` done and moves the single active phase to `SEQ2`.
