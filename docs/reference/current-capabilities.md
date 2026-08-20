---
title: Current capabilities
description: A snapshot of what Nimbus implements today — every major capability with a status and a link to its documentation.
sidebar:
  label: Current capabilities
  order: 5
---

This page is a snapshot of what Nimbus implements today. It is not a
roadmap: a capability appears here only when it exists in the shipped
implementation, and the surface can change quickly when a cleaner design is
preferred.

Each capability carries one of three statuses:

- **Available** — implemented and documented; use it today.
- **Available with caveats** — works, but with a bounded scope or a
  non-default enablement path described in the notes.
- **Not yet** — does not exist today; there is nothing to enable.

A status describes the implementation, not release maturity. Nimbus is beta
across every row below: APIs can break between releases.

## Core data platform

| Capability | Status | Notes |
| --- | --- | --- |
| Tenant creation and deletion | Available | Explicit creation over the admin API; each tenant gets its own storage namespace. See [tenant isolation](/operators/tenant-isolation/). |
| Document insert, update, delete, and point reads | Available | See the [HTTP API](/reference/native/http-api/). |
| Explicit queries and cursor-based pagination | Available | Opaque cursors; dedicated query and paginated-query endpoints. |
| Optional per-table schema validation | Available | A table without a schema accepts any document; installing a schema adds constraints, never removes write access. |
| Single-field and composite indexes | Available | Declared in the table schema, maintained atomically with writes, backfilled on creation. Equality and range planning for explicit query paths. |
| Live query subscriptions over WebSocket | Available | Index-aware evaluation with per-query dependency tracking. See the [WebSocket protocol](/reference/native/websocket-protocol/). |
| Scheduled mutations and cron jobs | Available | Durable, at-least-once execution; completion and failure results retained per job id; claimed-but-unfinished jobs are recovered on startup. |
| User authentication (OIDC and custom JWT) | Available | Functions verify identities issued by your identity provider. See [authenticate users](/developers/auth/). |

## Functions and runtime

| Capability | Status | Notes |
| --- | --- | --- |
| TypeScript queries, mutations, actions, and HTTP routes | Available | Convex-compatible function model. See [write functions](/developers/convex/). |
| Runtime bundle integrity | Available | Bundles are SHA-256 verified on every invocation. Deploys stage and activate through the [deploy & admin API](/reference/deploy-admin-api/). |
| Runtime-backed live subscriptions | Available | Dependency tracking is narrower than coarse table-level invalidation. |
| Node.js compatibility (`"use node"` actions) | Available with caveats | Node 22, 24 (default), and 26 are selectable targets; Node 20 is local-development only. The supported surface is bounded and evidence-backed — see [Node compatibility](/reference/runtimes/node-compat/). |
| Runtime permission grants | Available | Compatibility target and host access are separate axes; selecting a Node version grants nothing. See [runtime permissions](/concepts/runtime-permissions/). |
| Runtime and per-tenant engine diagnostics | Available | Runtime lane state plus per-tenant journal, admission, subscription-delivery, serving, and replica-freshness metrics over HTTP. See [observability](/operators/observability/). |

## Protocol adapters

| Adapter | Status | How it is enabled |
| --- | --- | --- |
| Convex | Available | Detected by `nimbus dev` and `nimbus start` from your `convex/` directory. See the [guide](/developers/convex/) and [compatibility reference](/reference/convex/compatibility/). |
| Cloud Functions for Firebase | Available | Detected from `firebase.json`. See the [guide](/developers/cloud-functions/) and [compatibility reference](/reference/cloud-functions/compatibility/). |
| Firestore | Available | Served by default on the main listener (`--no-firestore` switches it off); `nimbus dev` wires covered Firebase apps at the drop-in package automatically. See the [guide](/developers/firebase/) and [compatibility reference](/reference/firebase/compatibility/). |
| MongoDB wire protocol | Available | Served by default on `127.0.0.1:27017` (`--no-mongodb` switches it off); generated SCRAM credentials unless overridden (`--mongodb-username`, `NIMBUS_MONGODB_PASSWORD`); `nimbus dev` writes `NIMBUS_MONGODB_URL` to `.env.local` for detected driver apps. Loopback-only listener. See the [guide](/developers/mongodb/) and [operations reference](/reference/mongodb/operations/). |
| DynamoDB API | Available | Served by default on `127.0.0.1:8000` (`--no-dynamodb` switches it off); a generated access key bound to tenant `default` unless `--dynamodb-access-key KEY_ID:SECRET:TENANT` bindings are given; `nimbus dev` writes `NIMBUS_DYNAMODB_*` keys to `.env.local` for detected SDK apps. See the [guide](/developers/dynamodb/) and [feature coverage](/reference/dynamodb/feature-coverage/). |
| S3 API | Available with caveats | Served by default on `127.0.0.1:9000` when the port is free (`--no-s3` switches it off); its own credential registry (`--s3-access-key KEY_ID:SECRET:TENANT` or `NIMBUS_S3_ACCESS_KEYS`), a generated key bound to tenant `default` otherwise. Object and multipart operations only — no bucket lifecycle, copy, batch delete, tagging, ACL, or versioning. See the [compatibility reference](/reference/s3/compatibility/). |
| Cloudflare API | Available with caveats | Served by default on the main listener (`--no-cloudflare` switches it off). Only the Workers KV REST data plane is served today; Durable Objects have no production construction path, and D1 and R2 are also not served. See the [compatibility reference](/reference/cloudflare/compatibility/). |
| RESP (Redis) KV | Available with caveats | Started with the explicit `nimbus kv` command (not by `nimbus dev` or `nimbus start`); a loopback-only RESP2/RESP3 listener on `127.0.0.1:6380` with mandatory AUTH and a bounded string/key command surface. See the [compatibility reference](/reference/kv/compatibility/). |
| Native HTTP and WebSocket API | Available | Always on. See [build on the native API](/developers/native/). |
| Nimbus JavaScript SDK | Available | Services, sandboxes, and sessions from one client. See the [Agents guides](/agents/). |

## Storage and operations

| Capability | Status | Notes |
| --- | --- | --- |
| SQLite backend (default) | Available | One database file per tenant. See [storage backends](/operators/storage-backends/). |
| PostgreSQL backend | Available | One schema per tenant in a database you operate. |
| MySQL backend | Available | One database per tenant. |
| libSQL / Turso backend | Available | Local replica reads against a remote libSQL primary, with replica-freshness diagnostics. |
| redb backend | Available | Retained embedded key-value backend; prefer SQLite otherwise. |
| Encryption at rest | Available | Per-file data keys with master-key-file, key-directory, or AWS KMS providers, plus key-rotation commands. See [encryption](/operators/encryption/). |
| Backup and restore | Available with caveats | `nimbus backup create`/`restore` for the embedded SQLite and redb providers — one offline, per-tenant archive captured at each tenant's latest committed sequence (server stopped), verified by fingerprint on restore. External backends and encrypted data directories use native or cold-copy procedures; there is no continuous point-in-time recovery. See [backup & restore](/operators/backup-restore/). |
| Object byte plane and erasure health | Available with caveats | On by default with a local pack leg, local-only placement, and a per-deployment master key created on first use; the S3 adapter reads and writes through it. Erasure coding and cloud placement targets are opt-in via environment variables. `nimbus object-storage` administers placement, GC and erasure health/healing, offline byte-plane backup/restore, and destructive tenant removal. See [object storage](/operators/object-storage/). |
| Server deployment | Available | [Linux servers](/operators/deploy-linux/), the official [container image](/operators/container-image/), and a [desktop install](/operators/desktop-install/) for the operator console. |

## Services, sandboxes, and machines

| Capability | Status | Notes |
| --- | --- | --- |
| Network control plane | Available with caveats | Node-local portable plans, stable attachment and endpoint identity, durable port and segment leases, capability evidence, readiness, and fenced recovery are implemented. Concrete effects stay with server, sandbox, machine, proxy, KV, and node providers. Multi-node cluster transport is not yet available. See the [network control plane](/concepts/architecture/network-control-plane/). |
| Service, sandbox, and session APIs | Available with caveats | Declared services, isolated sandboxes, and scoped sessions over HTTP and the SDK. Publicly created sandboxes run sealed — deny-all egress, no caller-set mounts or resource limits; sessions are control-plane leases whose channel byte transport is not yet exposed to clients. See the [resource model](/concepts/resource-model/). |
| Sandbox isolation backends | Available with caveats | Sandboxes run as containers or libkrun microVMs on Linux hosts, with deny-by-default egress. Non-Linux hosts need a machine (below). |
| Machines (`nimbus machine`) | Available with caveats | A managed Linux VM that hosts sandboxes on macOS (and WSL2 on Windows). See the [CLI reference](/reference/cli/). |
| Compose-declared services | Available | `nimbus compose` manages service workloads and exports systemd units. See [node lifecycle](/operators/node-lifecycle/). |

## Not yet

| Capability | Status | Notes |
| --- | --- | --- |
| Multi-node clustering and horizontal scale-out | Not yet | A Nimbus deployment is a single process today. See [scaling](/concepts/scaling/). |
| Continuous point-in-time recovery and external-backend backup command | Not yet | `nimbus backup` covers the embedded providers offline; external backends use their native tooling, and there is no continuous or arbitrary-timestamp recovery. See [backup & restore](/operators/backup-restore/). |
| MongoDB change streams | Not yet | See [MongoDB operations](/reference/mongodb/operations/). |
| Automatic updates | Not yet | The server checks for new versions but never upgrades itself. See [updates](/operators/updates/). |
