---
title: Adapter crates
description: A crate-level tour of the five protocol adapters — Convex, Firestore, Cloud Functions, MongoDB, DynamoDB — the server shims that mount them, and the bridge layer behind their runtimes.
sidebar:
  label: Adapters
  order: 3
---

Nimbus speaks five foreign protocols through five dedicated adapter
crates. The conceptual contract — adapters translate, the engine decides
— is covered in [the adapter boundary](/concepts/adapter-boundary/);
this page is the crate-level map: which crate owns what, which way the
dependencies point, and how a request moves through the workspace.

## Five crates, one shape

| Crate | Owns | Mounted as |
| --- | --- | --- |
| `crates/nimbus-convex` | Convex function model, sync subscriptions, document identity, auth config | `/convex/{tenant}/*` routes + per-tenant WebSocket |
| `crates/nimbus-firebase` | Firestore v1 request/response model, generated gRPC protos, listen/write stream registries | `/v1/...` REST + Firestore gRPC service |
| `crates/nimbus-cloud-functions` | Functions app contract, registry, trigger executor, runtime invocation bridge | router fallback handler |
| `crates/nimbus-mongodb` | Wire protocol codec, command dispatch, SCRAM auth, cursors and sessions | dedicated TCP listener |
| `crates/nimbus-dynamodb` | Operation dispatch, AttributeValue codec, expression language, SigV4, key encoding | dedicated HTTP listener, `POST /` |

Every adapter is a plain library. None of them binds a socket, defines
an HTTP route, or knows that axum exists. Each exposes typed entry
points — `nimbus_dynamodb::dispatch`, `nimbus_mongodb::commands::dispatch`,
the operation functions in `nimbus-firebase`, the registries in
`nimbus-convex` and `nimbus-cloud-functions` — that take explicit
capabilities such as `Arc<Engine>` and return protocol-shaped results.

## Dependencies point at the engine, never the server

All five adapter crates depend on `nimbus-engine` (and `nimbus-core`
for the shared vocabulary of tenants, documents, queries, and errors).
None depends on `nimbus-server`. The DynamoDB crate states the rule in
its own module docs: it "must not depend on `nimbus-server` or `axum`"
— it exposes a dispatch entrypoint, and the server mounts it. The same
discipline holds across the set, with two refinements:

- Adapters that resolve a foreign namespace to a tenant
  (`nimbus-firebase`, `nimbus-mongodb`, `nimbus-dynamodb`,
  `nimbus-cloud-functions`) also depend on `nimbus-tenant` for
  isolation context.
- Adapters that execute user JavaScript — `nimbus-convex` and
  `nimbus-cloud-functions` — additionally depend on `nimbus-runtime`
  (the V8 surface) and on `nimbus-bridge` (below). The three pure
  data-protocol adapters have no runtime dependency at all.

The arrow never reverses: `nimbus-server` depends on every adapter
crate; no adapter crate can reach back into transport.

## The composition shims

Each adapter has a thin counterpart under
`crates/nimbus-server/src/adapters/` — the only place where protocol
meets transport:

- `crates/nimbus-server/src/adapters/convex/` owns the axum handlers
  for queries, mutations, actions, HTTP actions, and scheduling, the
  WebSocket socket loop, and the server-side host bridge wiring for
  Convex function execution.
- `crates/nimbus-server/src/adapters/firebase/` owns the REST handlers
  and, in its `grpc/` submodule, the tonic service implementation —
  unary calls, the listen stream, the write stream, and the WebSocket
  variant of `Listen`. The proto types and stream-state registries it
  builds on come from `nimbus_firebase::grpc`.
- `crates/nimbus-server/src/adapters/cloud_functions/` owns the
  fallback HTTP handler, callable-request handling, and the invoker
  that hands targets to the registry.
- `crates/nimbus-server/src/adapters/mongodb/` owns the TCP listener:
  accept loop, per-connection wire framing, and the loopback-only bind
  guard. `MongoDbConfig` lives here.
- `crates/nimbus-server/src/adapters/dynamodb/` owns the single-route
  listener and the TTL sweeper task, and re-exports `DynamoDbConfig`
  from the adapter crate, which owns its own config type.

A shim's whole vocabulary is extractors, headers, status codes, and
task lifecycles. If you find protocol semantics in a shim, it is in the
wrong crate.

## nimbus-bridge: the runtime host-call layer

When a deployed function calls `ctx.db.insert(...)`, that host call has
to cross from the V8 isolate back into the engine. The crate that owns
that crossing is `crates/nimbus-bridge` — a provider-neutral layer
shared by every runtime-executing adapter. Its modules tell the story:
`host_calls` executes synchronous and asynchronous host calls,
`capabilities` defines the engine-backed operations a runtime may
invoke, `admission` decides whether execution is admitted under the
tenant's isolation policy, `read_tracking` owns the canonical read-set
model used to drive reactive subscriptions, `responses` shapes the
envelope returned to the isolate, and `state` carries per-invocation
host state.

The bootstrap entry point, `build_runtime_host_bootstrap`, begins an
engine mutation execution unit whenever the invocation is a mutation —
which is how runtime writes end up on the same
[engine mutation path](/concepts/architecture/engine-mutation-path/) as
direct HTTP writes, with no bypass. Because both `nimbus-convex` and
`nimbus-cloud-functions` build their host bridges on `nimbus-bridge`,
the two function runtimes cannot drift apart on read-tracking, write
admission, or cancellation semantics. The isolate side of this seam is
described in [runtime isolates](/concepts/architecture/runtime-isolates/).

## Anatomy of a request

Two concrete walks, one routed and one on a sibling listener:

```text
Convex client                        DynamoDB SDK
POST /convex/{tenant}/mutation       POST /  (X-Amz-Target: ...PutItem)
        │                                    │
crates/nimbus-server                 crates/nimbus-server
  convex shim: route match,            dynamodb shim: dedicated port,
  tenant from path, bearer from        body-size cap, hand headers
  headers, registry lookup             and bytes to dispatch
        │                                    │
crates/nimbus-convex                 crates/nimbus-dynamodb
  validate function, verify            verify SigV4 → resolve tenant
  OIDC / custom JWT identity,          from access key, parse the
  build the invocation                 operation, AttributeValue → doc
        │                                    │
        └────────── crates/nimbus-engine ────┘
             one mutation path, tenant isolation,
             schema checks, indexes, commit log
        │                                    │
  Convex-shaped JSON result          DynamoDB-shaped JSON or
  via the shim                       exception name via the shim
```

The pattern generalizes: the **shim** accepts bytes on some transport;
the **adapter crate** normalizes and authenticates them into engine
vocabulary — `TenantId`, `Query`, `Mutation`, `Document`; the
**engine** executes under one set of rules; the adapter crate shapes
the result (including errors, in the protocol's own taxonomy) and the
shim writes it back. For functions, the middle step detours through the
runtime and returns via `nimbus-bridge`, but the engine call at the
bottom is the same.

## Per-adapter authentication

Each adapter verifies its protocol's native credentials inside the
adapter crate, then hands the engine a verified principal — never a raw
token. The shared trait lives in `crates/nimbus-auth`
(`ApplicationAuthVerifier`).

- **Convex** parses the deployment's auth configuration into OIDC
  providers (issuer domain plus application ID) and custom JWT
  providers (explicit issuer, JWKS, RS256 or ES256) in
  `crates/nimbus-convex/src/auth/`. The Convex registry implements
  `ApplicationAuthVerifier`. The server stores those verifiers in a
  deployment-owned silo map: the `/convex/{silo}` URL selects the verifier
  before the bearer is examined. A verifier result is therefore intrinsically
  bound to its silo, with no subject/issuer registry and no fallback to a
  process-wide Convex verifier. Other adapter families retain their own
  application-auth configuration.
- **Firestore** additionally supports an emulator-style mock user
  token mode, but only as an explicit opt-in on `FirebaseConfig`;
  it is off by default.
- **MongoDB** implements the SCRAM-SHA-256 conversation in
  `crates/nimbus-mongodb/src/auth.rs`, with per-connection
  conversation state; commands that require authentication refuse
  unauthenticated connections.
- **DynamoDB** verifies AWS SigV4 signatures in
  `crates/nimbus-dynamodb/src/auth/`. Strict verification is the
  default mode: signatures are checked against registered secrets and
  requests outside a ±15-minute timestamp window are rejected. Each
  access-key ID is bound to exactly one tenant, so the credential *is*
  the namespace. The signature-skipping lookup mode exists for local
  development only, and the server refuses to expose it beyond
  loopback.

How principals, tenants, and isolation decisions compose after this
point is the subject of [auth and trust](/concepts/architecture/auth-trust/)
and [tenancy](/concepts/architecture/tenancy/).

## What the layering buys

The mechanical payoff of this crate split, beyond the conceptual
guarantees of [the adapter boundary](/concepts/adapter-boundary/):
adapter behavior is tested as library code straight against an engine,
without standing up HTTP; every protocol funnels into the same engine
seam, so invariants are enforced once; and a new protocol lands as one
new crate plus one thin shim, leaving the existing surfaces untouched.
