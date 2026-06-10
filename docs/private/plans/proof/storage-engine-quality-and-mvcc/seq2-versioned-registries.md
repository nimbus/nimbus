---
status: done
phase: SEQ2
plan: docs/plans/storage-engine-quality-and-mvcc-plan.md
updated: 2026-06-06
---

# SEQ2 Versioned Registries

SEQ2 added versioned table, schema, index, and read-policy registry snapshots so
historical reads resolve one coherent read-shape bundle before consulting
document or index history.

## Scope

SEQ2 must make historical read-shape resolution explicit for:

- stable table identity across rename, recreate, import, drop, hide, and delete
  transitions
- schema snapshots as of a resolved historical read snapshot
- enabled index identities and lifecycle states as of the read snapshot
- access-policy snapshots as of the read snapshot
- a single read-shape bundle carried into later document/index history phases
- fail-closed behavior when a required registry or policy snapshot is missing,
  expired, unsupported, or in an unknown storage format generation

SEQ2 does not implement document or index version rows. SEQ3 and SEQ4 own those
physical histories after SEQ2 proves metadata identity and policy resolution.

## Implementation

| Area | Evidence |
| --- | --- |
| Event-derived registry oracle | `crates/nimbus-core/src/versioned_registry.rs` defines `VersionedRegistry` over ordered `TenantEventRecord`s. It validates record integrity, rejects duplicate event sequences, and reconstructs registry state at a resolved `HistoricalReadSnapshot`. |
| Read-shape bundle | `HistoricalReadShape` binds read snapshot, logical table name, stable `TableId`, optional `TableSchema`, queryable `IndexDefinition`s, `PolicySnapshotId`, and storage format generation. |
| Table lifecycle | The registry applies `StageHidden`, `ActivateHidden`, `MarkDeleting`, and `HardDelete` events so hidden replacements do not affect reads until activation, replaced identities become deleting, and deleting/hard-deleted identities are not visible by logical table name. |
| Schema and policy snapshots | `SchemaChangeEvent::SetTable` records the schema as of that event; `DeleteTable` removes the schema while preserving schemaless table identity; policy snapshots are derived from the table schema access-policy revision or the empty-policy revision. |
| Index lifecycle | `IndexLifecycleEvent` updates existing schema index definitions so pending indexes can become queryable only after the historical lifecycle event. |
| Format behavior | Registry format generation `0` fails closed with `HistoricalReadErrorKind::FormatMismatch`. |

## Read-Before-Edit Checklist

Before editing registry code, read:

- `crates/nimbus-core/src/mutation.rs`
- `crates/nimbus-core/src/schema.rs`
- `crates/nimbus-core/src/types.rs`
- `crates/nimbus-core/src/auth/mod.rs`
- `crates/nimbus-engine/src/service/schema.rs`
- `crates/nimbus-engine/src/service/queries/authorization.rs`
- backend tenant/table/index registry persistence code in `crates/nimbus-engine/src/persistence/tenant/`
- existing SATH tests and proof around table lifecycle, tenant events, replay,
  and storage format gates

## Starting Decisions From SEQ1

| Topic | Decision |
| --- | --- |
| Read coordinate | Historical product timestamps resolve to `HistoricalReadSnapshot` using latest commit at or before the read timestamp; same-timestamp ties choose the highest `CommitSequence`. |
| Cursor binding | Historical cursors bind the resolved read snapshot, not just caller timestamp. |
| Policy snapshot | Historical authorization requires an explicit `PolicySnapshotId`; missing snapshots fail closed. |
| Registry use | SEQ2 must make table/schema/index/policy metadata resolve as one coherent bundle before versioned document/index reads are exposed. |
| Format behavior | Unknown old/future registry layout generations fail closed with typed historical-read errors. |

## Verification Evidence

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed. |
| `cargo test -p nimbus-core versioned_registry -- --nocapture` | Passed: `8 passed, 0 failed`, `108 filtered out`. Covers policy/index as-of snapshots, hidden replacement activation, deleting state, before-create and hard-delete reads, schema deletion with schemaless identity, index lifecycle promotion, duplicate event sequence rejection, and format-generation fail-closed behavior. |
| `cargo check -p nimbus-core` | Passed. |

## SEQ2 Closeout

SEQ2 is complete. `docs/plans/storage-engine-quality-and-mvcc-plan.md` now
marks `SEQ2` done and moves the single active phase to `SEQ3`.
