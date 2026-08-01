---
title: Adapters
description: Nimbus speaks the protocols your clients already use — one engine behind several front doors. What an adapter is, how each surface is served, and which one to reach for.
sidebar:
  label: Overview
  order: 1
---

An adapter is a protocol front door. Your client uses a known wire protocol,
such as Convex, Firestore, Cloud Functions, MongoDB, DynamoDB, or the native
API. Nimbus serves every protocol from the same engine, storage layer, and
tenant boundary. Your choice changes the call shape. It does not change data
location or isolation. [The adapter boundary](/concepts/adapter-boundary/)
explains the design and its limits.

## How the surfaces are served

Every server serves these surfaces by default. The Convex-compatible surface,
the Firestore routes, and the native API share the main listener. MongoDB and
DynamoDB bind their conventional ports: `127.0.0.1:27017` and
`127.0.0.1:8000`.

`nimbus dev` wires your app automatically, and it treats two kinds of surface
differently.

**One app adapter.** Nimbus selects exactly one adapter to own your app's
wiring, codegen, and watch loop:

| Nimbus sees | App adapter |
| --- | --- |
| a `convex/` directory | Convex |
| a `firebase` dependency | Firestore |
| a `firebase.json` | Cloud Functions |

**Any number of wire surfaces.** These surfaces compose with any app adapter.
For example, a Convex app that installs `mongodb` keeps its Convex dev loop
and gets the MongoDB listener:

| Runtime dependency | Wire surface |
| --- | --- |
| `mongodb` or `mongoose` | MongoDB — writes `NIMBUS_MONGODB_URL` to `.env.local` |
| `@aws-sdk/client-dynamodb` or `@aws-sdk/lib-dynamodb` | DynamoDB — writes `NIMBUS_DYNAMODB_*` to `.env.local` |
| `@aws-sdk/client-s3` or `@aws-sdk/lib-storage` | [S3](/reference/s3/compatibility/) |

Wire-surface detection reads only the runtime `dependencies` in your
`package.json`. It does not read `devDependencies`, `optionalDependencies`,
or `peerDependencies`. Enabling a surface starts a listener, generates
credentials, and writes `.env.local`. A bare `aws-sdk` (v2) dependency is too
broad to imply DynamoDB or S3. It only produces a banner hint and never enables
a surface.

You can switch off the Firestore, MongoDB, DynamoDB, and S3 surfaces with
`nimbus start` flags. Use `--no-firestore`, `--no-mongodb`, `--no-dynamodb`,
or `--no-s3`. The Convex-compatible surface and the native API are always on.
Under `nimbus dev`, a runtime dependency enables its wire surface. Remove the
dependency to leave the surface off. See
[configuration](/reference/configuration/) for flags, ports, and credentials.

## Which surface should you reach for?

| Surface | Reach for it when | You write against | Migration guide |
| --- | --- | --- | --- |
| [Convex](/developers/convex/) | you want the full function model — queries, mutations, actions, and reactive subscriptions | `convex/` functions and the Convex clients | [Yes](/developers/convex/migrate/) |
| [Firestore](/developers/firebase/) | you have a Firebase app, or want Firestore's document SDKs | the stock `firebase/firestore` SDK | [Yes](/developers/firebase/migrate/) |
| [Cloud Functions](/developers/cloud-functions/) | you are porting Cloud Functions for Firebase workloads | `firebase-functions` v2 handlers | [Yes](/developers/cloud-functions/migrate/) |
| [MongoDB](/developers/mongodb/) | you already have code on the official MongoDB drivers | your existing MongoDB driver | — |
| [DynamoDB](/developers/dynamodb/) | you already have code on the AWS SDK | `@aws-sdk/client-dynamodb` | — |
| [Native API](/developers/native/) | you want any language, with no SDK | plain HTTP and WebSocket | — |

The ported-source surfaces include Convex, Firestore, and Cloud Functions.
Each has a migration guide for moving existing source to Nimbus. MongoDB,
DynamoDB, and the native API need no migration. Point an existing client at a
Nimbus endpoint and keep your code.

Every surface ships runnable examples:
[Convex](/developers/convex/examples/),
[Firestore](/developers/firebase/examples/),
[Cloud Functions](/developers/cloud-functions/examples/),
[MongoDB](/developers/mongodb/examples/),
[DynamoDB](/developers/dynamodb/examples/), and
[Native API](/developers/native/examples/).

## Compare what each one supports

Each surface translates a different part of the engine.
[Adapter capabilities](/reference/adapter-capabilities/) compares all six
surfaces. It covers CRUD, queries, indexes, subscriptions, transactions,
schema validation, auth, tenant binding, and default ports. It also links to
each surface reference. The full compatibility matrices are in the
[Reference](/reference/) section.

## Other wire surfaces

Nimbus also serves protocol surfaces that are not app front doors. This group
includes an [S3 API](/reference/s3/compatibility/), which `nimbus dev` wires
when you declare an AWS S3 client. It also includes a
[Cloudflare-compatible](/reference/cloudflare/compatibility/) Workers KV plane
and a [RESP (Redis)](/reference/kv/compatibility/) listener. Start the RESP
listener with `nimbus kv`. The Reference section documents each surface and
its maturity caveat.
