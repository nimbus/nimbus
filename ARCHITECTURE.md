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

All Rust workspace members, per the root `Cargo.toml`.

| Crate | Role |
| --- | --- |
| `nimbus` | Public facade re-exporting the stable surface for embedders. |
| `nimbus-adapters` | Feature-gated facade of re-exports over the adapter crates; no logic. |
| `nimbus-artifacts` | Artifact verification: OCI references, SLSA provenance, admission checks. |
| `nimbus-assets` | Embedded production asset catalog (distribution payloads, UI bytes, templates). |
| `nimbus-auth` | Application auth contract: `ApplicationAuthVerifier` bearer-token verification into `InvocationAuth`. |
| `nimbus-bin` | The `nimbus` binary entrypoint: a 5-line `main.rs` that installs tracing and calls into `nimbus-cli`. All CLI logic lives in `nimbus-cli`. |
| `nimbus-blob` | Content-addressed per-tenant byte storage and blob encryption decorator, with framed AEAD sourced from `nimbus-crypto`. |
| `nimbus-bridge` | Runtime host bridge: bootstraps per-invocation host state and routes V8 host calls into the engine. |
| `nimbus-cli` | The `nimbus` CLI application library: `start`, `dev`, `deploy`, `run`, `sandbox`, `init`, `machine`, `backup`, `compose`, `encryption`, codegen, and more. Invoked by the thin `nimbus-bin` entrypoint. |
| `nimbus-cloud-functions` | Cloud Functions-compatible adapter contracts and runtime bridge. |
| `nimbus-code-index` | Deploy-time structural JavaScript/TypeScript code-navigation index built with oxc. |
| `nimbus-convex` | Convex protocol semantics: function registry, subscriptions, document identity, host-call payloads. |
| `nimbus-core` | Shared types and validation. Zero I/O. |
| `nimbus-crypto` | At-rest envelope/keyring primitives, crypto-shred, and framed blob AEAD; depends only on `nimbus-core` plus external crypto crates. |
| `nimbus-dynamodb` | DynamoDB wire-protocol adapter: AttributeValue conversion, expressions, operation dispatch, SigV4, streams. |
| `nimbus-egress` | Egress policy compilation and enforcement-plan types (rules, DLP, credential injection) shared by `nimbus-proxy` and the runtime egress gateway. |
| `nimbus-engine` | Central coordinator (`Engine`): mutation path, query evaluation, subscriptions, scheduler, triggers. |
| `nimbus-firebase` | Firestore protocol semantics: REST/gRPC request models, queries, transactions, serialization. |
| `nimbus-fs` | In-process filesystem shell for V8 and WASI binders: mount table, `FsCaps`, and backends (Seam C). |
| `nimbus-kv` | RESP-native Nimbus KV listener: tenant-bound authentication over the tenant-aware storage tiering seam. |
| `nimbus-license` | License file loading and status (community / trial / enterprise). |
| `nimbus-machine` | Render-independent machine records and provider contracts shared by CLI and server. |
| `nimbus-mongodb` | MongoDB wire protocol: BSON bridging, command handlers, connections, auth. |
| `nimbus-node` | Node-side workload lifecycle: systemd transient units over D-Bus, reconciler, host lifecycle backends. |
| `nimbus-object-storage` | Native object-storage control-plane resolver: turns persisted placement policy and operator config into `BlobStore` compositions shared by S3, Convex `_storage`, and backup/restore. Deliberately not a protocol crate. |
| `nimbus-operator` | Operator (host administrator) security model for local and deploy servers. |
| `nimbus-provenance` | Runtime-bundle provenance policy plumbing over the artifact verifier. |
| `nimbus-proxy` | Pingora-based egress proxy: policy enforcement, DNS/CONNECT handling, TLS interception, connection pooling, and fairness for tenant egress traffic. |
| `nimbus-runtime` | V8 execution via `deno_core`; defines the runtime surface and the `HostBridge` trait. Zero workspace dependencies. |
| `nimbus-s3` | S3-compatible object surface over the Nimbus blob and metadata planes (Seam D). |
| `nimbus-sandbox` | Backend-agnostic sandbox and isolation lifecycle contracts. |
| `nimbus-server` | HTTP/WebSocket transport: axum router, adapter transport shims, embedded UI, local-server security. |
| `nimbus-services` | Service registry and service-manager primitives. |
| `nimbus-storage` | Persistence providers (SQLite, redb, Postgres, MySQL, libSQL) plus commit log, indexes, and scheduler state. |
| `nimbus-system` | System-tenant records, route inventory, and status projections. |
| `nimbus-tenant` | Tenant isolation decisions, workload identity, and admission policy. |
| `nimbus-testing` | Shared test fixtures and the deterministic verification harness. |
| `nimbus-workload-identity` | Workload-identity issuance seam: provider-auth policy, admission-anchored mint authorization, node/machine identity and trust-domain config, and short-lived JWT/SPIFFE-SVID minting (SI0–SI4). Production cluster-membership identity stays unconstructible until HS1; the `WorkloadIdentity` projection stays in `nimbus-tenant`. |
| `nimbus-workloads` | Workload admission, desired-state, placement, and execution-control seams shared by the node reconciler and scheduler. |
| `workspace-hack` | `cargo-hakari`-managed dependency-unification package. No product logic; exists only to speed up workspace builds. |

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
3. **Every mutation flows through an engine-owned commit path.** HTTP,
   WebSocket, scheduler, and runtime-originated writes never reach storage
   directly. There are three such paths, all engine-owned, and naming all
   three matters: an audit boundary that lists only the first will not notice
   a defect living in the other two.
   - The **queued journal path**, where client mutations are batched and
     committed in sequence order by the per-tenant committer. Its ordered
     publisher or serial actor arm is selected once when the tenant runtime is
     constructed; every queued, direct, execution-unit, progress-sync, and
     internal job consults that same immutable selection.
   - The **direct path**, `apply_mutation_with_mode*` behind the public
     `insert_document*`, `update_document*`, and `delete_document*` methods on
     `Engine` (`crates/nimbus-engine/src/engine/mutations/`).
   - The **execution-unit path**, `MutationExecutionUnit`, which runtime
     mutations use so that all writes in one function invocation commit as a
     single transaction (`crates/nimbus-engine/src/engine/execution_units/`).

   There is no side channel, and no fourth path may be added. Every one of
   these routes must classify an ambiguous durable outcome the same way — see
   the crash-and-replay obligation in the mutation-path docs.
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
seams. Provider-family Firestore path, default-database, and storage-locator
lowering (shared by Firebase and Cloud Functions) lives in `nimbus-core::firestore`.
`crates/nimbus-server/src/adapters/` holds only thin transport shims that mount
those crates; MongoDB and DynamoDB additionally run their own listeners.
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
Wasmtime runs WASM component bundles (opt-in; V8 remains the default
production backend) under the same `RuntimePolicy` and `HostBridge`
authority model, with cooperative fuel scheduling, a retained Store pool,
and bundle integrity enforcement; its plan verifier
(`scripts/verify-wasmtime-backend.sh`) gates the lane.
Host calls cross the `HostBridge` trait into `nimbus-bridge`, which routes
them to the engine.
→ <https://nimbusdocs.com/concepts/architecture/runtime-isolates/>

### Sandbox & machines

`nimbus-sandbox` defines backend-agnostic sandbox and isolation lifecycle
contracts; `nimbus-machine` owns the machine record model and provider
contracts shared by the CLI and the server control plane.
→ <https://nimbusdocs.com/concepts/architecture/sandbox-machines/>

### Egress & network trust

Workload egress is decided and enforced by two crates with a strict
PDP/PEP split: `nimbus-egress` is the pure decision core (compiled
allow-list policy, credential/DLP metadata, the node-global
`LayeredEgressPolicy` allow-ceiling — no transport dependencies), and
`nimbus-proxy` is the enforcement plane (the Pingora-based per-workload
`WorkloadPep` with per-sandbox listeners, ephemeral per-sandbox CA for
selective HTTPS interception, and an append-only decision log written
before any client-visible response). A node-scoped `EgressEngine` owns the
PEP lifecycle registry — keyed by `nimbus_core::WorkloadId`, never consulted
on the request path (enforced by a reachability lint) — and hosts the
node-wide seams: per-tenant fairness budgets and the decision-event fan-out.
Denies fail closed everywhere, including when policy requires proxy
enforcement but no PEP path exists.

### Auth & trust

Two principal classes are kept distinct: operators (host administrators,
`nimbus-operator`) and application users (`nimbus-auth`'s
`ApplicationAuthVerifier`). Artifact and runtime-bundle trust is enforced by
`nimbus-artifacts` and `nimbus-provenance`. Workload credential issuance
(distinct from either of the above) is a separate ladder — see
[Tenancy](#tenancy) below.
→ <https://nimbusdocs.com/concepts/architecture/auth-trust/>

### Tenancy

Every tenant gets an isolated persistence namespace. `nimbus-tenant` owns
isolation decisions, workload identity, and admission policy; `nimbus-system`
owns the system tenant's records and projections.

**Workload-identity ladder.** Three deliberately separate types carry a
workload's identity through the stack, each anchored to a different concern:

| Layer | Type | Home | Role |
| --- | --- | --- | --- |
| Routing key | `WorkloadId` | `nimbus-core/src/types.rs` | Opaque routing key. Lives in `nimbus-core` on purpose: `nimbus-proxy`'s per-workload PEP registry keys on it without depending on `nimbus-sandbox`. |
| Admitted identity | `WorkloadIdentity` | `nimbus-tenant/src/identity.rs` | Rich projection, constructible only via `from_decision(&TenantIsolationDecision)`; renders SPIFFE-shaped `subject()`/`spiffe_id()` strings. |
| Node-local name | `TenantWorkloadId` | `nimbus-node/src/host_lifecycle.rs` | systemd unit naming. |

`WorkloadIdentity` stays in `nimbus-tenant` rather than moving into a
dedicated identity crate: construction is gated on holding a real
`TenantIsolationDecision`, so minting is unreachable without an admission
decision. `nimbus-workload-identity` builds credential issuance (provider
auth policy, admission-anchored mint authorization, claim sets, mint/deny
audit events, short-lived JWT minting) on top of that projection instead of
replacing it — its mint-request types are likewise constructible only from a
`TenantIsolationDecision`, so the admission anchor holds end to end from
routing key to minted credential.
→ <https://nimbusdocs.com/concepts/architecture/tenancy/>

### Node lifecycle

`nimbus-node` manages workloads on a host: systemd transient units driven
over D-Bus, a desired-state reconciler, and host lifecycle backends with
status evidence.
→ <https://nimbusdocs.com/concepts/architecture/node-lifecycle/>

### CLI & codegen

`nimbus-bin` is a 5-line entrypoint (`crates/nimbus-bin/src/main.rs`) that
installs tracing and calls into `nimbus-cli`, which builds the actual command
surface. `crates/nimbus-cli/src/start/` boots the server; sibling modules
implement the rest (`dev`, `deploy`, `run`, `sandbox`, `init`, `machine`,
`backup`, `compose`, `encryption`, auth/token management, and codegen, which
embeds `@nimbus/codegen`).
→ <https://nimbusdocs.com/concepts/architecture/cli-codegen/>

### SDK & packages

`packages/nimbus` is the canonical first-party JS/TS SDK (zero runtime
dependencies). Compat packages come in two deliberate shapes:
**compat-over-canonical-SDK** — `packages/convex` re-exports/adapts
`@nimbus/nimbus` and must stay thin (the capability-boundary lint blocks it
from reaching past the adapter surface into raw transports) — and
**independent-wire-protocol** — `packages/firebase` is a standalone
Firestore gRPC/protobuf client (mostly generated code, no `@nimbus/nimbus`
dependency) because Firestore's wire contract is not the Nimbus sync
protocol; its size is protocol surface, not wrapper drift. MongoDB and
DynamoDB packages are trivially thin connection helpers over the official
drivers, since Nimbus speaks those wire protocols natively.
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
