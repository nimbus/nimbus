# SA11 Tenant KV Durability Contract Proof

Date: 2026-08-26  
Owner plan: `archive/nimbus-kv-durability-contract-plan.md`  
Band: SA11

## Source trace

| Question | Production evidence | Verdict |
| --- | --- | --- |
| How does standalone NKV persist? | `NimbusKvStore::durable_at` opens `RedbTenantKvStore` in `crates/nimbus-kv/src/store.rs`. | Direct local redb composition. |
| How does the Engine bridge persist? | `Engine::tenant_kv_*` calls `TenantKvStore` through `with_tenant_kv_store` in `crates/nimbus-engine/src/engine/kv.rs`. Only `TenantPersistence::Redb` is accepted. | Direct loaded-redb composition; other providers fail unsupported. |
| What is the write atomicity seam? | `TenantStore::kv_put`, `kv_delete`, `kv_apply_batch`, and `kv_update` open and commit redb write transactions in `crates/nimbus-storage/src/kv.rs`. Values and expiry-index effects share the transaction. | Local transaction durability and atomicity. |
| Is there a document commit sequence or event? | The Engine bridge invokes the store methods directly. It does not call the mutation committer or journal append interfaces. | No committer lease, document sequence, or tenant event. |
| Does current document recovery include KV? | PITR and changefeed consume typed tenant event journal records; tenant-KV produces none. | No current document PITR, replay, changefeed, or replication coverage. |
| Who owns future coverage? | The NKV architecture program names NKV-DR for backup/PITR and NKV6 for cluster replication. | Future work is routed, not implied. |

## Published contract

The contract now appears in:

- `docs/private/operating/nimbus-kv.md`, for operators and embedders;
- `docs/private/architecture/storage/persistence-engine-baseline.md`, so the
  document-journal contract does not appear to include tenant-KV;
- the archived NKVD plan, which owns the decision and future routing.

The text distinguishes process fencing and redb serialization from committer
leases and journal visibility. It also states that a closed-file copy is not a
supported NKV backup/restore contract.

## Verification

- `bash scripts/check-docs.sh`: passed; 109 pages were link-clean, the source
  map resolved, the private fence remained intact, and titles were unique.
- `bash scripts/verify-nimbus-docs-site.sh`: passed, 17/17 conditions.
- `git diff --check`: passed.
- Production files changed: none.

## Closure

SA11 is complete. The NKVD plan remains archived; no NKV implementation phase
is active. Every Band SA row is now terminal.
