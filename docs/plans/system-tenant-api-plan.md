# Plan: System Tenant and Management API

Canonical execution plan for making machine, service, and runtime state
observable via the Convex API and manageable via HTTP endpoints. Creates a
reserved `_neovex` system tenant, wires the machine and service managers to
persist state as documents, and exposes lifecycle actions as HTTP endpoints.
This makes state queryable from any Convex client (`useQuery`, CLI, SDK)
with reactive subscriptions — independently useful regardless of whether a
UI exists.

Reviewed against:

- `crates/neovex-server/src/adapters/convex/` — `ConvexRegistry::from_app_dir()`,
  runtime-backed function execution, `ConvexHostBridge`, `ctx.db.*` host ops
- `crates/neovex-server/src/router.rs` — existing REST routes
  (`/api/tenants/{tenant_id}/documents/{table}`, etc.)
- `crates/neovex-bin/src/main.rs` — `--app-dir` flag, CLI subcommands
- `crates/neovex-bin/src/machine/mod.rs` — `MachineManager` state tracking
- `crates/neovex-bin/src/compose/mod.rs` — Compose-backed service state tracking
- `crates/neovex-engine/src/service/` — `Service`, tenant runtime, subscription
  delivery, query planning

---

## Status

- **Status:** `active`
- **Primary owner:** this plan
- **Activation gate:** prerequisite for `docs/plans/desktop-ui-plan.md`
- **Related plans:**
  - `docs/plans/desktop-ui-plan.md` — the UI's React frontend consumes the
    query surface and HTTP endpoints this plan creates
  - `docs/reference/microvm-service-baseline.md` — architecture context for
    machine/service state

## Current Assessed State

- Machine and service state lives in CLI-level managers
  (`crates/neovex-bin/src/machine/`, `crates/neovex-bin/src/compose/`), not
  in the engine's document storage. No API client can observe machine state
  reactively.
- The Convex adapter supports runtime-backed function execution via
  `ConvexRegistry::from_app_dir()` with `ctx.db.*`, `ctx.scheduler.*`, and
  `ctx.services.*` host calls.
- No HTTP endpoints exist for machine/service lifecycle actions — these are
  CLI-only commands today.
- The REST API already supports per-tenant document browsing at
  `GET /api/tenants/{tenant_id}/documents/{table}`.
- No system tenant exists. No Convex function bundle ships with the binary.

## Control Plan Rules

1. Every lifecycle mutation flows through `Service::apply_mutation` for
   document writes. HTTP endpoints invoke the managers, then the managers
   update system-tenant documents — no bypass.
2. The system tenant name `_neovex` is reserved. User-created tenants
   must not start with `_`.
3. The read/write path split is intentional: Convex queries for reactive
   reads, HTTP endpoints for lifecycle writes that require host-level
   orchestration.

## Verification Contract

Each roadmap item must satisfy before closing:

- `cargo fmt --all --check` — clean
- `make clippy` — clean
- `make test` — green
- `npm run build --workspaces --if-present` — green
- `npm run test --workspaces --if-present` — green
- Manual verification described per item

## Architecture

### Read/write path split

- **Read path**: Convex queries on the `_neovex` system tenant → reactive
  subscriptions fire automatically when documents change. Any Convex client
  (`useQuery`, HTTP query, CLI) gets live state.
- **Write path**: Machine/service lifecycle actions (start, stop, restart,
  delete) require host-level orchestration (spawning VMs, managing
  processes) beyond what `ctx.db` can do. These are HTTP endpoints on
  `neovex-server` that invoke the managers directly. The managers update
  system-tenant documents on completion, triggering reactive subscription
  updates automatically.
- **Cross-tenant data browsing**: The system tenant's Convex functions can
  only access `_neovex` documents via `ctx.db`. Browsing user-tenant
  documents uses the existing REST API
  (`GET /api/tenants/{tenant_id}/documents/{table}`) directly.

### System tenant tables

| Table | Key fields | Purpose |
| --- | --- | --- |
| `machines` | name, kind, state, provider, resources, meta | Machine inventory |
| `services` | name, machineId, bundleId, kind, state, endpoints, health | Service registry |
| `bundles` | sha256, sizeBytes, sourceRef, status | Deployed bundles |
| `functions` | bundleId, path, kind, argsSchema, returnsSchema | Per-bundle function registry |
| `tables` | name, schema, rowCount, lastWriteAt | User-data table directory |
| `events` | source, level, category, message, data, correlationId | Event firehose |
| `runs` | bundleId, functionPath, kind, durationMs, status, error | Runtime invocations |
| `scheduled_jobs` | tenantId, functionPath, scheduledTime, status, args, result | Scheduled jobs |
| `cron_jobs` | tenantId, name, schedule, functionPath, lastRunAt, nextRunAt, status | Cron job definitions |

### Query surface (Convex functions)

```
machines.list({ filter? })                    → Machine[]
machines.byId({ id })                         → Machine | null
services.list({ machineId? })                 → Service[]
services.byId({ id })                         → Service | null
bundles.list()                                → Bundle[]
functions.list({ bundleId })                  → FunctionEntry[]
tables.list()                                 → TableSummary[]
tables.browse(...)                            → via REST: GET /api/tenants/{id}/documents/{table} (cross-tenant)
events.recent({ filter?, limit })             → Event[]
runs.recent({ filter?, limit })               → Run[]
runs.byId({ id })                             → Run | null
scheduled_jobs.list({ tenantId?, status? })    → ScheduledJob[]
cron_jobs.list({ tenantId? })                 → CronJob[]
system.status()                               → { uptime, version, health }  (action)
```

### HTTP lifecycle endpoints (new)

```
POST   /api/machines/{name}/start
POST   /api/machines/{name}/stop
POST   /api/machines/{name}/restart
POST   /api/machines/{name}/create
DELETE /api/machines/{name}
PATCH  /api/machines/{name}                    (rename, resource changes)
POST   /api/services/{name}/start
POST   /api/services/{name}/stop
POST   /api/services/{name}/restart
DELETE /api/services/{name}
POST   /api/system/token/rotate
POST   /api/system/shutdown
```

### Convex mutations (document-level writes)

```
bundles.{delete,promote}
tables.{create,setSchema,dropSchema,deleteRows}
```

## Roadmap

### ST1 — System tenant creation and schema

Create the `_neovex` system tenant automatically on server startup.
Define table schemas for all 9 system tables. Reserve the `_` prefix for
system tenants — reject user-created tenants starting with `_`.

**Verification:** (a) `_neovex` tenant exists after `neovex start` starts,
(b) all 9 tables are defined with correct schemas, (c) `POST /api/tenants`
with `_` prefix name → 400 rejected.

**Status:** `pending`

### ST2 — Machine and service state persistence

Wire the machine manager to write machine state as documents in the
`_neovex` system tenant on every state transition (init, start, stop,
delete, error). Wire the service manager similarly for service state
changes and health updates.

**Verification:** (a) `neovex machine init` + `neovex machine start`
creates documents in the `machines` table, (b) machine stop updates the
document state field, (c) `ctx.db.query("machines")` returns current
state from a Convex function on the system tenant, (d) document changes
trigger reactive subscription updates.

**Status:** `pending`

### ST3 — HTTP lifecycle endpoints

Add HTTP endpoints on `neovex-server` for machine/service lifecycle actions
and system operations. Endpoints invoke `MachineManager` /
`ServiceManager` directly, then the managers update system-tenant documents
on completion.

**Verification:** (a) `POST /api/machines/{name}/start` starts the machine,
(b) system-tenant `machines` document reflects the new state, (c)
reactive subscription fires on state change, (d) error responses use the
structured error schema from `websocket-protocol-plan.md`.

**Status:** `pending`

### ST4 — Convex function bundle and codegen

Implement the Convex function bundle at `packages/neovex-ui/convex/` with
query functions for all system tables and document-level mutation functions
(bundles, tables). Generate typed function refs via `@neovex/codegen`.
Configure the server to load this bundle via
`ConvexRegistry::from_app_dir()` on the system tenant.

**Verification:** (a) `useQuery(api.machines.list)` returns current machine
state from a React app, (b) `useQuery(api.scheduled_jobs.list)` returns
scheduled jobs, (c) typed function refs compile cleanly, (d) `npm run build`
succeeds.

**Status:** `pending`

## Execution Log

| Date | Item | Status | Notes |
| --- | --- | --- | --- |
| 2026-04-18 | Plan authored | — | Extracted from desktop-ui-plan.md as prerequisite |
