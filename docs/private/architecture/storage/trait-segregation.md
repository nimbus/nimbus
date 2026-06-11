# Storage Trait Segregation

Date: 2026-05-27

Nimbus storage capabilities are split by ownership, not by backend brand. The
goal is to make supported surfaces explicit while preserving the current async
executor seam for blocking work and cancellation.

## Current Posture

The existing async executor traits remain in
`crates/nimbus-storage/src/async_storage/traits.rs`:

- `EmbeddedPersistenceProvider`
- `TenantReadStorage`
- `TenantWriteStorage`
- `UsageStorage`

Those traits are still the engine's async boundary. They pass concrete stores
and transactions into closures because the engine uses query planning, durable
journal, schema, scheduler, trigger, and snapshot helpers that are richer than
generic CRUD.

## Focused Capability Traits

The focused capability traits live in `crates/nimbus-storage/src/traits/`:

| Trait | Ownership | Implemented by |
|-------|-----------|----------------|
| `TenantLifecycle` | Tenant discovery, open, create, delete | redb, SQLite, Postgres, MySQL, libSQL replica providers |
| `TenantPointRead` | Point document reads | redb, SQLite, Postgres, MySQL, libSQL replica tenant stores |
| `TenantPointWrite` | Point document writes through the durable commit path | redb, SQLite, Postgres, MySQL, libSQL replica tenant stores |
| `TenantRangeScan` | Table and index scans used by the query planner | redb, SQLite, Postgres, MySQL, libSQL replica tenant stores |
| `DurableJournal` | Recovery, streaming, and bootstrap journal reads | redb, SQLite, Postgres, MySQL, libSQL replica tenant stores |
| `SchedulerStore` | Scheduled work inspection | redb, SQLite, Postgres, MySQL, libSQL replica tenant stores |
| `ControlPlaneUsage` | Cross-tenant usage storage | retained redb usage storage |
| `KeyProviderSurface` | Local database key wrapping and unwrapping | local master-key file, key-directory, and AWS KMS providers when enabled |
| `StorageEngine` | Composite convenience for full tenant data stores | only stores that implement point read/write, range scan, journal, and scheduler |

## Boundaries

Composite traits are allowed only at composition roots that genuinely need the
full tenant store surface. New code should prefer the narrowest trait that
matches its operation.

Static-dispatch async traits may remain `async fn`. Object-safe wrappers are a
separate concern owned by MBA9 and should be added only for traits that are
actually used behind `dyn`.

Local encryption providers are key-provider surfaces, not tenant data
providers. The retained redb control plane is a usage/control surface, not a
tenant data backend.

## Non-Goals

- Do not import ExtendDB's DynamoDB-shaped names such as `TableEngine` or
  `DataEngine`.
- Do not flatten the engine's transaction closures into generic CRUD methods.
- Do not add stub implementations. A backend either implements a focused trait
  because it supports the capability, or it does not implement that trait.
