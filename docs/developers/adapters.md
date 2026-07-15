---
title: Adapters
description: Nimbus speaks the protocols your clients already use — one engine behind several front doors. What an adapter is, how each surface is served, and which one to reach for.
sidebar:
  label: Overview
  order: 1
---

An adapter is a protocol front door. Your client speaks the wire protocol it
already knows — Convex, Firestore, Cloud Functions, MongoDB, DynamoDB, or the
native API — and Nimbus serves it from the same engine, storage layer, and
tenant boundary as every other surface. Picking a door changes the shape of
your calls, not where your data lives or how it is isolated. The design and
its limits are explained in
[the adapter boundary](/concepts/adapter-boundary/).

## How the surfaces are served

Every server serves these surfaces by default. The Convex-compatible surface,
the Firestore routes, and the native API share the main listener; MongoDB and
DynamoDB each bind their own conventional port (`127.0.0.1:27017` and
`127.0.0.1:8000`).

`nimbus dev` wires your app automatically, and it treats two kinds of surface
differently.

**One app adapter.** The adapter that owns your app's wiring, codegen, and
watch loop is singular — exactly one is chosen:

| Nimbus sees | App adapter |
| --- | --- |
| a `convex/` directory | Convex |
| a `firebase` dependency | Firestore |
| a `firebase.json` | Cloud Functions |

**Any number of wire surfaces.** These are a set, and they compose with any
app adapter — a Convex app that also installs `mongodb` keeps its Convex dev
loop *and* gets the MongoDB listener:

| Runtime dependency | Wire surface |
| --- | --- |
| `mongodb` or `mongoose` | MongoDB — writes `NIMBUS_MONGODB_URL` to `.env.local` |
| `@aws-sdk/client-dynamodb` or `@aws-sdk/lib-dynamodb` | DynamoDB — writes `NIMBUS_DYNAMODB_*` to `.env.local` |
| `@aws-sdk/client-s3` or `@aws-sdk/lib-storage` | [S3](/reference/s3/compatibility/) |

Wire-surface detection reads only the runtime `dependencies` in your
`package.json` — not `devDependencies`, `optionalDependencies`, or
`peerDependencies` — because enabling one starts a listener, generates
credentials, and writes `.env.local`. A bare `aws-sdk` (v2) dependency is too
broad to imply DynamoDB or S3, so it only produces a banner hint and never
enables a surface.

The Firestore, MongoDB, DynamoDB, and S3 surfaces can each be switched off
(`--no-firestore`, `--no-mongodb`, `--no-dynamodb`, `--no-s3`); the
Convex-compatible surface and the native API are always on. Flags, ports, and
credentials are in [configuration](/reference/configuration/).

## Which surface should you reach for?

| Surface | Reach for it when | You write against | Migration guide |
| --- | --- | --- | --- |
| [Convex](/developers/convex/) | you want the full function model — queries, mutations, actions, and reactive subscriptions | `convex/` functions and the Convex clients | [Yes](/developers/convex/migrate/) |
| [Firestore](/developers/firebase/) | you have a Firebase app, or want Firestore's document SDKs | the stock `firebase/firestore` SDK | [Yes](/developers/firebase/migrate/) |
| [Cloud Functions](/developers/cloud-functions/) | you are porting Cloud Functions for Firebase workloads | `firebase-functions` v2 handlers | [Yes](/developers/cloud-functions/migrate/) |
| [MongoDB](/developers/mongodb/) | you already have code on the official MongoDB drivers | your existing MongoDB driver | — |
| [DynamoDB](/developers/dynamodb/) | you already have code on the AWS SDK | `@aws-sdk/client-dynamodb` | — |
| [Native API](/developers/native/) | you want any language, with no SDK | plain HTTP and WebSocket | — |

The ported-source surfaces (Convex, Firestore, Cloud Functions) carry a
migration guide because you are moving existing source onto Nimbus. The
driver surfaces (MongoDB, DynamoDB) and the native API need no migration —
you point an existing client at a Nimbus endpoint and keep your code.

Every surface ships runnable examples:
[Convex](/developers/convex/examples/),
[Firestore](/developers/firebase/examples/),
[Cloud Functions](/developers/cloud-functions/examples/),
[MongoDB](/developers/mongodb/examples/),
[DynamoDB](/developers/dynamodb/examples/), and
[Native API](/developers/native/examples/).

## Compare what each one supports

Surfaces are not interchangeable: each translates a different slice of the
engine. [Adapter capabilities](/reference/adapter-capabilities/) puts all six
side by side — CRUD, queries, indexes, subscriptions, transactions, schema
validation, auth, tenant binding, and default ports — with links into each
per-surface reference. The full per-surface compatibility matrices live in
the [Reference](/reference/) section.

## Other wire surfaces

Nimbus serves protocol surfaces that are not app front doors and have no
guides in this section: an [S3 API](/reference/s3/compatibility/) (wired by
`nimbus dev` when you declare an AWS S3 client, as above), a
[Cloudflare-compatible](/reference/cloudflare/compatibility/) Workers KV
plane, and a [RESP (Redis)](/reference/kv/compatibility/) listener started
with `nimbus kv`. Each is documented in the Reference section with its own
maturity caveat.
