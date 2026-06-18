# Architecture

Nimbus is a source-available, single-binary backend server: one `nimbus`
executable that speaks five protocol surfaces — Convex, Firestore (Firebase),
Cloud Functions, MongoDB, and DynamoDB — over a single engine, storage layer,
and V8 runtime. Clients keep their existing SDKs and drivers; Nimbus implements
the wire protocols. This file is a contributor map: enough to orient in the
repo and find the owning crate. The full architecture tour lives at
<https://nimbusdocs.com/concepts/architecture/>.

## Workspace map

### Rust crates (`crates/`)

All 27 workspace members, per the root `Cargo.toml`.

| Crate | Role |
| --- | --- |
| `nimbus` | Public facade re-exporting the stable surface for embedders. |
| `nimbus-adapters` | Feature-gated facade of re-exports over the adapter crates; no logic. |
| `nimbus-artifacts` | Artifact verification: OCI references, SLSA provenance, admission checks. |
| `nimbus-assets` | Embedded production asset catalog (distribution payloads, UI bytes, templates). |
| `nimbus-auth` | Application auth contract: `ApplicationAuthVerifier` bearer-token verification into `InvocationAuth`. |
| `nimbus-bin` | The `nimbus` CLI binary: `start`, `dev`, `deploy`, `init`, `machine`, `data`, `compose`, `encryption`, codegen, and more. |
| `nimbus-bridge` | Runtime host bridge: bootstraps per-invocation host state and routes V8 host calls into the engine. |
| `nimbus-cloud-functions` | Cloud Functions-compatible adapter contracts and runtime bridge. |
| `nimbus-convex` | Convex protocol semantics: function registry, subscriptions, document identity, host-call payloads. |
| `nimbus-core` | Shared types and validation. Zero I/O. |
| `nimbus-dynamodb` | DynamoDB wire-protocol adapter: AttributeValue conversion, expressions, operation dispatch, SigV4, streams. |
| `nimbus-engine` | Central coordinator (`Engine`): mutation path, query evaluation, subscriptions, scheduler, triggers. |
| `nimbus-firebase` | Firestore protocol semantics: REST/gRPC request models, queries, transactions, serialization. |
| `nimbus-license` | License file loading and status (community / trial / enterprise). |
| `nimbus-machine` | Render-independent machine records and provider contracts shared by CLI and server. |
| `nimbus-mongodb` | MongoDB wire protocol: BSON bridging, command handlers, connections, auth. |
| `nimbus-node` | Node-side workload lifecycle: systemd transient units over D-Bus, reconciler, host lifecycle backends. |
| `nimbus-operator` | Operator (host administrator) security model for local and deploy servers. |
| `nimbus-provenance` | Runtime-bundle provenance policy plumbing over the artifact verifier. |
| `nimbus-runtime` | V8 execution via `deno_core`; defines the runtime surface and the `HostBridge` trait. Zero workspace dependencies. |
| `nimbus-sandbox` | Backend-agnostic sandbox and isolation lifecycle contracts. |
| `nimbus-server` | HTTP/WebSocket transport: axum router, adapter transport shims, embedded UI, local-server security. |
| `nimbus-services` | Service registry and service-manager primitives. |
| `nimbus-storage` | Persistence providers (SQLite, redb, Postgres, MySQL, libSQL) plus commit log, indexes, and scheduler state. |
| `nimbus-system` | System-tenant records, route inventory, and status projections. |
| `nimbus-tenant` | Tenant isolation decisions, workload identity, and admission policy. |
| `nimbus-testing` | Shared test fixtures and the deterministic verification harness. |

### npm packages (`packages/`)

| Package (npm name) | Role |
| --- | --- |
| `packages/codegen` (`@nimbus/codegen`) | Internal codegen embedded in the `nimbus` binary; generates TypeScript types and runtime artifacts. Not published. |
| `packages/convex` (`convex`) | Drop-in `convex` package that points an existing Convex app at Nimbus with no source changes. |
| `packages/dynamodb` (`@nimbus/dynamodb`) | Connection helpers for pointing the official AWS SDK at a Nimbus DynamoDB endpoint. |
| `packages/firebase` (`firebase`) | Drop-in `firebase` package mirroring the modular `firebase/app` + `firebase/firestore` API against Nimbus; stock imports work unchanged. |
| `packages/mongodb` (`@nimbus/mongodb`) | Connection-string helper for pointing the official MongoDB Node.js driver at Nimbus. |
| `packages/nimbus` (`@nimbus/nimbus`) | First-party JavaScript/TypeScript SDK (`NimbusClient`, `NimbusProvider`, `useNimbus`). |
| `packages/nimbus-ui` (`nimbus-ui`) | Embedded operator console SPA served by `nimbus-server` at `/ui/*`. |

Naming note: `crates/nimbus` is the Rust embedding facade; `packages/nimbus`
is the JavaScript SDK. When you say "nimbus", say which.

## Architecture invariants

These rules are load-bearing. Breaking one requires an architecture
discussion, not a workaround.

1. **`nimbus-core` has zero I/O.** Types and validation only — no file reads,
   no network calls.
2. **`nimbus-runtime` has zero workspace dependencies.** It defines the V8
   execution surface and the `HostBridge` trait; all Nimbus-specific
   integration lives in the bridge implementation outside the crate.
3. **Every mutation flows through the engine-owned mutation path.** HTTP,
   WebSocket, scheduler, and runtime-originated writes all converge on the
   `apply_mutation_with_mode*` family behind the public `insert_document*`,
   `update_document*`, and `delete_document*` methods on `Engine`
   (`crates/nimbus-engine/src/engine/mutations/`). There is no side channel.
4. **Storage commits are atomic.** The document write, its supporting index
   effects, and the commit-log append happen in one storage transaction.
   Never a document without its index entries; never a commit entry without
   its document write.
5. **Runtime bundles are integrity-checked against their provenance.** A bundle
   loaded with a recorded SHA-256 is re-hashed and compared against that hash
   before every invocation; a tampered or stale bundle is rejected
   (`verify_integrity`, `crates/nimbus-runtime/src/runtime/bundle.rs`). A
   path-backed bundle loaded without recorded provenance carries no expected
   hash, so it is admitted on filesystem trust alone.
6. **Schema is optional.** A table without a schema accepts any document.
   Setting a schema adds constraints but never removes the ability to write.

## System index

Each system has a full page on the docs site; the paragraphs here are only
the repo-side orientation.

### Server & transport

`nimbus-server` owns all network I/O: the axum HTTP router, WebSocket
connections, the embedded operator UI at `/ui/*`, and local-server access
policy. `crates/nimbus-server/src/router.rs` is the composition root where
every route family is mounted.
→ <https://nimbusdocs.com/concepts/architecture/server-transport/>

### Adapters

Protocol semantics live in standalone crates — `nimbus-convex`,
`nimbus-firebase`, `nimbus-cloud-functions`, `nimbus-mongodb`,
`nimbus-dynamodb` — supported by the shared `nimbus-bridge` and `nimbus-auth`
seams. `crates/nimbus-server/src/adapters/` holds only thin transport shims
that mount those crates; MongoDB and DynamoDB additionally run their own
listeners.
→ <https://nimbusdocs.com/concepts/architecture/adapters/>

### Engine & mutation path

`nimbus-engine` exports `Engine` (`crates/nimbus-engine/src/engine/mod.rs`),
the coordinator every read, write, subscription, and scheduled job flows
through. Writes follow the single mutation path described in the invariants;
queries go through a pure evaluator with schema lookup and index selection
handled by the engine. A triggers subsystem
(`crates/nimbus-engine/src/triggers/`) is also part of this crate.
→ <https://nimbusdocs.com/concepts/architecture/engine-mutation-path/>

### Storage

`nimbus-storage` provides one provider per backend: SQLite, redb, Postgres,
MySQL, and libSQL, plus a simulation provider for tests. The at-rest format is
backend-conditional — SQLite stores documents in JSON columns, while the redb
store uses a MessagePack document codec — behind the same provider traits.
→ <https://nimbusdocs.com/concepts/architecture/storage/>

### Runtime & isolates

`nimbus-runtime` executes user JavaScript in V8 isolates via `deno_core`,
with watchdogs, limits, metrics, and Node-compatibility layers. The default
backend is V8; a feature-gated Bun/JSC backend exists under
`crates/nimbus-runtime/src/backends/bun_jsc/` (non-default, fail-closed).
Host calls cross the `HostBridge` trait into `nimbus-bridge`, which routes
them to the engine.
→ <https://nimbusdocs.com/concepts/architecture/runtime-isolates/>

### Sandbox & machines

`nimbus-sandbox` defines backend-agnostic sandbox and isolation lifecycle
contracts; `nimbus-machine` owns the machine record model and provider
contracts shared by the CLI and the server control plane.
→ <https://nimbusdocs.com/concepts/architecture/sandbox-machines/>

### Auth & trust

Two principal classes are kept distinct: operators (host administrators,
`nimbus-operator`) and application users (`nimbus-auth`'s
`ApplicationAuthVerifier`). Artifact and runtime-bundle trust is enforced by
`nimbus-artifacts` and `nimbus-provenance`.
→ <https://nimbusdocs.com/concepts/architecture/auth-trust/>

### Tenancy

Every tenant gets an isolated persistence namespace. `nimbus-tenant` owns
isolation decisions, workload identity, and admission policy; `nimbus-system`
owns the system tenant's records and projections.
→ <https://nimbusdocs.com/concepts/architecture/tenancy/>

### Node lifecycle

`nimbus-node` manages workloads on a host: systemd transient units driven
over D-Bus, a desired-state reconciler, and host lifecycle backends with
status evidence.
→ <https://nimbusdocs.com/concepts/architecture/node-lifecycle/>

### CLI & codegen

`nimbus-bin` builds the `nimbus` binary. `crates/nimbus-bin/src/start/` boots
the server; sibling modules implement the rest of the command surface (`dev`,
`deploy`, `init`, `machine`, `data`, `compose`, `encryption`, auth/token
management, and codegen, which embeds `@nimbus/codegen`).
→ <https://nimbusdocs.com/concepts/architecture/cli-codegen/>

### SDK & packages

`packages/nimbus` is the canonical first-party JS/TS SDK; `packages/convex`
is a compatibility wrapper over it for existing Convex apps. The Firebase,
MongoDB, and DynamoDB packages are thin helpers because Nimbus speaks those
wire protocols natively.
→ <https://nimbusdocs.com/concepts/architecture/sdk-packages/>

### Observability

Structured logging uses `tracing` across the server and engine. The server
tracks per-segment request latency, the runtime exposes metrics snapshots,
and storage has its own diagnostics surface.
→ <https://nimbusdocs.com/concepts/architecture/observability/>

## Where to start reading

- `crates/nimbus-bin/src/main.rs` — CLI entry point and command dispatch.
- `crates/nimbus-bin/src/start/mod.rs` — server boot path.
- `crates/nimbus-server/src/router.rs` — every mounted route family in one place.
- `crates/nimbus-engine/src/engine/mod.rs` — the `Engine` struct.
- `crates/nimbus-engine/src/engine/mutations/direct/api.rs` — the public mutation API.
- `crates/nimbus-runtime/src/lib.rs` — runtime surface and `HostBridge` exports.
- `crates/nimbus-storage/src/lib.rs` — storage providers and shared persistence seams.
- `packages/nimbus/src/index.ts` — JS SDK entry point.
