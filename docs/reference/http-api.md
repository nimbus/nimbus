# HTTP And WebSocket API

This document lists the public server routes exposed by Neovex today.

Native routes are always available. Convex routes are available only when the
server starts with `--convex-app-dir`.

## Core Service Routes

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/health` | health check |
| `GET` | `/debug/license/status` | current license snapshot and usage state |
| `GET` | `/debug/tenants/{tenant_id}/engine/metrics` | per-tenant engine durability, worker, serving, and provider-specific diagnostics such as `libsql` replica freshness |
| `GET` | `/demos` | redirects to the demo index |
| `GET` | `/demos/` | serves the demo directory |

## Tenant Routes

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/api/tenants` | create a tenant |
| `GET` | `/api/tenants` | list tenants |
| `DELETE` | `/api/tenants/{tenant_id}` | delete a tenant |

## Schema Routes

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/tenants/{tenant_id}/schema` | get the tenant schema |
| `GET` | `/api/tenants/{tenant_id}/schema/{table}` | get one table schema |
| `PUT` | `/api/tenants/{tenant_id}/schema/{table}` | replace one table schema |
| `DELETE` | `/api/tenants/{tenant_id}/schema/{table}` | delete one table schema |

## Document And Query Routes

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/api/tenants/{tenant_id}/documents` | insert a document |
| `GET` | `/api/tenants/{tenant_id}/documents/{table}` | list documents in a table |
| `GET` | `/api/tenants/{tenant_id}/documents/{table}/{document_id}` | get one document |
| `PATCH` | `/api/tenants/{tenant_id}/documents/{table}/{document_id}` | update one document |
| `DELETE` | `/api/tenants/{tenant_id}/documents/{table}/{document_id}` | delete one document |
| `POST` | `/api/tenants/{tenant_id}/query` | execute a query |
| `POST` | `/api/tenants/{tenant_id}/query/paginated` | execute a paginated query |
| `GET` | `/api/tenants/{tenant_id}/journal` | stream durable journal records after a sequence cursor |
| `GET` | `/api/tenants/{tenant_id}/journal/bootstrap` | export snapshot-plus-journal bootstrap metadata |

## Scheduling Routes

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/api/tenants/{tenant_id}/schedule` | schedule a mutation |
| `GET` | `/api/tenants/{tenant_id}/schedule` | list scheduled jobs |
| `DELETE` | `/api/tenants/{tenant_id}/schedule/{job_id}` | cancel a scheduled job |
| `GET` | `/api/tenants/{tenant_id}/schedule/history/{job_id}` | get a scheduled job result |
| `POST` | `/api/tenants/{tenant_id}/crons` | create a cron job |
| `GET` | `/api/tenants/{tenant_id}/crons` | list cron jobs |
| `DELETE` | `/api/tenants/{tenant_id}/crons/{name}` | delete a cron job |

## Native WebSocket Route

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/ws` | native live-query subscription transport |

Notes:

- non-browser clients can identify the tenant with the `X-Tenant-Id` header
- browser demos use `?tenant_id=` because native browser `WebSocket` clients
  cannot set custom headers

## Optional Convex Routes

These routes are present only when the server is started with
`--convex-app-dir`.

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/debug/runtime/metrics` | runtime limits and live runtime metrics |
| `POST` | `/convex/{tenant_id}/query` | Convex-style query dispatch |
| `POST` | `/convex/{tenant_id}/query/paginated` | Convex-style paginated query dispatch |
| `POST` | `/convex/{tenant_id}/mutation` | Convex-style mutation dispatch |
| `POST` | `/convex/{tenant_id}/action` | Convex-style action dispatch |
| `ANY` | `/convex/{tenant_id}/http` | Convex `httpAction` root dispatch |
| `ANY` | `/convex/{tenant_id}/http/{*path}` | Convex `httpAction` path dispatch |
| `POST` | `/convex/{tenant_id}/schedule/run_after` | schedule a Convex mutation after a delay |
| `POST` | `/convex/{tenant_id}/schedule/run_at` | schedule a Convex mutation at a time |
| `DELETE` | `/convex/{tenant_id}/schedule/{job_id}` | cancel a Convex scheduled job |
| `GET` | `/convex/{tenant_id}/ws` | Convex-style live-query WebSocket transport |
