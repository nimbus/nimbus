---
title: Native HTTP API
description: Endpoints, request shapes, and response shapes for the Nimbus native HTTP API.
sidebar:
  order: 1
---

The native HTTP API is the direct, language-neutral surface of a Nimbus
server. All endpoints accept and return JSON (`Content-Type:
application/json`) unless noted. For a guided walkthrough, see
[Use the native HTTP and WebSocket API](/developers/native/).

## Conventions

- **Base URL** — the server's listen address, for example
  `http://localhost:8080`.
- **Tenancy** — data routes are scoped by path:
  `/api/tenants/{tenant_id}/...`. A tenant must exist before its data routes
  work; unknown tenants return `404` with code `session.tenant_not_found`.
- **Authentication** — when the server runs with local security (the
  default for `nimbus start`), tenant, document, query, schema, scheduling,
  and WebSocket routes require a local admin credential: either
  `Authorization: Bearer <token>` or `X-Nimbus-Admin-Token: <token>`.
  Missing or invalid credentials return `401` with code `auth.unauthorized`.
- **Browser origins** — requests that carry an `Origin` header are only
  accepted from loopback HTTP origins (`localhost`, `127.0.0.1`, `[::1]`)
  on the server's port. Non-browser clients that send no `Origin` header
  are not subject to this check. Disallowed origins return `403`.
- **Errors** — every error response uses the structured envelope described
  in the [error reference](/reference/native/errors/).

## Health

| Method | Path | Success | Response body |
| --- | --- | --- | --- |
| GET | `/health` | 200 | `{"ok": true}` |

The health route requires no credential.

## Tenants

| Method | Path | Success | Response body |
| --- | --- | --- | --- |
| POST | `/api/tenants` | 201 | `{"id": "<tenant_id>"}` |
| GET | `/api/tenants` | 200 | `{"tenants": ["<tenant_id>", ...]}` |
| DELETE | `/api/tenants/{tenant_id}` | 204 | — |

`POST /api/tenants` body: `{"id": "<tenant_id>"}`.

## Documents

| Method | Path | Success | Response body |
| --- | --- | --- | --- |
| POST | `/api/tenants/{tenant_id}/documents` | 201 | `{"id": "<document_id>"}` |
| GET | `/api/tenants/{tenant_id}/documents/{table}` | 200 | `{"data": [<document>, ...]}` |
| GET | `/api/tenants/{tenant_id}/documents/{table}/{document_id}` | 200 | `{"document": <document>}` |
| PATCH | `/api/tenants/{tenant_id}/documents/{table}/{document_id}` | 200 | `{"id": "<document_id>"}` |
| DELETE | `/api/tenants/{tenant_id}/documents/{table}/{document_id}` | 204 | — |

Request bodies:

- **Insert** — `{"table": "<table>", "fields": {...}}`. The table is
  created implicitly on first write.
- **Update** — `{"patch": {...}}`. The patch is a partial field map merged
  into the existing document.

Document objects in responses carry three system fields alongside the
user-defined fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `_id` | string | Document id |
| `_creationTime` | number | Creation time, epoch milliseconds |
| `_updateTime` | number | Last update time, epoch milliseconds |

## Queries

| Method | Path | Success | Response body |
| --- | --- | --- | --- |
| POST | `/api/tenants/{tenant_id}/query` | 200 | `{"data": [<document>, ...]}` |
| POST | `/api/tenants/{tenant_id}/query/paginated` | 200 | `{"data": [...], "next_cursor": "<cursor>" \| null, "has_more": bool}` |

Query object (the body of `/query`, and the `query` field of
`/query/paginated`):

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `table` | string | yes | Table to query |
| `filters` | array | yes | Filter clauses; `[]` matches all documents |
| `order` | object | no | `{"field": "<field>", "direction": "asc" \| "desc"}` |
| `limit` | number | no | Maximum number of results |

Filter clause: `{"field": "<field>", "op": "<op>", "value": <json>}` with
operators `eq`, `neq`, `gt`, `gte`, `lt`, `lte`.

Paginated query body:

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `query` | object | yes | Query object as above |
| `page_size` | number | yes | Documents per page |
| `after` | string | no | Cursor from a previous page's `next_cursor` |

## Journal

The journal exposes a tenant's durable mutation log — the ordered record of
applied writes — for replication and replay. Both routes are read-only.

| Method | Path | Success | Response body |
| --- | --- | --- | --- |
| GET | `/api/tenants/{tenant_id}/journal` | 200 | Journal stream page (see below) |
| GET | `/api/tenants/{tenant_id}/journal/bootstrap` | 200 | Bootstrap payload (see below) |

`GET .../journal` streams records after a cursor. Query parameters:

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `after` | number | no | Sequence cursor; records *after* it are returned (default `0`, from the start) |
| `limit` | number | no | Maximum records per page (default `100`) |

Response body:

| Field | Type | Meaning |
| --- | --- | --- |
| `records` | array | Journal records, oldest first (see below) |
| `next_cursor` | number | Sequence to pass as the next call's `after` |
| `latest_sequence` | number | Highest sequence currently durable |
| `cursor_floor` | number | Lowest sequence still retained |
| `has_more` | boolean | Whether more records remain past this page |

Each record carries `version`, `sequence`, `timestamp` (epoch milliseconds),
`events`, `writes`, `scheduled_execution_id` (string or null), and
`integrity_sha256`.

`GET .../journal/bootstrap` returns a materialized snapshot plus the cursor
to resume streaming from, for replaying a tenant from scratch:

| Field | Type | Meaning |
| --- | --- | --- |
| `snapshot` | object | Materialized state: `version`, `applied_sequence`, `durable_head`, `schema`, `documents`, `scheduled_execution_ids` |
| `resume_after_sequence` | number | Pass as `after` to `.../journal` to continue from the snapshot |
| `bootstrap_cut_sequence` | number | Sequence the snapshot was cut at |
| `cursor_floor_sequence` | number | Lowest sequence still retained |

## Schema

Schemas are optional: a table without a schema accepts any document.
Setting a schema adds validation constraints.

| Method | Path | Success | Response body |
| --- | --- | --- | --- |
| GET | `/api/tenants/{tenant_id}/schema` | 200 | `{"tables": {"<table>": <table schema>, ...}}` |
| GET | `/api/tenants/{tenant_id}/schema/{table}` | 200 | `<table schema>` |
| PUT | `/api/tenants/{tenant_id}/schema/{table}` | 204 | — |
| DELETE | `/api/tenants/{tenant_id}/schema/{table}` | 204 | — |

Table schema shape (also the `PUT` request body; its `table` field must
match the path table or the request fails with `400`):

```json
{
  "table": "messages",
  "fields": [
    { "name": "text", "field_type": "string", "required": true },
    { "name": "votes", "field_type": "number", "required": false }
  ],
  "indexes": [
    { "name": "by_author", "fields": ["author"] }
  ]
}
```

Field types: `string`, `number`, `boolean`, `array`, `object`, `any`.
Index entries may also carry server-managed `id` and `state` fields
(`state` defaults to `enabled`); omit them when writing.

## Scheduled jobs

| Method | Path | Success | Response body |
| --- | --- | --- | --- |
| POST | `/api/tenants/{tenant_id}/schedule` | 201 | `{"job_id": "<job_id>"}` |
| GET | `/api/tenants/{tenant_id}/schedule` | 200 | `{"jobs": [<job>, ...]}` |
| DELETE | `/api/tenants/{tenant_id}/schedule/{job_id}` | 204 | — |
| GET | `/api/tenants/{tenant_id}/schedule/history/{job_id}` | 200 | `{"result": <job result>}` |

Schedule request body: `{"run_after_ms": <ms>, "mutation": <mutation>}`.

Mutation objects are tagged by `type`:

```json
{ "type": "insert", "table": "messages", "fields": { "text": "hi" } }
{ "type": "update", "table": "messages", "id": "<document_id>", "patch": { "text": "edited" } }
{ "type": "delete", "table": "messages", "id": "<document_id>" }
```

`insert` also accepts an optional `id`. A pending job has `id`, `run_at`
(epoch milliseconds), `mutation`, and `created_at`. A job result has `id`,
`run_at`, `finished_at`, `mutation`, `outcome` (`completed` or `failed`),
and `error` (string or null).

## Cron jobs

| Method | Path | Success | Response body |
| --- | --- | --- | --- |
| POST | `/api/tenants/{tenant_id}/crons` | 201 | — |
| GET | `/api/tenants/{tenant_id}/crons` | 200 | `{"crons": [<cron>, ...]}` |
| DELETE | `/api/tenants/{tenant_id}/crons/{name}` | 204 | — |

Create request body:

```json
{
  "name": "cleanup",
  "schedule": { "type": "interval", "seconds": 3600 },
  "mutation": { "type": "delete", "table": "temp", "id": "<document_id>" }
}
```

Interval schedules are the supported schedule type. A cron object in list
responses has `name`, `schedule`, `mutation`, `enabled`, `last_run`
(epoch milliseconds or null), `next_run`, and `created_at`.

## Service control

These routes manage services, sandboxes, and sessions. The
[Agents guides](/agents/) cover their request and response shapes
through a typed client.

| Method | Path | Purpose |
| --- | --- | --- |
| GET, POST | `/api/tenants/{tenant_id}/services` | List or create service definitions |
| GET, PUT, DELETE | `/api/tenants/{tenant_id}/services/{service_name}` | Read, update, or delete a service |
| POST | `/api/tenants/{tenant_id}/services/{service_name}/start` | Start a service |
| POST | `/api/tenants/{tenant_id}/services/{service_name}/stop` | Stop a service |
| GET, POST | `/api/tenants/{tenant_id}/sandboxes` | List or create sandboxes |
| GET | `/api/tenants/{tenant_id}/sandboxes/{sandbox_id}` | Read a sandbox |
| POST | `/api/tenants/{tenant_id}/sandboxes/{sandbox_id}/stop` | Stop a sandbox |
| GET, POST | `/api/sessions` | List or open sessions (tenant passed in query/body) |
| GET | `/api/sessions/{session_id}` | Read a session |
| POST | `/api/sessions/{session_id}/close` | Close a session |

## WebSocket

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/ws` | Upgrade to the `nimbus.v2` subscription protocol |

The tenant is identified by an `X-Tenant-Id` header or a `tenant_id` query
parameter. See the
[WebSocket protocol reference](/reference/native/websocket-protocol/).
