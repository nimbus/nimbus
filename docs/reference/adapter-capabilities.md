---
title: Adapter capabilities
description: One cross-adapter matrix reconciling what each Nimbus protocol front door translates today — CRUD, queries, indexes, subscriptions, transactions, schema, auth, tenancy, ports, and hard limits.
sidebar:
  label: Adapter capabilities
  order: 6
---

This page reconciles the per-adapter compatibility references into one
matrix. It is a snapshot of what each protocol front door translates today,
not a roadmap. Where a per-adapter page states a boundary, that page is the
source of truth; the cells here summarize and link to it.

Each capability cell carries one status:

- **Supported** — implemented and exercised on that adapter.
- **Supported with caveats** — works within a bounded surface; the owning
  reference page states the boundary.
- **Not translated** — the adapter exposes no surface for the capability;
  requests for it are rejected or have no equivalent.

Rows one through six carry a status. Rows seven through ten describe the
mechanism rather than grade it.

Convex and Native are data-and-compute surfaces. Cloud Functions is a
compute-and-trigger surface: its data-capability cells read against the
narrow `firebase-admin` slice callable from inside a function. Firestore,
MongoDB, and DynamoDB are data surfaces.

## Capability matrix

| Capability | [Convex](/reference/convex/compatibility/) | [Firestore](/reference/firebase/compatibility/) | [Cloud Functions](/reference/cloud-functions/compatibility/) | [MongoDB](/reference/mongodb/operations/) | [DynamoDB](/reference/dynamodb/feature-coverage/) | [Native](/reference/native/http-api/) |
| --- | --- | --- | --- | --- | --- | --- |
| Point CRUD | Supported | Supported | Supported with caveats | Supported | Supported | Supported |
| Explicit queries + pagination | Supported | Supported with caveats | Not translated | Supported with caveats | Supported | Supported |
| Secondary / composite indexes | Supported | Not translated | Not translated | Supported | Supported | Supported |
| Live subscriptions / change events | Supported | Supported | Supported | Not translated | Supported | Supported |
| Multi-document transactions / atomic batch | Supported with caveats | Supported | Not translated | Supported with caveats | Supported | Supported with caveats |
| Schema validation | Supported | Not translated | Not translated | Not translated | Not translated | Supported |
| Auth mechanism | OIDC / custom JWT | Bearer-token app auth | Service principal; callable auth | SCRAM-SHA-256 | SigV4 signed requests | Local admin token |
| Tenant binding model | `/convex/{tenantId}` path | `(default)` database; auth-scoped | `firebase.json` app root | Database name = tenant | Access key → one tenant | `/api/tenants/{id}` path |
| Default listener / port | Main listener (`:8080`) | Main listener (`:8080`) | Main listener (`:8080`) | `127.0.0.1:27017` | `127.0.0.1:8000` | Main listener (`:8080`) + `/ws` |
| Notable hard limits | No file storage, search, or crons | `(default)` database only; no offline persistence | Firestore document triggers only | Six filter operators; 16 MiB document; loopback only | No provisioned capacity; single stream shard | Loopback default; admin token required |

## Row notes

- **Point CRUD.** Convex omits `db.replace`. Cloud Functions CRUD is the
  covered `firebase-admin` `DocumentReference` `get` / `set` / `update` /
  `delete` slice.
- **Explicit queries + pagination.** Firestore covers the implemented
  query subset with cursors; nested field-path helpers stay narrow. MongoDB
  `find` accepts only `eq`, `ne`, `gt`, `gte`, `lt`, and `lte` and paginates
  through `getMore`. Native exposes query and cursor-paginated query
  endpoints. Cloud Functions exposes no query builder in its covered admin
  slice.
- **Secondary / composite indexes.** Convex declares indexes in the table
  schema. MongoDB exposes `createIndexes`, `dropIndexes`, and `listIndexes`.
  DynamoDB supports local and global secondary indexes. Native maintains
  single-field and composite indexes atomically with writes. Firestore and
  Cloud Functions document no index-declaration surface.
- **Live subscriptions / change events.** Convex and Native serve reactive
  query subscriptions over WebSocket. Firestore `onSnapshot` runs over the
  WebSocket `Listen` channel. Cloud Functions delivers Firestore document
  triggers. DynamoDB Streams expose change records. The MongoDB endpoint
  rejects `$changeStream` with `CommandNotSupported`.
- **Multi-document transactions / atomic batch.** Firestore covers
  `writeBatch` and `runTransaction`. MongoDB runs multi-statement
  transactions scoped to one database, failing a conflicting commit with
  `WriteConflict`. DynamoDB `TransactWriteItems` commits all items or none.
  Convex applies each mutation's writes as one atomic set with no
  interactive transaction API. The Native HTTP surface applies each mutation
  atomically with its index effects but exposes no multi-document batch
  endpoint.
- **Schema validation.** Convex and Native offer optional per-table schema
  validation; a table without a schema accepts any document. Firestore,
  MongoDB, and DynamoDB document no document-validation surface (DynamoDB
  defines key schema only).

## Additional wire surfaces

Three further surfaces ship today and are documented separately. They are
early, bounded surfaces; the notes below describe only what exists now.

- **S3.** An S3-compatible HTTP listener served by default on
  `127.0.0.1:9000` (`--no-s3` disables it), authenticated with per-tenant
  signed access keys. See [S3 compatibility](/reference/s3/compatibility/).
- **Cloudflare.** Cloudflare-compatible routes and a binding registry
  mounted on the main listener by default (`--no-cloudflare` disables them),
  including a KV binding surface. See
  [Cloudflare compatibility](/reference/cloudflare/compatibility/).
- **RESP KV.** A Redis-serialization-protocol key-value listener started
  with `nimbus kv`, bound to loopback `127.0.0.1:6380`, with RESP `AUTH` and
  a single bound tenant. See [KV compatibility](/reference/kv/compatibility/).
