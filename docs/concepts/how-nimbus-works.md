---
title: How Nimbus works
description: The single-binary architecture — one engine, one storage contract, and protocol adapters that let existing Convex, Firestore, Cloud Functions, MongoDB, and DynamoDB clients talk to it.
sidebar:
  label: How Nimbus works
  order: 2
---

Nimbus is a source-available reactive document backend that ships as one
binary. That binary is simultaneously the server, the protocol surfaces,
the function runtime, the storage layer, and the CLI you operate it with.
This page is the conceptual map: what lives inside the binary, how a
request moves through it, and why "drop-in compatible" is an architectural
property rather than a marketing one.

## One binary, five layers

Inside the single process, responsibilities are split into layers, each
with one job:

- **The transport layer** owns all network I/O: the native HTTP and
  WebSocket API, plus the routes and listeners that carry each
  compatibility protocol. It binds to loopback by default and is the
  integration point that wires every other layer together.
- **The protocol adapters** each own one foreign dialect — wire formats,
  auth handshakes, error shapes — and translate it into engine
  operations. Adapters are transport-agnostic libraries; the transport
  layer decides how each one is exposed.
- **The engine** is the central coordinator. Every read, write,
  subscription, and scheduled job — from any surface — flows through it.
  It owns validation, authorization, query planning, the mutation path,
  and subscription fan-out.
- **The runtime** executes your functions in-process in V8 isolates. It
  is deliberately standalone: it knows nothing about the rest of Nimbus
  and declares what it needs from the host through a single host-bridge
  contract. Bundles are SHA-256 integrity-checked before invocation.
- **The storage layer** sits behind one engine-facing contract with
  multiple providers: embedded SQLite is the default, redb is a second
  embedded option, and Postgres, MySQL, and libSQL are external options.
  See [storage backends](/operators/storage-backends/).

A small set of shared types defines the vocabulary — documents, tables,
mutations, schemas, queries — used across all of them, and the CLI wraps
the whole stack: `nimbus start` boots the server; `nimbus dev` runs the
local development loop.

## How a request flows

Every data operation, regardless of which protocol delivered it, ends up
on the same path:

```text
client → transport → adapter → engine → storage
```

The transport accepts the connection and admits the request — including
resolving which tenant it belongs to. The adapter for that protocol
decodes the wire format and authenticates the caller. The engine
authorizes the operation, validates it against the table's schema if one
exists, executes it, and the storage layer makes it durable. Results
travel back out through the same adapter, re-encoded in the dialect the
client expects.

Function invocations add one loop to this picture, not a second path.
When a request invokes one of your functions, the runtime runs it in a V8
isolate, and every host call it makes crosses the host-bridge contract
into a server-owned bridge that calls the same engine methods a direct
HTTP request would. A write from a function and a write from an HTTP
client converge on one engine-owned mutation path; there is no privileged
side channel for code running inside the database.
[Data and mutations](/concepts/data-and-mutations/) covers that path in
detail.

The runtime's isolation from the rest of the system is structural: it has
no dependency on the engine or storage and only defines the host-bridge
contract, which the rest of the stack supplies. Guest code can reach
exactly what the bridge exposes — nothing else. Functions can also opt
into Node.js compatibility targets while keeping the same in-process
invocation model; see
[how the Node runtime works](/concepts/nodejs-runtime/).

## Tenancy is a first-class boundary

A single Nimbus server hosts many tenants, and the tenant boundary is
built into every layer rather than filtered in at the edges: each request
is admitted to exactly one tenant before any lower layer sees it, every
engine operation is tenant-scoped, and each tenant owns a separate storage
namespace, so a cross-tenant query is inexpressible rather than merely
forbidden. This is also the scaling model — Nimbus scales by distributing
tenants, not by sharding inside one tenant's data. The full trust model is
in [tenant isolation](/concepts/tenant-isolation/), with operational
procedures in the
[tenant isolation operator guide](/operators/tenant-isolation/).

## What "drop-in compatible" means

Nimbus does not embed five databases. It runs one engine with one data
model, one mutation path, and one subscription system — and each adapter
teaches that engine to speak a foreign protocol natively enough that
existing clients work unchanged:

- **Convex**: HTTP function routes and the Convex WebSocket sync
  protocol, executing Convex-style queries, mutations, and actions in the
  V8 runtime.
- **Firestore**: the Firestore v1 API — REST document and query
  endpoints plus the gRPC surface, including `Listen` streams for
  realtime updates.
- **Cloud Functions**: HTTP-triggered and callable function workloads
  served over the same HTTP surface.
- **MongoDB**: a dedicated listener speaking the MongoDB wire protocol
  with SCRAM authentication, so official drivers connect with a normal
  connection string.
- **DynamoDB**: a dedicated HTTP endpoint accepting SigV4-signed,
  `X-Amz-Target`-dispatched requests from unmodified AWS SDK clients.

The architectural consequence is the interesting part: because adapters
translate at the boundary and share the engine underneath, every surface
inherits the same guarantees. A document written through the MongoDB
adapter is durably journaled, indexed, and pushed to subscribers exactly
like one written through Convex or the native API — the adapters own
dialect, never data semantics. Adapters authenticate callers and
normalize their identities; authorization happens once, in the engine,
for all of them.

Each adapter has a front door under [Developers](/developers/) and a
compatibility matrix under [Reference](/reference/), so you can see
precisely which operations each dialect supports today.

## Where to go next

- [Data and mutations](/concepts/data-and-mutations/) — the data model,
  the single mutation path, and the consistency guarantees it produces.
- [Tenant isolation](/concepts/tenant-isolation/) — the multi-tenant
  trust model in depth.
- [How the Node runtime works](/concepts/nodejs-runtime/) — the function
  runtime's compatibility contract.
- [Developers](/developers/) — pick a protocol surface and build.
- [Storage backends](/operators/storage-backends/) — choose and operate
  a persistence provider.
