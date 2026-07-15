---
title: Adapters
description: Nimbus speaks the protocols your clients already use — one engine behind several front doors. What an adapter is, which surface to pick, and where each guide lives.
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

Every server serves these surfaces by default, and `nimbus dev` detects which
one your app uses and wires the app to it automatically — see the
[developer quickstart](/get-started/quickstart/).

## Pick a surface

Each surface has a front door, and the ported-source surfaces (Convex,
Firestore, Cloud Functions) add a migration guide.

- [Convex](/developers/convex/) — the Convex function model, schema, and
  clients; [migrate a Convex app](/developers/convex/migrate/) or browse
  [example apps](/developers/convex/examples/).
- [Firestore](/developers/firebase/) — Firestore SDKs against Nimbus;
  [migrate a Firebase app](/developers/firebase/migrate/) or browse
  [example apps](/developers/firebase/examples/).
- [Cloud Functions](/developers/cloud-functions/) — Cloud Functions
  workloads; [migrate](/developers/cloud-functions/migrate/) or browse an
  [example bundle](/developers/cloud-functions/examples/).
- [MongoDB](/developers/mongodb/) — connect official MongoDB drivers;
  [driver recipes and example apps](/developers/mongodb/examples/).
- [DynamoDB](/developers/dynamodb/) — point AWS SDK clients at Nimbus;
  browse an [example app](/developers/dynamodb/examples/).
- [Native API](/developers/native/) — plain HTTP and WebSocket from any
  language, no SDK required; browse
  [example apps](/developers/native/examples/).

## Compare what each one supports

[Adapter capabilities](/reference/adapter-capabilities/) puts every surface
side by side — CRUD, queries, indexes, subscriptions, transactions, schema
validation, auth, tenant binding, and default ports — with links into each
per-surface reference. The full per-surface compatibility matrices live in
the [Reference](/reference/) section.

## Other wire surfaces

Nimbus also serves protocol surfaces that are not app front doors: an
[S3 API](/reference/s3/compatibility/), a
[Cloudflare-compatible](/reference/cloudflare/compatibility/) Workers KV
plane, and a [RESP (Redis)](/reference/kv/compatibility/) listener. They are
reference-only surfaces with their own maturity caveats, and they have no
guides in this section.
