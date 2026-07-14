---
title: Server and transport
description: How the nimbus-server crate composes every protocol surface — HTTP routes, WebSockets, gRPC, and sibling wire listeners — onto one engine without owning any protocol semantics.
sidebar:
  label: Server & transport
  order: 2
---

`crates/nimbus-server` is the integration point of the Nimbus binary: the
one crate where every front door — native API, Convex, Firestore, Cloud
Functions, MongoDB, DynamoDB, health, diagnostics, operator console — is
mounted onto a shared engine. It is also, by design, one of the least
interesting crates in the workspace. The server crate owns transport:
routes, upgrades, headers, status codes, and listener lifecycles. It does
not own protocol semantics (those live in the
[adapter crates](/concepts/architecture/adapters/)) and it does not own
data semantics (those live in the engine — see
[the engine and mutation path](/concepts/architecture/engine-mutation-path/)).

## Router composition

The public composition surface is two option bundles.

`RouterOptions` and `build_router`, in
`crates/nimbus-server/src/router.rs`, build the main axum router. Every
protocol surface is opt-in through a builder method:
`with_convex_registry`, `with_cloud_functions_registry`,
`with_firebase_config`, plus operator concerns such as
`with_service_manager`, `with_local_server_security`, and
`with_tenant_isolation_mode`. The only required input is the shared
`Arc<Engine>` handle from `nimbus-engine`.

`ServeOptions` and `serve`, in
`crates/nimbus-server/src/construction.rs`, wrap `RouterOptions` and add
the surfaces that cannot be router routes: `with_mongodb` and
`with_dynamodb` configure sibling listeners on their own ports. `serve`
takes an *already-bound* `tokio::net::TcpListener` — the server crate
never decides what address to bind (more on that below) — then runs the
router with graceful shutdown and spawns the sibling listeners around it.

A surface that is not configured does not exist. With no Convex registry
there are no `/convex` routes; with no Firebase config the Firestore
router is never merged; with no Cloud Functions registry there is no
fallback handler. At startup, `serve` also records each active protocol
listener into the system tenant, so the server's own composition is
observable as data.

## The route families

One HTTP listener carries every routed family:

```text
one HTTP/WebSocket listener
├── /health                            liveness, unauthenticated
├── /examples, /examples/*             bundled example apps
├── /ui/*                              operator console (CSP + sessions)
├── /api/*  and  /ws                   native API (admin-gated)
├── /api/admin/deploy                  deploy API (admin header only)
├── /debug/*                           diagnostics (admin-gated)
├── /convex/{tenant}/*                 Convex HTTP + per-tenant /ws sync
├── /v1/projects/.../databases/...     Firestore v1 REST
├── /google.firestore.v1.Firestore/*   Firestore gRPC + gRPC-Web
└── (fallback)                         Cloud Functions HTTP targets
```

- **Native API.** Tenant lifecycle, documents, queries, schema, journal,
  scheduling, and cron routes under `/api/tenants/...`; machine
  lifecycle under `/api/machines/...`; sessions, sandboxes, and service
  control under `/api/sessions` and `/api/tenants/{tenant}/services`.
  The native WebSocket sync protocol lives at `/ws`, with negotiation in
  `crates/nimbus-server/src/ws/`.
- **Convex.** `/convex/{tenant}/query`, `mutation`, `action`, paginated
  queries, HTTP actions under `/convex/{tenant}/http`, scheduling
  routes, and the Convex sync WebSocket at `/convex/{tenant}/ws` — the
  endpoint Convex client libraries connect to for reactive
  subscriptions.
- **Firestore.** The v1 REST surface
  (`documents:commit`, `documents:batchWrite`, `documents:batchGet`,
  `documents:beginTransaction`, `documents:rollback`,
  `documents:listCollectionIds`, `documents:runQuery`,
  `documents:runAggregationQuery`) and the gRPC surface at
  `/google.firestore.v1.Firestore/{method}`, served through tonic and
  wrapped in a gRPC-Web layer so browser SDKs work without a proxy. The
  `Listen` route is special: a GET upgrades to a WebSocket listen
  channel while a POST is served as a gRPC stream, and both share one
  service instance so retained listen targets and write-stream state
  survive reconnects.
- **Cloud Functions.** Deployed HTTP and callable function targets are
  not enumerable as static routes, so the family mounts as the router's
  fallback handler: any path no other family claims is dispatched
  against the Cloud Functions registry.
- **Health and diagnostics.** `/health` is public. `/debug/*` routes —
  license status, encryption status, runtime metrics, per-tenant
  consistency and engine diagnostics — sit inside the admin-gated
  family.

## The local admin gate

Admin routes are gated by middleware layered in `router.rs`, with the
policy logic itself owned by the `nimbus-operator` crate and adapted to
axum in `crates/nimbus-server/src/local_server/middleware.rs`. Three
middlewares cooperate:

1. An **origin allowlist** middleware wraps the whole router. It
   classifies each request into a route family and, for families that
   require it, rejects cross-origin browser requests before any handler
   runs.
2. A **credential extraction** middleware parses what the caller
   presented — the local admin token (minted on disk at first boot,
   rotatable) or an operator console session.
3. A **route-family gate** turns the extracted result into an
   allow/deny decision and writes an audit record either way.

The standard admin family accepts either credential form; the deploy
family (`/api/admin/deploy`) is stricter and accepts only the admin
token header. Service-control routes perform authorization in their
handlers instead, using the principal-class checks in
`crates/nimbus-server/src/http/authz.rs`, because they must distinguish
operators from tenant and workload principals rather than apply one
blanket gate. Application traffic — Convex function calls, Firestore
reads — never uses the admin gate; each adapter authenticates its own
protocol (see [adapter crates](/concepts/architecture/adapters/) and
[auth and trust](/concepts/architecture/auth-trust/)).

## CORS posture

The CORS layer in `router.rs` allows browser origins on loopback by
default: `localhost`, `127.0.0.1`, and `[::1]` on any port, over HTTP or
HTTPS. Additional exact origins can be granted with
`--cors-allow-origin` (or the comma-separated
`NIMBUS_CORS_ALLOW_ORIGINS` environment variable); values are normalized
to the browser `Origin` form and wildcards are rejected, so the
allowlist stays exact-match. The allow-listed request headers include
the Firebase and Google client headers plus the gRPC-Web headers, and
the gRPC status headers are exposed, so stock browser SDKs work against
a local server out of the box. Unconfigured non-loopback browser
origins are refused — the same local-first posture as the bind policy
below.

## MongoDB and DynamoDB: sibling listeners, not routes

Two protocols cannot be paths on the main router. MongoDB is a raw TCP
wire protocol, not HTTP. DynamoDB clients expect to own an endpoint root
— every operation is a `POST /` dispatched by the `X-Amz-Target` header.
So `serve` gives each a dedicated listener on its own port, sharing the
same `Arc<Engine>`:

- The **MongoDB listener**
  (`crates/nimbus-server/src/adapters/mongodb/listener.rs`) accepts TCP
  connections and runs each on its own task with per-connection state.
  It refuses to bind a non-loopback address outright.
- The **DynamoDB listener**
  (`crates/nimbus-server/src/adapters/dynamodb/listener.rs`) is a
  single-route axum app — `POST /` forwarding to the adapter crate's
  dispatch — with a request body cap enforced before parsing. The
  default port follows the DynamoDB Local convention so stock SDK
  endpoint configuration works. Binding refuses non-loopback addresses
  when the access-key registry is in its signature-skipping development
  mode; the default strict SigV4 mode may bind anywhere. A background
  TTL sweeper task runs alongside the listener.

When the main HTTP server returns, the sibling listener tasks are
aborted with it — one lifecycle for the whole transport layer.

## Bind policy lives in the binary

`serve` accepting a pre-bound listener is a deliberate seam. Deciding
*what* to bind is the CLI binary's job, in
`crates/nimbus-cli/src/start/boot.rs` and
`crates/nimbus-cli/src/start/network_bind.rs`:

- Loopback is the default. Binding a non-loopback host requires an
  explicit `--allow-network` opt-in, checked before any expensive
  startup work so a typo fails fast.
- A second tripwire refuses a public bind until the local admin token
  has been explicitly rotated at least once, so the auto-minted
  first-boot credential is never exposed to a network; rotations older
  than 30 days log an advisory warning without blocking restarts.
- Under systemd socket activation the binary adopts the inherited
  socket instead of binding, then applies the same two checks to the
  activated address.

The server crate stays policy-free: an embedder using
`nimbus-server` as a library makes its own listener decisions, while
`nimbus start` keeps the opinionated safe default.

## Why the server crate is thin

Keeping `nimbus-server` as composition glue is what makes the rest of
the architecture hold:

- **Protocol semantics stay testable without HTTP.** Each adapter crate
  is a plain library exercised directly against an engine; the server
  contributes only extractors and response plumbing.
- **One protocol can ride multiple transports.** Firestore `Listen` is
  served as gRPC, gRPC-Web, and WebSocket from a single shared service
  precisely because the semantics live below the transport.
- **The engine seam stays singular.** Every family, on every port, ends
  at the same `Arc<Engine>` — there is no per-protocol side door to the
  data (see [the adapter boundary](/concepts/adapter-boundary/)).
- **Adding a surface is additive.** A new protocol means a new adapter
  crate and a thin shim; the change to `nimbus-server` is another
  builder method and another merge into the router.
