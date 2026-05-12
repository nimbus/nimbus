# Current Capabilities

This document is a snapshot of what Nimbus currently implements. It is not a
roadmap. Nimbus is still pre-launch, so this surface can change quickly when a
cleaner design is preferred.

## Core Platform

- explicit tenant creation and deletion
- optional per-table schema validation over the native HTTP API
- document insert, update, delete, and point reads
- explicit query and paginated query endpoints
- single-field indexes with backfill for explicit query paths
- live query subscriptions over WebSocket
- durable scheduled mutations and recurring cron jobs
- scheduled job result lookup by `job_id`
- startup recovery for claimed-but-unfinished scheduled jobs

## Data-Layer Features

- schema CRUD per tenant and per table
- cursor-based pagination with opaque cursors
- single-field indexes maintained atomically with writes
- indexed equality planning for explicit query paths
- indexed string and number range planning for explicit query paths
- index-aware subscription evaluation for initial results and re-evaluation
- durable at-least-once scheduled job execution
- persisted scheduled job completion and failure results for observability
- schemaless behavior for tables without an installed schema
- per-tenant engine diagnostics for journal, worker, materialized-serving, and `libsql` replica freshness health at `GET /debug/tenants/{tenant_id}/engine/metrics`

## Runtime And Convex Surface

- optional Convex support through the in-repo `convex` package and V8 runtime
- generated runtime bundles with per-invocation SHA-256 verification
- named runtime queries, mutations, actions, and HTTP routes
- runtime-backed live subscriptions with narrower dependency tracking than
  coarse table-level invalidation
- runtime diagnostics at `GET /debug/runtime/metrics` when Convex support is
  enabled

See the dedicated references for detail:

- [MicroVM and service-control baseline](microvm-service-baseline.md)
- [HTTP and WebSocket API](http-api.md)
- [CLI reference](cli.md)
- [Convex compatibility](../convex/compatibility.md)
