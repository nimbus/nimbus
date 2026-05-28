status: done
date: 2026-05-27
phase: SATH0

# SATH0 Review

## Result

The storage architecture trust plan is now wired as an executable control
plane. The immediate gap set is intentionally narrower than the follow-on MVCC
roadmap: Nimbus should first make its current latest-row plus ordered-journal
storage posture complete, durable, observable, and cross-backend tested.

## Source-Backed Findings

| Severity | Finding | Evidence | Plan phase |
| --- | --- | --- | --- |
| Critical | The durable history surface is still named and shaped around document mutation batches. | `crates/nimbus-core/src/mutation.rs` defines `DurableMutationRecord` with `writes` and a scheduler execution id, while schema, table lifecycle, index lifecycle, and trigger cursor changes use separate durable paths. | SATH1-SATH3 |
| High | Destructive table cleanup exists but has no shared retention-floor decision boundary. | Backend table lifecycle modules expose `hard_delete_table_identity`, and durable journal streaming has a cursor floor, but table hard delete is not yet gated by retained snapshots, journal consumers, replicas, and materializers. | SATH4 |
| High | Read freshness has behavior and tests, but the API boundary is not a storage-wide contract. | Engine materialized read code has `ServingSnapshotManager`, and mutation-journal visibility tests prove waiting behavior; storage and engine still lack public typed `ReadVisibility`, `RequiredSequence`, and `PinnedServingSnapshot` contracts. | SATH5 |
| High | Backend format versions are implicit. | Snapshot export carries a `MaterializedJournalSnapshot` version, but backend stores do not expose a per-backend storage format/version record with startup validation and diagnostics. | SATH7 |
| Medium | Capabilities and health are not one machine-readable surface. | Existing table identity diagnostics and journal progress are useful, but reviewers cannot ask one DTO for backend layout, read consistency, event journal head, applied head, retention floor, freshness lag, format version, encryption posture, and recovery status. | SATH6 |
| Medium | Generated histories exist but do not yet exercise the whole storage contract. | `crates/nimbus-storage/src/tests/generated_history.rs` covers document histories; the required SATH corpus must include schemas, indexes, lifecycle, scheduler dedup, snapshots, crash points, replay, retention, and diagnostics. | SATH9 |
| Medium | Table lifecycle behavior exists in each backend, but transition semantics should be pure and shared. | redb, SQLite, Postgres, MySQL, and libSQL have lifecycle functions; the reusable state-machine proof should ensure every backend applies the same allowed transitions. | SATH8 |
| Low | Operator evidence needs one storage-trust path. | Current docs describe table identity and persistence, but the event journal, retention, diagnostics, and format gates need one closeout proof and operating reference. | SATH10-SATH11 |

## Reference Pattern Map

| Source | Useful pattern for Nimbus | Bound on adoption |
| --- | --- | --- |
| Convex | Stable internal table identity, table/index lifecycle state, dependency tracking by table identity, and explicit write-log/snapshot language. | Keep MVCC rows for the follow-on plan. |
| CockroachDB | Fail-loud storage format/version gates and randomized/metamorphic storage tests. | Do not adopt distributed MVCC or range-level machinery here. |
| TigerBeetle | Retention and compaction safety based on pinned visibility floors plus deterministic equivalence checks. | Do not build a custom direct-I/O storage engine. |
| ElectricSQL | Snapshot plus log-offset protocols for consumers and explicit backend capability callbacks. | Do not make one replication shape the core storage model. |
| ExtendDB | Backend-specific physical layout behind protocol-compatible semantics and disciplined provider tests. | Do not force per-table physical SQL tables without evidence. |

## SATH0 Artifacts

- `docs/plans/archive/storage-architecture-trust-hardening-plan.md` is the
  completed execution record.
- `scripts/verify-storage-architecture-trust-hardening.sh` is the reusable
  aggregate completion gate.
- `docs/technical-debt.md` now has SATH-owned rows for every implementation
  gap that remains open until closeout.

## Baseline Verification

Focused storage baseline before SATH implementation work:

```text
cargo check -p nimbus-core -p nimbus-storage --all-targets
Finished `dev` profile [unoptimized + debuginfo] target(s) in 38.31s
```
