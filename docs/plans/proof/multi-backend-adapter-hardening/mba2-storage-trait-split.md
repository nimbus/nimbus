# MBA2 Storage Trait Split

Date: 2026-05-27

final_traits: TenantLifecycle, TenantPointRead, TenantPointWrite,
TenantRangeScan, DurableJournal, SchedulerStore, ControlPlaneUsage,
KeyProviderSurface, StorageEngine

dispatch_posture: focused_static_traits_with_existing_async_executor_seam

## Before

`crates/nimbus-storage/src/async_storage/traits.rs` exposed the async executor
seam:

- `EmbeddedPersistenceProvider`
- `TenantReadStorage`
- `TenantWriteStorage`
- `UsageStorage`

That seam was already smaller than the old concrete store surface, but it did
not name the capability families that future backends should implement or opt
out of.

## After

`crates/nimbus-storage/src/traits/` now names focused capability traits:

| Trait | redb | SQLite | Postgres | MySQL | libSQL replica | retained control redb | local key providers |
|-------|------|--------|----------|-------|----------------|-----------------------|--------------------|
| `TenantLifecycle` | yes | yes | yes | yes | yes | not_applicable | not_applicable |
| `TenantPointRead` | yes | yes | yes | yes | yes | not_applicable | not_applicable |
| `TenantPointWrite` | yes | yes | yes | yes | yes | not_applicable | not_applicable |
| `TenantRangeScan` | yes | yes | yes | yes | yes | not_applicable | not_applicable |
| `DurableJournal` | yes | yes | yes | yes | yes | not_applicable | not_applicable |
| `SchedulerStore` | yes | yes | yes | yes | yes | not_applicable | not_applicable |
| `ControlPlaneUsage` | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | yes | not_applicable |
| `KeyProviderSurface` | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | not_applicable | yes |
| `StorageEngine` | yes | yes | yes | yes | yes | not_applicable | not_applicable |

## Call-Site Decision

The engine still uses `TenantReadStorage` and `TenantWriteStorage` at the async
blocking boundary. That is intentional: those executor traits preserve
cancellation, blocking runtime isolation, and the current
pre-commit/committed-write distinction. The focused traits are the capability
contract underneath that seam, not a forced object-erased replacement.

## Stub Audit

The new focused trait module contains no `unimplemented!()` stubs. Backends that
do not own a capability simply do not implement the corresponding trait.
