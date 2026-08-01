---
title: Developers
description: Build apps on Nimbus — functions, schema, scheduling, adapters, and the SDK.
sidebar:
  order: 1
---

Guides for app authors building on Nimbus: tutorials that teach the platform
and how-to guides for getting specific things done. Start with the
[developer quickstart](/get-started/quickstart/) if you have not run Nimbus
yet, then [build your first app](/developers/first-app/).

## Platform guides

- [Build your first app](/developers/first-app/): a complete tutorial with
  schema, functions, and live queries.
- [Authenticate users](/developers/auth/): wire an identity provider into
  your functions.
- [Node.js runtime](/developers/runtimes/nodejs/): use `"use node"` actions,
  packages, and bundling.
- Building for AI agents? Sandboxes, services, and sessions have their
  own [Agents](/agents/) section.

## Adapter guides

Each protocol surface has a front door, and the ported-source surfaces
(Convex, Firestore, Cloud Functions) add a migration guide. Every server
serves these surfaces by default, and `nimbus dev` detects which one your app
uses and wires the app to it automatically. Not sure which to reach for?
[Adapters](/developers/adapters/) explains what a front door is and compares
them side by side.

- [Convex](/developers/convex/): learn the Convex function model, schema, and
  clients. You can also [migrate a Convex app](/developers/convex/migrate/) or
  browse
  [example apps](/developers/convex/examples/).
- [Firestore](/developers/firebase/): use Firestore SDKs against Nimbus.
  You can also [migrate a Firebase app](/developers/firebase/migrate/) or
  browse
  [example apps](/developers/firebase/examples/).
- [Cloud Functions](/developers/cloud-functions/): run Cloud Functions
  workloads. You can also [migrate](/developers/cloud-functions/migrate/) or
  browse an
  [example bundle](/developers/cloud-functions/examples/).
- [MongoDB](/developers/mongodb/): connect official MongoDB drivers. See
  [driver recipes and example apps](/developers/mongodb/examples/).
- [DynamoDB](/developers/dynamodb/): point AWS SDK clients at Nimbus. Browse
  an [example app](/developers/dynamodb/examples/).
- [Native API](/developers/native/): use plain HTTP and WebSocket from any
  language without an SDK. Browse [example apps](/developers/native/examples/).

The full per-surface compatibility matrices live in the
[Reference](/reference/) section.
