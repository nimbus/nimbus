# Nimbus KV Durability Contract Plan (NKVD, Archived)

> Archived 2026-08-26 after NKVD0, NKVD1, and NKVD9 completed. This plan was
> promoted only to own Band SA11 contract work. It did not reopen the archived
> NKV0 implementation plan or start NKV1, NKV-DR, or NKV6.

Status: `complete, archived`  
Owner: NKV program  
Baseline: main @ `824e8a50a6f59f6bef1e49353b5bbe1792d15718`  
Proof: `docs/private/plans/proof/architecture-review-2026-07/sa11-tenant-kv-durability.md`

## Goal

State the current tenant-KV durability and recovery boundary from production
source. Distinguish local redb transaction durability from document-journal,
PITR, replication, and provider-parity guarantees. Publish the contract in the
NKV operating guide and storage baseline, then close Band SA11 without changing
production behavior.

## Decision

### NKVD-D1 Current durability is local redb transaction durability

- `TenantKvStore` writes value and expiry-index effects in one redb write
  transaction. A committed call survives restart of the same durable file,
  subject to redb's recovery contract.
- Standalone `nimbus kv` opens `RedbTenantKvStore` directly.
- `Engine::tenant_kv_*` invokes the loaded tenant store directly and supports
  `TenantPersistence::Redb` only.
- Neither composition uses the document committer, a committer lease, a
  document commit sequence, or a tenant event append.
- The Engine process fence and redb transaction serialization remain valid
  local guards. They do not make KV writes visible to journal consumers.
- Current document PITR, journal replay, changefeed, replication, and non-redb
  providers do not carry tenant-KV state.
- NKV-DR owns supported backup/restore and point-in-time recovery. NKV6 owns
  replication, distributed fencing, and cross-node ordering.
- Routing tenant-KV through the document committer requires a new NKV decision
  because it changes latency, ordering, recovery, and provider semantics.

## Scope

In scope: current-state contract documentation, source traces, owner routing,
and Band SA11 closure.

Out of scope: production code, a compatibility layer, NKV backup tooling,
replication, new provider implementations, committer integration, and BLI
promotion.

## Ledger

| ID | Task | Status | Evidence |
| --- | --- | --- | --- |
| NKVD0 | Trace standalone and Engine compositions, the redb transaction seam, and absent journal/provider paths. | `done` | `crates/nimbus-kv/src/store.rs`; `crates/nimbus-engine/src/engine/kv.rs`; `crates/nimbus-storage/src/kv.rs` |
| NKVD1 | Publish NKVD-D1 in the operator guide, storage baseline, and Band SA proof. | `done` | `docs/private/operating/nimbus-kv.md`; `docs/private/architecture/storage/persistence-engine-baseline.md`; SA11 proof |
| NKVD9 | Run docs gates, close SA11, and archive this plan. | `done` | `bash scripts/check-docs.sh`; `bash scripts/verify-nimbus-docs-site.sh` |

## Acceptance

- The word "durable" names the exact redb commit boundary.
- Both current compositions are explicit.
- No reader can infer journal, PITR, replication, or cross-provider coverage.
- Future recovery and cluster work has one named NKV owner each.
- No production behavior changes.
- Band SA11 is terminal and no NKV implementation phase is active.

## Execution Log

| Date | Item | Action | Evidence |
| --- | --- | --- | --- |
| 2026-08-26 | NKVD0 | promoted | Band SA11 created the minimal NKV owner plan after archived NKV0 left no active contract owner. Source audit confirmed direct redb calls in both compositions and no journal or provider path. |
| 2026-08-26 | NKVD1 | completed | Published NKVD-D1 in the operator and storage architecture records. Added the SA11 source-trace proof. No production code changed. |
| 2026-08-26 | NKVD9 | completed | Docs gates passed, SA11 closed, and the plan moved directly to archive. NKV1, NKV-DR, and NKV6 remain unscheduled future phases. |
