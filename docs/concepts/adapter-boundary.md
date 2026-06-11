---
title: The adapter boundary
description: How Nimbus's protocol adapters — Convex, Firestore, Cloud Functions, MongoDB, DynamoDB — relate to the one engine that owns the data and its guarantees.
sidebar:
  label: Adapter boundary
  order: 5
---

Nimbus is one engine with several front doors. The same server can speak
the Convex client protocol, the Firestore gRPC API, the Cloud Functions
trigger and invocation model, the MongoDB wire protocol, and the DynamoDB
HTTP API — and behind every one of those surfaces sits the same engine,
the same per-tenant document store, and the same rules.

The piece that makes this work is the **adapter boundary**: a strict
division of labor between the protocol adapters and the engine. Adapters
translate; the engine decides. This page explains that division and what
it buys you. For hands-on guides, start from the front door you care
about: [Convex](/developers/convex/), [Firebase](/developers/firebase/),
[Cloud Functions](/developers/cloud-functions/),
[MongoDB](/developers/mongodb/), [DynamoDB](/developers/dynamodb/), or
the [native API](/developers/native/).

## Adapters translate

An adapter's whole job is translation at the edge. Each one handles four
kinds of foreignness so the engine never has to:

- **Wire protocol.** The Convex adapter speaks Convex's HTTP and
  WebSocket sync protocol. The Firestore adapter speaks the Firestore
  gRPC surface, including its streaming listen and write channels. The
  MongoDB adapter speaks the MongoDB wire protocol on its own listener.
  The DynamoDB adapter speaks DynamoDB's target-dispatched HTTP API and
  its attribute-value encoding.
- **Authentication.** Each protocol's native handshake is verified at
  the adapter and translated into a Nimbus principal: OIDC and custom
  JWT identities on the Convex surface, SCRAM-SHA-256 on the MongoDB
  surface, SigV4 request signing on the DynamoDB surface. What reaches
  the engine is a verified principal, never a raw foreign credential.
- **Namespace mapping.** Every protocol has its own idea of "which
  database am I talking to" — a Convex deployment, a Firestore project,
  a MongoDB database name, a DynamoDB access key. The adapter resolves
  that idea to exactly one Nimbus tenant, and maps the protocol's
  collection or table concept onto the tenant's tables. On the DynamoDB
  surface the binding is credential-based: each registered access key is
  bound to one tenant, so a request is scoped by who signed it.
- **Result shaping.** Responses, errors, and change events go back out
  in the dialect the client expects — Firestore document protos and
  listen events, MongoDB reply documents and error codes, DynamoDB's
  exception names. Foreign clients should not be able to tell they are
  not talking to the original service, including when things fail.

Translation is the *limit* of an adapter's authority. An adapter never
defines what a write means, when it becomes visible, or whether it is
allowed.

## The engine owns semantics

Everything below the translation line is owned once, by the engine
(`nimbus-engine`) and the storage layer beneath it:

- **Documents and tables.** One per-tenant document store, structurally
  scoped per tenant (see [tenant isolation](/concepts/tenant-isolation/)).
- **The write path.** Every write — from any adapter, from the
  scheduler, or from a server-side function — becomes an atomic write
  batch executed by the engine. The document write, its index effects,
  and the commit record succeed or fail as one transaction.
- **Schema and validation.** Table schemas, document identity, and
  index maintenance are engine rules. An adapter cannot relax them for
  its own protocol.
- **Reads and queries.** Adapters lower their query dialects into
  engine query evaluation; they do not run their own scans against
  storage.
- **Reactivity.** Subscriptions, change streams, and trigger feeds all
  derive from the engine's committed-mutation events. Convex
  subscriptions, Firestore listen streams, Cloud Functions document
  triggers, and DynamoDB streams are four projections of the same
  commit history, not four change-tracking systems.
- **Scheduling and functions.** Scheduled work and server-side function
  execution run through the same engine paths, under the same
  [runtime permission model](/concepts/runtime-permissions/).

## No adapter has its own write path

This is the load-bearing rule. There is exactly one mutation path in
Nimbus, and every surface uses it:

- A MongoDB `insertMany`, a DynamoDB `PutItem`, a Firestore commit, and
  a Convex mutation all become engine write batches on the same code
  path.
- A server-side function calling `ctx.db.insert(...)` goes through the
  engine host-call bridge into that same path — running inside the
  runtime does not create a side channel.
- Function bundles are integrity-checked before execution, and host
  calls run under a session bound to the admitted tenant and principal.

Because there is no second path, there is no surface where atomicity,
schema validation, index maintenance, or tenant scoping could quietly
differ. A guarantee proven once on the engine path holds on every
protocol.

## What this buys you

**One data store, many protocols.** Your data lives in one place. The
protocol an application speaks is a client-side choice, not a data
placement decision: teams can keep an existing MongoDB driver, a
Firestore SDK, or a DynamoDB client while operating against the same
Nimbus server, and adopt another surface later without a migration.

**Consistent guarantees everywhere.** Atomic writes, schema rules,
index correctness, tenant isolation, and reactive updates behave the
same on every surface, because they are implemented once. There is no
"the Mongo surface is weaker about X" class of surprise — compatibility
differences are differences of *dialect coverage*, not of storage
semantics.

**Honest compatibility boundaries.** Since adapters only translate,
what each one supports is a checkable contract over a known engine. The
per-surface contracts are published as reference pages —
[Convex](/reference/convex/compatibility/),
[Firebase](/reference/firebase/compatibility/),
[Cloud Functions](/reference/cloud-functions/compatibility/),
[MongoDB](/reference/mongodb/operations/), and
[DynamoDB](/reference/dynamodb/feature-coverage/) — and a gap is always
"this expression isn't translated yet," never "this surface stores data
differently."

**A smaller trust surface.** Operators reason about one write path, one
permission model, and one isolation boundary, regardless of how many
protocols are enabled. Enabling an additional front door adds a
translator, not a new database.

## Where translation ends

The boundary also says what an adapter will *not* do. An adapter does
not emulate a foreign system's internals — its replication mechanics,
its operational tooling, its pricing-model artifacts — and it does not
bend engine rules to chase edge-case parity when doing so would fork
the semantics that every other surface relies on. Each compatibility
reference documents where its surface diverges for exactly this reason.

## Related pages

- [Tenant isolation](/concepts/tenant-isolation/) — the boundary every
  adapter request is admitted into.
- [Runtime permissions](/concepts/runtime-permissions/) — what
  server-side functions can touch once invoked.
- [How the Node runtime works](/concepts/nodejs-runtime/) — the
  function runtime that shares the same host-call path.
