# Plan: System Tenant and Management API

> Archived 2026-05-15. ST1-ST4 are complete and the non-UI desktop
> prerequisite gate is closed. Keep this file as the historical execution
> record for the `_nimbus` system tenant and management API work; new UI route
> and follow-up API work is owned by `docs/plans/desktop-ui-plan.md`.

Canonical execution plan for making machine, service, and runtime state
observable via the Convex API and manageable via HTTP endpoints. Creates a
reserved `_nimbus` system tenant, wires the machine and service managers to
persist state as documents, and exposes lifecycle actions as HTTP endpoints.
This makes state queryable from any Convex client (`useQuery`, CLI, SDK)
with reactive subscriptions — independently useful regardless of whether a
UI exists.

2026-05-15 desktop design review: the system tenant must also expose the
network and adapter posture required by `DESIGN.md`: route inventory,
listener status, WebSocket subscription state, published ports, server status,
and adapter capability/caveat documents. Without these documents the desktop UI would
collapse back into a machines/data dashboard instead of the intended
compute-storage-network operator console.

Reviewed against:

- `crates/nimbus-server/src/adapters/convex/` — `ConvexRegistry::from_app_dir()`,
  runtime-backed function execution, `ConvexHostBridge`, `ctx.db.*` host ops
- `crates/nimbus-server/src/router.rs` — existing REST routes
  (`/api/tenants/{tenant_id}/documents/{table}`, etc.)
- `crates/nimbus-bin/src/main.rs` — `--app-dir` flag, CLI subcommands
- `crates/nimbus-bin/src/machine/mod.rs` — `MachineManager` state tracking
- `crates/nimbus-bin/src/compose/mod.rs` — Compose-backed service state tracking
- `crates/nimbus-engine/src/service/` — `Service`, tenant runtime, subscription
  delivery, query planning

---

## Status

- **Status:** `archived`
- **Primary owner:** historical record only
- **Activation gate:** completed prerequisite for `docs/plans/desktop-ui-plan.md`
- **Related plans:**
  - `docs/plans/desktop-ui-plan.md` — the UI's React frontend consumes the
    query surface and HTTP endpoints this plan creates
  - `docs/architecture/sandbox/microvm-service-baseline.md` — architecture context for
    machine/service state

## Current Assessed State

- Machine launch/stop mechanics still live in `crates/nimbus-bin/src/machine/`,
  but `nimbus start` now exposes them through a server-owned machine lifecycle
  manager. Server-owned machine and service lifecycle endpoints project state
  into `_nimbus` system tables through `nimbus-server`.
- The Convex adapter supports runtime-backed function execution via
  `ConvexRegistry::from_app_dir()` with `ctx.db.*`, `ctx.scheduler.*`, and
  `ctx.services.*` host calls.
- HTTP endpoints exist for service start/stop/restart and machine
  create/start/stop/restart/update/delete. Local admin token rotation now uses
  the canonical `/api/system/token/rotate` path, and `/api/system/shutdown`
  requests graceful server shutdown. Remaining machine lifecycle gaps include
  any future machine rename decision.
- The REST API already supports per-tenant document browsing at
  `GET /api/tenants/{tenant_id}/documents/{table}`.
- ST1 implementation has started: server startup now bootstraps the reserved
  `_nimbus` tenant and system-table schemas, and the local admin tenant REST
  API rejects user tenants beginning with `_`. Startup also seeds static
  route, listener, adapter capability/caveat posture, and server status
  documents.
- ST2 implementation has started for server-owned sandbox services:
  `SandboxServiceManager` records live service activation/refresh state into
  the `_nimbus.services` table. Server-owned machine lifecycle actions now
  project shared `nimbus-machine` records into `_nimbus.machines`,
  `_nimbus.listeners`, and `_nimbus.ports` after each HTTP transition.
  User-tenant table metadata now projects into `_nimbus.tables` through
  engine-level committed-mutation and table-schema observers instead of
  per-adapter write hooks. This covers REST, Convex, Firebase, MongoDB,
  scheduler/runtime writes, and schema/collection lifecycle changes with
  tenant id, schema, row count, and write timestamp. Row counts use an
  engine-owned materialized/applied read helper that does not record user
  query-planning metrics.
  Scheduler and cron HTTP/Convex control paths now project pending jobs, final
  observed job results, and cron definitions into `_nimbus.scheduled_jobs` and
  `_nimbus.cron_jobs`. Active Convex deployments now project bundle and
  function inventory into `_nimbus.bundles` and `_nimbus.functions`. Convex
  HTTP query, paginated-query, mutation, and action invocations now append
  run records into `_nimbus.runs`. Server startup now records a singleton
  `_nimbus.system_status` document for the Overview and Settings surfaces.
- ST3 implementation has started for server-owned sandbox services and
  machines: tenant-scoped local-admin service start/stop/restart endpoints
  invoke `SandboxServiceManager`, and machine start/stop/restart endpoints
  invoke the configured server machine lifecycle manager. `nimbus start`
  wires that manager to the existing host machine roots and launch/stop
  implementation. Machine create/delete/resource update endpoints now exist;
  `nimbus machine init/start/stop/set/rm` now prefer those live local-server
  endpoints when server discovery is active and fall back to direct host-local
  execution only when no server is running;
  token rotation uses the canonical `/api/system/token/rotate` path, and
  `/api/system/shutdown` requests graceful server shutdown; machine rename is
  still an open product/API decision.
- ST4 implementation has started with a backend-only `packages/nimbus-ui`
  Convex function bundle for typed, bounded `_nimbus` table reads. The bundle
  intentionally has no React UI code yet. The server now embeds that generated
  bundle, loads it as the default `_nimbus` registry on `serve_with_options`,
  and keeps the registry separate from the user application registry and
  application-auth verifier.
- 2026-05-15 desktop readiness review: this plan owns the non-UI data and
  lifecycle prerequisites for the data-backed desktop UI. The localhost
  security and WebSocket protocol prerequisite plans are complete, and the
  server already has a minimal `/ui/*` auth/CSP bootstrap.
- 2026-05-15 UI design-system review: the original 9-table system tenant
  shape was too narrow for the desired UI. Add network and adapter posture
  tables now while the project is pre-launch.
- 2026-05-15 non-UI checkpoint: the ST1-ST4 implementation surfaces are in
  place with focused server, CLI, and `packages/nimbus-ui` verification. The
  CI-shaped runtime lane is green when it skips the dedicated Node-compat
  conformance corpus, and the workspace fallback lane is green outside the
  Codex sandbox. Raw `make test` remains broader than the required product
  gate because it runs runtime-owned Node-compat evidence.

## Control Plan Rules

1. Every lifecycle mutation flows through `Service::apply_mutation` for
   document writes. HTTP endpoints invoke the managers, then the managers
   update system-tenant documents — no bypass.
2. The system tenant name `_nimbus` is reserved. User-created tenants
   must not start with `_`.
3. The read/write path split is intentional: Convex queries for reactive
   reads, HTTP endpoints for lifecycle writes that require host-level
   orchestration.

## Verification Contract

Each roadmap item must satisfy before closing:

- `cargo fmt --all --check` — clean
- `make clippy` — clean
- Required Rust CI shape — green:
  `cargo test -p nimbus-runtime -- --skip runtime::tests::node_compat::`;
  `cargo nextest run --workspace --exclude nimbus-runtime`;
  `cargo test --workspace --exclude nimbus-runtime --doc`. When `nextest`
  is unavailable locally, use
  `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo test --workspace --exclude nimbus-runtime`
  as the fallback proof. The raw `make test` target runs the dedicated
  Node-compat conformance corpus and is broader than the required product
  runtime gate for this plan.
- `npm run build --workspaces --if-present` — green
- `npm run test --workspaces --if-present` — green
- Manual verification described per item

## Architecture

### Read/write path split

- **Read path**: Convex queries on the `_nimbus` system tenant → reactive
  subscriptions fire automatically when documents change. Any Convex client
  (`useQuery`, HTTP query, CLI) gets live state.
- **Write path**: Machine/service lifecycle actions (start, stop, restart,
  delete) require host-level orchestration (spawning VMs, managing
  processes) beyond what `ctx.db` can do. These are HTTP endpoints on
  `nimbus-server` that invoke the managers directly. The managers update
  system-tenant documents on completion, triggering reactive subscription
  updates automatically.
- **Cross-tenant data browsing**: The system tenant's Convex functions can
  only access `_nimbus` documents via `ctx.db`. Browsing user-tenant
  documents uses the existing REST API
  (`GET /api/tenants/{tenant_id}/documents/{table}`) directly.

### Machine lifecycle ownership boundary

The current `nimbus machine ...` implementation lives in `nimbus-bin` and
persists host-local machine records under the user's config/state/data roots.
That CLI path must not become a direct writer to the server-owned `_nimbus`
tenant: doing so would make a one-shot CLI process compete with the running
server for control-plane storage and would leave the desktop UI without a
single lifecycle authority.

The next safe boundary is:

- extract the machine control model (`MachineConfigRecord`,
  `MachineStateRecord`, provider capability facts, and render-independent
  lifecycle result types) into the reusable `nimbus-machine` library boundary
- let `nimbus-server` own machine lifecycle endpoints and `_nimbus` writes for
  UI/server-driven operations
- keep `nimbus-bin` CLI commands as clients of that lifecycle service when a
  local server is running, with direct host-local execution only as a
  deliberate offline/repair path
- keep machine state projection idempotent: every machine transition writes
  `machines`, `ports`, `listeners`, and `events` through the engine mutation
  path owned by the server process

This mirrors the already-landed service-control ownership: `nimbus-server`
owns live service activation and system-tenant state; `nimbus-bin` wires CLI
and startup ergonomics around that server-owned control plane.

2026-05-15 extraction note: `crates/nimbus-machine` now owns the shared
machine record/provider/runtime-state model. `nimbus-bin` still owns the CLI,
rendering, helper resolution, and launch manager, but it consumes these
records through the library boundary. `nimbus-server` now depends on the same
model and has a projection writer for `_nimbus.machines`, `_nimbus.listeners`,
and `_nimbus.ports`. `nimbus-server` also owns a machine lifecycle trait and
local-admin start/stop/restart endpoints; `nimbus-bin` installs a host-backed
adapter during `nimbus start` so those endpoints reuse the existing machine
record locks and launch/stop implementation. Machine create, delete,
resource-change, and CLI lifecycle preference now use the same server-owned
control plane when a local server is discoverable, with direct CLI execution
kept as the offline/repair path. The remaining lifecycle design decision is
whether machine rename should exist before launch.

### System tenant tables

| Table | Key fields | Purpose |
| --- | --- | --- |
| `machines` | name, kind, state, provider, resources, meta | Machine inventory |
| `services` | tenantId, name, machineId, bundleId, kind, state, endpoints, health | Service registry |
| `bundles` | sha256, sizeBytes, sourceRef, status | Deployed bundles |
| `functions` | bundleId, path, kind, argsSchema, returnsSchema | Per-bundle function registry |
| `tables` | tenantId, name, schema, rowCount, lastWriteAt | User-data table directory |
| `events` | source, level, category, message, data, correlationId | Event firehose |
| `runs` | bundleId, functionPath, kind, durationMs, status, error | Runtime invocations |
| `scheduled_jobs` | tenantId, functionPath, scheduledTime, status, args, result | Scheduled jobs |
| `cron_jobs` | tenantId, name, schedule, functionPath, lastRunAt, nextRunAt, status | Cron job definitions |
| `routes` | method, path, adapter, handler, authRequired, lastRequestAt | HTTP and adapter route inventory |
| `listeners` | adapter, protocol, address, state, version, error | REST/WS/MongoDB/Firebase/machine listener status |
| `subscriptions` | tenantId, adapter, queryKey, clientCount, lastDeliveryAt, error | Live WebSocket and adapter subscription status |
| `ports` | machineId, serviceId, hostPort, guestPort, protocol, state | Published service and machine API ports |
| `adapter_capabilities` | adapter, feature, status, caveat, evidence | Supported/caveated/not-claimed adapter capability posture |
| `system_status` | name, version, health, startedAt, updatedAt, details | Server status singleton for Overview and Settings |

### Query surface (Convex functions)

```
machines.list({ filter? })                    → Machine[]
machines.byId({ id })                         → Machine | null
services.list({ tenantId?, machineId?, state? }) → Service[]
services.byId({ id })                         → Service | null
bundles.list()                                → Bundle[]
functions.list({ bundleId })                  → FunctionEntry[]
tables.list({ tenantId?, limit? })            → TableSummary[]
tables.byName({ tenantId, name })             → TableSummary | null
tables.withIndexes({ tenantId, name })        → TableSummary + IndexSummary[] (derived or future index table)
tables.browse(...)                            → via REST: GET /api/tenants/{id}/documents/{table} (cross-tenant)
events.recent({ filter?, limit })             → Event[]
events.byCorrelationId({ correlationId })     → Event[]
runs.recent({ filter?, limit })               → Run[]
runs.byId({ id })                             → Run | null
scheduled_jobs.list({ tenantId?, status? })    → ScheduledJob[]
cron_jobs.list({ tenantId? })                 → CronJob[]
routes.list({ adapter? })                     → RouteEntry[]
listeners.list({ adapter? })                  → ListenerEntry[]
subscriptions.list({ tenantId?, adapter? })   → SubscriptionEntry[]
ports.list({ machineId?, serviceId? })        → PortEntry[]
adapter_capabilities.list({ adapter? })       → AdapterCapability[]
system.status()                               → { startedAt, version, health, details }  (query)
```

### HTTP lifecycle endpoints (new)

```
POST   /api/tenants/{tenant_id}/services/{service_name}/start
POST   /api/tenants/{tenant_id}/services/{service_name}/stop
POST   /api/tenants/{tenant_id}/services/{service_name}/restart
POST   /api/machines/{name}/start
POST   /api/machines/{name}/stop
POST   /api/machines/{name}/restart
POST   /api/machines/{name}/create
DELETE /api/machines/{name}
PATCH  /api/machines/{name}
POST   /api/system/token/rotate
POST   /api/system/shutdown
```

### Write surface

```
POST   /api/admin/deploy                         → bundle promotion / activation
PUT    /api/tenants/{tenant_id}/schema/{table}   → table schema create/update
DELETE /api/tenants/{tenant_id}/schema/{table}   → table schema removal
POST   /api/tenants/{tenant_id}/documents        → document insert
PATCH  /api/tenants/{tenant_id}/documents/{table}/{document_id}
DELETE /api/tenants/{tenant_id}/documents/{table}/{document_id}
```

### Required UI follow-up surfaces

These surfaces are not blockers for the completed non-UI prerequisite gate, but
they must be planned before the React route tree freezes:

```
GET    /api/tenants/{tenant_id}/indexes/{table}
POST   /api/tenants/{tenant_id}/indexes/{table}
DELETE /api/tenants/{tenant_id}/indexes/{table}/{index_name}
POST   /api/tenants/{tenant_id}/functions/{path}/invoke   # optional generic wrapper only if existing adapter invoke routes are not enough
```

- Index UI may start read-only by deriving implemented indexes from
  `tables.schema`, but create/drop needs an explicit local-admin REST API
  before DU7 claims index management.
- `events.byCorrelationId` is required for Runs and Function Runner drill-down.
- The Function Runner itself is required by `docs/plans/desktop-ui-plan.md`
  DU6.5. Only the generic wrapper endpoint above is optional; the UI may use
  existing Convex, HTTP route, Cloud Functions, or native invoke endpoints when
  they provide a stable request/result/correlation contract.
- `_nimbus.runs` currently records Convex HTTP query, paginated-query,
  mutation, and action invocations. Native HTTP document operations,
  scheduler executions, MongoDB wire operations, Firebase REST/gRPC operations,
  and Cloud Functions invocations need follow-up run recording before the
  Observability UI can honestly claim cross-adapter run history. Until that
  work is complete, Runs UI copy must label the coverage boundary rather than
  presenting a Convex-only stream as all Nimbus activity.

Do not add `_nimbus` Convex mutations that write user-tenant data or perform
host-level lifecycle work. The system tenant Convex bundle is the typed
reactive read surface. Writes that cross tenant boundaries, alter deployments,
or orchestrate machines/services stay on local-admin HTTP endpoints and can be
wrapped by the JS SDK/UI later without bypassing `Service`.

## Roadmap

### ST1 — System tenant creation and schema

Create the `_nimbus` system tenant automatically on server startup.
Define table schemas for all system tables, including network and adapter
posture tables. Reserve the `_` prefix for system tenants — reject
user-created tenants starting with `_`.

**Verification:** (a) `_nimbus` tenant exists after `nimbus start` starts,
(b) machine/service/runtime/network/adapter tables are defined with correct
schemas, (c) `POST /api/tenants` with `_` prefix name → 400 rejected.

**Status:** `in_progress`

**Current evidence:** `cargo test -p nimbus-server system_tenant --lib`
passed 12/12 focused tests, covering schema validity, idempotent `_nimbus`
bootstrap, seeded route/listener/adapter posture and system status documents,
live subscription state document projection, reserved-prefix rejection, system
Convex dispatch, and local admin tenant-list hiding.
`cargo test -p nimbus-server
core_http::tenants --lib` passed 6/6 tenant HTTP tests.
`cargo test -p nimbus-server --lib` passed 654/663 tests with 9 ignored.
Full workspace verification remains required before closing ST1.

### ST2 — Machine and service state persistence

Wire the machine manager to write machine state as documents in the
`_nimbus` system tenant on every state transition (init, start, stop,
delete, error). Wire the service manager similarly for service state
changes, health updates, published ports, and machine API reachability.

Do not implement machine writes by reaching from `nimbus-bin` into server
storage. Complete the machine lifecycle extraction boundary above first so
the server can own lifecycle endpoints and `_nimbus` mutation authority.

**Verification:** (a) `nimbus machine init` + `nimbus machine start`
creates documents in the `machines` table, (b) machine stop updates the
document state field, (c) `ctx.db.query("machines")` returns current
state from a Convex function on the system tenant, (d) document changes
trigger reactive subscription updates, (e) published ports and machine API
listener state are reflected in `ports` and `listeners`.

**Status:** `in_progress`

**Current evidence:** `cargo test -p nimbus-server service_manager --lib`
passed 7/7 tests, including proof that `SandboxServiceManager` activation
records a tenant-scoped `services` document in `_nimbus` with ready state,
backend health, and published endpoint data, and that stopping a service
projects stopped state with no live endpoints or stale service port
documents. Service endpoints now also project linked `_nimbus.ports` rows
with host port, guest port when known, protocol, and lifecycle state.
`cargo test -p nimbus-server
--lib` passed 654/663 tests with 9 ignored after the projection path was made
robust for direct router tests that activate services without the full serve
bootstrap.
`cargo test -p nimbus-server system_tenant --lib` passed 9/9 tests, including
machine state projection into `machines`, `listeners`, and `ports` from the
shared `nimbus-machine` record model. A later slice added machine lifecycle
HTTP projection proof: `cargo test -p nimbus-server machine_lifecycle --lib`
passed 2/2, covering manager invocation plus start/stop projection into
`machines`, `listeners`, and `ports`. Convex WebSocket subscription
registration and cleanup now project live `_nimbus.subscriptions` documents,
including a reactive `_nimbus` table subscription proof:
`cargo test -p nimbus-server websocket_protocol --lib` passed 6/6. Router
system-tenant preparation now records enabled Convex, Firebase, and Cloud
Functions listener posture, and MongoDB startup records its dedicated TCP
listener after bind. User-tenant schema changes and committed document
mutations now update `_nimbus.tables` through engine observers rather than
REST-only hooks; the focused proof sets a schema, inserts a document, reads
the row back through `tables:byName` and `tables:list`, verifies direct
reactive `_nimbus.tables` updates, and verifies the system row is removed
after deleting the document and schema. Scheduled job and cron control-plane
writes now update `_nimbus.scheduled_jobs` and
`_nimbus.cron_jobs`; pending scheduled jobs are removed on cancel, cron rows
are removed on delete, and fetching a completed scheduled-job result records
the final outcome payload.

### ST3 — HTTP lifecycle endpoints

Add HTTP endpoints on `nimbus-server` for machine/service lifecycle actions
and system operations. Endpoints invoke `MachineManager` /
`ServiceManager` directly, then the managers update system-tenant documents
on completion.

**Verification:** (a) `POST /api/machines/{name}/start` starts the machine,
(b) system-tenant `machines` document reflects the new state, (c)
reactive subscription fires on state change, (d) error responses use the
structured error schema from `docs/plans/archive/websocket-protocol-plan.md`.

**Status:** `in_progress`

**Current evidence:** Tenant-scoped service lifecycle endpoints are implemented
under local-admin routing:
`POST /api/tenants/{tenant_id}/services/{service_name}/start`,
`/stop`, and `/restart`. `cargo test -p nimbus-server service_manager --lib`
passed 7/7 tests, including HTTP start/stop proof against the local-admin
router, backend start/stop call counts, JSON response shape, and
`_nimbus.services` ready/stopped projection. `cargo test -p nimbus-server
--lib` passed 643/652 tests with 9 ignored. Machine start/stop/restart
endpoints are now implemented under local-admin routing and backed by the
configured server machine lifecycle manager; `nimbus start` provides a
host-backed manager adapter. `cargo test -p nimbus-server machine_lifecycle
--lib` passed 2/2, covering start/stop response shape, manager call ordering,
`_nimbus.machines`, `_nimbus.listeners`, and `_nimbus.ports` projection, and
404 behavior when no machine manager is configured. `cargo test -p
nimbus-server local_server_security --lib` passed 10/10 and now covers
local-admin protection for machine lifecycle routes before manager dispatch.
Machine and service lifecycle endpoints now append `_nimbus.events` records
for operator/audit evidence; `cargo test -p nimbus-server machine_lifecycle
--lib` and `cargo test -p nimbus-server service_manager --lib` both assert
those event documents. Machine create/delete/resource update endpoints now
exist and are covered by `cargo test -p nimbus-server machine_lifecycle
--lib`. Standalone `nimbus machine init/start/stop/set/rm` commands now
prefer the running local server's local-admin machine endpoints when server
discovery is live, so the server remains the `_nimbus` mutation authority;
they fall back to direct host-local execution only when no server is running.
Local admin token rotation now uses `/api/system/token/rotate` from
both server routes and the live CLI rotation path. `/api/system/shutdown`
requests the server's graceful shutdown signal and records an audit/system
event. Machine lifecycle reactive proof now subscribes directly to
`_nimbus.machines` and observes start/stop updates from the HTTP lifecycle
routes. Machine rename remains an open product/API decision before ST3
closes.

### ST4 — Convex function bundle and codegen

Implement the Convex function bundle at `packages/nimbus-ui/convex/` with
query functions for all system tables. Generate typed function refs via
`@nimbus/codegen`. Configure the server to install this bundle as the
`_nimbus` system registry. The serving path must keep that registry separate
from the user application registry and must protect `_nimbus` Convex calls
with the local server access contract, not application JWT auth. Do not add
system-tenant Convex mutations for cross-tenant document writes or host-level
lifecycle work; those remain HTTP writes.

**Verification:** (a) `useQuery(api.machines.list)` returns current machine
state from a React app, (b) `useQuery(api.scheduled_jobs.list)` returns
scheduled jobs, (c) `useQuery(api.listeners.list)` and
`useQuery(api.adapter_capabilities.list)` return network and adapter posture,
(d) typed function refs compile cleanly, (e) `npm run build` succeeds.

**Status:** `in_progress`

**Current evidence:** `npm run test --workspace nimbus-ui` passed, including
`convex codegen --app .` and `tsc -p tsconfig.json --noEmit`, for the
backend-only `_nimbus` query bundle. `npm run build --workspace nimbus-ui`
also passed. The first slice covers bounded indexed read queries for every
system table. The query handlers are runtime-self-contained so the generated
bundle does not depend on top-level TypeScript helpers after manifest
extraction. A later server slice added `ServeOptions::with_system_convex_registry`
and `_nimbus` dispatch through a system-only registry with local-admin access
checks; `cargo test -p nimbus-server system_tenant_convex_routes --lib` passed
2/2, `cargo test -p nimbus-server local_server_security --lib` passed 10/10,
`cargo test -p nimbus-server system_tenant --lib` passed 9/9, and
`cargo test -p nimbus-server --lib` passed 644/653 tests with 9 ignored.
The packaged bundle now autoloads from embedded artifacts by default:
`cargo test -p nimbus-server
serve_with_options_loads_embedded_system_convex_registry_by_default --lib`
passed 1/1 and proves `routes:list` works through `/convex/_nimbus/query`
without manual registry injection. The Convex schema manifest bridge now
preserves multi-field indexes, so `_nimbus.tables.byName` can use the
`by_tenantId_and_name` composite index while still loading through the
embedded system registry. Focused verification:
`cargo test -p nimbus-server
convex_schema_manifest_preserves_composite_indexes --lib` passed 1/1 and
`cargo test -p nimbus-server core_http::schema --lib` passed 3/3.
Active Convex deploy inventory now projects into `_nimbus.bundles` and
`_nimbus.functions`; `cargo test -p nimbus-server deploy --lib` passed 8/8
and includes proof that deploy activation replaces stale function inventory
with the new generation's function list. The deploy proof also verifies
`_nimbus.runs` records a successful `notes:list` query with duration and
start timestamp.
The `packages/nimbus-ui/src/system-query-proof.tsx` headless React proof now
typechecks `NimbusProvider`, `useQuery`, `useQueries`, and
`useNimbusConnectionState` against generated `_nimbus` API refs for machines,
scheduled jobs, listeners, adapter capabilities, and system status. The
earlier document-level mutation idea has been superseded by the read/write split:
cross-tenant document/schema writes, deploy promotion, and lifecycle writes
stay on HTTP endpoints.

## Immediate Non-UI Implementation Checklist

This is the concrete non-UI work to finish before DU1 real UI implementation:

1. ST1 is closed for the desktop prerequisite gate: startup bootstraps
   `_nimbus`, reserved user-tenant names are rejected, and the CI-shaped Rust
   verification lanes are green. A future `nimbus start` smoke can still be
   added as release-level evidence, but the API surface no longer depends on
   it.
2. Implement ST2 machine/service state writers in the owning managers. Every
   machine lifecycle transition must upsert `machines`, `ports`, `listeners`,
   and `events` documents through the engine mutation path. For machine
   lifecycle this requires the server-owned lifecycle boundary described
   above; standalone CLI lifecycle commands now use the running server when
   available and only execute directly as an offline/repair path.
3. Extend the seeded adapter posture writers from static route/listener/
   capability documents to live network posture. Convex WebSocket
   subscription documents and service-published-port documents now exist;
   Firebase/Convex/Cloud Functions HTTP listener posture and MongoDB TCP
   listener posture now record on server preparation/startup.
4. Finish ST3 HTTP lifecycle endpoints. Service start/stop/restart and
   machine create/start/stop/restart/update/delete now exist; token rotation
   is canonical at `/api/system/token/rotate`; shutdown exists at
   `/api/system/shutdown`; CLI lifecycle commands prefer the live server when
   available. Machine rename remains a product/API decision before launch if
   we decide it is needed.
5. Finish ST4 by keeping the generated refs proven through the headless React
   hook harness. The backend query bundle, server-side system-registry
   dispatch lane, packaged embedded autoload path, and `useQuery`/`useQueries`
   type proof now exist. The scheduler, system status, run history, and table
   directory query surfaces now have backend query-path proof. Cross-tenant
   document/schema writes and deploy promotion remain HTTP writes by design.
6. Add verification hooks that prove reactive `_nimbus` queries update when
   machine/service/network documents change. The first proof now covers
   `_nimbus.subscriptions` updates from a real Convex WebSocket subscribe and
   unsubscribe flow. Table metadata is now proven queryable through the
   packaged `_nimbus` Convex query surface, reactive through direct
   `_nimbus.tables` subscription updates, and updated from non-REST Convex
   mutation writes through the central engine observer seam.

## Execution Log

| Date | Item | Status | Notes |
| --- | --- | --- | --- |
| 2026-04-18 | Plan authored | — | Extracted from desktop-ui-plan.md as prerequisite |
| 2026-05-15 | Desktop design-system alignment | — | Expanded system tenant scope with network and adapter posture tables so the UI can implement the `DESIGN.md` Compute/Storage/Network model instead of only machine/service/data tabs. |
| 2026-05-15 | ST1 implementation slice | in progress | Added server startup `_nimbus` bootstrap, schemas for all 14 system tables, local admin REST reserved-prefix enforcement, and seeded static route/listener/adapter posture documents. Focused verification: `cargo test -p nimbus-server system_tenant --lib` 6/6 passed; `cargo test -p nimbus-server core_http::tenants --lib` 6/6 passed; `cargo test -p nimbus-server --lib` 639/648 passed with 9 ignored; `cargo fmt --all --check` and `git diff --check` passed after formatting. |
| 2026-05-15 | ST2 service-state first slice | in progress | Attached the server-owned `SandboxServiceManager` to `_nimbus` state persistence and recorded live sandbox service activation/refresh into the `services` table. Made projection idempotently ensure the system tenant so direct router tests and serve bootstrap both converge. Focused verification: `cargo test -p nimbus-server service_manager --lib` 6/6 passed; `cargo test -p nimbus-server --lib` 639/648 passed with 9 ignored. |
| 2026-05-15 | ST4 query-bundle first slice | in progress | Added private `packages/nimbus-ui` workspace with Convex schema and backend-only query modules for the `_nimbus` system tenant. Public query args use explicit nullable filters so generated function refs remain JSON-shaped and stable. Focused verification: `npm run test --workspace nimbus-ui` passed, including codegen and TypeScript typecheck; `npm run build --workspace nimbus-ui` passed. |
| 2026-05-15 | ST4 system registry dispatch slice | in progress | Added a separate server-owned system Convex registry path for `_nimbus` so system functions cannot fall through to the user application registry or application JWT verifier. `_nimbus` Convex calls use local server access when configured and remain callable in direct-router tests without local security. Focused verification: `cargo test -p nimbus-server system_tenant_convex_routes --lib` 2/2 passed; `cargo test -p nimbus-server local_server_security --lib` 10/10 passed; `cargo test -p nimbus-server system_tenant --lib` 8/8 passed; `cargo test -p nimbus-server --lib` 641/650 passed with 9 ignored. |
| 2026-05-15 | Machine lifecycle ownership review | in progress | Reviewed `crates/nimbus-bin/src/machine/*`, `docs/architecture/sandbox/macos-machine-flow.md`, and `docs/architecture/sandbox/microvm-service-baseline.md`. Decision: do not add direct `_nimbus` writes to CLI machine commands. The scalable path is to extract render-independent machine lifecycle types and let `nimbus-server` own machine lifecycle endpoints plus system-tenant projection, while the CLI becomes a client of that server path when available. |
| 2026-05-15 | Machine record model extraction | in progress | Added `crates/nimbus-machine` as the reusable owner for machine config/state/runtime records, provider capability facts, root/path resolution, and volume/image parsing. `nimbus-bin` now consumes those records through its existing machine module while retaining CLI/render/helper/launch ownership. Focused verification: `cargo check -p nimbus-machine -p nimbus-bin` passed; `cargo test -p nimbus-machine` passed 0/0; `cargo test -p nimbus-bin machine::tests::records_state --bin nimbus` passed 16/16; `cargo test -p nimbus-bin machine::manager::tests::helper_resolution --bin nimbus` passed 7/7; `cargo test -p nimbus-bin machine --bin nimbus` passed 180/180. |
| 2026-05-15 | Machine state projection writer | in progress | Added server-side projection from shared `nimbus-machine` records into `_nimbus.machines`, `_nimbus.listeners`, and `_nimbus.ports`. This gives future server-owned lifecycle endpoints a concrete `_nimbus` write contract without making CLI machine commands write system documents directly. Focused verification: `cargo test -p nimbus-server system_tenant --lib` 9/9 passed; `cargo test -p nimbus-server --lib` 642/651 passed with 9 ignored. |
| 2026-05-15 | ST3 service lifecycle endpoints | in progress | Added tenant-scoped local-admin service start/stop/restart routes backed by the server-owned `SandboxServiceManager`. The manager now exposes explicit lifecycle methods and projects stopped service state into `_nimbus.services`; the system table schema and `packages/nimbus-ui` query bundle now include `tenantId` on services. Focused verification: `cargo fmt --all --check` passed; `npm run test --workspace nimbus-ui` passed; `npm run build --workspace nimbus-ui` passed; `cargo test -p nimbus-server service_manager --lib` passed 7/7; `cargo test -p nimbus-server system_tenant --lib` passed 9/9; `cargo test -p nimbus-server --lib` passed 643/652 with 9 ignored. |
| 2026-05-15 | ST4 packaged system bundle autoload | in progress | Embedded the generated `packages/nimbus-ui/.nimbus/convex` artifacts into `nimbus-server`, materialized them into a guarded tempdir for path-backed runtime integrity checks, and made `ServeOptions` load the packaged `_nimbus` registry by default. Public `serve*` helpers now route through `ServeOptions` so embedders get the same system-registry default. The Convex schema manifest bridge now accepts generated `array.element` and `object.fields` validator keys, and `_nimbus` query handlers no longer depend on top-level helper capture. Focused verification: `npm run test --workspace nimbus-ui` passed; `npm run build --workspace nimbus-ui` passed; `cargo test -p nimbus-server serve_with_options_loads_embedded_system_convex_registry_by_default --lib` passed 1/1; `cargo test -p nimbus-server system_tenant --lib` passed 9/9; `cargo test -p nimbus-server service_manager --lib` passed 7/7; `cargo test -p nimbus-server --lib` passed 644/653 with 9 ignored. |
| 2026-05-15 | ST3 machine lifecycle endpoints | in progress | Added a `nimbus-server` machine lifecycle trait plus local-admin `POST /api/machines/{name}/{start,stop,restart}` endpoints. The endpoints invoke the configured manager, then project the returned shared machine records into `_nimbus.machines`, `_nimbus.listeners`, and `_nimbus.ports`. `nimbus-bin` now installs a host-backed adapter during `nimbus start`, reusing the existing machine roots, record locks, and launch/stop implementation through blocking worker tasks. Focused verification: `cargo check -p nimbus-server` passed; `cargo check -p nimbus-bin` passed; `cargo test -p nimbus-server machine_lifecycle --lib` passed 2/2; `cargo test -p nimbus-server local_server_security --lib` passed 10/10; `cargo test -p nimbus-server system_tenant --lib` passed 9/9; `cargo test -p nimbus-bin machine::tests::records_state --bin nimbus` passed 16/16; `cargo test -p nimbus-bin machine::tests::startup_failures --bin nimbus` passed 5/5. |
| 2026-05-15 | ST2/ST3 lifecycle events | in progress | Added append-only `_nimbus.events` recording for machine and service HTTP lifecycle actions. Events include source, category, action, state, correlation id, and createdAt so the future operator UI can show a durable activity stream without scraping logs. Focused verification: `cargo test -p nimbus-server machine_lifecycle --lib` passed 2/2; `cargo test -p nimbus-server service_manager --lib` passed 7/7; `cargo test -p nimbus-server system_tenant --lib` passed 9/9. |
| 2026-05-15 | ST3 machine create/update/delete endpoints | in progress | Extended the server machine lifecycle trait and local-admin API with `POST /api/machines/{name}/create`, `PATCH /api/machines/{name}`, and `DELETE /api/machines/{name}`. Create and update project `_nimbus.machines`, `_nimbus.listeners`, `_nimbus.ports`, and lifecycle events. Delete removes machine/listener/port projection documents and appends a delete event. `nimbus-bin` maps these calls to existing init/set/rm helpers without changing standalone CLI rendering. Focused verification: `cargo check -p nimbus-bin` passed; `cargo test -p nimbus-server machine_lifecycle --lib` passed 3/3; `cargo test -p nimbus-server system_tenant --lib` passed 9/9; `cargo test -p nimbus-server local_server_security --lib` passed 10/10; `cargo test -p nimbus-bin machine::tests::records_state --bin nimbus` passed 16/16. |
| 2026-05-15 | ST2 network subscription state | in progress | Added server-owned `_nimbus.subscriptions` projection for Convex WebSocket subscriptions. Subscribe creates a live document with tenant, adapter, query key, client count, and delivery timestamp; unsubscribe/disconnect removes it. Delivery timestamp refresh skips `_nimbus` tenant subscriptions to avoid self-triggering subscription-table churn. Focused verification: `cargo test -p nimbus-server record_subscription_state_projects_live_subscription_document --lib` passed 1/1; `cargo test -p nimbus-server convex_websocket_subscription_projects_live_system_subscription_state --lib` passed 1/1; `cargo test -p nimbus-server convex_websocket_runtime_subscription_uses_server_generated_request_correlation_ids --lib` passed 1/1; `cargo test -p nimbus-server system_tenant --lib` passed 10/10; `cargo test -p nimbus-server websocket_protocol --lib` passed 6/6. |
| 2026-05-15 | ST2 service port projection | in progress | Added `_nimbus.ports` projection for sandbox service published endpoints. Service start/upsert removes stale rows for the service, writes current endpoint rows with host port, protocol, state, and guest port when known, and service stop removes the stale endpoint rows. `PublishedEndpoint` now carries optional guest-port metadata from container/krun port bindings instead of guessing. Focused verification: `cargo test -p nimbus-server service_manager --lib` passed 7/7; `cargo test -p nimbus-server system_tenant --lib` passed 10/10; `npm run test --workspace nimbus-ui` passed; `npm run build --workspace nimbus-ui` passed. |
| 2026-05-15 | ST2 adapter listener posture | in progress | Added reusable listener-state projection and router preparation records for enabled Convex, Firebase, and Cloud Functions HTTP surfaces, plus MongoDB TCP listener projection after binding the dedicated MongoDB socket. Focused verification: `cargo test -p nimbus-server router_prepare_system_tenant_records_enabled_adapter_listeners --lib` passed 1/1. |
| 2026-05-15 | ST2/ST4 table metadata projection | in progress | Added `_nimbus.tables` projection for user-tenant schema and document writes. Schema set/delete and document insert/update/delete now refresh tenant-scoped table metadata with schema, row count, and write timestamp, deleting the system row once both schema and rows are gone. The packaged `tables:list` and `tables:byName` queries now include tenant filtering, and the Convex manifest bridge preserves composite indexes so `by_tenantId_and_name` loads through the embedded registry. The focused proof now subscribes to `_nimbus.tables`, observes the active row-count projection, verifies packaged `tables:byName`/`tables:list`, and observes cleanup after deleting the last row plus schema. Focused verification: `npm run test --workspace nimbus-ui` passed; `cargo test -p nimbus-server schema_and_document_writes_project_table_state_into_system_tenant --lib` passed 1/1; `cargo test -p nimbus-server convex_schema_manifest_preserves_composite_indexes --lib` passed 1/1; `cargo test -p nimbus-server system_tenant --lib` passed 12/12; `cargo test -p nimbus-server core_http::schema --lib` passed 3/3. |
| 2026-05-15 | ST2 diagnostics-neutral table row counts | in progress | Reworked `_nimbus.tables.rowCount` to call an engine-owned materialized/applied table count helper instead of `list_documents_async`, so internal system metadata projection no longer increments tenant query-planning metrics. Focused verification: `cargo test -p nimbus-server tenant_engine_metrics_route_surfaces_worker_and_serving_health_after_mixed_traffic --lib` passed 1/1 with the expected single application full-scan; `cargo test -p nimbus-server system_tenant --lib` passed 12/12; `cargo test -p nimbus-server core_http::schema --lib` passed 2/2; `cargo test -p nimbus-server serve_with_options_loads_embedded_system_convex_registry_by_default --lib` passed 1/1; `cargo fmt --all --check` passed. |
| 2026-05-15 | Broad server verification checkpoint | in progress | Re-ran the full `nimbus-server` library suite after the diagnostics-neutral row-count fix. Verification: `cargo test -p nimbus-server --lib` passed 653/662 tests with 9 ignored; `git diff --check` passed. |
| 2026-05-15 | ST2 centralized table projection observers | in progress | Replaced REST document/schema projection hooks with engine-level committed-mutation and table-schema observers. `nimbus-server` installs one idempotent `_nimbus.tables` projector per service, skips reserved tenants to avoid recursion, and serializes projection work so adjacent document/schema changes cannot resurrect stale table rows. Added proof that a Convex mutation updates `_nimbus.tables` through the shared committed-write seam, plus packaged `tables:byName`/`tables:list` and direct reactive subscription proofs. Verification: `cargo fmt --all --check` passed; `cargo test -p nimbus-server core_http::schema --lib` passed 3/3; `cargo test -p nimbus-server system_tenant --lib` passed 12/12; `cargo test -p nimbus-server convex_mutation_dispatches_existing_document_operations --lib` passed 1/1; `cargo test -p nimbus-engine --lib` passed 266/268 with 2 ignored; `cargo test -p nimbus-server --lib` passed 654/663 with 9 ignored. |
| 2026-05-15 | ST2/ST4 scheduler state projection | in progress | Added `_nimbus.scheduled_jobs` and `_nimbus.cron_jobs` projection for scheduler and cron control-plane paths. REST scheduling, Convex scheduling endpoints, async Convex scheduler host calls, cancel/delete paths, and scheduled-job history reads now sync or update the system rows. Server system-tenant preparation also syncs existing user-tenant scheduler state on startup. Focused verification: `cargo test -p nimbus-server schedule_endpoint_returns_job_id_and_lists_pending_job --lib` passed 1/1; `cargo test -p nimbus-server cancel_scheduled_job_endpoint_removes_pending_job --lib` passed 1/1; `cargo test -p nimbus-server cron_endpoints_create_list_and_delete_jobs --lib` passed 1/1; `cargo test -p nimbus-server scheduled_job_history_endpoint_reports_failures --lib` passed 1/1; `cargo test -p nimbus-server convex_cancel_scheduled_job_removes_pending_named_mutation --lib` passed 1/1; `cargo test -p nimbus-server scheduling --lib` passed 12/12. |
| 2026-05-15 | ST4 deploy inventory projection | in progress | Added active Convex deployment projection into `_nimbus.bundles` and `_nimbus.functions`. Server preparation records startup registries; deploy activation records `deploy:generation:N` source refs, computes a stable bundle sha from the runtime bundle or registry summary, upserts active function inventory, and removes stale active bundle/function rows. Focused verification: `cargo test -p nimbus-server deploy_activation_swaps_new_requests_to_new_generation --lib` passed 1/1; `cargo test -p nimbus-server deploy --lib` passed 8/8. |
| 2026-05-15 | ST4 run history projection | in progress | Added append-only `_nimbus.runs` recording for Convex HTTP query, paginated-query, mutation, and action routes. Run records include function path, kind, status, duration, start timestamp, and optional error object. `_nimbus` tenant reads are skipped to avoid the operator UI observing itself into noisy run history. Focused verification: `cargo test -p nimbus-server deploy_activation_swaps_new_requests_to_new_generation --lib` passed 1/1 and asserts a successful `notes:list` query appears in `runs:recent`. |
| 2026-05-15 | ST4 system status query | in progress | Added `_nimbus.system_status` as the server status singleton and exposed it through the packaged `system:status` Convex query. The document records health, Nimbus server version, startup timestamp, update timestamp, and listen-address details so Overview and Settings can read process status through the same system-tenant path as the rest of the operator surface. Focused verification: `npm run test --workspace nimbus-ui` passed; `npm run build --workspace nimbus-ui` passed; `cargo test -p nimbus-server system_tenant --lib` passed 12/12; `cargo test -p nimbus-server serve_with_options_loads_embedded_system_convex_registry_by_default --lib` passed 1/1 and asserts `system:status` resolves from the embedded bundle. |
| 2026-05-15 | ST3 canonical token rotation route | in progress | Renamed the live local-admin token rotation route from `/api/admin/token/rotate` to `/api/system/token/rotate` and updated the CLI live rotation client to use the same UI-facing system path. Focused verification: `cargo test -p nimbus-server local_admin --lib` passed 11/11; `cargo test -p nimbus-server local_ui --lib` passed 4/4; `cargo test -p nimbus-bin token --bin nimbus` passed 5/5. |
| 2026-05-15 | ST3 graceful shutdown route | in progress | Added a server-owned graceful shutdown signal to `serve_with_options` and exposed `POST /api/system/shutdown` through the local-admin router. The handler records a best-effort `_nimbus.events` lifecycle entry and local audit record before requesting shutdown. Focused verification: `cargo test -p nimbus-server local_admin --lib` passed 12/12, including a live-server proof that the endpoint returns `{ accepted: true }` and the serve task exits cleanly; `cargo test -p nimbus-server system_tenant --lib` passed 12/12 with the new route inventory. |
| 2026-05-15 | ST4 read/write split correction | in progress | Corrected the remaining ST4 wording so `_nimbus` Convex functions stay a typed reactive read surface. Cross-tenant document/schema writes, deploy activation, and machine/service lifecycle operations remain on local-admin HTTP endpoints and can be wrapped by the SDK/UI later without bypassing `Service`. |
| 2026-05-15 | ST3 CLI lifecycle server preference | in progress | Added a shared local-server HTTP client for CLI control-plane calls and made `nimbus machine init/start/stop/set/rm` prefer the running local server's local-admin machine endpoints when discovery is live. The create request now carries the same init-time fields as the CLI (`image`, SSH identity, Ignition override, bootc-native flag, EFI store, volumes, CPU/memory/disk), so the server path is not a weaker substitute. Direct host-local execution remains the fallback only when no live server is discoverable. Machine lifecycle tests now also subscribe to `_nimbus.machines` and prove start/stop routes publish reactive state updates. Verification: `cargo check -p nimbus-bin -p nimbus-server` passed; `cargo test -p nimbus-bin local_server --bin nimbus` passed 3/3; `cargo test -p nimbus-bin token --bin nimbus` passed 5/5; `cargo test -p nimbus-server machine_lifecycle --lib` passed 3/3; `cargo test -p nimbus-server local_server_security --lib` passed 10/10; `cargo test -p nimbus-bin machine --bin nimbus` passed 182/182; `cargo fmt --all --check` and `git diff --check` passed. |
| 2026-05-15 | ST4 React hook query proof | in progress | Added a headless `packages/nimbus-ui/src/system-query-proof.tsx` proof that compiles `NimbusProvider`, `useQuery`, `useQueries`, and `useNimbusConnectionState` against generated `_nimbus` refs for machines, scheduled jobs, listeners, adapter capabilities, and system status. This proves the React hook-facing contract without starting visible UI implementation. Verification: `npm run test --workspace nimbus-ui` passed; `npm run build --workspace nimbus-ui` passed. |
| 2026-05-15 | CI-shaped workspace verification checkpoint | passed | Confirmed the broad raw `make test` failure is outside this plan's required gate: it runs the runtime-owned Node-compat conformance corpus and failed with a libc++ vector hardening abort; a serialized raw `nimbus-runtime` repro completed with 303 passed, 111 failed, and 76 ignored in Node-compat/runtime evidence lanes. The required runtime lane, `cargo test -p nimbus-runtime -- --skip runtime::tests::node_compat::`, passed 172/172 with 15 ignored plus `locker_smoke` 8/8. Local `cargo nextest` is not installed, so the fallback workspace proof was run outside the Codex sandbox after the sandbox denied Unix socket and `ps` probes: `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo test --workspace --exclude nimbus-runtime` passed, including `nimbus-server` 654/654 with 9 ignored, MongoDB spec 23/23, reactive loop 32/32, and `nimbus-storage` 206/206 with 2 ignored. |
| 2026-05-15 | Final non-UI prerequisite verification | passed | Closed the post-review lint and audit cleanup. `make clippy` passed after replacing the run-history helper argument list with a `RunRecord` struct and fixing needless CLI borrows. `make deny` passed outside the sandbox because Cargo advisory DB locking needs write access under `~/.cargo`. `cargo fmt --all --check`, `npm run build --workspaces --if-present`, `npm run test --workspaces --if-present`, `cargo test -p nimbus-bin machine --bin nimbus` (182/182), `cargo test -p nimbus-server system_tenant --lib` (12/12), and `git diff --check` passed. After cleanup, the full local fallback for the non-runtime workspace lane, `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo test --workspace --exclude nimbus-runtime`, passed outside the sandbox, and `cargo test -p nimbus-runtime -- --skip runtime::tests::node_compat::` passed 172/172 with 15 ignored plus `locker_smoke` 8/8. Disk pressure was remediated with `cargo clean`, which freed 167.3 GiB; after the final rebuilds, `df -h .` reported roughly 88 GiB available. |
| 2026-05-15 | UI coherence follow-up surfaces | planned | Applied the desktop UI coherence review to this plan. The completed non-UI gate remains closed, but the React route plan now has explicit follow-ups for `events.byCorrelationId`, index list/create/drop APIs, a required Function Runner backed either by existing invoke routes or an optional generic wrapper endpoint, and broader run recording for native HTTP, scheduler, MongoDB, Firebase, and Cloud Functions traffic before the UI claims cross-adapter observability parity. |
