---
title: Observability
description: How diagnostics are wired into the binary — the public health probe, admin-gated debug surfaces, per-tenant engine snapshots, latency budgets, structured logs, and the access audit trail.
sidebar:
  label: Observability
  order: 13
---

Nimbus has no metrics sidecar, agent, or exporter process. Every
diagnostic surface is compiled into the single binary and follows one of
three shapes: pull-based JSON endpoints, structured log events on stdout,
and an append-only audit file. This page explains how those surfaces are
wired and what each one is for. For the operational walkthrough — tokens,
`curl` commands, response fields — see the
[operators' observability guide](/operators/observability/).

## One public probe, everything else gated

The HTTP router (`crates/nimbus-server/src/router.rs`) is composed from
route groups with different trust levels, and the diagnostic surfaces
split cleanly across two of them:

- The **public router** carries exactly one diagnostic route: `/health`.
  Its handler (`crates/nimbus-server/src/http/metadata.rs`) returns
  `{"ok":true}` unconditionally — no credential, no tenant, no storage
  read. It is a liveness probe for load balancers, nothing more.
- The **local admin router** carries every `/debug/*` route. Before a
  request reaches a handler it passes through the middleware chain in
  `crates/nimbus-server/src/local_server/middleware.rs`: the request path
  is classified into a route family, browser `Origin` headers are checked
  against the loopback allowlist, the admin credential is extracted, and
  a fail-closed gate rejects anything not authorized. Every gate
  decision — allow or deny — is also recorded in the audit log described
  below.

The route-family vocabulary itself (`health`, `debug`, `native_api`,
`deploy_admin`, the per-adapter families, …) lives in the
`nimbus-operator` crate, so the server, the gate, and the audit log all
agree on what kind of request they are looking at. The broader credential
model is covered in [Auth and trust](/concepts/architecture/auth-trust/).

## The debug surfaces

Five admin-gated routes expose the binary's internal state, all handled in
`crates/nimbus-server/src/http/metadata.rs`:

| Route | What it reports |
| --- | --- |
| `/debug/license/status` | License source, kind, entitlements, and live monthly-active-user accounting |
| `/debug/encryption/status` | Whether encryption at rest is on, which storage families are protected, and which key provider is configured |
| `/debug/runtime/metrics` | Effective runtime limits plus live invocation counters, per runtime lane |
| `/debug/tenants/{tenant_id}/engine/metrics` | The per-tenant engine diagnostics snapshot |
| `/debug/tenants/{tenant_id}/consistency` | An on-demand consistency verification report |

Two design choices are worth noting. `/debug/runtime/metrics` always
returns `200` with a stable shape — its fields are `null` until a
deployment is active, so a freshly started server never surfaces a
spurious error to the operator UI. And the tenant-scoped routes validate
the tenant id and enter the engine under an operator-class tenant
isolation context tagged with the diagnostic surface — diagnostics reads
are attributed and policed like any other operator access, not exempted
from it.

## What the per-tenant snapshot aggregates

The engine diagnostics endpoint is a thin transport wrapper: the server
asks the engine, and the engine assembles a
`TenantEngineDiagnosticsSnapshot` (`crates/nimbus-engine/src/tenant.rs`)
from the live per-tenant runtime. Its groups map one-to-one onto the
stages a write or read passes through:

- **Mutation admission** — the write admission gate: queue depth against
  capacity, age of the oldest queued mutation, admitted versus shed
  counts, and the current load-shedding phase. This is where overload
  shows up first.
- **Mutation journal** — durability progress: the durable head versus the
  applied head, the lag between them, pending responses, and the apply
  worker's health (running, start and restart counts, failures). A
  growing apply lag means commits are durable but not yet visible.
- **Subscription delivery** — the reactive fan-out queue: depth, worker
  health, and coalescing counters showing how many commits were batched
  per wakeup. This tells you whether live queries are keeping up with
  write volume.
- **Materialized read surface** and **serving snapshot manager** — the
  read path: how many tables and documents are resident in memory against
  their capacities, and how many versioned snapshots are retained, pinned
  by in-flight reads, or pruned. Together they describe read-path cache
  pressure.
- **Query planning** — counters splitting query executions by plan shape:
  full scan, single-field index, or composite index, for both plain and
  paginated queries. A rising full-scan count is the signal to add an
  index.
- **libSQL replica freshness** — present only when the tenant is served
  from an embedded libSQL replica: the sequence the replica must reach,
  what it has applied, and which barrier path recent reads took
  (`crates/nimbus-storage/src/libsql/freshness.rs`).

The snapshot is a point-in-time read of live counters — collecting it
does not pause the tenant.

## The consistency verifier

The consistency route runs an active check rather than reading counters.
The verifier (`crates/nimbus-engine/src/verification.rs`) fingerprints the
tenant's authoritative storage snapshot, the shadow materializer, and the
embedded replica — each digest covering schema, documents, and scheduled
executions at a sequence point — checks the journal bootstrap cut, and
reports every invariant violation as a structured mismatch naming both
sides. A healthy tenant returns `ok: true` with an empty mismatch list.
This exists because the engine maintains several derived views of the
same journal (see
[Engine and the mutation path](/concepts/architecture/engine-mutation-path/)),
and divergence between them must be detectable on demand, not just
asserted in tests.

## Structured logging

The binary initializes `tracing` with a stdout formatter at process start
(`crates/nimbus-bin/src/main.rs`). Filtering uses the standard `RUST_LOG`
variable, parsed as a `target=level` directive list (the
`tracing-subscriber` `Targets` filter), defaulting to `info` when unset.
There is no Nimbus-specific log configuration layer: crate-level targets
like `nimbus_server` and `nimbus_engine` are the filtering knobs.

### Latency budgets

Rather than exporting timing histograms, hot paths carry **budgeted
segment timers** that stay silent until a budget is exceeded. Two
mirrored modules define them:

- `crates/nimbus-server/src/latency.rs` — transport-side segments:
  `server.auth` (10 ms), `server.storage` (50 ms), `server.runtime`
  (100 ms).
- `crates/nimbus-engine/src/engine/latency.rs` — engine-side segments:
  `engine.tenant_load` (50 ms), `engine.wait_visibility` (25 ms),
  `engine.query_prepare` (5 ms), `engine.query_execute` (50 ms),
  `engine.query_cache` (5 ms).

When a segment overruns, a `WARN` event is emitted with structured
fields: the stable segment name, `elapsed_ms`, and `budget_ms`. The
timers finish on drop, so an early return or error path still reports.
The result is a log stream that is quiet at the default level but yields
an alertable, low-volume latency signal without any metrics
infrastructure.

## The access audit log

Independently of stdout logging, every request that reaches an
admin-gated route family is appended to a JSONL audit file —
authorization successes and failures alike. The writer
(`crates/nimbus-operator/src/audit.rs`) serializes one record per line
with the timestamp, route family, tenant id when one can be attributed,
the auth scope and credential method, the success flag, the request
origin, and the reason string. The file is created with owner-only
(`0600`) permissions and lives under the platform state directory
(`logs/access.jsonl`, resolved by `crates/nimbus-operator/src/paths.rs`).

The middleware emits these records at the gate, not in the handlers — so
a denied request is audited even though no handler ever ran, and the
audit trail cannot be bypassed by a handler bug. Tenant attribution works
across adapters: the writer extracts tenant ids from native and Convex
paths and from Firestore gRPC metadata headers, so a Firebase client's
admin-surface attempt is attributed to its project.

## What does not exist yet

Nimbus currently has **no Prometheus endpoint and no OpenTelemetry
exporter**. There is no `/metrics` route, and no Nimbus crate depends on
a metrics or tracing exporter (an OpenTelemetry dependency appears only
transitively inside the forked JavaScript runtime's telemetry crate; the
server neither configures nor exposes it). The supported model today is:

- scrape the `/debug/*` JSON endpoints with your own collector,
- ship stdout (or the systemd journal) to your log pipeline, and
- tail `access.jsonl` for security-relevant events.

If you need Prometheus-format metrics now, a small adapter that polls
`/debug/runtime/metrics` and the per-tenant snapshot is the intended
integration point, since both return stable JSON shapes.

## Where to go next

- [Operators' observability guide](/operators/observability/) — the hands-on walkthrough.
- [Server and transport](/concepts/architecture/server-transport/) — where the router and middleware live.
- [Auth and trust](/concepts/architecture/auth-trust/) — the credential model behind the admin gate.
- [Tenancy](/concepts/architecture/tenancy/) — what a tenant is, and why diagnostics are tenant-scoped.
