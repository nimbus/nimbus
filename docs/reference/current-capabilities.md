---
title: Current capabilities
description: An honest snapshot of what Nimbus implements today — every major capability with a status and a link to its documentation.
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
| Firestore | Available | Enable with `nimbus start --firestore`. See the [guide](/developers/firebase/) and [compatibility reference](/reference/firebase/compatibility/). |
| MongoDB wire protocol | Available | Enable with `nimbus start --mongodb-port` plus SCRAM credentials (`--mongodb-username`, `NIMBUS_MONGODB_PASSWORD`). Loopback-only listener. See the [guide](/developers/mongodb/) and [operations reference](/reference/mongodb/operations/). |
| DynamoDB API | Available | Enable with `nimbus start --dynamodb-port` and `--dynamodb-access-key KEY_ID:SECRET:TENANT` bindings (dedicated listener, DynamoDB Local convention port 8000). See the [guide](/developers/dynamodb/) and [feature coverage](/reference/dynamodb/feature-coverage/). |
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
| Backup and restore | Available with caveats | Storage-level procedures only — no first-class backup command and no point-in-time recovery. See [backup & restore](/operators/backup-restore/). |
| Production deployment | Available | [Linux servers](/operators/deploy-linux/), the official [container image](/operators/container-image/), and a [desktop install](/operators/desktop-install/) for the operator console. |

## Services, sandboxes, and machines

| Capability | Status | Notes |
| --- | --- | --- |
| Service, sandbox, and session APIs | Available | Declared services, isolated sandboxes, and scoped sessions over HTTP and the SDK. See the [resource model](/concepts/resource-model/). |
| Sandbox isolation backends | Available with caveats | Sandboxes run as containers or libkrun microVMs on Linux hosts, with deny-by-default egress. Non-Linux hosts need a machine (below). |
| Machines (`nimbus machine`) | Available with caveats | A managed Linux VM that hosts sandboxes on macOS (and WSL2 on Windows). See the [CLI reference](/reference/cli/). |
| Compose-declared services | Available | `nimbus compose` manages service workloads and exports systemd units. See [node lifecycle](/operators/node-lifecycle/). |

## Not yet

| Capability | Status | Notes |
| --- | --- | --- |
| Multi-node clustering and horizontal scale-out | Not yet | A Nimbus deployment is a single process today. See [scaling](/concepts/scaling/). |
| First-class backup command and point-in-time recovery | Not yet | Back up the storage layer directly; see [backup & restore](/operators/backup-restore/). |
| MongoDB change streams | Not yet | See [MongoDB operations](/reference/mongodb/operations/). |
| Automatic updates | Not yet | The server checks for new versions but never upgrades itself. See [updates](/operators/updates/). |
